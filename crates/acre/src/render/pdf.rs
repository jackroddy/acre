use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use krilla::Document;
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point as PdfPoint, Transform};
use krilla::num::NormalizedF32;
use krilla::page::PageSettings;
use krilla::paint::{Fill, FillRule, LineCap, LineJoin, StrokeDash};
use krilla::surface::Surface;
use krilla::text::{Font as PdfFont, GlyphId, KrillaGlyph};
use tiny_skia as sk;

use super::{oval, path_through, rounded_rect};
use crate::fonts::Fonts;
use crate::scene::{Color, Font, Point, Primitive, Scene, Stroke};
use crate::text::{self, Mark};

/// Points in a canvas pixel, taking a canvas pixel to be a CSS pixel,
/// 1/96 of an inch, so the PDF is the size the SVG declares.
const POINTS_PER_PIXEL: f64 = 0.75;

/// The scene at time `t` as a one-page PDF, `scale` CSS pixels to each
/// canvas pixel. Text is set in the scene's fonts, embedded as subsets,
/// so it can be selected and copied.
///
/// # Errors
///
/// If a font cannot be embedded.
///
/// # Panics
///
/// If the canvas scaled by `scale` has a width or height of zero.
pub fn draw_pdf(scene: &Scene, t: f64, scale: f64) -> Result<Vec<u8>, String> {
    let canvas = &scene.canvas;
    let points = (scale * POINTS_PER_PIXEL) as f32;
    let (width, height) = (canvas.width as f32, canvas.height as f32);
    let mut document = Document::new();
    let settings = PageSettings::from_wh(width * points, height * points)
        .expect("the canvas has a nonzero size");
    let mut page = document.start_page_with(settings);
    let mut surface = page.surface();
    surface.push_transform(&Transform::from_scale(points, points));
    let mut pdf = Pdf {
        fonts: &scene.fonts,
        embedded: Vec::new(),
        unicode: HashMap::new(),
    };
    if let Some(rect) = sk::Rect::from_xywh(0.0, 0.0, width, height) {
        let background = sk::PathBuilder::from_rect(rect);
        draw_path(&mut surface, &background, Some(canvas.background), None);
    }
    for prim in &scene.draw(t) {
        pdf.primitive(&mut surface, prim);
    }
    surface.pop();
    surface.finish();
    page.finish();
    document.finish().map_err(|e| e.to_string())
}

struct Pdf<'a> {
    fonts: &'a Fonts,

    /// Each font drawn from so far, as krilla holds it.
    embedded: Vec<(Font, PdfFont)>,

    /// The character each glyph of a font sets, from its cmap, for the
    /// glyphs of formulas, which carry no text of their own.
    unicode: HashMap<Font, HashMap<u16, char>>,
}

impl Pdf<'_> {
    fn primitive(&mut self, surface: &mut Surface, prim: &Primitive) {
        match prim {
            Primitive::Rect {
                origin,
                size,
                radius,
                fill,
                stroke,
            } => {
                if let Some(path) = rounded_rect(*origin, *size, *radius) {
                    draw_path(surface, &path, *fill, *stroke);
                }
            }
            Primitive::Ellipse {
                center,
                radius,
                fill,
                stroke,
            } => {
                if let Some(path) = oval(*center, *radius) {
                    draw_path(surface, &path, *fill, *stroke);
                }
            }
            Primitive::Line { from, to, stroke } => {
                if let Some(path) = path_through(&[*from, *to], false) {
                    draw_path(surface, &path, None, Some(*stroke));
                }
            }
            Primitive::Polyline { points, stroke } => {
                if let Some(path) = path_through(points, false) {
                    draw_path(surface, &path, None, Some(*stroke));
                }
            }
            Primitive::Polygon {
                points,
                fill,
                stroke,
            } => {
                if let Some(path) = path_through(points, true) {
                    draw_path(surface, &path, *fill, *stroke);
                }
            }
            Primitive::Text {
                origin,
                runs,
                size,
                width,
                h_align,
                v_align,
            } => {
                let layout = text::layout(self.fonts, runs, *size, *width);
                let starts = layout.place(*origin, *h_align, *v_align);
                for (line, start) in layout.lines.iter().zip(starts) {
                    self.line(surface, &line.glyphs, &layout.text, start);
                }
            }
        }
    }

    /// Draw the glyphs of one line whose baseline starts at `start`, in
    /// one text operation for each stretch in one font, size and color.
    fn line(&mut self, surface: &mut Surface, glyphs: &[text::Glyph], text: &str, start: Point) {
        let mut k = 0;
        while k < glyphs.len() {
            let glyph = &glyphs[k];
            let (font, size) = match glyph.mark {
                Mark::Outline { font, size, .. } => (font, size),
                Mark::Rule { width, height } => {
                    let rect = sk::Rect::from_xywh(
                        (start.x + glyph.x) as f32,
                        (start.y + glyph.y - height) as f32,
                        width as f32,
                        height as f32,
                    );
                    if let Some(rect) = rect {
                        let path = sk::PathBuilder::from_rect(rect);
                        draw_path(surface, &path, Some(glyph.color), None);
                    }
                    k += 1;
                    continue;
                }
            };
            let same = |g: &text::Glyph| {
                g.color == glyph.color
                    && matches!(g.mark, Mark::Outline { font: f, size: s, .. } if f == font && s == size)
            };
            let end = k + glyphs[k..].iter().take_while(|g| same(g)).count();
            self.glyphs(surface, &glyphs[k..end], text, start, font, size);
            k = end;
        }
    }

    /// Draw `glyphs`, all in `font` at `size` and in one color, each at
    /// the place the layout gave it.
    fn glyphs(
        &mut self,
        surface: &mut Surface,
        glyphs: &[text::Glyph],
        text: &str,
        start: Point,
        font: Font,
        size: f64,
    ) {
        let pdf_font = self.font(font);
        let mut chars = String::new();
        let mut placed: Vec<KrillaGlyph> = Vec::with_capacity(glyphs.len());
        let mut previous: Option<(&Range<usize>, Range<usize>)> = None;
        for (k, g) in glyphs.iter().enumerate() {
            let Mark::Outline { id, .. } = g.mark else {
                unreachable!("a stretch holds only outlines")
            };

            // glyphs of one cluster share its text, so a ligature or
            // a mark copies out once
            let range = match previous {
                Some((source, ref range)) if *source == g.text && !g.text.is_empty() => {
                    range.clone()
                }
                _ => {
                    let from = chars.len();
                    if g.text.is_empty() {
                        if let Some(c) = self.char_of(font, id) {
                            chars.push(c);
                        }
                    } else {
                        chars.push_str(&text[g.text.clone()]);
                    }
                    from..chars.len()
                }
            };
            previous = Some((&g.text, range.clone()));

            // the advance reaches the next glyph's place, so
            // every glyph lands where the layout put it
            let advance = match glyphs.get(k + 1) {
                Some(next) => next.x - g.x,
                None => g.advance,
            };
            placed.push(KrillaGlyph::new(
                GlyphId::new(id as u32),
                (advance / size) as f32,
                0.0,
                (-g.y / size) as f32,
                0.0,
                range,
                None,
            ));
        }
        surface.set_fill(Some(fill(glyphs[0].color)));
        surface.set_stroke(None);
        let origin = PdfPoint::from_xy((start.x + glyphs[0].x) as f32, start.y as f32);
        surface.draw_glyphs(origin, &placed, pdf_font, &chars, size as f32, false);
    }

    fn font(&mut self, font: Font) -> PdfFont {
        if let Some((_, f)) = self.embedded.iter().find(|(f, _)| *f == font) {
            return f.clone();
        }
        let bytes: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(self.fonts.bytes(font).clone());
        let pdf_font = PdfFont::new(bytes.into(), 0).expect("a font acre parsed parses again");
        self.embedded.push((font, pdf_font.clone()));
        pdf_font
    }

    /// The character glyph `id` of `font` sets, when its cmap maps one
    /// to it.
    fn char_of(&mut self, font: Font, id: u16) -> Option<char> {
        let fonts = self.fonts;
        let map = self.unicode.entry(font).or_insert_with(|| {
            let mut map = HashMap::new();
            let Some(cmap) = fonts.face(font).tables().cmap else {
                return map;
            };
            for subtable in cmap.subtables.into_iter().filter(|s| s.is_unicode()) {
                subtable.codepoints(|c| {
                    if let (Some(g), Some(c)) = (subtable.glyph_index(c), char::from_u32(c)) {
                        map.entry(g.0).or_insert(c);
                    }
                });
            }
            map
        });
        map.get(&id).copied()
    }
}

fn draw_path(surface: &mut Surface, path: &sk::Path, fill: Option<Color>, stroke: Option<Stroke>) {
    let mut builder = PathBuilder::new();
    for segment in path.segments() {
        match segment {
            sk::PathSegment::MoveTo(p) => builder.move_to(p.x, p.y),
            sk::PathSegment::LineTo(p) => builder.line_to(p.x, p.y),
            sk::PathSegment::QuadTo(p1, p) => builder.quad_to(p1.x, p1.y, p.x, p.y),
            sk::PathSegment::CubicTo(p1, p2, p) => {
                builder.cubic_to(p1.x, p1.y, p2.x, p2.y, p.x, p.y)
            }
            sk::PathSegment::Close => builder.close(),
        }
    }
    let Some(path) = builder.finish() else {
        return;
    };
    surface.set_fill(fill.map(self::fill));
    surface.set_stroke(stroke.map(|s| krilla::paint::Stroke {
        paint: rgb_of(s.color).into(),
        width: s.width as f32,
        line_cap: LineCap::Butt,
        line_join: LineJoin::Round,
        opacity: opacity(s.color),
        dash: s.dash.map(|[on, off]| StrokeDash {
            array: vec![on as f32, off as f32],
            offset: 0.0,
        }),
        ..krilla::paint::Stroke::default()
    }));
    surface.draw_path(&path);
}

fn fill(color: Color) -> Fill {
    Fill {
        paint: rgb_of(color).into(),
        opacity: opacity(color),
        rule: FillRule::NonZero,
    }
}

fn rgb_of(c: Color) -> rgb::Color {
    let [r, g, b] = [c.r, c.g, c.b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    rgb::Color::new(r, g, b)
}

fn opacity(c: Color) -> NormalizedF32 {
    NormalizedF32::new(c.a.clamp(0.0, 1.0)).unwrap_or(NormalizedF32::ONE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lang, scene};

    fn pdf(src: &str) -> lopdf::Document {
        let scene = scene::build(&lang::parse(src).unwrap(), &[]).unwrap();
        let bytes = draw_pdf(&scene, 0.0, 1.0).unwrap();
        lopdf::Document::load_mem(&bytes).unwrap()
    }

    fn name(object: &lopdf::Object) -> String {
        String::from_utf8_lossy(object.as_name().unwrap()).into_owned()
    }

    #[test]
    fn a_page_is_the_canvas_in_points() {
        let doc = pdf("canvas { size 400 200; box { at 10 10; size 50 50; } }");
        let pages = doc.get_pages();
        assert_eq!(pages.len(), 1);
        let page = doc.get_dictionary(pages[&1]).unwrap();
        let media_box: Vec<f32> = page
            .get(b"MediaBox")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_float().unwrap())
            .collect();
        assert_eq!(media_box, [0.0, 0.0, 300.0, 150.0]);
    }

    #[test]
    fn text_embeds_its_fonts_as_subsets() {
        let doc = pdf(r#"canvas {
            size 300 100;
            text { at 10 10; text "Sans and `mono` and $x^2$"; font sans; }
        }"#);
        let mut fonts: Vec<String> = doc
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .filter(|d| d.get(b"Type").is_ok_and(|t| name(t) == "FontDescriptor"))
            .map(|d| {
                let embedded = d.has(b"FontFile2") || d.has(b"FontFile3");
                assert!(embedded, "{d:?} carries no font file");
                name(d.get(b"FontName").unwrap())
            })
            .collect();
        fonts.sort();
        assert_eq!(fonts.len(), 3, "{fonts:?}");
        // a subset's name is six capitals and a plus before the font's
        assert!(fonts.iter().all(|f| f.as_bytes()[6] == b'+'), "{fonts:?}");
        assert!(fonts.iter().any(|f| f.ends_with("+NotoSans-Regular")));
        assert!(
            fonts
                .iter()
                .any(|f| f.ends_with("+JetBrainsMonoNL-Regular"))
        );
        assert!(fonts.iter().any(|f| f.ends_with("+NotoSansMath-Regular")));
    }

    #[test]
    fn text_can_be_copied_out() {
        let doc = pdf(r#"canvas { size 300 100; text { at 10 10; text "Hello"; font sans; } }"#);
        assert_eq!(doc.extract_text(&[1]).unwrap().trim(), "Hello");
    }
}

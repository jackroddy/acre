use std::collections::HashMap;
use std::fmt::{self, Display, Write};

use ttf_parser::{GlyphId, OutlineBuilder};

use crate::fonts::Fonts;
use crate::scene::{Color, Font, HAlign, Point, Primitive, Scene, Stroke, VAlign};
use crate::text::{self, Mark};

/// The scene at time `t` as an SVG document, `scale` CSS pixels to each
/// canvas pixel. Text is set as glyph outlines, so it draws the same in
/// every viewer and needs no font.
pub fn draw_svg(scene: &Scene, t: f64, scale: f64) -> String {
    let canvas = &scene.canvas;
    let (width, height) = (canvas.width as f64, canvas.height as f64);
    let mut svg = Svg {
        fonts: &scene.fonts,
        body: String::new(),
        defs: String::new(),
        glyphs: HashMap::new(),
    };
    svg.rect(
        Point::ZERO,
        Point::new(width, height),
        0.0,
        Some(canvas.background),
        None,
    );
    for prim in &scene.draw(t) {
        svg.primitive(prim);
    }

    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" \
         xmlns:xlink=\"http://www.w3.org/1999/xlink\" \
         width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
        Num(width * scale),
        Num(height * scale),
        Num(width),
        Num(height),
    );
    if !svg.defs.is_empty() {
        out.push_str("<defs>\n");
        out.push_str(&svg.defs);
        out.push_str("</defs>\n");
    }
    out.push_str(&svg.body);
    out.push_str("</svg>\n");
    out
}

struct Svg<'a> {
    fonts: &'a Fonts,
    body: String,

    /// One `<path>` per glyph drawn at each size, in canvas pixels
    /// from the glyph's origin.
    defs: String,

    /// The id of the path in `defs` for each glyph, font and size, the
    /// size as its bits, or `None` for a glyph with no outline, such as
    /// a space.
    glyphs: HashMap<(Font, u16, u64), Option<String>>,
}

impl Svg<'_> {
    fn primitive(&mut self, prim: &Primitive) {
        match prim {
            Primitive::Rect {
                origin,
                size,
                radius,
                fill,
                stroke,
            } => self.rect(*origin, *size, *radius, *fill, *stroke),
            Primitive::Ellipse {
                center,
                radius,
                fill,
                stroke,
            } => {
                if radius.x < 0.0 || radius.y < 0.0 {
                    return;
                }
                let _ = write!(
                    self.body,
                    "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"",
                    Num(center.x),
                    Num(center.y),
                    Num(radius.x),
                    Num(radius.y),
                );
                self.paint(*fill, *stroke);
            }
            Primitive::Line { from, to, stroke } => {
                let _ = write!(
                    self.body,
                    "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
                    Num(from.x),
                    Num(from.y),
                    Num(to.x),
                    Num(to.y),
                );
                self.paint(None, Some(*stroke));
            }
            Primitive::Polyline { points, stroke } => {
                self.points("polyline", points);
                self.paint(None, Some(*stroke));
            }
            Primitive::Polygon {
                points,
                fill,
                stroke,
            } => {
                self.points("polygon", points);
                self.paint(*fill, *stroke);
            }
            Primitive::Text {
                origin,
                runs,
                size,
                width,
                justify,
                h_align,
                v_align,
            } => {
                let layout = text::layout(self.fonts, runs, *size, *width, *justify);
                self.text(&layout, *origin, *h_align, *v_align);
            }
        }
    }

    fn rect(
        &mut self,
        origin: Point,
        size: Point,
        radius: f64,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    ) {
        if size.x < 0.0 || size.y < 0.0 {
            return;
        }
        let _ = write!(
            self.body,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            Num(origin.x),
            Num(origin.y),
            Num(size.x),
            Num(size.y),
        );
        let r = radius.min(size.x / 2.0).min(size.y / 2.0);
        if r > 0.0 {
            let _ = write!(self.body, " rx=\"{}\"", Num(r));
        }
        self.paint(fill, stroke);
    }

    fn points(&mut self, element: &str, points: &[Point]) {
        let _ = write!(self.body, "<{element} points=\"");
        for (k, p) in points.iter().enumerate() {
            let sep = if k > 0 { " " } else { "" };
            let _ = write!(self.body, "{sep}{},{}", Num(p.x), Num(p.y));
        }
        self.body.push('"');
    }

    /// Finish the open element with its fill and stroke.
    fn paint(&mut self, fill: Option<Color>, stroke: Option<Stroke>) {
        let out = &mut self.body;
        match fill {
            Some(c) => {
                let _ = write!(out, " fill=\"{}\"", Hex(c));
                if c.a < 1.0 {
                    let _ = write!(out, " fill-opacity=\"{}\"", Num(c.a.max(0.0) as f64));
                }
            }
            None => out.push_str(" fill=\"none\""),
        }
        if let Some(s) = stroke {
            let _ = write!(
                out,
                " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"",
                Hex(s.color),
                Num(s.width),
            );
            if s.color.a < 1.0 {
                let _ = write!(
                    out,
                    " stroke-opacity=\"{}\"",
                    Num(s.color.a.max(0.0) as f64)
                );
            }
            if let Some([on, off]) = s.dash {
                let _ = write!(out, " stroke-dasharray=\"{} {}\"", Num(on), Num(off));
            }
        }
        out.push_str("/>\n");
    }

    fn text(&mut self, layout: &text::Layout, origin: Point, h_align: HAlign, v_align: VAlign) {
        let starts = layout.place(origin, h_align, v_align);
        let mut color = None;
        for (line, start) in layout.lines.iter().zip(starts) {
            for glyph in &line.glyphs {
                let (x, y) = (start.x + glyph.x, start.y + glyph.y);
                let mark = match glyph.mark {
                    Mark::Outline { font, id, size } => match self.glyph(font, id, size) {
                        Some(def) => format!(
                            "<use xlink:href=\"#{def}\" x=\"{}\" y=\"{}\"/>\n",
                            Num(x),
                            Num(y),
                        ),
                        None => continue,
                    },
                    Mark::Rule { width, height } => {
                        if width <= 0.0 || height <= 0.0 {
                            continue;
                        }
                        format!(
                            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
                            Num(x),
                            Num(y - height),
                            Num(width),
                            Num(height),
                        )
                    }
                };
                if color != Some(glyph.color) {
                    if color.is_some() {
                        self.body.push_str("</g>\n");
                    }
                    let c = glyph.color;
                    let _ = write!(self.body, "<g fill=\"{}\"", Hex(c));
                    if c.a < 1.0 {
                        let _ = write!(self.body, " fill-opacity=\"{}\"", Num(c.a.max(0.0) as f64));
                    }
                    self.body.push_str(">\n");
                    color = Some(c);
                }
                self.body.push_str(&mark);
            }
        }
        if color.is_some() {
            self.body.push_str("</g>\n");
        }
    }

    /// The id of the path in `defs` that draws glyph `id` of `font` at
    /// `size`, written there the first time it is asked for.
    fn glyph(&mut self, font: Font, id: u16, size: f64) -> Option<String> {
        let key = (font, id, size.to_bits());
        if let Some(def) = self.glyphs.get(&key) {
            return def.clone();
        }
        let face = self.fonts.face(font);
        let mut outline = Outline {
            d: String::new(),
            scale: size / face.units_per_em() as f64,
        };
        let def = face.outline_glyph(GlyphId(id), &mut outline).map(|_| {
            let name = match font {
                Font::Mono => "mono".to_string(),
                Font::Sans => "sans".to_string(),
                Font::SansBold => "sans-bold".to_string(),
                Font::SansItalic => "sans-italic".to_string(),
                Font::Math => "math".to_string(),
                Font::Custom(k) => format!("font{k}"),
            };
            let def = format!("{name}-{id}-{}", Num(size));
            let _ = writeln!(self.defs, "<path id=\"{def}\" d=\"{}\"/>", outline.d);
            def
        });
        self.glyphs.insert(key, def.clone());
        def
    }
}

/// A glyph outline written as path data, `scale` canvas pixels to
/// each font unit, with y flipped to point down as the canvas does.
struct Outline {
    d: String,
    scale: f64,
}

impl Outline {
    fn point(&mut self, x: f32, y: f32) {
        let (x, y) = (x as f64 * self.scale, -y as f64 * self.scale);
        let _ = write!(self.d, "{} {}", Num(x), Num(y));
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.d.push('M');
        self.point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.d.push('L');
        self.point(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.d.push('Q');
        self.point(x1, y1);
        self.d.push(' ');
        self.point(x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.d.push('C');
        self.point(x1, y1);
        self.d.push(' ');
        self.point(x2, y2);
        self.d.push(' ');
        self.point(x, y);
    }

    fn close(&mut self) {
        self.d.push('Z');
    }
}

/// A number written with at most three decimals and no trailing zeros.
struct Num(f64);

impl Display for Num {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = format!("{:.3}", self.0);
        let s = if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.')
        } else {
            &s
        };
        f.write_str(if s == "-0" { "0" } else { s })
    }
}

/// A color as `#rrggbb`, without its alpha.
struct Hex(Color);

impl Display for Hex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b] =
            [self.0.r, self.0.g, self.0.b].map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        write!(f, "#{r:02x}{g:02x}{b:02x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lang, scene};

    fn svg(src: &str) -> String {
        let scene = scene::build(&lang::parse(src).unwrap(), &[]).unwrap();
        draw_svg(&scene, 0.0, 2.0)
    }

    #[test]
    fn a_drawing_is_well_formed_svg_of_the_canvas_size() {
        let out = svg(r#"canvas {
            size 300 200;
            box { at 10 10; size 100 40; radius 6; dash 4 2; text "Box"; }
            ellipse { at 150 10; size 60 30; }
            arrow { from 10 100; to 200 100; }
            text { at 10 150; text "x $\frac{a}{b}$"; font sans; }
        }"#);
        let doc = roxmltree::Document::parse(&out).unwrap();
        let root = doc.root_element();
        assert_eq!(root.tag_name().name(), "svg");
        assert_eq!(root.attribute("width"), Some("600"));
        assert_eq!(root.attribute("height"), Some("400"));
        assert_eq!(root.attribute("viewBox"), Some("0 0 300 200"));
        let count = |name: &str| {
            doc.descendants()
                .filter(|n| n.tag_name().name() == name)
                .count()
        };
        assert_eq!(count("ellipse"), 1);
        assert_eq!(count("line"), 1);
        assert!(count("polygon") >= 1, "the arrowhead");
        let rounded = doc
            .descendants()
            .find(|n| n.attribute("rx") == Some("6"))
            .expect("the box has rounded corners");
        assert_eq!(rounded.attribute("stroke-dasharray"), Some("4 2"));
    }

    #[test]
    fn text_is_glyph_outlines_each_defined_once() {
        let out = svg(r#"canvas { size 200 50; text { at 0 0; text "aaa b"; font sans; } }"#);
        let doc = roxmltree::Document::parse(&out).unwrap();
        let uses: Vec<_> = doc
            .descendants()
            .filter(|n| n.tag_name().name() == "use")
            .collect();
        let paths = doc
            .descendants()
            .filter(|n| n.tag_name().name() == "path")
            .count();
        // the space has no outline, so it has neither
        assert_eq!(uses.len(), 4);
        assert_eq!(paths, 2);
        assert!(doc.descendants().all(|n| n.tag_name().name() != "text"));
    }

    #[test]
    fn a_number_has_at_most_three_decimals_and_no_trailing_zeros() {
        let s = |v| Num(v).to_string();
        assert_eq!(s(2.0), "2");
        assert_eq!(s(0.12345), "0.123");
        assert_eq!(s(1.5), "1.5");
        assert_eq!(s(-0.0001), "0");
    }
}

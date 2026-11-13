//! A scene drawn into pixels.
//!
//! [`draw_frame`] samples a [`Scene`] at one instant and paints its
//! primitives onto a pixmap. Nothing here writes a file.

use tiny_skia as sk;
use ttf_parser::{GlyphId, OutlineBuilder};

use crate::fonts::Fonts;
use crate::scene::{Color, HAlign, Point, Primitive, Run, Scene, Stroke, VAlign};
use crate::text::{self, Mark};

/// The scene at time `t`, drawn onto a pixmap of `scale` pixels to each
/// canvas pixel.
///
/// # Panics
///
/// If the canvas scaled by `scale` has a width or height of zero.
pub fn draw_frame(scene: &Scene, t: f64, scale: f64) -> sk::Pixmap {
    let canvas = &scene.canvas;
    let size = |n: u32| (n as f64 * scale).round() as u32;
    let mut pixmap = sk::Pixmap::new(size(canvas.width), size(canvas.height))
        .expect("the canvas has a nonzero size");
    pixmap.fill(sk_color(canvas.background));
    let transform = sk::Transform::from_scale(scale as f32, scale as f32);
    draw_primitives(&mut pixmap, &scene.fonts, &scene.draw(t), transform, None);
    pixmap
}

/// The part of the scene at time `t` that falls in a `width` by
/// `height` pixmap, with the canvas drawn at `scale` pixels to each
/// canvas pixel and its top-left corner at `origin`. Pixels off the
/// canvas stay transparent.
///
/// # Panics
///
/// If `width` or `height` is zero.
pub fn draw_view(
    scene: &Scene,
    t: f64,
    width: u32,
    height: u32,
    scale: f64,
    origin: Point,
) -> sk::Pixmap {
    let mut pixmap = sk::Pixmap::new(width, height).expect("the view has a nonzero size");
    let transform = sk::Transform::from_scale(scale as f32, scale as f32)
        .post_translate(origin.x as f32, origin.y as f32);
    let canvas = &scene.canvas;
    let Some(rect) = sk::Rect::from_xywh(0.0, 0.0, canvas.width as f32, canvas.height as f32)
    else {
        return pixmap;
    };
    let path = sk::PathBuilder::from_rect(rect);
    let mut mask = sk::Mask::new(width, height).expect("the view has a nonzero size");
    mask.fill_path(&path, sk::FillRule::Winding, true, transform);
    pixmap.fill_path(
        &path,
        &paint(canvas.background),
        sk::FillRule::Winding,
        transform,
        None,
    );
    draw_primitives(
        &mut pixmap,
        &scene.fonts,
        &scene.draw(t),
        transform,
        Some(&mask),
    );
    pixmap
}

fn draw_primitives(
    pixmap: &mut sk::Pixmap,
    fonts: &Fonts,
    prims: &[Primitive],
    transform: sk::Transform,
    mask: Option<&sk::Mask>,
) {
    let paint_path = |pixmap: &mut sk::Pixmap, path: &sk::Path, fill, stroke| {
        paint_path(pixmap, path, fill, stroke, transform, mask)
    };
    for prim in prims {
        match prim {
            Primitive::Rect {
                origin,
                size,
                radius,
                fill,
                stroke,
            } => {
                if let Some(path) = rounded_rect(*origin, *size, *radius) {
                    paint_path(pixmap, &path, *fill, *stroke);
                }
            }
            Primitive::Ellipse {
                center,
                radius,
                fill,
                stroke,
            } => {
                let rect = sk::Rect::from_ltrb(
                    (center.x - radius.x) as f32,
                    (center.y - radius.y) as f32,
                    (center.x + radius.x) as f32,
                    (center.y + radius.y) as f32,
                );
                if let Some(path) = rect.and_then(sk::PathBuilder::from_oval) {
                    paint_path(pixmap, &path, *fill, *stroke);
                }
            }
            Primitive::Line { from, to, stroke } => {
                if let Some(path) = path_through(&[*from, *to], false) {
                    paint_path(pixmap, &path, None, Some(*stroke));
                }
            }
            Primitive::Polyline { points, stroke } => {
                if let Some(path) = path_through(points, false) {
                    paint_path(pixmap, &path, None, Some(*stroke));
                }
            }
            Primitive::Polygon {
                points,
                fill,
                stroke,
            } => {
                if let Some(path) = path_through(points, true) {
                    paint_path(pixmap, &path, *fill, *stroke);
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
                for (path, color) in
                    text_paths(fonts, runs, *origin, *size, *width, *h_align, *v_align)
                {
                    paint_path(pixmap, &path, Some(color), None);
                }
            }
        }
    }
}

fn paint_path(
    pixmap: &mut sk::Pixmap,
    path: &sk::Path,
    fill: Option<Color>,
    stroke: Option<Stroke>,
    transform: sk::Transform,
    mask: Option<&sk::Mask>,
) {
    if let Some(color) = fill {
        let paint = paint(color);
        pixmap.fill_path(path, &paint, sk::FillRule::Winding, transform, mask);
    }
    if let Some(stroke) = stroke {
        let style = sk::Stroke {
            width: stroke.width as f32,
            line_join: sk::LineJoin::Round,
            dash: stroke
                .dash
                .and_then(|[on, off]| sk::StrokeDash::new(vec![on as f32, off as f32], 0.0)),
            ..sk::Stroke::default()
        };
        pixmap.stroke_path(path, &paint(stroke.color), &style, transform, mask);
    }
}

/// The outline of the rectangle at `origin` of `size`, with corners
/// rounded to `radius`.
fn rounded_rect(origin: Point, size: Point, radius: f64) -> Option<sk::Path> {
    let rect = sk::Rect::from_xywh(
        origin.x as f32,
        origin.y as f32,
        size.x as f32,
        size.y as f32,
    )?;
    let r = (radius as f32)
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    if r <= 0.0 {
        return Some(sk::PathBuilder::from_rect(rect));
    }

    // a cubic's control points at this fraction of the radius from
    // each end give a quarter circle to within 0.03%
    let k = r * (1.0 - 0.552_284_8);
    let (l, t, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let mut p = sk::PathBuilder::new();
    p.move_to(l + r, t);
    p.line_to(rt - r, t);
    p.cubic_to(rt - k, t, rt, t + k, rt, t + r);
    p.line_to(rt, b - r);
    p.cubic_to(rt, b - k, rt - k, b, rt - r, b);
    p.line_to(l + r, b);
    p.cubic_to(l + k, b, l, b - k, l, b - r);
    p.line_to(l, t + r);
    p.cubic_to(l, t + k, l + k, t, l + r, t);
    p.close();
    p.finish()
}

fn paint(color: Color) -> sk::Paint<'static> {
    let mut paint = sk::Paint::default();
    paint.set_color(sk_color(color));
    paint
}

fn sk_color(c: Color) -> sk::Color {
    let [r, g, b, a] = [c.r, c.g, c.b, c.a].map(|v| v.clamp(0.0, 1.0));
    sk::Color::from_rgba(r, g, b, a).unwrap_or(sk::Color::TRANSPARENT)
}

fn path_through(points: &[Point], closed: bool) -> Option<sk::Path> {
    let (first, rest) = points.split_first()?;
    let mut builder = sk::PathBuilder::new();
    builder.move_to(first.x as f32, first.y as f32);
    for p in rest {
        builder.line_to(p.x as f32, p.y as f32);
    }
    if closed {
        builder.close();
    }
    builder.finish()
}

/// The outline of `runs` laid out by [`text::layout`] and placed so
/// `origin` sits where the alignments say, as one path per stretch of
/// glyphs in one color.
fn text_paths(
    fonts: &Fonts,
    runs: &[Run],
    origin: Point,
    size: f64,
    width: Option<f64>,
    h_align: HAlign,
    v_align: VAlign,
) -> Vec<(sk::Path, Color)> {
    let layout = text::layout(fonts, runs, size, width);
    let starts = layout.place(origin, h_align, v_align);
    let mut out = Vec::new();
    let mut builder = sk::PathBuilder::new();
    let mut color = None;
    for (line, start) in layout.lines.iter().zip(starts) {
        for glyph in &line.glyphs {
            if color.is_some_and(|c| c != glyph.color)
                && let Some(path) = std::mem::take(&mut builder).finish()
            {
                out.push((path, color.unwrap()));
            }
            color = Some(glyph.color);
            let (x, y) = (start.x + glyph.x, start.y + glyph.y);
            match glyph.mark {
                Mark::Outline { font, id, size } => {
                    let face = fonts.face(font);
                    let mut outline = Outline {
                        builder,
                        x: x as f32,
                        y: y as f32,
                        scale: (size / face.units_per_em() as f64) as f32,
                    };
                    face.outline_glyph(GlyphId(id), &mut outline);
                    builder = outline.builder;
                }
                Mark::Rule { width, height } => {
                    let rect = sk::Rect::from_xywh(
                        x as f32,
                        (y - height) as f32,
                        width as f32,
                        height as f32,
                    );
                    if let Some(rect) = rect {
                        builder.push_rect(rect);
                    }
                }
            }
        }
    }
    if let (Some(path), Some(color)) = (builder.finish(), color) {
        out.push((path, color));
    }
    out
}

/// A glyph outline being copied into a path, in canvas pixels.
struct Outline {
    builder: sk::PathBuilder,
    x: f32,
    y: f32,
    scale: f32,
}

impl Outline {
    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        // font units point up and canvas pixels point down
        (self.x + x * self.scale, self.y - y * self.scale)
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (x1, y1) = self.at(x1, y1);
        let (x, y) = self.at(x, y);
        self.builder.quad_to(x1, y1, x, y);
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (x1, y1) = self.at(x1, y1);
        let (x2, y2) = self.at(x2, y2);
        let (x, y) = self.at(x, y);
        self.builder.cubic_to(x1, y1, x2, y2, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Font;

    const RED: Color = Color::rgb(1.0, 0.0, 0.0);

    fn blank(width: u32, height: u32) -> sk::Pixmap {
        let mut pixmap = sk::Pixmap::new(width, height).unwrap();
        pixmap.fill(sk::Color::WHITE);
        pixmap
    }

    fn is_white(pixmap: &sk::Pixmap, x: u32, y: u32) -> bool {
        let p = pixmap.pixel(x, y).unwrap();
        (p.red(), p.green(), p.blue()) == (255, 255, 255)
    }

    /// The bounds of every non-white pixel, as `[left, top, right, bottom]`
    /// with right and bottom exclusive.
    fn ink(pixmap: &sk::Pixmap) -> [u32; 4] {
        let mut bounds = [u32::MAX, u32::MAX, 0, 0];
        for y in 0..pixmap.height() {
            for x in 0..pixmap.width() {
                if !is_white(pixmap, x, y) {
                    bounds = [
                        bounds[0].min(x),
                        bounds[1].min(y),
                        bounds[2].max(x + 1),
                        bounds[3].max(y + 1),
                    ];
                }
            }
        }
        bounds
    }

    #[test]
    fn a_filled_rect_covers_exactly_its_pixels() {
        let mut pixmap = blank(20, 20);
        let rect = Primitive::Rect {
            origin: Point::new(5.0, 6.0),
            size: Point::new(4.0, 3.0),
            radius: 0.0,
            fill: Some(RED),
            stroke: None,
        };
        draw_primitives(
            &mut pixmap,
            &Fonts::default(),
            &[rect],
            sk::Transform::identity(),
            None,
        );
        assert_eq!(ink(&pixmap), [5, 6, 9, 9]);
        let p = pixmap.pixel(6, 7).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (255, 0, 0));
    }

    #[test]
    fn a_polyline_is_stroked_and_not_filled() {
        let mut pixmap = blank(30, 30);
        let line = Primitive::Polyline {
            points: vec![
                Point::new(5.0, 5.0),
                Point::new(25.0, 5.0),
                Point::new(25.0, 25.0),
            ],
            stroke: Stroke::solid(RED, 2.0),
        };
        draw_primitives(
            &mut pixmap,
            &Fonts::default(),
            &[line],
            sk::Transform::identity(),
            None,
        );
        assert!(!is_white(&pixmap, 15, 5));
        assert!(!is_white(&pixmap, 25, 15));
        assert!(is_white(&pixmap, 15, 15));
    }

    #[test]
    fn centered_text_is_centered_on_its_origin() {
        let mut pixmap = blank(80, 80);
        let text = Primitive::Text {
            origin: Point::new(40.0, 40.0),
            runs: vec![Run {
                text: "8".into(),
                font: Font::Mono,
                color: Color::BLACK,
            }],
            size: 40.0,
            width: None,
            h_align: HAlign::Center,
            v_align: VAlign::Middle,
        };
        draw_primitives(
            &mut pixmap,
            &Fonts::default(),
            &[text],
            sk::Transform::identity(),
            None,
        );
        let [left, top, right, bottom] = ink(&pixmap);
        assert!(right > left && bottom > top, "nothing was drawn");
        let mid_x = (left + right) as f64 / 2.0;
        let mid_y = (top + bottom) as f64 / 2.0;
        assert!((mid_x - 40.0).abs() <= 1.0, "ink spans x {left}..{right}");
        assert!((mid_y - 40.0).abs() <= 1.0, "ink spans y {top}..{bottom}");
    }

    #[test]
    fn a_character_missing_from_the_font_still_takes_its_advance() {
        let mut pixmap = blank(200, 60);
        let text = |s: &str| Primitive::Text {
            origin: Point::new(10.0, 40.0),
            runs: vec![Run {
                text: s.into(),
                font: Font::Mono,
                color: Color::BLACK,
            }],
            size: 30.0,
            width: None,
            h_align: HAlign::Left,
            v_align: VAlign::Baseline,
        };
        draw_primitives(
            &mut pixmap,
            &Fonts::default(),
            &[text("a\u{10fffd}b")],
            sk::Transform::identity(),
            None,
        );
        let with_gap = ink(&pixmap);
        let mut pixmap = blank(200, 60);
        draw_primitives(
            &mut pixmap,
            &Fonts::default(),
            &[text("a b")],
            sk::Transform::identity(),
            None,
        );
        assert_eq!(with_gap[2], ink(&pixmap)[2]);
    }

    #[test]
    fn a_view_draws_the_canvas_scaled_and_moved_and_nothing_off_it() {
        let scene = Scene {
            canvas: crate::scene::Canvas {
                width: 10,
                height: 10,
                fps: 1,
                duration: 0.0,
                background: RED,
            },
            objects: Vec::new(),
            fonts: Fonts::default(),
        };
        let pixmap = draw_view(&scene, 0.0, 40, 40, 2.0, Point::new(5.0, 5.0));
        let red = |x, y| pixmap.pixel(x, y).unwrap().red() == 255;
        assert!(red(5, 5) && red(24, 24));
        assert_eq!(pixmap.pixel(4, 4).unwrap().alpha(), 0);
        assert_eq!(pixmap.pixel(25, 25).unwrap().alpha(), 0);
    }

    #[test]
    fn a_measured_width_matches_the_ink_drawn() {
        let runs = vec![Run {
            text: "Measure AV.".into(),
            font: Font::Sans,
            color: Color::BLACK,
        }];
        let width = text::layout(&Fonts::default(), &runs, 30.0, None).width();
        let mut pixmap = blank(300, 60);
        let label = Primitive::Text {
            origin: Point::new(10.0, 40.0),
            runs,
            size: 30.0,
            width: None,
            h_align: HAlign::Left,
            v_align: VAlign::Baseline,
        };
        draw_primitives(
            &mut pixmap,
            &Fonts::default(),
            &[label],
            sk::Transform::identity(),
            None,
        );
        let [left, _, right, _] = ink(&pixmap);
        // the ink starts at the first glyph's left bearing and ends at
        // the last one's right edge, so it falls inside the advances
        assert!((10..=13).contains(&left), "ink starts at {left}");
        assert!(
            (right as f64 - (10.0 + width)).abs() <= 3.0,
            "ink ends at {right}, advances at {}",
            10.0 + width
        );
    }

    #[test]
    fn a_frame_is_the_canvas_size_and_background() {
        let scene = Scene {
            canvas: crate::scene::Canvas {
                width: 12,
                height: 7,
                fps: 1,
                duration: 0.0,
                background: RED,
            },
            objects: Vec::new(),
            fonts: Fonts::default(),
        };
        let pixmap = draw_frame(&scene, 0.0, 1.0);
        assert_eq!((pixmap.width(), pixmap.height()), (12, 7));
        let p = pixmap.pixel(11, 6).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (255, 0, 0));
    }
}

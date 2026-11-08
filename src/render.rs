//! A scene drawn into pixels.
//!
//! [`draw_frame`] samples a [`Scene`] at one instant and paints its
//! primitives onto a pixmap. Nothing here writes a file.

use tiny_skia as sk;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

use crate::scene::{Color, HAlign, Point, Primitive, Scene, Stroke, VAlign};

static FONT: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf");

/// The scene at time `t`, drawn onto a pixmap the size of its canvas.
///
/// # Panics
///
/// If the canvas has a width or height of zero.
pub fn draw_frame(scene: &Scene, t: f64) -> sk::Pixmap {
    let canvas = &scene.canvas;
    let mut pixmap =
        sk::Pixmap::new(canvas.width, canvas.height).expect("the canvas has a nonzero size");
    pixmap.fill(sk_color(canvas.background));
    draw_primitives(&mut pixmap, &scene.draw(t));
    pixmap
}

fn draw_primitives(pixmap: &mut sk::Pixmap, prims: &[Primitive]) {
    let font = Face::parse(FONT, 0).expect("the embedded font parses");
    for prim in prims {
        match prim {
            Primitive::Rect {
                origin,
                size,
                fill,
                stroke,
            } => {
                let rect = sk::Rect::from_xywh(
                    origin.x as f32,
                    origin.y as f32,
                    size.x as f32,
                    size.y as f32,
                );
                if let Some(rect) = rect {
                    paint_path(pixmap, &sk::PathBuilder::from_rect(rect), *fill, *stroke);
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
                if let Some(path) = polyline(&[*from, *to]) {
                    paint_path(pixmap, &path, None, Some(*stroke));
                }
            }
            Primitive::Polyline { points, stroke } => {
                if let Some(path) = polyline(points) {
                    paint_path(pixmap, &path, None, Some(*stroke));
                }
            }
            Primitive::Text {
                origin,
                text,
                size,
                color,
                h_align,
                v_align,
            } => {
                if let Some(path) = text_path(&font, text, *origin, *size, *h_align, *v_align) {
                    paint_path(pixmap, &path, Some(*color), None);
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
) {
    let identity = sk::Transform::identity();
    if let Some(color) = fill {
        let paint = paint(color);
        pixmap.fill_path(path, &paint, sk::FillRule::Winding, identity, None);
    }
    if let Some(stroke) = stroke {
        let style = sk::Stroke {
            width: stroke.width as f32,
            line_join: sk::LineJoin::Round,
            ..sk::Stroke::default()
        };
        pixmap.stroke_path(path, &paint(stroke.color), &style, identity, None);
    }
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

fn polyline(points: &[Point]) -> Option<sk::Path> {
    let (first, rest) = points.split_first()?;
    let mut builder = sk::PathBuilder::new();
    builder.move_to(first.x as f32, first.y as f32);
    for p in rest {
        builder.line_to(p.x as f32, p.y as f32);
    }
    builder.finish()
}

/// The outline of `text` set in the embedded font, placed so `origin`
/// sits where the alignments say.
fn text_path(
    font: &Face,
    text: &str,
    origin: Point,
    size: f64,
    h_align: HAlign,
    v_align: VAlign,
) -> Option<sk::Path> {
    let scale = size as f32 / font.units_per_em() as f32;
    let glyphs: Vec<GlyphId> = text
        .chars()
        .map(|c| font.glyph_index(c).unwrap_or(GlyphId(0)))
        .collect();
    let advance = |g: GlyphId| font.glyph_hor_advance(g).unwrap_or(0) as f32 * scale;
    let width: f32 = glyphs.iter().map(|&g| advance(g)).sum();

    let x = origin.x as f32
        - match h_align {
            HAlign::Left => 0.0,
            HAlign::Center => width / 2.0,
            HAlign::Right => width,
        };
    let baseline = origin.y as f32
        + match v_align {
            VAlign::Top => font.ascender() as f32 * scale,

            // half the capital height rather than half the
            // ascender to descender span, so a digit or a
            // capital sits in the middle of a grid cell
            VAlign::Middle => font.capital_height().unwrap_or(font.ascender()) as f32 * scale / 2.0,
            VAlign::Baseline => 0.0,
        };

    let mut outline = Outline {
        builder: sk::PathBuilder::new(),
        x,
        y: baseline,
        scale,
    };
    for g in glyphs {
        font.outline_glyph(g, &mut outline);
        outline.x += advance(g);
    }
    outline.builder.finish()
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
            fill: Some(RED),
            stroke: None,
        };
        draw_primitives(&mut pixmap, &[rect]);
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
            stroke: Stroke {
                color: RED,
                width: 2.0,
            },
        };
        draw_primitives(&mut pixmap, &[line]);
        assert!(!is_white(&pixmap, 15, 5));
        assert!(!is_white(&pixmap, 25, 15));
        assert!(is_white(&pixmap, 15, 15));
    }

    #[test]
    fn centered_text_is_centered_on_its_origin() {
        let mut pixmap = blank(80, 80);
        let text = Primitive::Text {
            origin: Point::new(40.0, 40.0),
            text: "8".into(),
            size: 40.0,
            color: Color::BLACK,
            h_align: HAlign::Center,
            v_align: VAlign::Middle,
        };
        draw_primitives(&mut pixmap, &[text]);
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
            text: s.into(),
            size: 30.0,
            color: Color::BLACK,
            h_align: HAlign::Left,
            v_align: VAlign::Baseline,
        };
        draw_primitives(&mut pixmap, &[text("a\u{10fffd}b")]);
        let with_gap = ink(&pixmap);
        let mut pixmap = blank(200, 60);
        draw_primitives(&mut pixmap, &[text("a b")]);
        assert_eq!(with_gap[2], ink(&pixmap)[2]);
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
        };
        let pixmap = draw_frame(&scene, 0.0);
        assert_eq!((pixmap.width(), pixmap.height()), (12, 7));
        let p = pixmap.pixel(11, 6).unwrap();
        assert_eq!((p.red(), p.green(), p.blue()), (255, 0, 0));
    }
}

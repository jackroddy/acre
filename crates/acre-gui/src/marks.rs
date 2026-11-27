//! The marks drawn over the render of a scene: outlines, handles,
//! guides and the grid, painted with tiny-skia onto the pane's pixels.

use tiny_skia as sk;
use una::Rgba;

use crate::screen;

/// One shape drawn over the render, in the pane's pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    Line {
        from: screen::Point,
        to: screen::Point,
        width: f32,
        color: Rgba,
    },
    /// A line with a head at `to`.
    Arrow {
        from: screen::Point,
        to: screen::Point,
        width: f32,
        color: Rgba,
    },
    /// The outline of a rectangle, centred on its edges.
    Frame {
        rect: screen::Rect,
        width: f32,
        color: Rgba,
    },
    Fill {
        rect: screen::Rect,
        color: Rgba,
    },
    /// The outline of the ellipse inside `rect`.
    Oval {
        rect: screen::Rect,
        width: f32,
        color: Rgba,
    },
    /// A line through `points`, and back to the first when `closed`.
    Path {
        points: Vec<screen::Point>,
        closed: bool,
        width: f32,
        color: Rgba,
    },
    Dot {
        at: screen::Point,
        radius: f32,
        color: Rgba,
    },
    Ring {
        at: screen::Point,
        radius: f32,
        width: f32,
        color: Rgba,
    },
}

/// Paint `marks` in order onto `pixmap`.
pub fn paint(pixmap: &mut sk::PixmapMut, marks: &[Mark]) {
    for mark in marks {
        match mark {
            Mark::Line {
                from,
                to,
                width,
                color,
            } => stroke(pixmap, line(&[*from, *to], false), *width, *color),
            Mark::Arrow {
                from,
                to,
                width,
                color,
            } => {
                stroke(pixmap, line(&[*from, *to], false), *width, *color);
                for side in arrow_head(*from, *to) {
                    stroke(pixmap, line(&[*to, side], false), *width, *color);
                }
            }
            Mark::Frame { rect, width, color } => {
                let path = to_sk(*rect).map(sk::PathBuilder::from_rect);
                stroke(pixmap, path, *width, *color);
            }
            Mark::Fill { rect, color } => {
                if let Some(rect) = to_sk(*rect) {
                    let paint = paint_of(*color);
                    pixmap.fill_rect(rect, &paint, sk::Transform::identity(), None);
                }
            }
            Mark::Oval { rect, width, color } => {
                let path = to_sk(*rect).and_then(sk::PathBuilder::from_oval);
                stroke(pixmap, path, *width, *color);
            }
            Mark::Path {
                points,
                closed,
                width,
                color,
            } => stroke(pixmap, line(points, *closed), *width, *color),
            Mark::Dot { at, radius, color } => {
                if let Some(path) = sk::PathBuilder::from_circle(at.x, at.y, *radius) {
                    let paint = paint_of(*color);
                    let (rule, identity) = (sk::FillRule::Winding, sk::Transform::identity());
                    pixmap.fill_path(&path, &paint, rule, identity, None);
                }
            }
            Mark::Ring {
                at,
                radius,
                width,
                color,
            } => {
                let path = sk::PathBuilder::from_circle(at.x, at.y, *radius);
                stroke(pixmap, path, *width, *color);
            }
        }
    }
}

/// The ends of the two strokes of an arrow's head at `to`: a quarter of
/// the arrow long, each 30 degrees off the shaft.
fn arrow_head(from: screen::Point, to: screen::Point) -> [screen::Point; 2] {
    let back = (from - to) / 4.0;
    let (sin, cos) = 30f32.to_radians().sin_cos();
    let turn =
        |sin: f32| screen::Point::new(back.x * cos - back.y * sin, back.x * sin + back.y * cos);
    [to + turn(sin), to + turn(-sin)]
}

fn line(points: &[screen::Point], closed: bool) -> Option<sk::Path> {
    let (first, rest) = points.split_first()?;
    let mut path = sk::PathBuilder::new();
    path.move_to(first.x, first.y);
    for p in rest {
        path.line_to(p.x, p.y);
    }
    if closed {
        path.close();
    }
    path.finish()
}

fn stroke(pixmap: &mut sk::PixmapMut, path: Option<sk::Path>, width: f32, color: Rgba) {
    let Some(path) = path else { return };
    let stroke = sk::Stroke {
        width,
        ..sk::Stroke::default()
    };
    let paint = paint_of(color);
    pixmap.stroke_path(&path, &paint, &stroke, sk::Transform::identity(), None);
}

fn paint_of([red, green, blue, alpha]: Rgba) -> sk::Paint<'static> {
    let mut paint = sk::Paint::default();
    paint.set_color(sk::Color::from_rgba(red, green, blue, alpha).unwrap_or(sk::Color::BLACK));
    paint.anti_alias = true;
    paint
}

fn to_sk(rect: screen::Rect) -> Option<sk::Rect> {
    let max = rect.max();
    sk::Rect::from_ltrb(rect.min.x, rect.min.y, max.x, max.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_arrow_head_points_back_along_the_shaft() {
        let [a, b] = arrow_head(screen::Point::new(0.0, 0.0), screen::Point::new(40.0, 0.0));
        assert!(a.x < 40.0 && b.x < 40.0);
        assert!((a.y + b.y).abs() < 1e-4);
        assert!((a.distance(screen::Point::new(40.0, 0.0)) - 10.0).abs() < 1e-4);
    }

    #[test]
    fn a_filled_mark_covers_its_pixels_and_no_others() {
        let mut pixmap = sk::Pixmap::new(4, 4).unwrap();
        let rect = screen::Rect {
            min: screen::Point::new(1.0, 1.0),
            size: screen::Point::new(2.0, 2.0),
        };
        let red = [1.0, 0.0, 0.0, 1.0];
        paint(&mut pixmap.as_mut(), &[Mark::Fill { rect, color: red }]);
        let at = |x: u32, y: u32| pixmap.pixel(x, y).unwrap().red();
        assert_eq!((at(1, 1), at(2, 2)), (255, 255));
        assert_eq!((at(0, 0), at(3, 3)), (0, 0));
    }
}

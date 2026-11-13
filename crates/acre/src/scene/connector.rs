use super::diagram::Label;
use super::primitive::{HAlign, Point, Primitive, Stroke, VAlign};

/// The path a connector takes from its start to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Straight,
    /// Three segments at right angles, turning halfway along the longer
    /// of the two distances between the ends.
    Elbow,
    /// A cubic whose handles run along that same axis from each end.
    Curve,
}

/// What a connector draws at one end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Head {
    #[default]
    None,
    Triangle,
    Open,
    Dot,
}

/// Which side of the connector its label sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelSide {
    /// The side that faces up, or right on a vertical connector.
    #[default]
    Above,
    /// The left of the way from start to end.
    Left,
    /// The right of the way from start to end.
    Right,
}

/// A line from `from` to `to` along a route, with a head at either
/// end and an optional label beside it.
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    pub from: Point,
    pub to: Point,
    pub stroke: Stroke,
    pub route: Route,
    /// The head at `to`.
    pub head: Head,
    /// The head at `from`.
    pub tail: Head,
    pub label: Option<Label>,
    pub label_side: LabelSide,
    /// How far along the route the label sits, from 0 at `from` to 1
    /// at `to`.
    pub label_at: f64,
}

/// The gap between the line and its label.
const LABEL_GAP: f64 = 6.0;

/// The segments a curve is drawn as.
const CURVE_STEPS: usize = 24;

fn length(p: Point) -> f64 {
    (p.x * p.x + p.y * p.y).sqrt()
}

impl Connector {
    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let path = self.path();
        if path.len() < 2 {
            return;
        }
        let len: f64 = path.windows(2).map(|w| length(w[1] - w[0])).sum();
        let heads = [self.head, self.tail]
            .iter()
            .filter(|h| **h != Head::None)
            .count();
        let avail = len / heads.max(1) as f64;

        let [first, second] = [path[0], path[1]];
        let [prev, last] = [path[path.len() - 2], path[path.len() - 1]];
        let end_dir = (last - prev) * (1.0 / length(last - prev));
        let start_dir = (first - second) * (1.0 / length(first - second));

        let mut shaft = path.clone();
        shorten_end(&mut shaft, cut(self.head, self.stroke, avail));
        shaft.reverse();
        shorten_end(&mut shaft, cut(self.tail, self.stroke, avail));
        shaft.reverse();
        if let [from, to] = shaft[..] {
            out.push(Primitive::Line {
                from,
                to,
                stroke: self.stroke,
            });
        } else {
            out.push(Primitive::Polyline {
                points: shaft,
                stroke: self.stroke,
            });
        }
        draw_head(self.head, last, end_dir, self.stroke, avail, out);
        draw_head(self.tail, first, start_dir, self.stroke, avail, out);

        if let Some(label) = &self.label {
            let (at, along) = point_at(&path, self.label_at);
            out.push(self.place_label(label, at, along));
        }
    }

    /// The route as a list of points, each joined to the next by a
    /// straight segment, without repeated points.
    fn path(&self) -> Vec<Point> {
        let (a, b) = (self.from, self.to);
        let d = b - a;
        let mut points = match self.route {
            Route::Straight => vec![a, b],
            Route::Elbow if d.x == 0.0 || d.y == 0.0 => vec![a, b],
            Route::Elbow if d.x.abs() >= d.y.abs() => {
                let mid = a.x + d.x / 2.0;
                vec![a, Point::new(mid, a.y), Point::new(mid, b.y), b]
            }
            Route::Elbow => {
                let mid = a.y + d.y / 2.0;
                vec![a, Point::new(a.x, mid), Point::new(b.x, mid), b]
            }
            Route::Curve => {
                let (c1, c2) = if d.x.abs() >= d.y.abs() {
                    (
                        Point::new(a.x + d.x / 2.0, a.y),
                        Point::new(b.x - d.x / 2.0, b.y),
                    )
                } else {
                    (
                        Point::new(a.x, a.y + d.y / 2.0),
                        Point::new(b.x, b.y - d.y / 2.0),
                    )
                };
                (0..=CURVE_STEPS)
                    .map(|i| {
                        let t = i as f64 / CURVE_STEPS as f64;
                        let u = 1.0 - t;
                        a * (u * u * u)
                            + c1 * (3.0 * u * u * t)
                            + c2 * (3.0 * u * t * t)
                            + b * (t * t * t)
                    })
                    .collect()
            }
        };
        points.dedup();
        points
    }

    /// The label placed with its box clear of the line by `LABEL_GAP`.
    ///
    /// `along` is the direction of the line at `at`. The box is
    /// centered on the point `LABEL_GAP` plus the box's half extent
    /// along the normal away from `at`.
    fn place_label(&self, label: &Label, at: Point, along: Point) -> Primitive {
        let left = Point::new(along.y, -along.x);
        let normal = match self.label_side {
            LabelSide::Left => left,
            LabelSide::Right => left * -1.0,
            LabelSide::Above if left.y < -1e-9 || (left.y.abs() <= 1e-9 && left.x > 0.0) => left,
            LabelSide::Above => left * -1.0,
        };
        // the box runs from the first line's ascent down to the
        // baseline, so a descender may hang past it as it always has
        let layout = crate::text::layout(&label.runs, label.size, label.width);
        let (min, max) = layout.extent(Point::ZERO, HAlign::Center, VAlign::Baseline);
        let (half_w, ascent) = ((max.x - min.x) / 2.0, -min.y);
        let reach = LABEL_GAP + normal.x.abs() * half_w + normal.y.abs() * ascent / 2.0;
        let center = at + normal * reach;
        let origin = Point::new(center.x - (min.x + max.x) / 2.0, center.y + ascent / 2.0);
        label.at(origin, HAlign::Center, VAlign::Baseline)
    }
}

/// The distance a head takes off the end of the shaft.
fn cut(head: Head, stroke: Stroke, avail: f64) -> f64 {
    match head {
        Head::None | Head::Open => 0.0,
        Head::Triangle => head_len(stroke, avail),
        Head::Dot => dot_radius(stroke).min(avail / 2.0),
    }
}

fn head_len(stroke: Stroke, avail: f64) -> f64 {
    (8.0 + 3.0 * stroke.width).min(avail)
}

fn dot_radius(stroke: Stroke) -> f64 {
    2.5 + 1.5 * stroke.width
}

/// Draw `head` with its tip at `tip`, on a line that arrives going
/// `dir`.
fn draw_head(
    head: Head,
    tip: Point,
    dir: Point,
    stroke: Stroke,
    avail: f64,
    out: &mut Vec<Primitive>,
) {
    let len = head_len(stroke, avail);
    let back = tip - dir * len;
    let side = Point::new(-dir.y, dir.x) * (len * 0.45);
    match head {
        Head::None => {}
        Head::Triangle => out.push(Primitive::Polygon {
            points: vec![tip, back + side, back - side],
            fill: Some(stroke.color),
            stroke: None,
        }),
        Head::Open => out.push(Primitive::Polyline {
            points: vec![back + side, tip, back - side],
            stroke: Stroke::solid(stroke.color, stroke.width),
        }),
        Head::Dot => {
            let r = dot_radius(stroke).min(avail / 2.0);
            out.push(Primitive::Ellipse {
                center: tip - dir * r,
                radius: Point::new(r, r),
                fill: Some(stroke.color),
                stroke: None,
            });
        }
    }
}

/// Shorten `path` by `by` from its end, keeping at least two points.
fn shorten_end(path: &mut Vec<Point>, mut by: f64) {
    while path.len() > 2 {
        let n = path.len();
        let seg = length(path[n - 1] - path[n - 2]);
        if seg > by {
            break;
        }
        by -= seg;
        path.pop();
    }
    let n = path.len();
    let d = path[n - 1] - path[n - 2];
    let seg = length(d);
    if seg > 0.0 {
        path[n - 1] = path[n - 1] - d * (by.min(seg) / seg);
    }
}

/// The point a fraction `f` of the way along `path` by length, and
/// the unit direction of the segment it lies on.
fn point_at(path: &[Point], f: f64) -> (Point, Point) {
    let total: f64 = path.windows(2).map(|w| length(w[1] - w[0])).sum();
    let mut left = f.clamp(0.0, 1.0) * total;
    for (i, w) in path.windows(2).enumerate() {
        let seg = length(w[1] - w[0]);
        if left <= seg || i == path.len() - 2 {
            let dir = (w[1] - w[0]) * (1.0 / seg);
            return (w[0] + dir * left.min(seg), dir);
        }
        left -= seg;
    }
    unreachable!("a path has two points")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::primitive::{Color, Font};

    fn connector(from: (f64, f64), to: (f64, f64)) -> Connector {
        Connector {
            from: Point::new(from.0, from.1),
            to: Point::new(to.0, to.1),
            stroke: Stroke::solid(Color::BLACK, 2.0),
            route: Route::Straight,
            head: Head::Triangle,
            tail: Head::None,
            label: None,
            label_side: LabelSide::Above,
            label_at: 0.5,
        }
    }

    fn labeled(mut c: Connector, side: LabelSide) -> Connector {
        c.head = Head::None;
        c.label = Some(Label::plain("label", 14.0, Font::Sans, Color::BLACK));
        c.label_side = side;
        c
    }

    fn label_box(c: &Connector) -> (Point, Point) {
        let mut out = Vec::new();
        c.draw(&mut out);
        let Some(Primitive::Text {
            origin,
            runs,
            size,
            width,
            h_align,
            v_align,
        }) = out.last()
        else {
            panic!("expected the label last, got {out:?}");
        };
        let (min, max) =
            crate::text::layout(runs, *size, *width).extent(*origin, *h_align, *v_align);
        // the box stops at the baseline, as the placement does
        (min, Point::new(max.x, origin.y))
    }

    #[test]
    fn an_arrow_ends_its_shaft_where_the_head_begins() {
        let mut out = Vec::new();
        connector((0.0, 0.0), (100.0, 0.0)).draw(&mut out);
        match &out[..] {
            [
                Primitive::Line { to, .. },
                Primitive::Polygon { points, .. },
            ] => {
                assert_eq!(*to, Point::new(86.0, 0.0));
                assert_eq!(points[0], Point::new(100.0, 0.0));
            }
            other => panic!("expected a line and a head, got {other:?}"),
        }
    }

    #[test]
    fn a_tail_draws_at_the_start() {
        let mut c = connector((0.0, 0.0), (100.0, 0.0));
        c.head = Head::None;
        c.tail = Head::Triangle;
        let mut out = Vec::new();
        c.draw(&mut out);
        match &out[..] {
            [
                Primitive::Line { from, to, .. },
                Primitive::Polygon { points, .. },
            ] => {
                assert!((from.x - 14.0).abs() < 1e-9 && from.y == 0.0);
                assert_eq!(*to, Point::new(100.0, 0.0));
                assert_eq!(points[0], Point::new(0.0, 0.0));
            }
            other => panic!("expected a line and a head, got {other:?}"),
        }
    }

    #[test]
    fn a_dot_head_is_a_circle_ending_at_the_end() {
        let mut c = connector((0.0, 0.0), (100.0, 0.0));
        c.head = Head::Dot;
        let mut out = Vec::new();
        c.draw(&mut out);
        match &out[..] {
            [
                Primitive::Line { to, .. },
                Primitive::Ellipse { center, radius, .. },
            ] => {
                assert_eq!(*radius, Point::new(5.5, 5.5));
                assert_eq!(*center, Point::new(94.5, 0.0));
                assert_eq!(*to, *center);
            }
            other => panic!("expected a line and a dot, got {other:?}"),
        }
    }

    #[test]
    fn an_open_head_is_two_strokes_meeting_at_the_end() {
        let mut c = connector((0.0, 0.0), (100.0, 0.0));
        c.head = Head::Open;
        let mut out = Vec::new();
        c.draw(&mut out);
        match &out[..] {
            [
                Primitive::Line { to, .. },
                Primitive::Polyline { points, .. },
            ] => {
                assert_eq!(*to, Point::new(100.0, 0.0));
                assert_eq!(points[1], Point::new(100.0, 0.0));
                assert_eq!(points[0].x, points[2].x);
                assert_eq!(points[0].y, -points[2].y);
            }
            other => panic!("expected a line and a head, got {other:?}"),
        }
    }

    #[test]
    fn an_elbow_turns_at_right_angles_halfway() {
        let mut c = connector((0.0, 0.0), (100.0, 40.0));
        c.route = Route::Elbow;
        assert_eq!(
            c.path(),
            [
                Point::new(0.0, 0.0),
                Point::new(50.0, 0.0),
                Point::new(50.0, 40.0),
                Point::new(100.0, 40.0)
            ]
        );
        let mut c = connector((0.0, 0.0), (40.0, 100.0));
        c.route = Route::Elbow;
        assert_eq!(c.path()[1], Point::new(0.0, 50.0));
    }

    #[test]
    fn an_elbow_between_aligned_ends_is_one_segment() {
        let mut c = connector((0.0, 0.0), (100.0, 0.0));
        c.route = Route::Elbow;
        assert_eq!(c.path().len(), 2);
    }

    #[test]
    fn a_curve_leaves_and_arrives_along_the_axis() {
        let mut c = connector((0.0, 0.0), (100.0, 40.0));
        c.route = Route::Curve;
        let path = c.path();
        assert_eq!(path[0], Point::new(0.0, 0.0));
        assert_eq!(*path.last().unwrap(), Point::new(100.0, 40.0));
        let first = path[1] - path[0];
        let last = path[path.len() - 1] - path[path.len() - 2];
        assert!(first.y.abs() < first.x * 0.1);
        assert!(last.y.abs() < last.x * 0.1);
    }

    #[test]
    fn a_default_label_over_a_horizontal_line_keeps_its_baseline() {
        let c = labeled(connector((0.0, 100.0), (100.0, 100.0)), LabelSide::Above);
        let mut out = Vec::new();
        c.draw(&mut out);
        let Some(Primitive::Text { origin, .. }) = out.last() else {
            panic!("expected a label");
        };
        assert!((origin.x - 50.0).abs() < 1e-9);
        assert!((origin.y - 94.0).abs() < 1e-9);
    }

    #[test]
    fn a_label_beside_a_vertical_line_clears_it_on_either_side() {
        for (side, to_y) in [
            (LabelSide::Left, 0.0),
            (LabelSide::Right, 0.0),
            (LabelSide::Left, 200.0),
            (LabelSide::Right, 200.0),
        ] {
            let c = labeled(connector((100.0, 100.0), (100.0, to_y)), side);
            let (min, max) = label_box(&c);
            // left of a line drawn down the screen is to the right
            let rightward = (side == LabelSide::Left) == (to_y > 100.0);
            if rightward {
                assert!(
                    min.x >= 100.0 + LABEL_GAP - 1e-9,
                    "{side:?} {to_y}: {min:?}"
                );
            } else {
                assert!(
                    max.x <= 100.0 - LABEL_GAP + 1e-9,
                    "{side:?} {to_y}: {max:?}"
                );
            }
        }
    }

    #[test]
    fn a_default_label_beside_a_vertical_line_goes_to_its_right() {
        for to_y in [0.0, 200.0] {
            let c = labeled(connector((100.0, 100.0), (100.0, to_y)), LabelSide::Above);
            assert!(label_box(&c).0.x >= 100.0 + LABEL_GAP - 1e-9);
        }
    }

    #[test]
    fn a_label_beside_a_diagonal_line_stays_off_the_stroke() {
        // the distance from a point to the line y = x
        let off = |p: Point| (p.x - p.y).abs() / 2f64.sqrt();
        for side in [LabelSide::Left, LabelSide::Right, LabelSide::Above] {
            let c = labeled(connector((0.0, 0.0), (200.0, 200.0)), side);
            let (min, max) = label_box(&c);
            let corners = [min, max, Point::new(min.x, max.y), Point::new(max.x, min.y)];
            let nearest = corners
                .iter()
                .map(|p| off(*p))
                .fold(f64::INFINITY, f64::min);
            assert!(nearest >= LABEL_GAP - 1e-6, "{side:?}: {nearest}");
        }
    }

    #[test]
    fn a_label_sits_at_a_fraction_along_the_route() {
        let mut c = labeled(connector((0.0, 100.0), (100.0, 100.0)), LabelSide::Above);
        c.label_at = 0.25;
        let mut out = Vec::new();
        c.draw(&mut out);
        let Some(Primitive::Text { origin, .. }) = out.last() else {
            panic!("expected a label");
        };
        assert!((origin.x - 25.0).abs() < 1e-9);
    }
}

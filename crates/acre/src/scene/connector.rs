use super::Target;
use super::diagram::{Label, Shape};
use super::primitive::{HAlign, Motion, Point, Primitive, Stroke, VAlign};

/// The path a connector takes from its start to its end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Route {
    #[default]
    Straight,
    /// Segments at right angles. Ends on sides give an L when the sides
    /// are perpendicular and a Z when parallel. Without sides it turns
    /// halfway along the longer distance between the ends.
    Elbow,
    /// A cubic whose handles run out of each end's side, or along the
    /// longer distance between the ends when it has none.
    Curve,
    /// A cubic whose handles run along each end's way of travel, for
    /// half the distance between the ends along it. A `sweep` draws it.
    Sweep {
        /// The way it sets off from an end with no side.
        leave: Option<Dir>,
        /// The way it arrives at an end with no side.
        enter: Option<Dir>,
    },
}

/// A way of travel along an axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    Up,
    Down,
    Left,
    Right,
}

impl Dir {
    fn vector(self) -> Point {
        match self {
            Dir::Up => Point::new(0.0, -1.0),
            Dir::Down => Point::new(0.0, 1.0),
            Dir::Left => Point::new(-1.0, 0.0),
            Dir::Right => Point::new(1.0, 0.0),
        }
    }
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
    /// The direction the route leaves `from`, out of the box it sits on.
    pub from_dir: Option<Point>,
    /// The direction pointing out of the box at `to`, which the route
    /// arrives against.
    pub to_dir: Option<Point>,
    pub stroke: Stroke,
    pub route: Route,
    /// The head at `to`.
    pub head: Head,
    /// The head at `from`.
    pub tail: Head,
    /// The length of a triangle or open head and the diameter of a
    /// dot, when set.
    pub head_size: Option<f64>,
    pub label: Option<Label>,
    pub label_side: LabelSide,
    /// How far along the route the label sits, from 0 at `from` to 1
    /// at `to`.
    pub label_at: f64,
    /// The things `from` and `to` are attached to, which they follow as
    /// those things move and scale.
    pub follow: [Option<Box<Follow>>; 2],
    /// Whether both ends sit on one side of one thing, so the route
    /// leaves that side and comes back into it.
    pub looped: bool,
}

/// Where a connector ends: a point, or the outline of a box it stops at.
#[derive(Debug, Clone, PartialEq)]
pub enum End {
    Point(Point),
    Box(Shape),
    /// The middle of a side of a box or one of its corners, and the
    /// direction that points out of the box there, diagonal at a corner.
    Side {
        at: Point,
        out: Point,
    },
}

/// An end attached to a thing in the scene.
#[derive(Debug, Clone, PartialEq)]
pub struct Follow {
    /// The thing the end is attached to.
    pub target: Target,
    /// The end where layout put the object.
    pub end: End,
}

impl End {
    fn moved(&self, motion: Motion) -> End {
        match self {
            End::Point(p) => End::Point(*p),
            End::Box(shape) => End::Box(Shape {
                origin: motion.apply(shape.origin),
                size: shape.size * motion.scale,
                ..shape.clone()
            }),
            End::Side { at, out } => End::Side {
                at: motion.apply(*at),
                out: *out,
            },
        }
    }

    fn center(&self) -> Point {
        match self {
            End::Point(p) => *p,
            End::Side { at, .. } => *at,
            End::Box(s) => s.center(),
        }
    }

    /// Where the line touches this end, coming from `other`.
    fn on_edge(&self, other: &End) -> Point {
        match self {
            End::Box(s) => s.edge_toward(other.center()),
            _ => self.center(),
        }
    }

    /// The direction out of the side the end sits on, when it names one.
    fn facing(&self) -> Option<Point> {
        match self {
            End::Side { out, .. } => Some(*out),
            _ => None,
        }
    }
}

/// The points and directions of a connector from `from` to `to`, as
/// `[from, to]` and `[from_dir, to_dir]`.
pub fn place(from: &End, to: &End) -> ([Point; 2], [Option<Point>; 2]) {
    (
        [from.on_edge(to), to.on_edge(from)],
        [from.facing(), to.facing()],
    )
}

/// The gap between the line and its label.
const LABEL_GAP: f64 = 6.0;

/// The segments a curve is drawn as.
const CURVE_STEPS: usize = 24;

/// How far past both ends an elbow steps out when its ends face the
/// same way.
const STEP_OUT: f64 = 24.0;

/// How far a loop's curve stands out from the side it leaves, about.
const LOOP_REACH: f64 = 28.0;

/// `dir` when it runs along an axis. A corner's diagonal becomes the
/// axis of whichever of the corner's two sides faces `toward` more.
fn square(dir: Point, toward: Point) -> Point {
    if dir.x == 0.0 || dir.y == 0.0 {
        dir
    } else if dir.x * toward.x >= dir.y * toward.y {
        Point::new(dir.x.signum(), 0.0)
    } else {
        Point::new(0.0, dir.y.signum())
    }
}

fn length(p: Point) -> f64 {
    (p.x * p.x + p.y * p.y).sqrt()
}

impl Connector {
    /// This connector with each end where `motions` take it: an
    /// attached end with the thing it is attached to, and a free end on
    /// its own.
    pub fn followed(&self, motions: [Motion; 2]) -> Connector {
        let [from, to] = [0, 1].map(|i| match &self.follow[i] {
            Some(f) => f.end.moved(motions[i]),
            None if motions[i].is_none() => End::Point([self.from, self.to][i]),
            None => End::Point(motions[i].apply([self.from, self.to][i])),
        });
        let ([from_at, to_at], [from_dir, to_dir]) = place(&from, &to);
        Connector {
            from: from_at,
            to: to_at,
            from_dir,
            to_dir,
            ..self.clone()
        }
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let path = self.path();
        if path.len() < 2 {
            return;
        }
        draw_shaft(
            &path,
            self.stroke,
            self.head,
            self.tail,
            self.head_size,
            out,
        );
        if let Some(label) = &self.label {
            let (at, along) = point_at(&path, self.label_at);
            out.push(self.place_label(label, at, along));
        }
    }

    /// The route as a list of points, each joined to the next by a
    /// straight segment, without repeated points.
    fn path(&self) -> Vec<Point> {
        let (a, b) = (self.from, self.to);
        let mut points = match self.route {
            Route::Straight if !self.looped => vec![a, b],
            Route::Elbow => self.elbow(),
            Route::Straight | Route::Curve | Route::Sweep { .. } => {
                let (c1, c2) = self.handles();
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

    fn elbow(&self) -> Vec<Point> {
        let (a, b) = (self.from, self.to);
        let d = b - a;
        let from_dir = self.from_dir.map(|dir| square(dir, d));
        let to_dir = self.to_dir.map(|dir| square(dir, d * -1.0));
        match (from_dir, to_dir) {
            (Some(from), Some(to)) if (from.x != 0.0) != (to.x != 0.0) => {
                let corner = if from.x != 0.0 {
                    Point::new(b.x, a.y)
                } else {
                    Point::new(a.x, b.y)
                };
                vec![a, corner, b]
            }
            (Some(from), Some(to)) => {
                // parallel ends facing the same way both step out past
                // the farther one, and the rest turn halfway
                let horizontal = from.x != 0.0;
                let (ca, cb, out) = if horizontal {
                    (a.x, b.x, from.x)
                } else {
                    (a.y, b.y, from.y)
                };
                let turn = if from == to {
                    let farthest = if out > 0.0 { ca.max(cb) } else { ca.min(cb) };
                    farthest + out * STEP_OUT
                } else {
                    ca + (cb - ca) / 2.0
                };
                bend(a, b, horizontal, turn)
            }
            (Some(dir), None) | (None, Some(dir)) => bend_halfway(a, b, dir.x != 0.0),
            (None, None) if d.x == 0.0 || d.y == 0.0 => vec![a, b],
            (None, None) => bend_halfway(a, b, d.x.abs() >= d.y.abs()),
        }
    }

    /// The two inner control points of the cubic from `from` to `to`.
    fn handles(&self) -> (Point, Point) {
        let (a, b) = (self.from, self.to);
        let d = b - a;
        if let Route::Sweep { leave, enter } = self.route {
            return sweep_handles(a, b, self.from_dir, self.to_dir, leave, enter);
        }
        if self.looped
            && let Some(out) = self.from_dir
        {
            // each handle stands out LOOP_REACH and leans away from
            // the other end, so the loop bulges wider than its ends
            let apart = d * (1.0 / length(d));
            let lean = apart * (LOOP_REACH / 2.0);
            return (
                a + out * (LOOP_REACH * 1.4) - lean,
                b + out * (LOOP_REACH * 1.4) + lean,
            );
        }
        // with no side, a handle runs along the longer axis toward the
        // other end, for half the distance along it
        let along_x = d.x.abs() >= d.y.abs();
        let (axis, reach) = if along_x {
            (Point::new(d.x.signum(), 0.0), d.x.abs() / 2.0)
        } else {
            (Point::new(0.0, d.y.signum()), d.y.abs() / 2.0)
        };
        let side_reach = length(d) / 2.0;
        let c1 = match self.from_dir {
            Some(dir) => a + dir * side_reach,
            None => a + axis * reach,
        };
        let c2 = match self.to_dir {
            Some(dir) => b + dir * side_reach,
            None => b - axis * reach,
        };
        (c1, c2)
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
        let layout = crate::text::layout(
            &label.fonts,
            &label.runs,
            label.size,
            label.width,
            label.justify,
        );
        let (min, max) = layout.extent(Point::ZERO, HAlign::Center, VAlign::Baseline);
        let (half_w, ascent) = ((max.x - min.x) / 2.0, -min.y);
        let reach = LABEL_GAP + normal.x.abs() * half_w + normal.y.abs() * ascent / 2.0;
        let center = at + normal * reach;
        let origin = Point::new(center.x - (min.x + max.x) / 2.0, center.y + ascent / 2.0);
        label.at(origin, HAlign::Center, VAlign::Baseline)
    }
}

/// Three segments from `a` to `b` that turn where the horizontal or
/// vertical coordinate is `turn`.
fn bend(a: Point, b: Point, horizontal: bool, turn: f64) -> Vec<Point> {
    if horizontal {
        vec![a, Point::new(turn, a.y), Point::new(turn, b.y), b]
    } else {
        vec![a, Point::new(a.x, turn), Point::new(b.x, turn), b]
    }
}

/// The two inner control points of a sweep from `a` to `b`. A side
/// sets an end's way of travel, and `leave` or `enter` sets it at an
/// end with none.
fn sweep_handles(
    a: Point,
    b: Point,
    from_dir: Option<Point>,
    to_dir: Option<Point>,
    leave: Option<Dir>,
    enter: Option<Dir>,
) -> (Point, Point) {
    let d = b - a;
    let unit = |v: Point| v * (1.0 / length(v));
    // to_dir points out of the box at `b`, against the way of travel;
    // an end with no way at all, which a build never makes, heads
    // straight for the other end
    let out = from_dir.or(leave.map(Dir::vector)).map_or(unit(d), unit);
    let arrive = to_dir
        .map(|dir| dir * -1.0)
        .or(enter.map(Dir::vector))
        .map_or(unit(d), unit);
    // half the distance along the way of travel keeps the curve moving
    // that way from end to end; an end behind or level with the way
    // has no such distance and takes half the whole length instead
    let reach = |dir: Point| {
        let along = d.x * dir.x + d.y * dir.y;
        if along > 0.0 {
            along / 2.0
        } else {
            length(d) / 2.0
        }
    };
    (a + out * reach(out), b - arrive * reach(arrive))
}

fn bend_halfway(a: Point, b: Point, horizontal: bool) -> Vec<Point> {
    let turn = if horizontal {
        a.x + (b.x - a.x) / 2.0
    } else {
        a.y + (b.y - a.y) / 2.0
    };
    bend(a, b, horizontal, turn)
}

/// Draw `path`, with no point repeated in a row, with `head` at its last
/// point and `tail` at its first, or nothing for fewer than two points.
pub(super) fn draw_shaft(
    path: &[Point],
    stroke: Stroke,
    head: Head,
    tail: Head,
    head_size: Option<f64>,
    out: &mut Vec<Primitive>,
) {
    if path.len() < 2 {
        return;
    }
    let len: f64 = path.windows(2).map(|w| length(w[1] - w[0])).sum();
    let heads = [head, tail].iter().filter(|h| **h != Head::None).count();
    let avail = len / heads.max(1) as f64;

    let [first, second] = [path[0], path[1]];
    let [prev, last] = [path[path.len() - 2], path[path.len() - 1]];
    let end_dir = (last - prev) * (1.0 / length(last - prev));
    let start_dir = (first - second) * (1.0 / length(first - second));

    let size = HeadSize::new(stroke, head_size);
    let mut shaft = path.to_vec();
    shorten_end(&mut shaft, cut(head, size, avail));
    shaft.reverse();
    shorten_end(&mut shaft, cut(tail, size, avail));
    shaft.reverse();
    if let [from, to] = shaft[..] {
        out.push(Primitive::Line { from, to, stroke });
    } else {
        out.push(Primitive::Polyline {
            points: shaft,
            stroke,
        });
    }
    draw_head(head, last, end_dir, stroke, size, avail, out);
    draw_head(tail, first, start_dir, stroke, size, avail, out);
}

/// The size of a connector's heads before the shaft's length caps them.
#[derive(Debug, Clone, Copy)]
struct HeadSize {
    /// The length of a triangle or open head.
    len: f64,
    dot_radius: f64,
}

impl HeadSize {
    fn new(stroke: Stroke, head_size: Option<f64>) -> HeadSize {
        match head_size {
            Some(size) => HeadSize {
                len: size,
                dot_radius: size / 2.0,
            },
            None => HeadSize {
                len: 8.0 + 3.0 * stroke.width,
                dot_radius: 2.5 + 1.5 * stroke.width,
            },
        }
    }
}

/// The distance a head takes off the end of the shaft.
fn cut(head: Head, size: HeadSize, avail: f64) -> f64 {
    match head {
        Head::None | Head::Open => 0.0,
        Head::Triangle => size.len.min(avail),
        Head::Dot => size.dot_radius.min(avail / 2.0),
    }
}

/// Draw `head` with its tip at `tip`, on a line that arrives going
/// `dir`.
fn draw_head(
    head: Head,
    tip: Point,
    dir: Point,
    stroke: Stroke,
    size: HeadSize,
    avail: f64,
    out: &mut Vec<Primitive>,
) {
    let len = size.len.min(avail);
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
            let r = size.dot_radius.min(avail / 2.0);
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
    use crate::fonts::Fonts;
    use crate::scene::primitive::{Color, Font};

    fn connector(from: (f64, f64), to: (f64, f64)) -> Connector {
        Connector {
            from: Point::new(from.0, from.1),
            to: Point::new(to.0, to.1),
            from_dir: None,
            to_dir: None,
            stroke: Stroke::solid(Color::BLACK, 2.0),
            route: Route::Straight,
            head: Head::Triangle,
            tail: Head::None,
            head_size: None,
            label: None,
            label_side: LabelSide::Above,
            label_at: 0.5,
            follow: [None, None],
            looped: false,
        }
    }

    fn labeled(mut c: Connector, side: LabelSide) -> Connector {
        c.head = Head::None;
        c.label = Some(Label::plain(
            &Fonts::default(),
            "label",
            14.0,
            Font::Sans,
            Color::BLACK,
        ));
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
            justify,
            h_align,
            v_align,
            ..
        }) = out.last()
        else {
            panic!("expected the label last, got {out:?}");
        };
        let (min, max) = crate::text::layout(&Fonts::default(), runs, *size, *width, *justify)
            .extent(*origin, *h_align, *v_align);
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
    fn head_size_sets_a_triangle_length_and_a_dot_diameter() {
        let mut c = connector((0.0, 0.0), (100.0, 0.0));
        c.head_size = Some(6.0);
        c.tail = Head::Dot;
        let mut out = Vec::new();
        c.draw(&mut out);
        match &out[..] {
            [
                Primitive::Line { from, to, .. },
                Primitive::Polygon { points, .. },
                Primitive::Ellipse { radius, .. },
            ] => {
                let near =
                    |p: Point, x: f64, y: f64| (p.x - x).abs() < 1e-9 && (p.y - y).abs() < 1e-9;
                assert!(near(*to, 94.0, 0.0), "{to:?}");
                assert!(near(points[1], 94.0, 2.7), "{:?}", points[1]);
                assert!(near(*from, 3.0, 0.0), "{from:?}");
                assert!(near(*radius, 3.0, 3.0), "{radius:?}");
            }
            other => panic!("expected a line, a head and a dot, got {other:?}"),
        }
    }

    #[test]
    fn a_sweep_between_two_downs_only_moves_down() {
        let mut c = connector((0.0, 0.0), (1200.0, 190.0));
        c.route = Route::Sweep {
            leave: Some(Dir::Down),
            enter: Some(Dir::Down),
        };
        assert_eq!(
            c.handles(),
            (Point::new(0.0, 95.0), Point::new(1200.0, 95.0))
        );
        let path = c.path();
        assert!(path.windows(2).all(|w| w[1].y >= w[0].y), "{path:?}");
    }

    #[test]
    fn a_sweep_toward_an_end_behind_it_reaches_half_the_length() {
        let mut c = connector((0.0, 100.0), (0.0, 0.0));
        c.route = Route::Sweep {
            leave: Some(Dir::Down),
            enter: Some(Dir::Up),
        };
        assert_eq!(c.handles(), (Point::new(0.0, 150.0), Point::new(0.0, 50.0)));
    }

    #[test]
    fn a_sweep_takes_its_way_from_a_side() {
        let mut c = connector((0.0, 0.0), (300.0, 100.0));
        c.route = Route::Sweep {
            leave: Some(Dir::Down),
            enter: None,
        };
        c.to_dir = Some(Point::new(-1.0, 0.0));
        assert_eq!(
            c.handles(),
            (Point::new(0.0, 50.0), Point::new(150.0, 100.0))
        );
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

    fn with_sides(mut c: Connector, from: (f64, f64), to: (f64, f64)) -> Connector {
        c.from_dir = Some(Point::new(from.0, from.1));
        c.to_dir = Some(Point::new(to.0, to.1));
        c
    }

    #[test]
    fn an_elbow_between_perpendicular_sides_turns_once() {
        let mut c = with_sides(
            connector((0.0, 0.0), (100.0, 80.0)),
            (1.0, 0.0),
            (0.0, -1.0),
        );
        c.route = Route::Elbow;
        assert_eq!(
            c.path(),
            [
                Point::new(0.0, 0.0),
                Point::new(100.0, 0.0),
                Point::new(100.0, 80.0)
            ]
        );
        let mut c = with_sides(
            connector((0.0, 0.0), (100.0, 80.0)),
            (0.0, 1.0),
            (-1.0, 0.0),
        );
        c.route = Route::Elbow;
        assert_eq!(c.path()[1], Point::new(0.0, 80.0));
    }

    #[test]
    fn an_elbow_between_sides_facing_each_other_turns_halfway() {
        let mut c = with_sides(
            connector((0.0, 0.0), (100.0, 40.0)),
            (1.0, 0.0),
            (-1.0, 0.0),
        );
        c.route = Route::Elbow;
        assert_eq!(
            c.path()[1..3],
            [Point::new(50.0, 0.0), Point::new(50.0, 40.0)]
        );
        let mut c = with_sides(
            connector((0.0, 0.0), (40.0, 100.0)),
            (0.0, 1.0),
            (0.0, -1.0),
        );
        c.route = Route::Elbow;
        assert_eq!(
            c.path()[1..3],
            [Point::new(0.0, 50.0), Point::new(40.0, 50.0)]
        );
    }

    #[test]
    fn an_elbow_leaves_a_corner_along_the_side_that_faces_the_other_end() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        // bottom-right to top-left with the far end mostly to the
        // right, so the ends face right and left and it turns halfway
        let mut c = with_sides(connector((0.0, 0.0), (100.0, 40.0)), (h, h), (-h, -h));
        c.route = Route::Elbow;
        assert_eq!(
            c.path()[1..3],
            [Point::new(50.0, 0.0), Point::new(50.0, 40.0)]
        );
        // the far end mostly below, so the ends face down and up
        let mut c = with_sides(connector((0.0, 0.0), (40.0, 100.0)), (h, h), (-h, -h));
        c.route = Route::Elbow;
        assert_eq!(
            c.path()[1..3],
            [Point::new(0.0, 50.0), Point::new(40.0, 50.0)]
        );
        // bottom-right to the top of a box to its right and below:
        // right out of the corner, then down into the top, an L
        let mut c = with_sides(connector((0.0, 0.0), (100.0, 40.0)), (h, h), (0.0, -1.0));
        c.route = Route::Elbow;
        assert_eq!(c.path()[1], Point::new(100.0, 0.0));
    }

    #[test]
    fn a_curve_leaves_a_corner_along_its_diagonal() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let mut c = with_sides(connector((0.0, 0.0), (100.0, 0.0)), (h, h), (-h, h));
        c.route = Route::Curve;
        let (c1, c2) = c.handles();
        assert_eq!(c1, Point::new(50.0 * h, 50.0 * h));
        assert_eq!(c2, Point::new(100.0 - 50.0 * h, 50.0 * h));
    }

    #[test]
    fn a_straight_loop_draws_as_a_curve_out_of_its_side() {
        let mut c = with_sides(connector((0.0, 0.0), (20.0, 0.0)), (0.0, -1.0), (0.0, -1.0));
        c.looped = true;
        let path = c.path();
        assert_eq!(path.len(), CURVE_STEPS + 1);
        let top = path.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
        assert!((top + 0.75 * 1.4 * LOOP_REACH).abs() < 1e-9);
        // it leans out past both ends
        assert!(path.iter().any(|p| p.x < 0.0) && path.iter().any(|p| p.x > 20.0));
    }

    #[test]
    fn an_elbow_between_sides_facing_the_same_way_steps_out_past_both() {
        let mut c = with_sides(connector((0.0, 0.0), (100.0, 40.0)), (1.0, 0.0), (1.0, 0.0));
        c.route = Route::Elbow;
        assert_eq!(
            c.path()[1..3],
            [
                Point::new(100.0 + STEP_OUT, 0.0),
                Point::new(100.0 + STEP_OUT, 40.0)
            ]
        );
        let mut c = with_sides(
            connector((0.0, 0.0), (40.0, 100.0)),
            (0.0, -1.0),
            (0.0, -1.0),
        );
        c.route = Route::Elbow;
        assert_eq!(
            c.path()[1..3],
            [Point::new(0.0, -STEP_OUT), Point::new(40.0, -STEP_OUT)]
        );
    }

    #[test]
    fn an_elbow_with_one_side_leaves_along_it() {
        let mut c = connector((0.0, 0.0), (100.0, 40.0));
        c.route = Route::Elbow;
        c.from_dir = Some(Point::new(0.0, 1.0));
        assert_eq!(c.path()[1], Point::new(0.0, 20.0));
        c.from_dir = None;
        c.to_dir = Some(Point::new(0.0, -1.0));
        assert_eq!(c.path()[1], Point::new(0.0, 20.0));
    }

    #[test]
    fn a_curve_runs_its_handles_out_of_the_sides() {
        let mut c = with_sides(
            connector((0.0, 0.0), (100.0, 100.0)),
            (0.0, 1.0),
            (1.0, 0.0),
        );
        c.route = Route::Curve;
        let (c1, c2) = c.handles();
        let half = 100.0 * 2f64.sqrt() / 2.0;
        assert_eq!(c1, Point::new(0.0, half));
        assert_eq!(c2, Point::new(100.0 + half, 100.0));
        let path = c.path();
        let first = path[1] - path[0];
        let last = path[path.len() - 1] - path[path.len() - 2];
        assert!(first.x.abs() < first.y * 0.1);
        assert!(last.y.abs() < last.x.abs() * 0.5);
        assert!(last.x < 0.0);
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

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
    /// Centered on the line, which breaks around it.
    On,
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
    /// A point on a side of a box or one of its corners, and the
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
        let [from, to] = self.moved_ends(motions);
        let ([from_at, to_at], [from_dir, to_dir]) = place(&from, &to);
        Connector {
            from: from_at,
            to: to_at,
            from_dir,
            to_dir,
            ..self.clone()
        }
    }

    /// The ends `from` and `to` where `motions` take them.
    fn moved_ends(&self, motions: [Motion; 2]) -> [End; 2] {
        [0, 1].map(|i| match &self.follow[i] {
            Some(f) => f.end.moved(motions[i]),
            None if motions[i].is_none() => End::Point([self.from, self.to][i]),
            None => End::Point(motions[i].apply([self.from, self.to][i])),
        })
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let path = self.path();
        if path.len() < 2 {
            return;
        }
        let (i, at, along) = point_at(&path, self.label_at);
        let label = self
            .label
            .as_ref()
            .map(|label| self.place_label(label, at, along));
        // the box the line breaks around, the label's own padded by
        // the gap a label beside the line keeps from it
        let gap = match &label {
            Some((_, half)) if self.label_side == LabelSide::On => {
                let pad = *half + Point::new(LABEL_GAP, LABEL_GAP);
                Some((at - pad, at + pad))
            }
            _ => None,
        };
        if let Route::Sweep { .. } = self.route {
            let length: f64 = path.windows(2).map(|w| length(w[1] - w[0])).sum();
            self.draw_sweep(length, gap, out);
        } else if let Some((min, max)) = gap {
            let after = leave_box([at].iter().chain(&path[i + 1..]), min, max);
            let mut before = leave_box([at].iter().chain(path[..=i].iter().rev()), min, max);
            before.reverse();
            let stroke = self.stroke;
            draw_shaft(&before, stroke, Head::None, self.tail, self.head_size, out);
            draw_shaft(&after, stroke, self.head, Head::None, self.head_size, out);
        } else {
            draw_shaft(
                &path,
                self.stroke,
                self.head,
                self.tail,
                self.head_size,
                out,
            );
        }
        if let Some((text, _)) = label {
            out.push(text);
        }
    }

    /// Draw a sweep's shaft as one cubic that ends where each head
    /// begins, with the heads along the sweep's ways and a break where
    /// it crosses the box `gap`. `length` is the curve's length, about.
    fn draw_sweep(&self, length: f64, gap: Option<(Point, Point)>, out: &mut Vec<Primitive>) {
        let heads = [self.head, self.tail]
            .iter()
            .filter(|h| **h != Head::None)
            .count();
        let avail = length / heads.max(1) as f64;
        let size = HeadSize::new(self.stroke, self.head_size);
        let (leave, arrive) = self.sweep_ways();
        // the shaft runs between the backs of the heads, so it meets
        // each head at the middle of its back and along its axis
        let from = self.from + leave * cut(self.tail, size, avail);
        let to = self.to - arrive * cut(self.head, size, avail);
        let (c1, c2) = sweep_handles(from, to, leave, arrive);
        let cubic = [from, c1, c2, to];
        let pieces = match gap.and_then(|(min, max)| cubic_gap(cubic, min, max)) {
            Some((t0, t1)) => [
                (t0 > 0.0, split(cubic, t0).0),
                (t1 < 1.0, split(cubic, t1).1),
            ]
            .into_iter()
            .filter_map(|(drawn, piece)| drawn.then_some(piece))
            .collect(),
            None => vec![cubic],
        };
        for [from, c1, c2, to] in pieces {
            out.push(Primitive::Cubic {
                from,
                c1,
                c2,
                to,
                stroke: self.stroke,
            });
        }
        draw_head(self.head, self.to, arrive, self.stroke, size, avail, out);
        draw_head(
            self.tail,
            self.from,
            leave * -1.0,
            self.stroke,
            size,
            avail,
            out,
        );
    }

    /// The unit ways a sweep sets off from `from` and arrives at `to`. A
    /// side sets an end's way, and `leave` or `enter` sets it at an end
    /// with none.
    fn sweep_ways(&self) -> (Point, Point) {
        let Route::Sweep { leave, enter } = self.route else {
            unreachable!("only a sweep has ways");
        };
        let d = self.to - self.from;
        // to_dir points out of the box at `to`, against the way of
        // travel; an end with no way at all, which a build never makes,
        // heads straight for the other end
        let out = self
            .from_dir
            .or(leave.map(Dir::vector))
            .map_or(unit(d), unit);
        let arrive = self
            .to_dir
            .map(|dir| dir * -1.0)
            .or(enter.map(Dir::vector))
            .map_or(unit(d), unit);
        (out, arrive)
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
        if let Route::Sweep { .. } = self.route {
            let (out, arrive) = self.sweep_ways();
            return sweep_handles(a, b, out, arrive);
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

    /// The label placed with its box clear of the line by `LABEL_GAP`,
    /// or centered on `at` when it sits on the line, and the half size
    /// of its box.
    ///
    /// `along` is the direction of the line at `at`. The box is
    /// centered on the point `LABEL_GAP` plus the box's half extent
    /// along the normal away from `at`.
    fn place_label(&self, label: &Label, at: Point, along: Point) -> (Primitive, Point) {
        let left = Point::new(along.y, -along.x);
        let normal = match self.label_side {
            LabelSide::Left => left,
            LabelSide::Right => left * -1.0,
            LabelSide::Above if left.y < -1e-9 || (left.y.abs() <= 1e-9 && left.x > 0.0) => left,
            LabelSide::Above => left * -1.0,
            LabelSide::On => Point::ZERO,
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
        let reach = match self.label_side {
            LabelSide::On => 0.0,
            _ => LABEL_GAP + normal.x.abs() * half_w + normal.y.abs() * ascent / 2.0,
        };
        let center = at + normal * reach;
        let origin = Point::new(center.x - (min.x + max.x) / 2.0, center.y + ascent / 2.0);
        (
            label.at(origin, HAlign::Center, VAlign::Baseline),
            Point::new(half_w, ascent / 2.0),
        )
    }
}

/// Several lines joined into one arrow: a sweep from each `from` into a
/// junction, and an arrow on from the junction to `to`.
#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    /// One sweep per `from`, each ending at the junction.
    pub branches: Vec<Connector>,
    /// The arrow from the junction to `to`, which carries the head and
    /// the label.
    pub trunk: Connector,
    /// The junction, when `at` sets it.
    pub at: Option<Point>,
}

impl Merge {
    /// Put the junction, and every line between it and `froms` and `to`,
    /// one `from` per branch.
    pub fn join(&mut self, froms: &[End], to: &End) {
        let junction = self.at.unwrap_or_else(|| junction(froms, to));
        let ([at, to_at], [_, to_dir]) = place(&End::Point(junction), to);
        let way = if at == to_at {
            Point::new(0.0, 1.0)
        } else {
            unit(to_at - at)
        };
        self.trunk.from = at;
        self.trunk.to = to_at;
        self.trunk.to_dir = to_dir;
        for (branch, from) in self.branches.iter_mut().zip(froms) {
            let ([from_at, _], [from_dir, _]) = place(from, &End::Point(junction));
            branch.from = from_at;
            branch.to = junction;
            // a `from` with no side sets off the way the trunk runs,
            // and every branch arrives along it, so they meet the trunk
            // without a corner
            branch.from_dir = Some(from_dir.unwrap_or(way));
            branch.to_dir = Some(way * -1.0);
        }
    }

    /// This merge with each branch's `from` where `froms` take it, `to`
    /// where `to` takes it, and a junction `at` sets where `free` does.
    pub fn followed(&self, froms: &[Motion], to: Motion, free: Motion) -> Merge {
        let from_ends: Vec<End> = self
            .branches
            .iter()
            .zip(froms)
            .map(|(branch, &motion)| {
                let [end, _] = branch.moved_ends([motion, free]);
                end
            })
            .collect();
        let [_, to_end] = self.trunk.moved_ends([free, to]);
        let mut merge = Merge {
            at: self.at.map(|p| free.apply(p)),
            ..self.clone()
        };
        merge.join(&from_ends, &to_end);
        merge
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        for branch in &self.branches {
            branch.draw(out);
        }
        self.trunk.draw(out);
    }
}

/// The junction of a merge with no `at`: out of `to`'s side, or above
/// `to` when it names none, halfway to the nearest `from` along that way.
fn junction(froms: &[End], to: &End) -> Point {
    let up = Point::new(0.0, -1.0);
    let (base, out) = match to {
        End::Side { at, out } => (*at, *out),
        End::Box(s) => (Point::new(s.center().x, s.origin.y), up),
        End::Point(p) => (*p, up),
    };
    let nearest = froms
        .iter()
        .map(|from| {
            let d = from.on_edge(&End::Point(base)) - base;
            d.x * out.x + d.y * out.y
        })
        .fold(f64::INFINITY, f64::min);
    // with no `from` ahead of `to`, the junction steps out as far as an
    // elbow steps past its ends
    let reach = if nearest > 0.0 && nearest.is_finite() {
        nearest / 2.0
    } else {
        STEP_OUT
    };
    base + out * reach
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

/// The two inner control points of a sweep from `a` to `b` that sets
/// off along `out` and arrives along `arrive`, both unit vectors.
fn sweep_handles(a: Point, b: Point, out: Point, arrive: Point) -> (Point, Point) {
    let d = b - a;
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

fn unit(v: Point) -> Point {
    v * (1.0 / length(v))
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

/// The point a fraction `f` of the way along `path` by length, the
/// index of the segment it lies on, and that segment's unit direction.
fn point_at(path: &[Point], f: f64) -> (usize, Point, Point) {
    let total: f64 = path.windows(2).map(|w| length(w[1] - w[0])).sum();
    let mut left = f.clamp(0.0, 1.0) * total;
    for (i, w) in path.windows(2).enumerate() {
        let seg = length(w[1] - w[0]);
        if left <= seg || i == path.len() - 2 {
            let dir = (w[1] - w[0]) * (1.0 / seg);
            return (i, w[0] + dir * left.min(seg), dir);
        }
        left -= seg;
    }
    unreachable!("a path has two points")
}

/// The rest of `path`, which starts inside the box from `min` to
/// `max`, from where it first leaves the box, or nothing when it
/// never leaves.
fn leave_box<'a>(path: impl Iterator<Item = &'a Point>, min: Point, max: Point) -> Vec<Point> {
    let exit = |p: f64, d: f64, lo: f64, hi: f64| {
        if d > 0.0 {
            (hi - p) / d
        } else if d < 0.0 {
            (lo - p) / d
        } else {
            f64::INFINITY
        }
    };
    let mut rest = Vec::new();
    let mut prev: Option<Point> = None;
    for &p in path {
        match prev {
            Some(a) if rest.is_empty() => {
                let d = p - a;
                let t = exit(a.x, d.x, min.x, max.x).min(exit(a.y, d.y, min.y, max.y));
                if t < 1.0 {
                    rest.push(a + d * t);
                    rest.push(p);
                }
            }
            Some(_) => rest.push(p),
            None => {}
        }
        prev = Some(p);
    }
    rest
}

/// The parameters at which `cubic` enters and leaves the box from
/// `min` to `max` around the point of the curve nearest the box's
/// center, or none when that point lies outside the box.
fn cubic_gap(cubic: [Point; 4], min: Point, max: Point) -> Option<(f64, f64)> {
    const STEPS: usize = 64;
    let at = |t: f64| {
        let u = 1.0 - t;
        cubic[0] * (u * u * u)
            + cubic[1] * (3.0 * u * u * t)
            + cubic[2] * (3.0 * u * t * t)
            + cubic[3] * (t * t * t)
    };
    let inside = |t: f64| {
        let p = at(t);
        p.x > min.x && p.x < max.x && p.y > min.y && p.y < max.y
    };
    let center = (min + max) * 0.5;
    let middle = (0..=STEPS)
        .map(|i| i as f64 / STEPS as f64)
        .min_by(|a, b| length(at(*a) - center).total_cmp(&length(at(*b) - center)))?;
    if !inside(middle) {
        return None;
    }
    // step out each way to the first point outside, then halve the
    // last step 30 times to find where the curve crosses its edge
    let crossing = |step: f64| {
        let mut t = middle;
        while (0.0..=1.0).contains(&(t + step)) && inside(t + step) {
            t += step;
        }
        let (mut a, mut b) = (t, (t + step).clamp(0.0, 1.0));
        for _ in 0..30 {
            let m = (a + b) / 2.0;
            if inside(m) { a = m } else { b = m }
        }
        b
    };
    Some((crossing(-1.0 / STEPS as f64), crossing(1.0 / STEPS as f64)))
}

/// `cubic` split at parameter `t` into the curve before and the curve
/// after.
fn split(cubic: [Point; 4], t: f64) -> ([Point; 4], [Point; 4]) {
    let lerp = |a: Point, b: Point| a + (b - a) * t;
    let [p0, p1, p2, p3] = cubic;
    let (ab, bc, cd) = (lerp(p0, p1), lerp(p1, p2), lerp(p2, p3));
    let (abc, bcd) = (lerp(ab, bc), lerp(bc, cd));
    let mid = lerp(abc, bcd);
    ([p0, ab, abc, mid], [mid, bcd, cd, p3])
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

    fn sweep(from: (f64, f64), to: (f64, f64)) -> Connector {
        let mut c = connector(from, to);
        c.route = Route::Sweep {
            leave: Some(Dir::Down),
            enter: Some(Dir::Down),
        };
        c
    }

    #[test]
    fn a_sweep_draws_one_cubic_and_a_head_pointing_straight_down() {
        let mut out = Vec::new();
        sweep((0.0, 0.0), (1200.0, 190.0)).draw(&mut out);
        let [
            Primitive::Cubic { from, to, .. },
            Primitive::Polygon { points, .. },
        ] = &out[..]
        else {
            panic!("expected a cubic and a head, got {out:?}");
        };
        assert_eq!(*from, Point::new(0.0, 0.0));
        // the default head at stroke width 2 is 14 long, and its base
        // lies straight above the tip
        let tip = Point::new(1200.0, 190.0);
        assert_eq!(points[0], tip);
        assert_eq!(points[1].y, tip.y - 14.0);
        assert_eq!(points[2].y, tip.y - 14.0);
        assert_eq!(points[1].x + points[2].x, 2.0 * tip.x);
        assert_eq!(*to, Point::new(tip.x, tip.y - 14.0));
    }

    #[test]
    fn a_bent_sweep_meets_its_head_at_the_middle_of_its_back() {
        // short and wide, so the curve still bends within a head length
        // of its end
        let mut c = sweep((0.0, 0.0), (300.0, 30.0));
        c.tail = Head::Triangle;
        let mut out = Vec::new();
        c.draw(&mut out);
        let [
            Primitive::Cubic {
                from, c1, c2, to, ..
            },
            Primitive::Polygon { points: head, .. },
            Primitive::Polygon { points: tail, .. },
        ] = &out[..]
        else {
            panic!("expected a cubic and two heads, got {out:?}");
        };
        let back = |p: &[Point]| (p[1] + p[2]) * 0.5;
        assert_eq!(*to, back(head));
        assert_eq!(*from, back(tail));
        // the last handle lies on the head's axis and the first on the
        // tail's, both vertical here
        assert_eq!((c2.x, c1.x), (to.x, from.x));
    }

    #[test]
    fn a_straight_sweep_ends_its_curve_at_the_head_base() {
        let mut out = Vec::new();
        sweep((0.0, 0.0), (0.0, 100.0)).draw(&mut out);
        let Primitive::Cubic { to, .. } = &out[0] else {
            panic!("expected a cubic, got {:?}", out[0]);
        };
        assert!(length(*to - Point::new(0.0, 86.0)) < 1e-6, "{to:?}");
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

    /// The first and last point of each stretch of shaft drawn.
    fn shaft_ends(c: &Connector) -> Vec<(Point, Point)> {
        let mut out = Vec::new();
        c.draw(&mut out);
        out.iter()
            .filter_map(|p| match p {
                Primitive::Line { from, to, .. } | Primitive::Cubic { from, to, .. } => {
                    Some((*from, *to))
                }
                Primitive::Polyline { points, .. } => Some((points[0], points[points.len() - 1])),
                _ => None,
            })
            .collect()
    }

    /// The label's box padded by the gap the line breaks around it.
    fn break_box(c: &Connector) -> (Point, Point) {
        let (min, max) = label_box(c);
        let pad = Point::new(LABEL_GAP, LABEL_GAP);
        (min - pad, max + pad)
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn a_label_on_a_straight_line_breaks_it_around_the_label() {
        let c = labeled(connector((0.0, 100.0), (200.0, 100.0)), LabelSide::On);
        let (min, max) = break_box(&c);
        assert!(close((min.x + max.x) / 2.0, 100.0));
        assert!(close((min.y + max.y) / 2.0, 100.0));
        let ends = shaft_ends(&c);
        assert_eq!(ends.len(), 2);
        assert_eq!(ends[0].0, Point::new(0.0, 100.0));
        assert!(close(ends[0].1.x, min.x));
        assert!(close(ends[1].0.x, max.x));
        assert_eq!(ends[1].1, Point::new(200.0, 100.0));
    }

    #[test]
    fn a_label_on_an_elbow_breaks_the_segment_it_sits_on() {
        // the elbow runs (0, 0), (100, 0), (100, 100), (200, 100), so
        // halfway along is the middle of its upright segment
        let mut c = labeled(connector((0.0, 0.0), (200.0, 100.0)), LabelSide::On);
        c.route = Route::Elbow;
        c.head = Head::Triangle;
        let (min, max) = break_box(&c);
        let ends = shaft_ends(&c);
        assert_eq!(ends.len(), 2);
        assert_eq!(ends[0].0, Point::ZERO);
        assert!(close(ends[0].1.x, 100.0) && close(ends[0].1.y, min.y));
        assert!(close(ends[1].0.x, 100.0) && close(ends[1].0.y, max.y));
        let mut out = Vec::new();
        c.draw(&mut out);
        assert!(out.iter().any(|p| matches!(p,
            Primitive::Polygon { points, .. } if points[0] == Point::new(200.0, 100.0))));
    }

    #[test]
    fn a_label_on_a_sweep_breaks_its_curve_at_the_label_box() {
        let mut c = labeled(connector((0.0, 0.0), (200.0, 100.0)), LabelSide::On);
        c.route = Route::Sweep {
            leave: Some(Dir::Right),
            enter: Some(Dir::Right),
        };
        let (min, max) = break_box(&c);
        let on_edge = |p: Point| {
            let within = |v: f64, lo: f64, hi: f64| v > lo - 1e-6 && v < hi + 1e-6;
            within(p.x, min.x, max.x)
                && within(p.y, min.y, max.y)
                && [p.x - min.x, max.x - p.x, p.y - min.y, max.y - p.y]
                    .iter()
                    .any(|d| d.abs() < 1e-6)
        };
        let ends = shaft_ends(&c);
        assert_eq!(ends.len(), 2);
        assert_eq!(ends[0].0, Point::ZERO);
        assert!(on_edge(ends[0].1), "{:?}", ends[0].1);
        assert!(on_edge(ends[1].0), "{:?}", ends[1].0);
        assert!(ends[0].1.x < ends[1].0.x);
    }

    #[test]
    fn a_break_follows_the_line_when_its_ends_move() {
        let c = labeled(connector((0.0, 100.0), (200.0, 100.0)), LabelSide::On);
        let (min, _) = break_box(&c);
        let half = 100.0 - min.x;
        let doubled = Motion {
            pivot: Point::new(0.0, 100.0),
            scale: 2.0,
            offset: Point::ZERO,
        };
        let moved = c.followed([Motion::NONE, doubled]);
        let ends = shaft_ends(&moved);
        assert!(close(ends[0].1.x, 200.0 - half));
        assert!(close(ends[1].0.x, 200.0 + half));
    }
}

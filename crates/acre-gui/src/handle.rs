use acre::lang::{Program, Span};
use acre::scene::{Kind, Point, Scene, edit};

use crate::snap::{self, Bounds, Guide};

/// Half the side of a handle's square, in screen points.
pub const HANDLE: f32 = 4.0;

/// The smallest width or height, in canvas pixels, a resize leaves.
const MIN_SIDE: f64 = 1.0;

/// The one selected node that shows handles, with what they sit on in
/// canvas pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// A box, or an ellipse when `oval`, and the box it fills.
    Box {
        node: Span,
        bounds: Bounds,
        oval: bool,
    },
    /// A polygon, or a polyline when `open`, and its vertices.
    Path {
        node: Span,
        points: Vec<Point>,
        open: bool,
    },
    /// A line or an arrow, and where it starts and ends.
    Line { node: Span, ends: [Point; 2] },
}

/// Where along one axis a box's handle sits, and so which edge it drags.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edge {
    Start,
    Middle,
    End,
}

/// The handle a press lands on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grip {
    /// A handle on a box, by where it sits across and down.
    Box([Edge; 2]),
    /// The handle on a vertex, by its index.
    Vertex(usize),
    /// The handle on an end of a line, 0 at `from` and 1 at `to`.
    End(usize),
}

/// The change a drag of a handle makes.
#[derive(Debug, PartialEq)]
pub enum Reshape {
    /// The box of `node` goes from `from` to `to`.
    Resize {
        node: Span,
        from: Bounds,
        to: Bounds,
    },
    /// Vertex `index` of `node` moves by `by`.
    Vertex { node: Span, index: usize, by: Point },
    /// End `end` of the line `node`, 0 for `from` and 1 for `to`, is let
    /// go of at `to`, to go on the thing there or at that point.
    End { node: Span, end: usize, to: Point },
}

/// The node that shows handles when `selected` holds exactly one node
/// of `program`, a box, an ellipse, a polygon, a polyline, a line or an
/// arrow, drawn in `scene`.
pub fn target(program: &Program, scene: &Scene, selected: &[Span]) -> Option<Target> {
    let &[node] = selected else {
        return None;
    };
    let path = edit::node_path(program, node)?;
    let mut nodes = &program.nodes[..];
    let mut kind = "";
    for i in path {
        let n = nodes.get(i)?;
        kind = n.kind.text.as_str();
        nodes = &n.children;
    }
    let object = scene.objects.iter().find(|o| o.source == node && !o.tag)?;
    match (kind, &object.kind) {
        ("box" | "ellipse", Kind::Shape(s)) => Some(Target::Box {
            node,
            bounds: (s.origin, s.origin + s.size),
            oval: kind == "ellipse",
        }),
        ("polygon", Kind::Polygon(p)) => Some(Target::Path {
            node,
            points: p.points.clone(),
            open: false,
        }),
        ("polyline", Kind::Polyline(p)) => Some(Target::Path {
            node,
            points: p.points.clone(),
            open: true,
        }),
        ("line" | "arrow", Kind::Connector(c)) => Some(Target::Line {
            node,
            ends: [c.from, c.to],
        }),
        _ => None,
    }
}

impl Target {
    pub fn node(&self) -> Span {
        match self {
            Target::Box { node, .. } | Target::Path { node, .. } | Target::Line { node, .. } => {
                *node
            }
        }
    }

    /// Each handle and where it sits: eight on a box, at its corners and
    /// the middles of its sides, one on each vertex of a path, and one on
    /// each end of a line.
    pub fn handles(&self) -> Vec<(Grip, Point)> {
        match self {
            Target::Box { bounds, .. } => {
                let edges = [Edge::Start, Edge::Middle, Edge::End];
                edges
                    .into_iter()
                    .flat_map(|y| edges.map(|x| [x, y]))
                    .filter(|&sides| sides != [Edge::Middle; 2])
                    .map(|sides| (Grip::Box(sides), on_box(*bounds, sides)))
                    .collect()
            }
            Target::Path { points, .. } => points
                .iter()
                .enumerate()
                .map(|(i, &p)| (Grip::Vertex(i), p))
                .collect(),
            Target::Line { ends, .. } => vec![(Grip::End(0), ends[0]), (Grip::End(1), ends[1])],
        }
    }

    /// The handle within `reach` canvas pixels of `p` across and down,
    /// the nearest when several are.
    pub fn grip_at(&self, p: Point, reach: f64) -> Option<Grip> {
        self.handles()
            .into_iter()
            .map(|(grip, at)| (grip, (at.x - p.x).abs().max((at.y - p.y).abs())))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(grip, _)| grip)
    }

    /// Where the handle `grip` sits.
    pub fn grip_point(&self, grip: Grip) -> Option<Point> {
        match (self, grip) {
            (Target::Box { bounds, .. }, Grip::Box(sides)) => Some(on_box(*bounds, sides)),
            (Target::Path { points, .. }, Grip::Vertex(i)) => points.get(i).copied(),
            (Target::Line { ends, .. }, Grip::End(i)) => ends.get(i).copied(),
            _ => None,
        }
    }

    /// The change a drag of the handle `grip` by `by` canvas pixels
    /// makes, or `None` when the target has no such handle.
    pub fn reshape(&self, grip: Grip, by: Point) -> Option<Reshape> {
        match (self, grip) {
            (Target::Box { node, bounds, .. }, Grip::Box(sides)) => Some(Reshape::Resize {
                node: *node,
                from: *bounds,
                to: resized(*bounds, sides, on_box(*bounds, sides) + by),
            }),
            (Target::Path { node, points, .. }, Grip::Vertex(index)) if index < points.len() => {
                Some(Reshape::Vertex {
                    node: *node,
                    index,
                    by,
                })
            }
            (Target::Line { node, ends }, Grip::End(end)) if end < 2 => Some(Reshape::End {
                node: *node,
                end,
                to: ends[end] + by,
            }),
            _ => None,
        }
    }
}

/// Where the handle at `sides` sits on `b`.
fn on_box(b: Bounds, sides: [Edge; 2]) -> Point {
    let at = |lo: f64, hi: f64, edge: Edge| match edge {
        Edge::Start => lo,
        Edge::Middle => (lo + hi) / 2.0,
        Edge::End => hi,
    };
    Point::new(at(b.0.x, b.1.x, sides[0]), at(b.0.y, b.1.y, sides[1]))
}

/// `b` with the edges the handle at `sides` drags moved to `to`, each
/// stopped [`MIN_SIDE`] short of the edge across from it.
fn resized(b: Bounds, sides: [Edge; 2], to: Point) -> Bounds {
    let (mut lo, mut hi) = b;
    let axes = [(&mut lo.x, &mut hi.x, to.x), (&mut lo.y, &mut hi.y, to.y)];
    for ((lo, hi, to), edge) in axes.into_iter().zip(sides) {
        match edge {
            Edge::Start => *lo = to.min(*hi - MIN_SIDE),
            Edge::Middle => {}
            Edge::End => *hi = to.max(*lo + MIN_SIDE),
        }
    }
    (lo, hi)
}

/// Whether a drag of `grip` moves its handle across and down.
pub fn axes(grip: Grip) -> [bool; 2] {
    match grip {
        Grip::Box(sides) => sides.map(|e| e != Edge::Middle),
        Grip::Vertex(_) | Grip::End(_) => [true, true],
    }
}

/// Where a handle dragged to `p` goes: to whole pixels, then, when `on`,
/// to the edges and centres of the boxes in `others` and to the grid of
/// `grid` pixels as [`snap::snap`] snaps a point, with the guides that
/// show it. Only the axes `axes` holds snap.
pub fn snap_handle(
    p: Point,
    axes: [bool; 2],
    others: &[Bounds],
    reach: f64,
    grid: Option<f64>,
    on: bool,
) -> (Point, Vec<Guide>) {
    let p = Point::new(p.x.round(), p.y.round());
    if !on {
        return (p, Vec::new());
    }
    let snapped = snap::snap((p, p), others, Point::ZERO, reach, grid);
    let by = Point::new(
        if axes[0] { snapped.by.x } else { 0.0 },
        if axes[1] { snapped.by.y } else { 0.0 },
    );

    // a guide for a snap across is upright, and one for a snap down
    // lies flat
    let mut guides: Vec<Guide> = snapped
        .guides
        .into_iter()
        .filter(|g| axes[0] && g.from.x == g.to.x || axes[1] && g.from.y == g.to.y)
        .collect();

    // a point's left edge, centre and right edge are one line, and each
    // gets the same guide
    guides.dedup();
    (p + by, guides)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre::lang::parse;
    use acre::scene;

    fn b(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
        (Point::new(x0, y0), Point::new(x1, y1))
    }

    fn boxed(bounds: Bounds) -> Target {
        Target::Box {
            node: Span { start: 0, end: 1 },
            bounds,
            oval: false,
        }
    }

    /// The target of `src` with the node that starts with `node`
    /// selected.
    fn target_of(src: &str, node: &str) -> Option<Target> {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let start = src.find(node).unwrap();
        let mut nodes: Vec<&acre::lang::Node> = program.nodes.iter().collect();
        while let Some(n) = nodes.pop() {
            if n.span.start == start {
                return target(&program, &scene, &[n.span]);
            }
            nodes.extend(&n.children);
        }
        panic!("no node starts at `{node}`")
    }

    #[test]
    fn one_selected_box_ellipse_or_path_has_handles_and_others_none() {
        let src = "canvas { size 300 200; group { at 10 10; box b { at 5 5; size 20 10; } }
            ellipse e { at 100 100; size 30 30; } text t { at 0 150; text \"x\"; }
            polyline l { points 0 0 10 10 20 0; } }";
        let Some(Target::Box { bounds, oval, .. }) = target_of(src, "box b") else {
            panic!("the box has handles");
        };
        assert_eq!((bounds, oval), (b(15.0, 15.0, 35.0, 25.0), false));
        assert!(matches!(
            target_of(src, "ellipse e"),
            Some(Target::Box { oval: true, .. })
        ));
        assert!(matches!(
            target_of(src, "polyline l"),
            Some(Target::Path { open: true, ref points, .. }) if points.len() == 3
        ));
        assert_eq!(target_of(src, "text t"), None);
        assert_eq!(target_of(src, "group"), None);
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   arrow { from 100 50; to a right; } }";
        let Some(line @ Target::Line { ends, .. }) = target_of(src, "arrow") else {
            panic!("the arrow has handles");
        };
        assert_eq!(ends, [Point::new(100.0, 50.0), Point::new(30.0, 20.0)]);
        assert_eq!(
            line.grip_at(Point::new(31.0, 21.0), 3.0),
            Some(Grip::End(1))
        );
        assert!(matches!(
            line.reshape(Grip::End(0), Point::new(5.0, -5.0)),
            Some(Reshape::End { end: 0, to, .. }) if to == Point::new(105.0, 45.0)
        ));
    }

    #[test]
    fn a_box_has_a_handle_at_each_corner_and_side() {
        let handles = boxed(b(0.0, 0.0, 20.0, 10.0)).handles();
        assert_eq!(handles.len(), 8);
        let at: Vec<Point> = handles.iter().map(|h| h.1).collect();
        assert!(at.contains(&Point::new(10.0, 0.0)));
        assert!(at.contains(&Point::new(20.0, 5.0)));
        assert!(!at.contains(&Point::new(10.0, 5.0)));
    }

    #[test]
    fn a_press_takes_the_nearest_handle_within_reach() {
        let t = boxed(b(0.0, 0.0, 20.0, 10.0));
        assert_eq!(
            t.grip_at(Point::new(21.0, 9.0), 3.0),
            Some(Grip::Box([Edge::End, Edge::End]))
        );
        assert_eq!(
            t.grip_at(Point::new(10.5, -2.0), 3.0),
            Some(Grip::Box([Edge::Middle, Edge::Start]))
        );
        assert_eq!(t.grip_at(Point::new(10.0, 5.0), 3.0), None);
        let path = Target::Path {
            node: Span { start: 0, end: 1 },
            points: vec![Point::new(0.0, 0.0), Point::new(4.0, 0.0)],
            open: true,
        };
        assert_eq!(
            path.grip_at(Point::new(3.0, 0.0), 3.0),
            Some(Grip::Vertex(1))
        );
    }

    #[test]
    fn a_side_handle_drags_one_edge_and_a_corner_two() {
        let t = boxed(b(10.0, 10.0, 50.0, 30.0));
        let to = |grip, by| match t.reshape(grip, by) {
            Some(Reshape::Resize { to, .. }) => to,
            other => panic!("{other:?}"),
        };
        let right = Grip::Box([Edge::End, Edge::Middle]);
        assert_eq!(to(right, Point::new(5.0, 7.0)), b(10.0, 10.0, 55.0, 30.0));
        let top_left = Grip::Box([Edge::Start, Edge::Start]);
        assert_eq!(
            to(top_left, Point::new(-5.0, 2.0)),
            b(5.0, 12.0, 50.0, 30.0)
        );
    }

    #[test]
    fn an_edge_dragged_past_the_other_stops_a_pixel_short() {
        let t = boxed(b(10.0, 10.0, 50.0, 30.0));
        let Some(Reshape::Resize { to, .. }) = t.reshape(
            Grip::Box([Edge::Start, Edge::End]),
            Point::new(100.0, -100.0),
        ) else {
            panic!("a box resizes");
        };
        assert_eq!(to, b(49.0, 10.0, 50.0, 11.0));
    }

    #[test]
    fn a_dragged_handle_snaps_only_along_the_axes_it_moves() {
        // the other box's left edge at 100 is 2 away across, and its top
        // at 50 is 1 away down
        let others = [b(100.0, 50.0, 140.0, 80.0)];
        let p = Point::new(98.2, 49.0);
        let (to, guides) = snap_handle(p, [true, false], &others, 6.0, None, true);
        assert_eq!(to, Point::new(100.0, 49.0));
        assert_eq!(guides.len(), 1);
        assert_eq!(guides[0].from.x, guides[0].to.x);
        let (to, _) = snap_handle(p, [true, true], &others, 6.0, None, true);
        assert_eq!(to, Point::new(100.0, 50.0));
        let (to, guides) = snap_handle(p, [true, true], &others, 6.0, None, false);
        assert_eq!((to, guides.len()), (Point::new(98.0, 49.0), 0));
        let (to, _) = snap_handle(
            Point::new(23.0, 0.0),
            [true, true],
            &[],
            6.0,
            Some(10.0),
            true,
        );
        assert_eq!(to, Point::new(20.0, 0.0));
    }

    #[test]
    fn a_snapped_edge_lands_on_the_other_box_once_written() {
        // the other box's centre is at 42.5, half a pixel from where the
        // right edge is dragged to
        let src = "canvas { size 300 200; box a { at 0 0; size 20 10; } \
                   box b { at 34 50; size 17 20; } }";
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let a = scene.objects[0].source;
        let t = target(&program, &scene, &[a]).unwrap();
        let grip = Grip::Box([Edge::End, Edge::Middle]);
        let at = t.grip_point(grip).unwrap();
        let others = [scene.bounds(1, 0.0).unwrap()];
        let (to, _) = snap_handle(
            at + Point::new(21.7, 3.0),
            axes(grip),
            &others,
            6.0,
            None,
            true,
        );
        let Some(Reshape::Resize { node, from, to }) = t.reshape(grip, to - at) else {
            panic!("a box resizes");
        };
        let out = edit::apply(src, &edit::resize_edits(src, node, from, to).unwrap());
        assert!(out.contains("box a { at 0 0; size 42.5 10; }"), "{out}");
    }
}

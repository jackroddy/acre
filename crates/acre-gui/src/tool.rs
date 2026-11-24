use acre::lang::{Node, Program, Span};
use acre::scene::edit::{self, EndAt, NewNode, TextEdit};
use acre::scene::{Point, Scene};

/// The size in pixels of a box or an ellipse placed by a click.
pub const CLICKED_SIZE: Point = Point::new(80.0, 48.0);

/// How far, in screen points, a click on a path being drawn reaches to
/// land on a point already placed.
pub const CLOSE: f32 = 6.0;

/// The kinds of node a line or an arrow can end on by name.
const ATTACHABLE: [&str; 7] = [
    "box", "ellipse", "polygon", "polyline", "row", "matrix", "code",
];

/// What a press in the render pane does: select and move, or add a node
/// of a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Box,
    Ellipse,
    Text,
    Line,
    Arrow,
    Polygon,
    Polyline,
}

impl Tool {
    pub const ALL: [Tool; 8] = [
        Tool::Select,
        Tool::Box,
        Tool::Ellipse,
        Tool::Text,
        Tool::Line,
        Tool::Arrow,
        Tool::Polygon,
        Tool::Polyline,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Box => "Box",
            Tool::Ellipse => "Ellipse",
            Tool::Text => "Text",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Polygon => "Polygon",
            Tool::Polyline => "Polyline",
        }
    }

    /// How to use the tool.
    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "click to select, drag to move or to select in a box",
            Tool::Box | Tool::Ellipse => "drag corner to corner, or click for one 80 by 48",
            Tool::Text => "click to place a text and edit it",
            Tool::Line | Tool::Arrow => {
                "drag from one end to the other; an end on a shape joins it"
            }
            Tool::Polygon => "click each point, and click the first point again to close",
            Tool::Polyline => "click each point, and click the last point again to end",
        }
    }

    /// Whether the tool draws a path a click at a time.
    pub fn draws_path(self) -> bool {
        matches!(self, Tool::Polygon | Tool::Polyline)
    }
}

/// What a click does to the points of a polygon or a polyline being
/// drawn.
#[derive(Debug, PartialEq)]
pub enum Step {
    Added,
    /// The click landed on the last point and adds nothing.
    Ignored,
    /// The click ends the path: on the first point of a polygon, or on
    /// the last point of a polyline.
    Done,
}

/// Take a click at `p` on the path through `points`, all in canvas
/// pixels, where a click lands on a point within `close` pixels of it.
/// `closed` is true for a polygon and false for a polyline.
pub fn click_path(points: &mut Vec<Point>, p: Point, close: f64, closed: bool) -> Step {
    let near = |q: &Point| (p.x - q.x).hypot(p.y - q.y) <= close;
    if closed && points.len() >= 3 && near(&points[0]) {
        return Step::Done;
    }
    if points.last().is_some_and(near) {
        return if !closed && points.len() >= 2 {
            Step::Done
        } else {
            Step::Ignored
        };
    }
    points.push(p);
    Step::Added
}

/// Whether `points` are enough for a polygon, when `closed`, or for a
/// polyline.
pub fn enough(points: &[Point], closed: bool) -> bool {
    points.len() >= if closed { 3 } else { 2 }
}

/// The top-left corner and size of the box between corners `from` and
/// `to`, or `None` when it has no width or no height.
pub fn corners(from: Point, to: Point) -> Option<(Point, Point)> {
    let at = Point::new(from.x.min(to.x), from.y.min(to.y));
    let size = Point::new((to.x - from.x).abs(), (to.y - from.y).abs());
    (size.x > 0.0 && size.y > 0.0).then_some((at, size))
}

/// The place [`edit::insert_edits`] gives a node it adds to `program`,
/// as [`edit::node_path`] gives it: after the canvas's last child before
/// its timeline.
pub fn added_path(program: &Program) -> Vec<usize> {
    let children = program.nodes.first().map_or(&[][..], |c| &c.children[..]);
    let index = children
        .iter()
        .position(|n| n.kind.text == "timeline")
        .unwrap_or(children.len());
    vec![0, index]
}

/// Where an end of a new line or arrow goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum End {
    /// On the edge of the node with this span.
    Node(Span),
    /// At a point in canvas pixels.
    Point(Point),
}

/// The end at canvas point `at` in `scene` at time `t`: the node that
/// drew the object there when an end can name it, and `snapped`, the
/// point `at` snaps to, when not.
pub fn end_at(program: &Program, scene: &Scene, at: Point, snapped: Point, t: f64) -> End {
    scene
        .object_at(at, t)
        .map(|i| scene.objects[i].source)
        .filter(|&s| attachable(program, s).is_some())
        .map_or(End::Point(snapped), End::Node)
}

/// The node whose span is `span` when a line can end on it by name: a
/// shape, row, matrix or code that no `repeat` draws copies of.
fn attachable(program: &Program, span: Span) -> Option<&Node> {
    let path = edit::node_path(program, span)?;
    let mut nodes = &program.nodes[..];
    let mut node = None;
    for i in path {
        let n = &nodes[i];
        if n.kind.text == "repeat" {
            return None;
        }
        nodes = &n.children;
        node = Some(n);
    }
    node.filter(|n| ATTACHABLE.contains(&n.kind.text.as_str()))
}

/// The edits to `source`, which parses as `program`, that add a line, or
/// an arrow when `arrow`, from `from` to `to`, and name each node an end
/// is on that has no name yet. A point end is in canvas pixels, and
/// `origin` is where the source's 0 0 falls on the canvas. The error is
/// the reason for the status line.
pub fn line_edits(
    source: &str,
    program: &Program,
    from: End,
    to: End,
    origin: Point,
    arrow: bool,
) -> Result<Vec<TextEdit>, String> {
    let what = if arrow { "arrow" } else { "line" };
    match (from, to) {
        (End::Node(a), End::Node(b)) if a == b => {
            return Err(format!("an {what} needs two different things to join"));
        }
        (End::Point(a), End::Point(b)) if a == b => {
            return Err(format!("an {what} needs two different ends"));
        }
        _ => {}
    }
    let fail = |e: acre::lang::Error| format!("cannot add the {what}: {}", e.message);
    let mut edits = Vec::new();
    let mut ends = Vec::new();
    for end in [from, to] {
        let end_at = match end {
            End::Point(p) => EndAt::Point(p - origin),
            End::Node(span) => {
                let node = attachable(program, span).ok_or("no node can end it there")?;
                match &node.name {
                    Some(name) => EndAt::Name(name.text.clone()),
                    None => {
                        // over the source with the names given so far,
                        // so two new names differ
                        let named = edit::apply(source, &edits);
                        let name = edit::fresh_name(&named, &node.kind.text).map_err(fail)?;
                        edits.extend(edit::name_edits(source, span, &name).map_err(fail)?);
                        EndAt::Name(name)
                    }
                }
            }
        };
        ends.push(end_at);
    }
    let [from, to]: [EndAt; 2] = ends.try_into().expect("two ends");
    let node = if arrow {
        NewNode::Arrow { from, to }
    } else {
        NewNode::Line { from, to }
    };
    let text = node.text().map_err(fail)?;
    edits.extend(edit::insert_edits(source, &text).map_err(fail)?);
    Ok(edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre::lang::parse;
    use acre::scene;

    const SRC: &str = "canvas {\n    size 300 200;\n    box { at 10 10; size 40 40; }\n    \
                       box b { at 200 100; size 40 40; }\n    \
                       text { at 100 150; text \"t\"; }\n    \
                       repeat i 0 2 { box c { at 100 10; size 20 20; } }\n}\n";

    fn ends(src: &str, from: Point, to: Point) -> (End, End) {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        (
            end_at(&program, &scene, from, from, 0.0),
            end_at(&program, &scene, to, to, 0.0),
        )
    }

    fn drawn(src: &str, from: End, to: End) -> Result<String, String> {
        let program = parse(src).unwrap();
        let edits = line_edits(src, &program, from, to, Point::ZERO, true)?;
        let out = edit::apply(src, &edits);
        scene::build(&parse(&out).unwrap(), &[], &[]).unwrap();
        Ok(out)
    }

    #[test]
    fn an_arrow_between_two_boxes_names_the_one_without_a_name() {
        let (from, to) = ends(SRC, Point::new(20.0, 20.0), Point::new(210.0, 110.0));
        assert!(matches!((from, to), (End::Node(_), End::Node(_))));
        let out = drawn(SRC, from, to).unwrap();
        assert!(out.contains("box box1 { at 10 10;"), "{out}");
        assert!(out.contains("arrow { from box1; to b; }"), "{out}");
    }

    #[test]
    fn two_unnamed_ends_get_two_names() {
        let src = "canvas { size 300 200; box { at 10 10; size 40 40; } \
                   box { at 200 100; size 40 40; } }";
        let (from, to) = ends(src, Point::new(20.0, 20.0), Point::new(210.0, 110.0));
        let out = drawn(src, from, to).unwrap();
        assert!(out.contains("arrow { from box1; to box2; }"), "{out}");
    }

    #[test]
    fn an_end_on_text_empty_canvas_or_a_repeat_copy_is_a_point() {
        let (from, to) = ends(SRC, Point::new(101.0, 155.0), Point::new(150.0, 60.0));
        assert_eq!(from, End::Point(Point::new(101.0, 155.0)));
        assert_eq!(to, End::Point(Point::new(150.0, 60.0)));
        let (copy, _) = ends(SRC, Point::new(105.0, 15.0), Point::ZERO);
        assert_eq!(copy, End::Point(Point::new(105.0, 15.0)));
    }

    #[test]
    fn a_point_end_is_written_in_source_coordinates() {
        let program = parse(SRC).unwrap();
        let from = End::Point(Point::new(60.0, 30.0));
        let to = End::Point(Point::new(90.0, 30.5));
        let edits = line_edits(SRC, &program, from, to, Point::new(10.0, 10.0), false).unwrap();
        let out = edit::apply(SRC, &edits);
        assert!(out.contains("line { from 50 20; to 80 20.5; }"), "{out}");
    }

    #[test]
    fn the_same_thing_at_both_ends_or_no_length_is_refused() {
        let (from, to) = ends(SRC, Point::new(20.0, 20.0), Point::new(30.0, 30.0));
        assert_eq!(from, to);
        assert!(drawn(SRC, from, to).is_err());
        let p = End::Point(Point::new(5.0, 5.0));
        assert!(drawn(SRC, p, p).is_err());
    }

    #[test]
    fn a_polygon_closes_on_its_first_point_and_a_polyline_on_its_last() {
        let mut points = vec![Point::ZERO, Point::new(50.0, 0.0)];

        // too few points to close yet
        assert_eq!(
            click_path(&mut points, Point::new(2.0, 2.0), 6.0, true),
            Step::Added
        );
        points[2] = Point::new(25.0, 40.0);
        assert_eq!(
            click_path(&mut points, Point::new(26.0, 41.0), 6.0, true),
            Step::Ignored
        );
        assert_eq!(
            click_path(&mut points, Point::new(-3.0, 1.0), 6.0, true),
            Step::Done
        );
        assert_eq!(points.len(), 3);

        let mut points = vec![Point::ZERO];
        assert_eq!(
            click_path(&mut points, Point::ZERO, 6.0, false),
            Step::Ignored
        );
        points.push(Point::new(40.0, 0.0));
        assert_eq!(
            click_path(&mut points, Point::new(4.0, 0.0), 6.0, false),
            Step::Added
        );
        assert_eq!(
            click_path(&mut points, Point::new(5.0, 1.0), 6.0, false),
            Step::Done
        );
        assert!(enough(&points, false) && enough(&points, true));
        assert!(!enough(&points[..2], true));
    }

    #[test]
    fn a_box_needs_a_width_and_a_height() {
        let (at, size) = corners(Point::new(50.0, 10.0), Point::new(20.0, 40.0)).unwrap();
        assert_eq!((at, size), (Point::new(20.0, 10.0), Point::new(30.0, 30.0)));
        assert_eq!(corners(Point::ZERO, Point::new(30.0, 0.0)), None);
    }

    #[test]
    fn a_node_added_goes_before_the_timeline() {
        let src = "canvas { size 100 100; box { size 10 10; } timeline { } }";
        let program = parse(src).unwrap();
        assert_eq!(added_path(&program), [0, 1]);
        let edits = edit::insert_edits(src, "text { text \"x\"; }").unwrap();
        let out = parse(&edit::apply(src, &edits)).unwrap();
        let span = edit::node_span(&out, &added_path(&program)).unwrap();
        assert_eq!(out.nodes[0].children[1].span, span);
        assert_eq!(out.nodes[0].children[1].kind.text, "text");
    }
}

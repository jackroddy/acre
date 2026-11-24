use acre::lang::Span;
use acre::scene::{Point, Scene};

use crate::snap::Bounds;

/// A way to line up or space out the selected nodes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Arrange {
    Left,
    Centre,
    Right,
    Top,
    Middle,
    Bottom,
    /// Equal gaps from left to right.
    Across,
    /// Equal gaps from top to bottom.
    Down,
}

impl Arrange {
    /// The ways that line up two or more nodes.
    pub const ALIGN: [Arrange; 6] = [
        Arrange::Left,
        Arrange::Centre,
        Arrange::Right,
        Arrange::Top,
        Arrange::Middle,
        Arrange::Bottom,
    ];

    /// The ways that space out three or more nodes.
    pub const SPACE: [Arrange; 2] = [Arrange::Across, Arrange::Down];

    pub fn label(self) -> &'static str {
        match self {
            Arrange::Left => "Left",
            Arrange::Centre => "Centre",
            Arrange::Right => "Right",
            Arrange::Top => "Top",
            Arrange::Middle => "Middle",
            Arrange::Bottom => "Bottom",
            Arrange::Across => "Space across",
            Arrange::Down => "Space down",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Arrange::Left => "line up the selected nodes' left edges",
            Arrange::Centre => "centre the selected nodes on one vertical line",
            Arrange::Right => "line up the selected nodes' right edges",
            Arrange::Top => "line up the selected nodes' top edges",
            Arrange::Middle => "centre the selected nodes on one horizontal line",
            Arrange::Bottom => "line up the selected nodes' bottom edges",
            Arrange::Across => {
                "make the gaps between the selected nodes equal from left to right, keeping the outer two in place"
            }
            Arrange::Down => {
                "make the gaps between the selected nodes equal from top to bottom, keeping the outer two in place"
            }
        }
    }

    /// The verb for the status line when the arrangement is refused.
    pub fn verb(self) -> &'static str {
        match self {
            Arrange::Across | Arrange::Down => "space them out",
            _ => "align them",
        }
    }
}

/// The offsets that arrange `boxes` as `how` says, one for each box,
/// rounded to two decimals.
///
/// An alignment moves each box onto the line of the box around them all:
/// its left edge for [`Arrange::Left`], its centre for
/// [`Arrange::Centre`], and so on. A spacing sorts the boxes by their
/// left or top edge, keeps the first and the last in place, and moves
/// the others so the gaps between neighbours are equal.
pub fn offsets(how: Arrange, boxes: &[Bounds]) -> Vec<Point> {
    let mut by = vec![[0.0; 2]; boxes.len()];
    match how {
        Arrange::Across => space(boxes, 0, &mut by),
        Arrange::Down => space(boxes, 1, &mut by),
        _ => {
            let (axis, f) = match how {
                Arrange::Left => (0, 0.0),
                Arrange::Centre => (0, 0.5),
                Arrange::Right => (0, 1.0),
                Arrange::Top => (1, 0.0),
                Arrange::Middle => (1, 0.5),
                _ => (1, 1.0),
            };
            let lo = boxes
                .iter()
                .map(|b| coord(b.0, axis))
                .fold(f64::INFINITY, f64::min);
            let hi = boxes
                .iter()
                .map(|b| coord(b.1, axis))
                .fold(f64::NEG_INFINITY, f64::max);
            let line = lo + f * (hi - lo);
            for (b, by) in boxes.iter().zip(&mut by) {
                let (min, max) = (coord(b.0, axis), coord(b.1, axis));
                by[axis] = line - (min + f * (max - min));
            }
        }
    }
    by.into_iter()
        .map(|[x, y]| Point::new(hundredths(x), hundredths(y)))
        .collect()
}

/// Set `by[i][axis]` for each box of `boxes` but the first and last
/// along `axis`, so the gaps between neighbours along it are equal.
fn space(boxes: &[Bounds], axis: usize, by: &mut [[f64; 2]]) {
    if boxes.len() < 3 {
        return;
    }
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|&i, &j| coord(boxes[i].0, axis).total_cmp(&coord(boxes[j].0, axis)));
    let size = |i: usize| coord(boxes[i].1, axis) - coord(boxes[i].0, axis);
    let (first, last) = (order[0], order[order.len() - 1]);

    // the gap that leaves the last box where it is:
    //   gap = (last max - first min - sum of sizes) / (n - 1)
    let total: f64 = order.iter().map(|&i| size(i)).sum();
    let gap = (coord(boxes[last].1, axis) - coord(boxes[first].0, axis) - total)
        / (boxes.len() - 1) as f64;
    let mut next = coord(boxes[first].1, axis) + gap;
    for &i in &order[1..order.len() - 1] {
        by[i][axis] = next - coord(boxes[i].0, axis);
        next += size(i) + gap;
    }
}

/// The box around the objects of `scene` at time `t` that the node at
/// `node` or nodes inside it drew, or `None` when it drew none.
pub fn bounds(scene: &Scene, node: Span, t: f64) -> Option<Bounds> {
    (0..scene.objects.len())
        .filter(|&i| {
            let s = scene.objects[i].source;
            node.start <= s.start && s.end <= node.end
        })
        .filter_map(|i| scene.bounds(i, t))
        .reduce(|(a, b), (min, max)| {
            (
                Point::new(a.x.min(min.x), a.y.min(min.y)),
                Point::new(b.x.max(max.x), b.y.max(max.y)),
            )
        })
}

/// The nodes of `selected`, sorted by start and longest first as a
/// [`Selection`](crate::selection::Selection) keeps them, without those
/// inside another.
pub fn outermost(selected: &[Span]) -> Vec<Span> {
    let mut outer: Vec<Span> = Vec::new();
    for &s in selected {
        if outer.last().is_none_or(|o| o.end <= s.start) {
            outer.push(s);
        }
    }
    outer
}

fn coord(p: Point, axis: usize) -> f64 {
    if axis == 0 { p.x } else { p.y }
}

fn hundredths(n: f64) -> f64 {
    (n * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
        (Point::new(x0, y0), Point::new(x1, y1))
    }

    fn p(x: f64, y: f64) -> Point {
        Point::new(x, y)
    }

    const BOXES: [Bounds; 3] = [
        (Point::new(10.0, 50.0), Point::new(30.0, 60.0)),
        (Point::new(40.0, 0.0), Point::new(100.0, 20.0)),
        (Point::new(0.0, 30.0), Point::new(10.0, 40.0)),
    ];

    #[test]
    fn an_alignment_moves_each_box_onto_the_line_of_the_box_around_them_all() {
        assert_eq!(
            offsets(Arrange::Left, &BOXES),
            [p(-10.0, 0.0), p(-40.0, 0.0), p(0.0, 0.0)]
        );
        assert_eq!(
            offsets(Arrange::Centre, &BOXES),
            [p(30.0, 0.0), p(-20.0, 0.0), p(45.0, 0.0)]
        );
        assert_eq!(
            offsets(Arrange::Right, &BOXES),
            [p(70.0, 0.0), p(0.0, 0.0), p(90.0, 0.0)]
        );
        assert_eq!(
            offsets(Arrange::Top, &BOXES),
            [p(0.0, -50.0), p(0.0, 0.0), p(0.0, -30.0)]
        );
        assert_eq!(
            offsets(Arrange::Middle, &BOXES),
            [p(0.0, -25.0), p(0.0, 20.0), p(0.0, -5.0)]
        );
        assert_eq!(
            offsets(Arrange::Bottom, &BOXES),
            [p(0.0, 0.0), p(0.0, 40.0), p(0.0, 20.0)]
        );
    }

    #[test]
    fn a_spacing_keeps_the_outer_boxes_and_makes_the_gaps_equal() {
        // across, in order 2, 0, 1: sizes 10, 20 and 60 in 0..100
        // leave 10 to share, a gap of 5, so box 0 starts at 15
        assert_eq!(
            offsets(Arrange::Across, &BOXES),
            [p(5.0, 0.0), p(0.0, 0.0), p(0.0, 0.0)]
        );

        // down, in order 1, 2, 0: sizes 20, 10 and 10 in 0..60 leave
        // 20, a gap of 10, so box 2 starts at 30, where it is
        assert_eq!(offsets(Arrange::Down, &BOXES), [p(0.0, 0.0); 3]);
        let four = [
            b(0.0, 0.0, 10.0, 10.0),
            b(12.0, 0.0, 22.0, 10.0),
            b(50.0, 0.0, 70.0, 10.0),
            b(70.0, 0.0, 80.0, 10.0),
        ];
        let by = offsets(Arrange::Across, &four);
        let moved: Vec<f64> = four.iter().zip(&by).map(|(b, by)| b.0.x + by.x).collect();
        assert_eq!(moved, [0.0, 20.0, 40.0, 70.0]);
    }

    #[test]
    fn an_offset_is_rounded_to_two_decimals_and_two_boxes_take_no_spacing() {
        let boxes = [b(0.0, 0.0, 10.0, 10.0), b(3.3333, 0.0, 6.0, 10.0)];
        assert_eq!(offsets(Arrange::Left, &boxes)[1], p(-3.33, 0.0));
        assert_eq!(offsets(Arrange::Across, &boxes), [p(0.0, 0.0); 2]);
    }

    #[test]
    fn a_spacing_written_to_the_source_leaves_equal_gaps_between_the_nodes() {
        use acre::lang::parse;
        use acre::scene::{build, edit};

        // b is placed after a, so its `at a 30 5` changes by the
        // difference between their offsets
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   box b { at a 30 5; size 40 20; } group g { at 200 50; \
                   box c { at 0 0; size 10 10; } box d { at 20 0; size 10 10; } } }";
        let boxes_of = |src: &str| {
            let program = parse(src).unwrap();
            let scene = build(&program, &[], &[]).unwrap();
            let nodes: Vec<Span> = program.nodes[0].children.iter().map(|n| n.span).collect();
            let boxes: Vec<Bounds> = nodes
                .iter()
                .map(|&n| bounds(&scene, n, 0.0).unwrap())
                .collect();
            (nodes, boxes)
        };
        let (nodes, boxes) = boxes_of(src);
        assert_eq!(boxes[2], b(200.0, 50.0, 230.0, 60.0));
        let moves: Vec<(Span, f64, f64)> = nodes
            .iter()
            .zip(offsets(Arrange::Across, &boxes))
            .map(|(&n, by)| (n, by.x, by.y))
            .collect();
        let out = edit::apply(src, &edit::arrange_edits(src, &moves).unwrap());
        let (_, after) = boxes_of(&out);
        assert_eq!(after[0], boxes[0]);
        assert_eq!(after[2], boxes[2]);

        // sizes 20, 40 and 30 in 10..230 leave 130, a gap of 65,
        // so b starts at 30 + 65
        assert_eq!(after[1], b(95.0, 15.0, 135.0, 35.0));
    }

    #[test]
    fn only_the_outermost_selected_nodes_are_arranged() {
        let s = |start, end| Span { start, end };
        assert_eq!(
            outermost(&[s(0, 50), s(10, 20), s(60, 70), s(60, 65)]),
            [s(0, 50), s(60, 70)]
        );
    }
}

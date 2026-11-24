use acre::scene::Point;
use acre::scene::edit::Layout;

use crate::snap::Bounds;

/// The place nearest canvas point `p` among children laid out by
/// `layout` whose boxes, in source order, are `boxes`: the index of the
/// child a dragged one goes before, `boxes.len()` for after the last, and
/// the ends of the line that marks it. A child that draws nothing has no
/// box and offers no place. `None` when no child has a box.
///
/// A stack's places are its gaps along its direction, and a line marks
/// one across the stack. A grid's places are the left and right sides of
/// each child, in reading order, and a line marks one along that side.
pub fn slot(layout: Layout, boxes: &[Option<Bounds>], p: Point) -> Option<(usize, [Point; 2])> {
    let drawn: Vec<(usize, Bounds)> = boxes
        .iter()
        .enumerate()
        .filter_map(|(k, b)| Some((k, (*b)?)))
        .collect();
    let &(last, (_, last_max)) = drawn.last()?;
    let mut places: Vec<(usize, [Point; 2])> = Vec::new();
    match layout {
        Layout::Down | Layout::Right => {
            let right = layout == Layout::Right;

            // a point `along` the stack's direction and `across` it
            let at = |along: f64, across: f64| {
                if right {
                    Point::new(along, across)
                } else {
                    Point::new(across, along)
                }
            };
            let along = |q: Point| if right { q.x } else { q.y };
            let across = |q: Point| if right { q.y } else { q.x };
            let from = drawn
                .iter()
                .map(|(_, b)| across(b.0))
                .fold(f64::MAX, f64::min);
            let to = drawn
                .iter()
                .map(|(_, b)| across(b.1))
                .fold(f64::MIN, f64::max);
            let mut end_before: Option<f64> = None;
            for &(k, (min, max)) in &drawn {
                let gap = end_before.map_or(along(min), |e| (e + along(min)) / 2.0);
                places.push((k, [at(gap, from), at(gap, to)]));
                end_before = Some(along(max));
            }
            let end = along(last_max);
            places.push((last + 1, [at(end, from), at(end, to)]));
            let distance = |line: &[Point; 2]| (along(p) - along(line[0])).abs();
            places
                .into_iter()
                .min_by(|a, b| distance(&a.1).total_cmp(&distance(&b.1)))
        }
        Layout::Grid { .. } => {
            for &(k, (min, max)) in &drawn {
                places.push((k, [min, Point::new(min.x, max.y)]));
                places.push((k + 1, [Point::new(max.x, min.y), max]));
            }
            let distance = |line: &[Point; 2]| {
                let mid = Point::new(line[0].x, (line[0].y + line[1].y) / 2.0);
                (p.x - mid.x).hypot(p.y - mid.y)
            };
            places
                .into_iter()
                .min_by(|a, b| distance(&a.1).total_cmp(&distance(&b.1)))
        }
    }
}

/// The index among its siblings of a child moved from index `from` to
/// before the child at index `to`, or after the last when `to` is their
/// count.
pub fn moved_index(from: usize, to: usize) -> usize {
    if to > from { to - 1 } else { to }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(x0: f64, y0: f64, x1: f64, y1: f64) -> Option<Bounds> {
        Some((Point::new(x0, y0), Point::new(x1, y1)))
    }

    const P: fn(f64, f64) -> Point = Point::new;

    /// Three boxes down a stack, 10 apart: 0 to 20, 30 to 40, 50 to 80.
    fn down() -> Vec<Option<Bounds>> {
        vec![
            b(0.0, 0.0, 40.0, 20.0),
            b(10.0, 30.0, 30.0, 40.0),
            b(0.0, 50.0, 20.0, 80.0),
        ]
    }

    #[test]
    fn a_stack_offers_the_gap_nearest_the_pointer_along_it() {
        let boxes = down();
        let line = |y: f64| [P(0.0, y), P(40.0, y)];
        assert_eq!(
            slot(Layout::Down, &boxes, P(20.0, -5.0)),
            Some((0, line(0.0)))
        );
        assert_eq!(
            slot(Layout::Down, &boxes, P(99.0, 24.0)),
            Some((1, line(25.0)))
        );
        assert_eq!(
            slot(Layout::Down, &boxes, P(20.0, 46.0)),
            Some((2, line(45.0)))
        );
        assert_eq!(
            slot(Layout::Down, &boxes, P(20.0, 70.0)),
            Some((3, line(80.0)))
        );
    }

    #[test]
    fn a_stack_to_the_right_offers_its_gaps_across() {
        let boxes: Vec<Option<Bounds>> = down()
            .into_iter()
            .map(|b| b.map(|(min, max)| (P(min.y, min.x), P(max.y, max.x))))
            .collect();
        let line = |x: f64| [P(x, 0.0), P(x, 40.0)];
        assert_eq!(
            slot(Layout::Right, &boxes, P(24.0, 99.0)),
            Some((1, line(25.0)))
        );
        assert_eq!(
            slot(Layout::Right, &boxes, P(200.0, 0.0)),
            Some((3, line(80.0)))
        );
    }

    #[test]
    fn a_child_that_draws_nothing_offers_no_place() {
        let mut boxes = down();
        boxes[1] = None;
        assert_eq!(
            slot(Layout::Down, &boxes, P(0.0, 34.0)),
            Some((2, [P(0.0, 35.0), P(40.0, 35.0)]))
        );
        assert_eq!(slot(Layout::Down, &[None, None], P(0.0, 0.0)), None);
    }

    #[test]
    fn a_grid_offers_the_side_of_a_cell_nearest_the_pointer() {
        // two columns of 10 by 10 cells, 5 apart
        let boxes = vec![
            b(0.0, 0.0, 10.0, 10.0),
            b(15.0, 0.0, 25.0, 10.0),
            b(0.0, 15.0, 10.0, 25.0),
        ];
        let grid = Layout::Grid { columns: 2 };
        assert_eq!(
            slot(grid, &boxes, P(1.0, 5.0)),
            Some((0, [P(0.0, 0.0), P(0.0, 10.0)]))
        );
        assert_eq!(
            slot(grid, &boxes, P(30.0, 6.0)),
            Some((2, [P(25.0, 0.0), P(25.0, 10.0)]))
        );
        assert_eq!(
            slot(grid, &boxes, P(11.0, 22.0)),
            Some((3, [P(10.0, 15.0), P(10.0, 25.0)]))
        );
    }

    #[test]
    fn a_moved_child_lands_one_before_a_later_place() {
        assert_eq!(moved_index(0, 2), 1);
        assert_eq!(moved_index(0, 3), 2);
        assert_eq!(moved_index(2, 0), 0);
    }
}

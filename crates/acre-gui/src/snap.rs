use acre::scene::Point;

/// How far, in screen points, an edge or centre reaches to snap.
pub const REACH: f32 = 6.0;

/// The fewest screen points between two grid lines drawn.
const GRID_GAP: f32 = 8.0;

/// How close, in canvas pixels, two lines are to count as aligned once
/// an offset is rounded to two decimals.
const ALIGNED: f64 = 0.006;

/// A box in canvas pixels: its top-left and bottom-right corners.
pub type Bounds = (Point, Point);

/// A line that shows a snap, in canvas pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Guide {
    pub from: Point,
    pub to: Point,
}

/// Where a dragged box goes and the guides that show why.
#[derive(Debug, PartialEq)]
pub struct Snapped {
    /// The offset to move the box by: on the snap where one holds,
    /// rounded to two decimals, and to whole pixels where none does.
    pub by: Point,
    pub guides: Vec<Guide>,
}

/// Snap `moving`, dragged by `by`, so its left edge, centre or right edge
/// lines up with one of the boxes in `others`, and the same for its top,
/// middle and bottom, each axis on its own. An axis no box snaps snaps
/// the box's top-left corner to the grid of `grid` pixels when there is
/// one. A snap reaches `reach` canvas pixels.
pub fn snap(
    moving: Bounds,
    others: &[Bounds],
    by: Point,
    reach: f64,
    grid: Option<f64>,
) -> Snapped {
    let mut offset = [by.x, by.y];

    // the line each axis snapped to the grid at, or `None` when the
    // axis snapped to a box or did not snap
    let mut on_grid = [None; 2];
    let mut on_box = [false; 2];
    for axis in 0..2 {
        let own = lines(moving, axis);
        let near = others
            .iter()
            .flat_map(|&b| lines(b, axis))
            .flat_map(|to| own.map(|from| to - from))
            .map(|snap| (snap, (snap - offset[axis]).abs()))
            .filter(|&(_, d)| d <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((snap, _)) = near {
            offset[axis] = hundredths(snap);
            on_box[axis] = true;
            continue;
        }
        let corner = own[0] + offset[axis];
        on_grid[axis] = grid
            .map(|g| (corner / g).round() * g)
            .filter(|line| (line - corner).abs() <= reach);
        offset[axis] = match on_grid[axis] {
            Some(line) => hundredths(line - own[0]),
            None => offset[axis].round(),
        };
    }
    let mut guides = Vec::new();
    for axis in 0..2 {
        if on_box[axis] {
            guides.extend(aligned(moving, others, offset, axis));
        }
        if let Some(line) = on_grid[axis] {
            guides.push(guide(axis, line, cross(moved(moving, offset), axis)));
        }
    }
    Snapped {
        by: Point::new(offset[0], offset[1]),
        guides,
    }
}

/// The spacing, in canvas pixels, of the grid lines drawn at `zoom`
/// screen points per canvas pixel: `spacing` doubled until the lines are
/// at least [`GRID_GAP`] points apart.
pub fn grid_step(spacing: f64, zoom: f32) -> f64 {
    let mut step = spacing;
    while step * (zoom as f64) < GRID_GAP as f64 {
        step *= 2.0;
    }
    step
}

/// The guides along `axis` for every line of `moving`, moved by `offset`,
/// that lines up with a line of a box in `others`, one per place, long
/// enough to reach every box lined up there.
fn aligned(moving: Bounds, others: &[Bounds], offset: [f64; 2], axis: usize) -> Vec<Guide> {
    let moved = moved(moving, offset);
    let mut guides = Vec::new();
    for at in lines(moved, axis) {
        let mut span = cross(moved, axis);
        let mut hits = false;
        for &b in others {
            if lines(b, axis).iter().any(|l| (l - at).abs() <= ALIGNED) {
                let [lo, hi] = cross(b, axis);
                span = [span[0].min(lo), span[1].max(hi)];
                hits = true;
            }
        }
        if hits {
            guides.push(guide(axis, at, span));
        }
    }
    guides
}

/// The left edge, centre and right edge of `b` for axis 0, and its top,
/// middle and bottom for axis 1.
fn lines(b: Bounds, axis: usize) -> [f64; 3] {
    let [lo, hi] = along(b, axis);
    [lo, (lo + hi) / 2.0, hi]
}

/// Where `b` starts and ends along `axis`.
fn along(b: Bounds, axis: usize) -> [f64; 2] {
    match axis {
        0 => [b.0.x, b.1.x],
        _ => [b.0.y, b.1.y],
    }
}

/// Where `b` starts and ends across `axis`.
fn cross(b: Bounds, axis: usize) -> [f64; 2] {
    along(b, 1 - axis)
}

fn moved(b: Bounds, offset: [f64; 2]) -> Bounds {
    let by = Point::new(offset[0], offset[1]);
    (b.0 + by, b.1 + by)
}

/// The guide at `at` along `axis`, from `span[0]` to `span[1]` across it.
fn guide(axis: usize, at: f64, span: [f64; 2]) -> Guide {
    match axis {
        0 => Guide {
            from: Point::new(at, span[0]),
            to: Point::new(at, span[1]),
        },
        _ => Guide {
            from: Point::new(span[0], at),
            to: Point::new(span[1], at),
        },
    }
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

    #[test]
    fn an_edge_within_reach_snaps_and_shows_a_guide() {
        // the moving box's left edge lands at 103, 3 from the other's
        // left edge at 100
        let moving = b(0.0, 0.0, 20.0, 10.0);
        let other = b(100.0, 50.0, 160.0, 70.0);
        let s = snap(moving, &[other], Point::new(103.0, 0.0), 6.0, None);
        assert_eq!(s.by, Point::new(100.0, 0.0));
        assert_eq!(
            s.guides,
            vec![Guide {
                from: Point::new(100.0, 0.0),
                to: Point::new(100.0, 70.0),
            }]
        );
    }

    #[test]
    fn past_the_reach_nothing_snaps_and_the_offset_is_whole() {
        let moving = b(0.0, 0.0, 20.0, 10.0);
        let other = b(100.0, 50.0, 160.0, 70.0);
        let s = snap(moving, &[other], Point::new(200.4, 30.6), 6.0, None);
        assert_eq!(s.by, Point::new(200.0, 31.0));
        assert!(s.guides.is_empty());
    }

    #[test]
    fn the_nearest_line_wins_and_each_axis_snaps_on_its_own() {
        // across, the centre (10 + 88.5) and the right edge (20 + 88.5)
        // are 1.5 from the other's left edge and centre, nearer than any
        // other pair; down, the bottom (10 + 48) is 1 from the middle
        let moving = b(0.0, 0.0, 20.0, 10.0);
        let other = b(100.0, 40.0, 120.0, 78.0);
        let s = snap(moving, &[other], Point::new(88.5, 48.0), 6.0, None);
        assert_eq!(s.by, Point::new(90.0, 49.0));
    }

    #[test]
    fn a_snap_to_a_fractional_edge_keeps_two_decimals() {
        let moving = b(0.0, 0.0, 20.0, 10.0);
        let other = b(33.333, 0.0, 50.0, 10.0);
        let s = snap(moving, &[other], Point::new(33.0, 0.0), 6.0, None);
        assert_eq!(s.by.x, 33.33);
    }

    #[test]
    fn every_line_lined_up_gets_a_guide_that_reaches_each_box() {
        // the same width as the box above and below it, so its left
        // edge, centre and right edge all line up with both
        let moving = b(0.0, 0.0, 20.0, 10.0);
        let above = b(50.0, 0.0, 70.0, 5.0);
        let below = b(50.0, 80.0, 70.0, 90.0);
        let s = snap(moving, &[above, below], Point::new(48.0, 40.0), 6.0, None);
        assert_eq!(s.by, Point::new(50.0, 40.0));
        let xs: Vec<f64> = s.guides.iter().map(|g| g.from.x).collect();
        assert_eq!(xs, vec![50.0, 60.0, 70.0]);
        for g in &s.guides {
            assert_eq!((g.from.y, g.to.y), (0.0, 90.0));
        }
    }

    #[test]
    fn the_grid_takes_the_corner_only_where_no_box_snaps() {
        let moving = b(3.0, 3.0, 23.0, 13.0);
        let other = b(100.0, 200.0, 130.0, 230.0);

        // across, the right edge (23 + 78 = 101) snaps to the other's
        // left edge; down, the top (3 + 36 = 39) goes to the grid at 40
        let s = snap(moving, &[other], Point::new(78.0, 36.0), 6.0, Some(10.0));
        assert_eq!(s.by, Point::new(77.0, 37.0));
        assert_eq!(s.guides.len(), 2);

        let s = snap(moving, &[], Point::new(78.0, 36.0), 6.0, None);
        assert_eq!(s.by, Point::new(78.0, 36.0));
    }

    #[test]
    fn grid_lines_thin_out_when_zoomed_far_out() {
        assert_eq!(grid_step(10.0, 1.0), 10.0);
        assert_eq!(grid_step(10.0, 0.5), 20.0);
        assert_eq!(grid_step(10.0, 0.1), 80.0);
    }
}

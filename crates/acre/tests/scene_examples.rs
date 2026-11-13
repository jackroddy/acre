//! The two animations the scene model was designed against, built by
//! hand: an array being sorted, and a line plot drawing itself.

use acre::fonts::Fonts;
use acre::scene::*;

fn canvas(duration: f64) -> Canvas {
    Canvas {
        width: 640,
        height: 360,
        fps: 30,
        duration,
        background: Color::WHITE,
    }
}

const HIGHLIGHT: Color = Color::rgb(1.0, 0.85, 0.4);

/// Swap the cells at columns `a` and `b` of a one-row grid between `t0`
/// and `t1`, highlighting them first.
fn swap(grid: &mut Grid, a: usize, b: usize, t0: f64, t1: f64) {
    let shift = grid.slot(0, b) - grid.slot(0, a);
    for (col, dir) in [(a, 1.0), (b, -1.0)] {
        let cell = grid.cell_mut(0, col);
        cell.fill = Track::keyframes(vec![
            Keyframe {
                time: t0,
                value: Color::WHITE,
                easing: Easing::Linear,
            },
            Keyframe {
                time: t0 + 0.2,
                value: HIGHLIGHT,
                easing: Easing::Linear,
            },
            Keyframe {
                time: t1,
                value: HIGHLIGHT,
                easing: Easing::Linear,
            },
            Keyframe {
                time: t1 + 0.2,
                value: Color::WHITE,
                easing: Easing::Linear,
            },
        ]);
        cell.offset = Track::keyframes(vec![
            Keyframe {
                time: t0 + 0.2,
                value: Point::ZERO,
                easing: Easing::Linear,
            },
            Keyframe {
                time: t1,
                value: shift * dir,
                easing: Easing::EaseInOut,
            },
        ]);
    }
}

fn array_swap() -> Scene {
    let mut grid = Grid::new(1, 5, ["5", "2", "4", "1", "3"]);
    grid.origin = Track::Constant(Point::new(200.0, 160.0));
    swap(&mut grid, 0, 3, 0.5, 1.5);
    Scene {
        canvas: canvas(2.0),
        objects: vec![Object::new(grid)],
        fonts: Fonts::default(),
    }
}

fn self_drawing_plot() -> Scene {
    let mut plot = Plot::new(
        Point::new(80.0, 40.0),
        Point::new(500.0, 260.0),
        Axis::new(0.0, 4.0),
        Axis::new(0.0, 16.0),
    );
    plot.x.ticks = (0..=4).map(|i| Tick::at(i as f64)).collect();
    plot.y.ticks = [0.0, 4.0, 8.0, 12.0, 16.0].map(Tick::at).to_vec();
    plot.x.label = Some("x".into());
    plot.y.label = Some("x squared".into());
    let points: Vec<Point> = (0..=4)
        .map(|i| Point::new(i as f64, (i * i) as f64))
        .collect();
    let mut line = Series::new(points.clone(), SeriesKind::Line { width: 2.0 });
    line.color = Track::Constant(Color::rgb(0.1, 0.4, 0.9));
    line.reveal = Track::from_to(0.5, 0.0, 2.5, 1.0);
    let mut dots = Series::new(points, SeriesKind::Scatter { radius: 4.0 });
    dots.color = Track::Constant(Color::rgb(0.1, 0.4, 0.9));
    dots.reveal = Track::from_to(0.5, 0.0, 2.5, 1.0);
    plot.series.push(line);
    plot.series.push(dots);
    let mut object = Object::new(plot);
    object.opacity = Track::from_to(0.0, 0.0, 0.5, 1.0);
    Scene {
        canvas: canvas(3.0),
        objects: vec![object],
        fonts: Fonts::default(),
    }
}

fn rect_origins(prims: &[Primitive]) -> Vec<Point> {
    prims
        .iter()
        .filter_map(|p| match p {
            Primitive::Rect { origin, .. } => Some(*origin),
            _ => None,
        })
        .collect()
}

#[test]
fn array_swap_moves_two_cells_past_each_other() {
    let scene = array_swap();
    assert_eq!(scene.canvas.frame_count(), 60);
    let before = rect_origins(&scene.draw(0.0));
    let after = rect_origins(&scene.draw(2.0));
    assert_eq!(before.len(), 5);
    assert_eq!(after[0], before[3]);
    assert_eq!(after[3], before[0]);
    assert_eq!(after[1], before[1]);

    let mid = rect_origins(&scene.draw(1.1));
    assert_eq!(mid[0], mid[3]);
    assert_eq!(mid[0], before[1] + (before[2] - before[1]) * 0.5);

    match &scene.draw(1.0)[0] {
        Primitive::Rect { fill, .. } => assert_eq!(fill.unwrap(), HIGHLIGHT),
        other => panic!("expected rect, got {other:?}"),
    }
}

#[test]
fn plot_fades_in_then_draws_its_line() {
    let scene = self_drawing_plot();
    assert_eq!(scene.canvas.frame_count(), 90);

    assert!(scene.draw(0.0).is_empty());

    let faded = scene.draw(0.25);
    match &faded[0] {
        Primitive::Line { stroke, .. } => assert_eq!(stroke.color.a, 0.5),
        other => panic!("expected axis line, got {other:?}"),
    }
    assert!(
        !faded
            .iter()
            .any(|p| matches!(p, Primitive::Polyline { .. }))
    );

    let full = scene.draw(3.0);
    let line = full
        .iter()
        .find_map(|p| match p {
            Primitive::Polyline { points, .. } => Some(points),
            _ => None,
        })
        .unwrap();
    assert_eq!(line.len(), 5);
    assert_eq!(line[0], Point::new(80.0, 300.0));
    assert_eq!(line[4], Point::new(580.0, 40.0));
    let dots = full
        .iter()
        .filter(|p| matches!(p, Primitive::Ellipse { .. }))
        .count();
    assert_eq!(dots, 5);
}

// set ACRE_FRAMES to a directory to keep the frames as
// PNGs, one subdirectory per example
#[test]
fn both_examples_render_every_frame() {
    let out = std::env::var_os("ACRE_FRAMES").map(std::path::PathBuf::from);
    for (name, scene) in [("array_swap", array_swap()), ("plot", self_drawing_plot())] {
        let count = scene.canvas.frame_count();
        let width = count.to_string().len().max(4);
        for frame in 0..count {
            let pixmap = acre::render::draw_frame(&scene, scene.canvas.time_of(frame), 1.0);
            assert_eq!(pixmap.width(), scene.canvas.width);
            if let Some(out) = &out {
                let dir = out.join(name);
                std::fs::create_dir_all(&dir).unwrap();
                pixmap
                    .save_png(dir.join(format!("{frame:0width$}.png")))
                    .unwrap();
            }
        }
    }
}

//! An animation as a function of time.
//!
//! A [`Scene`] is a canvas and the objects on it. Every property that
//! moves is a [`Track`], and sampling the scene at an instant lowers it
//! to a list of [`Primitive`]s in paint order. Nothing here touches a
//! file or a pixel.

pub mod grid;
pub mod plot;
pub mod primitive;
pub mod track;

pub use grid::{Cell, Grid};
pub use plot::{Axis, Plot, Range, Series, SeriesKind, Tick};
pub use primitive::{Color, HAlign, Point, Primitive, Stroke, VAlign};
pub use track::{Easing, Keyframe, Lerp, Track};

/// The output frame: its size in pixels, timing, and background.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// The length of the animation in seconds.
    pub duration: f64,
    pub background: Color,
}

impl Canvas {
    /// How many frames the animation renders to, and one for a still.
    pub fn frame_count(&self) -> usize {
        ((self.duration * self.fps as f64).ceil() as usize).max(1)
    }

    /// The time in seconds at which `frame` is sampled.
    pub fn time_of(&self, frame: usize) -> f64 {
        frame as f64 / self.fps as f64
    }
}

/// A whole animation.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub canvas: Canvas,
    /// The objects in paint order, first at the back.
    pub objects: Vec<Object>,
}

/// One thing on the canvas, with the properties every kind shares.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub kind: Kind,
    pub opacity: Track<f64>,
    /// When the object is on the canvas; absent means always.
    pub lifetime: Option<Lifetime>,
}

/// What an object is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Grid(Grid),
    Plot(Plot),
}

/// A half-open interval of seconds, `start..end`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Lifetime {
    pub start: f64,
    pub end: f64,
}

impl Lifetime {
    pub fn contains(&self, t: f64) -> bool {
        self.start <= t && t < self.end
    }
}

impl Object {
    /// An always-visible, fully opaque object.
    pub fn new(kind: impl Into<Kind>) -> Self {
        Object {
            kind: kind.into(),
            opacity: Track::Constant(1.0),
            lifetime: None,
        }
    }

    /// Append this object at time `t` to `out`.
    pub fn draw(&self, t: f64, out: &mut Vec<Primitive>) {
        if self.lifetime.is_some_and(|l| !l.contains(t)) {
            return;
        }
        let opacity = self.opacity.at(t);
        if opacity <= 0.0 {
            return;
        }
        let start = out.len();
        match &self.kind {
            Kind::Grid(grid) => grid.draw(t, out),
            Kind::Plot(plot) => plot.draw(t, out),
        }
        if opacity < 1.0 {
            for prim in &mut out[start..] {
                prim.fade(opacity);
            }
        }
    }
}

impl From<Grid> for Kind {
    fn from(grid: Grid) -> Self {
        Kind::Grid(grid)
    }
}

impl From<Plot> for Kind {
    fn from(plot: Plot) -> Self {
        Kind::Plot(plot)
    }
}

impl Scene {
    /// Everything to draw at time `t`, in paint order.
    pub fn draw(&self, t: f64) -> Vec<Primitive> {
        let mut out = Vec::new();
        for object in &self.objects {
            object.draw(t, &mut out);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canvas() -> Canvas {
        Canvas {
            width: 100,
            height: 100,
            fps: 10,
            duration: 1.25,
            background: Color::WHITE,
        }
    }

    #[test]
    fn frame_count_rounds_up_to_cover_the_duration() {
        let canvas = canvas();
        assert_eq!(canvas.frame_count(), 13);
        assert_eq!(canvas.time_of(0), 0.0);
        assert_eq!(canvas.time_of(5), 0.5);
    }

    #[test]
    fn a_still_renders_one_frame() {
        let canvas = Canvas {
            duration: 0.0,
            ..canvas()
        };
        assert_eq!(canvas.frame_count(), 1);
    }

    #[test]
    fn an_object_outside_its_lifetime_draws_nothing() {
        let mut object = Object::new(Grid::new(1, 1, ["x"]));
        object.lifetime = Some(Lifetime {
            start: 1.0,
            end: 2.0,
        });
        let scene = Scene {
            canvas: canvas(),
            objects: vec![object],
        };
        assert!(scene.draw(0.5).is_empty());
        assert_eq!(scene.draw(1.0).len(), 2);
        assert!(scene.draw(2.0).is_empty());
    }

    #[test]
    fn opacity_fades_every_color_of_the_object() {
        let mut object = Object::new(Grid::new(1, 1, ["x"]));
        object.opacity = Track::from_to(0.0, 0.0, 1.0, 1.0);
        let scene = Scene {
            canvas: canvas(),
            objects: vec![object],
        };
        assert!(scene.draw(0.0).is_empty());
        let prims = scene.draw(0.5);
        match &prims[0] {
            Primitive::Rect { fill, stroke, .. } => {
                assert_eq!(fill.unwrap().a, 0.5);
                assert_eq!(stroke.unwrap().color.a, 0.5);
            }
            other => panic!("expected rect, got {other:?}"),
        }
        match &prims[1] {
            Primitive::Text { color, .. } => assert_eq!(color.a, 0.5),
            other => panic!("expected text, got {other:?}"),
        }
    }
}

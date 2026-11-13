use std::ops::{Add, Mul, Sub};

/// A position or size in canvas pixels, origin top-left.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }
}

impl Add for Point {
    type Output = Point;
    fn add(self, rhs: Point) -> Point {
        Point::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Point {
    type Output = Point;
    fn sub(self, rhs: Point) -> Point {
        Point::new(self.x - rhs.x, self.y - rhs.y)
    }
}

impl Mul<f64> for Point {
    type Output = Point;
    fn mul(self, rhs: f64) -> Point {
        Point::new(self.x * rhs, self.y * rhs)
    }
}

/// An RGBA color with components in 0..=1, alpha not premultiplied.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }

    /// The same color with its alpha multiplied by `opacity`.
    pub fn faded(self, opacity: f64) -> Self {
        Color {
            a: self.a * opacity as f32,
            ..self
        }
    }
}

/// How a line or an outline is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f64,
    /// The lengths of a dash and of the gap after it, or `None` for a
    /// solid line.
    pub dash: Option<[f64; 2]>,
}

impl Stroke {
    pub const fn solid(color: Color, width: f64) -> Self {
        Stroke {
            color,
            width,
            dash: None,
        }
    }
}

/// A stretch of text in one font and one color.
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub text: String,
    pub font: Font,
    pub color: Color,
}

/// The typeface text is set in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Font {
    #[default]
    Mono,
    Sans,
    SansBold,
    SansItalic,
}

/// Where a text's origin sits on its box, horizontally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
}

/// Where a text's origin sits on its box, vertically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VAlign {
    Top,
    Middle,
    #[default]
    Baseline,
}

/// One thing the renderer knows how to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Rect {
        origin: Point,
        size: Point,
        /// The radius of the rounded corners.
        radius: f64,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Ellipse {
        center: Point,
        radius: Point,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Line {
        from: Point,
        to: Point,
        stroke: Stroke,
    },
    Polyline {
        points: Vec<Point>,
        stroke: Stroke,
    },
    /// A closed outline through `points`.
    Polygon {
        points: Vec<Point>,
        fill: Option<Color>,
        stroke: Option<Stroke>,
    },
    Text {
        origin: Point,
        /// One or more lines, split at `\n` in any run.
        runs: Vec<Run>,
        size: f64,
        /// The width lines wrap within, at spaces.
        width: Option<f64>,
        h_align: HAlign,
        v_align: VAlign,
    },
}

/// The box around `prims`, as its top-left and bottom-right corners, or
/// `None` when they cover nothing. Text covers its advances, from its
/// font's ascent to its descent.
pub fn bounds(prims: &[Primitive]) -> Option<(Point, Point)> {
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut add = |p: Point, pad: f64| {
        min = Point::new(min.x.min(p.x - pad), min.y.min(p.y - pad));
        max = Point::new(max.x.max(p.x + pad), max.y.max(p.y + pad));
    };
    for prim in prims {
        match prim {
            Primitive::Rect { origin, size, .. } => {
                add(*origin, 0.0);
                add(*origin + *size, 0.0);
            }
            Primitive::Ellipse { center, radius, .. } => {
                add(*center - *radius, 0.0);
                add(*center + *radius, 0.0);
            }
            Primitive::Line { from, to, stroke } => {
                add(*from, stroke.width / 2.0);
                add(*to, stroke.width / 2.0);
            }
            Primitive::Polyline { points, stroke } => {
                for p in points {
                    add(*p, stroke.width / 2.0);
                }
            }
            Primitive::Polygon { points, .. } => {
                for p in points {
                    add(*p, 0.0);
                }
            }
            Primitive::Text {
                origin,
                runs,
                size,
                width,
                h_align,
                v_align,
            } => {
                let (min, max) =
                    crate::text::layout(runs, *size, *width).extent(*origin, *h_align, *v_align);
                add(min, 0.0);
                add(max, 0.0);
            }
        }
    }
    (min.x <= max.x).then_some((min, max))
}

impl Primitive {
    /// Multiply every color's alpha by `opacity`.
    pub fn fade(&mut self, opacity: f64) {
        let fade_stroke = |s: &mut Option<Stroke>| {
            if let Some(s) = s {
                s.color = s.color.faded(opacity);
            }
        };
        match self {
            Primitive::Rect { fill, stroke, .. }
            | Primitive::Ellipse { fill, stroke, .. }
            | Primitive::Polygon { fill, stroke, .. } => {
                if let Some(c) = fill {
                    *c = c.faded(opacity);
                }
                fade_stroke(stroke);
            }
            Primitive::Line { stroke, .. } | Primitive::Polyline { stroke, .. } => {
                stroke.color = stroke.color.faded(opacity);
            }
            Primitive::Text { runs, .. } => {
                for run in runs {
                    run.color = run.color.faded(opacity);
                }
            }
        }
    }
}

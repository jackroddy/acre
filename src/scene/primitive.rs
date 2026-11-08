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
        text: String,
        size: f64,
        color: Color,
        h_align: HAlign,
        v_align: VAlign,
    },
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
            Primitive::Text { color, .. } => *color = color.faded(opacity),
        }
    }
}

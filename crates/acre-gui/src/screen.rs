//! Points and rectangles in the pixels of a pane, so the layout
//! arithmetic of the window needs no toolkit type.

use std::ops::{Add, Div, Mul, Sub};

/// A position, or an offset, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in pixels: its top left corner and its size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub min: Point,
    pub size: Point,
}

impl Point {
    pub const ZERO: Point = Point { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }

    pub fn distance(self, other: Point) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

impl Add for Point {
    type Output = Point;

    fn add(self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }
}

impl Sub for Point {
    type Output = Point;

    fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }
}

impl Mul<f32> for Point {
    type Output = Point;

    fn mul(self, by: f32) -> Point {
        Point::new(self.x * by, self.y * by)
    }
}

impl Div<f32> for Point {
    type Output = Point;

    fn div(self, by: f32) -> Point {
        Point::new(self.x / by, self.y / by)
    }
}

impl Rect {
    /// The rectangle with corners `a` and `b`, in either order.
    pub fn from_corners(a: Point, b: Point) -> Rect {
        let min = Point::new(a.x.min(b.x), a.y.min(b.y));
        let max = Point::new(a.x.max(b.x), a.y.max(b.y));
        Rect {
            min,
            size: max - min,
        }
    }

    pub fn max(&self) -> Point {
        self.min + self.size
    }

    /// The rectangle grown by `by` on every side.
    pub fn expand(&self, by: f32) -> Rect {
        Rect {
            min: self.min - Point::new(by, by),
            size: self.size + Point::new(2.0 * by, 2.0 * by),
        }
    }
}

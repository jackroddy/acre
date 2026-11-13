use super::primitive::{Color, Font, HAlign, Point, Primitive, Run, Stroke, VAlign};

/// Text in runs of font and color at one size, placed by whatever
/// holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub runs: Vec<Run>,
    pub size: f64,
}

/// A box or an ellipse filling the rectangle at `origin` of `size`, with
/// an optional label centered in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub origin: Point,
    pub size: Point,
    pub oval: bool,
    /// The radius of a box's rounded corners.
    pub radius: f64,
    pub fill: Option<Color>,
    pub stroke: Option<Stroke>,
    pub label: Option<Label>,
}

/// A straight line, with an optional arrowhead at `to` and an optional
/// label above its middle.
#[derive(Debug, Clone, PartialEq)]
pub struct Connector {
    pub from: Point,
    pub to: Point,
    pub stroke: Stroke,
    pub head: bool,
    pub label: Option<Label>,
}

/// Free-standing text, placed by its top edge and `align`.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub origin: Point,
    pub label: Label,
    pub align: HAlign,
}

/// Which way from the thing it marks a brace or a ruler's labels face.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    /// The unit vector pointing out of this side.
    fn outward(self) -> Point {
        match self {
            Side::Top => Point::new(0.0, -1.0),
            Side::Bottom => Point::new(0.0, 1.0),
            Side::Left => Point::new(-1.0, 0.0),
            Side::Right => Point::new(1.0, 0.0),
        }
    }
}

/// A curly or square bracket from `from` to `to`, opening toward the
/// thing it marks and pointing to `side`, with an optional label
/// beyond its tip.
#[derive(Debug, Clone, PartialEq)]
pub struct Brace {
    pub from: Point,
    pub to: Point,
    pub side: Side,
    pub depth: f64,
    pub square: bool,
    pub stroke: Stroke,
    pub label: Option<Label>,
}

/// A scale of `count` equal units from `origin`, with a tick at each
/// boundary and a number over every `every`th unit.
#[derive(Debug, Clone, PartialEq)]
pub struct Ruler {
    pub origin: Point,
    pub unit: f64,
    pub count: usize,
    /// The number over the first unit.
    pub first: f64,
    /// How much the number grows from one unit to the next.
    pub step: f64,
    pub every: usize,
    /// Where the numbers sit; the ticks point the other way.
    pub side: Side,
    pub stroke: Stroke,
    pub label: Label,
}

impl Shape {
    pub fn center(&self) -> Point {
        self.origin + self.size * 0.5
    }

    /// The point where a line from the center toward `target` crosses
    /// the outline, or the center when `target` is the center.
    pub fn edge_toward(&self, target: Point) -> Point {
        let c = self.center();
        let d = target - c;
        let (rx, ry) = (self.size.x / 2.0, self.size.y / 2.0);
        if d == Point::ZERO || rx <= 0.0 || ry <= 0.0 {
            return c;
        }
        let t = if self.oval {
            1.0 / ((d.x / rx).powi(2) + (d.y / ry).powi(2)).sqrt()
        } else {
            (rx / d.x.abs()).min(ry / d.y.abs())
        };
        c + d * t
    }

    pub fn draw(&self, out: &mut Vec<Primitive>) {
        if self.oval {
            out.push(Primitive::Ellipse {
                center: self.center(),
                radius: self.size * 0.5,
                fill: self.fill,
                stroke: self.stroke,
            });
        } else {
            out.push(Primitive::Rect {
                origin: self.origin,
                size: self.size,
                radius: self.radius,
                fill: self.fill,
                stroke: self.stroke,
            });
        }
        if let Some(label) = &self.label {
            out.push(label.at(self.center(), HAlign::Center, VAlign::Middle));
        }
    }
}

impl Connector {
    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let d = self.to - self.from;
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len == 0.0 {
            return;
        }
        let dir = d * (1.0 / len);
        let head_len = (8.0 + 3.0 * self.stroke.width).min(len);
        let shaft_end = if self.head {
            self.to - dir * head_len
        } else {
            self.to
        };
        out.push(Primitive::Line {
            from: self.from,
            to: shaft_end,
            stroke: self.stroke,
        });
        if self.head {
            let side = Point::new(-dir.y, dir.x) * (head_len * 0.45);
            out.push(Primitive::Polygon {
                points: vec![self.to, shaft_end + side, shaft_end - side],
                fill: Some(self.stroke.color),
                stroke: None,
            });
        }
        if let Some(label) = &self.label {
            let mid = (self.from + self.to) * 0.5;
            out.push(label.at(mid - Point::new(0.0, 6.0), HAlign::Center, VAlign::Baseline));
        }
    }
}

impl Brace {
    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let d = self.to - self.from;
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len == 0.0 {
            return;
        }
        let along = d * (1.0 / len);
        let n = self.side.outward();
        let mid = (self.from + self.to) * 0.5;
        let tip = if self.square {
            let h = self.depth;
            out.push(Primitive::Polyline {
                points: vec![self.from, self.from + n * h, self.to + n * h, self.to],
                stroke: self.stroke,
            });
            mid + n * h
        } else {
            // each half is a quarter turn out of the end, a straight
            // run, and a quarter turn into the tip:
            //
            //   from ╮________╭ tip ╮________╭ to
            //
            // r is half the depth, and at most a quarter of the length
            // so the turns never overlap
            let r = (self.depth / 2.0).min(len / 4.0);
            let mut points = Vec::new();
            quad(
                &mut points,
                self.from,
                self.from + n * r,
                self.from + along * r + n * r,
            );
            quad(
                &mut points,
                mid - along * r + n * r,
                mid + n * r,
                mid + n * (2.0 * r),
            );
            quad(
                &mut points,
                mid + n * (2.0 * r),
                mid + n * r,
                mid + along * r + n * r,
            );
            quad(
                &mut points,
                self.to - along * r + n * r,
                self.to + n * r,
                self.to,
            );
            out.push(Primitive::Polyline {
                points,
                stroke: self.stroke,
            });
            mid + n * (2.0 * r)
        };
        if let Some(label) = &self.label {
            let at = tip + n * 6.0;
            let (h, v) = match self.side {
                Side::Top => (HAlign::Center, VAlign::Baseline),
                Side::Bottom => (HAlign::Center, VAlign::Top),
                Side::Left => (HAlign::Right, VAlign::Middle),
                Side::Right => (HAlign::Left, VAlign::Middle),
            };
            out.push(label.at(at, h, v));
        }
    }
}

/// Append the points of the quadratic curve from `a` through control
/// `c` to `b`, `a` included.
fn quad(points: &mut Vec<Point>, a: Point, c: Point, b: Point) {
    const STEPS: usize = 12;
    for i in 0..=STEPS {
        let t = i as f64 / STEPS as f64;
        let u = 1.0 - t;
        points.push(a * (u * u) + c * (2.0 * u * t) + b * (t * t));
    }
}

impl Ruler {
    pub fn draw(&self, out: &mut Vec<Primitive>) {
        let end = self.origin + Point::new(self.unit * self.count as f64, 0.0);
        out.push(Primitive::Line {
            from: self.origin,
            to: end,
            stroke: self.stroke,
        });
        let out_dir = self.side.outward();
        for i in 0..=self.count {
            let x = self.origin.x + i as f64 * self.unit;
            let major = self.every > 0 && i % self.every == 0;
            let tick = if major { 5.0 } else { 3.0 };
            let base = Point::new(x, self.origin.y);
            out.push(Primitive::Line {
                from: base,
                to: base - out_dir * tick,
                stroke: self.stroke,
            });
        }
        let every = self.every.max(1);
        for i in (0..self.count).step_by(every) {
            let value = self.first + self.step * i as f64;
            let at = Point::new(self.origin.x + (i as f64 + 0.5) * self.unit, self.origin.y)
                + out_dir * 4.0;
            let v = match self.side {
                Side::Bottom => VAlign::Top,
                _ => VAlign::Baseline,
            };
            let label = self.label.with_text(format!("{value}"));
            out.push(label.at(at, HAlign::Center, v));
        }
    }
}

impl Text {
    pub fn draw(&self, out: &mut Vec<Primitive>) {
        out.push(self.label.at(self.origin, self.align, VAlign::Top));
    }
}

impl Label {
    /// Text in one font and color.
    pub fn plain(text: impl Into<String>, size: f64, font: Font, color: Color) -> Self {
        Label {
            runs: vec![Run {
                text: text.into(),
                font,
                color,
            }],
            size,
        }
    }

    /// The text of every run, in order.
    pub fn text(&self) -> String {
        self.runs.iter().map(|r| r.text.as_str()).collect()
    }

    /// `text` in the font and color of this label's first run.
    pub fn with_text(&self, text: impl Into<String>) -> Self {
        let (font, color) = self
            .runs
            .first()
            .map_or((Font::Mono, Color::BLACK), |r| (r.font, r.color));
        Label::plain(text, self.size, font, color)
    }

    pub fn set_color(&mut self, color: Color) {
        for run in &mut self.runs {
            run.color = color;
        }
    }

    pub(super) fn at(&self, origin: Point, h_align: HAlign, v_align: VAlign) -> Primitive {
        Primitive::Text {
            origin,
            runs: self.runs.clone(),
            size: self.size,
            h_align,
            v_align,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(oval: bool) -> Shape {
        Shape {
            origin: Point::new(0.0, 0.0),
            size: Point::new(100.0, 40.0),
            oval,
            radius: 0.0,
            fill: None,
            stroke: None,
            label: None,
        }
    }

    #[test]
    fn a_box_edge_is_where_the_line_to_the_target_leaves_it() {
        let b = shape(false);
        assert_eq!(
            b.edge_toward(Point::new(200.0, 20.0)),
            Point::new(100.0, 20.0)
        );
        assert_eq!(
            b.edge_toward(Point::new(50.0, -100.0)),
            Point::new(50.0, 0.0)
        );
        assert_eq!(
            b.edge_toward(Point::new(150.0, 70.0)),
            Point::new(90.0, 40.0)
        );
    }

    #[test]
    fn an_ellipse_edge_lies_on_the_ellipse() {
        let e = shape(true);
        let p = e.edge_toward(Point::new(130.0, 90.0)) - e.center();
        let on = (p.x / 50.0).powi(2) + (p.y / 20.0).powi(2);
        assert!((on - 1.0).abs() < 1e-9);
    }

    #[test]
    fn an_arrow_ends_its_shaft_where_the_head_begins() {
        let arrow = Connector {
            from: Point::new(0.0, 0.0),
            to: Point::new(100.0, 0.0),
            stroke: Stroke::solid(Color::BLACK, 2.0),
            head: true,
            label: None,
        };
        let mut out = Vec::new();
        arrow.draw(&mut out);
        match &out[..] {
            [
                Primitive::Line { to, .. },
                Primitive::Polygon { points, .. },
            ] => {
                assert_eq!(*to, Point::new(86.0, 0.0));
                assert_eq!(points[0], Point::new(100.0, 0.0));
            }
            other => panic!("expected a line and a head, got {other:?}"),
        }
    }
}

use super::primitive::{Color, HAlign, Point, Primitive, Stroke, VAlign};

/// Text with its size and color, placed by whatever holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct Label {
    pub text: String,
    pub size: f64,
    pub color: Color,
}

/// A box or an ellipse filling the rectangle at `origin` of `size`, with
/// an optional label centered in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub origin: Point,
    pub size: Point,
    pub oval: bool,
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

impl Text {
    pub fn draw(&self, out: &mut Vec<Primitive>) {
        out.push(self.label.at(self.origin, self.align, VAlign::Top));
    }
}

impl Label {
    fn at(&self, origin: Point, h_align: HAlign, v_align: VAlign) -> Primitive {
        Primitive::Text {
            origin,
            text: self.text.clone(),
            size: self.size,
            color: self.color,
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
            stroke: Stroke {
                color: Color::BLACK,
                width: 2.0,
            },
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

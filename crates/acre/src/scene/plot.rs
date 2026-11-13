use super::primitive::{Color, Font, HAlign, Point, Primitive, Run, Stroke, VAlign};
use super::track::{Lerp, Track};

/// The data interval an axis shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

impl Range {
    pub const fn new(min: f64, max: f64) -> Self {
        Range { min, max }
    }

    /// Where `v` falls in the range, 0 at `min` and 1 at `max`.
    pub fn unit(&self, v: f64) -> f64 {
        (v - self.min) / (self.max - self.min)
    }
}

impl Lerp for Range {
    fn lerp(a: &Self, b: &Self, u: f64) -> Self {
        Range {
            min: f64::lerp(&a.min, &b.min, u),
            max: f64::lerp(&a.max, &b.max, u),
        }
    }
}

/// A labelled position on an axis.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub value: f64,
    pub label: String,
}

impl Tick {
    /// A tick labelled with its value.
    pub fn at(value: f64) -> Self {
        Tick {
            value,
            label: value.to_string(),
        }
    }
}

/// One axis of a plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Axis {
    pub range: Track<Range>,
    pub ticks: Vec<Tick>,
    pub label: Option<String>,
}

impl Axis {
    /// An axis over `min..=max` with no ticks and no label.
    pub fn new(min: f64, max: f64) -> Self {
        Axis {
            range: Track::Constant(Range::new(min, max)),
            ticks: Vec::new(),
            label: None,
        }
    }
}

/// A 2D plot: two axes and the series drawn against them.
#[derive(Debug, Clone, PartialEq)]
pub struct Plot {
    /// The top-left corner of the plotting area.
    pub origin: Point,
    /// The width and height of the plotting area.
    pub size: Point,
    pub x: Axis,
    pub y: Axis,
    pub axis_stroke: Stroke,
    pub text_size: f64,
    pub text_color: Color,
    pub series: Vec<Series>,
}

/// One set of data points and how to draw them.
#[derive(Debug, Clone, PartialEq)]
pub struct Series {
    /// The points in data coordinates.
    pub points: Vec<Point>,
    pub kind: SeriesKind,
    pub color: Track<Color>,
    /// How much of the series is shown, 0 for none and 1 for all.
    pub reveal: Track<f64>,
}

/// How a series is drawn, and what its reveal fraction means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SeriesKind {
    /// A polyline through the points. Reveal draws it up to a fraction
    /// of the way along, counted in points.
    Line { width: f64 },
    /// A circle at each point. Reveal shows the first fraction of them.
    Scatter { radius: f64 },
    /// A bar from zero to each point. Reveal scales every bar's height.
    /// `width` is in data units along x.
    Bars { width: f64 },
}

impl Series {
    pub fn new(points: Vec<Point>, kind: SeriesKind) -> Self {
        Series {
            points,
            kind,
            color: Track::Constant(Color::BLACK),
            reveal: Track::Constant(1.0),
        }
    }
}

impl Plot {
    /// A plot filling `size` at `origin`, with axes but no series.
    pub fn new(origin: Point, size: Point, x: Axis, y: Axis) -> Self {
        Plot {
            origin,
            size,
            x,
            y,
            axis_stroke: Stroke::solid(Color::BLACK, 1.0),
            text_size: 14.0,
            text_color: Color::BLACK,
            series: Vec::new(),
        }
    }

    /// The canvas position of the data point `p` at time `t`.
    pub fn to_canvas(&self, p: Point, t: f64) -> Point {
        let (x, y) = (self.x.range.at(t), self.y.range.at(t));
        Point::new(
            self.origin.x + x.unit(p.x) * self.size.x,
            // y grows upwards in data and downwards on the canvas
            self.origin.y + (1.0 - y.unit(p.y)) * self.size.y,
        )
    }

    /// Append this plot at time `t` to `out`: axes, then every series in
    /// order.
    pub fn draw(&self, t: f64, out: &mut Vec<Primitive>) {
        self.draw_axes(t, out);
        for series in &self.series {
            self.draw_series(series, t, out);
        }
    }

    fn draw_axes(&self, t: f64, out: &mut Vec<Primitive>) {
        let tick_len = self.text_size * 0.4;
        let pad = self.text_size * 0.4;
        let bottom_left = self.origin + Point::new(0.0, self.size.y);
        let bottom_right = self.origin + self.size;
        out.push(Primitive::Line {
            from: bottom_left,
            to: bottom_right,
            stroke: self.axis_stroke,
        });
        out.push(Primitive::Line {
            from: self.origin,
            to: bottom_left,
            stroke: self.axis_stroke,
        });
        let (xr, yr) = (self.x.range.at(t), self.y.range.at(t));
        for tick in &self.x.ticks {
            let x = self.origin.x + xr.unit(tick.value) * self.size.x;
            let top = Point::new(x, bottom_left.y);
            out.push(Primitive::Line {
                from: top,
                to: top + Point::new(0.0, tick_len),
                stroke: self.axis_stroke,
            });
            out.push(self.text(
                top + Point::new(0.0, tick_len + pad),
                &tick.label,
                HAlign::Center,
                VAlign::Top,
            ));
        }
        for tick in &self.y.ticks {
            let y = self.origin.y + (1.0 - yr.unit(tick.value)) * self.size.y;
            let right = Point::new(self.origin.x, y);
            out.push(Primitive::Line {
                from: right - Point::new(tick_len, 0.0),
                to: right,
                stroke: self.axis_stroke,
            });
            out.push(self.text(
                right - Point::new(tick_len + pad, 0.0),
                &tick.label,
                HAlign::Right,
                VAlign::Middle,
            ));
        }
        if let Some(label) = &self.x.label {
            let below_ticks = tick_len + pad + self.text_size + pad;
            out.push(self.text(
                Point::new(
                    bottom_left.x + self.size.x / 2.0,
                    bottom_left.y + below_ticks,
                ),
                label,
                HAlign::Center,
                VAlign::Top,
            ));
        }
        if let Some(label) = &self.y.label {
            out.push(self.text(
                self.origin - Point::new(0.0, pad),
                label,
                HAlign::Left,
                VAlign::Baseline,
            ));
        }
    }

    fn text(&self, origin: Point, text: &str, h_align: HAlign, v_align: VAlign) -> Primitive {
        Primitive::Text {
            origin,
            runs: vec![Run {
                text: text.to_string(),
                font: Font::Mono,
                color: self.text_color,
                bullet: false,
            }],
            size: self.text_size,
            width: None,
            justify: false,
            h_align,
            v_align,
        }
    }

    fn draw_series(&self, series: &Series, t: f64, out: &mut Vec<Primitive>) {
        let n = series.points.len();
        if n == 0 {
            return;
        }
        let color = series.color.at(t);
        let reveal = series.reveal.at(t).clamp(0.0, 1.0);
        // the last point index shown, with a fraction into the next
        // segment
        let pos = reveal * (n - 1) as f64;
        let whole = pos.floor() as usize;
        let frac = pos - whole as f64;
        match series.kind {
            SeriesKind::Line { width } => {
                let mut points: Vec<Point> = series.points[..=whole]
                    .iter()
                    .map(|&p| self.to_canvas(p, t))
                    .collect();
                if frac > 0.0 {
                    let partial =
                        Point::lerp(&series.points[whole], &series.points[whole + 1], frac);
                    points.push(self.to_canvas(partial, t));
                }
                if points.len() >= 2 {
                    out.push(Primitive::Polyline {
                        points,
                        stroke: Stroke::solid(color, width),
                    });
                }
            }
            SeriesKind::Scatter { radius } => {
                for &p in &series.points[..=whole] {
                    out.push(Primitive::Ellipse {
                        center: self.to_canvas(p, t),
                        radius: Point::new(radius, radius),
                        fill: Some(color),
                        stroke: None,
                    });
                }
            }
            SeriesKind::Bars { width } => {
                for &p in &series.points {
                    let base = self.to_canvas(Point::new(p.x - width / 2.0, 0.0), t);
                    let top = self.to_canvas(Point::new(p.x + width / 2.0, p.y * reveal), t);
                    out.push(Primitive::Rect {
                        origin: Point::new(base.x, base.y.min(top.y)),
                        size: Point::new(top.x - base.x, (top.y - base.y).abs()),
                        radius: 0.0,
                        fill: Some(color),
                        stroke: None,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot() -> Plot {
        Plot::new(
            Point::new(100.0, 100.0),
            Point::new(200.0, 100.0),
            Axis::new(0.0, 10.0),
            Axis::new(0.0, 5.0),
        )
    }

    fn polylines(prims: &[Primitive]) -> Vec<&Vec<Point>> {
        prims
            .iter()
            .filter_map(|p| match p {
                Primitive::Polyline { points, .. } => Some(points),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn maps_data_to_canvas_with_y_flipped() {
        let plot = plot();
        assert_eq!(
            plot.to_canvas(Point::new(0.0, 0.0), 0.0),
            Point::new(100.0, 200.0)
        );
        assert_eq!(
            plot.to_canvas(Point::new(10.0, 5.0), 0.0),
            Point::new(300.0, 100.0)
        );
        assert_eq!(
            plot.to_canvas(Point::new(5.0, 2.5), 0.0),
            Point::new(200.0, 150.0)
        );
    }

    #[test]
    fn axes_are_two_lines_plus_a_line_and_a_label_per_tick() {
        let mut plot = plot();
        plot.x.ticks = vec![Tick::at(0.0), Tick::at(5.0), Tick::at(10.0)];
        plot.y.ticks = vec![Tick::at(5.0)];
        plot.x.label = Some("x".into());
        let mut out = Vec::new();
        plot.draw(0.0, &mut out);
        let lines = out
            .iter()
            .filter(|p| matches!(p, Primitive::Line { .. }))
            .count();
        let texts = out
            .iter()
            .filter(|p| matches!(p, Primitive::Text { .. }))
            .count();
        assert_eq!(lines, 2 + 4);
        assert_eq!(texts, 4 + 1);
    }

    #[test]
    fn a_line_reveals_point_by_point() {
        let mut plot = plot();
        let mut series = Series::new(
            vec![
                Point::new(0.0, 0.0),
                Point::new(5.0, 5.0),
                Point::new(10.0, 0.0),
            ],
            SeriesKind::Line { width: 2.0 },
        );
        series.reveal = Track::from_to(0.0, 0.0, 1.0, 1.0);
        plot.series.push(series);

        let mut out = Vec::new();
        plot.draw(0.0, &mut out);
        assert!(polylines(&out).is_empty());

        out.clear();
        plot.draw(0.25, &mut out);
        let line = polylines(&out)[0];
        assert_eq!(line.len(), 2);
        assert_eq!(line[1], Point::new(150.0, 150.0));

        out.clear();
        plot.draw(0.5, &mut out);
        assert_eq!(polylines(&out)[0].len(), 2);

        out.clear();
        plot.draw(1.0, &mut out);
        let line = polylines(&out)[0];
        assert_eq!(line.len(), 3);
        assert_eq!(line[2], Point::new(300.0, 200.0));
    }

    #[test]
    fn scatter_shows_the_first_points() {
        let mut plot = plot();
        let mut series = Series::new(
            vec![
                Point::new(1.0, 1.0),
                Point::new(2.0, 2.0),
                Point::new(3.0, 3.0),
            ],
            SeriesKind::Scatter { radius: 3.0 },
        );
        series.reveal = Track::Constant(0.5);
        plot.series.push(series);
        let mut out = Vec::new();
        plot.draw(0.0, &mut out);
        let dots = out
            .iter()
            .filter(|p| matches!(p, Primitive::Ellipse { .. }))
            .count();
        assert_eq!(dots, 2);
    }

    #[test]
    fn bars_grow_from_zero() {
        let mut plot = plot();
        let mut series = Series::new(vec![Point::new(5.0, 4.0)], SeriesKind::Bars { width: 2.0 });
        series.reveal = Track::Constant(0.5);
        plot.series.push(series);
        let mut out = Vec::new();
        plot.draw(0.0, &mut out);
        let bar = out
            .iter()
            .find_map(|p| match p {
                Primitive::Rect { origin, size, .. } => Some((*origin, *size)),
                _ => None,
            })
            .unwrap();
        assert_eq!(bar.0, Point::new(180.0, 160.0));
        assert_eq!(bar.1, Point::new(40.0, 40.0));
    }
}

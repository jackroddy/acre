//! An animation as a function of time.
//!
//! A [`Scene`] is a canvas and the objects on it. Every property that
//! moves is a [`Track`], and sampling the scene at an instant lowers it
//! to a list of [`Primitive`]s in paint order. Nothing here touches a
//! file or a pixel.

mod build;
pub mod code;
mod connector;
mod data;
pub mod diagram;
mod expand;
pub mod grid;
mod markup;
pub mod matrix;
pub mod plot;
pub mod primitive;
pub mod row;
pub mod schema;
mod timeline;
pub mod track;

pub use build::{FontFile, build, font_files};
pub use code::Code;
pub use connector::{Connector, End, Follow, Head, LabelSide, Route};
pub use data::{DataFile, data_files};
pub use diagram::{Brace, Label, Polygon, Polyline, Ruler, Shape, Side, Text};
pub use grid::{Cell, Grid};
pub use matrix::{Matrix, MatrixCell};
pub use plot::{Axis, Plot, Range, Series, SeriesKind, Tick};
pub use primitive::{Color, Font, HAlign, Motion, Point, Primitive, Run, Stroke, VAlign};
pub use row::{Row, RowCell};
pub use track::{Easing, Keyframe, Lerp, Track};

use std::borrow::Cow;

use crate::fonts::Fonts;
use crate::lang::Span;

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
    /// The fonts every text in the scene is set in.
    pub fonts: Fonts,
    /// The labelled parts of the animation, in order of start.
    pub sections: Vec<Section>,
}

/// A labelled part of an animation, which runs until the next one starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// When the part starts, in seconds.
    pub start: f64,
    pub title: String,
    /// The labelled parts of this one, in order of start.
    pub subsections: Vec<Subsection>,
}

/// A labelled part of a section, which runs until the next subsection or
/// section starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Subsection {
    /// When the part starts, in seconds.
    pub start: f64,
    pub title: String,
}

/// One thing on the canvas, with the properties every kind shares.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub kind: Kind,
    pub opacity: Track<f64>,
    /// The size of the drawing relative to the size layout gave it.
    pub scale: Track<f64>,
    /// The point the object scales about.
    pub pivot: Point,
    /// How far the drawing is from where layout put it.
    pub offset: Track<Point>,
    /// Changes to the object's colors, applied in order.
    pub paints: Vec<Paint>,
    /// When the object is on the canvas; absent means always.
    pub lifetime: Option<Lifetime>,
    /// The node in the source that made the object, or an empty span
    /// for an object made in code.
    pub source: Span,
    /// The animated groups the object sits in, innermost first.
    pub groups: Vec<Animation>,
    /// The cells of a row that move, scale or fade on their own, by
    /// index.
    pub cells: Vec<(usize, Animation)>,
}

/// How a group around objects, or one cell of a row, moves, scales and
/// fades over time.
#[derive(Debug, Clone, PartialEq)]
pub struct Animation {
    pub opacity: Track<f64>,
    pub scale: Track<f64>,
    /// The point it scales about.
    pub pivot: Point,
    pub offset: Track<Point>,
}

impl Animation {
    /// An animation that changes nothing, scaling about `pivot`.
    pub fn new(pivot: Point) -> Self {
        Animation {
            opacity: Track::Constant(1.0),
            scale: Track::Constant(1.0),
            pivot,
            offset: Track::Constant(Point::ZERO),
        }
    }

    pub fn motion(&self, t: f64) -> Motion {
        Motion {
            pivot: self.pivot,
            scale: self.scale.at(t),
            offset: self.offset.at(t),
        }
    }
}

/// A thing in the scene another follows: the object made by a node, or
/// one cell of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    /// The node that made the object.
    pub source: Span,
    pub cell: Option<usize>,
}

/// What an object is.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Grid(Grid),
    Plot(Plot),
    Shape(Shape),
    Polygon(Polygon),
    Polyline(Polyline),
    Connector(Connector),
    Text(Text),
    Row(Row),
    Matrix(Matrix),
    Brace(Brace),
    Ruler(Ruler),
    Code(Code),
}

/// A color of an object that changes over time.
#[derive(Debug, Clone, PartialEq)]
pub struct Paint {
    pub part: Part,
    /// The row cell it colors, or `None` for the whole object.
    pub cell: Option<usize>,
    pub color: Track<Color>,
    /// The time from which the track's color replaces the object's.
    pub start: f64,
}

/// Which color of an object a [`Paint`] changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Part {
    Fill,
    Stroke,
    /// Every run of its text.
    Text,
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
            scale: Track::Constant(1.0),
            pivot: Point::ZERO,
            offset: Track::Constant(Point::ZERO),
            paints: Vec::new(),
            lifetime: None,
            source: Span::default(),
            groups: Vec::new(),
            cells: Vec::new(),
        }
    }

    /// This object, made by the node at `source`.
    pub fn from_source(self, source: Span) -> Self {
        Object { source, ..self }
    }

    /// How the object is moved and scaled at time `t`, by its own
    /// tracks and by the groups it sits in.
    pub fn motion(&self, t: f64) -> Motion {
        self.own_motion(t).then(self.group_motion(t))
    }

    fn own_motion(&self, t: f64) -> Motion {
        Motion {
            pivot: self.pivot,
            scale: self.scale.at(t),
            offset: self.offset.at(t),
        }
    }

    fn group_motion(&self, t: f64) -> Motion {
        self.groups
            .iter()
            .fold(Motion::NONE, |m, g| m.then(g.motion(t)))
    }

    /// How opaque the object is at time `t`, its groups included.
    pub fn opacity_at(&self, t: f64) -> f64 {
        self.groups
            .iter()
            .fold(self.opacity.at(t), |o, g| o * g.opacity.at(t))
    }
}

impl Kind {
    /// Append what this kind draws at time `t` to `out`, where layout
    /// put it.
    pub fn draw(&self, t: f64, out: &mut Vec<Primitive>) {
        match self {
            Kind::Grid(grid) => grid.draw(t, out),
            Kind::Plot(plot) => plot.draw(t, out),
            Kind::Shape(shape) => shape.draw(out),
            Kind::Polygon(polygon) => polygon.draw(out),
            Kind::Polyline(polyline) => polyline.draw(out),
            Kind::Connector(connector) => connector.draw(out),
            Kind::Text(text) => text.draw(out),
            Kind::Row(row) => row.draw(out),
            Kind::Matrix(matrix) => matrix.draw(out),
            Kind::Brace(brace) => brace.draw(out),
            Kind::Ruler(ruler) => ruler.draw(out),
            Kind::Code(code) => code.draw(out),
        }
    }

    /// The color `part` of `cell`, or of the whole object when `cell` is
    /// `None`, at build time: `None` for a fill it does not have. The
    /// error says why the object has no such color to change.
    pub fn color(&self, part: Part, cell: Option<usize>) -> Result<Option<Color>, &'static str> {
        let text = |label: &Option<Label>| match label {
            Some(label) => Ok(label.runs.first().map(|r| r.color)),
            None => Err("has no text"),
        };
        let stroke = |stroke: &Option<Stroke>| match stroke {
            Some(s) => Ok(Some(s.color)),
            None => Err("has no outline"),
        };
        if cell.is_some() && !matches!(self, Kind::Row(_)) {
            return Err("has no cells; only a row does");
        }
        match (self, part) {
            (Kind::Shape(s), Part::Fill) => Ok(s.fill),
            (Kind::Shape(s), Part::Stroke) => stroke(&s.stroke),
            (Kind::Shape(s), Part::Text) => text(&s.label),
            (Kind::Polygon(p), Part::Fill) => Ok(p.fill),
            (Kind::Polygon(p), Part::Stroke) => stroke(&p.stroke),
            (Kind::Polyline(p), Part::Stroke) => Ok(Some(p.stroke.color)),
            (Kind::Text(t), Part::Text) => Ok(t.label.runs.first().map(|r| r.color)),
            (Kind::Connector(c), Part::Stroke) => Ok(Some(c.stroke.color)),
            (Kind::Connector(c), Part::Text) => text(&c.label),
            (Kind::Row(r), _) => {
                let Some(i) = cell else {
                    return Err("is a row, whose colors change by cell: `NAME N`");
                };
                let Some(cell) = r.cells.get(i) else {
                    return Err("has no such cell");
                };
                match part {
                    Part::Fill => Ok(cell.fill),
                    Part::Stroke => stroke(&cell.stroke),
                    Part::Text => text(&cell.label),
                }
            }
            (Kind::Matrix(m), Part::Fill) => Ok(m.fill),
            (Kind::Matrix(m), Part::Stroke) => stroke(&m.stroke),
            (Kind::Matrix(m), Part::Text) => Ok(m.text.runs.first().map(|r| r.color)),
            (Kind::Brace(b), Part::Stroke) => Ok(Some(b.stroke.color)),
            (Kind::Brace(b), Part::Text) => text(&b.label),
            (Kind::Ruler(r), Part::Stroke) => Ok(Some(r.stroke.color)),
            (Kind::Ruler(r), Part::Text) => Ok(r.label.runs.first().map(|r| r.color)),
            (Kind::Code(c), Part::Fill) => Ok(c.fill),
            (Kind::Code(c), Part::Stroke) => stroke(&c.stroke),
            (_, Part::Fill) => Err("has no fill"),
            (_, Part::Stroke) => Err("has no outline"),
            (_, Part::Text) => Err("has no text"),
        }
    }

    /// Set the color `part` of `cell`, or of the whole object, to
    /// `color`, where [`Kind::color`] finds one.
    pub fn repaint(&mut self, part: Part, cell: Option<usize>, color: Color) {
        let text = |label: &mut Label| {
            for run in &mut label.runs {
                run.color = color;
            }
        };
        let stroke = |stroke: &mut Option<Stroke>| {
            if let Some(s) = stroke {
                s.color = color;
            }
        };
        match (self, part) {
            (Kind::Shape(s), Part::Fill) => s.fill = Some(color),
            (Kind::Shape(s), Part::Stroke) => stroke(&mut s.stroke),
            (Kind::Shape(s), Part::Text) => s.label.iter_mut().for_each(text),
            (Kind::Polygon(p), Part::Fill) => p.fill = Some(color),
            (Kind::Polygon(p), Part::Stroke) => stroke(&mut p.stroke),
            (Kind::Polyline(p), Part::Stroke) => p.stroke.color = color,
            (Kind::Text(t), Part::Text) => text(&mut t.label),
            (Kind::Connector(c), Part::Stroke) => c.stroke.color = color,
            (Kind::Connector(c), Part::Text) => c.label.iter_mut().for_each(text),
            (Kind::Row(r), part) => {
                let Some(cell) = cell.and_then(|i| r.cells.get_mut(i)) else {
                    return;
                };
                match part {
                    Part::Fill => cell.fill = Some(color),
                    Part::Stroke => stroke(&mut cell.stroke),
                    Part::Text => cell.label.iter_mut().for_each(text),
                }
            }
            (Kind::Matrix(m), Part::Fill) => m.repaint_fill(color),
            (Kind::Matrix(m), Part::Stroke) => m.repaint_stroke(color),
            (Kind::Matrix(m), Part::Text) => m.repaint_text(color),
            (Kind::Brace(b), Part::Stroke) => b.stroke.color = color,
            (Kind::Brace(b), Part::Text) => b.label.iter_mut().for_each(text),
            (Kind::Ruler(r), Part::Stroke) => r.stroke.color = color,
            (Kind::Ruler(r), Part::Text) => text(&mut r.label),
            (Kind::Code(c), Part::Fill) => c.fill = Some(color),
            (Kind::Code(c), Part::Stroke) => stroke(&mut c.stroke),
            _ => {}
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
    /// Everything to draw at time `t`, in paint order. A row cell moved
    /// or scaled at `t` is drawn after the last row, above every cell
    /// that is not.
    pub fn draw(&self, t: f64) -> Vec<Primitive> {
        let mut out = Vec::new();
        let mut lifted = Vec::new();
        let last_row = self
            .objects
            .iter()
            .rposition(|o| matches!(o.kind, Kind::Row(_)));
        for (i, object) in self.objects.iter().enumerate() {
            self.draw_parts(object, t, &mut out, &mut lifted);
            if Some(i) == last_row {
                out.append(&mut lifted);
            }
        }
        out
    }

    /// Append `object` at time `t` to `out`.
    fn draw_object(&self, object: &Object, t: f64, out: &mut Vec<Primitive>) {
        let mut lifted = Vec::new();
        self.draw_parts(object, t, out, &mut lifted);
        out.append(&mut lifted);
    }

    /// Append `object` at time `t` to `out`, but its row cells that are
    /// moved or scaled at `t` to `lifted`.
    fn draw_parts(
        &self,
        object: &Object,
        t: f64,
        out: &mut Vec<Primitive>,
        lifted: &mut Vec<Primitive>,
    ) {
        if object.lifetime.is_some_and(|l| !l.contains(t)) {
            return;
        }
        let opacity = object.opacity_at(t);
        if opacity <= 0.0 {
            return;
        }
        let start = out.len();
        let lifted_start = lifted.len();
        let mut kind = Cow::Borrowed(&object.kind);
        for paint in object.paints.iter().filter(|p| t >= p.start) {
            kind.to_mut()
                .repaint(paint.part, paint.cell, paint.color.at(t));
        }
        // a thing that follows another takes that thing's motion in
        // place of its own groups', which that motion already holds
        let motion = match &*kind {
            Kind::Connector(c) if c.follow.iter().any(Option::is_some) => {
                let groups = object.group_motion(t);
                let motions = c.follow.each_ref().map(|f| match f {
                    Some(f) => self.motion_of(f.target, t),
                    None => groups,
                });
                if motions.iter().all(Motion::is_none) {
                    c.draw(out);
                } else {
                    c.followed(motions).draw(out);
                }
                object.own_motion(t)
            }
            Kind::Brace(
                b @ Brace {
                    follow: Some(target),
                    ..
                },
            ) => {
                let motion = self.motion_of(*target, t);
                if motion.is_none() {
                    b.draw(out);
                } else {
                    let moved = Brace {
                        from: motion.apply(b.from),
                        to: motion.apply(b.to),
                        ..b.clone()
                    };
                    moved.draw(out);
                }
                object.own_motion(t)
            }
            Kind::Row(row) => {
                if object.cells.is_empty() {
                    row.draw(out);
                } else {
                    draw_cells(row, &object.cells, t, out, lifted);
                }
                match row.follow {
                    Some(f) => object.own_motion(t).then(self.motion_of(f, t)),
                    None => object.motion(t),
                }
            }
            Kind::Matrix(
                m @ Matrix {
                    follow: Some(f), ..
                },
            ) => {
                m.draw(out);
                object.own_motion(t).then(self.motion_of(*f, t))
            }
            kind => {
                kind.draw(t, out);
                object.motion(t)
            }
        };
        for prims in [&mut out[start..], &mut lifted[lifted_start..]] {
            if !motion.is_none() {
                for prim in prims.iter_mut() {
                    prim.transform(motion);
                }
            }
            if opacity < 1.0 {
                for prim in prims.iter_mut() {
                    prim.fade(opacity);
                }
            }
        }
    }

    /// How `target` is moved and scaled at time `t`.
    fn motion_of(&self, target: Target, t: f64) -> Motion {
        let Some(object) = self.objects.iter().find(|o| o.source == target.source) else {
            return Motion::NONE;
        };
        let cell = target
            .cell
            .and_then(|i| object.cells.iter().find(|(c, _)| *c == i))
            .map_or(Motion::NONE, |(_, a)| a.motion(t));
        cell.then(object.motion(t))
    }

    /// The box around what object `index` draws at time `t`, as its
    /// top-left and bottom-right corners, or `None` when it draws
    /// nothing.
    pub fn bounds(&self, index: usize, t: f64) -> Option<(Point, Point)> {
        let mut prims = Vec::new();
        self.draw_object(&self.objects[index], t, &mut prims);
        primitive::bounds(&self.fonts, &prims)
    }

    /// Each character the scene's text holds that no font has a glyph
    /// for, once, in the order the text first holds it. Formulas are
    /// not searched.
    pub fn missing_glyphs(&self) -> Vec<char> {
        let mut prims = Vec::new();
        for object in &self.objects {
            object.kind.draw(0.0, &mut prims);
        }
        let mut out = Vec::new();
        for prim in &prims {
            let Primitive::Text { runs, .. } = prim else {
                continue;
            };
            for run in runs.iter().filter(|r| r.font != Font::Math) {
                for c in run.text.chars() {
                    if c != '\n' && self.fonts.cover(run.font, c).is_none() && !out.contains(&c) {
                        out.push(c);
                    }
                }
            }
        }
        out
    }

    /// The object whose box holds `point` at time `t`, the smallest when
    /// several do and the one painted last among equals.
    pub fn object_at(&self, point: Point, t: f64) -> Option<usize> {
        let mut best: Option<(usize, f64)> = None;
        for index in 0..self.objects.len() {
            let Some((min, max)) = self.bounds(index, t) else {
                continue;
            };
            if point.x < min.x || point.x > max.x || point.y < min.y || point.y > max.y {
                continue;
            }
            let area = (max.x - min.x) * (max.y - min.y);
            if best.is_none_or(|(_, a)| area <= a) {
                best = Some((index, area));
            }
        }
        best.map(|(index, _)| index)
    }
}

/// Append the cells of `row` to `out` at time `t`, each moved, scaled
/// and faded by its entry in `cells`, and those moved or scaled at `t`
/// to `lifted` instead.
fn draw_cells(
    row: &Row,
    cells: &[(usize, Animation)],
    t: f64,
    out: &mut Vec<Primitive>,
    lifted: &mut Vec<Primitive>,
) {
    for i in 0..row.cells.len() {
        let Some((_, animation)) = cells.iter().find(|(c, _)| *c == i) else {
            row.draw_cell(i, out);
            continue;
        };
        let opacity = animation.opacity.at(t);
        if opacity <= 0.0 {
            continue;
        }
        let motion = animation.motion(t);
        let dest = if motion.is_none() {
            &mut *out
        } else {
            &mut *lifted
        };
        let start = dest.len();
        row.draw_cell(i, dest);
        for prim in &mut dest[start..] {
            if !motion.is_none() {
                prim.transform(motion);
            }
            if opacity < 1.0 {
                prim.fade(opacity);
            }
        }
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
    fn missing_glyphs_names_each_character_no_font_has_once() {
        let src = "canvas { size 100 100; font sans; \
                   text { at 0 0; text \"\u{10fffd} \u{221e} `\u{1d43}`\"; } \
                   box { at 0 20; size 50 20; text \"\u{10fffe} \u{10fffd}\"; } }";
        let scene = build(&crate::lang::parse(src).unwrap(), &[], &[]).unwrap();
        assert_eq!(scene.missing_glyphs(), ['\u{10fffd}', '\u{10fffe}']);
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
            fonts: Fonts::default(),
            sections: Vec::new(),
        };
        assert!(scene.draw(0.5).is_empty());
        assert_eq!(scene.draw(1.0).len(), 2);
        assert!(scene.draw(2.0).is_empty());
    }

    #[test]
    fn a_connector_follows_the_moved_and_scaled_edge_of_its_target() {
        let shape = Shape {
            origin: Point::new(0.0, 0.0),
            size: Point::new(100.0, 40.0),
            oval: false,
            radius: 0.0,
            fill: None,
            stroke: Some(Stroke::solid(Color::BLACK, 2.0)),
            label: None,
            padding: Point::ZERO,
            text_align: None,
        };
        let source = Span { start: 1, end: 2 };
        let mut target = Object::new(Kind::Shape(shape.clone())).from_source(source);
        target.pivot = Point::new(0.0, 20.0);
        target.scale = Track::from_to(0.0, 1.0, 1.0, 2.0);
        target.offset = Track::from_to(0.0, Point::ZERO, 1.0, Point::new(0.0, 100.0));
        let line = Connector {
            from: Point::new(300.0, 20.0),
            to: Point::new(100.0, 20.0),
            from_dir: None,
            to_dir: None,
            stroke: Stroke::solid(Color::BLACK, 2.0),
            route: Route::Straight,
            head: Head::None,
            tail: Head::None,
            head_size: None,
            label: None,
            label_side: LabelSide::Above,
            label_at: 0.5,
            follow: [
                None,
                Some(Box::new(Follow {
                    target: Target { source, cell: None },
                    end: End::Side {
                        at: Point::new(100.0, 20.0),
                        out: Point::new(1.0, 0.0),
                    },
                })),
            ],
            looped: false,
        };
        let scene = Scene {
            canvas: canvas(),
            objects: vec![target, Object::new(Kind::Connector(line))],
            fonts: Fonts::default(),
            sections: Vec::new(),
        };
        let ends = |t: f64| match scene.draw(t)[1] {
            Primitive::Line { from, to, .. } => (from, to),
            ref other => panic!("expected a line, got {other:?}"),
        };
        assert_eq!(
            ends(0.0),
            (Point::new(300.0, 20.0), Point::new(100.0, 20.0))
        );
        assert_eq!(
            ends(1.0),
            (Point::new(300.0, 20.0), Point::new(200.0, 120.0))
        );
        match scene.draw(1.0)[0] {
            Primitive::Rect {
                origin,
                size,
                stroke,
                ..
            } => {
                assert_eq!(
                    (origin, size),
                    (Point::new(0.0, 80.0), Point::new(200.0, 80.0))
                );
                assert_eq!(stroke.unwrap().width, 4.0);
            }
            ref other => panic!("expected a rect, got {other:?}"),
        }
    }

    #[test]
    fn opacity_fades_every_color_of_the_object() {
        let mut object = Object::new(Grid::new(1, 1, ["x"]));
        object.opacity = Track::from_to(0.0, 0.0, 1.0, 1.0);
        let scene = Scene {
            canvas: canvas(),
            objects: vec![object],
            fonts: Fonts::default(),
            sections: Vec::new(),
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
            Primitive::Text { runs, .. } => assert_eq!(runs[0].color.a, 0.5),
            other => panic!("expected text, got {other:?}"),
        }
    }
}

use std::collections::HashMap;

use super::{
    Canvas, Color, Connector, HAlign, Kind, Label, Object, Point, Scene, Shape, Stroke, Text,
};
use crate::lang::{Error, Node, Program, Property, Span, ValueKind};

const STROKE_WIDTH: f64 = 2.0;
const TEXT_SIZE: f64 = 16.0;
const FPS: u32 = 30;

/// The scene a program describes.
pub fn build(program: &Program) -> Result<Scene, Error> {
    let canvas = match program.nodes.as_slice() {
        [] => return Err(Error::new(Span::default(), "the file has no `canvas`")),
        [node, ..] if node.kind.text != "canvas" => {
            return Err(Error::new(
                node.kind.span,
                format!(
                    "expected `canvas` at the top level, found `{}`",
                    node.kind.text
                ),
            ));
        }
        [_, extra, ..] => {
            return Err(Error::new(extra.kind.span, "a file holds one `canvas`"));
        }
        [node] => node,
    };
    let props = Props::new(canvas, &["size", "background"], true)?;
    let size = props.require("size")?;
    let [width, height] = size.numbers::<2>()?;
    let background = match props.get("background") {
        Some(p) => p.color()?,
        None => Color::WHITE,
    };

    let mut shapes: HashMap<&str, Shape> = HashMap::new();
    for node in &canvas.children {
        if let (Some(oval), Some(name)) = (shape_kind(node), &node.name)
            && shapes.insert(&name.text, shape(node, oval)?).is_some()
        {
            return Err(Error::new(
                name.span,
                format!("two shapes are named `{}`", name.text),
            ));
        }
    }

    let mut objects = Vec::new();
    for node in &canvas.children {
        let kind = match (shape_kind(node), &node.name) {
            (Some(_), Some(name)) => Kind::Shape(shapes[name.text.as_str()].clone()),
            (Some(oval), None) => Kind::Shape(shape(node, oval)?),
            (None, _) => match node.kind.text.as_str() {
                "line" => Kind::Connector(connector(node, &shapes, false)?),
                "arrow" => Kind::Connector(connector(node, &shapes, true)?),
                "text" => Kind::Text(text(node)?),
                other => {
                    return Err(Error::new(
                        node.kind.span,
                        format!(
                            "unknown node `{other}`; a canvas holds box, ellipse, line, arrow and text"
                        ),
                    ));
                }
            },
        };
        objects.push(Object::new(kind));
    }

    Ok(Scene {
        canvas: Canvas {
            width: pixels(width, size)?,
            height: pixels(height, size)?,
            fps: FPS,
            duration: 0.0,
            background,
        },
        objects,
    })
}

fn shape_kind(node: &Node) -> Option<bool> {
    match node.kind.text.as_str() {
        "box" => Some(false),
        "ellipse" => Some(true),
        _ => None,
    }
}

fn shape(node: &Node, oval: bool) -> Result<Shape, Error> {
    let props = Props::new(
        node,
        &[
            "at",
            "size",
            "fill",
            "stroke",
            "stroke-width",
            "text",
            "text-size",
            "text-color",
        ],
        false,
    )?;
    let [x, y] = props.require("at")?.numbers::<2>()?;
    let [w, h] = props.require("size")?.numbers::<2>()?;
    let fill = match props.get("fill") {
        Some(p) => p.color_or_none()?,
        None => Some(Color::WHITE),
    };
    Ok(Shape {
        origin: Point::new(x, y),
        size: Point::new(w, h),
        oval,
        fill,
        stroke: props.stroke()?,
        label: props.label("text", "text-size", "text-color")?,
    })
}

fn connector(node: &Node, shapes: &HashMap<&str, Shape>, head: bool) -> Result<Connector, Error> {
    let props = Props::new(
        node,
        &[
            "from",
            "to",
            "stroke",
            "stroke-width",
            "label",
            "text-size",
            "text-color",
        ],
        false,
    )?;
    let from = end(props.require("from")?, shapes)?;
    let to = end(props.require("to")?, shapes)?;
    let center = |end: &End| match end {
        End::Point(p) => *p,
        End::Shape(s) => s.center(),
    };
    let on_edge = |end: &End, other: &End| match end {
        End::Point(p) => *p,
        End::Shape(s) => s.edge_toward(center(other)),
    };
    let stroke = props.stroke()?.unwrap_or(Stroke {
        color: Color::BLACK,
        width: STROKE_WIDTH,
    });
    Ok(Connector {
        from: on_edge(&from, &to),
        to: on_edge(&to, &from),
        stroke,
        head,
        label: props.label("label", "text-size", "text-color")?,
    })
}

fn text(node: &Node) -> Result<Text, Error> {
    let props = Props::new(node, &["at", "text", "size", "color", "align"], false)?;
    let [x, y] = props.require("at")?.numbers::<2>()?;
    let align = match props.get("align") {
        None => HAlign::Left,
        Some(p) => match p.word()? {
            "left" => HAlign::Left,
            "center" => HAlign::Center,
            "right" => HAlign::Right,
            _ => return Err(Error::new(p.span, "`align` is left, center or right")),
        },
    };
    let label = props
        .label("text", "size", "color")?
        .ok_or_else(|| Error::new(node.kind.span, "`text` needs `text`"))?;
    Ok(Text {
        origin: Point::new(x, y),
        label,
        align,
    })
}

enum End<'a> {
    Point(Point),
    Shape(&'a Shape),
}

fn end<'a>(p: &Property, shapes: &'a HashMap<&str, Shape>) -> Result<End<'a>, Error> {
    match p.values.as_slice() {
        [v] => match &v.kind {
            ValueKind::Word(name) => shapes
                .get(name.as_str())
                .map(End::Shape)
                .ok_or_else(|| Error::new(v.span, format!("no box or ellipse is named `{name}`"))),
            _ => Err(Error::new(v.span, "expected a shape's name or two numbers")),
        },
        _ => p.numbers::<2>().map(|[x, y]| End::Point(Point::new(x, y))),
    }
}

fn pixels(v: f64, p: &Property) -> Result<u32, Error> {
    if v < 1.0 || v.fract() != 0.0 || v > u32::MAX as f64 {
        return Err(Error::new(
            p.span,
            "a canvas size is two whole numbers of pixels",
        ));
    }
    Ok(v as u32)
}

/// A node's properties, checked against the keys its kind takes.
struct Props<'a> {
    node: &'a Node,
}

impl<'a> Props<'a> {
    fn new(node: &'a Node, keys: &[&str], children: bool) -> Result<Self, Error> {
        let kind = &node.kind.text;
        if let (false, Some(child)) = (children, node.children.first()) {
            return Err(Error::new(
                child.kind.span,
                format!("a `{kind}` holds no nodes"),
            ));
        }
        for (i, p) in node.properties.iter().enumerate() {
            let key = &p.key.text;
            if !keys.contains(&key.as_str()) {
                return Err(Error::new(
                    p.key.span,
                    format!(
                        "`{kind}` has no property `{key}`; it takes {}",
                        keys.join(", ")
                    ),
                ));
            }
            if node.properties[..i].iter().any(|q| q.key.text == *key) {
                return Err(Error::new(p.key.span, format!("`{key}` is set twice")));
            }
        }
        Ok(Props { node })
    }

    fn get(&self, key: &str) -> Option<&'a Property> {
        self.node.properties.iter().find(|p| p.key.text == key)
    }

    fn require(&self, key: &str) -> Result<&'a Property, Error> {
        self.get(key).ok_or_else(|| {
            Error::new(
                self.node.kind.span,
                format!("`{}` needs `{key}`", self.node.kind.text),
            )
        })
    }

    /// `stroke` and `stroke-width`, or `None` for `stroke none`.
    fn stroke(&self) -> Result<Option<Stroke>, Error> {
        let color = match self.get("stroke") {
            Some(p) => p.color_or_none()?,
            None => Some(Color::BLACK),
        };
        let width = match self.get("stroke-width") {
            Some(p) => p.numbers::<1>()?[0],
            None => STROKE_WIDTH,
        };
        Ok(color.map(|color| Stroke { color, width }))
    }

    fn label(&self, text: &str, size: &str, color: &str) -> Result<Option<Label>, Error> {
        let Some(p) = self.get(text) else {
            return Ok(None);
        };
        Ok(Some(Label {
            text: p.string()?,
            size: match self.get(size) {
                Some(p) => p.numbers::<1>()?[0],
                None => TEXT_SIZE,
            },
            color: match self.get(color) {
                Some(p) => p.color()?,
                None => Color::BLACK,
            },
        }))
    }
}

impl Property {
    fn numbers<const N: usize>(&self) -> Result<[f64; N], Error> {
        let mut out = [0.0; N];
        let bad = || {
            let noun = if N == 1 { "number" } else { "numbers" };
            Error::new(self.span, format!("`{}` takes {N} {noun}", self.key.text))
        };
        if self.values.len() != N {
            return Err(bad());
        }
        for (slot, v) in out.iter_mut().zip(&self.values) {
            match v.kind {
                ValueKind::Number(n) => *slot = n,
                _ => return Err(bad()),
            }
        }
        Ok(out)
    }

    fn string(&self) -> Result<String, Error> {
        match self.values.as_slice() {
            [v] => match &v.kind {
                ValueKind::Str(s) => Ok(s.clone()),
                _ => Err(self.expected("a string")),
            },
            _ => Err(self.expected("a string")),
        }
    }

    fn word(&self) -> Result<&str, Error> {
        match self.values.as_slice() {
            [v] => match &v.kind {
                ValueKind::Word(w) => Ok(w),
                _ => Err(self.expected("a word")),
            },
            _ => Err(self.expected("a word")),
        }
    }

    fn color(&self) -> Result<Color, Error> {
        self.color_or_none()?
            .ok_or_else(|| self.expected("a color"))
    }

    fn color_or_none(&self) -> Result<Option<Color>, Error> {
        match self.values.as_slice() {
            [v] => match &v.kind {
                ValueKind::Color([r, g, b, a]) => Ok(Some(Color::rgba(
                    *r as f32 / 255.0,
                    *g as f32 / 255.0,
                    *b as f32 / 255.0,
                    *a as f32 / 255.0,
                ))),
                ValueKind::Word(w) if w == "none" => Ok(None),
                _ => Err(self.expected("a color or `none`")),
            },
            _ => Err(self.expected("a color or `none`")),
        }
    }

    fn expected(&self, what: &str) -> Error {
        Error::new(self.span, format!("`{}` takes {what}", self.key.text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;

    fn scene(src: &str) -> Result<Scene, (usize, usize, String)> {
        let program = parse(src).unwrap();
        build(&program).map_err(|e| {
            let (line, col) = e.line_col(src);
            (line, col, e.message)
        })
    }

    #[test]
    fn a_diagram_builds_in_source_order() {
        let s = scene(
            r#"canvas {
                size 300 100;
                background #000000;
                box a { at 0 30; size 100 40; text "a"; }
                ellipse b { at 200 30; size 100 40; }
                arrow { from a; to b; label "goes to"; }
                text { at 10 10; text "title"; align left; }
            }"#,
        )
        .unwrap();
        assert_eq!((s.canvas.width, s.canvas.height), (300, 100));
        assert_eq!(s.canvas.background, Color::BLACK);
        assert_eq!(s.canvas.frame_count(), 1);
        let kinds: Vec<&str> = s
            .objects
            .iter()
            .map(|o| match o.kind {
                Kind::Shape(_) => "shape",
                Kind::Connector(_) => "connector",
                Kind::Text(_) => "text",
                _ => "other",
            })
            .collect();
        assert_eq!(kinds, ["shape", "shape", "connector", "text"]);
    }

    #[test]
    fn an_arrow_between_shapes_runs_edge_to_edge() {
        let s = scene(
            "canvas { size 300 100;
                box a { at 0 30; size 100 40; }
                box b { at 200 30; size 100 40; }
                arrow { from a; to b; } }",
        )
        .unwrap();
        match &s.objects[2].kind {
            Kind::Connector(c) => {
                assert_eq!(c.from, Point::new(100.0, 50.0));
                assert_eq!(c.to, Point::new(200.0, 50.0));
            }
            other => panic!("expected a connector, got {other:?}"),
        }
    }

    #[test]
    fn an_arrow_may_name_a_shape_declared_after_it() {
        assert!(
            scene(
                "canvas { size 10 10; arrow { from a; to 5 5; }
                 box a { at 0 0; size 2 2; } }"
            )
            .is_ok()
        );
    }

    #[test]
    fn mistakes_point_at_the_offending_text() {
        assert_eq!(
            scene("canvas { size 10 10;\n  box { at 1 2; colour red; } }").unwrap_err(),
            (
                2,
                17,
                "`box` has no property `colour`; it takes at, size, fill, stroke, \
                 stroke-width, text, text-size, text-color"
                    .into()
            )
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  arrow { from a; to 1 1; } }").unwrap_err(),
            (2, 16, "no box or ellipse is named `a`".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  box { at 1; size 2 2; } }").unwrap_err(),
            (2, 9, "`at` takes 2 numbers".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  box { size 2 2; } }").unwrap_err(),
            (2, 3, "`box` needs `at`".into())
        );
        assert_eq!(
            scene("canvas { size 10.5 10; }").unwrap_err(),
            (1, 10, "a canvas size is two whole numbers of pixels".into())
        );
        assert_eq!(
            scene(
                "canvas { size 10 10;\n box a { at 0 0; size 1 1; }\n box a { at 0 0; size 1 1; } }"
            )
            .unwrap_err(),
            (3, 6, "two shapes are named `a`".into())
        );
    }

    #[test]
    fn stroke_none_draws_no_outline() {
        let s =
            scene("canvas { size 10 10; box { at 0 0; size 5 5; stroke none; fill #ff0000; } }")
                .unwrap();
        match &s.objects[0].kind {
            Kind::Shape(shape) => {
                assert_eq!(shape.stroke, None);
                assert_eq!(shape.fill, Some(Color::rgb(1.0, 0.0, 0.0)));
            }
            other => panic!("expected a shape, got {other:?}"),
        }
    }
}

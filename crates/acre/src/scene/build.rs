use std::collections::HashMap;

use super::code::{self, Code};
use super::expand::expand;
use super::markup;
use super::schema;
use super::{
    Brace, Canvas, Color, Connector, Font, HAlign, Kind, Label, Object, Point, Row, RowCell, Ruler,
    Run, Scene, Shape, Side, Stroke, Text,
};
use crate::lang::{Error, Node, Program, Property, Span, Value, ValueKind};

const STROKE_WIDTH: f64 = 2.0;
const TEXT_SIZE: f64 = 16.0;
const FPS: u32 = 30;
/// The space `margin` leaves around the drawing of an `auto` canvas.
const AUTO_MARGIN: f64 = 20.0;

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
    let (canvas, colors) = expand(canvas)?;
    let canvas = &canvas;
    let props = Props::new(canvas, &keys("canvas"), true)?;
    let size = props.require("size")?;
    let [width, height] = canvas_size(size)?;
    let margin = match props.get("margin") {
        Some(p) if width.is_some() && height.is_some() => {
            return Err(Error::new(p.span, "`margin` needs an `auto` in `size`"));
        }
        Some(p) => {
            let m = p.numbers::<1>()?[0];
            if m < 0.0 {
                return Err(p.expected("a number that is not negative"));
            }
            m
        }
        None => AUTO_MARGIN,
    };
    let background = match props.get("background") {
        Some(p) => p.color()?,
        None => Color::WHITE,
    };
    let mut names = Names {
        font: match props.get("font") {
            Some(p) => font(p)?,
            None => Font::Mono,
        },
        colors,
        palettes: HashMap::new(),
        shapes: HashMap::new(),
        rows: HashMap::new(),
        codes: HashMap::new(),
    };

    // palettes first, so that a node may name one wherever it sits
    for node in &canvas.children {
        if node.kind.text == "palette" {
            let name = node
                .name
                .as_ref()
                .ok_or_else(|| Error::new(node.kind.span, "a `palette` needs a name"))?;
            let palette = palette(node)?;
            if names.palettes.insert(&name.text, palette).is_some() {
                return Err(twice("palettes", name));
            }
        }
    }

    let mut objects = add_canvas_objects(canvas, &names, Point::ZERO)?;

    // an `auto` side is the box around what is drawn plus the margin
    // on both sides, so the drawing moves to start at the margin
    let mut offset = Point::ZERO;
    let mut sides = [width.unwrap_or(0.0), height.unwrap_or(0.0)];
    if width.is_none() || height.is_none() {
        let (min, max) = bounds_of(&objects).unwrap_or((Point::ZERO, Point::ZERO));
        if width.is_none() {
            sides[0] = (max.x - min.x + 2.0 * margin).ceil();
            offset.x = margin - min.x;
        }
        if height.is_none() {
            sides[1] = (max.y - min.y + 2.0 * margin).ceil();
            offset.y = margin - min.y;
        }
        objects = add_canvas_objects(canvas, &names, offset)?;
    }

    Ok(Scene {
        canvas: Canvas {
            width: pixels(sides[0], size)?,
            height: pixels(sides[1], size)?,
            fps: FPS,
            duration: 0.0,
            background,
        },
        objects,
    })
}

/// The objects of the canvas node, drawn with its origin at `offset`.
fn add_canvas_objects<'a>(
    canvas: &'a Node,
    names: &Names<'a>,
    offset: Point,
) -> Result<Vec<Object>, Error> {
    // named shapes and rows, so that a node may name any of them
    // wherever it sits in the file
    let mut names = names.clone();
    let mut places = Places::new();
    name_things(&canvas.children, offset, &mut names, &mut places)?;
    let mut objects = Vec::new();
    let mut panels = 0;
    let pass = Pass {
        names: &names,
        places: &places,
        measuring: false,
    };
    add_objects(&canvas.children, offset, &pass, &mut panels, &mut objects)?;
    Ok(objects)
}

/// The width and height `size` gives, `None` for each `auto`.
fn canvas_size(size: &Property) -> Result<[Option<f64>; 2], Error> {
    let bad = || size.expected("two numbers, or `auto` for either");
    let [w, h] = size.values.as_slice() else {
        return Err(bad());
    };
    let side = |v: &Value| match &v.kind {
        ValueKind::Number(n) => Ok(Some(*n)),
        ValueKind::Word(w) if w == "auto" => Ok(None),
        _ => Err(bad()),
    };
    Ok([side(w)?, side(h)?])
}

/// Build every named shape and row in `nodes` and the groups among them,
/// in source order, into `names`.
fn name_things<'a>(
    nodes: &'a [Node],
    offset: Point,
    names: &mut Names<'a>,
    places: &mut Places,
) -> Result<(), Error> {
    for node in nodes {
        if node.kind.text == "group" {
            let at = group_at(node, names, offset)?;
            name_things(&node.children, at, names, places)?;
            continue;
        }
        if node.kind.text == "panel" {
            let place = panel_place(node, names, offset, None)?;
            name_things(&node.children, place.contents, names, places)?;
            continue;
        }
        if node.kind.text == "stack" || node.kind.text == "grid" {
            let slots = lay_out(node, offset, names, places)?;
            for (child, slot) in node.children.iter().zip(slots) {
                places.insert(child.span.start, slot);
                name_things(std::slice::from_ref(child), slot, names, places)?;
            }
            continue;
        }
        let Some(name) = &node.name else { continue };
        let key = name.text.as_str();
        let used = names.shapes.contains_key(key)
            || names.rows.contains_key(key)
            || names.codes.contains_key(key);
        let taken = match node.kind.text.as_str() {
            "box" | "ellipse" => {
                let shape = shape(node, names, offset)?;
                names.shapes.insert(key, shape);
                used
            }
            "row" => {
                let row = row(node, names, offset)?;
                names.rows.insert(key, row);
                used
            }
            "code" => {
                let code = code(node, names, offset)?;
                names.codes.insert(key, code);
                used
            }
            _ => false,
        };
        if taken {
            return Err(twice("things", name));
        }
    }
    Ok(())
}

/// Append the objects `nodes` draw, with the groups among them opened,
/// to `out` in source order.
fn add_objects(
    nodes: &[Node],
    offset: Point,
    pass: &Pass,
    panels: &mut usize,
    out: &mut Vec<Object>,
) -> Result<(), Error> {
    let names = pass.names;
    for node in nodes {
        let named = node.name.as_ref().map(|n| n.text.as_str());
        let kind = match node.kind.text.as_str() {
            "palette" => continue,
            "caption" => {
                // a caption goes under everything before it
                let below = bounds_of(out).map_or(offset.y, |(_, max)| max.y);
                let t = caption(node, names, Point::new(offset.x, below))?;
                out.push(Object::new(Kind::Text(t)).from_source(node.span));
                continue;
            }
            "group" => {
                let at = group_at(node, names, offset)?;
                add_objects(&node.children, at, pass, panels, out)?;
                continue;
            }
            "panel" => {
                add_panel(node, pass, offset, panels, out)?;
                continue;
            }
            "stack" | "grid" => {
                for child in &node.children {
                    let slot = pass.places[&child.span.start];
                    add_objects(std::slice::from_ref(child), slot, pass, panels, out)?;
                }
                continue;
            }
            "line" | "arrow" => {
                let head = node.kind.text == "arrow";
                let repeat = node
                    .properties
                    .iter()
                    .find(|p| p.key.text == "repeat")
                    .map_or(Ok(1), |p| p.count())?;
                for shift in 0..repeat {
                    let c = match connector(node, names, head, offset, shift) {
                        Ok(c) => c,
                        // a measure leaves out what names a thing not
                        // placed yet
                        Err(_) if pass.measuring => continue,
                        Err(e) => return Err(e),
                    };
                    out.push(Object::new(Kind::Connector(c)).from_source(node.span));
                }
                continue;
            }
            "box" | "ellipse" => match named {
                Some(name) => Kind::Shape(names.shapes[name].clone()),
                None => Kind::Shape(shape(node, names, offset)?),
            },
            "row" => match named {
                Some(name) => Kind::Row(names.rows[name].clone()),
                None => Kind::Row(row(node, names, offset)?),
            },
            "code" => match named {
                Some(name) => Kind::Code(names.codes[name].clone()),
                None => Kind::Code(code(node, names, offset)?),
            },
            "text" => Kind::Text(text(node, names, offset)?),
            "table" => {
                for t in table(node, names, offset)? {
                    out.push(Object::new(Kind::Text(t)).from_source(node.span));
                }
                continue;
            }
            "brace" => match brace(node, names, offset) {
                Ok(b) => Kind::Brace(b),
                Err(_) if pass.measuring => continue,
                Err(e) => return Err(e),
            },
            "ruler" => Kind::Ruler(ruler(node, names, offset)?),
            other => {
                return Err(Error::new(
                    node.kind.span,
                    format!(
                        "unknown node `{other}`; a canvas holds box, ellipse, line, arrow, \
                         text, table, row, code, brace, ruler, group, panel, caption, stack, grid, \
                         palette and style"
                    ),
                ));
            }
        };
        out.push(Object::new(kind).from_source(node.span));
    }
    Ok(())
}

/// Where a panel's frame, heading and contents go.
struct PanelPlace {
    /// The top-left corner of the panel: its frame's when it has one.
    corner: Point,
    /// The top-left corner of the heading, inside the frame's padding.
    at: Point,
    /// The origin of the nodes in the panel, under the heading.
    contents: Point,
    heading: Option<Label>,
}

/// Place `node`, a panel, numbered `number` when it has one.
fn panel_place(
    node: &Node,
    names: &Names,
    offset: Point,
    number: Option<usize>,
) -> Result<PanelPlace, Error> {
    let props = Props::new(node, &keys("panel"), true)?;
    let corner = match props.get("at") {
        Some(p) => self::at(p, names, offset)?,
        None => offset,
    };
    let at = match framed(&props)? {
        true => {
            let pad = props.number_or("padding", 16.0)?;
            corner + Point::new(pad, pad)
        }
        false => corner,
    };
    let font = match props.get("title-font") {
        Some(p) => font(p)?,
        None => Font::SansBold,
    };
    let heading = match props.get("title") {
        None => None,
        Some(p) => {
            let mut runs = Vec::new();
            if let Some(n) = number {
                runs.push(Run {
                    text: format!("{n}   "),
                    font,
                    color: props.color_or("number-color", Color::rgb(0.54, 0.54, 0.54))?,
                });
            }
            runs.extend(
                markup::runs(
                    &p.string()?,
                    font,
                    props.color_or("title-color", Color::BLACK)?,
                    &names.colors,
                )
                .map_err(|message| Error::new(p.span, message))?,
            );
            Some(Label {
                runs,
                size: props.number_or("title-size", 18.0)?,
                width: None,
            })
        }
    };
    let below = match &heading {
        Some(h) => h.size().y + props.number_or("gap", 16.0)?,
        None => 0.0,
    };
    Ok(PanelPlace {
        corner,
        at,
        contents: at + Point::new(0.0, below),
        heading,
    })
}

/// Whether a panel draws a frame: it has a `stroke` or a `fill`.
fn framed(props: &Props) -> Result<bool, Error> {
    let stroke = props.stroke(None)?.is_some();
    let fill = match props.get("fill") {
        Some(p) => p.color_or_none()?.is_some(),
        None => false,
    };
    Ok(stroke || fill)
}

/// Append a panel's heading, frame, contents and captions to `out`.
fn add_panel(
    node: &Node,
    pass: &Pass,
    offset: Point,
    panels: &mut usize,
    out: &mut Vec<Object>,
) -> Result<(), Error> {
    let names = pass.names;
    let props = Props::new(node, &keys("panel"), true)?;
    let numbered = match props.get("number") {
        Some(p) => p.word()? != "none",
        None => true,
    };
    let number = numbered.then(|| {
        *panels += 1;
        *panels
    });
    let place = panel_place(node, names, offset, number)?;
    let start = out.len();
    if let Some(heading) = place.heading {
        let text = Text {
            origin: place.at,
            label: heading,
            align: HAlign::Left,
        };
        out.push(Object::new(Kind::Text(text)).from_source(node.span));
    }
    let (captions, contents): (Vec<&Node>, Vec<&Node>) =
        node.children.iter().partition(|c| c.kind.text == "caption");
    let contents: Vec<Node> = contents.into_iter().cloned().collect();
    add_objects(&contents, place.contents, pass, panels, out)?;

    let max = bounds_of(&out[start..]).map_or(place.at, |(_, max)| max);
    let mut bottom = max.y;
    if framed(&props)? {
        // the frame reaches past the contents by the padding, and is at
        // least `width` by `height`
        let pad = props.number_or("padding", 16.0)?;
        let size = max - place.corner + Point::new(pad, pad);
        let size = Point::new(
            size.x.max(props.number_or("width", 0.0)?),
            size.y.max(props.number_or("height", 0.0)?),
        );
        bottom = place.corner.y + size.y;
        let fill = match props.get("fill") {
            Some(p) => p.color_or_none()?,
            None => None,
        };
        let frame = Shape {
            origin: place.corner,
            size,
            oval: false,
            radius: props.number_or("radius", 0.0)?,
            fill,
            stroke: props.stroke(None)?,
            label: None,
            padding: Point::ZERO,
            text_align: None,
        };
        out.insert(
            start,
            Object::new(Kind::Shape(frame)).from_source(node.span),
        );
    }
    for c in captions {
        let t = caption(c, names, Point::new(place.corner.x, bottom))?;
        bottom = t.origin.y + t.label.size().y;
        out.push(Object::new(Kind::Text(t)).from_source(c.span));
    }
    Ok(())
}

/// A caption whose top-left corner goes `gap` under `above`.
fn caption(node: &Node, names: &Names, above: Point) -> Result<Text, Error> {
    let props = Props::new(node, &keys("caption"), false)?;
    let mut label = props
        .label("text", "size", "color", names)?
        .ok_or_else(|| Error::new(node.kind.span, "`caption` needs `text`"))?;
    if props.get("size").is_none() {
        label.size = 13.0;
    }
    if let Some(p) = props.get("width") {
        label.width = Some(p.numbers::<1>()?[0]);
    }
    Ok(Text {
        origin: above + Point::new(0.0, props.number_or("gap", 10.0)?),
        label,
        align: HAlign::Left,
    })
}

/// The box around what `objects` draw at time 0.
fn bounds_of(objects: &[Object]) -> Option<(Point, Point)> {
    let mut prims = Vec::new();
    for o in objects {
        o.draw(0.0, &mut prims);
    }
    super::primitive::bounds(&prims)
}

/// The slot of each child of a stack or a grid, by where the child's
/// node starts in the source.
type Places = HashMap<usize, Point>;

/// What a pass that adds objects reads.
struct Pass<'p, 'a> {
    names: &'p Names<'a>,
    places: &'p Places,
    /// Whether the pass only measures, and so leaves out lines, arrows
    /// and braces, which may name things not placed yet.
    measuring: bool,
}

/// The box around what `node` draws with its origin at `slot`, without
/// its lines, arrows and braces, from a copy of the names so far.
fn measure<'a>(
    node: &'a Node,
    slot: Point,
    names: &Names<'a>,
    places: &Places,
) -> Result<Option<(Point, Point)>, Error> {
    let mut names = names.clone();
    let mut places = places.clone();
    let node = std::slice::from_ref(node);
    name_things(node, slot, &mut names, &mut places)?;
    let pass = Pass {
        names: &names,
        places: &places,
        measuring: true,
    };
    let mut objects = Vec::new();
    add_objects(node, slot, &pass, &mut 0, &mut objects)?;
    Ok(bounds_of(&objects))
}

/// The slot of each child of `node`, a stack or a grid. A child's size
/// runs from its slot to the far edge of what it draws, so a child's own
/// `at` adds space before it.
fn lay_out<'a>(
    node: &'a Node,
    offset: Point,
    names: &Names<'a>,
    places: &Places,
) -> Result<Vec<Point>, Error> {
    let props = Props::new(node, &keys(&node.kind.text), true)?;
    let at = match props.get("at") {
        Some(p) => self::at(p, names, offset)?,
        None => offset,
    };
    let align = match props.get("align") {
        None => 0.0,
        Some(p) => match p.word()? {
            "start" => 0.0,
            "center" => 0.5,
            "end" => 1.0,
            _ => return Err(Error::new(p.span, "`align` is start, center or end")),
        },
    };
    let size_at = |child: &'a Node, slot: Point| -> Result<Point, Error> {
        Ok(match measure(child, slot, names, places)? {
            Some((_, max)) => Point::new((max.x - slot.x).max(0.0), (max.y - slot.y).max(0.0)),
            None => Point::ZERO,
        })
    };

    if node.kind.text == "stack" {
        let across = match props.get("direction") {
            None => false,
            Some(p) => match p.word()? {
                "down" => false,
                "right" => true,
                _ => return Err(Error::new(p.span, "`direction` is down or right")),
            },
        };
        let gap = props.number_or("gap", 0.0)?;
        let sizes = node
            .children
            .iter()
            .map(|c| size_at(c, at))
            .collect::<Result<Vec<_>, _>>()?;
        let breadth = |s: &Point| if across { s.y } else { s.x };
        let widest = sizes.iter().map(breadth).fold(0.0, f64::max);
        let mut along = 0.0;
        let mut slots = Vec::new();
        for size in &sizes {
            let side = (widest - breadth(size)) * align;
            slots.push(if across {
                at + Point::new(along, side)
            } else {
                at + Point::new(side, along)
            });
            along += if across { size.x } else { size.y } + gap;
        }
        return Ok(slots);
    }

    // a grid: cells row by row, each column as wide as its widest
    // child and each row as tall as its tallest
    let columns = props.count_or("columns", 1)?.max(1);
    let gap = match props.get("gap") {
        None => Point::ZERO,
        Some(p) => match p.values.len() {
            1 => {
                let [n] = p.numbers::<1>()?;
                Point::new(n, n)
            }
            _ => {
                let [x, y] = p.numbers::<2>()?;
                Point::new(x, y)
            }
        },
    };
    let sizes = node
        .children
        .iter()
        .map(|c| size_at(c, at))
        .collect::<Result<Vec<_>, _>>()?;
    let rows = sizes.len().div_ceil(columns);
    let mut widths = vec![0.0f64; columns];
    let mut heights = vec![0.0f64; rows];
    for (k, size) in sizes.iter().enumerate() {
        widths[k % columns] = widths[k % columns].max(size.x);
        heights[k / columns] = heights[k / columns].max(size.y);
    }
    let lefts: Vec<f64> = widths
        .iter()
        .scan(0.0, |x, w| {
            let left = *x;
            *x += w + gap.x;
            Some(left)
        })
        .collect();
    let tops: Vec<f64> = heights
        .iter()
        .scan(0.0, |y, h| {
            let top = *y;
            *y += h + gap.y;
            Some(top)
        })
        .collect();
    Ok(sizes
        .iter()
        .enumerate()
        .map(|(k, size)| {
            let (c, r) = (k % columns, k / columns);
            at + Point::new(
                lefts[c] + (widths[c] - size.x) * align,
                tops[r] + (heights[r] - size.y) * align,
            )
        })
        .collect())
}

/// Where a group puts the origin of the nodes in it.
fn group_at(node: &Node, names: &Names, offset: Point) -> Result<Point, Error> {
    let props = Props::new(node, &keys("group"), true)?;
    match props.get("at") {
        Some(p) => at(p, names, offset),
        None => Ok(offset),
    }
}

/// The point an `at` names: two numbers from the origin of the group it
/// sits in, or a shape, row or row cell's top-left corner and two
/// optional numbers from there.
fn at(p: &Property, names: &Names, offset: Point) -> Result<Point, Error> {
    if let [first, ..] = p.values.as_slice()
        && let ValueKind::Number(_) = first.kind
    {
        let [x, y] = p.numbers::<2>()?;
        return Ok(offset + Point::new(x, y));
    }
    // a row's name and one number is a cell, so with three values the
    // two numbers are the offset from the whole row
    let named = match p.values.len() {
        3 => &p.values[..1],
        4 => &p.values[..2],
        _ => &p.values[..],
    };
    let (thing, _) = target(p, named, names, 0)?;
    match &p.values[named.len()..] {
        [] => Ok(thing.origin),
        [dx, dy] => match (&dx.kind, &dy.kind) {
            (ValueKind::Number(dx), ValueKind::Number(dy)) => {
                Ok(thing.origin + Point::new(*dx, *dy))
            }
            _ => Err(p.expected("a name and two numbers")),
        },
        _ => Err(p.expected("two numbers, or a name and two optional numbers")),
    }
}

/// The property names node kind `kind` takes.
fn keys(kind: &str) -> Vec<&'static str> {
    schema::node(kind)
        .expect("every kind the builder reads is in the schema")
        .keys()
}

/// What the nodes of a canvas may refer to by name, and the canvas's
/// default font.
#[derive(Clone)]
struct Names<'a> {
    font: Font,
    /// The `let` names that stand for one color, for markup.
    colors: HashMap<String, Color>,
    palettes: HashMap<&'a str, Palette>,
    shapes: HashMap<&'a str, Shape>,
    rows: HashMap<&'a str, Row>,
    codes: HashMap<&'a str, Code>,
}

/// How a palette draws each cell text it lists.
type Palette = HashMap<String, Symbol>;

#[derive(Clone)]
struct Symbol {
    fill: Color,
    note: Option<String>,
    text_color: Option<Color>,
}

fn twice(what: &str, name: &crate::lang::Word) -> Error {
    Error::new(name.span, format!("two {what} are named `{}`", name.text))
}

fn palette(node: &Node) -> Result<Palette, Error> {
    if let Some(child) = node.children.first() {
        return Err(Error::new(child.kind.span, "a `palette` holds no nodes"));
    }
    let mut out = Palette::new();
    for p in &node.properties {
        if p.key.text != "symbol" {
            return Err(Error::new(
                p.key.span,
                "a `palette` holds only `symbol` lines",
            ));
        }
        let bad = || {
            Error::new(
                p.span,
                "`symbol` takes a string, a fill, and an optional note and text color",
            )
        };
        let [text, fill, rest @ ..] = p.values.as_slice() else {
            return Err(bad());
        };
        let (ValueKind::Str(text), ValueKind::Color(fill)) = (&text.kind, &fill.kind) else {
            return Err(bad());
        };
        let mut symbol = Symbol {
            fill: rgba(fill),
            note: None,
            text_color: None,
        };
        for v in rest {
            match &v.kind {
                ValueKind::Str(n) if symbol.note.is_none() && symbol.text_color.is_none() => {
                    symbol.note = Some(n.clone())
                }
                ValueKind::Color(c) if symbol.text_color.is_none() => {
                    symbol.text_color = Some(rgba(c))
                }
                _ => return Err(bad()),
            }
        }
        out.insert(text.clone(), symbol);
    }
    Ok(out)
}

fn shape(node: &Node, names: &Names, offset: Point) -> Result<Shape, Error> {
    let props = Props::new(node, &keys("box"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let fill = match props.get("fill") {
        Some(p) => p.color_or_none()?,
        None => Some(Color::WHITE),
    };
    let mut label = props.label("text", "text-size", "text-color", names)?;
    if let (Some(label), Some(p)) = (&mut label, props.get("width")) {
        label.width = Some(p.numbers::<1>()?[0]);
    }
    let padding = match props.get("padding") {
        None => Point::ZERO,
        Some(p) => match p.values.len() {
            1 => {
                let [n] = p.numbers::<1>()?;
                Point::new(n, n)
            }
            _ => {
                let [x, y] = p.numbers::<2>()?;
                Point::new(x, y)
            }
        },
    };
    let size = match (props.get("size"), &label) {
        (Some(p), _) => {
            let [w, h] = p.numbers::<2>()?;
            Point::new(w, h)
        }
        (None, Some(label)) => label.size() + padding * 2.0,
        (None, None) => {
            return Err(Error::new(
                node.kind.span,
                format!("`{}` needs `size` or `text`", node.kind.text),
            ));
        }
    };
    let text_align = match props.get("text-align") {
        None => None,
        Some(p) => Some(match p.word()? {
            "left" => HAlign::Left,
            "center" => HAlign::Center,
            "right" => HAlign::Right,
            _ => return Err(Error::new(p.span, "`text-align` is left, center or right")),
        }),
    };
    Ok(Shape {
        origin,
        size,
        oval: node.kind.text == "ellipse",
        radius: props.number_or("radius", 0.0)?,
        fill,
        stroke: props.stroke(Some(Color::BLACK))?,
        label,
        padding,
        text_align,
    })
}

fn row(node: &Node, names: &Names, offset: Point) -> Result<Row, Error> {
    let props = Props::new(node, &keys("row"), true)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let [unit_w, unit_h] = props.require("unit")?.numbers::<2>()?;
    let span = props.count_or("span", 1)?;
    let palette = match props.get("palette") {
        None => None,
        Some(p) => {
            let name = p.word()?;
            Some(
                names
                    .palettes
                    .get(name)
                    .ok_or_else(|| Error::new(p.span, format!("no palette is named `{name}`")))?,
            )
        }
    };
    let notes = match props.get("notes") {
        None => false,
        Some(p) => match p.word()? {
            "show" => true,
            "hide" => false,
            _ => return Err(Error::new(p.span, "`notes` is show or hide")),
        },
    };
    let fill = match props.get("fill") {
        Some(p) => p.color_or_none()?,
        None => Some(Color::WHITE),
    };
    let stroke = props.stroke(Some(Color::BLACK))?;
    let font = props.font_or(names.font)?;
    let label = |text: &str| -> Result<Label, Error> {
        Ok(Label::plain(
            text,
            props.number_or("text-size", TEXT_SIZE)?,
            font,
            props.color_or("text-color", Color::BLACK)?,
        ))
    };
    let note = |text: &str| -> Result<Label, Error> {
        Ok(Label::plain(
            text,
            props.number_or("note-size", TEXT_SIZE * 0.7)?,
            match props.get("note-font") {
                Some(p) => self::font(p)?,
                None => Font::Mono,
            },
            props.color_or("note-color", Color::BLACK)?,
        ))
    };

    let mut cells = Vec::new();
    let mut start = 0;
    let mut push = |text: &str, span: usize, cell: Option<&Props>| -> Result<(), Error> {
        let from_palette = palette.and_then(|p| p.get(text));
        let mut out = RowCell {
            start,
            span,
            fill: from_palette.map_or(fill, |s| Some(s.fill)),
            stroke,
            label: (!text.is_empty()).then(|| label(text)).transpose()?,
            note: match from_palette.and_then(|s| s.note.as_ref()) {
                Some(n) if notes => Some(note(n)?),
                _ => None,
            },
        };
        if let (Some(l), Some(c)) = (&mut out.label, from_palette.and_then(|s| s.text_color)) {
            l.set_color(c);
        }
        if let Some(cell) = cell {
            if let Some(p) = cell.get("fill") {
                out.fill = p.color_or_none()?;
            }
            if cell.get("stroke").is_some()
                || cell.get("stroke-width").is_some()
                || cell.get("dash").is_some()
            {
                out.stroke = cell.stroke(stroke.map(|s| s.color))?;
            }
            if let Some(l) = &mut out.label
                && let Some(p) = cell.get("text-color")
            {
                l.set_color(p.color()?);
            }
            if let Some(p) = cell.get("note") {
                out.note = Some(note(&p.string()?)?);
            }
        }
        start += span;
        cells.push(out);
        Ok(())
    };
    if let Some(p) = props.get("chars") {
        for c in p.string()?.chars() {
            push(c.encode_utf8(&mut [0; 4]), span, None)?;
        }
    }
    if let Some(p) = props.get("cells") {
        for v in &p.values {
            match &v.kind {
                ValueKind::Str(text) => push(text, span, None)?,
                _ => return Err(Error::new(v.span, "`cells` takes strings")),
            }
        }
    }
    for child in &node.children {
        if child.kind.text != "cell" {
            return Err(Error::new(
                child.kind.span,
                format!("a `row` holds `cell` nodes, not `{}`", child.kind.text),
            ));
        }
        let cell = Props::new(child, &keys("cell"), false)?;
        let text = match cell.get("text") {
            Some(p) => p.string()?,
            None => String::new(),
        };
        push(&text, cell.count_or("span", span)?, Some(&cell))?;
    }
    Ok(Row {
        origin,
        unit: Point::new(unit_w, unit_h),
        gap: props.number_or("gap", 0.0)?,
        radius: props.number_or("radius", 0.0)?,
        cells,
    })
}

fn connector(
    node: &Node,
    names: &Names,
    head: bool,
    offset: Point,
    shift: usize,
) -> Result<Connector, Error> {
    let props = Props::new(node, &keys("line"), false)?;
    let from = end(props.require("from")?, names, offset, shift)?;
    let to = end(props.require("to")?, names, offset, shift)?;
    let center = |end: &End| match end {
        End::Point(p) => *p,
        End::Box(s) => s.center(),
    };
    let on_edge = |end: &End, other: &End| match end {
        End::Point(p) => *p,
        End::Box(s) => s.edge_toward(center(other)),
    };
    let stroke = props
        .stroke(Some(Color::BLACK))?
        .unwrap_or(Stroke::solid(Color::BLACK, STROKE_WIDTH));
    Ok(Connector {
        from: on_edge(&from, &to),
        to: on_edge(&to, &from),
        stroke,
        head,
        label: props.label("label", "text-size", "text-color", names)?,
    })
}

fn text(node: &Node, names: &Names, offset: Point) -> Result<Text, Error> {
    let props = Props::new(node, &keys("text"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let align = match props.get("align") {
        None => HAlign::Left,
        Some(p) => match p.word()? {
            "left" => HAlign::Left,
            "center" => HAlign::Center,
            "right" => HAlign::Right,
            _ => return Err(Error::new(p.span, "`align` is left, center or right")),
        },
    };
    let mut label = props
        .label("text", "size", "color", names)?
        .ok_or_else(|| Error::new(node.kind.span, "`text` needs `text`"))?;
    if let Some(p) = props.get("width") {
        label.width = Some(p.numbers::<1>()?[0]);
    }
    Ok(Text {
        origin,
        label,
        align,
    })
}

/// The texts of a table: one `row` line of strings per row, with each
/// string set at its column's x and aligned to it.
fn table(node: &Node, names: &Names, offset: Point) -> Result<Vec<Text>, Error> {
    let (rows, rest): (Vec<&Property>, Vec<&Property>) =
        node.properties.iter().partition(|p| p.key.text == "row");
    let settings = Node {
        properties: rest.into_iter().cloned().collect(),
        ..node.clone()
    };
    let props = Props::new(&settings, &keys("table"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let columns = props.require("columns")?;
    let xs: Vec<f64> = columns
        .values
        .iter()
        .map(|v| match v.kind {
            ValueKind::Number(n) => Ok(n),
            _ => Err(columns.expected("numbers")),
        })
        .collect::<Result<_, _>>()?;
    let aligns: Vec<HAlign> = match props.get("align") {
        None => vec![HAlign::Left; xs.len()],
        Some(p) => p
            .values
            .iter()
            .map(|v| match &v.kind {
                ValueKind::Word(w) if w == "left" => Ok(HAlign::Left),
                ValueKind::Word(w) if w == "center" => Ok(HAlign::Center),
                ValueKind::Word(w) if w == "right" => Ok(HAlign::Right),
                _ => Err(Error::new(v.span, "an alignment is left, center or right")),
            })
            .collect::<Result<_, _>>()?,
    };
    if aligns.len() != xs.len() {
        return Err(Error::new(
            node.kind.span,
            "a `table` takes one `align` per column",
        ));
    }
    let height = props.number_or("row-height", TEXT_SIZE * 1.5)?;
    let size = props.number_or("size", TEXT_SIZE)?;
    let font = props.font_or(names.font)?;
    let color = props.color_or("color", Color::BLACK)?;

    let mut out = Vec::new();
    for (k, row) in rows.iter().enumerate() {
        if row.values.len() > xs.len() {
            return Err(Error::new(
                row.values[xs.len()].span,
                format!("the table has {} columns", xs.len()),
            ));
        }
        for (column, v) in row.values.iter().enumerate() {
            let ValueKind::Str(text) = &v.kind else {
                return Err(Error::new(v.span, "a table's cells are strings"));
            };
            let runs = markup::runs(text, font, color, &names.colors)
                .map_err(|message| Error::new(v.span, message))?;
            out.push(Text {
                origin: origin + Point::new(xs[column], k as f64 * height),
                label: Label {
                    runs,
                    size,
                    width: None,
                },
                align: aligns[column],
            });
        }
    }
    Ok(out)
}

fn code(node: &Node, names: &Names, offset: Point) -> Result<Code, Error> {
    let props = Props::new(node, &keys("code"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let lang = match props.get("lang") {
        None => code::Lang::Text,
        Some(p) => {
            let name = p.word()?;
            code::Lang::named(name)
                .ok_or_else(|| Error::new(p.span, "`lang` is rust, python, acre or text"))?
        }
    };
    let source = props.require("text")?.string()?;
    let color = |key: &str, default: Color| props.color_or(key, default);
    let hex = |v: u32| {
        Color::rgb(
            (v >> 16) as f32 / 255.0,
            ((v >> 8) & 0xff) as f32 / 255.0,
            (v & 0xff) as f32 / 255.0,
        )
    };
    let colors = [
        color("text-color", hex(0x24292f))?,
        color("keyword-color", hex(0xcf222e))?,
        color("type-color", hex(0x953800))?,
        color("string-color", hex(0x0a3069))?,
        color("comment-color", hex(0x6e7781))?,
        color("number-color", hex(0x0550ae))?,
    ];
    let lines = code::highlight(&source, lang)
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|(text, class)| Run {
                    text,
                    font: Font::Mono,
                    color: colors[class as usize],
                })
                .collect()
        })
        .collect();
    let numbers = match props.get("line-numbers") {
        None => None,
        Some(p) => match p.word()? {
            "show" => Some(color("line-number-color", hex(0x8c959f))?),
            "hide" => None,
            _ => return Err(Error::new(p.span, "`line-numbers` is show or hide")),
        },
    };
    let emphasize = match props.get("emphasize") {
        None => Vec::new(),
        Some(p) => p
            .values
            .iter()
            .map(|v| match v.kind {
                ValueKind::Number(n) if n >= 1.0 && n.fract() == 0.0 => Ok(n as usize),
                _ => Err(Error::new(v.span, "`emphasize` takes line numbers from 1")),
            })
            .collect::<Result<_, _>>()?,
    };
    let padding = match props.get("padding") {
        None => Point::new(14.0, 10.0),
        Some(p) => match p.values.len() {
            1 => {
                let [n] = p.numbers::<1>()?;
                Point::new(n, n)
            }
            _ => {
                let [x, y] = p.numbers::<2>()?;
                Point::new(x, y)
            }
        },
    };
    Ok(Code {
        origin,
        lines,
        size: props.number_or("size", 14.0)?,
        padding,
        numbers,
        fill: match props.get("fill") {
            Some(p) => p.color_or_none()?,
            None => Some(hex(0xf6f8fa)),
        },
        stroke: props.stroke(None)?,
        radius: props.number_or("radius", 6.0)?,
        emphasize,
        emphasis: color("emphasis", hex(0xfff3b0))?,
    })
}

fn brace(node: &Node, names: &Names, offset: Point) -> Result<Brace, Error> {
    let props = Props::new(node, &keys("brace"), false)?;
    let side = match props.get("side") {
        Some(p) => side(p)?,
        None => Side::Top,
    };
    let gap = props.number_or("gap", 4.0)?;
    let (from, to) = match props.get("over") {
        Some(p) => {
            let [left, top, right, bottom] = bounds(p, names)?;
            match side {
                Side::Top => (Point::new(left, top - gap), Point::new(right, top - gap)),
                Side::Bottom => (
                    Point::new(left, bottom + gap),
                    Point::new(right, bottom + gap),
                ),
                Side::Left => (Point::new(left - gap, top), Point::new(left - gap, bottom)),
                Side::Right => (
                    Point::new(right + gap, top),
                    Point::new(right + gap, bottom),
                ),
            }
        }
        None => {
            let [x0, y0] = props.require("from")?.numbers::<2>()?;
            let [x1, y1] = props.require("to")?.numbers::<2>()?;
            (offset + Point::new(x0, y0), offset + Point::new(x1, y1))
        }
    };
    let square = match props.get("shape") {
        None => false,
        Some(p) => match p.word()? {
            "curly" => false,
            "square" => true,
            _ => return Err(Error::new(p.span, "`shape` is curly or square")),
        },
    };
    Ok(Brace {
        from,
        to,
        side,
        depth: props.number_or("depth", 10.0)?,
        square,
        stroke: props
            .stroke(Some(Color::BLACK))?
            .unwrap_or(Stroke::solid(Color::BLACK, 1.0)),
        label: props.label("label", "text-size", "text-color", names)?,
    })
}

fn ruler(node: &Node, names: &Names, offset: Point) -> Result<Ruler, Error> {
    let props = Props::new(node, &keys("ruler"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    Ok(Ruler {
        origin,
        unit: props.require("unit")?.numbers::<1>()?[0],
        count: props.count_or("count", 1)?,
        first: props.number_or("first", 0.0)?,
        step: props.number_or("step", 1.0)?,
        every: props.count_or("every", 1)?,
        side: match props.get("side") {
            Some(p) => side(p)?,
            None => Side::Top,
        },
        stroke: props
            .stroke(Some(Color::BLACK))?
            .unwrap_or(Stroke::solid(Color::BLACK, 1.0)),
        label: Label::plain(
            "",
            props.number_or("text-size", TEXT_SIZE * 0.7)?,
            props.font_or(names.font)?,
            props.color_or("text-color", Color::BLACK)?,
        ),
    })
}

/// Where a connector ends: a point, or the outline of a box it stops at.
enum End {
    Point(Point),
    Box(Shape),
}

/// The end a connector's `from` or `to` names. It is two numbers, a
/// shape's name, or a row's name and a cell's index. A side after a
/// name puts the end at the middle of that side.
fn end(p: &Property, names: &Names, offset: Point, shift: usize) -> Result<End, Error> {
    if let [first, ..] = p.values.as_slice()
        && let ValueKind::Number(_) = first.kind
    {
        return p
            .numbers::<2>()
            .map(|[x, y]| End::Point(offset + Point::new(x, y)));
    }
    let (thing, rest) = target(p, &p.values, names, shift)?;
    match rest {
        [] => Ok(End::Box(thing)),
        [v] => match &v.kind {
            ValueKind::Word(w) => {
                let [left, top, right, bottom] = [
                    thing.origin.x,
                    thing.origin.y,
                    thing.origin.x + thing.size.x,
                    thing.origin.y + thing.size.y,
                ];
                let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
                Ok(End::Point(match w.as_str() {
                    "top" => Point::new(cx, top),
                    "bottom" => Point::new(cx, bottom),
                    "left" => Point::new(left, cy),
                    "right" => Point::new(right, cy),
                    _ => return Err(Error::new(v.span, "a side is top, bottom, left or right")),
                }))
            }
            _ => Err(Error::new(v.span, "expected a side")),
        },
        [_, extra, ..] => Err(Error::new(extra.span, "too many values")),
    }
}

/// The left, top, right and bottom of what `over` names: a shape, a
/// whole row, or a row's cells from one index to another.
fn bounds(p: &Property, names: &Names) -> Result<[f64; 4], Error> {
    let corners = |s: &Shape| {
        [
            s.origin.x,
            s.origin.y,
            s.origin.x + s.size.x,
            s.origin.y + s.size.y,
        ]
    };
    let (first, rest) = target(p, &p.values, names, 0)?;
    let [left, top, mut right, mut bottom] = corners(&first);
    match rest {
        [] => {}
        [last] => {
            let mut to = p.values[..1].to_vec();
            to.push(last.clone());
            let (last, _) = target(p, &to, names, 0)?;
            let c = corners(&last);
            right = c[2];
            bottom = c[3];
        }
        [_, extra, ..] => return Err(Error::new(extra.span, "too many values")),
    }
    Ok([left, top, right, bottom])
}

/// The box that the leading values of `values` name, as a shape, and
/// the values after them. A name alone is a shape or a whole row. A
/// row's name and a number is one of its cells.
fn target<'v>(
    p: &Property,
    values: &'v [Value],
    names: &Names,
    shift: usize,
) -> Result<(Shape, &'v [Value]), Error> {
    let Some((name, rest)) = values.split_first() else {
        return Err(p.expected("a name"));
    };
    let ValueKind::Word(word) = &name.kind else {
        return Err(Error::new(name.span, "expected a name"));
    };
    let as_box = |origin: Point, size: Point| Shape {
        origin,
        size,
        oval: false,
        radius: 0.0,
        fill: None,
        stroke: None,
        label: None,
        padding: Point::ZERO,
        text_align: None,
    };
    if let Some(shape) = names.shapes.get(word.as_str()) {
        return Ok((shape.clone(), rest));
    }
    if let Some(code) = names.codes.get(word.as_str()) {
        return match rest.split_first() {
            Some((
                Value {
                    kind: ValueKind::Number(n),
                    span,
                },
                rest,
            )) => {
                let line = *n as usize + shift;
                if n.fract() != 0.0 || *n < 1.0 || line > code.lines.len() {
                    return Err(Error::new(
                        *span,
                        format!("code `{word}` has lines 1 to {}", code.lines.len()),
                    ));
                }
                let (origin, size) = code.line_rect(line);
                Ok((as_box(origin, size), rest))
            }
            _ => Ok((as_box(code.origin, code.extent()), rest)),
        };
    }
    let Some(row) = names.rows.get(word.as_str()) else {
        return Err(Error::new(
            name.span,
            format!("no box, ellipse, row or code is named `{word}`"),
        ));
    };
    match rest.split_first() {
        Some((
            Value {
                kind: ValueKind::Number(n),
                span,
            },
            rest,
        )) => {
            let index = *n as usize + shift;
            if n.fract() != 0.0 || *n < 0.0 || index >= row.cells.len() {
                return Err(Error::new(
                    *span,
                    format!("row `{word}` has cells 0 to {}", row.cells.len() - 1),
                ));
            }
            let (origin, size) = row.cell_rect(index);
            Ok((as_box(origin, size), rest))
        }
        _ => {
            let units: usize = row.cells.iter().map(|c| c.span).sum();
            let size = Point::new(units as f64 * row.unit.x, row.unit.y);
            Ok((as_box(row.origin, size), rest))
        }
    }
}

fn side(p: &Property) -> Result<Side, Error> {
    match p.word()? {
        "top" => Ok(Side::Top),
        "bottom" => Ok(Side::Bottom),
        "left" => Ok(Side::Left),
        "right" => Ok(Side::Right),
        _ => Err(Error::new(p.span, "a side is top, bottom, left or right")),
    }
}

fn font(p: &Property) -> Result<Font, Error> {
    match p.word()? {
        "mono" => Ok(Font::Mono),
        "sans" => Ok(Font::Sans),
        "sans-bold" => Ok(Font::SansBold),
        "sans-italic" => Ok(Font::SansItalic),
        _ => Err(Error::new(
            p.span,
            "`font` is mono, sans, sans-bold or sans-italic",
        )),
    }
}

fn rgba([r, g, b, a]: &[u8; 4]) -> Color {
    Color::rgba(
        *r as f32 / 255.0,
        *g as f32 / 255.0,
        *b as f32 / 255.0,
        *a as f32 / 255.0,
    )
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

    fn number_or(&self, key: &str, default: f64) -> Result<f64, Error> {
        match self.get(key) {
            Some(p) => Ok(p.numbers::<1>()?[0]),
            None => Ok(default),
        }
    }

    fn count_or(&self, key: &str, default: usize) -> Result<usize, Error> {
        match self.get(key) {
            Some(p) => p.count(),
            None => Ok(default),
        }
    }

    fn color_or(&self, key: &str, default: Color) -> Result<Color, Error> {
        match self.get(key) {
            Some(p) => p.color(),
            None => Ok(default),
        }
    }

    fn font_or(&self, default: Font) -> Result<Font, Error> {
        match self.get("font") {
            Some(p) => font(p),
            None => Ok(default),
        }
    }

    /// `stroke`, `stroke-width` and `dash`, or `None` for `stroke none`
    /// or when neither `stroke` nor `color` gives a color.
    fn stroke(&self, color: Option<Color>) -> Result<Option<Stroke>, Error> {
        let color = match self.get("stroke") {
            Some(p) => p.color_or_none()?,
            None => color,
        };
        let width = self.number_or("stroke-width", STROKE_WIDTH)?;
        let dash = match self.get("dash") {
            Some(p) => Some(p.numbers::<2>()?),
            None => None,
        };
        Ok(color.map(|color| Stroke { color, width, dash }))
    }

    fn label(
        &self,
        text: &str,
        size: &str,
        color: &str,
        names: &Names,
    ) -> Result<Option<Label>, Error> {
        let Some(p) = self.get(text) else {
            return Ok(None);
        };
        let runs = markup::runs(
            &p.string()?,
            self.font_or(names.font)?,
            self.color_or(color, Color::BLACK)?,
            &names.colors,
        )
        .map_err(|message| Error::new(p.span, message))?;
        Ok(Some(Label {
            runs,
            size: self.number_or(size, TEXT_SIZE)?,
            width: None,
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

    fn count(&self) -> Result<usize, Error> {
        let n = self.numbers::<1>()?[0];
        if n < 0.0 || n.fract() != 0.0 {
            return Err(self.expected("a whole number"));
        }
        Ok(n as usize)
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
                ValueKind::Color(c) => Ok(Some(rgba(c))),
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
                 stroke-width, dash, radius, text, text-size, text-color, font, width, \
                 padding, text-align, style"
                    .into()
            )
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  arrow { from a; to 1 1; } }").unwrap_err(),
            (2, 16, "no box, ellipse, row or code is named `a`".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  box { at 1; size 2 2; } }").unwrap_err(),
            (2, 9, "`at` takes 2 numbers".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  text { size 2; } }").unwrap_err(),
            (2, 3, "`text` needs `text`".into())
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
            (3, 6, "two things are named `a`".into())
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

    #[test]
    fn a_row_lays_cells_end_to_end_and_takes_fills_from_its_palette() {
        let s = scene(
            r#"canvas { size 200 100;
                palette dna { symbol "A" #ff0000 "01"; }
                row r {
                    at 10 20; unit 10 30; span 2; palette dna; notes show;
                    cells "A" "C";
                    cell { span 1; fill none; }
                }
                brace { over r 0 1; side bottom; gap 0; }
                arrow { from 0 0; to r 2 top; } }"#,
        )
        .unwrap();
        let Kind::Row(row) = &s.objects[0].kind else {
            panic!("expected a row");
        };
        let starts: Vec<(usize, usize)> = row.cells.iter().map(|c| (c.start, c.span)).collect();
        assert_eq!(starts, [(0, 2), (2, 2), (4, 1)]);
        assert_eq!(row.cells[0].fill, Some(Color::rgb(1.0, 0.0, 0.0)));
        assert_eq!(row.cells[0].note.as_ref().unwrap().text(), "01");
        assert_eq!(row.cells[1].fill, Some(Color::WHITE));
        assert_eq!(row.cells[1].note, None);
        assert_eq!(row.cells[2].fill, None);
        let Kind::Brace(brace) = &s.objects[1].kind else {
            panic!("expected a brace");
        };
        assert_eq!(
            (brace.from, brace.to),
            (Point::new(10.0, 50.0), Point::new(50.0, 50.0))
        );
        let Kind::Connector(arrow) = &s.objects[2].kind else {
            panic!("expected an arrow");
        };
        assert_eq!(arrow.to, Point::new(55.0, 20.0));
    }

    #[test]
    fn a_row_cell_past_the_end_is_an_error() {
        assert_eq!(
            scene("canvas { size 10 10;\n row r { at 0 0; unit 1 1; cells \"a\"; }\n arrow { from r 1; to 0 0; } }")
                .unwrap_err(),
            (3, 17, "row `r` has cells 0 to 0".into())
        );
    }

    #[test]
    fn a_group_moves_what_it_holds_and_at_can_start_from_a_name() {
        let s = scene(
            r#"canvas { size 200 200;
                let ink #ff0000;
                style tile { unit 10 10; stroke ink; }
                group {
                    at 100 50;
                    row r { at 0 0; style tile; chars "ab"; }
                    text { at r 5 -20; text "r"; }
                }
                line { from r 0 bottom; to 0 0; repeat 2; } }"#,
        )
        .unwrap();
        let Kind::Row(row) = &s.objects[0].kind else {
            panic!("expected a row");
        };
        assert_eq!(row.origin, Point::new(100.0, 50.0));
        assert_eq!(row.cells.len(), 2);
        assert_eq!(
            row.cells[0].stroke.unwrap().color,
            Color::rgb(1.0, 0.0, 0.0)
        );
        let Kind::Text(text) = &s.objects[1].kind else {
            panic!("expected a text");
        };
        assert_eq!(text.origin, Point::new(105.0, 30.0));
        let froms: Vec<Point> = s.objects[2..]
            .iter()
            .map(|o| match &o.kind {
                Kind::Connector(c) => c.from,
                other => panic!("expected a line, got {other:?}"),
            })
            .collect();
        assert_eq!(froms, [Point::new(105.0, 60.0), Point::new(115.0, 60.0)]);
    }

    #[test]
    fn a_table_sets_each_string_at_its_column_and_row() {
        let s = scene(
            r#"canvas { size 100 100; let red #ff0000;
                table { at 10 20; columns 0 50; align left right; row-height 30;
                    row "a" "{red b}"; row "c"; } }"#,
        )
        .unwrap();
        let placed: Vec<(Point, HAlign, String)> = s
            .objects
            .iter()
            .map(|o| match &o.kind {
                Kind::Text(t) => (t.origin, t.align, t.label.text()),
                other => panic!("expected a text, got {other:?}"),
            })
            .collect();
        assert_eq!(
            placed,
            [
                (Point::new(10.0, 20.0), HAlign::Left, "a".into()),
                (Point::new(60.0, 20.0), HAlign::Right, "b".into()),
                (Point::new(10.0, 50.0), HAlign::Left, "c".into()),
            ]
        );
    }

    #[test]
    fn each_object_knows_its_node_and_a_point_finds_the_smallest() {
        let src = "canvas { size 100 100;
                box big { at 0 0; size 100 100; }
                box small { at 10 10; size 20 20; } }";
        let s = scene(src).unwrap();
        let node = |i: usize| {
            let span = s.objects[i].source;
            &src[span.start..span.end]
        };
        assert_eq!(node(1), "box small { at 10 10; size 20 20; }");
        assert_eq!(s.object_at(Point::new(15.0, 15.0), 0.0), Some(1));
        assert_eq!(s.object_at(Point::new(50.0, 50.0), 0.0), Some(0));
        assert_eq!(s.object_at(Point::new(150.0, 50.0), 0.0), None);
    }

    #[test]
    fn a_box_without_a_size_fits_its_wrapped_text_and_padding() {
        let s = scene(
            r#"canvas { size 400 400;
                box { at 0 0; text "one two three four"; width 60; padding 10 5; text-align left; } }"#,
        )
        .unwrap();
        let Kind::Shape(b) = &s.objects[0].kind else {
            panic!("expected a box");
        };
        let label = b.label.as_ref().unwrap();
        assert_eq!(b.size, label.size() + Point::new(20.0, 10.0));
        assert!(label.size().x <= 60.0);
        assert!(
            label.size().y > 2.0 * label.size * 1.3,
            "the text wrapped to three lines or more"
        );
        assert!(
            scene("canvas { size 10 10; box { at 0 0; } }").is_err(),
            "a box needs a size or text"
        );
    }

    #[test]
    fn panels_number_themselves_and_hold_their_contents_under_a_heading() {
        let s = scene(
            r#"canvas { size 400 400;
                style panel { title-size 20; gap 10; }
                panel { at 10 10; title "first"; box { at 0 0; size 50 20; } }
                panel { at 10 200; title "second"; stroke #000000; padding 5; width 300;
                    box { at 0 0; size 50 20; }
                    caption { text "under"; gap 4; }
                }
                panel { at 300 10; title "aside"; number none; } }"#,
        )
        .unwrap();
        let heading = |i: usize| match &s.objects[i].kind {
            Kind::Text(t) => (t.origin, t.label.text()),
            other => panic!("expected a heading, got {other:?}"),
        };
        assert_eq!(heading(0), (Point::new(10.0, 10.0), "1   first".into()));
        let Kind::Shape(first_box) = &s.objects[1].kind else {
            panic!("expected the box");
        };
        let Kind::Text(h) = &s.objects[0].kind else {
            unreachable!()
        };
        assert_eq!(first_box.origin.y, 10.0 + h.label.size().y + 10.0);

        // a framed panel's `at` is its frame's corner, and its heading
        // sits inside the padding
        let Kind::Shape(frame) = &s.objects[2].kind else {
            panic!("expected the frame");
        };
        assert_eq!(frame.origin, Point::new(10.0, 200.0));
        assert_eq!(heading(3), (Point::new(15.0, 205.0), "2   second".into()));
        let Kind::Text(caption) = &s.objects[5].kind else {
            panic!("expected the caption");
        };
        assert_eq!(caption.origin.y, frame.origin.y + frame.size.y + 4.0);
        assert_eq!(frame.size.x, 300.0);
        assert_eq!(heading(6).1, "aside");
    }

    #[test]
    fn a_stack_places_each_child_after_the_one_before() {
        let s = scene(
            r#"canvas { size 400 400;
                arrow { from 0 0; to b left; }
                stack { at 10 20; gap 5; align center;
                    box a { at 0 0; size 100 20; }
                    box b { at 0 0; size 40 10; }
                    box { at 0 3; size 60 10; }
                } }"#,
        )
        .unwrap();
        let origin = |i: usize| match &s.objects[i].kind {
            Kind::Shape(b) => b.origin,
            other => panic!("expected a box, got {other:?}"),
        };
        assert_eq!(origin(1), Point::new(10.0, 20.0));
        assert_eq!(origin(2), Point::new(40.0, 45.0));
        // a child's own `at` adds to its slot, and its size runs from
        // the slot, so the gap before it is the same
        assert_eq!(origin(3), Point::new(30.0, 63.0));
        let Kind::Connector(arrow) = &s.objects[0].kind else {
            panic!("expected the arrow");
        };
        assert_eq!(arrow.to, Point::new(40.0, 50.0));
    }

    #[test]
    fn a_grid_sizes_columns_and_rows_by_their_largest_child() {
        let s = scene(
            r#"canvas { size 400 400;
                grid { at 0 0; columns 2; gap 10;
                    box { at 0 0; size 30 10; }
                    box { at 0 0; size 10 40; }
                    box { at 0 0; size 50 10; }
                    box { at 0 0; size 10 10; }
                } }"#,
        )
        .unwrap();
        let origins: Vec<Point> = s
            .objects
            .iter()
            .map(|o| match &o.kind {
                Kind::Shape(b) => b.origin,
                other => panic!("expected a box, got {other:?}"),
            })
            .collect();
        assert_eq!(
            origins,
            [
                Point::new(0.0, 0.0),
                Point::new(60.0, 0.0),
                Point::new(0.0, 50.0),
                Point::new(60.0, 50.0),
            ]
        );
    }

    #[test]
    fn a_code_block_highlights_its_lines_and_names_them_from_one() {
        let s = scene(
            r#"canvas { size 400 400;
                code snippet {
                    at 10 10; lang rust; line-numbers show; emphasize 2;
                    text """
                        fn key(&self) -> u64 {
                            *self.state
                        }
                    """;
                }
                arrow { from snippet 2 right; to 390 390; } }"#,
        )
        .unwrap();
        let Kind::Code(code) = &s.objects[0].kind else {
            panic!("expected a code block");
        };
        assert_eq!(code.lines.len(), 3);
        assert_eq!(code.lines[0][0].text, "fn");
        assert_ne!(code.lines[0][0].color, code.lines[0][1].color);
        assert_eq!(code.emphasize, [2]);
        let (line, size) = code.line_rect(2);
        let Kind::Connector(arrow) = &s.objects[1].kind else {
            panic!("expected the arrow");
        };
        assert_eq!(
            arrow.from,
            Point::new(line.x + size.x, line.y + size.y / 2.0)
        );
        assert_eq!(
            scene(
                "canvas { size 10 10;\n code c { text \"a\"; }\n arrow { from c 2 left; to 0 0; } }"
            )
            .unwrap_err()
            .2,
            "code `c` has lines 1 to 1"
        );
    }

    /// The box around everything `s` draws at time 0.
    fn drawn(s: &Scene) -> (Point, Point) {
        bounds_of(&s.objects).unwrap()
    }

    #[test]
    fn an_auto_height_wraps_the_drawing_in_the_margin() {
        let s = scene(
            "canvas { size 300 auto; margin 10;
                box a { at 40 30; size 100 40; } }",
        )
        .unwrap();
        assert_eq!((s.canvas.width, s.canvas.height), (300, 60));
        let (min, max) = drawn(&s);
        // the width is fixed, so the box keeps its x
        assert_eq!((min.x, min.y), (40.0, 10.0));
        assert_eq!(max.y, 50.0);
    }

    #[test]
    fn two_autos_size_the_canvas_to_the_drawing_and_move_it_to_the_margin() {
        let s = scene(
            "canvas { size auto auto;
                box a { at 100 50; size 100 40; stroke none; fill #ff0000; }
                arrow { from a; to 400 90; } }",
        )
        .unwrap();
        let (min, max) = drawn(&s);
        assert_eq!((min.x, min.y), (20.0, 20.0));
        assert_eq!(s.canvas.width as f64, (max.x + 20.0).ceil());
        assert_eq!(s.canvas.height as f64, (max.y + 20.0).ceil());
    }

    #[test]
    fn auto_placement_moves_named_references_with_the_drawing() {
        let s = scene(
            "canvas { size auto auto; margin 0;
                box a { at 100 100; size 40 40; stroke none; fill #ff0000; }
                box b { at 200 100; size 40 40; stroke none; fill #ff0000; }
                arrow { from a; to b; } }",
        )
        .unwrap();
        let Kind::Connector(c) = &s.objects[2].kind else {
            panic!("expected a connector");
        };
        assert_eq!(c.from, Point::new(40.0, 20.0));
        assert_eq!(c.to, Point::new(100.0, 20.0));
    }

    #[test]
    fn a_margin_without_an_auto_size_is_an_error() {
        let (line, col, message) = scene("canvas { size 10 10; margin 5; }").unwrap_err();
        assert_eq!((line, col), (1, 22));
        assert_eq!(message, "`margin` needs an `auto` in `size`");
        assert!(scene("canvas { size auto 10; margin -1; }").is_err());
        assert!(scene("canvas { size auto; }").is_err());
        assert!(scene("canvas { size auto fast; }").is_err());
    }
}

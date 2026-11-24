use std::collections::HashMap;
use std::f64::consts::FRAC_1_SQRT_2;

use super::code::{self, Code};
use super::connector::{self, Dir, End, Follow};
use super::data::{self, Table};
use super::expand::expand;
use super::markup;
use super::schema;
use super::timeline::{self, Later, TextAction};
use super::{
    Brace, Canvas, Color, Connector, Font, HAlign, Head, Kind, Label, LabelSide, Matrix,
    MatrixCell, Merge, Object, Point, Polygon, Polyline, Route, Row, RowCell, Ruler, Run, Scene,
    Shape, Side, Stroke, Target, Text, TextChange,
};
use crate::fonts::{Fonts, Typeface};
use crate::lang::{Error, Node, Program, Property, Span, Value, ValueKind};
use crate::math::MathFont;
use crate::text::LINE_HEIGHT;

const STROKE_WIDTH: f64 = 2.0;
const TEXT_SIZE: f64 = 16.0;
const FPS: u32 = 30;
/// The space `margin` leaves around the drawing of an `auto` canvas.
const AUTO_MARGIN: f64 = 20.0;

/// A font file the canvas lists in `fonts`, for a program to read.
#[derive(Debug, Clone, PartialEq)]
pub struct FontFile {
    /// The name `font` takes for it, or `math` for the math font.
    pub name: String,
    /// The path as written, which a program resolves against the file
    /// the program came from.
    pub path: String,
    /// Where the path is in the source.
    pub span: Span,
}

/// The name that sets the math font in `fonts`.
const MATH_FONT: &str = "math";

/// The font files `program` lists, in source order. A program reads
/// them all and passes their bytes, in this order, to [`build`].
pub fn font_files(program: &Program) -> Result<Vec<FontFile>, Error> {
    match program.nodes.as_slice() {
        [canvas, ..] if canvas.kind.text == "canvas" => listed_fonts(canvas),
        _ => Ok(Vec::new()),
    }
}

/// The files of the `fonts` node of `canvas`.
fn listed_fonts(canvas: &Node) -> Result<Vec<FontFile>, Error> {
    let mut out: Vec<FontFile> = Vec::new();
    let mut nodes = canvas.children.iter().filter(|n| n.kind.text == "fonts");
    let Some(node) = nodes.next() else {
        return Ok(out);
    };
    if let Some(extra) = nodes.next() {
        return Err(Error::new(extra.kind.span, "a canvas holds one `fonts`"));
    }
    if let Some(name) = &node.name {
        return Err(Error::new(name.span, "`fonts` takes no name"));
    }
    if let Some(child) = node.children.first() {
        return Err(Error::new(child.kind.span, "`fonts` holds no nodes"));
    }
    for p in &node.properties {
        let name = p.key.text.as_str();
        if BUILT_IN_FONTS.contains(&name) {
            return Err(Error::new(
                p.key.span,
                format!("`{name}` is a font acre embeds, so it cannot be a font name"),
            ));
        }
        if out.iter().any(|f| f.name == name) {
            return Err(twice("fonts", &p.key));
        }
        let path = match p.values.as_slice() {
            [
                Value {
                    kind: ValueKind::Str(path),
                    span,
                },
            ] => (path.clone(), *span),
            _ => return Err(p.expected("a quoted path to a font file")),
        };
        out.push(FontFile {
            name: name.into(),
            path: path.0,
            span: path.1,
        });
    }
    Ok(out)
}

const BUILT_IN_FONTS: [&str; 5] = ["mono", "mono-bold", "sans", "sans-bold", "sans-italic"];

/// The fonts `listed` names, from their files' `bytes` in the same
/// order.
fn read_fonts(listed: &[FontFile], bytes: &[Vec<u8>], canvas: &Node) -> Result<Fonts, Error> {
    if listed.len() != bytes.len() {
        return Err(Error::new(
            canvas.span,
            format!(
                "`fonts` lists {} files and {} were read",
                listed.len(),
                bytes.len()
            ),
        ));
    }
    let mut custom = Vec::new();
    let mut math = None;
    for (file, data) in listed.iter().zip(bytes) {
        let data: std::sync::Arc<[u8]> = data.as_slice().into();
        let bad = |why: &str| Error::new(file.span, format!("`{}` {why}", file.path));
        if file.name == MATH_FONT {
            math = Some(MathFont::parse(data).map_err(bad)?);
        } else {
            let face = Typeface::parse(data).ok_or_else(|| bad("is not a font file"))?;
            custom.push((file.name.clone(), face));
        }
    }
    Ok(Fonts::new(custom, math))
}

/// The scene a program describes. `font_bytes` holds the contents of
/// each file [`font_files`] lists, and `data_bytes` of each file
/// [`data_files`](super::data_files) lists, in that order.
pub fn build(
    program: &Program,
    font_bytes: &[Vec<u8>],
    data_bytes: &[Vec<u8>],
) -> Result<Scene, Error> {
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
    let (canvas, colors, badge) = expand(canvas)?;
    let canvas = &canvas;
    let fonts = read_fonts(&listed_fonts(canvas)?, font_bytes, canvas)?;
    let tables = data::read(&data::listed(canvas)?, data_bytes, canvas)?;
    let texts = timeline::texts(canvas)?;
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
            Some(p) => font(p, &fonts)?,
            None => Font::Mono,
        },
        colors,
        badge: Badge::of_style(badge, canvas.span, &fonts)?,
        fonts: fonts.clone(),
        tables: &tables,
        texts: &texts,
        palettes: HashMap::new(),
        shapes: HashMap::new(),
        rows: HashMap::new(),
        matrices: HashMap::new(),
        codes: HashMap::new(),
        outlines: HashMap::new(),
        sources: HashMap::new(),
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

    let (mut objects, mut places) = add_canvas_objects(canvas, &names, Point::ZERO)?;

    // an `auto` side is the box around what is drawn plus the margin
    // on both sides, so the drawing moves to start at the margin
    let mut offset = Point::ZERO;
    let mut sides = [width.unwrap_or(0.0), height.unwrap_or(0.0)];
    if width.is_none() || height.is_none() {
        let (min, max) = bounds_of(&fonts, &objects).unwrap_or((Point::ZERO, Point::ZERO));
        if width.is_none() {
            sides[0] = (max.x - min.x + 2.0 * margin).ceil();
            offset.x = margin - min.x;
        }
        if height.is_none() {
            sides[1] = (max.y - min.y + 2.0 * margin).ceil();
            offset.y = margin - min.y;
        }
        (objects, places) = add_canvas_objects(canvas, &names, offset)?;
    }

    let mut laters = Vec::new();
    shown_later(canvas, &places, &mut laters)?;
    let last = timeline::apply(canvas, &mut objects, &fonts, &laters)?;
    let duration = match props.get("duration") {
        Some(p) => {
            let d = match p.values.as_slice() {
                [
                    Value {
                        kind: ValueKind::Time(d),
                        ..
                    },
                ] if *d >= 0.0 => *d,
                _ => return Err(p.expected("a time in seconds, such as `4s`")),
            };
            if d < last {
                return Err(Error::new(
                    p.span,
                    format!("the timeline runs to {last}s, past this `duration`"),
                ));
            }
            d
        }
        None => last,
    };
    let sections = timeline::sections(canvas, duration)?;

    Ok(Scene {
        canvas: Canvas {
            width: pixels(sides[0], size)?,
            height: pixels(sides[1], size)?,
            fps: FPS,
            duration,
            background,
        },
        objects,
        fonts,
        sections,
    })
}

/// The objects of the canvas node, drawn with its origin at `offset`,
/// and the slot of each child of a stack or a grid.
fn add_canvas_objects<'a>(
    canvas: &'a Node,
    names: &Names<'a>,
    offset: Point,
) -> Result<(Vec<Object>, Places), Error> {
    // named shapes and rows, so that a node may name any of them
    // wherever it sits in the file
    let mut names = names.clone();
    let mut places = Places::new();
    name_things(&canvas.children, offset, &mut names, &mut places)?;
    let mut objects = Vec::new();
    let mut counts = Counts::default();
    let pass = Pass {
        names: &names,
        places: &places,
        measuring: false,
    };
    add_objects(&canvas.children, offset, &pass, &mut counts, &mut objects)?;
    Ok((objects, places))
}

/// Every child of a stack in `parent` and below that sets `space shown`,
/// where `places` holds the slot of each child of a stack.
fn shown_later<'a>(
    parent: &'a Node,
    places: &Places,
    out: &mut Vec<Later<'a>>,
) -> Result<(), Error> {
    let stack = parent.kind.text == "stack";
    let across = stack && stack_across(&Props::new(parent, &keys("stack"), true)?)?;
    for (k, child) in parent.children.iter().enumerate() {
        if let Some(space) = child.properties.iter().find(|p| p.key.text == "space") {
            match space.word()? {
                "always" => {}
                "shown" if stack => {
                    let after = &parent.children[k + 1..];
                    let shift = match after.first() {
                        Some(next) => {
                            let d = places[&next.span.start] - places[&child.span.start];
                            if across {
                                Point::new(d.x, 0.0)
                            } else {
                                Point::new(0.0, d.y)
                            }
                        }
                        None => Point::ZERO,
                    };
                    out.push(Later {
                        node: child,
                        space,
                        shift,
                        after,
                    });
                }
                "shown" => {
                    return Err(Error::new(
                        space.span,
                        "`space shown` is for a child of a stack",
                    ));
                }
                _ => return Err(Error::new(space.span, "`space` is always or shown")),
            }
        }
        shown_later(child, places, out)?;
    }
    Ok(())
}

/// Whether the stack whose properties are `props` runs right rather
/// than down.
fn stack_across(props: &Props) -> Result<bool, Error> {
    match props.get("direction") {
        None => Ok(false),
        Some(p) => match p.word()? {
            "down" => Ok(false),
            "right" => Ok(true),
            _ => Err(Error::new(p.span, "`direction` is down or right")),
        },
    }
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

/// Build every named shape, row and matrix in `nodes` and the groups among them,
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
            || names.matrices.contains_key(key)
            || names.codes.contains_key(key)
            || names.outlines.contains_key(key);
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
            "matrix" => {
                let matrix = matrix(node, names, offset)?;
                names.matrices.insert(key, matrix);
                used
            }
            "code" => {
                let code = code(node, names, offset)?;
                names.codes.insert(key, code);
                used
            }
            "polygon" => {
                let polygon = polygon(node, names, offset)?;
                names.outlines.insert(key, extent(&polygon.points));
                used
            }
            "polyline" => {
                let polyline = polyline(node, names, offset)?;
                names.outlines.insert(key, extent(&polyline.points));
                used
            }
            _ => false,
        };
        if taken {
            return Err(twice("things", name));
        }
        if matches!(
            node.kind.text.as_str(),
            "box" | "ellipse" | "polygon" | "polyline" | "row" | "matrix" | "code"
        ) {
            names.sources.insert(key, node.span);
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
    counts: &mut Counts,
    out: &mut Vec<Object>,
) -> Result<(), Error> {
    for node in nodes {
        let start = out.len();
        add_node(node, offset, pass, counts, out)?;
        add_tag(node, pass.names, &mut counts.tags, start, out)?;
    }
    Ok(())
}

/// The space between a tag and its node.
const TAG_GAP: f64 = 8.0;

/// Append the tag `node` sets, if it sets one, left of what `out[start..]`
/// draws and centered on it. `tags` counts the `tag auto` nodes so far.
fn add_tag(
    node: &Node,
    names: &Names,
    tags: &mut usize,
    start: usize,
    out: &mut Vec<Object>,
) -> Result<(), Error> {
    let Some(p) = node.properties.iter().find(|p| p.key.text == "tag") else {
        return Ok(());
    };
    let text = match p.values.as_slice() {
        [
            Value {
                kind: ValueKind::Word(w),
                ..
            },
        ] if w == "auto" => {
            *tags += 1;
            format!("({})", letters(*tags))
        }
        [
            Value {
                kind: ValueKind::Str(s),
                ..
            },
        ] => s.clone(),
        _ => return Err(p.expected("a quoted text or `auto`")),
    };
    let Some((min, max)) = bounds_of(&names.fonts, &out[start..]) else {
        return Ok(());
    };
    let props = Props::new(node, &keys(&node.kind.text), true)?;
    let font = match props.get("tag-font") {
        Some(p) => font(p, &names.fonts)?,
        None => Font::SansBold,
    };
    let color = props.color_or("tag-color", Color::BLACK)?;
    let runs = markup::runs(&text, font, color, &names.colors)
        .map_err(|message| Error::new(p.span, message))?;
    let label = Label {
        runs,
        size: props.number_or("tag-size", 14.0)?,
        width: None,
        justify: false,
        fonts: names.fonts.clone(),
    };
    let size = label.size();
    let text = Text {
        origin: Point::new(min.x - TAG_GAP - size.x, (min.y + max.y - size.y) * 0.5),
        label,
        align: HAlign::Left,
        rotate: 0.0,
    };
    out.push(Object {
        tag: true,
        ..Object::new(Kind::Text(text)).from_source(node.span)
    });
    Ok(())
}

/// The letters of the `n`th tag from 1: a to z, then aa, ab and on.
fn letters(mut n: usize) -> String {
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'a' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).expect("the letters a to z are ASCII")
}

/// How many panels and `tag auto` nodes a pass has numbered so far.
#[derive(Default)]
struct Counts {
    panels: usize,
    tags: usize,
}

/// Append the objects `node` draws to `out`.
fn add_node(
    node: &Node,
    offset: Point,
    pass: &Pass,
    counts: &mut Counts,
    out: &mut Vec<Object>,
) -> Result<(), Error> {
    let names = pass.names;
    let named = node.name.as_ref().map(|n| n.text.as_str());
    let kind = match node.kind.text.as_str() {
        "palette" | "fonts" | "data" | "timeline" => return Ok(()),
        "caption" => {
            // a caption goes under everything before it
            let below = bounds_of(&names.fonts, out).map_or(offset.y, |(_, max)| max.y);
            let t = caption(node, names, Point::new(offset.x, below))?;
            out.push(Object::new(Kind::Text(t)).from_source(node.span));
            return Ok(());
        }
        "group" => {
            let at = group_at(node, names, offset)?;
            add_objects(&node.children, at, pass, counts, out)?;
            return Ok(());
        }
        "panel" => {
            add_panel(node, pass, offset, counts, out)?;
            return Ok(());
        }
        "stack" | "grid" => {
            for child in &node.children {
                let slot = pass.places[&child.span.start];
                add_objects(std::slice::from_ref(child), slot, pass, counts, out)?;
            }
            return Ok(());
        }
        "line" | "arrow" => {
            let head = match node.kind.text.as_str() {
                "arrow" => Head::Triangle,
                _ => Head::None,
            };
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
            return Ok(());
        }
        "sweep" => {
            match sweep(node, names, offset) {
                Ok(c) => out.push(Object::new(Kind::Connector(c)).from_source(node.span)),
                // a measure leaves out what names a thing not
                // placed yet
                Err(_) if pass.measuring => {}
                Err(e) => return Err(e),
            }
            return Ok(());
        }
        "merge" => {
            match merge(node, names, offset) {
                Ok(m) => out.push(Object::new(Kind::Merge(m)).from_source(node.span)),
                // a measure leaves out what names a thing not
                // placed yet
                Err(_) if pass.measuring => {}
                Err(e) => return Err(e),
            }
            return Ok(());
        }
        "box" | "ellipse" => match named {
            Some(name) => Kind::Shape(names.shapes[name].clone()),
            None => Kind::Shape(shape(node, names, offset)?),
        },
        "polygon" => Kind::Polygon(polygon(node, names, offset)?),
        "polyline" => Kind::Polyline(polyline(node, names, offset)?),
        "row" => match named {
            Some(name) => Kind::Row(names.rows[name].clone()),
            None => Kind::Row(row(node, names, offset)?),
        },
        "matrix" => match named {
            Some(name) => Kind::Matrix(names.matrices[name].clone()),
            None => Kind::Matrix(matrix(node, names, offset)?),
        },
        "code" => match named {
            Some(name) => Kind::Code(names.codes[name].clone()),
            None => Kind::Code(code(node, names, offset)?),
        },
        "text" => {
            let (badge, t) = text(node, names, offset)?;
            if let Some(badge) = badge {
                out.push(Object::new(Kind::Shape(badge)).from_source(node.span));
            }
            Kind::Text(t)
        }
        "marker" => match marker(node, names, offset) {
            Ok(m) => Kind::Shape(m),
            Err(_) if pass.measuring => return Ok(()),
            Err(e) => return Err(e),
        },
        "math" => Kind::Text(math(node, names, offset)?),
        "table" => {
            for t in table(node, names, offset)? {
                out.push(Object::new(Kind::Text(t)).from_source(node.span));
            }
            return Ok(());
        }
        "brace" => match brace(node, names, offset) {
            Ok(b) => Kind::Brace(b),
            Err(_) if pass.measuring => return Ok(()),
            Err(e) => return Err(e),
        },
        "ruler" => Kind::Ruler(ruler(node, names, offset)?),
        other => {
            return Err(Error::new(
                node.kind.span,
                format!(
                    "unknown node `{other}`; a canvas holds box, ellipse, polygon, polyline, line, arrow, \
                         text, math, marker, table, row, matrix, code, brace, ruler, group, panel, caption, stack, grid, \
                         palette and style"
                ),
            ));
        }
    };
    let texts = text_changes(node, &kind, names)?;
    out.push(Object {
        texts,
        ..Object::new(kind).from_source(node.span)
    });
    Ok(())
}

/// The texts the timeline's `text` actions give `node`, whose object is
/// `kind`, in order of start.
fn text_changes(node: &Node, kind: &Kind, names: &Names) -> Result<Vec<TextChange>, Error> {
    let Some(actions) = names.texts.get(&node.span) else {
        return Ok(Vec::new());
    };
    let props = Props::new(node, &keys(&node.kind.text), node.kind.text == "row")?;
    let labels = match kind {
        Kind::Shape(s) => changed_labels(
            &props,
            s.label.as_ref(),
            "text-size",
            "text-color",
            actions,
            names,
        )?,
        Kind::Text(t) => changed_labels(&props, Some(&t.label), "size", "color", actions, names)?,
        Kind::Row(r) => {
            let mut labels = Vec::new();
            for a in actions {
                let cell = a
                    .cell
                    .expect("`timeline::texts` changes a row's text by cell");
                let Some(cell) = r.cells.get(cell) else {
                    let name = node.name.as_ref().map_or("", |n| n.text.as_str());
                    return Err(Error::new(
                        a.span,
                        format!("row `{name}` has cells 0 to {}", r.cells.len() - 1),
                    ));
                };
                labels.push(match &cell.label {
                    Some(l) => l.with_text(a.text.as_str()),
                    None => Label::plain(
                        &names.fonts,
                        a.text.as_str(),
                        props.number_or("text-size", TEXT_SIZE)?,
                        props.font_or(names.font, &names.fonts)?,
                        props.color_or("text-color", Color::BLACK)?,
                    ),
                });
            }
            labels
        }
        _ => unreachable!("`timeline::texts` lets through only boxes, ellipses, texts and rows"),
    };
    Ok(actions
        .iter()
        .zip(labels)
        .map(|(a, label)| TextChange {
            cell: a.cell,
            start: a.start,
            label,
        })
        .collect())
}

/// `label` with its text set to that of each of `actions`, in the font
/// of `props` and its `color`. A node without text takes the text size
/// of its `size` and the wrap width of its `width`.
fn changed_labels(
    props: &Props,
    label: Option<&Label>,
    size: &str,
    color: &str,
    actions: &[TextAction],
    names: &Names,
) -> Result<Vec<Label>, Error> {
    let base = match label {
        Some(l) => l.clone(),
        None => Label {
            runs: Vec::new(),
            size: props.number_or(size, TEXT_SIZE)?,
            width: props.number("width")?,
            justify: false,
            fonts: names.fonts.clone(),
        },
    };
    let font = props.font_or(names.font, &names.fonts)?;
    let color = props.color_or(color, Color::BLACK)?;
    actions
        .iter()
        .map(|a| {
            let runs = markup::runs(&a.text, font, color, &names.colors)
                .map_err(|message| Error::new(a.span, message))?;
            Ok(Label {
                runs,
                ..base.clone()
            })
        })
        .collect()
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
        Some(p) => font(p, &names.fonts)?,
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
                    bullet: false,
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
                justify: false,
                fonts: names.fonts.clone(),
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
    counts: &mut Counts,
    out: &mut Vec<Object>,
) -> Result<(), Error> {
    let names = pass.names;
    let props = Props::new(node, &keys("panel"), true)?;
    let numbered = match props.get("number") {
        Some(p) => p.word()? != "none",
        None => true,
    };
    let number = numbered.then(|| {
        counts.panels += 1;
        counts.panels
    });
    let place = panel_place(node, names, offset, number)?;
    let start = out.len();
    if let Some(heading) = place.heading {
        let text = Text {
            origin: place.at,
            label: heading,
            align: HAlign::Left,
            rotate: 0.0,
        };
        out.push(Object::new(Kind::Text(text)).from_source(node.span));
    }
    let (captions, contents): (Vec<&Node>, Vec<&Node>) =
        node.children.iter().partition(|c| c.kind.text == "caption");
    let contents: Vec<Node> = contents.into_iter().cloned().collect();
    add_objects(&contents, place.contents, pass, counts, out)?;

    let max = bounds_of(&names.fonts, &out[start..]).map_or(place.at, |(_, max)| max);
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
        rotate: 0.0,
    })
}

/// The box around what `objects` draw at time 0, at the widest text
/// the timeline gives each.
fn bounds_of(fonts: &Fonts, objects: &[Object]) -> Option<(Point, Point)> {
    let mut prims = Vec::new();
    for o in objects {
        o.draw_laid_out(&mut prims);
    }
    super::primitive::bounds(fonts, &prims)
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
    add_objects(node, slot, &pass, &mut Counts::default(), &mut objects)?;
    Ok(bounds_of(&names.fonts, &objects))
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
        let across = stack_across(&props)?;
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
/// sits in, or a thing's top-left corner or the point F of the way along
/// one of its sides, and two optional numbers from there. The thing is a
/// shape, a row, a row's cell, a matrix, or with a side a matrix's cell.
fn at(p: &Property, names: &Names, offset: Point) -> Result<Point, Error> {
    if let [first, ..] = p.values.as_slice()
        && let ValueKind::Number(_) = first.kind
    {
        let [x, y] = p.numbers::<2>()?;
        return Ok(offset + Point::new(x, y));
    }
    let named = at_named(&p.values);
    let (thing, cell, left) = target(p, named, names, 0)?;
    if let (Some(_), [v, ..]) = (cell, left)
        && let ValueKind::Word(name) = &p.values[0].kind
        && names.matrices.contains_key(name.as_str())
    {
        return Err(Error::new(
            v.span,
            "`at` takes a matrix's name and two optional numbers, or a cell, a side and how far along it",
        ));
    }
    let (point, rest) = match &p.values[named.len()..] {
        [side, f, rest @ ..] if matches!(side.kind, ValueKind::Word(_)) => {
            (along_side(&thing, side, f)?, rest)
        }
        [side] if matches!(side.kind, ValueKind::Word(_)) => {
            return Err(Error::new(
                side.span,
                "a side in `at` takes how far along it, from 0 to 1",
            ));
        }
        rest => (thing.origin, rest),
    };
    match rest {
        [] => Ok(point),
        [dx, dy] => match (&dx.kind, &dy.kind) {
            (ValueKind::Number(dx), ValueKind::Number(dy)) => Ok(point + Point::new(*dx, *dy)),
            _ => Err(p.expected("a name and two numbers")),
        },
        _ => Err(p.expected("two numbers, or a name and two optional numbers")),
    }
}

/// The leading values of an `at` that name a thing: those before a side,
/// or without one, those before the last two numbers.
pub(super) fn at_named(values: &[Value]) -> &[Value] {
    if let Some(k) = values
        .iter()
        .skip(1)
        .position(|v| matches!(v.kind, ValueKind::Word(_)))
    {
        return &values[..k + 1];
    }
    // a row's name and one number is a cell, so with three values the
    // two numbers are the offset from the whole row
    match values.len() {
        3 => &values[..1],
        4 => &values[..2],
        _ => values,
    }
}

/// The point F of the way along side `side` of `thing`, left to right
/// on the top and bottom and top to bottom on the left and right. On an
/// ellipse the point lies on the outline.
fn along_side(thing: &Shape, side: &Value, f: &Value) -> Result<Point, Error> {
    let side = match &side.kind {
        ValueKind::Word(w) => side_named(w)
            .ok_or_else(|| Error::new(side.span, "a side is top, bottom, left or right"))?,
        _ => return Err(Error::new(side.span, "expected a side")),
    };
    let f = match f.kind {
        ValueKind::Number(n) if (0.0..=1.0).contains(&n) => n,
        _ => {
            return Err(Error::new(
                f.span,
                "how far along a side is a number from 0 to 1",
            ));
        }
    };
    let out = side.outward();
    let half = thing.size * 0.5;
    let t = 2.0 * f - 1.0;
    let across = match side {
        Side::Top | Side::Bottom => Point::new(t * half.x, 0.0),
        Side::Left | Side::Right => Point::new(0.0, t * half.y),
    };
    // t is the point's distance from the side's middle in half
    // sides; on an ellipse x^2/a^2 + y^2/b^2 = 1, so the outline
    // there is sqrt(1 - t^2) of the half depth from the center
    let depth = if thing.oval {
        (1.0 - t * t).sqrt()
    } else {
        1.0
    };
    Ok(thing.center() + across + Point::new(out.x * half.x, out.y * half.y) * depth)
}

/// How far a node of `size` moves so that the point its `place` names
/// lands where the point `default` names sat. `default` is a fraction
/// of the width and height from the top-left corner.
fn placed(props: &Props, size: Point, default: Point) -> Result<Point, Error> {
    let Some(p) = props.get("place") else {
        return Ok(Point::ZERO);
    };
    let (x, y) = match p.word()? {
        "top-left" => (0.0, 0.0),
        "top" => (0.5, 0.0),
        "top-right" => (1.0, 0.0),
        "left" => (0.0, 0.5),
        "center" => (0.5, 0.5),
        "right" => (1.0, 0.5),
        "bottom-left" => (0.0, 1.0),
        "bottom" => (0.5, 1.0),
        "bottom-right" => (1.0, 1.0),
        _ => {
            return Err(Error::new(
                p.span,
                "`place` is center, top, bottom, left, right, top-left, top-right, \
                 bottom-left or bottom-right",
            ));
        }
    };
    Ok(Point::new(
        size.x * (default.x - x),
        size.y * (default.y - y),
    ))
}

/// The property names node kind `kind` takes.
pub(super) fn keys(kind: &str) -> Vec<&'static str> {
    schema::node(kind)
        .expect("every kind the builder reads is in the schema")
        .keys()
}

/// What the nodes of a canvas may refer to by name, and the canvas's
/// default font.
#[derive(Clone)]
struct Names<'a> {
    font: Font,
    fonts: Fonts,
    /// The `let` names that stand for one color, for markup.
    colors: HashMap<String, Color>,
    /// The look of the number on a `marker` and on numbered `text`.
    badge: Badge,
    /// The table of each `data` node, by its name.
    tables: &'a HashMap<String, Table>,
    /// The timeline's `text` actions on each node, by the node's source.
    texts: &'a HashMap<Span, Vec<TextAction>>,
    palettes: HashMap<&'a str, Palette>,
    shapes: HashMap<&'a str, Shape>,
    rows: HashMap<&'a str, Row>,
    matrices: HashMap<&'a str, Matrix>,
    codes: HashMap<&'a str, Code>,
    /// The box around each named polygon and polyline, as its top-left
    /// corner and size.
    outlines: HashMap<&'a str, (Point, Point)>,
    /// The node of each named shape, polygon, polyline, row, matrix and
    /// code.
    sources: HashMap<&'a str, Span>,
}

impl Names<'_> {
    /// The node of the thing `name` names, which
    /// [`target`] has found.
    fn source_of(&self, name: &Value) -> Span {
        match &name.kind {
            ValueKind::Word(word) => self.sources[word.as_str()],
            _ => unreachable!("a target is named by a word"),
        }
    }
}

/// How a palette draws each cell text it lists.
type Palette = HashMap<String, Symbol>;

#[derive(Clone)]
struct Symbol {
    fill: Color,
    note: Option<String>,
    text_color: Option<Color>,
}

pub(super) fn twice(what: &str, name: &crate::lang::Word) -> Error {
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
    let text_align = match props.get("text-align") {
        None => None,
        Some(p) => Some(match p.word()? {
            "left" => HAlign::Left,
            "center" => HAlign::Center,
            "right" => HAlign::Right,
            "justify" => {
                if let Some(label) = &mut label {
                    justify(p, label)?;
                }
                HAlign::Left
            }
            _ => {
                return Err(Error::new(
                    p.span,
                    "`text-align` is left, center, right or justify",
                ));
            }
        }),
    };
    let changed = match names.texts.get(&node.span) {
        Some(actions) => changed_labels(
            &props,
            label.as_ref(),
            "text-size",
            "text-color",
            actions,
            names,
        )?,
        None => Vec::new(),
    };
    let size = match props.get("size") {
        Some(p) => {
            let [w, h] = p.numbers::<2>()?;
            Point::new(w, h)
        }
        // the widest text the timeline gives the box sizes it, so
        // the box keeps one size as its text changes
        None => match label
            .iter()
            .chain(&changed)
            .map(Label::size)
            .reduce(|a, b| Point::new(a.x.max(b.x), a.y.max(b.y)))
        {
            Some(widest) => widest + padding * 2.0,
            None => {
                return Err(Error::new(
                    node.kind.span,
                    format!("`{}` needs `size` or `text`", node.kind.text),
                ));
            }
        },
    };

    Ok(Shape {
        origin: origin + placed(&props, size, Point::ZERO)?,
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

fn polygon(node: &Node, names: &Names, offset: Point) -> Result<Polygon, Error> {
    let props = Props::new(node, &keys("polygon"), false)?;
    let p = props.require("points")?;
    let points = points(p, names, offset)?;
    if points.len() < 3 {
        return Err(Error::new(
            p.span,
            "a `polygon` needs at least three points",
        ));
    }
    let fill = match props.get("fill") {
        Some(p) => p.color_or_none()?,
        None => Some(Color::WHITE),
    };
    Ok(Polygon {
        points,
        fill,
        stroke: props.stroke(Some(Color::BLACK))?,
    })
}

fn polyline(node: &Node, names: &Names, offset: Point) -> Result<Polyline, Error> {
    let props = Props::new(node, &keys("polyline"), false)?;
    let p = props.require("points")?;
    let mut points = points(p, names, offset)?;
    if points.len() < 2 {
        return Err(Error::new(p.span, "a `polyline` needs at least two points"));
    }
    points.dedup();
    let stroke = props
        .stroke(Some(Color::BLACK))?
        .unwrap_or(Stroke::solid(Color::BLACK, STROKE_WIDTH));
    Ok(Polyline {
        points,
        stroke,
        head: props.choice("head", Head::None, &HEADS)?,
        tail: props.choice("tail", Head::None, &HEADS)?,
        head_size: props.number("head-size")?,
    })
}

/// The points `points` lists as `X Y X Y ...`, or takes from a table
/// as `DATA XCOL YCOL`, from `offset`.
fn points(p: &Property, names: &Names, offset: Point) -> Result<Vec<Point>, Error> {
    if let Some(Value {
        kind: ValueKind::Word(name),
        span,
    }) = p.values.first()
    {
        return data_table(names, name, *span)?.points(p, offset);
    }
    let mut numbers = Vec::with_capacity(p.values.len());
    for v in &p.values {
        match v.kind {
            ValueKind::Number(n) => numbers.push(n),
            _ => return Err(Error::new(v.span, "`points` takes numbers")),
        }
    }
    if numbers.len() % 2 == 1 {
        return Err(Error::new(
            p.span,
            "`points` takes pairs of numbers, `X Y X Y ...`, and has one left over",
        ));
    }
    let (pairs, _) = numbers.as_chunks::<2>();
    Ok(pairs
        .iter()
        .map(|&[x, y]| offset + Point::new(x, y))
        .collect())
}

/// The table of the `data` named `name`, which `span` names.
fn data_table<'n>(names: &Names<'n>, name: &str, span: Span) -> Result<&'n Table, Error> {
    names
        .tables
        .get(name)
        .ok_or_else(|| Error::new(span, format!("no `data` is named `{name}`")))
}

/// The box around `points`, as its top-left corner and size.
fn extent(points: &[Point]) -> (Point, Point) {
    let mut min = Point::new(f64::INFINITY, f64::INFINITY);
    let mut max = Point::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        min = Point::new(min.x.min(p.x), min.y.min(p.y));
        max = Point::new(max.x.max(p.x), max.y.max(p.y));
    }
    (min, max - min)
}

fn row(node: &Node, names: &Names, offset: Point) -> Result<Row, Error> {
    let props = Props::new(node, &keys("row"), true)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let follow = props.get("at").and_then(|p| cell_named(p, names));
    let [unit_w, unit_h] = props.require("unit")?.numbers::<2>()?;
    let span = props.count_or("span", 1)?;
    let weighted = match (props.get("width"), props.get("weights")) {
        (Some(w), Some(p)) => Some((w.numbers::<1>()?[0], weights(p, names)?, p)),
        (None, None) => None,
        (Some(w), None) => return Err(Error::new(w.span, "`width` goes with `weights`")),
        (None, Some(p)) => return Err(Error::new(p.span, "`weights` goes with `width`")),
    };
    if let (Some(_), Some(p)) = (&weighted, props.get("span")) {
        return Err(Error::new(
            p.span,
            "a row with `weights` gives each cell one weight and takes no `span`",
        ));
    }
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
    let font = props.font_or(names.font, &names.fonts)?;
    let label = |text: &str| -> Result<Label, Error> {
        Ok(Label::plain(
            &names.fonts,
            text,
            props.number_or("text-size", TEXT_SIZE)?,
            font,
            props.color_or("text-color", Color::BLACK)?,
        ))
    };
    let note = |text: &str| -> Result<Label, Error> {
        Ok(Label::plain(
            &names.fonts,
            text,
            props.number_or("note-size", TEXT_SIZE * 0.7)?,
            match props.get("note-font") {
                Some(p) => self::font(p, &names.fonts)?,
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
            left: start as f64 * unit_w,
            width: span as f64 * unit_w,
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
    let cell_keys: Vec<&str> = keys("cell").into_iter().filter(|k| *k != "at").collect();
    for child in &node.children {
        if child.kind.text != "cell" {
            return Err(Error::new(
                child.kind.span,
                format!("a `row` holds `cell` nodes, not `{}`", child.kind.text),
            ));
        }
        let cell = Props::new(child, &cell_keys, false)?;
        let text = match cell.get("text") {
            Some(p) => p.string()?,
            None => String::new(),
        };
        if let (Some(_), Some(p)) = (&weighted, cell.get("span")) {
            return Err(Error::new(
                p.span,
                "a cell of a row with `weights` takes its width from its weight, not `span`",
            ));
        }
        push(&text, cell.count_or("span", span)?, Some(&cell))?;
    }
    if let Some((width, weights, p)) = weighted {
        share(&mut cells, width, &weights, p)?;
    }
    let mut row = Row {
        origin,
        height: super::row::height(unit_h, &cells),
        noted: cells.iter().any(|c| c.note.is_some()),
        gap: props.number_or("gap", 0.0)?,
        radius: props.number_or("radius", 0.0)?,
        cells,
        follow,
    };
    let size = Point::new(row.width(), row.height);
    row.origin = row.origin + placed(&props, size, Point::ZERO)?;
    Ok(row)
}

/// The weights `weights` lists as `N...`, or takes from a table as
/// `DATA COL`.
fn weights(p: &Property, names: &Names) -> Result<Vec<f64>, Error> {
    let weights = match p.values.first() {
        Some(Value {
            kind: ValueKind::Word(name),
            span,
        }) => data_table(names, name, *span)?.column_numbers(p)?,
        _ => p
            .values
            .iter()
            .map(|v| match v.kind {
                ValueKind::Number(n) => Ok(n),
                _ => Err(Error::new(v.span, "`weights` takes numbers")),
            })
            .collect::<Result<_, _>>()?,
    };
    if weights.iter().any(|w| *w < 0.0) || weights.iter().sum::<f64>() <= 0.0 {
        return Err(p.expected("weights of 0 or more, not all 0"));
    }
    Ok(weights)
}

/// Give `cells` their shares of `width` in proportion to `weights`,
/// left to right, with an error at `p` when the counts differ.
fn share(cells: &mut [RowCell], width: f64, weights: &[f64], p: &Property) -> Result<(), Error> {
    if weights.len() != cells.len() {
        return Err(Error::new(
            p.span,
            format!(
                "`weights` gives {} weights for {} cells",
                weights.len(),
                cells.len()
            ),
        ));
    }
    let total: f64 = weights.iter().sum();
    let mut left = 0.0;
    for (cell, w) in cells.iter_mut().zip(weights) {
        cell.left = left;
        cell.width = width * w / total;
        left += cell.width;
    }
    Ok(())
}

/// The row cell `p`, an `at`, names, as in `at r 3` or `at r 3 0 -40`,
/// which [`at`] has checked.
fn cell_named(p: &Property, names: &Names) -> Option<Target> {
    let named = at_named(&p.values);
    let (_, cell, _) = target(p, named, names, 0).ok()?;
    Some(Target {
        source: names.source_of(&named[0]),
        cell: Some(cell?),
    })
}

fn matrix(node: &Node, names: &Names, offset: Point) -> Result<Matrix, Error> {
    let props = Props::new(node, &keys("matrix"), true)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let follow = props.get("at").and_then(|p| cell_named(p, names));
    let count = |key: &str| -> Result<usize, Error> {
        let p = props.require(key)?;
        match p.count()? {
            0 => Err(p.expected("a whole number from 1")),
            n => Ok(n),
        }
    };
    let (rows, columns) = (count("rows")?, count("columns")?);
    let [unit_w, unit_h] = props.require("unit")?.numbers::<2>()?;
    let fill = match props.get("fill") {
        Some(p) => p.color_or_none()?,
        None => Some(Color::WHITE),
    };
    let stroke = props.stroke(Some(Color::BLACK))?;
    let width = props.number_or("stroke-width", STROKE_WIDTH)?;
    let dash = match props.get("dash") {
        Some(p) => Some(p.numbers::<2>()?),
        None => None,
    };
    let text = Label::plain(
        &names.fonts,
        "",
        props.number_or("text-size", TEXT_SIZE)?,
        props.font_or(names.font, &names.fonts)?,
        props.color_or("text-color", Color::BLACK)?,
    );
    let numbers = |key: &str| match props.get(key) {
        Some(p) => p.numbers::<2>().map(|[first, step]| Some((first, step))),
        None => Ok(None),
    };

    let plain = MatrixCell {
        fill,
        stroke,
        label: None,
        outlined: false,
    };
    let mut cells = vec![plain; rows * columns];
    if let Some(p) = props.get("cells") {
        let Some(Value {
            kind: ValueKind::Word(name),
            span,
        }) = p.values.first()
        else {
            return Err(p.expected("a data name and three column names"));
        };
        for (r, c, t) in data_table(names, name, *span)?.cells(p, rows, columns)? {
            cells[r * columns + c].label = (!t.is_empty()).then(|| text.with_text(t));
        }
    }
    let mut text_colors = vec![None; rows * columns];
    let cell_keys: Vec<&str> = keys("cell")
        .into_iter()
        .filter(|k| !matches!(*k, "span" | "note"))
        .collect();
    for child in &node.children {
        if child.kind.text != "cell" {
            return Err(Error::new(
                child.kind.span,
                format!("a `matrix` holds `cell` nodes, not `{}`", child.kind.text),
            ));
        }
        let cell = Props::new(child, &cell_keys, false)?;
        let p = cell.require("at")?;
        let [r, c] = p.numbers::<2>()?;
        let i = matrix_index(rows, columns, r, c, p.span, "the matrix")?;
        let out = &mut cells[i];
        if let Some(p) = cell.get("fill") {
            out.fill = p.color_or_none()?;
        }
        if ["stroke", "stroke-width", "dash"]
            .iter()
            .any(|k| cell.get(k).is_some())
        {
            let current = out.stroke;
            let color = match cell.get("stroke") {
                Some(p) => p.color_or_none()?,
                None => current.map(|s| s.color),
            };
            let width = match cell.get("stroke-width") {
                Some(p) => p.numbers::<1>()?[0],
                None => current.map_or(width, |s| s.width),
            };
            let dash = match cell.get("dash") {
                Some(p) => Some(p.numbers::<2>()?),
                None => current.map_or(dash, |s| s.dash),
            };
            out.stroke = color.map(|color| Stroke { color, width, dash });
            out.outlined = true;
        }
        if let Some(p) = cell.get("text") {
            let t = p.string()?;
            out.label = (!t.is_empty()).then(|| text.with_text(t));
        }
        if let Some(p) = cell.get("text-color") {
            text_colors[i] = Some(p.color()?);
        }
    }
    for (cell, color) in cells.iter_mut().zip(text_colors) {
        if let (Some(label), Some(color)) = (&mut cell.label, color) {
            label.set_color(color);
        }
    }
    let size = Point::new(columns as f64 * unit_w, rows as f64 * unit_h);
    Ok(Matrix {
        origin: origin + placed(&props, size, Point::ZERO)?,
        rows,
        columns,
        unit: Point::new(unit_w, unit_h),
        cells,
        fill,
        stroke,
        text,
        row_numbers: numbers("row-numbers")?,
        column_numbers: numbers("column-numbers")?,
        follow,
    })
}

/// The index of the cell at row `r` and column `c` of a matrix of
/// `rows` by `columns`, or an error at `span`, where `what` names the
/// matrix, when it has no such cell.
fn matrix_index(
    rows: usize,
    columns: usize,
    r: f64,
    c: f64,
    span: Span,
    what: &str,
) -> Result<usize, Error> {
    let fits = |n: f64, count: usize| n.fract() == 0.0 && n >= 0.0 && n < count as f64;
    if !fits(r, rows) || !fits(c, columns) {
        return Err(Error::new(
            span,
            format!(
                "{what} has rows 0 to {} and columns 0 to {}",
                rows - 1,
                columns - 1
            ),
        ));
    }
    Ok(r as usize * columns + c as usize)
}

fn connector(
    node: &Node,
    names: &Names,
    default_head: Head,
    offset: Point,
    shift: usize,
) -> Result<Connector, Error> {
    let props = Props::new(node, &keys("line"), false)?;
    let (mut from, from_target) = end(props.require("from")?, names, offset, shift)?;
    let to_prop = props.require("to")?;
    let (mut to, to_target) = end(to_prop, names, offset, shift)?;
    let looped = match (&from, &to) {
        (End::Box(thing), End::Box(_)) if from_target == to_target => {
            let side = match props.get("side") {
                Some(p) => side(p)?,
                None => Side::Top,
            };
            (from, to) = loop_ends(thing, side);
            true
        }
        _ => false,
    };
    if !looped && let Some(p) = props.get("side") {
        return Err(Error::new(
            p.span,
            "`side` places a loop, whose `from` and `to` name one thing with no side",
        ));
    }
    let ([from_at, to_at], [from_dir, to_dir]) = connector::place(&from, &to);
    if from_at == to_at {
        return Err(Error::new(
            to_prop.span,
            "this end meets the other, so the line draws nothing",
        ));
    }
    let follow =
        |end: End, target: Option<Target>| target.map(|target| Box::new(Follow { target, end }));
    let stroke = props
        .stroke(Some(Color::BLACK))?
        .unwrap_or(Stroke::solid(Color::BLACK, STROKE_WIDTH));
    Ok(Connector {
        from: from_at,
        to: to_at,
        from_dir,
        to_dir,
        follow: [follow(from, from_target), follow(to, to_target)],
        looped,
        stroke,
        route: props.choice(
            "route",
            Route::Straight,
            &[
                ("straight", Route::Straight),
                ("elbow", Route::Elbow),
                ("curve", Route::Curve),
            ],
        )?,
        head: props.choice("head", default_head, &HEADS)?,
        tail: props.choice("tail", Head::None, &HEADS)?,
        head_size: props.number("head-size")?,
        label: props.label("label", "text-size", "text-color", names)?,
        label_side: props.choice("label-side", LabelSide::Above, &LABEL_SIDES)?,
        label_at: props.number_or("label-at", 0.5)?,
    })
}

fn sweep(node: &Node, names: &Names, offset: Point) -> Result<Connector, Error> {
    let props = Props::new(node, &keys("sweep"), false)?;
    let from_prop = props.require("from")?;
    let (from, from_target) = end(from_prop, names, offset, 0)?;
    let to_prop = props.require("to")?;
    let (to, to_target) = end(to_prop, names, offset, 0)?;
    let ([from_at, to_at], [from_dir, to_dir]) = connector::place(&from, &to);
    if from_at == to_at {
        return Err(Error::new(
            to_prop.span,
            "this end meets the other, so the sweep draws nothing",
        ));
    }
    let leave = way(&props, "leave", from_prop, from_dir.is_some())?;
    let enter = way(&props, "enter", to_prop, to_dir.is_some())?;
    let follow =
        |end: End, target: Option<Target>| target.map(|target| Box::new(Follow { target, end }));
    let stroke = props
        .stroke(Some(Color::BLACK))?
        .unwrap_or(Stroke::solid(Color::BLACK, STROKE_WIDTH));
    Ok(Connector {
        from: from_at,
        to: to_at,
        from_dir,
        to_dir,
        follow: [follow(from, from_target), follow(to, to_target)],
        looped: false,
        stroke,
        route: Route::Sweep { leave, enter },
        head: props.choice("head", Head::Triangle, &HEADS)?,
        tail: props.choice("tail", Head::None, &HEADS)?,
        head_size: props.number("head-size")?,
        label: props.label("label", "text-size", "text-color", names)?,
        label_side: props.choice("label-side", LabelSide::Above, &LABEL_SIDES)?,
        label_at: props.number_or("label-at", 0.5)?,
    })
}

/// The node and property that may be set more than once: each `from` of
/// a `merge` starts a line of its own.
const REPEATED: (&str, &str) = ("merge", "from");

fn merge(node: &Node, names: &Names, offset: Point) -> Result<Merge, Error> {
    let props = Props::new(node, &keys("merge"), false)?;
    props.require("from")?;
    let to_prop = props.require("to")?;
    let (to, to_target) = end(to_prop, names, offset, 0)?;
    let at = props
        .get("at")
        .map(|p| p.numbers::<2>().map(|[x, y]| offset + Point::new(x, y)))
        .transpose()?;
    let stroke = props
        .stroke(Some(Color::BLACK))?
        .unwrap_or(Stroke::solid(Color::BLACK, STROKE_WIDTH));
    let tail = props.choice("tail", Head::None, &HEADS)?;
    let head_size = props.number("head-size")?;
    let line = |follow: [Option<Box<Follow>>; 2]| Connector {
        from: Point::ZERO,
        to: Point::ZERO,
        from_dir: None,
        to_dir: None,
        follow,
        looped: false,
        stroke,
        route: Route::Straight,
        head: Head::None,
        tail: Head::None,
        head_size,
        label: None,
        label_side: LabelSide::Above,
        label_at: 0.5,
    };
    let mut froms = Vec::new();
    let mut branches = Vec::new();
    for p in props.all("from") {
        let (from, target) = end(p, names, offset, 0)?;
        let follow = target.map(|target| {
            Box::new(Follow {
                target,
                end: from.clone(),
            })
        });
        branches.push(Connector {
            route: Route::Sweep {
                leave: None,
                enter: None,
            },
            tail,
            ..line([follow, None])
        });
        froms.push(from);
    }
    let to_follow = to_target.map(|target| {
        Box::new(Follow {
            target,
            end: to.clone(),
        })
    });
    let mut merge = Merge {
        branches,
        trunk: Connector {
            head: props.choice("head", Head::Triangle, &HEADS)?,
            label: props.label("label", "text-size", "text-color", names)?,
            label_side: props.choice("label-side", LabelSide::Above, &LABEL_SIDES)?,
            label_at: props.number_or("label-at", 0.5)?,
            ..line([None, to_follow])
        },
        at,
    };
    merge.join(&froms, &to);
    if merge.trunk.from == merge.trunk.to {
        let span = props.get("at").unwrap_or(to_prop).span;
        return Err(Error::new(
            span,
            "the junction meets `to`, so the arrow draws nothing",
        ));
    }
    Ok(merge)
}

/// The way `key` gives a sweep's end, which `end` gives as a point or
/// a thing with no side when `side` is false.
fn way(props: &Props, key: &str, end: &Property, side: bool) -> Result<Option<Dir>, Error> {
    match (props.get(key), side) {
        (Some(p), true) => Err(Error::new(
            p.span,
            format!("this end's side gives its way, so it takes no `{key}`"),
        )),
        (Some(_), false) => Ok(Some(props.choice(key, Dir::Down, &DIRS)?)),
        (None, true) => Ok(None),
        (None, false) => Err(Error::new(
            end.span,
            format!("this end has no side, so the sweep needs `{key}`"),
        )),
    }
}

const DIRS: [(&str, Dir); 4] = [
    ("up", Dir::Up),
    ("down", Dir::Down),
    ("left", Dir::Left),
    ("right", Dir::Right),
];

const LABEL_SIDES: [(&str, LabelSide); 4] = [
    ("above", LabelSide::Above),
    ("left", LabelSide::Left),
    ("right", LabelSide::Right),
    ("on", LabelSide::On),
];

const HEADS: [(&str, Head); 4] = [
    ("none", Head::None),
    ("triangle", Head::Triangle),
    ("open", Head::Open),
    ("dot", Head::Dot),
];

/// Text placed by its top edge. With a `number`, the badge for it sits
/// at the left and the text is set to its right.
fn text(node: &Node, names: &Names, offset: Point) -> Result<(Option<Shape>, Text), Error> {
    let props = Props::new(node, &keys("text"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let align = props.align()?;
    let mut label = props
        .label("text", "size", "color", names)?
        .ok_or_else(|| Error::new(node.kind.span, "`text` needs `text`"))?;
    if let Some(p) = props.get("width") {
        label.width = Some(p.numbers::<1>()?[0]);
    }
    if let Some(p) = props.get("text-align") {
        match p.word()? {
            "justify" => justify(p, &mut label)?,
            _ => {
                return Err(Error::new(
                    p.span,
                    "`text-align` on `text` is justify; `align` places the text",
                ));
            }
        }
    }
    let rotate = props.number_or("rotate", 0.0)?;
    let start = origin;
    let mut origin = origin;
    let mut badge = None;
    if let Some(p) = props.get("number") {
        if let Some(a) = props.get("align") {
            return Err(Error::new(a.span, "numbered text is aligned left"));
        }
        let b = &names.badge;
        let first_line = LINE_HEIGHT * label.size;
        let center = origin + Point::new(b.diameter / 2.0, first_line / 2.0);
        badge = Some(b.shape(p.count()?, center));
        let indent = b.diameter + BADGE_GAP;
        origin.x += indent;
        if let Some(width) = &mut label.width {
            *width = (*width - indent).max(0.0);
        }
    }
    // the box runs from the badge's left to the text's right
    let indent = origin.x - start.x;
    let size = label.size() + Point::new(indent, 0.0);
    let shift = placed(&props, size, Point::new(along(align), 0.0))?;
    origin = origin + shift;
    if let Some(b) = &mut badge {
        b.origin = b.origin + shift;
    }
    Ok((
        badge,
        Text {
            origin,
            label,
            align,
            rotate,
        },
    ))
}

/// How far along its width a text sits at its origin when aligned by
/// `align`.
fn along(align: HAlign) -> f64 {
    match align {
        HAlign::Left => 0.0,
        HAlign::Center => 0.5,
        HAlign::Right => 1.0,
    }
}

/// Set `label` to stretch its lines to its width, as `p` asks.
fn justify(p: &Property, label: &mut Label) -> Result<(), Error> {
    if label.width.is_none() {
        return Err(Error::new(p.span, "`text-align justify` needs `width`"));
    }
    label.justify = true;
    Ok(())
}

/// A number in a filled circle, centered on the point `at` names.
fn marker(node: &Node, names: &Names, offset: Point) -> Result<Shape, Error> {
    let props = Props::new(node, &keys("marker"), false)?;
    let at = props.require("at")?;
    let badge = Badge::read(&props, &names.fonts)?;
    let mut center = match end(at, names, offset, 0)?.0 {
        End::Point(p) => p,
        End::Side { at, .. } => at,
        End::Box(s) => s.center(),
    };
    // a side puts the circle outside it, touching
    let side = at.values.iter().skip(1).find_map(|v| match &v.kind {
        ValueKind::Word(w) => side_named(w),
        _ => None,
    });
    let outward = side.map_or(Point::ZERO, Side::outward);
    center = center + outward * (badge.diameter / 2.0);
    let number = props.require("number")?.count()?;
    Ok(badge.shape(number, center))
}

/// The gap between a badge and the text numbered by it.
const BADGE_GAP: f64 = 8.0;

/// How the number in a `marker` or beside numbered `text` is drawn.
#[derive(Clone)]
struct Badge {
    fill: Color,
    text_color: Color,
    text_size: f64,
    font: Font,
    diameter: f64,
    fonts: Fonts,
}

impl Badge {
    fn read(props: &Props, fonts: &Fonts) -> Result<Badge, Error> {
        Ok(Badge {
            fonts: fonts.clone(),
            fill: props.color_or("fill", Color::BLACK)?,
            text_color: props.color_or("text-color", Color::WHITE)?,
            text_size: props.number_or("text-size", 12.0)?,
            font: props.font_or(Font::SansBold, fonts)?,
            diameter: props.number_or("diameter", 20.0)?,
        })
    }

    /// The badge that `style marker` sets, whose properties are
    /// `properties`; `span` is where a mistake in them is reported when
    /// they carry none.
    fn of_style(properties: Vec<Property>, span: Span, fonts: &Fonts) -> Result<Badge, Error> {
        let word = |text: &str| crate::lang::Word {
            text: text.into(),
            span,
        };
        let node = Node {
            kind: word("marker"),
            name: None,
            head: Vec::new(),
            properties,
            children: Vec::new(),
            span,
        };
        Badge::read(&Props::new(&node, &keys("marker"), false)?, fonts)
    }

    fn shape(&self, number: usize, center: Point) -> Shape {
        let size = Point::new(self.diameter, self.diameter);
        Shape {
            origin: center - size * 0.5,
            size,
            oval: true,
            radius: 0.0,
            fill: Some(self.fill),
            stroke: None,
            label: Some(Label::plain(
                &self.fonts,
                number.to_string(),
                self.text_size,
                self.font,
                self.text_color,
            )),
            padding: Point::ZERO,
            text_align: None,
        }
    }
}

/// A formula in display style, placed by its top edge like a `text`.
fn math(node: &Node, names: &Names, offset: Point) -> Result<Text, Error> {
    let props = Props::new(node, &keys("math"), false)?;
    let origin = match props.get("at") {
        Some(p) => at(p, names, offset)?,
        None => offset,
    };
    let p = props.require("text")?;
    let formula = p.string()?;
    crate::math::parse(&formula).map_err(|message| Error::new(p.span, message))?;
    let run = Run {
        text: format!("\\displaystyle {formula}"),
        font: Font::Math,
        color: props.color_or("color", Color::BLACK)?,
        bullet: false,
    };
    let label = Label {
        runs: vec![run],
        size: props.number_or("size", TEXT_SIZE)?,
        width: None,
        justify: false,
        fonts: names.fonts.clone(),
    };
    let align = props.align()?;
    let shift = placed(&props, label.size(), Point::new(along(align), 0.0))?;
    Ok(Text {
        origin: origin + shift,
        label,
        align,
        rotate: 0.0,
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
    let font = props.font_or(names.font, &names.fonts)?;
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
                    justify: false,
                    fonts: names.fonts.clone(),
                },
                align: aligns[column],
                rotate: 0.0,
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
                    bullet: false,
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
    let mut code = Code {
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
        fonts: names.fonts.clone(),
    };
    code.origin = code.origin + placed(&props, code.extent(), Point::ZERO)?;
    Ok(code)
}

fn brace(node: &Node, names: &Names, offset: Point) -> Result<Brace, Error> {
    let props = Props::new(node, &keys("brace"), false)?;
    let side = match props.get("side") {
        Some(p) => side(p)?,
        None => Side::Top,
    };
    let gap = props.number_or("gap", 4.0)?;
    let mut follow = None;
    let (from, to) = match props.get("over") {
        Some(p) => {
            let ([left, top, right, bottom], source) = bounds(p, names)?;
            follow = Some(source);
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
        follow,
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
            &names.fonts,
            "",
            props.number_or("text-size", TEXT_SIZE * 0.7)?,
            props.font_or(names.font, &names.fonts)?,
            props.color_or("text-color", Color::BLACK)?,
        ),
    })
}

/// The end a connector's `from` or `to` names, and the thing it names.
/// It is two numbers, a shape's name, a row's name and a cell's index,
/// or a matrix's name and a cell's row and column. A side or a corner
/// after a name puts the end at the middle of that side or at that
/// corner, facing out of it, and a side and a number F puts it F of the
/// way along that side.
fn end(
    p: &Property,
    names: &Names,
    offset: Point,
    shift: usize,
) -> Result<(End, Option<Target>), Error> {
    if let [first, ..] = p.values.as_slice()
        && let ValueKind::Number(_) = first.kind
    {
        return p
            .numbers::<2>()
            .map(|[x, y]| (End::Point(offset + Point::new(x, y)), None));
    }
    let (thing, cell, rest) = target(p, &p.values, names, shift)?;
    let source = Target {
        source: names.source_of(&p.values[0]),
        cell,
    };
    let end = match rest {
        [] => End::Box(thing),
        [v] => match &v.kind {
            ValueKind::Word(w) => {
                let [left, top, right, bottom] = [
                    thing.origin.x,
                    thing.origin.y,
                    thing.origin.x + thing.size.x,
                    thing.origin.y + thing.size.y,
                ];
                let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
                let (at, out) = match w.as_str() {
                    "top" => (Point::new(cx, top), Point::new(0.0, -1.0)),
                    "bottom" => (Point::new(cx, bottom), Point::new(0.0, 1.0)),
                    "left" => (Point::new(left, cy), Point::new(-1.0, 0.0)),
                    "right" => (Point::new(right, cy), Point::new(1.0, 0.0)),
                    "top-left" => corner(&thing, -1.0, -1.0),
                    "top-right" => corner(&thing, 1.0, -1.0),
                    "bottom-left" => corner(&thing, -1.0, 1.0),
                    "bottom-right" => corner(&thing, 1.0, 1.0),
                    _ => {
                        return Err(Error::new(
                            v.span,
                            "a side is top, bottom, left or right, and a corner is top-left, top-right, bottom-left or bottom-right",
                        ));
                    }
                };
                End::Side { at, out }
            }
            _ => return Err(Error::new(v.span, "expected a side or a corner")),
        },
        [side, f] => End::Side {
            at: along_side(&thing, side, f)?,
            out: match &side.kind {
                ValueKind::Word(w) => side_named(w).map_or(Point::ZERO, Side::outward),
                _ => Point::ZERO,
            },
        },
        [_, _, extra, ..] => return Err(Error::new(extra.span, "too many values")),
    };
    Ok((end, Some(source)))
}

/// How far apart, at most, a loop's ends sit on their side.
const LOOP_SPREAD: f64 = 20.0;

/// The ends of a loop out of `side` of `thing` and back into it, a
/// little either side of the side's middle. The loop runs clockwise.
fn loop_ends(thing: &Shape, side: Side) -> (End, End) {
    let out = side.outward();
    let along = Point::new(-out.y, out.x);
    let half = thing.size * 0.5;
    let half_side = (along.x * half.x + along.y * half.y).abs();
    let depth = (out.x * half.x + out.y * half.y).abs();
    let s = (half_side / 2.0).min(LOOP_SPREAD / 2.0);
    // on an ellipse the ends sit on the outline, short of the
    // box's side
    let depth = if thing.oval && half_side > 0.0 {
        depth * (1.0 - (s / half_side).powi(2)).sqrt()
    } else {
        depth
    };
    let at = |sign: f64| thing.center() + out * depth + along * (sign * s);
    (
        End::Side { at: at(-1.0), out },
        End::Side { at: at(1.0), out },
    )
}

/// The corner of `thing` that `sx` and `sy` point to, each -1 or 1, and
/// the diagonal pointing out of it.
fn corner(thing: &Shape, sx: f64, sy: f64) -> (Point, Point) {
    // an ellipse's corner is where the diagonal of its box crosses it
    let reach = if thing.oval { FRAC_1_SQRT_2 } else { 1.0 };
    let half = thing.size * (0.5 * reach);
    (
        thing.center() + Point::new(sx * half.x, sy * half.y),
        Point::new(sx, sy) * FRAC_1_SQRT_2,
    )
}

/// The left, top, right and bottom of the box around everything `over`
/// names, and the thing the brace follows. Each name is a shape, a
/// whole row, a row's cell, a row's cells from one index to another, or
/// a matrix or one of its cells.
/// The brace follows the first name, which is one cell only when it
/// names one.
fn bounds(p: &Property, names: &Names) -> Result<([f64; 4], Target), Error> {
    let corners = |s: &Shape| {
        [
            s.origin.x,
            s.origin.y,
            s.origin.x + s.size.x,
            s.origin.y + s.size.y,
        ]
    };
    let mut values = p.values.as_slice();
    let mut around: Option<[f64; 4]> = None;
    let mut follow = None;
    while !values.is_empty() {
        let (first, mut cell, mut rest) = target(p, values, names, 0)?;
        let [left, top, mut right, mut bottom] = corners(&first);
        // a second number after a cell or a code line ends a run
        if values.len() - rest.len() == 2
            && let Some((
                last @ Value {
                    kind: ValueKind::Number(_),
                    ..
                },
                after,
            )) = rest.split_first()
        {
            cell = None;
            let (last, _, _) = target(p, &[values[0].clone(), last.clone()], names, 0)?;
            let c = corners(&last);
            right = c[2];
            bottom = c[3];
            rest = after;
        }
        follow.get_or_insert(Target {
            source: names.source_of(&values[0]),
            cell,
        });
        around = Some(match around {
            None => [left, top, right, bottom],
            Some([l, t, r, b]) => [l.min(left), t.min(top), r.max(right), b.max(bottom)],
        });
        values = rest;
    }
    match (around, follow) {
        (Some(around), Some(follow)) => Ok((around, follow)),
        _ => Err(p.expected("a name")),
    }
}

/// The box that the leading values of `values` name, as a shape, the
/// cell it is when it is a row's or a matrix's, and the values after
/// them. A name alone is a shape, a whole row or a whole matrix. A
/// row's name and a number is one of its cells, and a matrix's name and
/// two numbers is the cell at that row and column.
fn target<'v>(
    p: &Property,
    values: &'v [Value],
    names: &Names,
    shift: usize,
) -> Result<(Shape, Option<usize>, &'v [Value]), Error> {
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
        return Ok((shape.clone(), None, rest));
    }
    if let Some(&(origin, size)) = names.outlines.get(word.as_str()) {
        return Ok((as_box(origin, size), None, rest));
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
                Ok((as_box(origin, size), None, rest))
            }
            _ => Ok((as_box(code.origin, code.extent()), None, rest)),
        };
    }
    if let Some(matrix) = names.matrices.get(word.as_str()) {
        return match rest {
            [
                Value {
                    kind: ValueKind::Number(r),
                    span: r_span,
                },
                Value {
                    kind: ValueKind::Number(c),
                    span: c_span,
                },
                rest @ ..,
            ] => {
                let span = Span {
                    start: r_span.start,
                    end: c_span.end,
                };
                let what = format!("matrix `{word}`");
                let c = c + shift as f64;
                let i = matrix_index(matrix.rows, matrix.columns, *r, c, span, &what)?;
                let (origin, size) = matrix.cell_rect(i / matrix.columns, i % matrix.columns);
                Ok((as_box(origin, size), Some(i), rest))
            }
            [
                Value {
                    kind: ValueKind::Number(_),
                    span,
                },
                ..,
            ] => Err(Error::new(
                *span,
                format!("a cell of matrix `{word}` is `{word} R C`, its row and column"),
            )),
            _ => Ok((as_box(matrix.origin, matrix.size()), None, rest)),
        };
    }
    let Some(row) = names.rows.get(word.as_str()) else {
        return Err(Error::new(
            name.span,
            format!("no box, ellipse, polygon, polyline, row, matrix or code is named `{word}`"),
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
            Ok((as_box(origin, size), Some(index), rest))
        }
        _ => {
            let size = Point::new(row.width(), row.height);
            Ok((as_box(row.origin, size), None, rest))
        }
    }
}

fn side(p: &Property) -> Result<Side, Error> {
    side_named(p.word()?).ok_or_else(|| Error::new(p.span, "a side is top, bottom, left or right"))
}

fn side_named(word: &str) -> Option<Side> {
    match word {
        "top" => Some(Side::Top),
        "bottom" => Some(Side::Bottom),
        "left" => Some(Side::Left),
        "right" => Some(Side::Right),
        _ => None,
    }
}

fn font(p: &Property, fonts: &Fonts) -> Result<Font, Error> {
    let name = p.word()?;
    match name {
        "mono" => Ok(Font::Mono),
        "mono-bold" => Ok(Font::MonoBold),
        "sans" => Ok(Font::Sans),
        "sans-bold" => Ok(Font::SansBold),
        "sans-italic" => Ok(Font::SansItalic),
        _ => fonts.lookup(name).ok_or_else(|| {
            Error::new(
                p.span,
                "`font` is mono, mono-bold, sans, sans-bold, sans-italic or a name from `fonts`",
            )
        }),
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
pub(super) struct Props<'a> {
    node: &'a Node,
}

impl<'a> Props<'a> {
    pub(super) fn new(node: &'a Node, keys: &[&str], children: bool) -> Result<Self, Error> {
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
            if (kind.as_str(), key.as_str()) != REPEATED
                && node.properties[..i].iter().any(|q| q.key.text == *key)
            {
                return Err(Error::new(p.key.span, format!("`{key}` is set twice")));
            }
        }
        Ok(Props { node })
    }

    fn get(&self, key: &str) -> Option<&'a Property> {
        self.node.properties.iter().find(|p| p.key.text == key)
    }

    /// Every setting of `key`, in order, for the one property that may
    /// repeat.
    fn all(&self, key: &'a str) -> impl Iterator<Item = &'a Property> {
        self.node
            .properties
            .iter()
            .filter(move |p| p.key.text == key)
    }

    pub(super) fn require(&self, key: &str) -> Result<&'a Property, Error> {
        self.get(key).ok_or_else(|| {
            Error::new(
                self.node.kind.span,
                format!("`{}` needs `{key}`", self.node.kind.text),
            )
        })
    }

    fn number(&self, key: &str) -> Result<Option<f64>, Error> {
        self.get(key).map(|p| Ok(p.numbers::<1>()?[0])).transpose()
    }

    fn number_or(&self, key: &str, default: f64) -> Result<f64, Error> {
        match self.get(key) {
            Some(p) => Ok(p.numbers::<1>()?[0]),
            None => Ok(default),
        }
    }

    /// The value of the word `key` takes from `choices`, or `default`
    /// when `key` is absent.
    fn choice<T: Copy>(&self, key: &str, default: T, choices: &[(&str, T)]) -> Result<T, Error> {
        let Some(p) = self.get(key) else {
            return Ok(default);
        };
        let word = p.word()?;
        match choices.iter().find(|(name, _)| *name == word) {
            Some((_, value)) => Ok(*value),
            None => {
                let names: Vec<&str> = choices.iter().map(|(name, _)| *name).collect();
                Err(Error::new(
                    p.span,
                    format!("`{key}` is one of {}", names.join(", ")),
                ))
            }
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

    fn align(&self) -> Result<HAlign, Error> {
        let Some(p) = self.get("align") else {
            return Ok(HAlign::Left);
        };
        match p.word()? {
            "left" => Ok(HAlign::Left),
            "center" => Ok(HAlign::Center),
            "right" => Ok(HAlign::Right),
            _ => Err(Error::new(p.span, "`align` is left, center or right")),
        }
    }

    fn font_or(&self, default: Font, fonts: &Fonts) -> Result<Font, Error> {
        match self.get("font") {
            Some(p) => font(p, fonts),
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
            self.font_or(names.font, &names.fonts)?,
            self.color_or(color, Color::BLACK)?,
            &names.colors,
        )
        .map_err(|message| Error::new(p.span, message))?;
        Ok(Some(Label {
            runs,
            size: self.number_or(size, TEXT_SIZE)?,
            width: None,
            justify: false,
            fonts: names.fonts.clone(),
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

    pub(super) fn expected(&self, what: &str) -> Error {
        Error::new(self.span, format!("`{}` takes {what}", self.key.text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;
    use crate::scene::data_files;
    use crate::scene::primitive::{Primitive, VAlign};

    fn scene(src: &str) -> Result<Scene, (usize, usize, String)> {
        let program = parse(src).unwrap();
        build(&program, &[], &[]).map_err(|e| {
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
    fn a_math_node_sets_its_formula_in_display_style() {
        let s =
            scene(r#"canvas { size 100 100; math { at 5 6; text """\sum_i x_i"""; size 20; } }"#)
                .unwrap();
        let Kind::Text(t) = &s.objects[0].kind else {
            panic!("expected text");
        };
        assert_eq!(t.origin, Point::new(5.0, 6.0));
        assert_eq!(t.label.runs[0].font, Font::Math);
        assert_eq!(t.label.runs[0].text, r"\displaystyle \sum_i x_i");
        assert_eq!(
            scene("canvas { size 10 10;\n  math { text \"\\\\fra\"; } }").unwrap_err(),
            (2, 10, "`\\fra` is not a command acre knows".into())
        );
    }

    #[test]
    fn mistakes_point_at_the_offending_text() {
        assert_eq!(
            scene("canvas { size 10 10;\n  box { at 1 2; colour red; } }").unwrap_err(),
            (
                2,
                17,
                "`box` has no property `colour`; it takes at, place, size, fill, stroke, \
                 stroke-width, dash, radius, text, text-size, text-color, font, width, \
                 padding, text-align, tag, tag-size, tag-color, tag-font, anchor, space, style"
                    .into()
            )
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  arrow { from a; to 1 1; } }").unwrap_err(),
            (
                2,
                16,
                "no box, ellipse, polygon, polyline, row, matrix or code is named `a`".into()
            )
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
                    at 10 20; unit 10 30; span 2; palette dna; notes show; note-size 8;
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
        let places: Vec<(f64, f64, usize)> = row
            .cells
            .iter()
            .map(|c| (c.left, c.width, c.span))
            .collect();
        assert_eq!(places, [(0.0, 20.0, 2), (20.0, 20.0, 2), (40.0, 10.0, 1)]);
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
    fn a_brace_over_several_names_spans_the_box_around_them() {
        let s = scene(
            "canvas { size 300 200;
                box a { at 10 20; size 50 30; }
                box b { at 100 40; size 60 30; }
                row r { at 0 100; unit 10 10; cells \"x\" \"y\" \"z\"; }
                brace { over a b; side bottom; gap 0; }
                brace { over b a r 1 2; side left; gap 0; } }",
        )
        .unwrap();
        let Kind::Brace(two) = &s.objects[3].kind else {
            panic!("expected a brace");
        };
        assert_eq!(
            (two.from, two.to),
            (Point::new(10.0, 70.0), Point::new(160.0, 70.0))
        );
        let Kind::Brace(three) = &s.objects[4].kind else {
            panic!("expected a brace");
        };
        assert_eq!(
            (three.from, three.to),
            (Point::new(10.0, 20.0), Point::new(10.0, 110.0))
        );
    }

    #[test]
    fn a_brace_over_a_name_that_names_nothing_is_an_error() {
        assert_eq!(
            scene("canvas { size 10 10;\n box a { at 0 0; size 5 5; }\n brace { over a nope; } }")
                .unwrap_err(),
            (
                3,
                17,
                "no box, ellipse, polygon, polyline, row, matrix or code is named `nope`".into()
            )
        );
        assert_eq!(
            scene("canvas { size 10 10;\n box a { at 0 0; size 5 5; }\n brace { over a 3; } }")
                .unwrap_err(),
            (3, 17, "expected a name".into())
        );
    }

    #[test]
    fn a_ruler_on_the_left_or_right_runs_down_with_its_ticks_and_numbers_on_that_side() {
        let s = scene(
            "canvas { size 200 200;
                ruler { at 50 10; unit 20; count 3; side left; }
                ruler { at 150 10; unit 20; count 3; every 2; side right; } }",
        )
        .unwrap();
        for (i, dx, h) in [(0, -1.0, HAlign::Right), (1, 1.0, HAlign::Left)] {
            let Kind::Ruler(ruler) = &s.objects[i].kind else {
                panic!("expected a ruler");
            };
            let x = ruler.origin.x;
            let mut out = Vec::new();
            ruler.draw(&mut out);
            let Primitive::Line { from, to, .. } = &out[0] else {
                panic!("expected the ruler's line");
            };
            assert_eq!((*from, *to), (Point::new(x, 10.0), Point::new(x, 70.0)));
            let Primitive::Line { from, to, .. } = &out[1] else {
                panic!("expected a tick");
            };
            assert_eq!(
                (*from, *to),
                (Point::new(x, 10.0), Point::new(x + dx * 5.0, 10.0))
            );
            let Primitive::Text {
                origin,
                h_align,
                v_align,
                ..
            } = &out[5]
            else {
                panic!("expected a number");
            };
            assert_eq!(*origin, Point::new(x + dx * 8.0, 20.0));
            assert_eq!((*h_align, *v_align), (h, VAlign::Middle));
        }
    }

    #[test]
    fn a_connector_end_may_sit_at_a_corner() {
        let s = scene(
            "canvas { size 300 200;
                box a { at 0 0; size 100 40; }
                ellipse e { at 200 100; size 80 40; }
                arrow { from a bottom-right; to e top-left; }
                line { from a top-left; to e bottom-right; } }",
        )
        .unwrap();
        let Kind::Connector(c) = &s.objects[2].kind else {
            panic!("expected an arrow");
        };
        let h = FRAC_1_SQRT_2;
        assert_eq!(c.from, Point::new(100.0, 40.0));
        assert_eq!(c.to, Point::new(240.0 - 40.0 * h, 120.0 - 20.0 * h));
        assert_eq!(c.from_dir, Some(Point::new(h, h)));
        assert_eq!(c.to_dir, Some(Point::new(-h, -h)));
        let Kind::Connector(c) = &s.objects[3].kind else {
            panic!("expected a line");
        };
        assert_eq!(c.from, Point::new(0.0, 0.0));
        assert_eq!(c.to, Point::new(240.0 + 40.0 * h, 120.0 + 20.0 * h));
    }

    #[test]
    fn a_connector_end_naming_no_side_or_corner_is_an_error() {
        assert_eq!(
            scene("canvas { size 10 10;\n box a { at 0 0; size 5 5; }\n line { from a middle; to 0 0; } }")
                .unwrap_err(),
            (
                3,
                16,
                "a side is top, bottom, left or right, and a corner is top-left, top-right, bottom-left or bottom-right"
                    .into()
            )
        );
    }

    #[test]
    fn an_arrow_from_a_thing_to_itself_loops_out_of_one_side() {
        let s = scene(
            "canvas { size 300 200;
                box a { at 100 100; size 80 40; }
                ellipse e { at 0 0; size 40 20; }
                arrow { from a; to a; }
                arrow { from a; to a; side right; }
                arrow { from e; to e; side bottom; } }",
        )
        .unwrap();
        let ends = |i: usize| {
            let Kind::Connector(c) = &s.objects[i].kind else {
                panic!("expected an arrow");
            };
            assert!(c.looped);
            (c.from, c.to, c.from_dir)
        };
        assert_eq!(
            ends(2),
            (
                Point::new(130.0, 100.0),
                Point::new(150.0, 100.0),
                Some(Point::new(0.0, -1.0))
            )
        );
        assert_eq!(
            ends(3),
            (
                Point::new(180.0, 110.0),
                Point::new(180.0, 130.0),
                Some(Point::new(1.0, 0.0))
            )
        );
        // the ends sit 20 apart on the outline of a 40 by 20 ellipse
        let (from, to, _) = ends(4);
        assert_eq!((from.x, to.x), (30.0, 10.0));
        assert!((from.y - (10.0 + 10.0 * 0.75_f64.sqrt())).abs() < 1e-9);
    }

    #[test]
    fn a_side_on_a_line_that_is_not_a_loop_is_an_error() {
        assert_eq!(
            scene("canvas { size 10 10;\n box a { at 0 0; size 5 5; }\n line { from a; to 9 9; side top; } }")
                .unwrap_err(),
            (
                3,
                25,
                "`side` places a loop, whose `from` and `to` name one thing with no side".into()
            )
        );
    }

    fn merge_of(body: &str) -> Merge {
        let s = scene(&format!(
            "canvas {{ size 300 300;
                box a {{ at 0 0; size 40 20; }}
                box b {{ at 100 0; size 40 20; }}
                box t {{ at 40 200; size 100 40; }}
                {body} }}"
        ))
        .unwrap();
        match &s.objects[3].kind {
            Kind::Merge(m) => m.clone(),
            other => panic!("expected a merge, got {other:?}"),
        }
    }

    #[test]
    fn a_merge_joins_each_from_halfway_to_the_nearest_and_runs_on_to_to() {
        let m = merge_of("merge { from a bottom; from b bottom; to t top; label \"x\"; }");
        // the froms sit 180 above the top of t, so the junction sits 90
        // above it
        let junction = Point::new(90.0, 110.0);
        assert_eq!(m.branches.len(), 2);
        assert_eq!(
            m.branches
                .iter()
                .map(|b| (b.from, b.to))
                .collect::<Vec<_>>(),
            [
                (Point::new(20.0, 20.0), junction),
                (Point::new(120.0, 20.0), junction)
            ]
        );
        assert!(m.branches.iter().all(|b| b.head == Head::None));
        assert_eq!(m.branches[0].to_dir, Some(Point::new(0.0, -1.0)));
        assert_eq!(
            (m.trunk.from, m.trunk.to),
            (junction, Point::new(90.0, 200.0))
        );
        assert_eq!(m.trunk.head, Head::Triangle);
        assert!(m.trunk.label.is_some());
    }

    #[test]
    fn a_merge_joins_at_at_and_a_from_with_no_side_leaves_the_way_the_trunk_runs() {
        let m = merge_of("merge { from 0 100; from b; to t top; at 90 150; tail dot; }");
        let junction = Point::new(90.0, 150.0);
        assert_eq!(m.trunk.from, junction);
        assert_eq!(m.branches[0].from_dir, Some(Point::new(0.0, 1.0)));
        assert_eq!(m.branches[1].to, junction);
        assert!(m.branches.iter().all(|b| b.tail == Head::Dot));
    }

    #[test]
    fn only_a_merge_may_set_from_more_than_once() {
        let twice = |node: &str| {
            scene(&format!("canvas {{ size 10 10; {node} }}"))
                .unwrap_err()
                .2
        };
        assert_eq!(
            twice("merge { from 0 0; from 9 0; to 5 9; to 5 8; }"),
            "`to` is set twice"
        );
        assert_eq!(
            twice("arrow { from 0 0; from 9 0; to 5 9; }"),
            "`from` is set twice"
        );
    }

    #[test]
    fn a_sweep_reads_its_ways_heads_and_label() {
        let s = scene(
            "canvas { size 100 100;
                box a { at 60 60; size 10 10; }
                sweep { from 0 0; to a top; leave down; head-size 4;
                    label \"x\"; label-side on; label-at 0.25; } }",
        )
        .unwrap();
        match &s.objects[1].kind {
            Kind::Connector(c) => {
                assert!(c.label.is_some());
                assert_eq!((c.label_side, c.label_at), (LabelSide::On, 0.25));
                assert_eq!(
                    c.route,
                    Route::Sweep {
                        leave: Some(Dir::Down),
                        enter: None
                    }
                );
                assert_eq!((c.head, c.head_size), (Head::Triangle, Some(4.0)));
            }
            other => panic!("expected a connector, got {other:?}"),
        }
    }

    #[test]
    fn a_sweep_end_with_no_way_or_two_ways_is_an_error() {
        assert_eq!(
            scene("canvas { size 10 10;\n sweep { from 0 0; to 9 9; leave down; } }").unwrap_err(),
            (
                2,
                20,
                "this end has no side, so the sweep needs `enter`".into()
            )
        );
        assert_eq!(
            scene(
                "canvas { size 10 10;\n box a { at 0 0; size 5 5; }\n sweep { from a bottom; to 9 9; leave down; enter down; } }"
            )
            .unwrap_err(),
            (
                3,
                33,
                "this end's side gives its way, so it takes no `leave`".into()
            )
        );
    }

    #[test]
    fn a_line_whose_ends_meet_is_an_error() {
        let message = "this end meets the other, so the line draws nothing";
        assert_eq!(
            scene("canvas { size 10 10;\n arrow { from 5 5; to 5 5; } }").unwrap_err(),
            (2, 20, message.into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n box a { at 0 0; size 5 5; }\n arrow { from a top; to a top; } }")
                .unwrap_err(),
            (3, 22, message.into())
        );
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
    fn text_align_justify_sets_a_box_or_text_to_justify_within_its_width() {
        let s = scene(
            r#"canvas { size 400 400;
                box { at 0 0; text "one two three four"; width 60; text-align justify; }
                text { at 0 100; text "one two three four"; width 60; text-align justify; } }"#,
        )
        .unwrap();
        let Kind::Shape(b) = &s.objects[0].kind else {
            panic!("expected a box");
        };
        assert!(b.label.as_ref().unwrap().justify);
        assert_eq!(b.text_align, Some(HAlign::Left));
        let Kind::Text(t) = &s.objects[1].kind else {
            panic!("expected text");
        };
        assert!(t.label.justify);
        let err = |src: &str| scene(src).unwrap_err().2;
        assert_eq!(
            err(r#"canvas { size 9 9; text { at 0 0; text "a"; text-align justify; } }"#),
            "`text-align justify` needs `width`"
        );
        assert_eq!(
            err(r#"canvas { size 9 9; text { at 0 0; text "a"; width 5; text-align left; } }"#),
            "`text-align` on `text` is justify; `align` places the text"
        );
    }

    #[test]
    fn tags_letter_through_the_canvas_and_sit_left_of_their_node() {
        let s = scene(
            r#"canvas { size auto auto; margin 0;
                style tags { tag-size 10; tag-color #ff0000; }
                panel { title "one"; box { tag auto; at 40 0; size 50 20; } }
                box { tag "(x)"; at 40 100; size 50 20; }
                panel { title "two"; at 200 0; box { tag auto; style tags; at 40 0; size 50 30; } } }"#,
        )
        .unwrap();
        let tags: Vec<&Text> = s
            .objects
            .iter()
            .filter(|o| o.tag)
            .map(|o| match &o.kind {
                Kind::Text(t) => t,
                other => panic!("expected a tag, got {other:?}"),
            })
            .collect();
        let texts: Vec<String> = tags.iter().map(|t| t.label.text()).collect();
        assert_eq!(texts, ["(a)", "(x)", "(b)"]);
        assert_eq!(tags[2].label.size, 10.0);
        assert_eq!(tags[2].label.runs[0].color, Color::rgb(1.0, 0.0, 0.0));

        let Kind::Shape(b) = &s.objects[s.objects.len() - 2].kind else {
            panic!("expected the box");
        };
        let size = tags[2].label.size();
        assert!((tags[2].origin.x + size.x + TAG_GAP - b.origin.x).abs() < 1e-9);
        assert!((tags[2].origin.y + size.y / 2.0 - (b.origin.y + 15.0)).abs() < 1e-9);
    }

    #[test]
    fn an_auto_canvas_holds_a_tag_and_a_tag_takes_text_or_auto() {
        let s = scene("canvas { size auto auto; margin 0; box { tag auto; at 0 0; size 50 20; } }")
            .unwrap();
        let Kind::Shape(b) = &s.objects[0].kind else {
            panic!("expected the box");
        };
        let Kind::Text(tag) = &s.objects[1].kind else {
            panic!("expected the tag");
        };
        assert_eq!(tag.origin.x, 0.0);
        assert_eq!(b.origin.x, tag.label.size().x + TAG_GAP);
        assert_eq!(
            scene("canvas { size 100 100; box { tag 3; size 50 20; } }")
                .unwrap_err()
                .2,
            "`tag` takes a quoted text or `auto`"
        );
        assert_eq!(letters(1), "a");
        assert_eq!(letters(26), "z");
        assert_eq!(letters(27), "aa");
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
    fn a_row_is_as_tall_as_its_tallest_note_reaches() {
        let s = scene(
            r#"canvas { size 400 400;
                stack r { at 0 0; gap 0;
                    row r { unit 20 20; note-size 30; cell { text "a"; } cell { note "x"; } }
                    box b { size 10 10; }
                }
                row low { at 0 200; unit 20 60; note-size 10; cell { note "x"; } } }"#,
        )
        .unwrap();
        let Kind::Row(row) = &s.objects[0].kind else {
            panic!("expected a row");
        };
        let height = row.height;
        assert!(height > 20.0);
        let (_, max) = s.bounds(0, 0.0).unwrap();
        assert!((max.y - height).abs() < 1e-9, "{max:?} {height}");
        let Kind::Shape(b) = &s.objects[1].kind else {
            panic!("expected a box");
        };
        assert_eq!(b.origin.y, height);
        let Kind::Row(low) = &s.objects[2].kind else {
            panic!("expected a row");
        };
        assert_eq!(low.height, 60.0);
    }

    #[test]
    fn a_weighted_row_shares_its_width_less_the_gaps() {
        let s = scene(
            r#"canvas { size 300 100;
                row r { at 10 20; unit 0 30; width 200; gap 4; weights 1 2 1;
                    cells "a" "b" "c"; }
                brace { over r 1; side bottom; gap 0; }
                arrow { from 0 0; to r 2 top; }
                arrow { from 0 0; to r right; } }"#,
        )
        .unwrap();
        let Kind::Row(row) = &s.objects[0].kind else {
            panic!("expected a row");
        };
        assert_eq!(
            row.cell_rect(1),
            (Point::new(62.0, 20.0), Point::new(96.0, 30.0))
        );
        let Kind::Brace(brace) = &s.objects[1].kind else {
            panic!("expected a brace");
        };
        assert_eq!(
            (brace.from, brace.to),
            (Point::new(62.0, 50.0), Point::new(158.0, 50.0))
        );
        let Kind::Connector(arrow) = &s.objects[2].kind else {
            panic!("expected an arrow");
        };
        assert_eq!(arrow.to, Point::new(185.0, 20.0));
        let Kind::Connector(whole) = &s.objects[3].kind else {
            panic!("expected an arrow");
        };
        assert_eq!(whole.to, Point::new(210.0, 35.0));
    }

    #[test]
    fn a_row_takes_its_weights_from_a_data_column() {
        let src = r#"canvas { size 300 100;
            data d { file "d.tsv"; }
            row r { at 0 0; unit 0 10; width 90; weights d n;
                cell { text "x"; } cell { text "y"; } } }"#;
        let s = scene_data(src, &[b"name\tn\nx\t2\ny\t1\n"]).unwrap();
        let Kind::Row(row) = &s.objects[0].kind else {
            panic!("expected a row");
        };
        let widths: Vec<f64> = row.cells.iter().map(|c| c.width).collect();
        assert_eq!(widths, [60.0, 30.0]);
    }

    #[test]
    fn a_weighted_row_says_what_is_wrong_with_its_weights() {
        let err = |body: &str| {
            let src =
                format!("canvas {{ size 9 9;\n row r {{ unit 0 9; cells \"a\" \"b\"; {body} }} }}");
            scene(&src).unwrap_err().2
        };
        assert_eq!(err("width 9;"), "`width` goes with `weights`");
        assert_eq!(err("weights 1 1;"), "`weights` goes with `width`");
        assert_eq!(
            err("width 9; weights 1 2 3;"),
            "`weights` gives 3 weights for 2 cells"
        );
        assert_eq!(
            err("width 9; weights 0 0;"),
            "`weights` takes weights of 0 or more, not all 0"
        );
        assert_eq!(err("width 9; weights 1 x;"), "`weights` takes numbers");
        assert_eq!(
            err("width 9; weights 1 1; span 2;"),
            "a row with `weights` gives each cell one weight and takes no `span`"
        );
        assert_eq!(
            err("width 9; weights 1 1 1; cell { span 2; }"),
            "a cell of a row with `weights` takes its width from its weight, not `span`"
        );
    }

    #[test]
    fn a_row_with_a_note_lines_up_every_label() {
        let s = scene(
            r#"canvas { size 400 400;
                row r { unit 20 40; cell { text "a"; } cell { text "b"; note "x"; } } }"#,
        )
        .unwrap();
        let Kind::Row(row) = &s.objects[0].kind else {
            panic!("expected a row");
        };
        let mut prims = Vec::new();
        row.draw(&mut prims);
        let ys: Vec<f64> = prims
            .iter()
            .filter_map(|p| match p {
                Primitive::Text { origin, runs, .. } if runs[0].text != "x" => Some(origin.y),
                _ => None,
            })
            .collect();
        assert_eq!(ys.len(), 2);
        assert_eq!(ys[0], ys[1]);
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
        bounds_of(&Fonts::default(), &s.objects).unwrap()
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
    fn a_connector_reads_its_route_heads_and_label_placement() {
        let s = scene(
            "canvas { size 100 100;
                line { from 0 0; to 50 50; }
                arrow { from 0 0; to 50 50; route curve; head dot; tail open;
                    head-size 4; label \"x\"; label-side right; label-at 0.25; } }",
        )
        .unwrap();
        let connector = |i: usize| match &s.objects[i].kind {
            Kind::Connector(c) => c.clone(),
            other => panic!("expected a connector, got {other:?}"),
        };
        let line = connector(0);
        assert_eq!(
            (line.route, line.head, line.tail),
            (Route::Straight, Head::None, Head::None)
        );
        assert_eq!(line.label_side, LabelSide::Above);
        assert_eq!(line.label_at, 0.5);
        let arrow = connector(1);
        assert_eq!(
            (arrow.route, arrow.head, arrow.tail),
            (Route::Curve, Head::Dot, Head::Open)
        );
        assert_eq!(arrow.label_side, LabelSide::Right);
        assert_eq!(arrow.label_at, 0.25);
        assert_eq!((line.head_size, arrow.head_size), (None, Some(4.0)));
        assert_eq!(
            connector_head("arrow { from 0 0; to 5 5; }"),
            Head::Triangle
        );
        assert_eq!(
            connector_head("arrow { from 0 0; to 5 5; head none; }"),
            Head::None
        );
    }

    fn connector_head(node: &str) -> Head {
        let s = scene(&format!("canvas {{ size 10 10; {node} }}")).unwrap();
        match &s.objects[0].kind {
            Kind::Connector(c) => c.head,
            other => panic!("expected a connector, got {other:?}"),
        }
    }

    #[test]
    fn an_unknown_route_or_head_is_an_error() {
        let (_, _, message) =
            scene("canvas { size 10 10; line { from 0 0; to 5 5; route bent; } }").unwrap_err();
        assert_eq!(message, "`route` is one of straight, elbow, curve");
        let (_, _, message) =
            scene("canvas { size 10 10; line { from 0 0; to 5 5; head big; } }").unwrap_err();
        assert_eq!(message, "`head` is one of none, triangle, open, dot");
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

    fn shape_at(s: &Scene, i: usize) -> Shape {
        match &s.objects[i].kind {
            Kind::Shape(shape) => shape.clone(),
            other => panic!("expected a shape, got {other:?}"),
        }
    }

    #[test]
    fn a_marker_is_a_numbered_circle_centered_where_it_is_put() {
        let s = scene(
            "canvas { size 300 100;
                box a { at 100 20; size 100 40; }
                marker { at 50 50; number 3; }
                marker { at a right; number 4; }
                marker { at a; number 5; } }",
        )
        .unwrap();
        let [m, side, center] = [1, 2, 3].map(|i| shape_at(&s, i));
        assert!(m.oval);
        assert_eq!(m.center(), Point::new(50.0, 50.0));
        assert_eq!(m.label.as_ref().unwrap().text(), "3");
        assert_eq!(side.center(), Point::new(210.0, 40.0));
        assert_eq!(center.center(), Point::new(150.0, 40.0));
    }

    #[test]
    fn style_marker_sets_the_badge_of_markers_and_numbered_text() {
        let s = scene(
            "canvas { size 300 100;
                let red #ff0000;
                style marker { fill red; diameter 30; }
                marker { at 20 20; number 1; }
                marker { at 60 20; number 2; fill #00ff00; }
                text { at 0 50; number 3; text \"hi\"; width 200; } }",
        )
        .unwrap();
        let red = Some(Color::rgba(1.0, 0.0, 0.0, 1.0));
        let (a, b, c) = (shape_at(&s, 0), shape_at(&s, 1), shape_at(&s, 2));
        assert_eq!((a.fill, a.size.x), (red, 30.0));
        assert_eq!(b.fill, Some(Color::rgba(0.0, 1.0, 0.0, 1.0)));
        assert_eq!((c.fill, c.size.x), (red, 30.0));
        assert_eq!(c.label.as_ref().unwrap().text(), "3");
        match &s.objects[3].kind {
            Kind::Text(t) => {
                assert_eq!(t.origin.x, 38.0);
                assert_eq!(t.label.width, Some(162.0));
            }
            other => panic!("expected text, got {other:?}"),
        }
    }

    #[test]
    fn a_marker_needs_a_place_and_a_number() {
        assert!(scene("canvas { size 9 9; marker { number 1; } }").is_err());
        assert!(scene("canvas { size 9 9; marker { at 1 1; } }").is_err());
        assert!(scene("canvas { size 9 9; marker { at 1 1; number 1.5; } }").is_err());
        assert!(scene("canvas { size 9 9; text { text \"a\"; number 1; align right; } }").is_err());
    }

    const BOLD: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Bold.ttf");
    const MATH: &[u8] = include_bytes!("../../assets/fonts/NotoSansMath-Regular.ttf");
    const SANS: &[u8] = include_bytes!("../../assets/fonts/NotoSans-Regular.ttf");

    /// The scene `src` builds with `fonts` as the contents of the files
    /// it lists.
    fn scene_with(src: &str, fonts: &[&[u8]]) -> Result<Scene, (usize, usize, String)> {
        let program = parse(src).unwrap();
        let bytes: Vec<Vec<u8>> = fonts.iter().map(|f| f.to_vec()).collect();
        build(&program, &bytes, &[]).map_err(|e| {
            let (line, col) = e.line_col(src);
            (line, col, e.message)
        })
    }

    #[test]
    fn the_files_a_canvas_lists_come_in_source_order_with_their_places() {
        let src = "canvas {\n  size 9 9;\n  fonts {\n    serif \"a/serif.ttf\";\n    math \"m.otf\";\n  }\n}";
        let files = font_files(&parse(src).unwrap()).unwrap();
        let named: Vec<_> = files
            .iter()
            .map(|f| (f.name.as_str(), f.path.as_str()))
            .collect();
        assert_eq!(named, [("serif", "a/serif.ttf"), ("math", "m.otf")]);
        let at = files[0].span.start;
        assert_eq!(&src[at..at + 13], "\"a/serif.ttf\"");
        assert!(
            font_files(&parse("canvas { size 9 9; }").unwrap())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_registered_font_is_a_name_font_takes_and_text_is_measured_in_it() {
        let src = "canvas { size 200 50; fonts { heavy \"h.ttf\"; }
            text { at 0 0; text \"Hello\"; font heavy; }
            text { at 0 20; text \"Hello\"; font sans-bold; } }";
        let s = scene_with(src, &[BOLD]).unwrap();
        let (Kind::Text(a), Kind::Text(b)) = (&s.objects[0].kind, &s.objects[1].kind) else {
            panic!("expected texts");
        };
        assert_eq!(a.label.runs[0].font, Font::Custom(0));
        assert_eq!(a.label.size(), b.label.size());
        assert_eq!(s.fonts.bytes(Font::Custom(0)).as_ref(), BOLD);
    }

    #[test]
    fn a_registered_font_can_be_the_canvas_default() {
        let src = "canvas { size 200 50; font heavy; fonts { heavy \"h.ttf\"; }
            text { at 0 0; text \"Hello\"; } }";
        let s = scene_with(src, &[BOLD]).unwrap();
        let Kind::Text(t) = &s.objects[0].kind else {
            panic!("expected text");
        };
        assert_eq!(t.label.runs[0].font, Font::Custom(0));
    }

    #[test]
    fn a_font_no_fonts_node_names_is_an_error() {
        let (_, _, message) =
            scene("canvas { size 9 9; text { at 0 0; text \"a\"; font heavy; } }").unwrap_err();
        assert!(message.contains("`fonts`"), "{message}");
    }

    #[test]
    fn fonts_that_do_not_load_are_errors_naming_the_file_and_line() {
        let src = "canvas { size 9 9;\n fonts {\n  a \"notes.txt\";\n }\n}";
        let (line, _, message) = scene_with(src, &[b"not a font"]).unwrap_err();
        assert_eq!(line, 3);
        assert!(
            message.contains("`notes.txt` is not a font file"),
            "{message}"
        );

        let src = "canvas { size 9 9;\n fonts {\n  math \"text.ttf\";\n }\n}";
        let (line, _, message) = scene_with(src, &[SANS]).unwrap_err();
        assert_eq!(line, 3);
        assert!(
            message.contains("`text.ttf` has no MATH table"),
            "{message}"
        );

        let (_, _, message) = scene_with(src, &[]).unwrap_err();
        assert!(message.contains("lists 1 files"), "{message}");
    }

    #[test]
    fn a_font_name_is_taken_once_and_never_an_embedded_one() {
        let (_, _, message) =
            scene_with("canvas { size 9 9; fonts { sans \"a.ttf\"; } }", &[SANS]).unwrap_err();
        assert!(message.contains("`sans`"), "{message}");
        let twice = "canvas { size 9 9; fonts { a \"a.ttf\"; a \"b.ttf\"; } }";
        assert!(font_files(&parse(twice).unwrap()).is_err());
        let not_a_path = "canvas { size 9 9; fonts { a 3; } }";
        assert!(font_files(&parse(not_a_path).unwrap()).is_err());
    }

    #[test]
    fn points_are_pairs_from_the_group_origin_and_may_be_lets_or_sums() {
        let s = scene(
            "canvas { size 200 200; let w 40; let start 0 0;
                group { at 10 20; polygon { points start w 0 (w / 2) (w + 10); } }
                polyline { points 0 0 5 5 5 5 9 0; } }",
        )
        .unwrap();
        let Kind::Polygon(p) = &s.objects[0].kind else {
            panic!("expected a polygon");
        };
        assert_eq!(
            p.points,
            [
                Point::new(10.0, 20.0),
                Point::new(50.0, 20.0),
                Point::new(30.0, 70.0)
            ]
        );
        assert_eq!(p.fill, Some(Color::WHITE));
        assert_eq!(p.stroke, Some(Stroke::solid(Color::BLACK, STROKE_WIDTH)));
        let Kind::Polyline(l) = &s.objects[1].kind else {
            panic!("expected a polyline");
        };
        assert_eq!(l.points.len(), 3);
        assert_eq!(
            s.bounds(0, 0.0),
            Some((p.points[0], Point::new(50.0, 70.0)))
        );
        assert_eq!(s.object_at(Point::new(30.0, 40.0), 0.0), Some(0));
    }

    #[test]
    fn points_too_few_or_odd_point_at_the_points_line() {
        assert_eq!(
            scene("canvas { size 10 10;\n  polygon { points 0 0 5 5; } }").unwrap_err(),
            (2, 13, "a `polygon` needs at least three points".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  polyline { points 0 0; } }").unwrap_err(),
            (2, 14, "a `polyline` needs at least two points".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n  polyline { points 0 0 5; } }").unwrap_err(),
            (
                2,
                14,
                "`points` takes pairs of numbers, `X Y X Y ...`, and has one left over".into()
            )
        );
    }

    /// The scene `src` builds with `data` as the contents of the data
    /// files it names.
    fn scene_data(src: &str, data: &[&[u8]]) -> Result<Scene, (usize, usize, String)> {
        let program = parse(src).unwrap();
        let bytes: Vec<Vec<u8>> = data.iter().map(|d| d.to_vec()).collect();
        build(&program, &[], &bytes).map_err(|e| {
            let (line, col) = e.line_col(src);
            (line, col, e.message)
        })
    }

    #[test]
    fn the_data_files_a_canvas_names_come_in_source_order_with_their_places() {
        let src = "canvas {\n  size 9 9;\n  data a { file \"x/a.tsv\"; }\n  data b { file \"b.CSV\"; }\n}";
        let files = data_files(&parse(src).unwrap()).unwrap();
        let named: Vec<_> = files
            .iter()
            .map(|f| (f.name.as_str(), f.path.as_str()))
            .collect();
        assert_eq!(named, [("a", "x/a.tsv"), ("b", "b.CSV")]);
        let at = files[0].span.start;
        assert_eq!(&src[at..at + 9], "\"x/a.tsv\"");
    }

    #[test]
    fn points_take_columns_of_a_data_file_one_per_row() {
        let src = "canvas { size 200 200;
            data d { file \"d.tsv\"; }
            data c { file \"c.csv\"; }
            group { at 10 20; polygon { points d x y; } }
            polyline { points c b a; } }";
        let tsv = b"y\tx\tname\n0\t0\tone\n0\t40\ttwo\n\n50\t20\tthree\n";
        let csv = b"a,b\n1,2\n3,4\n";
        let s = scene_data(src, &[tsv, csv]).unwrap();
        let Kind::Polygon(p) = &s.objects[0].kind else {
            panic!("expected a polygon");
        };
        assert_eq!(
            p.points,
            [
                Point::new(10.0, 20.0),
                Point::new(50.0, 20.0),
                Point::new(30.0, 70.0)
            ]
        );
        let Kind::Polyline(l) = &s.objects[1].kind else {
            panic!("expected a polyline");
        };
        assert_eq!(l.points, [Point::new(2.0, 1.0), Point::new(4.0, 3.0)]);
    }

    #[test]
    fn points_from_data_name_what_is_wrong_where_it_is() {
        let xy: &[u8] = b"x\ty\n0\t0\n5\tfive\n9\t9\n";
        let at = |points: &str| {
            let src = format!(
                "canvas {{ size 10 10;\n  data d {{ file \"d.tsv\"; }}\n  polygon {{ points {points}; }} }}"
            );
            scene_data(&src, &[xy]).unwrap_err()
        };
        assert_eq!(at("e x y"), (3, 20, "no `data` is named `e`".into()));
        assert_eq!(
            at("d x z"),
            (
                3,
                24,
                "`d.tsv` has no column `z`; its columns are x, y".into()
            )
        );
        assert_eq!(
            at("d x y"),
            (
                3,
                24,
                "`d.tsv` line 3 has `five` in column `y`, which is not a number".into()
            )
        );
        assert_eq!(
            at("d x"),
            (
                3,
                13,
                "`points DATA XCOL YCOL` takes two column names after the data name".into()
            )
        );
        let src =
            "canvas { size 10 10;\n  data d { file \"d.tsv\"; }\n  polyline { points d x y; } }";
        assert_eq!(
            scene_data(src, &[b"x\ty\n1\t1\n"]).unwrap_err(),
            (3, 14, "a `polyline` needs at least two points".into())
        );
        assert_eq!(
            scene_data(src, &[b"x\ty\n1\t1\n2\n"]).unwrap_err(),
            (
                2,
                17,
                "`d.tsv` line 3 has fewer cells than the 2 columns the first line names".into()
            )
        );
    }

    #[test]
    fn a_data_node_needs_one_name_a_file_and_a_known_extension() {
        let err = |src: &str| {
            let e = data_files(&parse(src).unwrap()).unwrap_err();
            let (line, col) = e.line_col(src);
            (line, col, e.message)
        };
        assert_eq!(
            err("canvas { size 9 9;\n data a { file \"a.tsv\"; }\n data a { file \"b.tsv\"; } }"),
            (3, 7, "two `data` nodes are named `a`".into())
        );
        assert_eq!(
            err("canvas { size 9 9;\n group { data a { file \"a.tsv\"; } } }"),
            (2, 10, "a `data` goes in the canvas".into())
        );
        assert_eq!(
            err("canvas { size 9 9;\n data a { } }"),
            (2, 2, "`data` needs `file`".into())
        );
        assert_eq!(
            err("canvas { size 9 9;\n data a { file \"a.txt\"; } }"),
            (
                2,
                16,
                "`a.txt` is not a .tsv or .csv file, which `data` reads".into()
            )
        );
        assert_eq!(
            err("canvas { size 9 9;\n data { file \"a.tsv\"; } }"),
            (2, 2, "a `data` needs a name".into())
        );
    }

    #[test]
    fn an_arrow_attaches_to_the_box_around_a_polygon() {
        let s = scene(
            "canvas { size 300 200;
                polygon p { points 0 0 100 0 50 80; }
                arrow { from p; to 200 40; }
                arrow { from 200 0; to p bottom; } }",
        )
        .unwrap();
        let ends = |i: usize| match &s.objects[i].kind {
            Kind::Connector(c) => (c.from, c.to),
            other => panic!("expected a connector, got {other:?}"),
        };
        assert_eq!(ends(1).0, Point::new(100.0, 40.0));
        assert_eq!(ends(2).1, Point::new(50.0, 80.0));
    }

    #[test]
    fn a_polyline_draws_its_heads_at_its_ends() {
        let s = scene(
            "canvas { size 200 200;
                polyline { points 0 0 100 0 100 100; head triangle; tail dot; } }",
        )
        .unwrap();
        let prims = s.draw(0.0);
        let Primitive::Polyline { points, .. } = &prims[0] else {
            panic!("expected the shaft, got {:?}", prims[0]);
        };
        assert!(points.last().unwrap().y < 100.0);
        let Primitive::Polygon { points, .. } = &prims[1] else {
            panic!("expected the head, got {:?}", prims[1]);
        };
        assert_eq!(points[0], Point::new(100.0, 100.0));
        assert!(matches!(prims[2], Primitive::Ellipse { .. }));
        let fill = "canvas { size 200 200; polyline l { points 0 0 9 9; }
            timeline { 1s { fill l #ff0000; } } }";
        assert_eq!(scene(fill).unwrap_err().2, "`l` has no fill");
    }

    #[test]
    fn the_math_font_is_swapped_for_a_file_with_a_math_table() {
        let src = "canvas { size 200 50; fonts { math \"m.ttf\"; }
            math { at 0 0; text \"x^2\"; } }";
        let s = scene_with(src, &[MATH]).unwrap();
        assert_eq!(s.fonts.bytes(Font::Math).as_ref(), MATH);
        let plain = scene("canvas { size 200 50; math { at 0 0; text \"x^2\"; } }").unwrap();
        let width = |s: &Scene| match &s.objects[0].kind {
            Kind::Text(t) => t.label.size().x,
            other => panic!("expected text, got {other:?}"),
        };
        assert_eq!(width(&s), width(&plain));
    }

    fn matrix_of(s: &Scene, i: usize) -> &Matrix {
        match &s.objects[i].kind {
            Kind::Matrix(m) => m,
            other => panic!("expected a matrix, got {other:?}"),
        }
    }

    /// Each text object `i` draws, with where it sits and how.
    fn texts(s: &Scene, i: usize) -> Vec<(String, Point, HAlign, VAlign)> {
        let mut prims = Vec::new();
        s.objects[i].kind.draw(0.0, &mut prims);
        prims
            .iter()
            .filter_map(|p| match p {
                Primitive::Text {
                    origin,
                    runs,
                    h_align,
                    v_align,
                    ..
                } => Some((runs[0].text.clone(), *origin, *h_align, *v_align)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_matrix_lays_out_its_cells_by_row_and_column_with_numbers_outside() {
        let s = scene(
            "canvas { size 200 200;
                matrix m { at 40 30; rows 2; columns 3; unit 20 10;
                    row-numbers 1 2; column-numbers 0 5; } }",
        )
        .unwrap();
        let m = matrix_of(&s, 0);
        assert_eq!(m.cells.len(), 6);
        assert_eq!(
            m.cell_rect(1, 2),
            (Point::new(80.0, 40.0), Point::new(20.0, 10.0))
        );
        assert_eq!(m.cells[0].fill, Some(Color::WHITE));
        assert_eq!(m.cells[0].stroke.unwrap().color, Color::BLACK);
        let row = |t: &str, y: f64| (t.into(), Point::new(34.0, y), HAlign::Right, VAlign::Middle);
        let column = |t: &str, x: f64| {
            (
                t.into(),
                Point::new(x, 24.0),
                HAlign::Center,
                VAlign::Baseline,
            )
        };
        assert_eq!(
            texts(&s, 0),
            [
                row("1", 35.0),
                row("3", 45.0),
                column("0", 50.0),
                column("5", 70.0),
                column("10", 90.0),
            ]
        );
        let (min, max) = s.bounds(0, 0.0).unwrap();
        assert!(min.x < 34.0 && min.y < 24.0);
        assert_eq!(max.x, 100.0);
    }

    #[test]
    fn a_matrix_cell_overrides_the_matrix_and_an_outlined_cell_draws_last() {
        let s = scene(
            "canvas { size 200 200;
                matrix m { at 0 0; rows 2; columns 2; unit 10 10; stroke-width 0.5; text-color #0000ff;
                    cell { at 0 0; fill #ff0000; text \"a\"; }
                    cell { at 0 0; stroke #00ff00; dash 2 1; text-color #ffffff; }
                    cell { at 1 0; fill none; stroke-width 3; } } }",
        )
        .unwrap();
        let m = matrix_of(&s, 0);
        let red = Color::rgb(1.0, 0.0, 0.0);
        let green = Color::rgb(0.0, 1.0, 0.0);
        assert_eq!(m.cells[0].fill, Some(red));
        assert_eq!(
            m.cells[0].stroke,
            Some(Stroke {
                color: green,
                width: 0.5,
                dash: Some([2.0, 1.0]),
            })
        );
        assert_eq!(
            m.cells[0].label.as_ref().unwrap().runs[0].color,
            Color::WHITE
        );
        assert_eq!(m.cells[2].fill, None);
        assert_eq!(
            m.cells[2].stroke,
            Some(Stroke {
                color: Color::BLACK,
                width: 3.0,
                dash: None,
            })
        );
        assert_eq!(m.cells[1].stroke.unwrap().width, 0.5);

        let mut prims = Vec::new();
        m.draw(&mut prims);
        let origins: Vec<Point> = prims
            .iter()
            .filter_map(|p| match p {
                Primitive::Rect { origin, .. } => Some(*origin),
                _ => None,
            })
            .collect();
        assert_eq!(
            origins,
            [
                Point::new(10.0, 0.0),
                Point::new(10.0, 10.0),
                Point::new(0.0, 0.0),
                Point::new(0.0, 10.0),
            ]
        );
    }

    #[test]
    fn matrix_cells_take_text_from_a_data_file_and_a_cell_text_wins() {
        let src = "canvas { size 200 200;
            data d { file \"d.tsv\"; }
            matrix m { at 0 0; rows 2; columns 2; unit 10 10; font sans; cells d r c score;
                cell { at 1 1; text \"x\"; } } }";
        let tsv = b"c\tr\tscore\n0\t0\t7\n1\t1\t-3\n0\t1\t\n";
        let s = scene_data(src, &[tsv]).unwrap();
        let m = matrix_of(&s, 0);
        let text: Vec<Option<String>> = m
            .cells
            .iter()
            .map(|c| c.label.as_ref().map(Label::text))
            .collect();
        assert_eq!(text, [Some("7".into()), None, None, Some("x".into())]);
        assert_eq!(m.cells[0].label.as_ref().unwrap().runs[0].font, Font::Sans);
    }

    #[test]
    fn matrix_mistakes_name_what_is_wrong_where_it_is() {
        let err = |body: &str| {
            let src = format!(
                "canvas {{ size 10 10;\n data d {{ file \"d.tsv\"; }}\n matrix m {{ at 0 0; rows 2; columns 3; unit 9 9; {body} }} }}"
            );
            let tsv: &[u8] = b"r\tc\tt\tx\n0\t0\ta\t1.5\n2\t0\tb\t1\n";
            scene_data(&src, &[tsv]).unwrap_err().2
        };
        assert_eq!(
            err("cell { at 2 0; }"),
            "the matrix has rows 0 to 1 and columns 0 to 2"
        );
        assert_eq!(
            err("cell { at 0 1.5; }"),
            "the matrix has rows 0 to 1 and columns 0 to 2"
        );
        assert_eq!(err("cell { fill none; }"), "`cell` needs `at`");
        assert_eq!(
            err("cell { at 0 0; span 2; }"),
            "`cell` has no property `span`; it takes at, text, fill, stroke, stroke-width, dash, \
             text-color, style"
        );
        assert_eq!(err("box { }"), "a `matrix` holds `cell` nodes, not `box`");
        assert_eq!(err("cells e r c t;"), "no `data` is named `e`");
        assert_eq!(
            err("cells d r col t;"),
            "`d.tsv` has no column `col`; its columns are r, c, t, x"
        );
        assert_eq!(
            err("cells d r c;"),
            "`cells DATA RCOL CCOL TEXTCOL` takes three column names after the data name"
        );
        assert_eq!(
            err("cells d x c t;"),
            "`d.tsv` line 2 has `1.5` in column `x`, which is not a whole number from 0 to 1"
        );
        assert_eq!(
            err("cells d r c t;"),
            "`d.tsv` line 3 has `2` in column `r`, which is not a whole number from 0 to 1"
        );
        assert_eq!(
            scene("canvas { size 10 10; matrix { rows 0; columns 1; unit 1 1; } }")
                .unwrap_err()
                .2,
            "`rows` takes a whole number from 1"
        );
        assert_eq!(
            scene("canvas { size 10 10; row { unit 1 1; cell { at 0 0; } } }")
                .unwrap_err()
                .2,
            "`cell` has no property `at`; it takes text, note, span, fill, stroke, stroke-width, \
             dash, text-color, style"
        );
    }

    #[test]
    fn an_arrow_and_a_brace_take_a_matrix_cell_by_row_and_column() {
        let s = scene(
            "canvas { size 300 300;
                matrix m { at 100 100; rows 3; columns 4; unit 20 10; }
                arrow { from 0 105; to m 1 2 left; }
                brace { over m 0 1 m 0 2; gap 0; }
                line { from m 2 0 bottom; to 0 0; repeat 2; }
                arrow { from m right; to 300 115; } }",
        )
        .unwrap();
        let Kind::Connector(arrow) = &s.objects[1].kind else {
            panic!("expected an arrow");
        };
        assert_eq!(arrow.to, Point::new(140.0, 115.0));
        let Kind::Brace(brace) = &s.objects[2].kind else {
            panic!("expected a brace");
        };
        assert_eq!(
            (brace.from, brace.to),
            (Point::new(120.0, 100.0), Point::new(160.0, 100.0))
        );
        let froms: Vec<Point> = s.objects[3..5]
            .iter()
            .map(|o| match &o.kind {
                Kind::Connector(c) => c.from,
                other => panic!("expected a line, got {other:?}"),
            })
            .collect();
        assert_eq!(froms, [Point::new(110.0, 130.0), Point::new(130.0, 130.0)]);
        let Kind::Connector(whole) = &s.objects[5].kind else {
            panic!("expected an arrow");
        };
        assert_eq!(whole.from, Point::new(180.0, 115.0));
    }

    #[test]
    fn a_matrix_cell_past_the_edge_or_without_a_column_is_an_error() {
        let src = |end: &str| {
            format!(
                "canvas {{ size 10 10;\n matrix m {{ at 0 0; rows 2; columns 3; unit 1 1; }}\n arrow {{ from {end}; to 9 9; }} }}"
            )
        };
        assert_eq!(
            scene(&src("m 1 3")).unwrap_err(),
            (
                3,
                17,
                "matrix `m` has rows 0 to 1 and columns 0 to 2".into()
            )
        );
        assert_eq!(
            scene(&src("m 1")).unwrap_err(),
            (
                3,
                17,
                "a cell of matrix `m` is `m R C`, its row and column".into()
            )
        );
        assert_eq!(
            scene(
                "canvas { size 10 10;\n matrix m { at 0 0; rows 2; columns 3; unit 1 1; }\n text { at m 1 1 5 5; text \"a\"; } }"
            )
            .unwrap_err()
            .2,
            "`at` takes a matrix's name and two optional numbers, or a cell, a side and how far along it"
        );
    }

    #[test]
    fn a_point_on_a_matrix_finds_it() {
        let s = scene(
            "canvas { size 300 300;
                matrix m { at 100 100; rows 3; columns 4; unit 20 10; column-numbers 0 1; } }",
        )
        .unwrap();
        assert_eq!(s.object_at(Point::new(150.0, 120.0), 0.0), Some(0));
        assert_eq!(s.object_at(Point::new(110.0, 96.0), 0.0), Some(0));
        assert_eq!(s.object_at(Point::new(150.0, 150.0), 0.0), None);
    }

    #[test]
    fn at_may_name_a_point_along_a_side_and_place_puts_a_node_by_its_own_point() {
        let s = scene(
            r#"canvas { size 500 300;
                box bar { at 0 0; size 400 20; }
                row strip { at bar bottom 0.375 0 30; place top; unit 24 24; chars "ACGN"; }
                box b { at bar right 0.5; place left; size 10 10; }
                matrix m { at 0 100; rows 2; columns 3; unit 20 10; }
                box c { at m 1 2 bottom 0.5; place top; size 10 10; }
                box d { at strip 1 left 1 -5 0; place bottom-right; size 10 10; } }"#,
        )
        .unwrap();
        let origin = |k: usize| match &s.objects[k].kind {
            Kind::Shape(shape) => shape.origin,
            Kind::Row(row) => row.origin,
            other => panic!("expected a box or a row, got {other:?}"),
        };
        // 0.375 of 400 is 150, less half the row's 96
        assert_eq!(origin(1), Point::new(102.0, 50.0));
        assert_eq!(origin(2), Point::new(400.0, 5.0));
        assert_eq!(origin(4), Point::new(45.0, 120.0));
        assert_eq!(origin(5), Point::new(111.0, 64.0));
    }

    #[test]
    fn a_connector_end_may_sit_along_a_side() {
        let s = scene(
            "canvas { size 300 200;
                box a { at 0 0; size 100 40; }
                ellipse e { at 200 100; size 100 40; }
                arrow { from a bottom 0.25; to e top 0.25; } }",
        )
        .unwrap();
        let Kind::Connector(c) = &s.objects[2].kind else {
            panic!("expected an arrow");
        };
        assert_eq!(c.from, Point::new(25.0, 40.0));
        assert_eq!(c.from_dir, Some(Point::new(0.0, 1.0)));
        // a quarter along an ellipse's top is half its half width from
        // the middle, where the outline is sqrt(3/4) of the half height
        // above the center
        assert_eq!(c.to, Point::new(225.0, 120.0 - 20.0 * 0.75f64.sqrt()));
        assert_eq!(c.to_dir, Some(Point::new(0.0, -1.0)));
    }

    #[test]
    fn place_on_text_moves_its_box_and_align_alone_keeps_it_where_it_was() {
        let s = scene(
            r#"canvas { size 300 300;
                text { at 100 100; text "word"; align center; }
                text { at 100 100; text "word"; align center; place center; }
                text { at 100 100; text "word"; place bottom-right; } }"#,
        )
        .unwrap();
        let text = |k: usize| match &s.objects[k].kind {
            Kind::Text(t) => t.clone(),
            other => panic!("expected text, got {other:?}"),
        };
        let size = text(0).label.size();
        assert_eq!(text(0).origin, Point::new(100.0, 100.0));
        assert_eq!(text(1).origin, Point::new(100.0, 100.0 - size.y / 2.0));
        assert_eq!(text(2).origin, Point::new(100.0, 100.0) - size);
    }

    #[test]
    fn a_side_in_at_needs_how_far_along_it_from_0_to_1() {
        let src = |at: &str| {
            format!(
                "canvas {{ size 10 10;\n box a {{ at 0 0; size 5 5; }}\n box {{ at {at}; size 1 1; }} }}"
            )
        };
        assert_eq!(
            scene(&src("a top")).unwrap_err(),
            (
                3,
                13,
                "a side in `at` takes how far along it, from 0 to 1".into()
            )
        );
        assert_eq!(
            scene(&src("a top 2")).unwrap_err(),
            (3, 17, "how far along a side is a number from 0 to 1".into())
        );
        assert_eq!(
            scene(&src("a middle 0.5")).unwrap_err(),
            (3, 13, "a side is top, bottom, left or right".into())
        );
        assert_eq!(
            scene("canvas { size 10 10;\n box { size 1 1; place middle; } }")
                .unwrap_err()
                .2,
            "`place` is center, top, bottom, left, right, top-left, top-right, \
             bottom-left or bottom-right"
        );
    }
}

//! The source edits that move drawn nodes by an offset or change a
//! node's text.
//!
//! A viewer that drags objects asks [`move_edits`] for the changes to
//! the numbers that place their nodes, and [`apply`] makes them.
//! One that edits an object's text reads it with [`text_of`] and asks
//! [`text_edits`] for the change to its string.
//! [`node_path`] and [`node_span`] find a node again after an edit
//! shifts its span. [`insert_edits`], [`delete_edits`], [`copy_text`]
//! and [`paste_edits`] add, delete and copy whole nodes.

mod nodes;

pub use nodes::{
    EndAt, NewNode, copy_text, delete_edits, fresh_name, insert_edits, name_edits, paste_edits,
};

use super::build::at_named;
use crate::lang::{self, Error, Node, Property, Span, Value, ValueKind};

/// The text that replaces the source in `span`, which is empty for an
/// insertion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub span: Span,
    pub text: String,
}

/// `source` with `edits` made, in any order.
///
/// # Panics
///
/// If two edits overlap, or an edit falls outside `source`.
pub fn apply(source: &str, edits: &[TextEdit]) -> String {
    let mut sorted: Vec<&TextEdit> = edits.iter().collect();
    sorted.sort_by_key(|e| e.span.start);
    let mut out = String::with_capacity(source.len());
    let mut done = 0;
    for e in sorted {
        out.push_str(&source[done..e.span.start]);
        out.push_str(&e.text);
        done = e.span.end;
    }
    out.push_str(&source[done..]);
    out
}

/// The edits to `source` that move the nodes whose spans are `nodes`,
/// as objects' [`source`](super::Object::source) gives them, `dx` pixels
/// across and `dy` down, in source order. A number keeps as many decimals
/// as it was written with, or as the offset needs up to two, whichever is
/// more.
///
/// The edits change each node's `at`, adding `DX DY` to an `at` that
/// names a thing and an `at X Y` to a node without one, or the numbers of
/// its `from`, `to` and `points`. A group's `at` moves what it holds one
/// to one, so `dx` and `dy` are the same in the node's own coordinates.
/// On a canvas whose `size` has an `auto`, the drawing moves to keep its
/// margin, so the nodes move by `dx` and `dy` against the rest.
///
/// The nodes move as one. A node inside another of `nodes` moves with it
/// and takes no edits of its own. An `at`, an end, a marker's `at` or a
/// brace's `over` that names a thing in `nodes`, or in one of them,
/// follows that thing and takes no edit.
///
/// # Errors
///
/// If `source` does not parse, no node has a span in `nodes`, or a node
/// is placed neither by numbers it sets itself nor by things that move
/// with it. An end, a marker or a brace on a thing that stays put is
/// refused. The error is the first such node's and says why.
pub fn move_edits(source: &str, nodes: &[Span], dx: f64, dy: f64) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let mut found = nodes
        .iter()
        .map(|&span| locate(&program, span))
        .collect::<Result<Vec<_>, _>>()?;
    found.sort_by_key(|(_, path)| path[path.len() - 1].span.start);
    let mut outer: Vec<(&Node, Vec<&Node>)> = Vec::new();
    for (canvas, path) in found {
        let start = path[path.len() - 1].span.start;
        if outer
            .last()
            .is_none_or(|(_, o)| o[o.len() - 1].span.end <= start)
        {
            outer.push((canvas, path));
        }
    }
    let mut moved = Vec::new();
    for (_, path) in &outer {
        nodes::defined(path[path.len() - 1], false, &mut moved);
    }
    let mut edits = Vec::new();
    for (canvas, path) in &outer {
        let (target, around) = path.split_last().expect("`locate` gives the node it finds");
        let mover = Mover {
            source,
            canvas,
            target,
            by: [dx, dy],
            moved: &moved,
            paste: false,
        };
        mover.check_place(around)?;
        edits.extend(mover.edits()?.ok_or_else(|| no_place(target))?);
    }
    edits.retain(|e| source[e.span.start..e.span.end] != e.text);
    edits.sort_by_key(|e| e.span.start);
    Ok(edits)
}

/// Why `node` has no place of its own to move.
fn no_place(node: &Node) -> Error {
    if let Some(p) = own(node, "over") {
        return Error::new(
            p.span,
            "the brace is set by the things it brackets; move them instead",
        );
    }
    match node.kind.text.as_str() {
        "caption" => Error::new(
            node.kind.span,
            "a caption sits under what comes before it and has no place of its own",
        ),
        other => Error::new(node.kind.span, format!("a `{other}` has no place to move")),
    }
}

/// The text of the node whose span is `node`, markup included, as
/// [`text_edits`] changes it, or `None` when the node's kind takes text
/// and the node sets none.
///
/// # Errors
///
/// As [`text_edits`].
pub fn text_of(source: &str, node: Span) -> Result<Option<String>, Error> {
    let program = lang::parse(source)?;
    let (canvas, path) = locate(&program, node)?;
    let (target, around) = path.split_last().expect("`locate` gives the node it finds");
    let mover = Mover {
        source,
        canvas,
        target,
        by: [0.0; 2],
        moved: &[],
        paste: false,
    };
    let text = mover.text(around)?;
    Ok(text.set.map(|(_, s)| s.to_string()))
}

/// The edits to `source` that set the text of the node whose span is
/// `node` to `new_text`, written as a string literal that reads back as
/// `new_text`.
///
/// A node's text is its `text` on a box, an ellipse, a text, a math, a
/// code or a caption, and its `label` on a line, an arrow, a sweep, a
/// merge or a brace. A `"""` string stays one when it can hold the new
/// text. A node without the property gets one before its first property,
/// as [`move_edits`] adds an `at`. There are no edits when the text is
/// unchanged, or when the node has none and `new_text` is empty.
///
/// # Errors
///
/// If `source` does not parse, no node has the span `node`, its kind has
/// no text, or its text is not its own to change: it comes from a `let`
/// or a style, the node is a copy of a `repeat`, or the timeline sets
/// it. Also if no string literal reads back as `new_text`.
pub fn text_edits(source: &str, node: Span, new_text: &str) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let (canvas, path) = locate(&program, node)?;
    let (target, around) = path.split_last().expect("`locate` gives the node it finds");
    let mover = Mover {
        source,
        canvas,
        target,
        by: [0.0; 2],
        moved: &[],
        paste: false,
    };
    let NodeText { key, set } = mover.text(around)?;
    let unwritable = || {
        Error::new(
            target.kind.span,
            "no string literal holds this text; edit it by hand",
        )
    };
    match set {
        Some((_, old)) if old == new_text => Ok(Vec::new()),
        Some((value, _)) => {
            let span = value.span;
            let line = &source[source[..span.start].rfind('\n').map_or(0, |k| k + 1)..];
            let indent = &line[..line.len() - line.trim_start_matches(' ').len()];
            let block = source[span.start..].starts_with("\"\"\"");
            let literal = lang::string_literal(new_text, block, indent).ok_or_else(unwritable)?;
            Ok(vec![TextEdit {
                span,
                text: literal,
            }])
        }
        None if new_text.is_empty() => Ok(Vec::new()),
        None => {
            let literal = lang::string_literal(new_text, false, "").ok_or_else(unwritable)?;
            Ok(vec![mover.insert(&format!("{key} {literal}"))])
        }
    }
}

/// The place in `program` of the node whose span is `span`: its index
/// among the top nodes, then among each parent's children in turn.
pub fn node_path(program: &lang::Program, span: Span) -> Option<Vec<usize>> {
    let mut path = Vec::new();
    let mut nodes = &program.nodes[..];
    loop {
        let i = nodes
            .iter()
            .position(|n| n.span.start <= span.start && span.end <= n.span.end)?;
        path.push(i);
        if nodes[i].span == span {
            return Some(path);
        }
        nodes = &nodes[i].children;
    }
}

/// The span of the node at `path` in `program`, as [`node_path`] gives
/// it, or `None` when no node is there.
pub fn node_span(program: &lang::Program, path: &[usize]) -> Option<Span> {
    let (&last, around) = path.split_last()?;
    let mut nodes = &program.nodes[..];
    for &i in around {
        nodes = &nodes.get(i)?.children;
    }
    Some(nodes.get(last)?.span)
}

/// The canvas of `program`, and the node whose span is `node` after the
/// nodes around it, outermost first.
fn locate(program: &lang::Program, node: Span) -> Result<(&Node, Vec<&Node>), Error> {
    let canvas = program
        .nodes
        .first()
        .ok_or_else(|| Error::new(node, "the file has no `canvas`"))?;
    let mut path = Vec::new();
    if !find(&canvas.children, node, &mut path) {
        return Err(Error::new(node, "no node in the file has this span"));
    }
    Ok((canvas, path))
}

/// Push onto `path` the node in `nodes` whose span is `span` and the
/// nodes around it, outermost first, and say whether it was found.
fn find<'a>(nodes: &'a [Node], span: Span, path: &mut Vec<&'a Node>) -> bool {
    for node in nodes {
        if node.span.start > span.start || node.span.end < span.end {
            continue;
        }
        path.push(node);
        if node.span == span || find(&node.children, span, path) {
            return true;
        }
        path.pop();
    }
    false
}

/// The property that holds a node's text.
struct NodeText<'a> {
    /// `text` or `label`.
    key: &'static str,
    /// The value and its text, when the node sets the property.
    set: Option<(&'a Value, &'a str)>,
}

/// What [`move_edits`] reads for one moved node, [`paste_edits`] for
/// one pasted node, or [`text_of`] and [`text_edits`] for their node.
/// `by` is 0 for the text.
struct Mover<'a> {
    source: &'a str,
    canvas: &'a Node,
    target: &'a Node,
    by: [f64; 2],
    /// The names given in the nodes moved or pasted together, each with
    /// whether a `repeat` copies it.
    //
    // an end, a marker or a brace on one of these things follows it, and
    // so does an `at`: those take no edit where a move of the node alone
    // refuses them or adds the offset
    moved: &'a [(&'a str, bool)],
    /// Whether the node is pasted, so that an end, a marker or a brace
    /// on any thing follows it.
    paste: bool,
}

impl Mover<'_> {
    /// The edits that move the node, or `None` when it has no place of
    /// its own.
    fn edits(&self) -> Result<Option<Vec<TextEdit>>, Error> {
        let edits = match self.target.kind.text.as_str() {
            "box" | "ellipse" | "row" | "matrix" | "text" | "math" | "table" | "ruler" | "code"
            | "group" | "panel" | "stack" | "grid" => self.at()?,
            "marker" => self.marker()?,
            "line" | "arrow" | "sweep" | "brace" => {
                if let Some(p) = own(self.target, "over") {
                    let follows = p.values.iter().all(|v| {
                        !matches!(v.kind, ValueKind::Word(_)) || self.follows(Some(v), false)
                    });
                    return Ok(follows.then(Vec::new));
                }
                let mut edits = self.end("from")?;
                edits.extend(self.end("to")?);
                edits
            }
            "merge" => self.merge()?,
            "polygon" | "polyline" => self.points()?,
            _ => return Ok(None),
        };
        Ok(Some(edits))
    }

    /// Whether `value` names a thing the node follows: one in `moved`,
    /// or in a paste any thing unless `moved_only`.
    fn follows(&self, value: Option<&Value>, moved_only: bool) -> bool {
        let Some(Value {
            kind: ValueKind::Word(w),
            ..
        }) = value
        else {
            return false;
        };
        self.paste && !moved_only
            || self
                .moved
                .iter()
                .any(|&(name, repeated)| w == name || repeated && nodes::digits_after(w, name))
    }

    /// An error unless the node's place is its own to set: not a copy
    /// of a `repeat`, not a slot in a stack or grid, and not moved or
    /// scaled by the timeline, itself or with what it sits in.
    fn check_place(&self, around: &[&Node]) -> Result<(), Error> {
        let span = self.target.kind.span;
        if around.iter().any(|n| n.kind.text == "repeat") {
            return Err(Error::new(
                span,
                "the node is drawn once for each number of a `repeat`, so moving one copy would move them all",
            ));
        }
        if let Some(parent) = around.last()
            && matches!(parent.kind.text.as_str(), "stack" | "grid")
        {
            return Err(Error::new(
                span,
                format!(
                    "a child of a {} takes the slot layout gives it; move the {0} instead",
                    parent.kind.text
                ),
            ));
        }
        for node in around.iter().copied().chain([self.target]) {
            let Some(name) = &node.name else { continue };
            if let Some(action) = self.timeline_action(&name.text, &["move", "scale"]) {
                return Err(Error::new(
                    action.span,
                    format!(
                        "the timeline {}s `{}`, so its place differs from one instant to the next",
                        action.key.text, name.text
                    ),
                ));
            }
        }
        Ok(())
    }

    /// The first action in the timeline whose key is one of `keys` and
    /// that names `name`.
    fn timeline_action(&self, name: &str, keys: &[&str]) -> Option<&Property> {
        self.canvas
            .children
            .iter()
            .filter(|n| n.kind.text == "timeline")
            .flat_map(|t| &t.children)
            .flat_map(|entry| &entry.properties)
            .find(|p| {
                keys.contains(&p.key.text.as_str())
                    && matches!(p.values.first(), Some(Value { kind: ValueKind::Word(w), .. }) if w == name)
            })
    }

    /// The node's own property `key`, or an error when it has none and
    /// takes `key` from a style, or `None` when neither sets it.
    fn property(&self, key: &str) -> Result<Option<&Property>, Error> {
        if let Some(p) = own(self.target, key) {
            return Ok(Some(p));
        }
        let named = own(self.target, "style")
            .into_iter()
            .flat_map(|p| &p.values)
            .filter_map(|v| match &v.kind {
                ValueKind::Word(w) => Some(w.as_str()),
                _ => None,
            });
        for style in [self.target.kind.text.as_str()].into_iter().chain(named) {
            let set = self.canvas.children.iter().any(|n| {
                n.kind.text == "style"
                    && n.name.as_ref().is_some_and(|w| w.text == style)
                    && own(n, key).is_some()
            });
            if set {
                return Err(Error::new(
                    self.target.kind.span,
                    format!(
                        "`{key}` comes from style `{style}`, which other nodes may share; set it on the node to change it"
                    ),
                ));
            }
        }
        Ok(None)
    }

    /// An error if a value of `p` comes from a `let` or a `( ... )`.
    fn literal(&self, p: &Property) -> Result<(), Error> {
        for v in &p.values {
            match &v.kind {
                ValueKind::Group(_) => {
                    return Err(Error::new(
                        v.span,
                        format!(
                            "`{}` is worked out from `( ... )`; edit it by hand",
                            p.key.text
                        ),
                    ));
                }
                ValueKind::Word(w) if self.is_let(w) => {
                    return Err(Error::new(
                        v.span,
                        format!(
                            "`{w}` comes from a `let`, which other nodes may share; edit it by hand"
                        ),
                    ));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The node's text, or an error when it is not the node's own to
    /// change.
    fn text(&self, around: &[&Node]) -> Result<NodeText<'_>, Error> {
        let span = self.target.kind.span;
        let kind = self.target.kind.text.as_str();
        let key = match kind {
            "box" | "ellipse" | "text" | "math" | "code" | "caption" => "text",
            "line" | "arrow" | "sweep" | "merge" | "brace" => "label",
            "row" | "matrix" | "table" => {
                return Err(Error::new(
                    span,
                    format!("the text of a {kind} is in its cells, which no edit picks out yet"),
                ));
            }
            _ => return Err(Error::new(span, format!("a `{kind}` has no text"))),
        };
        if around.iter().any(|n| n.kind.text == "repeat") {
            return Err(Error::new(
                span,
                "the node is drawn once for each number of a `repeat`, so its text is every copy's",
            ));
        }
        if let Some(name) = &self.target.name
            && let Some(action) = self.timeline_action(&name.text, &["text"])
        {
            return Err(Error::new(
                action.span,
                format!(
                    "the timeline sets the text of `{}`, so it differs from one instant to the next",
                    name.text
                ),
            ));
        }
        let Some(p) = self.property(key)? else {
            return Ok(NodeText { key, set: None });
        };
        self.literal(p)?;
        match p.values.as_slice() {
            [
                v @ Value {
                    kind: ValueKind::Str(s),
                    ..
                },
            ] => Ok(NodeText {
                key,
                set: Some((v, s.as_str())),
            }),
            _ => Err(p.expected("a string")),
        }
    }

    fn is_let(&self, word: &str) -> bool {
        self.canvas.properties.iter().any(|p| {
            p.key.text == "let"
                && matches!(p.values.first(), Some(Value { kind: ValueKind::Word(w), .. }) if w == word)
        })
    }

    /// The edits to the node's `at`: its X and Y, its DX and DY after a
    /// named thing, or a new `at` when it has none.
    fn at(&self) -> Result<Vec<TextEdit>, Error> {
        let Some(p) = self.property("at")? else {
            return Ok(self.new_at().into_iter().collect());
        };
        self.literal(p)?;
        if self.follows(p.values.first(), true) {
            return Ok(Vec::new());
        }
        if let [first, ..] = p.values.as_slice()
            && let ValueKind::Number(_) = first.kind
        {
            return self.pair(p, &p.values);
        }
        let named = at_named(&p.values).len();
        let rest = match &p.values[named..] {
            [side, _, rest @ ..] if matches!(side.kind, ValueKind::Word(_)) => rest,
            rest => rest,
        };
        if rest.is_empty() {
            let end = p.values.last().map_or(p.key.span.end, |v| v.span.end);
            return Ok(self
                .offsets()
                .map(|offsets| TextEdit {
                    span: Span { start: end, end },
                    text: format!(" {offsets}"),
                })
                .into_iter()
                .collect());
        }
        self.pair(p, rest)
    }

    /// The edits to a marker's `at`, which takes no offset from a named
    /// thing.
    fn marker(&self) -> Result<Vec<TextEdit>, Error> {
        let Some(p) = self.property("at")? else {
            return Ok(Vec::new());
        };
        self.literal(p)?;
        if self.follows(p.values.first(), false) {
            return Ok(Vec::new());
        }
        match p.values.first() {
            Some(Value {
                kind: ValueKind::Word(name),
                ..
            }) => Err(Error::new(
                p.span,
                format!("the marker sits on `{name}` and its `at` takes no offset; move `{name}`"),
            )),
            _ => self.pair(p, &p.values),
        }
    }

    /// The edits to the end `key` of a line, an arrow, a sweep or a
    /// brace, which must be a point.
    fn end(&self, key: &str) -> Result<Vec<TextEdit>, Error> {
        let Some(p) = self.property(key)? else {
            return Ok(Vec::new());
        };
        self.end_point(p)
    }

    fn end_point(&self, p: &Property) -> Result<Vec<TextEdit>, Error> {
        self.literal(p)?;
        if self.follows(p.values.first(), false) {
            return Ok(Vec::new());
        }
        if let Some(Value {
            kind: ValueKind::Word(name),
            ..
        }) = p.values.first()
        {
            return Err(Error::new(
                p.span,
                format!(
                    "`{}` names `{name}` and follows it; move `{name}`",
                    p.key.text
                ),
            ));
        }
        self.pair(p, &p.values)
    }

    /// The edits to every `from`, the `to` and the `at` of a merge.
    fn merge(&self) -> Result<Vec<TextEdit>, Error> {
        let mut edits = Vec::new();
        let froms: Vec<&Property> = self
            .target
            .properties
            .iter()
            .filter(|p| p.key.text == "from")
            .collect();
        if froms.is_empty() {
            self.property("from")?;
        }
        for p in froms {
            edits.extend(self.end_point(p)?);
        }
        edits.extend(self.end("to")?);
        if let Some(p) = self.property("at")? {
            self.literal(p)?;
            edits.extend(self.pair(p, &p.values)?);
        }
        Ok(edits)
    }

    /// The edits to every point of a polygon's or a polyline's
    /// `points`.
    fn points(&self) -> Result<Vec<TextEdit>, Error> {
        let Some(p) = self.property("points")? else {
            return Ok(Vec::new());
        };
        self.literal(p)?;
        if let Some(Value {
            kind: ValueKind::Word(data),
            ..
        }) = p.values.first()
        {
            return Err(Error::new(
                p.span,
                format!(
                    "the points come from data `{data}`; move the node by putting it in a group"
                ),
            ));
        }
        let mut edits = Vec::new();
        for pair in p.values.chunks(2) {
            edits.extend(self.pair(p, pair)?);
        }
        Ok(edits)
    }

    /// The edits that add the offset to `values`, two numbers of `p`.
    fn pair(&self, p: &Property, values: &[Value]) -> Result<Vec<TextEdit>, Error> {
        let [x, y] = values else {
            return Err(p.expected("two numbers"));
        };
        [(x, self.by[0]), (y, self.by[1])]
            .into_iter()
            .map(|(v, by)| {
                let ValueKind::Number(n) = v.kind else {
                    return Err(Error::new(v.span, "expected a number"));
                };
                let written = &self.source[v.span.start..v.span.end];
                let decimals = written.split_once('.').map_or(0, |(_, f)| f.len());
                Ok(TextEdit {
                    span: v.span,
                    text: number(n + by, decimals.max(places(by))),
                })
            })
            .collect()
    }

    /// The offset as two numbers, or `None` when both are 0.
    fn offsets(&self) -> Option<String> {
        let [x, y] = self.by.map(|n| number(n, places(n)));
        (x != "0" || y != "0").then(|| format!("{x} {y}"))
    }

    /// The insertion of `at DX DY`, or `None` when the offset is 0.
    fn new_at(&self) -> Option<TextEdit> {
        let offsets = self.offsets()?;
        Some(self.insert(&format!("at {offsets}")))
    }

    /// The insertion of `property;` before the node's first property or
    /// child, on its line when it shares the brace's line and on a line
    /// of its own at the same indent when not.
    fn insert(&self, property: &str) -> TextEdit {
        let node = self.target;
        let open = open_brace(self.source, node);
        let first = node
            .properties
            .iter()
            .map(|p| p.span.start)
            .chain(node.children.iter().map(|c| c.span.start))
            .min();
        let (at, text) = match first {
            Some(start) => {
                let between = &self.source[open..start];
                let text = match between.rfind('\n') {
                    Some(k) => format!("{property};\n{}", &between[k + 1..]),
                    None => format!("{property}; "),
                };
                (start, text)
            }
            None => (open + 1, format!(" {property}; ")),
        };
        TextEdit {
            span: Span { start: at, end: at },
            text,
        }
    }
}

/// The property `key` written in `node` itself.
fn own<'a>(node: &'a Node, key: &str) -> Option<&'a Property> {
    node.properties.iter().find(|p| p.key.text == key)
}

/// The offset of the `{` that opens the body of `node`.
fn open_brace(source: &str, node: &Node) -> usize {
    let head = node
        .head
        .last()
        .map(|v| v.span.end)
        .into_iter()
        .chain(node.name.as_ref().map(|n| n.span.end))
        .fold(node.kind.span.end, usize::max);
    let bytes = source.as_bytes();
    let mut i = head;
    loop {
        match bytes[i] {
            b'{' => return i,
            b'/' => {
                while bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => i += 1,
        }
    }
}

/// The fewest decimals, up to two, that write `n` rounded to two.
fn places(n: f64) -> usize {
    let hundredths = (n * 100.0).round() as i64;
    if hundredths % 100 == 0 {
        0
    } else if hundredths % 10 == 0 {
        1
    } else {
        2
    }
}

/// `n` rounded to `decimals` places and written with that many, with no
/// sign on a zero.
fn number(n: f64, decimals: usize) -> String {
    let scale = 10f64.powi(decimals as i32);
    let n = (n * scale).round() / scale;
    let n = if n == 0.0 { 0.0 } else { n };
    format!("{n:.decimals$}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{format, parse};
    use crate::scene::{Scene, build};

    fn scene(src: &str) -> Scene {
        build(&parse(src).unwrap(), &[], &[]).unwrap()
    }

    /// `src` in acre's layout, with the node of object `index` moved by
    /// `dx` and `dy`, checked as [`moved_spans`] checks it.
    fn moved(src: &str, index: usize, dx: f64, dy: f64) -> String {
        let src = format(src).unwrap();
        let span = scene(&src).objects[index].source;
        moved_spans(&src, &[span], dx, dy)
    }

    /// `src` in acre's layout, with the nodes that start with `nodes`
    /// moved together by `dx` and `dy`, checked as [`moved_spans`]
    /// checks it.
    fn moved_together(src: &str, nodes: &[&str], dx: f64, dy: f64) -> String {
        let src = format(src).unwrap();
        let spans: Vec<Span> = nodes.iter().map(|n| span_of(&src, n)).collect();
        moved_spans(&src, &spans, dx, dy)
    }

    /// `src`, in acre's layout, with the nodes at `spans` moved by `dx`
    /// and `dy`, after checking that the result keeps that layout and
    /// that the box of every object the nodes draw moved by exactly `dx`
    /// and `dy`.
    fn moved_spans(src: &str, spans: &[Span], dx: f64, dy: f64) -> String {
        let before = scene(src);
        let edits = move_edits(src, spans, dx, dy).unwrap();
        let out = apply(src, &edits);
        assert_eq!(format(&out).unwrap(), out);
        let after = scene(&out);
        assert_eq!(before.objects.len(), after.objects.len());
        for (k, o) in before.objects.iter().enumerate() {
            let inside = |s: &Span| s.start <= o.source.start && o.source.end <= s.end;
            if !spans.iter().any(inside) {
                continue;
            }
            let (min0, max0) = before.bounds(k, 0.0).unwrap();
            let (min1, max1) = after.bounds(k, 0.0).unwrap();
            for (a, b) in [(min0, min1), (max0, max1)] {
                assert!((b.x - a.x - dx).abs() < 1e-9, "{a:?} to {b:?} in\n{out}");
                assert!((b.y - a.y - dy).abs() < 1e-9, "{a:?} to {b:?} in\n{out}");
            }
        }
        out
    }

    /// The span of the node of `src` that starts with `node`.
    fn span_of(src: &str, node: &str) -> Span {
        let program = parse(src).unwrap();
        let start = src.find(node).unwrap();
        let mut stack: Vec<&Node> = program.nodes.iter().collect();
        while let Some(n) = stack.pop() {
            if n.span.start == start {
                return n.span;
            }
            stack.extend(&n.children);
        }
        panic!("no node starts at `{node}`");
    }

    /// Why the node of `src` that starts with `node` cannot move.
    fn refusal(src: &str, node: &str) -> String {
        move_edits(src, &[span_of(src, node)], 5.0, 3.0)
            .unwrap_err()
            .message
    }

    #[test]
    fn apply_makes_edits_in_any_order() {
        let edits = [
            TextEdit {
                span: Span { start: 7, end: 8 },
                text: "B".into(),
            },
            TextEdit {
                span: Span { start: 0, end: 0 },
                text: ">".into(),
            },
            TextEdit {
                span: Span { start: 2, end: 4 },
                text: "".into(),
            },
        ];
        assert_eq!(apply("a cd e b f", &edits), ">a  e B f");
    }

    #[test]
    fn an_at_of_two_numbers_takes_the_offset() {
        let out = moved(
            "canvas { size 300 200; box { at 40 60; size 50 30; text \"a\"; } }",
            0,
            7.0,
            -9.0,
        );
        assert!(out.contains("at 47 51;"), "{out}");
    }

    #[test]
    fn a_number_keeps_its_decimals_or_the_offsets_up_to_two() {
        let src = "canvas { size 300 200; box { at 40 60.5; size 50 30; } }\n";
        let span = scene(src).objects[0].source;
        let out = apply(src, &move_edits(src, &[span], 3.0, 2.0).unwrap());
        assert!(out.contains("at 43 62.5;"), "{out}");
        let out = apply(src, &move_edits(src, &[span], 3.5, 0.25).unwrap());
        assert!(out.contains("at 43.5 60.75;"), "{out}");
        let out = apply(src, &move_edits(src, &[span], 1.004, -0.333).unwrap());
        assert!(out.contains("at 41 60.17;"), "{out}");

        let src = "canvas { size 300 200; box { at 3 0.25; size 50 30; } }\n";
        let span = scene(src).objects[0].source;
        let out = apply(src, &move_edits(src, &[span], -3.0, -0.25).unwrap());
        assert!(out.contains("at 0 0.00;"), "{out}");

        let src = "canvas { size 300 200; box { size 50 30; } }\n";
        let span = scene(src).objects[0].source;
        let out = apply(src, &move_edits(src, &[span], 2.5, -0.004).unwrap());
        assert!(out.contains("at 2.5 0;"), "{out}");
    }

    #[test]
    fn a_move_that_rounds_to_nothing_makes_no_edits() {
        let src = "canvas { size 300 200; box { size 50 30; } box { at 4 4; size 5 5; } }\n";
        let s = scene(src);
        for o in &s.objects {
            assert_eq!(
                move_edits(src, &[o.source], 0.002, -0.004).unwrap(),
                Vec::new()
            );
        }
    }

    #[test]
    fn an_at_from_a_thing_takes_the_offset_in_its_dx_and_dy() {
        let base = "box a { at 20 20; size 60 40; }
            row r { at 20 100; unit 20 20; cells \"x\" \"y\"; }
            matrix m { at 150 20; unit 20 20; rows 2; columns 2; }";
        let cases = [
            ("at a 10 0;", "at a 15 3;"),
            ("at a;", "at a 5 3;"),
            ("at r 1;", "at r 1 5 3;"),
            ("at r 1 2 2;", "at r 1 7 5;"),
            ("at a right 0.5;", "at a right 0.5 5 3;"),
            ("at a bottom 0.25 -10 4;", "at a bottom 0.25 -5 7;"),
            ("at m 1 1 bottom 0.5;", "at m 1 1 bottom 0.5 5 3;"),
        ];
        for (at, want) in cases {
            let src = format!("canvas {{ size 300 200; {base} box {{ {at} size 10 10; }} }}");
            let out = moved(&src, 3, 5.0, 3.0);
            assert!(out.contains(want), "{at} gave\n{out}");
        }
    }

    #[test]
    fn a_node_without_at_gets_one_on_the_line_of_its_first_property() {
        let out = moved(
            "canvas { size 300 200; box { size 50 30; fill #ff0000; } }",
            0,
            12.0,
            8.0,
        );
        assert!(out.contains("box { at 12 8; size 50 30;"), "{out}");
    }

    #[test]
    fn a_node_without_at_gets_one_on_a_line_of_its_own_when_its_body_has_lines() {
        let src = "canvas {
    size 300 200;
    text {
        // the title
        text \"title\";
        size 20;
    }
}
";
        let out = moved(src, 0, 12.0, 8.0);
        assert!(
            out.contains("        // the title\n        at 12 8;\n        text \"title\";"),
            "{out}"
        );
    }

    #[test]
    fn a_group_without_at_gets_one_before_its_first_child() {
        let src = "canvas {
    size 300 200;
    group g {
        box { at 10 10; size 20 20; }
    }
    group h { box { at 10 10; size 20 20; } }
}
";
        let s = scene(src);
        let out = apply(
            src,
            &move_edits(src, &[span_of(src, "group g")], 4.0, 6.0).unwrap(),
        );
        assert!(
            out.contains("group g {\n        at 4 6;\n        box"),
            "{out}"
        );
        let out = apply(
            &out,
            &move_edits(&out, &[span_of(&out, "group h")], 4.0, 6.0).unwrap(),
        );
        assert!(out.contains("group h { at 4 6; box"), "{out}");
        let after = scene(&out);
        for k in 0..2 {
            assert_eq!(
                after.bounds(k, 0.0).unwrap().0,
                s.bounds(k, 0.0).unwrap().0 + crate::scene::Point::new(4.0, 6.0)
            );
        }
    }

    #[test]
    fn an_empty_body_gets_an_at_between_its_braces() {
        let src = "canvas { size 300 200; group {} }\n";
        let program = parse(src).unwrap();
        let span = program.nodes[0].children[0].span;
        let out = apply(src, &move_edits(src, &[span], 4.0, 6.0).unwrap());
        assert_eq!(out, "canvas { size 300 200; group { at 4 6; } }\n");
        assert_eq!(format(&out).unwrap(), out);
    }

    #[test]
    fn a_node_in_a_group_moves_one_to_one() {
        let src = "canvas {
            size 400 300;
            group { at 100 50; group { at 30 30; box { at 10 10; size 40 20; } } }
        }";
        let out = moved(src, 0, -25.0, 14.0);
        assert!(out.contains("box { at -15 24;"), "{out}");
    }

    #[test]
    fn a_whole_group_moves_by_its_at() {
        let src = "canvas {
            size 400 300;
            group { at 100 50; tag \"(a)\"; box { at 10 10; size 40 20; } }
        }";
        // object 1 is the group's tag, whose source is the group
        let out = moved(src, 1, 6.0, 4.0);
        assert!(out.contains("group { at 106 54;"), "{out}");
        moved(&out, 0, 6.0, 4.0);
    }

    #[test]
    fn placed_rotated_and_numbered_text_moves_by_its_at() {
        for text in [
            "text { at 100 100; text \"turned\"; rotate 30; }",
            "text { at 100 100; text \"centered\"; place center; }",
            "text { at 100 100; text \"right\"; align right; }",
            "text { at 100 100; text \"one\"; number 1; }",
            "math { at 100 100; text \"x^2\"; }",
        ] {
            let src = format!("canvas {{ size 300 300; {text} }}");
            moved(&src, 0, 11.0, -7.0);
        }
    }

    #[test]
    fn every_kind_with_an_at_moves_by_it() {
        for (node, index) in [
            ("ellipse { at 50 50; size 40 20; }", 0),
            ("row { at 50 50; unit 20 20; cells \"a\" \"b\"; }", 0),
            (
                "row { at 50 50; width 100; weights 1 2; unit 20 20; cells \"a\" \"b\"; }",
                0,
            ),
            ("matrix { at 50 50; unit 20 20; rows 2; columns 3; }", 0),
            (
                "table { at 50 50; columns 0 40; row \"a\" \"b\"; row \"c\" \"d\"; }",
                0,
            ),
            ("ruler { at 50 50; unit 20; count 4; }", 0),
            (
                "code { at 50 50; lang rust; text \"\"\"let x = 1;\"\"\"; }",
                0,
            ),
            ("panel { at 50 50; title \"p\"; box { size 20 20; } }", 0),
            (
                "stack { at 50 50; tag \"s\"; box { size 20 20; } box { size 20 20; } }",
                2,
            ),
            (
                "grid { at 50 50; columns 2; tag \"g\"; box { size 20 20; } box { size 20 20; } }",
                2,
            ),
            ("marker { at 50 50; number 3; }", 0),
        ] {
            let src = format(&format!("canvas {{ size 300 300; {node} }}")).unwrap();
            let kind = node.split(' ').next().unwrap();
            assert_eq!(scene(&src).objects[index].source, span_of(&src, kind));
            moved(&src, index, 9.0, 13.0);
        }
    }

    #[test]
    fn lines_shapes_and_braces_move_by_their_points() {
        for node in [
            "line { from 10 10; to 100 40; }",
            "arrow { from 10 10; to 100 40; route elbow; label \"x\"; }",
            "sweep { from 10 10; to 100 40; leave right; enter down; }",
            "brace { from 10 50; to 100 50; side top; label \"b\"; }",
            "polygon { points 10 10 60 10 30 50; fill #00ff00; }",
            "polyline { points 10 10 60 10.5 30 50; }",
            "merge { from 10 10; from 10 80; to 150 45; }",
            "merge { from 10 10; from 10 80; to 150 45; at 80 45; }",
        ] {
            let src = format!("canvas {{ size 300 300; {node} }}");
            moved(&src, 0, 9.0, 13.0);
        }
        let out = moved(
            "canvas { size 300 300; polyline { points 10 10 60 10.5 30 50; } }",
            0,
            2.0,
            1.0,
        );
        assert!(out.contains("points 12 11 62 11.5 32 51;"), "{out}");
    }

    #[test]
    fn a_line_inside_a_group_moves_one_to_one() {
        let src = "canvas { size 300 300; group { at 40 40; arrow { from 0 0; to 50 20; } } }";
        moved(src, 0, -4.0, 6.0);
    }

    #[test]
    fn on_an_auto_canvas_a_node_moves_against_the_rest() {
        let src =
            "canvas { size auto auto; box { at 0 0; size 20 20; } box { at 50 0; size 20 20; } }\n";
        let before = scene(src);
        let out = apply(
            src,
            &move_edits(src, &[before.objects[0].source], -10.0, 4.0).unwrap(),
        );
        let after = scene(&out);
        let gap = |s: &Scene| s.bounds(1, 0.0).unwrap().0 - s.bounds(0, 0.0).unwrap().0;
        let d = gap(&before) - gap(&after);
        assert_eq!((d.x, d.y), (-10.0, 4.0));
    }

    #[test]
    fn a_value_from_a_let_or_an_expression_is_refused() {
        let src = "canvas { size 300 200; let x 40; box b { at x 20; size 5 5; } }";
        assert!(refusal(src, "box b").contains("`x` comes from a `let`"));
        let src = "canvas { size 300 200; box b { at (20 + 20) 20; size 5 5; } }";
        assert!(refusal(src, "box b").contains("`( ... )`"));
        let src = "canvas { size 300 200; let p 10 10; line l { from p; to 50 50; } }";
        assert!(refusal(src, "line l").contains("`p` comes from a `let`"));
    }

    #[test]
    fn a_place_from_a_style_is_refused() {
        let src = "canvas { size 300 200; style s { at 10 10; } box b { style s; size 5 5; } }";
        assert!(refusal(src, "box b").contains("`at` comes from style `s`"));
        let src = "canvas { size 300 200; style box { at 10 10; } box b { size 5 5; } }";
        assert!(refusal(src, "box b").contains("`at` comes from style `box`"));
        let src = "canvas { size 300 200; style l { from 1 1; } line k { style l; to 5 5; } }";
        assert!(refusal(src, "line k").contains("`from` comes from style `l`"));
        // the node's own `at` goes over the style's
        let src =
            "canvas { size 300 200; style s { at 10 10; } box { style s; at 1 1; size 5 5; } }";
        let span = scene(src).objects[0].source;
        assert_eq!(move_edits(src, &[span], 1.0, 0.0).unwrap().len(), 1);
    }

    #[test]
    fn a_slot_in_a_stack_or_grid_is_refused() {
        let src = "canvas { size 300 200; stack { box b { size 5 5; } } }";
        assert!(refusal(src, "box b").contains("child of a stack"));
        let src = "canvas { size 300 200; grid { columns 2; box b { size 5 5; } } }";
        assert!(refusal(src, "box b").contains("child of a grid"));
        // a node in a group in a stack has its own place in the group
        let src = "canvas { size 300 200; stack { group { box { at 1 1; size 5 5; } } } }";
        moved(src, 0, 3.0, 3.0);
    }

    #[test]
    fn a_copy_of_a_repeat_is_refused() {
        let src = "canvas { size 300 200; repeat i 0 2 { box b { at 10 10; size 5 5; } } }";
        assert!(refusal(src, "box b").contains("`repeat`"));
        let src = "canvas { size 300 200; repeat i 0 2 { group { box b { size 5 5; } } } }";
        assert!(refusal(src, "box b").contains("`repeat`"));
    }

    #[test]
    fn a_node_the_timeline_moves_or_scales_is_refused() {
        let src = "canvas { size 300 200; box b { at 1 1; size 5 5; }
            timeline { 0s to 1s { move b 10 0; } } }";
        assert!(refusal(src, "box b").contains("the timeline moves `b`"));
        let src = "canvas { size 300 200; box b { at 1 1; size 5 5; }
            timeline { 0s to 1s { scale b 2; } } }";
        assert!(refusal(src, "box b").contains("the timeline scales `b`"));
        let src = "canvas { size 300 200; group g { box b { at 1 1; size 5 5; } }
            timeline { 0s to 1s { scale g 2; } } }";
        assert!(refusal(src, "box b").contains("the timeline scales `g`"));
        // a change of color leaves the place alone
        let src = "canvas { size 300 200; box b { at 1 1; size 5 5; }
            timeline { 0s to 1s { fill b #ff0000; } } }";
        moved(src, 0, 2.0, 2.0);
    }

    #[test]
    fn an_end_or_a_marker_on_a_thing_is_refused() {
        let things = "box a { at 10 10; size 20 20; } box c { at 100 10; size 20 20; }";
        for (node, why) in [
            ("arrow k { from a; to 50 50; }", "`from` names `a`"),
            ("line k { from 0 0; to c left 0.5; }", "`to` names `c`"),
            (
                "merge k { from 0 0; from a; to 50 50; }",
                "`from` names `a`",
            ),
            (
                "marker k { at a right; number 1; }",
                "the marker sits on `a`",
            ),
            ("brace k { over a c; side top; }", "the things it brackets"),
            (
                "polygon k { points d x y; }",
                "the points come from data `d`",
            ),
        ] {
            let src = format!("canvas {{ size 300 200; {things} {node} }}");
            let got = refusal(&src, node);
            assert!(got.contains(why), "{node}: {got}");
        }
    }

    #[test]
    fn a_thing_placed_at_another_moved_with_it_moves_once() {
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   box b { at a 10 0; size 20 20; } }";
        let out = moved_together(src, &["box a", "box b"], 5.0, 3.0);
        assert!(
            out.contains("at 15 13;") && out.contains("at a 10 0;"),
            "{out}"
        );
    }

    #[test]
    fn an_arrow_moved_with_both_its_ends_takes_no_edit() {
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   box b { at 100 50; size 20 20; } arrow k { from a; to b; } }";
        let out = moved_together(src, &["box a", "box b", "arrow k"], 5.0, 3.0);
        assert!(out.contains("from a;") && out.contains("to b;"), "{out}");
        assert!(
            out.contains("at 15 13;") && out.contains("at 105 53;"),
            "{out}"
        );
    }

    #[test]
    fn a_whole_diagram_moves_as_one() {
        let src = "canvas { size 400 300;
            box a { at 20 20; size 40 30; text \"a\"; }
            box b { at 150 20; size 40 30; }
            group g { at 60 150; box c { at 0 0; size 40 30; } }
            text t { at a 0 -12; text \"a label\"; }
            arrow k { from a right; to b left; label \"k\"; }
            line l { from a bottom 0.5; to c top; }
            merge m { from a; from b; to c; }
            marker n { at b right; number 1; }
            brace o { over a b; side top; label \"both\"; }
            polygon p { points 250 200 300 200 280 250; }
        }";
        let nodes = [
            "box a",
            "box b",
            "group g",
            "text t",
            "arrow k",
            "line l",
            "merge m",
            "marker n",
            "brace o",
            "polygon p",
        ];
        let out = moved_together(src, &nodes, -7.5, 4.0);
        for kept in [
            "at a 0 -12;",
            "from a right;",
            "to c top;",
            "at b right;",
            "over a b;",
        ] {
            assert!(out.contains(kept), "{kept} in\n{out}");
        }
    }

    #[test]
    fn an_end_on_a_thing_that_stays_put_is_refused_and_a_literal_end_moves() {
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   box c { at 200 10; size 20 20; } arrow k { from a; to 100 50; } }";
        let out = moved_together(src, &["box a", "arrow k"], 5.0, 3.0);
        assert!(
            out.contains("from a;") && out.contains("to 105 53;"),
            "{out}"
        );
        let spans = [span_of(src, "box c"), span_of(src, "arrow k")];
        let e = move_edits(src, &spans, 5.0, 3.0).unwrap_err();
        assert!(e.message.contains("`from` names `a`"), "{}", e.message);
        let src = "canvas { size 300 200; box a { at 10 10; size 20 20; } \
                   box c { at 200 10; size 20 20; } brace o { over a c; side top; } }";
        let spans = [span_of(src, "box a"), span_of(src, "brace o")];
        let e = move_edits(src, &spans, 5.0, 3.0).unwrap_err();
        assert!(
            e.message.contains("the things it brackets"),
            "{}",
            e.message
        );
    }

    #[test]
    fn a_node_inside_another_moved_node_takes_no_edits_of_its_own() {
        let src = "canvas { size 300 200; group g { at 100 100; box c { at 0 0; size 20 20; } } \
                   arrow k { from c; to 10 10; } }";
        let out = moved_together(src, &["group g", "box c", "arrow k"], 5.0, 0.0);
        assert!(
            out.contains("at 105 100;") && out.contains("at 0 0;"),
            "{out}"
        );
        assert!(
            out.contains("from c;") && out.contains("to 15 10;"),
            "{out}"
        );
        let span = span_of(src, "group g");
        assert_eq!(
            move_edits(src, &[span, span], 5.0, 0.0).unwrap(),
            move_edits(src, &[span], 5.0, 0.0).unwrap()
        );
    }

    #[test]
    fn an_end_on_a_copy_of_a_repeat_in_a_moved_group_follows_it() {
        let src = "canvas { size 300 200; group g { at 10 10; \
                   repeat i 0 2 { box b { at (i * 30) 0; size 20 20; } } } \
                   arrow k { from b1; to 200 100; } }";
        let out = moved_together(src, &["group g", "arrow k"], 5.0, 0.0);
        assert!(
            out.contains("from b1;") && out.contains("to 205 100;"),
            "{out}"
        );
    }

    #[test]
    fn one_node_that_cannot_move_stops_them_all() {
        let src = "canvas { size 300 200; let x 10; box a { at 10 10; size 40 40; } \
                   box b { at x 100; size 40 40; } }";
        let spans = [span_of(src, "box a"), span_of(src, "box b")];
        let e = move_edits(src, &spans, 5.0, 0.0).unwrap_err();
        assert!(e.message.contains("`let`"), "{}", e.message);
    }

    #[test]
    fn a_caption_or_a_span_that_is_no_node_is_refused() {
        let src = "canvas { size 300 200; panel { title \"p\"; caption { text \"c\"; } } }";
        assert!(refusal(src, "caption").contains("a caption sits under"));
        let err = move_edits(src, &[Span { start: 3, end: 9 }], 1.0, 1.0).unwrap_err();
        assert!(err.message.contains("no node"), "{}", err.message);
        assert!(move_edits("canvas {", &[Span::default()], 1.0, 1.0).is_err());
    }

    #[test]
    fn a_node_path_counts_siblings_down_from_the_top() {
        let src = "canvas { size 300 200; box a { at 10 10; } group { box b { at 0 0; } box c { at 5 5; } } }";
        let program = parse(src).unwrap();
        let c = span_of(src, "box c");
        assert_eq!(node_path(&program, c), Some(vec![0, 1, 1]));
        assert_eq!(node_span(&program, &[0, 1, 1]), Some(c));
        assert_eq!(node_path(&program, program.nodes[0].span), Some(vec![0]));
    }

    #[test]
    fn a_node_path_finds_the_node_again_after_a_move_shifts_its_span() {
        let src =
            format("canvas { size 300 200; box a { size 20 20; } group { box b { at 0 0; } } }")
                .unwrap();
        let b = span_of(&src, "box b");
        let path = node_path(&parse(&src).unwrap(), b).unwrap();
        let edits = move_edits(&src, &[span_of(&src, "box a")], 5.0, 5.0).unwrap();
        let out = apply(&src, &edits);
        let moved = node_span(&parse(&out).unwrap(), &path).unwrap();
        assert_ne!(moved, b);
        assert_eq!(moved, span_of(&out, "box b"));
    }

    #[test]
    fn a_span_or_path_with_no_node_has_no_answer() {
        let src = "canvas { size 300 200; box a { at 10 10; } }";
        let program = parse(src).unwrap();
        assert_eq!(node_path(&program, Span { start: 3, end: 9 }), None);
        assert_eq!(node_span(&program, &[0, 1]), None);
        assert_eq!(node_span(&program, &[1]), None);
        assert_eq!(node_span(&program, &[]), None);
    }

    /// `src` in acre's layout, with the text of the node of object
    /// `index` set to `text`, after checking that the result keeps that
    /// layout, builds, and reads back `text` for the node.
    fn retexted(src: &str, index: usize, text: &str) -> String {
        let src = format(src).unwrap();
        let span = scene(&src).objects[index].source;
        let out = apply(&src, &text_edits(&src, span, text).unwrap());
        assert_eq!(format(&out).unwrap(), out);
        let after = scene(&out);
        let path = node_path(&parse(&src).unwrap(), span).unwrap();
        let node = node_span(&parse(&out).unwrap(), &path).unwrap();
        assert_eq!(after.objects[index].source, node);
        assert_eq!(text_of(&out, node).unwrap().as_deref(), Some(text), "{out}");
        out
    }

    /// Whether object `index` of the scene of `src` draws a run of
    /// `text`.
    fn draws(src: &str, index: usize, text: &str) -> bool {
        format!("{:?}", scene(src).objects[index].kind).contains(&format!("text: {text:?}"))
    }

    /// Why the text of the node of `src` that starts with `node` cannot
    /// change.
    fn text_refusal(src: &str, node: &str) -> String {
        text_edits(src, span_of(src, node), "new")
            .unwrap_err()
            .message
    }

    #[test]
    fn every_kind_with_text_takes_a_new_text() {
        for (node, index) in [
            ("box { at 50 50; size 80 30; text \"old\"; }", 0),
            ("ellipse { at 50 50; size 80 30; text \"old\"; }", 0),
            ("text { at 50 50; text \"old\"; }", 0),
            ("line { from 10 10; to 100 40; label \"old\"; }", 0),
            ("arrow { from 10 10; to 100 40; label \"old\"; }", 0),
            (
                "sweep { from 10 10; to 100 40; leave right; enter down; label \"old\"; }",
                0,
            ),
            (
                "merge { from 10 10; from 10 80; to 150 45; label \"old\"; }",
                0,
            ),
            (
                "brace { from 10 50; to 100 50; side top; label \"old\"; }",
                0,
            ),
        ] {
            let src = format!("canvas {{ size 300 300; {node} }}");
            let out = retexted(&src, index, "say \"new\"");
            assert!(draws(&out, index, "say \"new\""), "{out}");
        }
        let src = "canvas { size 300 300; math { at 50 50; text \"x^2\"; } }";
        let out = retexted(src, 0, r"\frac{1}{2}");
        assert!(out.contains(r#"text "\\frac{1}{2}";"#), "{out}");
        let src = "canvas { size 300 300; panel { title \"p\"; box { size 20 20; } caption { text \"old\"; } } }";
        let caption = scene(src)
            .objects
            .iter()
            .position(|o| o.source == span_of(src, "caption"))
            .unwrap();
        retexted(src, caption, "new");
    }

    #[test]
    fn a_new_text_is_escaped_as_the_lexer_reads_it() {
        let src = "canvas { size 300 300; box { at 50 50; size 80 30; text \"old\"; } }";
        let out = retexted(src, 0, "a \"b\" \\ c\nd");
        assert!(out.contains(r#"text "a \"b\" \\ c\nd";"#), "{out}");
        let out = retexted(src, 0, r"*em* \*star\* $\alpha$");
        assert!(out.contains(r#"text "*em* \*star\* $\alpha$";"#), "{out}");
    }

    #[test]
    fn a_block_string_stays_a_block_string() {
        let src = "canvas {
    size 300 300;
    code {
        at 10 10;
        lang rust;
        text \"\"\"
            let x = 1;
        \"\"\";
    }
}
";
        let out = retexted(src, 0, "let x = 2;\nlet y = \"3\";\n    x + y");
        assert!(
            out.contains(
                "        text \"\"\"\n            let x = 2;\n            let y = \"3\";\n                x + y\n        \"\"\";\n"
            ),
            "{out}"
        );
        let out = retexted(&out, 0, "x");
        assert!(out.contains("text \"\"\"x\"\"\";"), "{out}");
    }

    #[test]
    fn an_unchanged_text_makes_no_edits() {
        let src = "canvas { size 300 300; box { at 50 50; size 80 30; text \"a\\nb\"; } box { at 1 1; size 5 5; } }";
        let s = scene(src);
        assert_eq!(
            text_edits(src, s.objects[0].source, "a\nb").unwrap(),
            Vec::new()
        );
        assert_eq!(
            text_edits(src, s.objects[1].source, "").unwrap(),
            Vec::new()
        );
        assert_eq!(text_of(src, s.objects[1].source).unwrap(), None);
    }

    #[test]
    fn a_node_without_text_gets_it_where_a_move_puts_an_at() {
        let out = retexted(
            "canvas { size 300 300; box { at 50 50; size 80 30; } }",
            0,
            "new",
        );
        assert!(out.contains("box { text \"new\"; at 50 50;"), "{out}");
        let out = retexted(
            "canvas { size 300 300; arrow { from 10 10; to 100 40; } }",
            0,
            "a\nb",
        );
        assert!(
            out.contains("arrow { label \"a\\nb\"; from 10 10;"),
            "{out}"
        );
    }

    #[test]
    fn a_text_that_is_not_the_nodes_own_is_refused() {
        for (src, node, why) in [
            (
                "canvas { size 300 200; let t \"x\"; box b { at 1 1; size 5 5; text t; } }",
                "box b",
                "`t` comes from a `let`",
            ),
            (
                "canvas { size 300 200; style s { text \"x\"; } box b { style s; size 5 5; } }",
                "box b",
                "`text` comes from style `s`",
            ),
            (
                "canvas { size 300 200; style arrow { label \"x\"; } arrow a { from 1 1; to 9 9; } }",
                "arrow a",
                "`label` comes from style `arrow`",
            ),
            (
                "canvas { size 300 200; repeat i 0 2 { box b { at (i * 10) 1; size 5 5; text \"x\"; } } }",
                "box b",
                "`repeat`",
            ),
            (
                "canvas { size 300 200; box b { at 1 1; size 5 5; text \"x\"; }
                    timeline { 1s { text b \"y\"; } } }",
                "box b",
                "the timeline sets the text of `b`",
            ),
            (
                "canvas { size 300 200; row r { at 1 1; unit 9 9; cells \"a\"; } }",
                "row r",
                "in its cells",
            ),
            (
                "canvas { size 300 200; polygon p { points 1 1 9 1 5 9; } }",
                "polygon p",
                "a `polygon` has no text",
            ),
        ] {
            let got = text_refusal(src, node);
            assert!(got.contains(why), "{node}: {got}");
        }
        let src = "canvas { size 300 200; box b { at 1 1; size 5 5; text \"x\"; } }";
        let err = text_edits(src, span_of(src, "box b"), "$\"$ \"\"\"").unwrap_err();
        assert!(err.message.contains("no string literal"), "{}", err.message);
    }
}

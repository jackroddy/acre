//! The source edits that add, name, delete and copy whole nodes.
//!
//! A viewer writes a new node with [`NewNode::text`], in source
//! coordinates as [`Scene::origin`](crate::scene::Scene::origin) gives
//! them, and adds it with [`insert_edits`]. [`fresh_name`] and
//! [`name_edits`] name a node a new arrow ends on. [`delete_edits`]
//! removes nodes, and [`copy_text`] and [`paste_edits`] carry them from
//! one place to another as text.

use std::collections::HashSet;

use super::{Mover, TextEdit, apply, locate, number, open_brace, places};
use crate::lang::{self, Error, Node, Program, Span, Value, ValueKind};
use crate::scene::Point;
use crate::scene::build;
use crate::scene::expand::RESERVED;

/// A node a viewer draws, in source coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum NewNode {
    Box { at: Point, size: Point },
    Ellipse { at: Point, size: Point },
    Text { at: Point, text: String },
    Line { from: EndAt, to: EndAt },
    Arrow { from: EndAt, to: EndAt },
    Polygon { points: Vec<Point> },
    Polyline { points: Vec<Point> },
}

/// Where a new line or arrow ends: a point, or the edge of the thing a
/// name names.
#[derive(Debug, Clone, PartialEq)]
pub enum EndAt {
    Point(Point),
    Name(String),
}

impl NewNode {
    /// The node on one line and without a name, such as
    /// `box { at 40 60; size 120 48; }`, with every number rounded to two
    /// decimals.
    ///
    /// # Errors
    ///
    /// If no string literal reads back as the text of a text.
    pub fn text(&self) -> Result<String, Error> {
        let point =
            |p: &Point| format!("{} {}", number(p.x, places(p.x)), number(p.y, places(p.y)));
        let end = |e: &EndAt| match e {
            EndAt::Point(p) => point(p),
            EndAt::Name(name) => name.clone(),
        };
        let points = |ps: &[Point]| ps.iter().map(point).collect::<Vec<_>>().join(" ");
        Ok(match self {
            NewNode::Box { at, size } => {
                format!("box {{ at {}; size {}; }}", point(at), point(size))
            }
            NewNode::Ellipse { at, size } => {
                format!("ellipse {{ at {}; size {}; }}", point(at), point(size))
            }
            NewNode::Text { at, text } => {
                let literal = lang::string_literal(text, false, "").ok_or_else(|| {
                    Error::new(Span::default(), "no string literal holds this text")
                })?;
                format!("text {{ at {}; text {literal}; }}", point(at))
            }
            NewNode::Line { from, to } => format!("line {{ from {}; to {}; }}", end(from), end(to)),
            NewNode::Arrow { from, to } => {
                format!("arrow {{ from {}; to {}; }}", end(from), end(to))
            }
            NewNode::Polygon { points: ps } => format!("polygon {{ points {}; }}", points(ps)),
            NewNode::Polyline { points: ps } => format!("polyline {{ points {}; }}", points(ps)),
        })
    }
}

/// The edits to `source` that add `node_text`, one or more nodes, at the
/// end of the canvas, before its timeline, so that they draw on top of
/// the rest. Lines after the first take the indent of the canvas's
/// children on top of their own.
///
/// # Errors
///
/// If `source` has no canvas, or `node_text` is not nodes that may sit
/// on a canvas.
pub fn insert_edits(source: &str, node_text: &str) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let canvas = canvas_of(&program)?;
    nodes_in(node_text)?;
    Ok(vec![insertion(source, canvas, node_text)])
}

/// A name for a new node of kind `kind`: the kind and the lowest number
/// from 1 that make a word `source` does not hold, that acre does not
/// read itself, and that no `repeat` makes of a name inside it.
///
/// # Errors
///
/// If `source` does not parse.
pub fn fresh_name(source: &str, kind: &str) -> Result<String, Error> {
    let program = lang::parse(source)?;
    Ok(Taken::of(&program).fresh(kind, false))
}

/// The edits to `source` that name the node whose span is `node` `name`.
///
/// # Errors
///
/// If `source` does not parse, no node has the span `node`, the node
/// has a name or values before its `{` already, or it is drawn once for
/// each number of a `repeat`.
pub fn name_edits(source: &str, node: Span, name: &str) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let (_, path) = locate(&program, node)?;
    let (target, around) = path.split_last().expect("`locate` gives the node it finds");
    in_repeat(target, around, "naming it would name every copy")?;
    if let Some(have) = &target.name {
        return Err(Error::new(
            have.span,
            format!("the node is named `{}` already", have.text),
        ));
    }
    if let Some(v) = target.head.first() {
        return Err(Error::new(
            v.span,
            "the node has values before its `{`, so a name has no place there",
        ));
    }
    let end = target.kind.span.end;
    Ok(vec![TextEdit {
        span: Span { start: end, end },
        text: format!(" {name}"),
    }])
}

/// The edits to `source` that delete the nodes whose spans are `nodes`,
/// each with its line when nothing else is on it, and the blank line
/// that leaves two blank lines together. A node inside another of
/// `nodes` goes with it.
///
/// # Errors
///
/// If `source` does not parse, a span is the canvas's or no node's, a
/// node is drawn once for each number of a `repeat`, or something left
/// in the file names a node that goes. Also if the file builds and would
/// no longer build: the error is the build's.
pub fn delete_edits(source: &str, nodes: &[Span]) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let targets = targets(&program, nodes, "deleting it would delete every copy")?;
    let mut names = Vec::new();
    for target in &targets {
        defined(target, false, &mut names);
    }
    let gone: Vec<Span> = targets.iter().map(|n| n.span).collect();
    for node in &program.nodes {
        refers(node, "", &names, &gone)?;
    }

    let mut ranges: Vec<Span> = Vec::new();
    for span in gone {
        match ranges.last_mut() {
            Some(last) if source[last.end..span.start].bytes().all(|b| b == b' ') => {
                last.end = span.end;
            }
            _ => ranges.push(span),
        }
    }
    let ranges: Vec<Span> = ranges.into_iter().map(|r| removal(source, r)).collect();
    let mut edits: Vec<TextEdit> = Vec::new();
    for span in ranges {
        match edits.last_mut() {
            Some(last) if last.span.end == span.start => last.span.end = span.end,
            _ => edits.push(TextEdit {
                span,
                text: String::new(),
            }),
        }
    }
    for e in &mut edits {
        e.span = without_double_blank(source, e.span);
    }

    if build(&program, &[], &[]).is_ok() {
        let out = apply(source, &edits);
        let rebuilt = lang::parse(&out).and_then(|p| build(&p, &[], &[]).map(|_| ()));
        if let Err(e) = rebuilt {
            return Err(Error::new(targets[0].kind.span, e.message));
        }
    }
    Ok(edits)
}

/// The text of the nodes whose spans are `nodes`, in source order, one
/// after another on lines of their own, with the indent of each node's
/// first line taken off the lines after it. A node inside another of
/// `nodes` goes with it.
///
/// # Errors
///
/// If `source` does not parse, a span is the canvas's or no node's, a
/// node is drawn once for each number of a `repeat`, or a node is the
/// timeline.
pub fn copy_text(source: &str, nodes: &[Span]) -> Result<String, Error> {
    let program = lang::parse(source)?;
    let targets = targets(&program, nodes, "copy the `repeat` instead")?;
    let mut texts = Vec::new();
    for node in targets {
        if node.kind.text == "timeline" {
            return Err(Error::new(
                node.kind.span,
                "a canvas holds one timeline, so it cannot be copied",
            ));
        }
        let line_start = line_start(source, node.span.start);
        let indent = source[line_start..node.span.start].len()
            - source[line_start..node.span.start]
                .trim_start_matches(' ')
                .len();
        let text = &source[node.span.start..node.span.end];
        let mut lines = text.split('\n');
        let mut out = lines.next().unwrap_or_default().to_string();
        for line in lines {
            let spaces = line.len() - line.trim_start_matches(' ').len();
            out.push('\n');
            out.push_str(&line[spaces.min(indent)..]);
        }
        texts.push(out);
    }
    Ok(texts.join("\n"))
}

/// The edit to `source` that adds `text`, nodes as [`copy_text`] gives
/// them, at the end of the canvas as [`insert_edits`] does, moved `dx`
/// pixels across and `dy` down, and the place of each added node as
/// [`node_path`](super::node_path) gives it in the edited source.
///
/// A name in `text` that `source` holds, or that a `repeat` in `source`
/// makes, becomes a fresh one as [`fresh_name`] picks it, and so does
/// every word in `text` that names it. The nodes move as
/// [`move_edits`](super::move_edits) moves them, except that an end, a
/// marker or a brace on a thing follows it, and so does an `at` on a
/// thing in `text`.
///
/// # Errors
///
/// If `source` has no canvas, `text` is not nodes that may sit on a
/// canvas, or a node of `text` cannot move as `move_edits` explains.
pub fn paste_edits(
    source: &str,
    text: &str,
    dx: f64,
    dy: f64,
) -> Result<(Vec<TextEdit>, Vec<Vec<usize>>), Error> {
    let program = lang::parse(source)?;
    let canvas = canvas_of(&program)?;
    let pasted = nodes_in(text)?;

    let (renamed, names) = renamed(&program, &pasted, text);
    let moved: Vec<(&str, bool)> = names.iter().map(|(n, r)| (n.as_str(), *r)).collect();

    let insert = insertion(source, canvas, &renamed);
    let start = insert.span.start;
    let end = start + insert.text.len();
    let joined = apply(source, std::slice::from_ref(&insert));
    let program = lang::parse(&joined)?;
    let canvas = canvas_of(&program)?;
    let mut moves = Vec::new();
    let mut paths = Vec::new();
    for (i, node) in canvas.children.iter().enumerate() {
        if node.span.start < start || node.span.end > end {
            continue;
        }
        paths.push(vec![0, i]);
        let mover = Mover {
            source: &joined,
            canvas,
            target: node,
            by: [dx, dy],
            moved: &moved,
            paste: true,
        };
        moves.extend(mover.edits()?.unwrap_or_default());
    }
    for e in &mut moves {
        e.span.start -= start;
        e.span.end -= start;
    }
    let text = apply(&joined[start..end], &moves);
    Ok((
        vec![TextEdit {
            span: insert.span,
            text,
        }],
        paths,
    ))
}

/// `text`, whose nodes are `pasted`, with each name that clashes with
/// `program` renamed as [`paste_edits`] says, and every name in `text`
/// as it is after, with whether a `repeat` copies it.
fn renamed(program: &Program, pasted: &Program, text: &str) -> (String, Vec<(String, bool)>) {
    let target = Taken::of(program);
    let mut taken = Taken {
        words: &Taken::of(pasted).words | &target.words,
        repeated: target.repeated.clone(),
    };
    let mut names = Vec::new();
    for node in &pasted.nodes {
        defined(node, false, &mut names);
    }
    let mut renames: Vec<(&str, String, bool)> = Vec::new();
    for &(word, repeated) in &names {
        let clashes =
            target.takes(word) || repeated && target.words.iter().any(|w| digits_after(w, word));
        if clashes && !renames.iter().any(|(old, ..)| *old == word) {
            let new = taken.fresh(
                word.trim_end_matches(|c: char| c.is_ascii_digit()),
                repeated,
            );
            taken.words.insert(new.clone());
            renames.push((word, new, repeated));
        }
    }
    let mut edits = Vec::new();
    for node in &pasted.nodes {
        rename(node, &renames, &mut edits);
    }
    let names = names
        .iter()
        .map(
            |&(word, repeated)| match renames.iter().find(|(old, ..)| *old == word) {
                Some((_, new, _)) => (new.clone(), repeated),
                None => (word.to_string(), repeated),
            },
        )
        .collect();
    (apply(text, &edits), names)
}

/// The canvas of `program`.
fn canvas_of(program: &Program) -> Result<&Node, Error> {
    match program.nodes.first() {
        Some(n) if n.kind.text == "canvas" => Ok(n),
        _ => Err(Error::new(Span::default(), "the file has no `canvas`")),
    }
}

/// `text` parsed, when it is one or more nodes that may sit on a
/// canvas. An error's span is in `text`.
fn nodes_in(text: &str) -> Result<Program, Error> {
    let program = lang::parse(text)
        .map_err(|e| Error::new(e.span, format!("the text is not acre nodes: {}", e.message)))?;
    if program.nodes.is_empty() {
        return Err(Error::new(Span::default(), "the text holds no nodes"));
    }
    for node in &program.nodes {
        let why = match node.kind.text.as_str() {
            "canvas" => "the text holds a whole canvas; copy the nodes inside it",
            "timeline" => "a canvas holds one timeline, so the text cannot add one",
            _ => continue,
        };
        return Err(Error::new(node.kind.span, why));
    }
    Ok(program)
}

/// The edit that adds `text` after the last property or child of
/// `canvas` before its timeline: on a line of its own at the indent of
/// the canvas's children when that one ends its line, and after a space
/// on the same line when not.
fn insertion(source: &str, canvas: &Node, text: &str) -> TextEdit {
    let before = canvas
        .children
        .iter()
        .find(|n| n.kind.text == "timeline")
        .map_or(canvas.span.end, |t| t.span.start);
    let anchor = canvas
        .properties
        .iter()
        .map(|p| p.span)
        .chain(canvas.children.iter().map(|n| n.span))
        .filter(|s| s.end <= before)
        .max_by_key(|s| s.end);
    let open = open_brace(source, canvas);
    let canvas_line = line_start(source, canvas.span.start);
    let canvas_indent = indent_at(source, canvas_line);
    let Some(anchor) = anchor else {
        let indent = format!("{canvas_indent}    ");
        return TextEdit {
            span: Span {
                start: open + 1,
                end: open + 1,
            },
            text: format!(" {}", indented(text, &indent)),
        };
    };
    let anchor_line = line_start(source, anchor.start);
    let indent = if anchor_line <= open {
        format!("{canvas_indent}    ")
    } else {
        indent_at(source, anchor_line).to_string()
    };
    let line_end = source[anchor.end..]
        .find('\n')
        .map_or(source.len(), |k| anchor.end + k);
    let rest = source[anchor.end..line_end].trim_start();
    let (at, text) = if rest.is_empty() || rest.starts_with("//") {
        (line_end, format!("\n{indent}{}", indented(text, &indent)))
    } else {
        (anchor.end, format!(" {}", indented(text, &indent)))
    };
    TextEdit {
        span: Span { start: at, end: at },
        text,
    }
}

/// `text` with `indent` before every line after the first that is not
/// blank.
fn indented(text: &str, indent: &str) -> String {
    let mut lines = text.split('\n');
    let mut out = lines.next().unwrap_or_default().to_string();
    for line in lines {
        out.push('\n');
        if !line.trim().is_empty() {
            out.push_str(indent);
            out.push_str(line);
        }
    }
    out
}

/// The offset of the start of the line that holds `offset`.
fn line_start(source: &str, offset: usize) -> usize {
    source[..offset].rfind('\n').map_or(0, |k| k + 1)
}

/// The spaces that open the line starting at `start`.
fn indent_at(source: &str, start: usize) -> &str {
    let line = &source[start..];
    &line[..line.len() - line.trim_start_matches(' ').len()]
}

/// An error unless the node is its own and not one copy of a `repeat`
/// around it; `why` says what acting on it would do.
fn in_repeat(target: &Node, around: &[&Node], why: &str) -> Result<(), Error> {
    if around.iter().any(|n| n.kind.text == "repeat") {
        return Err(Error::new(
            target.kind.span,
            format!("the node is drawn once for each number of a `repeat`; {why}"),
        ));
    }
    Ok(())
}

/// The nodes whose spans are `spans`, in source order, without those
/// inside another of them.
fn targets<'a>(
    program: &'a Program,
    spans: &[Span],
    repeat_why: &str,
) -> Result<Vec<&'a Node>, Error> {
    let mut targets = Vec::new();
    for &span in spans {
        if program.nodes.first().is_some_and(|c| c.span == span) {
            return Err(Error::new(
                span,
                "the canvas is the whole drawing; pick the nodes on it",
            ));
        }
        let (_, path) = locate(program, span)?;
        let (target, around) = path.split_last().expect("`locate` gives the node it finds");
        in_repeat(target, around, repeat_why)?;
        targets.push(*target);
    }
    targets.sort_by_key(|n| n.span.start);
    let mut outer: Vec<&Node> = Vec::new();
    for node in targets {
        if outer.last().is_none_or(|o| o.span.end <= node.span.start) {
            outer.push(node);
        }
    }
    Ok(outer)
}

/// Push onto `names` every name given in `node` and the nodes inside it,
/// each with whether a `repeat` copies it, which `in_repeat` says of
/// `node`.
pub(super) fn defined<'a>(node: &'a Node, in_repeat: bool, names: &mut Vec<(&'a str, bool)>) {
    if let Some(name) = &node.name {
        names.push((&name.text, in_repeat));
    }
    let in_repeat = in_repeat || node.kind.text == "repeat";
    for child in &node.children {
        defined(child, in_repeat, names);
    }
}

/// An error naming the first word in `node` and the nodes inside it,
/// those whose spans are `gone` aside, that names one of `names`.
/// `around` is the kind of the node `node` sits in.
fn refers(node: &Node, around: &str, names: &[(&str, bool)], gone: &[Span]) -> Result<(), Error> {
    if gone.contains(&node.span) {
        return Ok(());
    }
    let named = |w: &str| {
        names
            .iter()
            .find(|&&(name, repeated)| w == name || repeated && digits_after(w, name))
    };
    let who = match (node.kind.text.as_str(), around) {
        ("timeline", _) | (_, "timeline") => "the timeline".to_string(),
        (kind, _) if kind.starts_with(['a', 'e', 'i', 'o', 'u']) => format!("an `{kind}`"),
        (kind, _) => format!("a `{kind}`"),
    };
    let values = node
        .head
        .iter()
        .map(|v| (v, node.kind.text.as_str()))
        .chain(
            node.properties
                .iter()
                .flat_map(|p| p.values.iter().map(|v| (v, p.key.text.as_str()))),
        );
    for (value, key) in values {
        let mut found = None;
        each_word(value, &mut |v, w| {
            if found.is_none() && named(w).is_some() {
                found = Some((v.span, w.to_string()));
            }
        });
        if let Some((span, word)) = found {
            return Err(Error::new(
                span,
                format!("{who} names `{word}` in its `{key}`; change that first"),
            ));
        }
    }
    for child in &node.children {
        refers(child, &node.kind.text, names, gone)?;
    }
    Ok(())
}

/// Call `f` with every word of `value` and the value that holds it,
/// inside `( ... )` too.
fn each_word<'a>(value: &'a Value, f: &mut impl FnMut(&'a Value, &'a str)) {
    match &value.kind {
        ValueKind::Word(w) => f(value, w),
        ValueKind::Group(values) => {
            for v in values {
                each_word(v, f);
            }
        }
        _ => {}
    }
}

/// Whether `word` is `name` with a number after it, as a `repeat` names
/// its copies.
pub(super) fn digits_after(word: &str, name: &str) -> bool {
    word.strip_prefix(name)
        .is_some_and(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit()))
}

/// The span that deletes `span`: with its whole line, through the line
/// break, when the line holds nothing else but a comment after it, and
/// with one space next to it when not.
pub(super) fn removal(source: &str, span: Span) -> Span {
    let start = line_start(source, span.start);
    let end = source[span.end..]
        .find('\n')
        .map_or(source.len(), |k| span.end + k);
    let after = source[span.end..end].trim_start();
    if source[start..span.start].trim().is_empty() && (after.is_empty() || after.starts_with("//"))
    {
        return Span {
            start,
            end: (end + 1).min(source.len()),
        };
    }
    if source[span.end..].starts_with(' ') {
        Span {
            start: span.start,
            end: span.end + 1,
        }
    } else if source[..span.start].ends_with(' ') {
        Span {
            start: span.start - 1,
            end: span.end,
        }
    } else {
        span
    }
}

/// `span`, a deletion, with the blank line after it too when it takes
/// whole lines from between two blank ones.
fn without_double_blank(source: &str, span: Span) -> Span {
    if span.start != line_start(source, span.start) || !source[..span.end].ends_with('\n') {
        return span;
    }
    let above = source[..span.start]
        .strip_suffix('\n')
        .map(|s| &s[line_start(s, s.len())..]);
    let below_end = source[span.end..].find('\n').map(|k| span.end + k + 1);
    let below = below_end.map(|e| &source[span.end..e]);
    match (above, below, below_end) {
        (Some(a), Some(b), Some(e)) if a.trim().is_empty() && b.trim().is_empty() => Span {
            start: span.start,
            end: e,
        },
        _ => span,
    }
}

/// Push onto `edits` the renaming in `node` and the nodes inside it of
/// each name in `renames` to its new one, and of the words a `repeat`
/// makes of it when it is copied.
fn rename(node: &Node, renames: &[(&str, String, bool)], edits: &mut Vec<TextEdit>) {
    let new = |w: &str| {
        renames.iter().find_map(|(old, new, repeated)| {
            if w == *old {
                Some(new.clone())
            } else if *repeated && digits_after(w, old) {
                Some(format!("{new}{}", &w[old.len()..]))
            } else {
                None
            }
        })
    };
    if let Some(name) = &node.name
        && let Some(text) = new(&name.text)
    {
        edits.push(TextEdit {
            span: name.span,
            text,
        });
    }
    for value in node
        .head
        .iter()
        .chain(node.properties.iter().flat_map(|p| &p.values))
    {
        each_word(value, &mut |v, w| {
            if let Some(text) = new(w) {
                edits.push(TextEdit { span: v.span, text });
            }
        });
    }
    for child in &node.children {
        rename(child, renames, edits);
    }
}

/// The words a new name may not be.
struct Taken {
    /// Every name and every word in a value of a program.
    words: HashSet<String>,
    /// The names inside a `repeat`, which it copies with a number after.
    repeated: Vec<String>,
}

impl Taken {
    fn of(program: &Program) -> Taken {
        let mut taken = Taken {
            words: HashSet::new(),
            repeated: Vec::new(),
        };
        for node in &program.nodes {
            taken.add(node, false);
        }
        taken
    }

    fn add(&mut self, node: &Node, in_repeat: bool) {
        if let Some(name) = &node.name {
            self.words.insert(name.text.clone());
            if in_repeat {
                self.repeated.push(name.text.clone());
            }
        }
        for value in node
            .head
            .iter()
            .chain(node.properties.iter().flat_map(|p| &p.values))
        {
            each_word(value, &mut |_, w| {
                self.words.insert(w.to_string());
            });
        }
        let in_repeat = in_repeat || node.kind.text == "repeat";
        for child in &node.children {
            self.add(child, in_repeat);
        }
    }

    fn takes(&self, word: &str) -> bool {
        self.words.contains(word)
            || RESERVED.contains(&word)
            || self.repeated.iter().any(|name| digits_after(word, name))
    }

    /// `base` and the lowest number from 1 that make a word not taken,
    /// with `_` after `base` when a `repeat` makes every such word. A name
    /// `repeated` inside a `repeat` also starts no word with a number
    /// after it.
    fn fresh(&self, base: &str, repeated: bool) -> String {
        let mut base = base.to_string();
        while self
            .repeated
            .iter()
            .any(|name| base == *name || digits_after(&base, name))
        {
            base.push('_');
        }
        (1..)
            .map(|n| format!("{base}{n}"))
            .find(|name| {
                !self.takes(name) && !(repeated && self.words.iter().any(|w| digits_after(w, name)))
            })
            .expect("some number makes a word not taken")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{format, parse};
    use crate::scene::Scene;
    use crate::scene::edit::{move_edits, node_span};

    fn scene(src: &str) -> Scene {
        build(&parse(src).unwrap(), &[], &[]).unwrap()
    }

    /// `src` with `edits` made, after checking that the result keeps
    /// acre's layout and builds.
    fn edited(src: &str, edits: &[TextEdit]) -> String {
        let out = apply(src, edits);
        assert_eq!(format(&out).unwrap(), out, "from\n{src}");
        scene(&out);
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

    fn deleted(src: &str, nodes: &[&str]) -> String {
        let spans: Vec<Span> = nodes.iter().map(|n| span_of(src, n)).collect();
        edited(src, &delete_edits(src, &spans).unwrap())
    }

    fn delete_refusal(src: &str, node: &str) -> String {
        delete_edits(src, &[span_of(src, node)])
            .unwrap_err()
            .message
    }

    fn pasted(src: &str, text: &str, dx: f64, dy: f64) -> (String, Vec<Span>) {
        let (edits, paths) = paste_edits(src, text, dx, dy).unwrap();
        let out = edited(src, &edits);
        let program = parse(&out).unwrap();
        let spans = paths
            .iter()
            .map(|p| node_span(&program, p).unwrap())
            .collect();
        (out, spans)
    }

    const P: fn(f64, f64) -> Point = Point::new;

    fn every_new_node() -> Vec<NewNode> {
        vec![
            NewNode::Box {
                at: P(40.0, 60.0),
                size: P(120.0, 48.0),
            },
            NewNode::Ellipse {
                at: P(10.5, 20.0),
                size: P(30.0, 30.25),
            },
            NewNode::Text {
                at: P(100.0, 20.0),
                text: "say \"hi\"".into(),
            },
            NewNode::Line {
                from: EndAt::Point(P(0.0, 0.0)),
                to: EndAt::Name("a".into()),
            },
            NewNode::Arrow {
                from: EndAt::Name("a".into()),
                to: EndAt::Point(P(200.0, 150.0)),
            },
            NewNode::Polygon {
                points: vec![P(0.0, 0.0), P(50.0, 0.0), P(25.0, 40.0)],
            },
            NewNode::Polyline {
                points: vec![P(0.0, 0.0), P(50.0, 10.0)],
            },
        ]
    }

    #[test]
    fn a_new_node_is_written_on_one_line() {
        let texts: Vec<String> = every_new_node().iter().map(|n| n.text().unwrap()).collect();
        assert_eq!(
            texts,
            [
                "box { at 40 60; size 120 48; }",
                "ellipse { at 10.5 20; size 30 30.25; }",
                "text { at 100 20; text \"say \\\"hi\\\"\"; }",
                "line { from 0 0; to a; }",
                "arrow { from a; to 200 150; }",
                "polygon { points 0 0 50 0 25 40; }",
                "polyline { points 0 0 50 10; }",
            ]
        );
    }

    #[test]
    fn a_new_node_goes_last_before_the_timeline_in_every_layout() {
        let canvases = [
            "canvas { size 300 200; box a { at 10 10; size 20 20; } }\n",
            "canvas {\n    size 300 200;\n    box a { at 10 10; size 20 20; }\n}\n",
            "canvas {\n    size 300 200;\n\n    box a { at 10 10; size 20 20; } // the a\n    // trailing\n}\n",
            "canvas {\n    size 300 200;\n    box a {\n        at 10 10;\n        size 20 20;\n    }\n\n    timeline {\n        0s to 1s { move a 10 0; }\n    }\n}\n",
            "canvas { size 300 200; box a { at 10 10; size 20 20; } timeline { 1s { hide a; } } }\n",
            "canvas { size 300 200;\n    box a { at 10 10; size 20 20; }\n}\n",
        ];
        for src in canvases {
            let before = scene(src);
            for node in every_new_node() {
                let text = node.text().unwrap();
                let out = edited(src, &insert_edits(src, &text).unwrap());
                let after = scene(&out);
                assert_eq!(after.objects.len(), before.objects.len() + 1, "{out}");
                let last = after.objects.last().unwrap().source;
                assert_eq!(&out[last.start..last.end], text, "{out}");
                if let Some(t) = out.find("timeline") {
                    assert!(last.end < t, "{out}");
                }
            }
        }
    }

    #[test]
    fn a_canvas_of_properties_only_takes_a_node_after_them() {
        for src in [
            "canvas { size 300 200; }\n",
            "canvas {\n    size 300 200;\n}\n",
            "canvas { size 300 200;\n}\n",
        ] {
            let out = edited(src, &insert_edits(src, "box { size 10 10; }").unwrap());
            assert_eq!(scene(&out).objects.len(), 1, "{out}");
        }
    }

    #[test]
    fn several_lines_of_nodes_take_the_indent_of_the_canvas_children() {
        let src = "canvas {\n    size 300 200;\n    box a { size 10 10; }\n}\n";
        let text = "group {\n    at 5 5;\n\n    box { size 4 4; }\n}\nbox { size 2 2; }";
        let out = edited(src, &insert_edits(src, text).unwrap());
        assert!(
            out.ends_with("    group {\n        at 5 5;\n\n        box { size 4 4; }\n    }\n    box { size 2 2; }\n}\n"),
            "{out}"
        );
    }

    #[test]
    fn text_that_is_not_nodes_for_a_canvas_is_refused() {
        let src = "canvas { size 300 200; }\n";
        for (text, why) in [
            ("", "no nodes"),
            ("size 3 4;", "not acre nodes"),
            ("box {", "not acre nodes"),
            ("canvas { size 1 1; }", "whole canvas"),
            ("timeline { 1s { hide a; } }", "one timeline"),
        ] {
            let message = insert_edits(src, text).unwrap_err().message;
            assert!(message.contains(why), "{text}: {message}");
            let message = paste_edits(src, text, 10.0, 10.0).unwrap_err().message;
            assert!(message.contains(why), "{text}: {message}");
        }
    }

    #[test]
    fn a_node_lands_where_asked_on_an_auto_canvas() {
        let src = "canvas {\n    size auto auto;\n    box { at 300 -40; size 200 200; }\n}\n";
        let before = scene(src);
        assert_ne!(before.origin, Point::ZERO);
        let click = P(60.0, 70.0);
        let at = P(click.x - before.origin.x, click.y - before.origin.y);
        let node = NewNode::Box {
            at,
            size: P(40.0, 30.0),
        };
        let out = edited(src, &insert_edits(src, &node.text().unwrap()).unwrap());
        let after = scene(&out);
        assert_eq!(after.origin, before.origin);
        let (min, max) = after.bounds(1, 0.0).unwrap();
        assert!(
            (min.x - click.x).abs() < 1e-9 && (min.y - click.y).abs() < 1e-9,
            "{min:?}"
        );
        assert!((max.x - min.x - 40.0).abs() < 1e-9, "{max:?}");
    }

    #[test]
    fn an_auto_canvas_without_auto_has_its_origin_at_the_corner() {
        assert_eq!(
            scene("canvas { size 100 100; box { size 5 5; } }").origin,
            Point::ZERO
        );
    }

    #[test]
    fn a_fresh_name_clashes_with_no_word_and_no_repeat_copy() {
        let src = "canvas {
    size 300 200;
    let box2 10;
    box box1 { size box2 10; }
    text { at 0 0; text \"box4\"; }
    repeat i 0 2 { box box3 { at (i * 20) 50; size 10 10; } }
    repeat i 0 1 { box b { at (i * 20) 90; size 10 10; } }
}
";
        assert_eq!(fresh_name(src, "box").unwrap(), "box4");
        assert_eq!(fresh_name(src, "ellipse").unwrap(), "ellipse1");
        assert_eq!(fresh_name(src, "b").unwrap(), "b_1");
        assert_eq!(fresh_name(src, "top").unwrap(), "top1");
        let src = "canvas { size 300 200; box in { size 5 5; } }";
        assert!(fresh_name(src, "i").unwrap() == "i1");
    }

    #[test]
    fn naming_a_node_puts_the_name_after_its_kind() {
        let src = "canvas {\n    size 300 200;\n    box { size 50 30; }\n}\n";
        let span = span_of(src, "box");
        let mut edits = name_edits(src, span, "box1").unwrap();
        let arrow = NewNode::Arrow {
            from: EndAt::Point(P(200.0, 100.0)),
            to: EndAt::Name("box1".into()),
        };
        edits.extend(insert_edits(src, &arrow.text().unwrap()).unwrap());
        let out = edited(src, &edits);
        assert!(
            out.contains("box box1 { size 50 30; }\n    arrow { from 200 100; to box1; }"),
            "{out}"
        );
    }

    #[test]
    fn a_node_with_a_name_or_head_or_in_a_repeat_is_not_named() {
        let src = "canvas { size 300 200; box a { size 5 5; } repeat i 0 1 { box { size 5 5; } } }";
        let refuse = |node: &str| {
            name_edits(src, span_of(src, node), "x")
                .unwrap_err()
                .message
        };
        assert!(refuse("box a").contains("named `a` already"));
        assert!(refuse("repeat").contains("values before"));
        assert!(refuse("box { size 5 5; } }").contains("repeat"));
    }

    #[test]
    fn a_node_alone_on_its_lines_goes_with_them() {
        let src = "canvas {
    size 300 200;
    box a { size 5 5; }
    // a group
    group {
        at 5 5;
        box { size 1 1; } // inner
    } // after
    box c { size 5 5; }
}
";
        let out = deleted(src, &["group {"]);
        assert_eq!(
            out,
            "canvas {\n    size 300 200;\n    box a { size 5 5; }\n    // a group\n    box c { size 5 5; }\n}\n"
        );
        let out = deleted(src, &["box { size 1 1; }"]);
        assert!(out.contains("at 5 5;\n    } // after"), "{out}");
    }

    #[test]
    fn a_node_sharing_its_line_goes_with_one_space() {
        let src = "canvas { size 300 200; box a { size 5 5; } box b { size 5 5; } box c { size 5 5; } }\n";
        assert_eq!(
            deleted(src, &["box b"]),
            "canvas { size 300 200; box a { size 5 5; } box c { size 5 5; } }\n"
        );
        assert_eq!(
            deleted(src, &["box c"]),
            "canvas { size 300 200; box a { size 5 5; } box b { size 5 5; } }\n"
        );
        assert_eq!(
            deleted(src, &["box b", "box c", "box a"]),
            "canvas { size 300 200; }\n"
        );
        let src = "canvas {\n    size 300 200;\n    box a { size 5 5; } box b { size 5 5; }\n}\n";
        assert_eq!(
            deleted(src, &["box b"]),
            "canvas {\n    size 300 200;\n    box a { size 5 5; }\n}\n"
        );
        assert_eq!(
            deleted(src, &["box a", "box b"]),
            "canvas {\n    size 300 200;\n}\n"
        );
    }

    #[test]
    fn deleting_between_blank_lines_leaves_one() {
        let src = "canvas {
    size 300 200;

    box a { size 5 5; }
    box b { size 5 5; }

    box c { size 5 5; }
}
";
        assert_eq!(
            deleted(src, &["box a", "box b"]),
            "canvas {\n    size 300 200;\n\n    box c { size 5 5; }\n}\n"
        );
        assert_eq!(
            deleted(src, &["box c"]),
            "canvas {\n    size 300 200;\n\n    box a { size 5 5; }\n    box b { size 5 5; }\n\n}\n"
        );
    }

    #[test]
    fn several_nodes_go_at_once_and_one_inside_another_with_it() {
        let src = "canvas {
    size 300 200;
    box a { size 5 5; }
    arrow { from a; to 100 100; }
    group g {
        box b { size 5 5; }
        line { from b; to 0 0; }
    }
    box c { size 5 5; }
}
";
        let out = deleted(src, &["arrow", "box a", "box b", "group g"]);
        assert_eq!(
            out,
            "canvas {\n    size 300 200;\n    box c { size 5 5; }\n}\n"
        );
    }

    #[test]
    fn a_node_something_names_is_not_deleted() {
        let base = "canvas {
    size 300 200;
    box a { at 100 50; size 50 30; }
    row r { at 0 100; unit 20 20; cells \"x\" \"y\"; }
    group g { box gb { at 200 0; size 10 10; } }
    REF
}
";
        let cases = [
            (
                "arrow { from 0 0; to a; }",
                "box a",
                "an `arrow` names `a` in its `to`",
            ),
            ("line { from a left; to 0 0; }", "box a", "in its `from`"),
            ("box { at a 10 0; size 5 5; }", "box a", "in its `at`"),
            (
                "box { at r 1; size 5 5; }",
                "row r",
                "names `r` in its `at`",
            ),
            ("brace { over a; label \"x\"; }", "box a", "in its `over`"),
            (
                "merge { from a; from 0 0; to 100 100; }",
                "box a",
                "in its `from`",
            ),
            ("marker { at a; number 1; }", "box a", "in its `at`"),
            ("arrow { from 0 0; to gb; }", "group g", "names `gb`"),
            (
                "timeline { 0s to 1s { move g 10 0; } }",
                "group g",
                "the timeline names `g` in its `move`",
            ),
            ("let far a;", "box a", "a `canvas` names `a` in its `let`"),
        ];
        for (reference, node, why) in cases {
            let src = base.replace("REF", reference);
            scene(&src);
            let message = delete_refusal(&src, node);
            assert!(message.contains(why), "{reference}: {message}");
        }
    }

    #[test]
    fn a_node_named_by_another_that_goes_too_is_deleted() {
        let src = "canvas {\n    size 300 200;\n    box a { size 5 5; }\n    arrow { from 0 0; to a; }\n}\n";
        assert_eq!(
            deleted(src, &["box a", "arrow"]),
            "canvas {\n    size 300 200;\n}\n"
        );
    }

    #[test]
    fn a_repeat_copy_names_its_copies() {
        let src = "canvas {
    size 300 200;
    repeat i 0 1 { box b { at (i * 20) 0; size 10 10; } }
    arrow { from 0 50; to b1; }
}
";
        assert!(delete_refusal(src, "repeat").contains("names `b1`"));
        assert!(delete_refusal(src, "box b").contains("each number of a `repeat`"));
    }

    #[test]
    fn the_canvas_is_refused() {
        let src = "canvas {\n    size 300 200;\n    box a { size 5 5; }\n    caption { text \"x\"; }\n}\n";
        assert!(delete_refusal(src, "canvas").contains("whole drawing"));
        assert!(copy_text(src, &[span_of(src, "canvas")]).is_err());
    }

    #[test]
    fn copied_nodes_lose_the_indent_of_their_first_line() {
        let src = "canvas {
    size 300 200;
    box a { size 5 5; }
    group {
        at 5 5;
        box b { size 1 1; }
    }
}
";
        let text = copy_text(
            src,
            &[
                span_of(src, "group"),
                span_of(src, "box a"),
                span_of(src, "box b"),
            ],
        )
        .unwrap();
        assert_eq!(
            text,
            "box a { size 5 5; }\ngroup {\n    at 5 5;\n    box b { size 1 1; }\n}"
        );
    }

    #[test]
    fn a_paste_moves_by_the_offset_and_renames_what_clashes() {
        let src = "canvas {
    size 300 200;
    box a { at 10 10; size 40 30; }
    box b { at 100 10; size 40 30; }
    arrow { from a; to b; }
    box c { at a 0 50; size 40 30; }
    text { at 10 150; text \"a\"; }
    timeline {
        0s to 1s { move a 10 0; }
    }
}
";
        let spans: Vec<Span> = ["box a", "box b", "arrow", "box c", "text"]
            .iter()
            .map(|n| span_of(src, n))
            .collect();
        let text = copy_text(src, &spans).unwrap();
        let (out, pasted) = pasted(src, &text, 10.0, 10.0);
        assert!(
            out.contains(
                "    box a1 { at 20 20; size 40 30; }
    box b1 { at 110 20; size 40 30; }
    arrow { from a1; to b1; }
    box c1 { at a1 0 50; size 40 30; }
    text { at 20 160; text \"a\"; }
    timeline {"
            ),
            "{out}"
        );
        let before = scene(src);
        let after = scene(&out);
        assert_eq!(pasted.len(), 5);
        for (k, span) in spans.iter().enumerate() {
            let old = before
                .objects
                .iter()
                .position(|o| o.source == *span)
                .unwrap();
            let new = after
                .objects
                .iter()
                .position(|o| o.source == pasted[k])
                .unwrap();
            let (min0, _) = before.bounds(old, 0.0).unwrap();
            let (min1, _) = after.bounds(new, 0.0).unwrap();
            assert!(
                (min1.x - min0.x - 10.0).abs() < 1e-9,
                "{k}: {min0:?} {min1:?}"
            );
            assert!(
                (min1.y - min0.y - 10.0).abs() < 1e-9,
                "{k}: {min0:?} {min1:?}"
            );
        }
    }

    #[test]
    fn a_paste_keeps_names_that_do_not_clash_and_ends_on_things_outside_it() {
        let src = "canvas {\n    size 300 200;\n    box a { at 10 10; size 40 30; }\n}\n";
        let text = "box q { at 100 100; size 10 10; }\narrow { from a; to q; }\nbox { at a 5 5; size 3 3; }";
        let (out, _) = pasted(src, text, -5.0, 2.5);
        assert!(
            out.ends_with(
                "    box q { at 95 102.5; size 10 10; }\n    arrow { from a; to q; }\n    box { at a 0 7.5; size 3 3; }\n}\n"
            ),
            "{out}"
        );
    }

    #[test]
    fn a_paste_renames_a_repeat_and_the_words_naming_its_copies() {
        let src = "canvas {\n    size 300 200;\n    box b1 { size 5 5; }\n}\n";
        let text = "group {\n    repeat i 0 1 { box b { at (i * 20) 0; size 10 10; } }\n    arrow { from b0; to b1; }\n}";
        let (out, _) = pasted(src, text, 10.0, 10.0);
        assert!(out.contains("box b2 { at (i * 20) 0;"), "{out}");
        assert!(out.contains("arrow { from b20; to b21; }"), "{out}");
        assert!(out.contains("group {\n        at 10 10;"), "{out}");
    }

    #[test]
    fn a_paste_that_cannot_move_is_refused() {
        let src = "canvas {\n    size 300 200;\n    let x 10;\n    style s { at 5 5; }\n}\n";
        assert!(paste_edits(src, "box { at x 0; size 5 5; }", 10.0, 10.0).is_err());
        assert!(paste_edits(src, "box { style s; size 5 5; }", 10.0, 10.0).is_err());
    }

    #[test]
    fn a_paste_is_one_edit_whose_node_moves_as_a_move_would() {
        let src = "canvas {\n    size 300 200;\n    polygon p { points 0 0 10 0 5 8; }\n}\n";
        let text = copy_text(src, &[span_of(src, "polygon")]).unwrap();
        let (edits, _) = paste_edits(src, &text, 3.0, 4.0).unwrap();
        assert_eq!(edits.len(), 1);
        let moved = apply(
            src,
            &move_edits(src, &[span_of(src, "polygon")], 3.0, 4.0).unwrap(),
        );
        let out = edited(src, &edits);
        assert!(
            out.contains("polygon p1 { points 3 4 13 4 8 12; }"),
            "{out}"
        );
        assert!(moved.contains("points 3 4 13 4 8 12;"), "{moved}");
    }
}

//! The source edits that move a drawn node by an offset.
//!
//! A viewer that drags an object asks [`move_edits`] for the changes to
//! the numbers that place the object's node, and [`apply`] makes them.
//! [`node_path`] and [`node_span`] find a node again after an edit
//! shifts its span.

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

/// The edits to `source` that move the node whose span is `node`, as an
/// object's [`source`](super::Object::source) gives it, `dx` pixels
/// across and `dy` down, in source order. A number keeps as many decimals
/// as it was written with, and a new number is whole.
///
/// The edits change the node's `at`, adding `DX DY` to an `at` that names
/// a thing and an `at X Y` to a node without one, or the numbers of its
/// `from`, `to` and `points`. A group's `at` moves what it holds one to
/// one, so `dx` and `dy` are the same in the node's own coordinates. On
/// a canvas whose `size` has an `auto`, the drawing moves to keep its
/// margin, so the node moves by `dx` and `dy` against the rest.
///
/// # Errors
///
/// If `source` does not parse, no node has the span `node`, or the node
/// is not placed by numbers it sets itself: the error says why.
pub fn move_edits(source: &str, node: Span, dx: f64, dy: f64) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let canvas = program
        .nodes
        .first()
        .ok_or_else(|| Error::new(node, "the file has no `canvas`"))?;
    let mut path = Vec::new();
    if !find(&canvas.children, node, &mut path) {
        return Err(Error::new(node, "no node in the file has this span"));
    }
    let (target, around) = path.split_last().expect("`find` pushes the node it finds");
    let mover = Mover {
        source,
        canvas,
        target,
        by: [dx, dy],
    };
    mover.check_place(around)?;
    let mut edits = match target.kind.text.as_str() {
        "box" | "ellipse" | "row" | "matrix" | "text" | "math" | "table" | "ruler" | "code"
        | "group" | "panel" | "stack" | "grid" => mover.at()?,
        "marker" => mover.marker()?,
        "line" | "arrow" | "sweep" | "brace" => {
            if let Some(p) = own(target, "over") {
                return Err(Error::new(
                    p.span,
                    "the brace is set by the things it brackets; move them instead",
                ));
            }
            let mut edits = mover.end("from")?;
            edits.extend(mover.end("to")?);
            edits
        }
        "merge" => mover.merge()?,
        "polygon" | "polyline" => mover.points()?,
        "caption" => {
            return Err(Error::new(
                target.kind.span,
                "a caption sits under what comes before it and has no place of its own",
            ));
        }
        other => {
            return Err(Error::new(
                target.kind.span,
                format!("a `{other}` has no place to move"),
            ));
        }
    };
    edits.retain(|e| source[e.span.start..e.span.end] != e.text);
    edits.sort_by_key(|e| e.span.start);
    Ok(edits)
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

/// What one call to [`move_edits`] reads.
struct Mover<'a> {
    source: &'a str,
    canvas: &'a Node,
    target: &'a Node,
    by: [f64; 2],
}

impl Mover<'_> {
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
            if let Some(action) = self.timeline_action(&name.text) {
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

    /// The first `move` or `scale` in the timeline that names `name`.
    fn timeline_action(&self, name: &str) -> Option<&Property> {
        self.canvas
            .children
            .iter()
            .filter(|n| n.kind.text == "timeline")
            .flat_map(|t| &t.children)
            .flat_map(|entry| &entry.properties)
            .find(|p| {
                matches!(p.key.text.as_str(), "move" | "scale")
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
                        "`{key}` comes from style `{style}`, which other nodes may share; set it on the node to move it"
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
                    text: number(n + by, decimals),
                })
            })
            .collect()
    }

    /// The offset as two whole numbers, or `None` when both are 0.
    fn offsets(&self) -> Option<String> {
        let [x, y] = self.by.map(|n| number(n, 0));
        (x != "0" || y != "0").then(|| format!("{x} {y}"))
    }

    /// The insertion of `at DX DY` before the node's first property or
    /// child, on its line when it shares the brace's line and on a line
    /// of its own at the same indent when not.
    fn new_at(&self) -> Option<TextEdit> {
        let offsets = self.offsets()?;
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
                    Some(k) => format!("at {offsets};\n{}", &between[k + 1..]),
                    None => format!("at {offsets}; "),
                };
                (start, text)
            }
            None => (open + 1, format!(" at {offsets}; ")),
        };
        Some(TextEdit {
            span: Span { start: at, end: at },
            text,
        })
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
    /// `dx` and `dy`, after checking that the result keeps that layout
    /// and that the box of every object the node draws moved by exactly
    /// `dx` and `dy`.
    fn moved(src: &str, index: usize, dx: f64, dy: f64) -> String {
        let src = format(src).unwrap();
        let before = scene(&src);
        let span = before.objects[index].source;
        let edits = move_edits(&src, span, dx, dy).unwrap();
        let out = apply(&src, &edits);
        assert_eq!(format(&out).unwrap(), out);
        let after = scene(&out);
        assert_eq!(before.objects.len(), after.objects.len());
        for (k, o) in before.objects.iter().enumerate() {
            if o.source.start < span.start || o.source.end > span.end {
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
        move_edits(src, span_of(src, node), 5.0, 3.0)
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
    fn a_number_is_written_whole_unless_it_had_decimals() {
        let src = "canvas { size 300 200; box { at 40 60.5; size 50 30; } }\n";
        let span = scene(src).objects[0].source;
        let out = apply(src, &move_edits(src, span, 3.6, 3.26).unwrap());
        assert!(out.contains("at 44 63.8;"), "{out}");

        let src = "canvas { size 300 200; box { at 3 0.25; size 50 30; } }\n";
        let span = scene(src).objects[0].source;
        let out = apply(src, &move_edits(src, span, -3.2, -0.25).unwrap());
        assert!(out.contains("at 0 0.00;"), "{out}");
    }

    #[test]
    fn a_move_that_rounds_to_nothing_makes_no_edits() {
        let src = "canvas { size 300 200; box { size 50 30; } box { at 4 4; size 5 5; } }\n";
        let s = scene(src);
        for o in &s.objects {
            assert_eq!(move_edits(src, o.source, 0.2, -0.4).unwrap(), Vec::new());
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
            &move_edits(src, span_of(src, "group g"), 4.0, 6.0).unwrap(),
        );
        assert!(
            out.contains("group g {\n        at 4 6;\n        box"),
            "{out}"
        );
        let out = apply(
            &out,
            &move_edits(&out, span_of(&out, "group h"), 4.0, 6.0).unwrap(),
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
        let out = apply(src, &move_edits(src, span, 4.0, 6.0).unwrap());
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
            &move_edits(src, before.objects[0].source, -10.0, 4.0).unwrap(),
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
        assert_eq!(move_edits(src, span, 1.0, 0.0).unwrap().len(), 1);
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
    fn a_caption_or_a_span_that_is_no_node_is_refused() {
        let src = "canvas { size 300 200; panel { title \"p\"; caption { text \"c\"; } } }";
        assert!(refusal(src, "caption").contains("a caption sits under"));
        let err = move_edits(src, Span { start: 3, end: 9 }, 1.0, 1.0).unwrap_err();
        assert!(err.message.contains("no node"), "{}", err.message);
        assert!(move_edits("canvas {", Span::default(), 1.0, 1.0).is_err());
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
        let edits = move_edits(&src, span_of(&src, "box a"), 5.0, 5.0).unwrap();
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
}

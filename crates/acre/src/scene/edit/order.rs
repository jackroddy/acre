//! The edits that move a child of a stack or a grid to another place
//! among its siblings.

use super::nodes::{in_repeat, indent_at, line_start, removal, without_double_blank};
use super::{TextEdit, apply, locate};
use crate::lang::{self, Error, Node, Program, Span};
use crate::scene::build::{self, Props, grid_columns, keys, stack_across};
use crate::scene::expand::expand;

/// How a stack or a grid places its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// A stack whose children run down.
    Down,
    /// A stack whose children run right.
    Right,
    /// A grid of `columns` cells across, filled row by row.
    Grid { columns: usize },
}

/// The children of the stack or grid that holds a node.
#[derive(Debug, Clone, PartialEq)]
pub struct Siblings {
    pub layout: Layout,
    /// The children's spans in source order, the node's among them.
    pub children: Vec<Span>,
}

/// The stack or grid that holds the node whose span is `node`: how it
/// lays out its children, and their spans.
///
/// # Errors
///
/// If `source` does not parse, or the node is not a child of a stack or
/// a grid, or is drawn once for each number of a `repeat`. Also if the
/// stack's `direction` or the grid's `columns` is not one acre reads.
pub fn siblings(source: &str, node: Span) -> Result<Siblings, Error> {
    let program = lang::parse(source)?;
    let (canvas, parent) = parent_of(&program, node)?;

    // the `direction` or `columns` may come from a style or a `let`
    let (canvas, _, _) = expand(canvas)?;
    let expanded = find(&canvas, parent.span).unwrap_or(parent);
    let kind = parent.kind.text.as_str();
    let props = Props::new(expanded, &keys(kind), true)?;
    let layout = match kind {
        "stack" if stack_across(&props)? => Layout::Right,
        "stack" => Layout::Down,
        _ => Layout::Grid {
            columns: grid_columns(&props)?,
        },
    };
    Ok(Siblings {
        layout,
        children: parent.children.iter().map(|c| c.span).collect(),
    })
}

/// The edits to `source` that move the node whose span is `node`, a
/// child of a stack or a grid, to just before its sibling whose span is
/// `before`, or after the last of them when `before` is `None`. The
/// stack lays it out in its new place.
///
/// The node goes as one piece of text with its properties and the
/// nodes inside it. When the node is alone on its lines, it goes with
/// them, with the comment on its line, and with the comment lines
/// directly above it, and lands on lines of its own at its siblings'
/// indent. A node that shares its line goes on its own, and lands on
/// the line of the sibling it goes before.
///
/// A move to where the node already is makes no edits.
///
/// # Errors
///
/// If `source` does not parse, the node is not a child of a stack or a
/// grid or is drawn once for each number of a `repeat`, or `before` is
/// not another child of the same parent. Also if the file builds and
/// would no longer build: the error is the build's.
pub fn reorder_edits(
    source: &str,
    node: Span,
    before: Option<Span>,
) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let (_, parent) = parent_of(&program, node)?;
    let children = &parent.children;
    let from = children
        .iter()
        .position(|c| c.span == node)
        .expect("`parent_of` gives the node's parent");
    let to = match before {
        None => children.len(),
        Some(span) => children
            .iter()
            .position(|c| c.span == span)
            .ok_or_else(|| {
                Error::new(
                    span,
                    format!(
                        "the node to go before is not a child of the same {}",
                        parent.kind.text
                    ),
                )
            })?,
    };
    if to == from || to == from + 1 {
        return Ok(Vec::new());
    }

    let carried = Carried::of(source, &children[from]);
    let indent = children_indent(source, parent);
    let insertion = match children.get(to) {
        Some(next) => carried.before(source, next, &indent),
        None => carried.after(source, &children[children.len() - 1], &indent),
    };
    let edits = vec![
        TextEdit {
            span: carried.span,
            text: String::new(),
        },
        insertion,
    ];

    if build::build(&program, &[], &[]).is_ok() {
        let out = apply(source, &edits);
        let rebuilt = lang::parse(&out).and_then(|p| build::build(&p, &[], &[]).map(|_| ()));
        if let Err(e) = rebuilt {
            return Err(Error::new(children[from].kind.span, e.message));
        }
    }
    Ok(edits)
}

/// The canvas of `program` and the stack or grid whose child has the
/// span `node`.
fn parent_of(program: &Program, node: Span) -> Result<(&Node, &Node), Error> {
    let (canvas, path) = locate(program, node)?;
    let (target, around) = path.split_last().expect("`locate` gives the node it finds");
    in_repeat(target, around, "reorder the `repeat` instead")?;
    match around.last() {
        Some(parent) if matches!(parent.kind.text.as_str(), "stack" | "grid") => {
            Ok((canvas, parent))
        }
        _ => Err(Error::new(
            target.kind.span,
            "the node is not a child of a stack or a grid, so it has no slot to change",
        )),
    }
}

/// The node in `node` or below whose span is `span`.
fn find(node: &Node, span: Span) -> Option<&Node> {
    if node.span == span {
        return Some(node);
    }
    node.children.iter().find_map(|c| find(c, span))
}

/// A node's text as a move takes it out of the source.
struct Carried {
    /// What the move deletes.
    span: Span,
    /// The lines that go, without the indent of the node's line.
    lines: Vec<String>,
    /// Whether the last line ends in a comment.
    commented: bool,
}

impl Carried {
    fn of(source: &str, node: &Node) -> Carried {
        let start = line_start(source, node.span.start);
        let end = line_end(source, node.span.end);
        let after = source[node.span.end..end].trim_start();
        let alone = source[start..node.span.start].trim().is_empty()
            && (after.is_empty() || after.starts_with("//"));
        let indent = indent_at(source, start).len();
        if !alone {
            return Carried {
                span: removal(source, node.span),
                lines: outdented(&source[node.span.start..node.span.end], indent),
                commented: false,
            };
        }
        let mut first = start;
        while let Some(above) = comment_above(source, first) {
            first = above;
        }
        let span = Span {
            start: first,
            end: (end + 1).min(source.len()),
        };
        Carried {
            span: without_opening_blank(source, without_double_blank(source, span)),
            lines: outdented(&source[first..end], indent),
            commented: !after.is_empty(),
        }
    }

    /// The insertion of the lines, at `indent`, before the sibling
    /// `next`: above it and the comment lines directly above it when it
    /// starts its line, and on its line when not.
    fn before(&self, source: &str, next: &Node, indent: &str) -> TextEdit {
        let line = line_start(source, next.span.start);
        if source[line..next.span.start].trim().is_empty() {
            let mut at = line;
            while let Some(above) = comment_above(source, at) {
                at = above;
            }
            return insert(at, format!("{}\n", self.indented(indent, true)));
        }
        let gap = if self.commented {
            format!("\n{indent}")
        } else {
            " ".to_string()
        };
        insert(
            next.span.start,
            format!("{}{gap}", self.indented(indent, false)),
        )
    }

    /// The insertion of the lines, at `indent`, after the sibling
    /// `last`: on lines of their own after its line when it ends its
    /// line, and on its line when not.
    fn after(&self, source: &str, last: &Node, indent: &str) -> TextEdit {
        let end = line_end(source, last.span.end);
        let rest = source[last.span.end..end].trim_start();
        if (rest.is_empty() || rest.starts_with("//")) && end < source.len() {
            return insert(end + 1, format!("{}\n", self.indented(indent, true)));
        }
        let tail = if self.commented {
            format!(
                "\n{}",
                indent_at(source, line_start(source, last.span.start))
            )
        } else {
            String::new()
        };
        insert(
            last.span.end,
            format!(" {}{tail}", self.indented(indent, false)),
        )
    }

    /// The lines joined, each but a blank one after `indent`, the first
    /// too when `first`.
    fn indented(&self, indent: &str, first: bool) -> String {
        let mut out = String::new();
        for (k, line) in self.lines.iter().enumerate() {
            if k > 0 {
                out.push('\n');
            }
            if !line.is_empty() && (k > 0 || first) {
                out.push_str(indent);
            }
            out.push_str(line);
        }
        out
    }
}

/// The indent of the children of `parent`: that of the first child to
/// start its line, or one more step than the parent's line.
fn children_indent(source: &str, parent: &Node) -> String {
    for child in &parent.children {
        let line = line_start(source, child.span.start);
        if source[line..child.span.start].trim().is_empty() {
            return indent_at(source, line).to_string();
        }
    }
    let line = line_start(source, parent.span.start);
    format!("{}    ", indent_at(source, line))
}

fn insert(at: usize, text: String) -> TextEdit {
    TextEdit {
        span: Span { start: at, end: at },
        text,
    }
}

/// The offset of the line break that ends the line holding `offset`,
/// or the end of `source`.
fn line_end(source: &str, offset: usize) -> usize {
    source[offset..]
        .find('\n')
        .map_or(source.len(), |k| offset + k)
}

/// The start of the line above the one that starts at `line`, when that
/// line holds a comment and nothing else.
fn comment_above(source: &str, line: usize) -> Option<usize> {
    let start = line_start(source, line.checked_sub(1)?);
    source[start..line]
        .trim_start()
        .starts_with("//")
        .then_some(start)
}

/// `span`, a deletion of whole lines, with the blank line after it too
/// when the line above it opens a body.
fn without_opening_blank(source: &str, span: Span) -> Span {
    let above = match span.start.checked_sub(1) {
        Some(k) => &source[line_start(source, k)..k],
        None => return span,
    };
    let below = line_end(source, span.end);
    if above.trim_end().ends_with('{')
        && below < source.len()
        && source[span.end..below].trim().is_empty()
    {
        return Span {
            start: span.start,
            end: below + 1,
        };
    }
    span
}

/// The lines of `text` with up to `indent` spaces taken off the start of
/// each, trailing spaces kept.
fn outdented(text: &str, indent: usize) -> Vec<String> {
    text.split('\n')
        .map(|line| {
            let spaces = line.len() - line.trim_start_matches(' ').len();
            line[spaces.min(indent)..].to_string()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{format, parse};

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

    /// `src` with the node that starts with `node` moved before the one
    /// that starts with `before`, or to the end, after checking that the
    /// result keeps acre's layout when `src` does, and builds.
    fn reordered(src: &str, node: &str, before: Option<&str>) -> String {
        let before = before.map(|b| span_of(src, b));
        let edits = reorder_edits(src, span_of(src, node), before).unwrap();
        assert_eq!(edits.len(), 2);
        let out = apply(src, &edits);
        if format(src).unwrap() == src {
            assert_eq!(format(&out).unwrap(), out, "from\n{src}");
        }
        build::build(&parse(&out).unwrap(), &[], &[]).unwrap();
        out
    }

    fn refusal(src: &str, node: &str, before: Option<&str>) -> String {
        let before = before.map(|b| span_of(src, b));
        reorder_edits(src, span_of(src, node), before)
            .unwrap_err()
            .message
    }

    const STACK: &str = "canvas {
    size 300 300;
    stack s {
        at 10 10;
        gap 4;
        box a { size 20 10; }
        box b {
            size 20 30;
        }
        box c { size 20 20; } // the last
    }
}
";

    #[test]
    fn a_child_moves_up_or_down_with_its_lines() {
        assert_eq!(
            reordered(STACK, "box c", Some("box a")),
            "canvas {
    size 300 300;
    stack s {
        at 10 10;
        gap 4;
        box c { size 20 20; } // the last
        box a { size 20 10; }
        box b {
            size 20 30;
        }
    }
}
"
        );
        assert_eq!(
            reordered(STACK, "box b", Some("box a")),
            "canvas {
    size 300 300;
    stack s {
        at 10 10;
        gap 4;
        box b {
            size 20 30;
        }
        box a { size 20 10; }
        box c { size 20 20; } // the last
    }
}
"
        );
        assert_eq!(
            reordered(STACK, "box a", Some("box c")),
            "canvas {
    size 300 300;
    stack s {
        at 10 10;
        gap 4;
        box b {
            size 20 30;
        }
        box a { size 20 10; }
        box c { size 20 20; } // the last
    }
}
"
        );
    }

    #[test]
    fn a_child_moves_to_the_end_after_the_comment_of_the_last() {
        assert_eq!(
            reordered(STACK, "box a", None),
            "canvas {
    size 300 300;
    stack s {
        at 10 10;
        gap 4;
        box b {
            size 20 30;
        }
        box c { size 20 20; } // the last
        box a { size 20 10; }
    }
}
"
        );
    }

    #[test]
    fn the_stack_lays_the_moved_child_out_in_its_new_slot() {
        let out = reordered(STACK, "box c", Some("box a"));
        let scene = build::build(&parse(&out).unwrap(), &[], &[]).unwrap();
        let top = |name: &str| {
            let span = span_of(&out, name);
            let i = scene.objects.iter().position(|o| o.source == span).unwrap();
            scene.bounds(i, 0.0).unwrap().0.y
        };
        assert_eq!(
            [top("box c"), top("box a"), top("box b")],
            [10.0, 34.0, 48.0]
        );
    }

    #[test]
    fn comment_lines_directly_above_go_with_the_child() {
        let src = "canvas {
    size 300 300;
    stack {
        // first
        // and a second line
        box a { size 20 10; }

        // b's
        box b { size 20 10; }
        box c { size 20 10; }
    }
}
";
        assert_eq!(
            reordered(src, "box a", None),
            "canvas {
    size 300 300;
    stack {
        // b's
        box b { size 20 10; }
        box c { size 20 10; }
        // first
        // and a second line
        box a { size 20 10; }
    }
}
"
        );
        assert_eq!(
            reordered(src, "box c", Some("box b")),
            "canvas {
    size 300 300;
    stack {
        // first
        // and a second line
        box a { size 20 10; }

        box c { size 20 10; }
        // b's
        box b { size 20 10; }
    }
}
"
        );
    }

    #[test]
    fn a_child_between_blank_lines_leaves_one() {
        let src = "canvas {
    size 300 300;
    stack {
        box a { size 20 10; }

        box b { size 20 10; }

        box c { size 20 10; }
    }
}
";
        assert_eq!(
            reordered(src, "box b", Some("box a")),
            "canvas {
    size 300 300;
    stack {
        box b { size 20 10; }
        box a { size 20 10; }

        box c { size 20 10; }
    }
}
"
        );
    }

    #[test]
    fn a_child_on_a_shared_line_moves_on_that_line() {
        let src = "canvas { size 300 300; stack { box a { size 1 1; } box b { size 2 2; } box c { size 3 3; } } }\n";
        assert_eq!(
            reordered(src, "box c", Some("box a")),
            "canvas { size 300 300; stack { box c { size 3 3; } box a { size 1 1; } box b { size 2 2; } } }\n"
        );
        assert_eq!(
            reordered(src, "box a", Some("box c")),
            "canvas { size 300 300; stack { box b { size 2 2; } box a { size 1 1; } box c { size 3 3; } } }\n"
        );
        assert_eq!(
            reordered(src, "box a", None),
            "canvas { size 300 300; stack { box b { size 2 2; } box c { size 3 3; } box a { size 1 1; } } }\n"
        );
    }

    #[test]
    fn a_child_moves_between_a_shared_line_and_lines_of_its_own() {
        let src = "canvas {
    size 300 300;
    stack { box a { size 1 1; }
        box b { size 2 2; } // b
    }
}
";
        let out = reordered(src, "box b", Some("box a"));
        assert_eq!(
            out,
            "canvas {
    size 300 300;
    stack { box b { size 2 2; } // b
        box a { size 1 1; }
    }
}
"
        );
        assert_eq!(format(&out).unwrap(), out);
    }

    #[test]
    fn a_grid_child_moves_in_reading_order() {
        let src = "canvas {
    size 300 300;
    grid {
        columns 2;
        box a { size 10 10; }
        box b { size 10 10; }
        box c { size 10 10; }
    }
}
";
        assert_eq!(
            reordered(src, "box c", Some("box b")),
            "canvas {
    size 300 300;
    grid {
        columns 2;
        box a { size 10 10; }
        box c { size 10 10; }
        box b { size 10 10; }
    }
}
"
        );
    }

    #[test]
    fn a_move_to_where_the_child_is_makes_no_edits() {
        let a = span_of(STACK, "box a");
        let b = span_of(STACK, "box b");
        let c = span_of(STACK, "box c");
        assert_eq!(reorder_edits(STACK, a, Some(a)).unwrap(), []);
        assert_eq!(reorder_edits(STACK, a, Some(b)).unwrap(), []);
        assert_eq!(reorder_edits(STACK, c, None).unwrap(), []);
    }

    #[test]
    fn a_node_outside_a_stack_or_in_a_repeat_or_a_stranger_is_refused() {
        let src = "canvas {
    size 300 300;
    box x { at 0 0; size 5 5; }
    stack {
        repeat i 0 2 { box r { size 4 4; } }
        box a { size 4 4; }
        group { box g { at 0 0; size 4 4; } }
    }
    stack { box o { size 4 4; } }
}
";
        assert!(refusal(src, "box x", None).contains("not a child of a stack or a grid"));
        assert!(refusal(src, "box g", None).contains("not a child of a stack or a grid"));
        assert!(refusal(src, "box r", None).contains("`repeat`"));
        assert_eq!(
            refusal(src, "box a", Some("box o")),
            "the node to go before is not a child of the same stack"
        );
        assert_eq!(
            refusal(src, "box a", Some("box g")),
            "the node to go before is not a child of the same stack"
        );
    }

    #[test]
    fn siblings_gives_the_layout_from_the_source_and_its_styles() {
        let src = "canvas {
    size 300 300;
    let n 3;
    style across { direction right; }
    stack { box a { size 1 1; } box b { size 1 1; } }
    stack { style across; box c { size 1 1; } }
    grid { columns (n); box d { size 1 1; } }
    grid { box e { size 1 1; } }
}
";
        let of = |node: &str| siblings(src, span_of(src, node)).unwrap();
        assert_eq!(
            of("box b"),
            Siblings {
                layout: Layout::Down,
                children: vec![span_of(src, "box a"), span_of(src, "box b")],
            }
        );
        assert_eq!(of("box c").layout, Layout::Right);
        assert_eq!(of("box d").layout, Layout::Grid { columns: 3 });
        assert_eq!(of("box e").layout, Layout::Grid { columns: 1 });
        assert!(siblings(src, span_of(src, "stack")).is_err());
    }
}

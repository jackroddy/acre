//! The source edits that set a property such as `fill` on nodes, and
//! the reading of where each node's value comes from.

use super::nodes::{EndAt, removal};
use super::{Mover, TextEdit, apply, locate, own};
use crate::lang::{self, Error, Node, Span, Value, ValueKind};
use crate::scene::{build, expand, schema};

/// Where a node's value of a property comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Origin {
    /// The node sets it.
    Node,
    /// The style of this name sets it.
    Style(String),
    /// Nothing sets it, so the kind's default applies.
    Default,
}

/// A node's value of one property, as [`property_values`] reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Setting<'k> {
    pub key: &'k str,
    /// The values, with a `let`'s name replaced by the values it names
    /// and a `( ... )` by what it comes to, or none when the default
    /// applies.
    pub values: Vec<ValueKind>,
    pub origin: Origin,
}

/// The value of each of `keys` that the kind of the node whose span is
/// `node` takes, in the order of `keys`, and where it comes from: the
/// node, the last style that sets it, or the default.
///
/// # Errors
///
/// If `source` does not parse, or no node has the span `node`.
pub fn property_values<'k>(
    source: &str,
    node: Span,
    keys: &[&'k str],
) -> Result<Vec<Setting<'k>>, Error> {
    let program = lang::parse(source)?;
    let (canvas, path) = locate(&program, node)?;
    let target = path[path.len() - 1];
    let Some(kind) = schema::node(&target.kind.text) else {
        return Ok(Vec::new());
    };
    let styles = styles_of(canvas, target);
    let settings = keys
        .iter()
        .filter(|key| kind.property(key).is_some())
        .map(|&key| {
            let set = own(target, key).map(|p| (p, Origin::Node)).or_else(|| {
                styles.iter().rev().find_map(|style| {
                    let p = own(style, key)?;
                    let name = style.name.as_ref().map_or("", |w| w.text.as_str());
                    Some((p, Origin::Style(name.to_string())))
                })
            });
            match set {
                Some((p, origin)) => Setting {
                    key,
                    values: resolved(canvas, &p.values),
                    origin,
                },
                None => Setting {
                    key,
                    values: Vec::new(),
                    origin: Origin::Default,
                },
            }
        })
        .collect();
    Ok(settings)
}

/// The style nodes on `canvas` that apply to `target`, the one named
/// after its kind first and then those its `style` names in order, so
/// that a later one goes over an earlier one.
fn styles_of<'a>(canvas: &'a Node, target: &Node) -> Vec<&'a Node> {
    let named = own(target, "style")
        .into_iter()
        .flat_map(|p| &p.values)
        .filter_map(|v| match &v.kind {
            ValueKind::Word(w) => Some(w.as_str()),
            _ => None,
        });
    [target.kind.text.as_str()]
        .into_iter()
        .chain(named)
        .filter_map(|name| {
            canvas
                .children
                .iter()
                .find(|n| n.kind.text == "style" && n.name.as_ref().is_some_and(|w| w.text == name))
        })
        .collect()
}

/// `values` with the names of the `let`s on `canvas` replaced by their
/// values and each `( ... )` by what it comes to, or `values` as they
/// are if those do not work out.
fn resolved(canvas: &Node, values: &[Value]) -> Vec<ValueKind> {
    let worked_out = expand::lets(canvas).and_then(|lets| expand::substitute(values, &lets));
    worked_out
        .as_deref()
        .unwrap_or(values)
        .iter()
        .map(|v| v.kind.clone())
        .collect()
}

/// The edits to `source` that set `key` to `value`, written as it is
/// to go in the source, on each node whose span is in `nodes`. A node's
/// own value of `key` is replaced, and a node without one gets one
/// before its first property, which goes over a style's. An empty
/// `value` takes away the node's own value, so a style's or the default
/// applies.
///
/// # Errors
///
/// If `source` does not parse, no node has a span in `nodes`, `value` is
/// not values a property holds, or a node is not its own to change: its
/// kind takes no `key`, it is drawn once for each number of a `repeat`,
/// or its value comes from a `let` or `( ... )`. Also if the file builds
/// and would no longer build: the error is the build's. The error is the
/// first such node's.
pub fn property_edits(
    source: &str,
    nodes: &[Span],
    key: &str,
    value: &str,
) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let mut nodes = nodes.to_vec();
    nodes.sort_by_key(|s| s.start);
    nodes.dedup();
    let mut edits = Vec::new();
    for &span in &nodes {
        let (canvas, path) = locate(&program, span)?;
        let (target, around) = path.split_last().expect("`locate` gives the node it finds");
        check_values(target, key, value)?;
        let kind = target.kind.text.as_str();
        if schema::node(kind).and_then(|k| k.property(key)).is_none() {
            return Err(Error::new(
                target.kind.span,
                format!("a `{kind}` has no property `{key}`"),
            ));
        }
        if around.iter().any(|n| n.kind.text == "repeat") {
            return Err(Error::new(
                target.kind.span,
                format!(
                    "the node is drawn once for each number of a `repeat`, so its `{key}` is every copy's"
                ),
            ));
        }
        let mover = Mover {
            source,
            canvas,
            target,
            by: [0.0; 2],
            moved: &[],
            paste: false,
        };
        match own(target, key) {
            Some(p) => {
                mover.literal(p)?;
                if value.is_empty() {
                    edits.push(TextEdit {
                        span: removal(source, p.span),
                        text: String::new(),
                    });
                    continue;
                }
                let span = match (p.values.first(), p.values.last()) {
                    (Some(first), Some(last)) => Span {
                        start: first.span.start,
                        end: last.span.end,
                    },
                    _ => Span {
                        start: p.key.span.end,
                        end: p.key.span.end,
                    },
                };
                let text = if p.values.is_empty() {
                    format!(" {value}")
                } else {
                    value.to_string()
                };
                edits.push(TextEdit { span, text });
            }
            None if value.is_empty() => {}
            None => edits.push(mover.insert(&format!("{key} {value}"))),
        }
    }
    edits.retain(|e| source[e.span.start..e.span.end] != e.text);

    if !edits.is_empty() && build(&program, &[], &[]).is_ok() {
        let out = apply(source, &edits);
        let rebuilt = lang::parse(&out).and_then(|p| build(&p, &[], &[]).map(|_| ()));
        if let Err(e) = rebuilt {
            let first = locate(&program, nodes[0])?.1;
            return Err(Error::new(first[first.len() - 1].kind.span, e.message));
        }
    }
    Ok(edits)
}

/// The edits to `source` that put end `key`, `from` or `to`, of the line
/// or arrow whose span is `node` at `end`. The edits do not check that
/// the file still builds, since a name `end` gives may come from other
/// edits made with them, such as [`name_edits`](super::name_edits).
///
/// # Errors
///
/// If `source` does not parse, no node has the span `node`, it is no
/// line or arrow, or the end is not its own to change: it is drawn once
/// for each number of a `repeat`, or the end comes from a style, a `let`
/// or `( ... )`.
pub fn end_edits(source: &str, node: Span, key: &str, end: &EndAt) -> Result<Vec<TextEdit>, Error> {
    let program = lang::parse(source)?;
    let (canvas, path) = locate(&program, node)?;
    let (target, around) = path.split_last().expect("`locate` gives the node it finds");
    let kind = target.kind.text.as_str();
    if !matches!(kind, "line" | "arrow") || !matches!(key, "from" | "to") {
        return Err(Error::new(
            target.kind.span,
            format!("a `{kind}` has no end `{key}` to move"),
        ));
    }
    if around.iter().any(|n| n.kind.text == "repeat") {
        return Err(Error::new(
            target.kind.span,
            "the node is drawn once for each number of a `repeat`, so its ends are every copy's",
        ));
    }
    let Some(p) = own(target, key) else {
        return Err(Error::new(
            target.kind.span,
            format!("the `{key}` comes from a style, which other nodes may share; edit it by hand"),
        ));
    };
    let mover = Mover {
        source,
        canvas,
        target,
        by: [0.0; 2],
        moved: &[],
        paste: false,
    };
    mover.literal(p)?;
    let span = match (p.values.first(), p.values.last()) {
        (Some(first), Some(last)) => Span {
            start: first.span.start,
            end: last.span.end,
        },
        _ => return Err(p.expected("an end")),
    };
    let text = end.text();
    if source[span.start..span.end] == text {
        return Ok(Vec::new());
    }
    Ok(vec![TextEdit { span, text }])
}

/// An error unless `value` is empty or reads as the values of one
/// property, and nothing more.
fn check_values(target: &Node, key: &str, value: &str) -> Result<(), Error> {
    if value.is_empty() {
        return Ok(());
    }
    let probe = format!("probe {{ {key} {value}; }}");
    let holds_values = lang::parse(&probe).is_ok_and(|p| {
        matches!(p.nodes.as_slice(), [n] if n.children.is_empty() && n.properties.len() == 1
            && !n.properties[0].values.is_empty())
    });
    if !holds_values {
        return Err(Error::new(
            target.kind.span,
            format!("`{value}` is not a value `{key}` holds"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::{format, parse};
    use crate::scene::Point;

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

    /// `src` with `key` set to `value` on the nodes that start with
    /// `nodes`, after checking that the result keeps acre's layout and
    /// builds.
    fn set(src: &str, nodes: &[&str], key: &str, value: &str) -> String {
        let spans: Vec<Span> = nodes.iter().map(|n| span_of(src, n)).collect();
        let out = apply(src, &property_edits(src, &spans, key, value).unwrap());
        assert_eq!(format(&out).unwrap(), out, "from\n{src}");
        build(&parse(&out).unwrap(), &[], &[]).unwrap();
        out
    }

    fn refusal(src: &str, node: &str, key: &str, value: &str) -> String {
        property_edits(src, &[span_of(src, node)], key, value)
            .unwrap_err()
            .message
    }

    /// Where the value of `key` on the node that starts with `node`
    /// comes from, and the value.
    fn value(src: &str, node: &str, key: &str) -> (Origin, Vec<ValueKind>) {
        let settings = property_values(src, span_of(src, node), &[key]).unwrap();
        let [Setting { values, origin, .. }] = settings.as_slice() else {
            panic!("no `{key}` on `{node}`");
        };
        (origin.clone(), values.clone())
    }

    #[test]
    fn a_nodes_own_value_is_replaced() {
        let src = "canvas {\n    size 100 100;\n    box a { size 5 5; fill #ff0000; stroke-width 2; }\n}\n";
        let out = set(src, &["box a"], "fill", "#00ff00");
        assert!(
            out.contains("box a { size 5 5; fill #00ff00; stroke-width 2; }"),
            "{out}"
        );
        let out = set(src, &["box a"], "stroke-width", "3.5");
        assert!(out.contains("stroke-width 3.5; }"), "{out}");
        let out = set(src, &["box a"], "fill", "none");
        assert!(out.contains("fill none;"), "{out}");
        assert!(
            property_edits(src, &[span_of(src, "box a")], "fill", "#ff0000")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_node_without_the_property_gets_one_before_its_first() {
        let src = "canvas {\n    size 100 100;\n    box a { size 5 5; }\n    box b {\n        size 5 5;\n    }\n    panel c { }\n}\n";
        let out = set(src, &["box a"], "stroke", "#123456");
        assert!(out.contains("box a { stroke #123456; size 5 5; }"), "{out}");
        let out = set(src, &["box b"], "dash", "4 2");
        assert!(
            out.contains("box b {\n        dash 4 2;\n        size 5 5;\n    }"),
            "{out}"
        );
        let out = set(src, &["panel c"], "fill", "#eeeeee");
        assert!(out.contains("panel c { fill #eeeeee; }"), "{out}");
    }

    #[test]
    fn a_value_set_on_the_node_goes_over_a_style() {
        let src = "canvas {\n    size 100 100;\n    style s { fill #ff0000; }\n    style box { stroke #0000ff; }\n    box a { style s; size 5 5; }\n}\n";
        assert_eq!(
            value(src, "box a", "fill"),
            (
                Origin::Style("s".into()),
                vec![ValueKind::Color([255, 0, 0, 255])]
            )
        );
        assert_eq!(
            value(src, "box a", "stroke"),
            (
                Origin::Style("box".into()),
                vec![ValueKind::Color([0, 0, 255, 255])]
            )
        );
        let out = set(src, &["box a"], "fill", "#00ff00");
        assert!(out.contains("style s { fill #ff0000; }"), "{out}");
        assert_eq!(
            value(&out, "box a", "fill"),
            (Origin::Node, vec![ValueKind::Color([0, 255, 0, 255])])
        );
    }

    #[test]
    fn a_later_style_goes_over_an_earlier_one_and_a_let_is_read_through() {
        let src = "canvas { size 100 100; let red #ff0000; style s { fill #000000; } style t { fill red; } box a { style s t; size 5 5; } }";
        assert_eq!(
            value(src, "box a", "fill"),
            (
                Origin::Style("t".into()),
                vec![ValueKind::Color([255, 0, 0, 255])]
            )
        );
        assert_eq!(value(src, "box a", "dash"), (Origin::Default, vec![]));
    }

    #[test]
    fn only_the_keys_the_kind_takes_are_read() {
        let src = "canvas { size 100 100; line l { from 0 0; to 5 5; } }";
        let keys: Vec<&str> =
            property_values(src, span_of(src, "line l"), &["fill", "stroke", "dash"])
                .unwrap()
                .iter()
                .map(|s| s.key)
                .collect();
        assert_eq!(keys, ["stroke", "dash"]);
    }

    #[test]
    fn an_empty_value_takes_the_nodes_own_away() {
        let src = "canvas {\n    size 100 100;\n    box a {\n        size 5 5;\n        dash 4 2;\n    }\n    box b { dash 1 1; size 5 5; }\n}\n";
        let out = set(src, &["box a"], "dash", "");
        assert!(out.contains("box a {\n        size 5 5;\n    }"), "{out}");
        let out = set(src, &["box b"], "dash", "");
        assert!(out.contains("box b { size 5 5; }"), "{out}");
        assert!(
            property_edits(src, &[span_of(src, "box a")], "fill", "")
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn several_nodes_take_the_value_in_one_set_of_edits() {
        let src = "canvas {\n    size 100 100;\n    group g {\n        at 1 1;\n        box t { size 5 5; text-color #000000; }\n    }\n    box a { size 5 5; }\n    box b { size 5 5; text-color #ffffff; }\n}\n";
        let out = set(src, &["box t", "box a", "box b"], "text-color", "#ff000080");
        assert!(out.contains("text-color #ff000080; }\n    }"), "{out}");
        assert!(
            out.contains("box a { text-color #ff000080; size 5 5; }"),
            "{out}"
        );
        assert!(
            out.contains("box b { size 5 5; text-color #ff000080; }"),
            "{out}"
        );
    }

    #[test]
    fn a_value_that_is_not_the_nodes_own_is_refused() {
        let src = "canvas { size 100 100; let red #ff0000; box a { fill red; size 5 5; } }";
        assert!(refusal(src, "box a", "fill", "#000000").contains("`red` comes from a `let`"));
        let src = "canvas { size 100 100; box a { stroke-width (1 + 1); size 5 5; } }";
        assert!(refusal(src, "box a", "stroke-width", "3").contains("`( ... )`"));
        let src = "canvas { size 100 100; repeat i 0 2 { box { size 5 5; } } }";
        assert!(refusal(src, "box {", "fill", "#000000").contains("`repeat`"));
    }

    #[test]
    fn a_blended_color_is_read_as_its_value_and_not_edited() {
        let src = "canvas { size 100 100; let bg #000000; box a { fill (mix bg #ffffff 0.5); size 5 5; } }";
        assert_eq!(
            value(src, "box a", "fill"),
            (Origin::Node, vec![ValueKind::Color([128, 128, 128, 255])])
        );
        assert_eq!(
            refusal(src, "box a", "fill", "#000000"),
            "`fill` is worked out from `( ... )`; edit it by hand"
        );
    }

    #[test]
    fn a_key_the_kind_does_not_take_or_a_bad_value_is_refused() {
        let src = "canvas { size 100 100; line l { from 0 0; to 5 5; } box a { size 5 5; } }";
        assert_eq!(
            refusal(src, "line l", "fill", "#000000"),
            "a `line` has no property `fill`"
        );
        assert!(refusal(src, "box a", "fill", "#000000; size 1 1").contains("not a value"));
        assert!(refusal(src, "box a", "fill", "} box {").contains("not a value"));
        assert!(refusal(src, "box a", "fill", "3").contains("`fill`"));
    }

    #[test]
    fn one_node_that_cannot_take_the_value_stops_them_all() {
        let src = "canvas { size 100 100; box a { size 5 5; } line l { from 0 0; to 5 5; } }";
        let spans = [span_of(src, "box a"), span_of(src, "line l")];
        assert!(property_edits(src, &spans, "fill", "#000000").is_err());
    }

    /// `src` with end `key` of the node that starts with `node` at `end`,
    /// after checking that the result keeps acre's layout and builds.
    fn ended(src: &str, node: &str, key: &str, end: EndAt) -> String {
        let out = apply(src, &end_edits(src, span_of(src, node), key, &end).unwrap());
        assert_eq!(format(&out).unwrap(), out, "from\n{src}");
        build(&parse(&out).unwrap(), &[], &[]).unwrap();
        out
    }

    #[test]
    fn an_end_takes_a_side_a_corner_a_point_along_or_a_point() {
        use crate::scene::Side;
        use crate::scene::edit::Spot;
        let src = "canvas {\n    size 100 100;\n    box a { at 10 10; size 20 20; }\n    \
                   arrow { from a; to 80 80; }\n}\n";
        let on = |spot| EndAt::On("a".into(), spot);
        let out = ended(src, "arrow", "from", on(Spot::Side(Side::Top)));
        assert!(out.contains("arrow { from a top; to 80 80; }"), "{out}");
        let corner = Spot::Corner {
            top: false,
            left: true,
        };
        let out = ended(src, "arrow", "from", on(corner));
        assert!(
            out.contains("arrow { from a bottom-left; to 80 80; }"),
            "{out}"
        );
        let out = ended(src, "arrow", "from", on(Spot::Along(Side::Right, 0.254)));
        assert!(out.contains("from a right 0.25; to"), "{out}");
        let out = ended(src, "arrow", "from", EndAt::Point(Point::new(5.0, 7.5)));
        assert!(out.contains("arrow { from 5 7.5; to 80 80; }"), "{out}");
        let same = end_edits(src, span_of(src, "arrow"), "from", &EndAt::Name("a".into()));
        assert_eq!(same.unwrap(), []);
    }

    #[test]
    fn an_end_not_the_lines_own_is_refused() {
        let end = EndAt::Point(Point::new(1.0, 1.0));
        let refused = |src: &str, node: &str, key: &str| {
            end_edits(src, span_of(src, node), key, &end)
                .unwrap_err()
                .message
        };
        let src = "canvas { size 100 100; let p 5 5; line { from p; to 9 9; } }";
        assert!(refused(src, "line", "from").contains("`p` comes from a `let`"));
        let src = "canvas { size 100 100; style line { from 0 0; } line { to 9 9; } }";
        assert!(refused(src, "line { to", "from").contains("comes from a style"));
        let src = "canvas { size 100 100; repeat i 0 2 { line { from 0 0; to 9 9; } } }";
        assert!(refused(src, "line", "to").contains("`repeat`"));
        let src = "canvas { size 100 100; box a { size 5 5; } line { from 0 0; to 9 9; } }";
        assert!(refused(src, "box", "to").contains("no end"));
        assert!(refused(src, "line", "label").contains("no end"));
    }
}

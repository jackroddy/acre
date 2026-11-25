use std::collections::HashMap;

use super::Color;
use crate::lang::{Error, Node, Property, Value, ValueKind};

/// The words properties take as they are, which a `let` would hide.
pub(super) const RESERVED: &[&str] = &[
    "none",
    "left",
    "center",
    "right",
    "top",
    "bottom",
    "show",
    "hide",
    "curly",
    "square",
    "mono",
    "mono-bold",
    "sans",
    "sans-bold",
    "sans-italic",
    "in",
    "out",
    "to",
    "ease",
    "linear",
    "in-out",
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
    "mix",
];

/// The canvas, the colors its lets name, and the `marker` style's
/// properties.
pub(super) type Expanded = (Node, HashMap<String, Color>, Vec<Property>);

/// The canvas with its `let` lines, `style` nodes and `repeat` nodes
/// applied and removed, and the colors its lets name. A style named after a node kind applies
/// to every node of that kind. A node's own property wins over a style's,
/// and a later style over an earlier one. The properties of the style
/// named `marker` come back as well, for the numbered text that wears
/// the same badge.
pub(super) fn expand(canvas: &Node) -> Result<Expanded, Error> {
    one_name(canvas)?;
    let lets = lets(canvas)?;
    let properties: Vec<&Property> = canvas
        .properties
        .iter()
        .filter(|p| p.key.text != "let")
        .collect();

    let mut styles: HashMap<&str, Vec<Property>> = HashMap::new();
    for node in &canvas.children {
        if node.kind.text != "style" {
            continue;
        }
        let name = node
            .name
            .as_ref()
            .ok_or_else(|| Error::new(node.kind.span, "a `style` needs a name"))?;
        if let Some(child) = node.children.first() {
            return Err(Error::new(child.kind.span, "a `style` holds no nodes"));
        }
        if styles.insert(&name.text, node.properties.clone()).is_some() {
            return Err(Error::new(
                name.span,
                format!("two styles are named `{}`", name.text),
            ));
        }
    }

    let mut out = Node {
        properties: properties
            .into_iter()
            .map(|p| {
                Ok(Property {
                    values: substitute(&p.values, &lets)?,
                    ..p.clone()
                })
            })
            .collect::<Result<_, Error>>()?,
        children: Vec::new(),
        ..canvas.clone()
    };
    for node in &canvas.children {
        if node.kind.text != "style" {
            apply(node, &lets, &styles, &mut out.children)?;
        }
    }
    let badge = match styles.get("marker") {
        Some(properties) => properties
            .iter()
            .map(|p| {
                Ok(Property {
                    values: substitute(&p.values, &lets)?,
                    ..p.clone()
                })
            })
            .collect::<Result<_, Error>>()?,
        None => Vec::new(),
    };
    let colors = lets
        .iter()
        .filter_map(|(name, values)| match values.as_slice() {
            [
                Value {
                    kind: ValueKind::Color([r, g, b, a]),
                    ..
                },
            ] => {
                let [r, g, b, a] = [r, g, b, a].map(|v| *v as f32 / 255.0);
                Some((name.to_string(), Color::rgba(r, g, b, a)))
            }
            _ => None,
        })
        .collect();
    Ok((out, colors, badge))
}

/// The values each `let` on `canvas` names, with the lets above it,
/// groups and blends already worked out.
pub(super) fn lets(canvas: &Node) -> Result<HashMap<&str, Vec<Value>>, Error> {
    let mut lets: HashMap<&str, Vec<Value>> = HashMap::new();
    for p in canvas.properties.iter().filter(|p| p.key.text == "let") {
        let Some((name, values)) = p.values.split_first() else {
            return Err(Error::new(p.span, "`let` takes a name and its values"));
        };
        let ValueKind::Word(word) = &name.kind else {
            return Err(Error::new(name.span, "a `let` name is a bare word"));
        };
        if values.is_empty() {
            return Err(Error::new(p.span, "`let` takes a name and its values"));
        }
        if RESERVED.contains(&word.as_str()) {
            return Err(Error::new(
                name.span,
                format!("`{word}` is a word acre reads itself, so it cannot be a `let` name"),
            ));
        }
        // substituted now, so a let may use the lets above it
        let values = substitute(values, &lets)?;
        if lets.insert(word, values).is_some() {
            return Err(Error::new(name.span, format!("`{word}` is set twice")));
        }
    }
    Ok(lets)
}

/// Push `node` onto `out`, with its children, styles applied and names
/// substituted, or push the copies a `repeat` node makes.
fn apply(
    node: &Node,
    lets: &HashMap<&str, Vec<Value>>,
    styles: &HashMap<&str, Vec<Property>>,
    out: &mut Vec<Node>,
) -> Result<(), Error> {
    if node.kind.text == "repeat" {
        return repeat(node, lets, styles, out);
    }
    one_name(node)?;
    let mut own: Vec<Property> = Vec::new();

    // a style named after the node's kind comes first, so a style the
    // node names, and the node's own properties, go over it
    let mut inherited: Vec<Property> = styles
        .get(node.kind.text.as_str())
        .cloned()
        .unwrap_or_default();
    for p in &node.properties {
        if p.key.text != "style" {
            own.push(p.clone());
            continue;
        }
        if p.values.is_empty() {
            return Err(Error::new(p.span, "`style` takes one or more style names"));
        }
        for v in &p.values {
            let ValueKind::Word(name) = &v.kind else {
                return Err(Error::new(v.span, "a style's name is a bare word"));
            };
            let style = styles
                .get(name.as_str())
                .ok_or_else(|| Error::new(v.span, format!("no style is named `{name}`")))?;
            for q in style {
                inherited.retain(|r| r.key.text != q.key.text);
                inherited.push(q.clone());
            }
        }
    }
    inherited.retain(|q| own.iter().all(|p| p.key.text != q.key.text));
    let properties = inherited
        .into_iter()
        .chain(own)
        .map(|p| {
            Ok(Property {
                values: substitute(&p.values, lets)?,
                ..p
            })
        })
        .collect::<Result<_, Error>>()?;
    let mut children = Vec::new();
    for c in &node.children {
        apply(c, lets, styles, &mut children)?;
    }
    out.push(Node {
        kind: node.kind.clone(),
        name: node.name.clone(),
        head: node.head.clone(),
        properties,
        children,
        span: node.span,
    });
    Ok(())
}

/// An error unless `node` has at most one name before `{`, or opens
/// with a time.
fn one_name(node: &Node) -> Result<(), Error> {
    if node.kind.text.is_empty() {
        return Ok(());
    }
    match node.head.as_slice() {
        [] => Ok(()),
        [one] => Err(Error::new(one.span, "a node's name is a bare word")),
        [_, extra, ..] => Err(Error::new(
            extra.span,
            "a node takes at most one name before `{`",
        )),
    }
}

/// Push a copy of the children of `repeat INDEX FIRST LAST { ... }` onto
/// `out` for each whole number from FIRST to LAST, both included, with
/// INDEX standing for that number. A node named inside the repeat has
/// the number added to its name in each copy, and so does each word
/// that names it there.
fn repeat(
    node: &Node,
    lets: &HashMap<&str, Vec<Value>>,
    styles: &HashMap<&str, Vec<Property>>,
    out: &mut Vec<Node>,
) -> Result<(), Error> {
    let [index, first, last] = node.head.as_slice() else {
        return Err(Error::new(
            node.kind.span,
            "`repeat` takes an index name, a first number and a last number before `{`",
        ));
    };
    let ValueKind::Word(word) = &index.kind else {
        return Err(Error::new(index.span, "a `repeat` index is a bare word"));
    };
    if RESERVED.contains(&word.as_str()) {
        return Err(Error::new(
            index.span,
            format!("`{word}` is a word acre reads itself, so it cannot be a `repeat` index"),
        ));
    }
    if lets.contains_key(word.as_str()) {
        return Err(Error::new(
            index.span,
            format!("`{word}` already names a value, so it cannot be a `repeat` index"),
        ));
    }
    if let Some(p) = node.properties.first() {
        return Err(Error::new(p.span, "a `repeat` holds nodes, not properties"));
    }
    let whole = |v: &Value| match substitute(std::slice::from_ref(v), lets)?.as_slice() {
        [
            Value {
                kind: ValueKind::Number(n),
                ..
            },
        ] if n.fract() == 0.0 => Ok(*n as i64),
        _ => Err(Error::new(
            v.span,
            "a `repeat` counts between whole numbers",
        )),
    };
    let (first, last) = (whole(first)?, whole(last)?);

    let mut names = Vec::new();
    named(&node.children, &mut names);
    for i in first..=last {
        let mut lets = lets.clone();
        let value = Value {
            kind: ValueKind::Number(i as f64),
            span: index.span,
        };
        lets.insert(word.as_str(), vec![value]);
        for child in &node.children {
            let mut child = child.clone();
            rename(&mut child, &names, i);
            apply(&child, &lets, styles, out)?;
        }
    }
    Ok(())
}

/// Push the name of every node in `nodes`, and in their children, onto
/// `names`.
fn named(nodes: &[Node], names: &mut Vec<String>) {
    for node in nodes {
        if let Some(name) = &node.name {
            names.push(name.text.clone());
        }
        named(&node.children, names);
    }
}

/// Add `i` to the name of `node` and of its children, and to each word
/// of their properties, wherever the name is one of `names`.
fn rename(node: &mut Node, names: &[String], i: i64) {
    if let Some(name) = &mut node.name
        && names.contains(&name.text)
    {
        name.text = format!("{}{i}", name.text);
    }
    for p in &mut node.properties {
        for v in &mut p.values {
            if let ValueKind::Word(w) = &mut v.kind
                && names.contains(w)
            {
                *w = format!("{w}{i}");
            }
        }
    }
    for child in &mut node.children {
        rename(child, names, i);
    }
}

/// `values` with each let name replaced by its values and each group
/// replaced by the number it comes to. A group that opens with the name
/// of a let of several values, such as `(tints i)`, is replaced by the
/// one the rest of the group counts to, from 0, and one that opens with
/// `mix` by the color it blends.
pub(super) fn substitute(
    values: &[Value],
    lets: &HashMap<&str, Vec<Value>>,
) -> Result<Vec<Value>, Error> {
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        match &v.kind {
            ValueKind::Word(w) if lets.contains_key(w.as_str()) => {
                out.extend(lets[w.as_str()].iter().cloned());
            }
            ValueKind::Group(inner)
                if let [first, rest @ ..] = inner.as_slice()
                    && first.kind == ValueKind::Word("mix".into()) =>
            {
                out.push(Value {
                    kind: ValueKind::Color(mix(rest, v, lets)?),
                    span: v.span,
                });
            }
            ValueKind::Group(inner)
                if let [first, rest @ ..] = inner.as_slice()
                    && let ValueKind::Word(name) = &first.kind
                    && let Some(list) = lets.get(name.as_str())
                    && list.len() > 1 =>
            {
                let rest = substitute(rest, lets)?;
                let mut terms = rest.as_slice();
                let n = sum(&mut terms, v)?;
                if let Some(extra) = terms.first() {
                    return Err(Error::new(extra.span, "expected an operator"));
                }
                if n.fract() != 0.0 || n < 0.0 || n >= list.len() as f64 {
                    return Err(Error::new(
                        v.span,
                        format!(
                            "`{name}` has {} values, so `({name} N)` takes a whole N from 0 to {}",
                            list.len(),
                            list.len() - 1
                        ),
                    ));
                }
                out.push(Value {
                    kind: list[n as usize].kind.clone(),
                    span: v.span,
                });
            }
            ValueKind::Group(inner) => {
                let inner = substitute(inner, lets)?;
                let mut terms = inner.as_slice();
                let n = sum(&mut terms, v)?;
                if let Some(extra) = terms.first() {
                    return Err(Error::new(extra.span, "expected an operator"));
                }
                out.push(Value {
                    kind: ValueKind::Number(n),
                    span: v.span,
                });
            }
            _ => out.push(v.clone()),
        }
    }
    Ok(out)
}

/// The color of `(mix A B F)`, whose values after `mix` are `operands`:
/// F of the way from color A to color B in each of red, green, blue and
/// alpha, rounded to the nearest whole value with halves rounded up.
fn mix(
    operands: &[Value],
    group: &Value,
    lets: &HashMap<&str, Vec<Value>>,
) -> Result<[u8; 4], Error> {
    let [a, b, f] = operands else {
        return Err(Error::new(
            group.span,
            "`mix` takes two colors and a fraction, as in `(mix A B 0.5)`",
        ));
    };

    // each operand on its own, so an error points at it and not at
    // the `let` its values came from
    let one = |v: &Value| match substitute(std::slice::from_ref(v), lets)?.as_slice() {
        [one] => Ok(one.kind.clone()),
        _ => Err(Error::new(v.span, "this stands for several values")),
    };
    let color = |v: &Value| match one(v)? {
        ValueKind::Color(c) => Ok(c),
        _ => Err(Error::new(
            v.span,
            "`mix` blends two colors, and this is not a color",
        )),
    };
    let (a, b) = (color(a)?, color(b)?);
    let f = match one(f)? {
        ValueKind::Number(n) if (0.0..=1.0).contains(&n) => n,
        _ => {
            return Err(Error::new(
                f.span,
                "the fraction of a `mix` is a number from 0 to 1",
            ));
        }
    };
    Ok(std::array::from_fn(|i| {
        let (a, b) = (f64::from(a[i]), f64::from(b[i]));
        (a + (b - a) * f).round() as u8
    }))
}

// a group is evaluated by precedence climbing over its values:
//
//   sum     = product (("+" | "-") product)*
//   product = term (("*" | "/") term)*
//   term    = number | "-" term
//
// a nested group arrives here already a number. `(a -1)` lexes as a
// name and the number -1, so a negative number straight after a term
// reads as a subtraction

fn sum(terms: &mut &[Value], group: &Value) -> Result<f64, Error> {
    let mut n = product(terms, group)?;
    loop {
        match terms.first().map(|v| &v.kind) {
            Some(ValueKind::Op('+')) => {
                *terms = &terms[1..];
                n += product(terms, group)?;
            }
            Some(ValueKind::Op('-')) => {
                *terms = &terms[1..];
                n -= product(terms, group)?;
            }
            Some(ValueKind::Number(m)) if *m < 0.0 => {
                n += product(terms, group)?;
            }
            _ => return Ok(n),
        }
    }
}

fn product(terms: &mut &[Value], group: &Value) -> Result<f64, Error> {
    let mut n = term(terms, group)?;
    loop {
        match terms.first().map(|v| &v.kind) {
            Some(ValueKind::Op('*')) => {
                *terms = &terms[1..];
                n *= term(terms, group)?;
            }
            Some(ValueKind::Op('/')) => {
                *terms = &terms[1..];
                n /= term(terms, group)?;
            }
            _ => return Ok(n),
        }
    }
}

fn term(terms: &mut &[Value], group: &Value) -> Result<f64, Error> {
    let Some((first, rest)) = terms.split_first() else {
        return Err(Error::new(group.span, "expected a number before `)`"));
    };
    *terms = rest;
    match &first.kind {
        ValueKind::Number(n) => Ok(*n),
        ValueKind::Op('-') => Ok(-term(terms, group)?),
        ValueKind::Word(w) => Err(Error::new(first.span, format!("`{w}` is not a number"))),
        _ => Err(Error::new(first.span, "expected a number")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;

    fn expanded(src: &str) -> Node {
        expand(&parse(src).unwrap().nodes[0]).unwrap().0
    }

    fn values(p: &Property) -> Vec<ValueKind> {
        p.values.iter().map(|v| v.kind.clone()).collect()
    }

    #[test]
    fn a_let_name_stands_for_its_values_wherever_it_appears() {
        let canvas = expanded(
            "canvas { let ink #102030; let unit 15 46; size 10 10;
                row { unit unit; stroke ink; } }",
        );
        assert!(canvas.properties.iter().all(|p| p.key.text != "let"));
        let row = &canvas.children[0];
        assert_eq!(
            values(&row.properties[0]),
            [ValueKind::Number(15.0), ValueKind::Number(46.0)]
        );
        assert_eq!(
            values(&row.properties[1]),
            [ValueKind::Color([0x10, 0x20, 0x30, 0xff])]
        );
    }

    #[test]
    fn a_group_comes_to_a_number_after_names_are_replaced() {
        let canvas = expanded(
            "canvas { let u 15; let x (u * 4); size (x + 2 * u) (100 - (u - 5) / 2) (-u) (x -1); }",
        );
        assert_eq!(
            values(&canvas.properties[0]),
            [
                ValueKind::Number(90.0),
                ValueKind::Number(95.0),
                ValueKind::Number(-15.0),
                ValueKind::Number(59.0),
            ]
        );
    }

    #[test]
    fn a_node_keeps_its_own_properties_over_its_styles() {
        let canvas = expanded(
            "canvas { size 10 10;
                style a { fill #000000; radius 4; }
                style b { radius 8; }
                box { style a b; fill #ffffff; } }",
        );
        assert_eq!(canvas.children.len(), 1);
        let props: Vec<(String, Vec<ValueKind>)> = canvas.children[0]
            .properties
            .iter()
            .map(|p| (p.key.text.clone(), values(p)))
            .collect();
        assert_eq!(
            props,
            [
                ("radius".into(), vec![ValueKind::Number(8.0)]),
                ("fill".into(), vec![ValueKind::Color([255, 255, 255, 255])]),
            ]
        );
    }

    #[test]
    fn a_let_may_not_hide_a_word_acre_reads() {
        let program = parse("canvas { let left 230; size 10 10; }").unwrap();
        let err = expand(&program.nodes[0]).unwrap_err();
        assert_eq!(
            err.message,
            "`left` is a word acre reads itself, so it cannot be a `let` name"
        );
    }

    #[test]
    fn a_style_named_after_a_kind_applies_to_every_node_of_that_kind() {
        let canvas = expanded(
            "canvas { size 10 10;
                style box { radius 4; fill #000000; }
                box { fill #ffffff; }
                text { }
            }",
        );
        let keys = |n: usize| -> Vec<String> {
            canvas.children[n]
                .properties
                .iter()
                .map(|p| p.key.text.clone())
                .collect()
        };
        assert_eq!(keys(0), ["radius", "fill"]);
        assert_eq!(
            values(&canvas.children[0].properties[1]),
            [ValueKind::Color([255, 255, 255, 255])]
        );
        assert!(keys(1).is_empty());
    }

    #[test]
    fn a_repeat_copies_its_children_for_each_index_from_first_to_last() {
        let canvas = expanded(
            "canvas { size 10 10; let w 20; let tints #000000 #ffffff;
                box first { }
                repeat i 1 (w / 10) {
                    box b { at (i * w) 0; fill (tints (i - 1)); }
                    arrow { from b bottom; to first; }
                } }",
        );
        let kinds: Vec<(&str, Option<&str>)> = canvas
            .children
            .iter()
            .map(|n| {
                (
                    n.kind.text.as_str(),
                    n.name.as_ref().map(|w| w.text.as_str()),
                )
            })
            .collect();
        assert_eq!(
            kinds,
            [
                ("box", Some("first")),
                ("box", Some("b1")),
                ("arrow", None),
                ("box", Some("b2")),
                ("arrow", None),
            ]
        );
        let b2 = &canvas.children[3];
        assert_eq!(
            values(&b2.properties[0]),
            [ValueKind::Number(40.0), ValueKind::Number(0.0)]
        );
        assert_eq!(
            values(&b2.properties[1]),
            [ValueKind::Color([255, 255, 255, 255])]
        );
        assert_eq!(
            values(&canvas.children[4].properties[0]),
            [
                ValueKind::Word("b2".into()),
                ValueKind::Word("bottom".into())
            ]
        );
        assert_eq!(
            values(&canvas.children[4].properties[1]),
            [ValueKind::Word("first".into())]
        );
    }

    #[test]
    fn a_nested_repeat_adds_the_outer_index_to_a_name_first() {
        let canvas = expanded(
            "canvas { size 10 10;
                repeat i 0 1 { repeat j i 1 { box b { at i j; } } } }",
        );
        let names: Vec<&str> = canvas
            .children
            .iter()
            .map(|n| n.name.as_ref().unwrap().text.as_str())
            .collect();
        assert_eq!(names, ["b00", "b01", "b11"]);
    }

    #[test]
    fn a_repeat_whose_last_is_before_its_first_draws_nothing() {
        let canvas = expanded("canvas { size 10 10; repeat i 0 -1 { box { } } }");
        assert!(canvas.children.is_empty());
    }

    #[test]
    fn a_repeat_index_may_not_hide_a_name() {
        let err = |src: &str| expand(&parse(src).unwrap().nodes[0]).unwrap_err().message;
        assert_eq!(
            err("canvas { let i 2; repeat i 0 3 { box { } } }"),
            "`i` already names a value, so it cannot be a `repeat` index"
        );
        assert_eq!(
            err("canvas { repeat i 0 1 { repeat i 0 1 { box { } } } }"),
            "`i` already names a value, so it cannot be a `repeat` index"
        );
        assert_eq!(
            err("canvas { repeat i 0 1.5 { box { } } }"),
            "a `repeat` counts between whole numbers"
        );
        assert_eq!(
            err("canvas { repeat i 3 { box { } } }"),
            "`repeat` takes an index name, a first number and a last number before `{`"
        );
    }

    #[test]
    fn a_list_index_outside_the_list_is_an_error() {
        let program = parse("canvas { let t 1 2 3; size (t 3) 1; }").unwrap();
        let err = expand(&program.nodes[0]).unwrap_err();
        assert_eq!(
            err.message,
            "`t` has 3 values, so `(t N)` takes a whole N from 0 to 2"
        );
    }

    #[test]
    fn a_mix_blends_each_channel_and_rounds_halves_up() {
        let canvas = expanded(
            "canvas { size 10 10; background (mix #000000 #ffffff 0.5);
                box { fill (mix #00000000 #ff8040ff 0.25); stroke (mix #102030 #405060 0); }
                box { fill (mix #102030 #405060 1); } }",
        );
        assert_eq!(
            values(&canvas.properties[1]),
            [ValueKind::Color([0x80, 0x80, 0x80, 0xff])]
        );
        let first = &canvas.children[0];
        assert_eq!(
            values(&first.properties[0]),
            [ValueKind::Color([64, 32, 16, 64])]
        );
        assert_eq!(
            values(&first.properties[1]),
            [ValueKind::Color([0x10, 0x20, 0x30, 0xff])]
        );
        assert_eq!(
            values(&canvas.children[1].properties[0]),
            [ValueKind::Color([0x40, 0x50, 0x60, 0xff])]
        );
    }

    #[test]
    fn a_mix_takes_let_names_nested_mixes_and_goes_in_a_let() {
        let (canvas, colors, _) = expand(
            &parse(
                "canvas { size 10 10; let background #000000; let red #ff0000; let f 0.25;
                    let card (mix background red (f * 2));
                    let tints (mix background red 0) (mix (mix background red 1) #ffffff 0.5);
                    box { fill card; stroke (tints 1); } }",
            )
            .unwrap()
            .nodes[0],
        )
        .unwrap();
        let fill = &canvas.children[0].properties;
        assert_eq!(values(&fill[0]), [ValueKind::Color([128, 0, 0, 255])]);
        assert_eq!(values(&fill[1]), [ValueKind::Color([255, 128, 128, 255])]);
        assert_eq!(colors["card"], Color::rgba(128.0 / 255.0, 0.0, 0.0, 1.0));
    }

    #[test]
    fn a_mix_needs_two_colors_and_a_fraction_from_0_to_1() {
        fn err(src: &str) -> (String, &str) {
            let program = parse(src).unwrap();
            let err = expand(&program.nodes[0]).unwrap_err();
            (err.message, &src[err.span.start..err.span.end])
        }
        assert_eq!(
            err("canvas { size 1 1; box { fill (mix #000000 #ffffff 1.5); } }"),
            (
                "the fraction of a `mix` is a number from 0 to 1".into(),
                "1.5"
            )
        );
        assert_eq!(
            err("canvas { size 1 1; box { fill (mix #000000 #ffffff (0 - 0.5)); } }"),
            (
                "the fraction of a `mix` is a number from 0 to 1".into(),
                "(0 - 0.5)"
            )
        );
        assert_eq!(
            err("canvas { size 1 1; let w 4; box { fill (mix #000000 w 0.5); } }"),
            (
                "`mix` blends two colors, and this is not a color".into(),
                "w"
            )
        );
        assert_eq!(
            err("canvas { size 1 1; box { fill (mix #000000 #ffffff #ffffff); } }"),
            (
                "the fraction of a `mix` is a number from 0 to 1".into(),
                "#ffffff"
            )
        );
        assert_eq!(
            err(
                "canvas { size 1 1; let two #000000 #ffffff; box { fill (mix two #ffffff 0.5); } }"
            ),
            ("this stands for several values".into(), "two")
        );
        assert_eq!(
            err("canvas { size 1 1; box { fill (mix #000000 #ffffff); } }"),
            (
                "`mix` takes two colors and a fraction, as in `(mix A B 0.5)`".into(),
                "(mix #000000 #ffffff)"
            )
        );
        assert_eq!(
            err("canvas { size 1 1; box { fill (mix #000000 #ffffff 0.5 0.5); } }").0,
            "`mix` takes two colors and a fraction, as in `(mix A B 0.5)`"
        );
        assert_eq!(
            err("canvas { size 1 1; let mix #000000; }").0,
            "`mix` is a word acre reads itself, so it cannot be a `let` name"
        );
        assert_eq!(
            err("canvas { size (#000000 + 1) 1; }"),
            ("expected a number".into(), "#000000")
        );
    }

    #[test]
    fn a_node_other_than_repeat_takes_at_most_one_name() {
        let program = parse("canvas { size 1 1; box a b { } }").unwrap();
        let err = expand(&program.nodes[0]).unwrap_err();
        assert_eq!(err.message, "a node takes at most one name before `{`");
        assert_eq!(err.span.start, 25);
    }

    #[test]
    fn an_unknown_style_is_an_error() {
        let program = parse("canvas { size 10 10; box { style nope; } }").unwrap();
        let err = expand(&program.nodes[0]).unwrap_err();
        assert_eq!(err.message, "no style is named `nope`");
    }
}

use std::collections::HashMap;

use super::Color;
use crate::lang::{Error, Node, Property, Value, ValueKind};

/// The words properties take as they are, which a `let` would hide.
const RESERVED: &[&str] = &[
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
    "sans",
    "sans-bold",
    "sans-italic",
];

/// The canvas, the colors its lets name, and the `marker` style's
/// properties.
pub(super) type Expanded = (Node, HashMap<String, Color>, Vec<Property>);

/// The canvas with its `let` lines and `style` nodes applied and removed,
/// and the colors its lets name. A style named after a node kind applies
/// to every node of that kind. A node's own property wins over a style's,
/// and a later style over an earlier one. The properties of the style
/// named `marker` come back as well, for the numbered text that wears
/// the same badge.
pub(super) fn expand(canvas: &Node) -> Result<Expanded, Error> {
    let mut lets: HashMap<&str, Vec<Value>> = HashMap::new();
    let mut properties = Vec::new();
    for p in &canvas.properties {
        if p.key.text != "let" {
            properties.push(p.clone());
            continue;
        }
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
                    ..p
                })
            })
            .collect::<Result<_, Error>>()?,
        children: Vec::new(),
        ..canvas.clone()
    };
    for node in &canvas.children {
        if node.kind.text != "style" {
            out.children.push(apply(node, &lets, &styles)?);
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

/// `node` and its children with styles applied and names substituted.
fn apply(
    node: &Node,
    lets: &HashMap<&str, Vec<Value>>,
    styles: &HashMap<&str, Vec<Property>>,
) -> Result<Node, Error> {
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
    let children = node
        .children
        .iter()
        .map(|c| apply(c, lets, styles))
        .collect::<Result<_, _>>()?;
    Ok(Node {
        kind: node.kind.clone(),
        name: node.name.clone(),
        properties,
        children,
        span: node.span,
    })
}

/// `values` with each let name replaced by its values and each group
/// replaced by the number it comes to.
fn substitute(values: &[Value], lets: &HashMap<&str, Vec<Value>>) -> Result<Vec<Value>, Error> {
    let mut out = Vec::with_capacity(values.len());
    for v in values {
        match &v.kind {
            ValueKind::Word(w) if lets.contains_key(w.as_str()) => {
                out.extend(lets[w.as_str()].iter().cloned());
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
    fn an_unknown_style_is_an_error() {
        let program = parse("canvas { size 10 10; box { style nope; } }").unwrap();
        let err = expand(&program.nodes[0]).unwrap_err();
        assert_eq!(err.message, "no style is named `nope`");
    }
}

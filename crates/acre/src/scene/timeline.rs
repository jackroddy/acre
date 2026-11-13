//! The `timeline` node: what happens to named nodes, and when.
//!
//! ```text
//! timeline {
//!     0s to 1s { scale a 1.5; }
//!     1s to 2s ease linear { move a 0 80; fade b out; }
//!     2s { show c; fill r 3 #ffcc00; }
//! }
//! ```
//!
//! Each entry is a span, `T to T`, or an instant, `T`, and holds
//! actions. An action changes one thing about one node from where it
//! stands to a new value, over the span or at the instant. The
//! actions on one thing make one [`Track`].

use std::collections::HashMap;

use super::primitive::{self, Color, Point};
use super::track::{Easing, Keyframe, Lerp, Track};
use super::{Kind, Object, Paint, Part};
use crate::fonts::Fonts;
use crate::lang::{Error, Node, Property, Span, Value, ValueKind};

/// The easing of a span that names none.
const EASING: Easing = Easing::EaseInOut;

const EASINGS: [(&str, Easing); 4] = [
    ("linear", Easing::Linear),
    ("in", Easing::EaseIn),
    ("out", Easing::EaseOut),
    ("in-out", Easing::EaseInOut),
];

/// What one action changes, to what.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Change {
    Scale(f64),
    Move(Point),
    Opacity(f64),
    Color(Part, Color),
}

/// The thing a run of actions changes, which makes one track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Channel {
    Scale,
    Move,
    Opacity,
    Color(Part, Option<usize>),
}

impl Change {
    fn channel(self, cell: Option<usize>) -> Channel {
        match self {
            Change::Scale(_) => Channel::Scale,
            Change::Move(_) => Channel::Move,
            Change::Opacity(_) => Channel::Opacity,
            Change::Color(part, _) => Channel::Color(part, cell),
        }
    }
}

struct Action {
    start: f64,
    end: f64,
    /// `Hold` for an action at an instant.
    easing: Easing,
    change: Change,
    /// Whether the action is `show` or `fade NAME in`, which hide the
    /// node until they run when they are its first.
    reveals: bool,
    span: Span,
}

/// The actions on one thing of the node a name names.
type Run<'a> = ((&'a str, Channel), Vec<Action>);

/// Apply the canvas's `timeline` to `objects`, and return the time of
/// its last change, or 0 without one.
pub(super) fn apply(canvas: &Node, objects: &mut [Object], fonts: &Fonts) -> Result<f64, Error> {
    let mut nodes = HashMap::new();
    named(&canvas.children, true, &mut nodes)?;
    let mut timelines = canvas.children.iter().filter(|n| n.kind.text == "timeline");
    let Some(timeline) = timelines.next() else {
        return Ok(0.0);
    };
    if let Some(extra) = timelines.next() {
        return Err(Error::new(extra.kind.span, "a canvas holds one `timeline`"));
    }
    if let Some(name) = &timeline.name {
        return Err(Error::new(name.span, "`timeline` takes no name"));
    }
    if let Some(p) = timeline.properties.first() {
        return Err(Error::new(
            p.key.span,
            "a `timeline` holds entries such as `1s to 2s { ... }`, not properties",
        ));
    }

    let (runs, last) = runs(timeline, &nodes)?;
    for ((name, channel), actions) in runs {
        let node = nodes[name].expect("`target` lets through only names one node has");
        let indices: Vec<usize> = (0..objects.len())
            .filter(|&i| objects[i].source == node.span)
            .collect();
        match channel {
            Channel::Scale => {
                let track = track(1.0, &actions, |c| match c {
                    Change::Scale(s) => s,
                    _ => unreachable!("a scale run holds scales"),
                });
                let pivot = pivot(node, objects, &indices, fonts)?;
                for &i in &indices {
                    objects[i].scale = track.clone();
                    objects[i].pivot = pivot;
                }
            }
            Channel::Move => {
                let track = track(Point::ZERO, &actions, |c| match c {
                    Change::Move(p) => p,
                    _ => unreachable!("a move run holds moves"),
                });
                for &i in &indices {
                    objects[i].offset = track.clone();
                }
            }
            Channel::Opacity => {
                let base = if actions[0].reveals { 0.0 } else { 1.0 };
                let track = track(base, &actions, |c| match c {
                    Change::Opacity(o) => o,
                    _ => unreachable!("an opacity run holds opacities"),
                });
                for &i in &indices {
                    objects[i].opacity = track.clone();
                }
            }
            Channel::Color(part, cell) => {
                for &i in &indices {
                    let paint = paint(&objects[i].kind, name, part, cell, &actions)?;
                    objects[i].paints.push(paint);
                }
            }
        }
    }
    Ok(last)
}

/// The actions of `timeline` on each thing of each node, in the order
/// the things first appear, each run in time order, and the time of
/// the last change.
fn runs<'a>(
    timeline: &'a Node,
    nodes: &HashMap<&str, Option<&Node>>,
) -> Result<(Vec<Run<'a>>, f64), Error> {
    let mut runs: Vec<Run> = Vec::new();
    let mut last = 0.0f64;
    for entry in &timeline.children {
        let (start, end, easing) = when(entry)?;
        last = last.max(end);
        let instant = start == end;
        for p in &entry.properties {
            let (name, cell, rest) = target(p, nodes)?;
            let change = change(p, rest)?;
            if !instant && matches!(p.key.text.as_str(), "show" | "hide") {
                return Err(Error::new(
                    p.key.span,
                    format!(
                        "`{}` happens at an instant, such as `2s {{ ... }}`; `fade` takes a span",
                        p.key.text
                    ),
                ));
            }
            let action = Action {
                start,
                end,
                easing: if instant { Easing::Hold } else { easing },
                change,
                reveals: match p.key.text.as_str() {
                    "show" => true,
                    "fade" => matches!(rest, [v] if v.kind == ValueKind::Word("in".into())),
                    _ => false,
                },
                span: p.span,
            };
            let key = (name, change.channel(cell));
            match runs.iter_mut().find(|(k, _)| *k == key) {
                Some((_, run)) => run.push(action),
                None => runs.push((key, vec![action])),
            }
        }
    }
    for ((name, _), actions) in &mut runs {
        actions.sort_by(|a, b| a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end)));
        for pair in actions.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            if b.start < a.end || (a.start == b.start && a.end == b.end) {
                return Err(Error::new(
                    b.span,
                    format!(
                        "this changes `{name}` while another action from {}s to {}s does",
                        a.start, a.end
                    ),
                ));
            }
        }
    }
    Ok((runs, last))
}

/// The paint that `actions` make of color `part` of `kind`, the object
/// of the node `name` names.
fn paint(
    kind: &Kind,
    name: &str,
    part: Part,
    cell: Option<usize>,
    actions: &[Action],
) -> Result<Paint, Error> {
    let color_of = |c| match c {
        Change::Color(_, c) => c,
        _ => unreachable!("a color run holds colors"),
    };
    let first = &actions[0];
    if let (Kind::Row(row), Some(cell)) = (kind, cell)
        && cell >= row.cells.len()
    {
        return Err(Error::new(
            first.span,
            format!("row `{name}` has cells 0 to {}", row.cells.len() - 1),
        ));
    }
    let base = match kind.color(part, cell) {
        Ok(Some(c)) => c,
        // a fill the node lacks fades in from clear
        Ok(None) => color_of(first.change).faded(0.0),
        Err(why) => return Err(Error::new(first.span, format!("`{name}` {why}"))),
    };
    Ok(Paint {
        part,
        cell,
        color: track(base, actions, color_of),
        start: first.start,
    })
}

/// Every named node in `children` and below that a timeline can change,
/// or `None` for a name two nodes share. A `timeline` below the canvas
/// is an error.
fn named<'a>(
    children: &'a [Node],
    top: bool,
    out: &mut HashMap<&'a str, Option<&'a Node>>,
) -> Result<(), Error> {
    for node in children {
        match node.kind.text.as_str() {
            "timeline" if !top => {
                return Err(Error::new(
                    node.kind.span,
                    "a `timeline` goes in the canvas",
                ));
            }
            "palette" | "style" | "fonts" | "timeline" => continue,
            _ => {}
        }
        if let Some(name) = &node.name {
            out.entry(name.text.as_str())
                .and_modify(|n| *n = None)
                .or_insert(Some(node));
        }
        named(&node.children, false, out)?;
    }
    Ok(())
}

/// When an entry's actions start and end, and how a span eases.
fn when(entry: &Node) -> Result<(f64, f64, Easing), Error> {
    if !entry.kind.text.is_empty() {
        return Err(Error::new(
            entry.kind.span,
            "a `timeline` holds entries that start with a time, such as `1s to 2s { ... }`",
        ));
    }
    if let Some(child) = entry.children.first() {
        return Err(Error::new(child.span, "an entry holds actions, not nodes"));
    }
    let time = |v: &Value| match v.kind {
        ValueKind::Time(t) if t >= 0.0 => Ok(t),
        _ => Err(Error::new(
            v.span,
            "expected a time in seconds, such as `1.5s`",
        )),
    };
    let word = |v: &Value, w: &str| v.kind == ValueKind::Word(w.into());
    match entry.head.as_slice() {
        [at] => Ok((time(at)?, time(at)?, Easing::Hold)),
        [from, to, end, rest @ ..] if word(to, "to") => {
            let (start, end_t) = (time(from)?, time(end)?);
            if end_t <= start {
                return Err(Error::new(
                    end.span,
                    format!("this span ends at {end_t}s, which is not after it starts at {start}s"),
                ));
            }
            let easing = match rest {
                [] => EASING,
                [ease, name] if word(ease, "ease") => EASINGS
                    .iter()
                    .find(|(n, _)| word(name, n))
                    .map(|(_, e)| *e)
                    .ok_or_else(|| {
                        Error::new(name.span, "an easing is linear, in, out or in-out")
                    })?,
                [first, ..] => {
                    return Err(Error::new(
                        first.span,
                        "after the times, a span takes only `ease linear | in | out | in-out`",
                    ));
                }
            };
            Ok((start, end_t, easing))
        }
        [_, second, ..] => Err(Error::new(
            second.span,
            "an entry is a time, `2s`, or a span, `1s to 2s`",
        )),
        [] => unreachable!("the parser opens a timed node with a time"),
    }
}

/// The name and cell an action's values start with, and the values
/// after them.
fn target<'p>(
    p: &'p Property,
    nodes: &HashMap<&str, Option<&Node>>,
) -> Result<(&'p str, Option<usize>, &'p [Value]), Error> {
    let Some((first, rest)) = p.values.split_first() else {
        return Err(Error::new(
            p.span,
            format!("`{}` takes a node's name", p.key.text),
        ));
    };
    let ValueKind::Word(name) = &first.kind else {
        return Err(Error::new(first.span, "expected a node's name"));
    };
    let node = match nodes.get(name.as_str()) {
        Some(Some(node)) => *node,
        Some(None) => {
            return Err(Error::new(
                first.span,
                format!("two nodes are named `{name}`, so a timeline cannot tell which"),
            ));
        }
        None => return Err(Error::new(first.span, format!("no node is named `{name}`"))),
    };
    if matches!(
        node.kind.text.as_str(),
        "group" | "panel" | "stack" | "grid"
    ) {
        return Err(Error::new(
            first.span,
            format!(
                "`{name}` is a {}; a timeline changes the nodes inside it",
                node.kind.text
            ),
        ));
    }
    match rest.split_first() {
        Some((
            Value {
                kind: ValueKind::Number(n),
                span,
            },
            rest,
        )) if node.kind.text == "row" && is_color_action(p) => {
            if n.fract() != 0.0 || *n < 0.0 {
                return Err(Error::new(*span, "a cell's index is a whole number from 0"));
            }
            Ok((name, Some(*n as usize), rest))
        }
        _ => Ok((name, None, rest)),
    }
}

fn is_color_action(p: &Property) -> bool {
    matches!(p.key.text.as_str(), "fill" | "stroke" | "text-color")
}

/// What action `p` changes, from the values after its target.
fn change(p: &Property, rest: &[Value]) -> Result<Change, Error> {
    let number = |v: &Value| match v.kind {
        ValueKind::Number(n) => Ok(n),
        _ => Err(Error::new(v.span, "expected a number")),
    };
    let expected = |what: &str| Error::new(p.span, format!("`{}` takes {what}", p.key.text));
    let color = |part| match rest {
        [
            Value {
                kind: ValueKind::Color([r, g, b, a]),
                ..
            },
        ] => {
            let [r, g, b, a] = [r, g, b, a].map(|v| *v as f32 / 255.0);
            Ok(Change::Color(part, Color::rgba(r, g, b, a)))
        }
        _ => Err(expected(
            "a node's name, a cell's index for a row, and a color",
        )),
    };
    match p.key.text.as_str() {
        "scale" => match rest {
            [s] => {
                let s = number(s)?;
                if s < 0.0 {
                    return Err(expected("a name and a factor that is not negative"));
                }
                Ok(Change::Scale(s))
            }
            _ => Err(expected("a name and a factor, such as `scale a 1.5`")),
        },
        "move" => match rest {
            [dx, dy] => Ok(Change::Move(Point::new(number(dx)?, number(dy)?))),
            _ => Err(expected(
                "a name and how far across and down from where layout put it",
            )),
        },
        "fade" => match rest {
            [v] => match &v.kind {
                ValueKind::Word(w) if w == "in" => Ok(Change::Opacity(1.0)),
                ValueKind::Word(w) if w == "out" => Ok(Change::Opacity(0.0)),
                ValueKind::Number(o) if (0.0..=1.0).contains(o) => Ok(Change::Opacity(*o)),
                _ => Err(Error::new(
                    v.span,
                    "a fade goes `in`, `out` or to an opacity from 0 to 1",
                )),
            },
            _ => Err(expected("a name and `in`, `out` or an opacity")),
        },
        "show" | "hide" => match rest {
            [] => Ok(Change::Opacity(if p.key.text == "show" {
                1.0
            } else {
                0.0
            })),
            [extra, ..] => Err(Error::new(extra.span, "too many values")),
        },
        "fill" => color(Part::Fill),
        "stroke" => color(Part::Stroke),
        "text-color" => color(Part::Text),
        other => Err(Error::new(
            p.key.span,
            format!(
                "unknown action `{other}`; a timeline takes scale, move, fade, show, hide, fill, \
                 stroke and text-color"
            ),
        )),
    }
}

/// The track that starts at `base` and takes each action's value in
/// turn. `actions` are in time order and do not overlap.
fn track<T: Lerp>(base: T, actions: &[Action], value: impl Fn(Change) -> T) -> Track<T> {
    let mut keys = Vec::with_capacity(2 * actions.len());
    let mut before = base;
    for a in actions {
        let after = value(a.change);
        keys.push(Keyframe {
            time: a.start,
            value: before,
            easing: Easing::Linear,
        });
        keys.push(Keyframe {
            time: a.end,
            value: after.clone(),
            easing: a.easing,
        });
        before = after;
    }
    Track::keyframes(keys)
}

/// The point the node scales about: the point of the box around what its
/// objects draw that its `anchor` names, the center by default.
fn pivot(
    node: &Node,
    objects: &[Object],
    indices: &[usize],
    fonts: &Fonts,
) -> Result<Point, Error> {
    let mut prims = Vec::new();
    for &i in indices {
        objects[i].kind.draw(0.0, &mut prims);
    }
    let (min, max) = primitive::bounds(fonts, &prims).unwrap_or((Point::ZERO, Point::ZERO));
    let mid = (min + max) * 0.5;
    let anchor = node.properties.iter().find(|p| p.key.text == "anchor");
    let Some(p) = anchor else {
        return Ok(mid);
    };
    let word = match p.values.as_slice() {
        [
            Value {
                kind: ValueKind::Word(w),
                ..
            },
        ] => w.as_str(),
        _ => "",
    };
    Ok(match word {
        "center" => mid,
        "top" => Point::new(mid.x, min.y),
        "bottom" => Point::new(mid.x, max.y),
        "left" => Point::new(min.x, mid.y),
        "right" => Point::new(max.x, mid.y),
        "top-left" => min,
        "top-right" => Point::new(max.x, min.y),
        "bottom-left" => Point::new(min.x, max.y),
        "bottom-right" => max,
        _ => {
            return Err(Error::new(
                p.span,
                "`anchor` is center, top, bottom, left, right, top-left, top-right, \
                 bottom-left or bottom-right",
            ));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lang::parse;
    use crate::scene::{Primitive, Scene, build};

    fn scene(timeline: &str) -> Result<Scene, (usize, usize, String)> {
        let src = format!(
            "canvas {{ size 400 300;
    box a {{ at 0 0; size 100 40; fill #ffffff; }}
    box b {{ at 200 0; size 100 40; anchor top-left; }}
    arrow link {{ from a right; to b left; }}
    row r {{ at 0 200; unit 20 20; cells \"x\" \"y\" \"z\"; }}
    text t {{ at 0 100; text \"hi\"; }}
    {timeline}
}}"
        );
        let program = parse(&src).map_err(|e| {
            let (l, c) = e.line_col(&src);
            (l, c, e.message)
        })?;
        build(&program, &[]).map_err(|e| {
            let (l, c) = e.line_col(&src);
            (l, c, e.message)
        })
    }

    fn object(s: &Scene, i: usize) -> &Object {
        &s.objects[i]
    }

    /// The rect of the first primitive object `i` draws at `t`.
    fn rect(s: &Scene, i: usize, t: f64) -> (Point, Point, f64) {
        let mut prims = Vec::new();
        s.draw_object(&s.objects[i], t, &mut prims);
        match prims[0] {
            Primitive::Rect {
                origin,
                size,
                stroke,
                ..
            } => (origin, size, stroke.map_or(0.0, |s| s.width)),
            ref other => panic!("expected a rect, got {other:?}"),
        }
    }

    fn line(s: &Scene, i: usize, t: f64) -> (Point, Point) {
        let mut prims = Vec::new();
        s.draw_object(&s.objects[i], t, &mut prims);
        match prims[0] {
            Primitive::Line { from, to, .. } => (from, to),
            ref other => panic!("expected a line, got {other:?}"),
        }
    }

    fn err(timeline: &str) -> String {
        scene(timeline).unwrap_err().2
    }

    #[test]
    fn a_scale_eases_in_out_by_default_and_linear_when_named() {
        let s = scene("timeline { 0s to 2s { scale a 3; } 2s to 4s ease linear { scale a 1; } }")
            .unwrap();
        let scale = &object(&s, 0).scale;
        assert_eq!(scale.at(0.0), 1.0);
        assert_eq!(scale.at(1.0), 2.0);
        assert!(scale.at(0.5) < 1.5);
        assert_eq!(scale.at(2.0), 3.0);
        assert_eq!(scale.at(3.0), 2.0);
        assert_eq!(scale.at(9.0), 1.0);
    }

    #[test]
    fn a_move_goes_to_an_offset_from_where_layout_put_the_node() {
        let s = scene("timeline { 1s to 2s ease linear { move a 40 -20; } 3s { move a 0 0; } }")
            .unwrap();
        let offset = &object(&s, 0).offset;
        assert_eq!(offset.at(1.0), Point::ZERO);
        assert_eq!(offset.at(1.5), Point::new(20.0, -10.0));
        assert_eq!(offset.at(2.9), Point::new(40.0, -20.0));
        assert_eq!(offset.at(3.0), Point::ZERO);
        assert_eq!(rect(&s, 0, 2.0).0, Point::new(40.0, -20.0));
    }

    #[test]
    fn fades_go_in_out_or_to_a_value_and_a_first_fade_in_starts_hidden() {
        let s = scene(
            "timeline { 0s to 1s ease linear { fade a 0.5; fade t in; } 1s to 2s ease linear { fade t out; } }",
        )
        .unwrap();
        let a = &object(&s, 0).opacity;
        assert_eq!(a.at(0.0), 1.0);
        assert_eq!(a.at(0.5), 0.75);
        assert_eq!(a.at(5.0), 0.5);
        let t = &object(&s, 4).opacity;
        assert_eq!(t.at(0.0), 0.0);
        assert_eq!(t.at(0.5), 0.5);
        assert_eq!(t.at(1.0), 1.0);
        assert_eq!(t.at(1.5), 0.5);
        assert_eq!(t.at(2.0), 0.0);
    }

    #[test]
    fn show_and_hide_switch_at_their_instant_and_a_node_shown_later_starts_hidden() {
        let s = scene("timeline { 1s { show t; } 2s { hide t; hide a; } }").unwrap();
        let t = &object(&s, 4).opacity;
        assert_eq!(
            [t.at(0.0), t.at(0.99), t.at(1.0), t.at(1.99), t.at(2.0)],
            [0.0, 0.0, 1.0, 1.0, 0.0]
        );
        let a = &object(&s, 0).opacity;
        assert_eq!([a.at(0.0), a.at(2.0)], [1.0, 0.0]);
        assert!(s.draw(0.5).len() < s.draw(1.5).len());
    }

    #[test]
    fn colors_change_over_a_span_or_at_an_instant_and_a_cell_can_be_named() {
        let s = scene(
            "timeline { 0s to 1s ease linear { fill a #000000; stroke link #ff0000; } 2s { fill r 1 #ff0000; text-color r 1 #ffffff; } }",
        )
        .unwrap();
        let mut prims = Vec::new();
        s.draw_object(&s.objects[0], 0.5, &mut prims);
        let Primitive::Rect { fill, .. } = prims[0] else {
            panic!("expected a rect")
        };
        assert_eq!(fill, Some(Color::rgb(0.5, 0.5, 0.5)));

        let cell_fill = |t: f64| {
            let mut prims = Vec::new();
            s.draw_object(&s.objects[3], t, &mut prims);
            prims
                .iter()
                .filter_map(|p| match p {
                    Primitive::Rect { fill, .. } => Some(*fill),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let red = Some(Color::rgb(1.0, 0.0, 0.0));
        assert_ne!(cell_fill(1.9)[1], red);
        assert_eq!(cell_fill(2.0)[1], red);
        assert_ne!(cell_fill(2.0)[0], red);
        let paints = &object(&s, 2).paints;
        assert_eq!(paints[0].color.at(1.0), Color::rgb(1.0, 0.0, 0.0));
    }

    #[test]
    fn a_node_scales_about_its_center_with_its_stroke() {
        let s = scene("timeline { 0s to 1s { scale a 2; } }").unwrap();
        let (origin, size, width) = rect(&s, 0, 1.0);
        assert_eq!(origin, Point::new(-50.0, -20.0));
        assert_eq!(size, Point::new(200.0, 80.0));
        assert_eq!(width, 4.0);
    }

    #[test]
    fn a_node_scales_about_the_corner_its_anchor_names() {
        let s = scene("timeline { 0s to 1s { scale b 2; } }").unwrap();
        let (origin, size, _) = rect(&s, 1, 1.0);
        assert_eq!(origin, Point::new(200.0, 0.0));
        assert_eq!(size, Point::new(200.0, 80.0));
    }

    #[test]
    fn a_connector_follows_a_moved_node_and_a_scaled_node() {
        let s = scene("timeline { 0s to 1s { move a 0 100; } 1s to 2s { scale b 2; } }").unwrap();
        assert_eq!(line(&s, 2, 0.0).0, Point::new(100.0, 20.0));
        assert_eq!(line(&s, 2, 1.0).0, Point::new(100.0, 120.0));
        // b doubles from its top-left, so the middle of its left side,
        // where the head's tip sits, moves from y 20 to y 40
        let mut prims = Vec::new();
        s.draw_object(&s.objects[2], 2.0, &mut prims);
        let Primitive::Polygon { points, .. } = &prims[1] else {
            panic!("expected the head")
        };
        assert_eq!(points[0], Point::new(200.0, 40.0));
    }

    #[test]
    fn duration_runs_to_the_last_change_and_a_canvas_duration_can_lengthen_it() {
        assert_eq!(scene("").unwrap().canvas.duration, 0.0);
        let s = scene("timeline { 1s to 2.5s { scale a 2; } 0.5s { hide t; } }").unwrap();
        assert_eq!(s.canvas.duration, 2.5);
        let s = scene("duration 4s; timeline { 1s to 2.5s { scale a 2; } }").unwrap();
        assert_eq!(s.canvas.duration, 4.0);
        assert_eq!(
            err("duration 2s; timeline { 1s to 2.5s { scale a 2; } }"),
            "the timeline runs to 2.5s, past this `duration`"
        );
    }

    #[test]
    fn mistakes_in_a_timeline_point_at_the_offending_text() {
        assert_eq!(
            scene("timeline {\n 1s { scale nobody 2; } }").unwrap_err(),
            (8, 13, "no node is named `nobody`".into())
        );
        assert_eq!(
            err("timeline { 2s to 1s { scale a 2; } }"),
            "this span ends at 1s, which is not after it starts at 2s"
        );
        assert_eq!(
            err("timeline { 0s to 1s { show a; } }"),
            "`show` happens at an instant, such as `2s { ... }`; `fade` takes a span"
        );
        assert_eq!(
            err("timeline { 0s to 2s { scale a 2; } 1s to 3s { scale a 1; } }"),
            "this changes `a` while another action from 0s to 2s does"
        );
        assert_eq!(
            err("timeline { 0s to 1s ease bouncy { scale a 2; } }"),
            "an easing is linear, in, out or in-out"
        );
        assert_eq!(
            err("timeline { 0s { fill r #ff0000; } }"),
            "`r` is a row, whose colors change by cell: `NAME N`"
        );
        assert_eq!(
            err("timeline { 0s { fill r 3 #ff0000; } }"),
            "row `r` has cells 0 to 2"
        );
        assert_eq!(
            err("timeline { 0s { fill t #ff0000; } }"),
            "`t` has no fill"
        );
        assert_eq!(
            err("timeline { 0s { spin a; } }"),
            "unknown action `spin`; a timeline takes scale, move, fade, show, hide, fill, stroke and text-color"
        );
        assert_eq!(
            err("group { timeline { } }"),
            "a `timeline` goes in the canvas"
        );
    }
}

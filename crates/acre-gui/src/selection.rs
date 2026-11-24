use acre::lang::{Node, Program, Span};
use acre::scene::Scene;
use acre::scene::edit;

use crate::snap::Bounds;

/// The nodes picked out by a click or a selection box, in source order.
/// Each is kept by its place in the source tree, so an edit that shifts
/// spans leaves it selected.
#[derive(Debug, Default, PartialEq)]
pub struct Selection {
    paths: Vec<Vec<usize>>,
    /// The nodes' spans in the program the shown scene was built from,
    /// one for each of `paths`.
    spans: Vec<Span>,
}

impl Selection {
    /// The selection of the nodes at `spans` in `program`, leaving out a
    /// span no node has.
    pub fn of(program: &Program, spans: &[Span]) -> Selection {
        let mut found: Vec<(Vec<usize>, Span)> = spans
            .iter()
            .filter(|s| s.start < s.end)
            .filter_map(|&s| Some((edit::node_path(program, s)?, s)))
            .collect();
        found.sort_by_key(|(_, s)| (s.start, std::cmp::Reverse(s.end)));
        found.dedup_by_key(|(_, s)| *s);
        let (paths, spans) = found.into_iter().unzip();
        Selection { paths, spans }
    }

    /// The same nodes in `program`, built after an edit, without those
    /// that are gone.
    pub fn follow(self, program: &Program) -> Selection {
        let (paths, spans) = self
            .paths
            .into_iter()
            .filter_map(|p| {
                let span = edit::node_span(program, &p)?;
                Some((p, span))
            })
            .unzip();
        Selection { paths, spans }
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    /// Whether the node at `node` is selected.
    pub fn contains(&self, node: Span) -> bool {
        self.spans.contains(&node)
    }
}

/// The nodes a press on an object of the node at `pressed` drags: every
/// node in `selected` when one of them is or holds `pressed`, and
/// `pressed` alone when none does.
pub fn dragged(selected: &[Span], pressed: Span) -> Vec<Span> {
    let holds = |s: &Span| s.start <= pressed.start && pressed.end <= s.end;
    if selected.iter().any(holds) {
        selected.to_vec()
    } else {
        vec![pressed]
    }
}

/// The nodes on the canvas of `program` whose objects in `scene` at time
/// `t` all lie inside `area`, in source order. A node whose objects all
/// lie inside stands for the nodes in it, so a box around a group
/// selects the group and a box around one of its children the child.
pub fn in_box(program: &Program, scene: &Scene, t: f64, area: Bounds) -> Vec<Span> {
    let drawn: Vec<(Span, Bounds)> = (0..scene.objects.len())
        .filter(|&i| scene.objects[i].source.start < scene.objects[i].source.end)
        .filter_map(|i| Some((scene.objects[i].source, scene.bounds(i, t)?)))
        .collect();
    let mut picked = Vec::new();
    if let Some(canvas) = program.nodes.first() {
        for node in &canvas.children {
            pick(node, &drawn, area, &mut picked);
        }
    }
    picked
}

/// Push onto `picked` `node` when it draws something in `drawn` and all
/// of that lies inside `area`, or else the nodes in it that do.
fn pick(node: &Node, drawn: &[(Span, Bounds)], area: Bounds, picked: &mut Vec<Span>) {
    let (min, max) = area;
    let mut any = false;
    let mut all_inside = true;
    for (source, (lo, hi)) in drawn {
        if source.start < node.span.start || node.span.end < source.end {
            continue;
        }
        any = true;
        all_inside &= min.x <= lo.x && min.y <= lo.y && hi.x <= max.x && hi.y <= max.y;
    }
    if !any {
        return;
    }
    if all_inside {
        picked.push(node.span);
        return;
    }
    for child in &node.children {
        pick(child, drawn, area, picked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre::lang::parse;
    use acre::scene::{self, Point};

    const SRC: &str = "canvas { size 300 200; box a { at 10 10; size 40 40; } \
                       box b { at 200 100; size 40 40; } }";

    /// What a click at `point` on the scene of `src` selects.
    fn click(src: &str, point: Point) -> Selection {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let spans: Vec<Span> = scene
            .object_at(point, 0.0)
            .map(|i| scene.objects[i].source)
            .into_iter()
            .collect();
        Selection::of(&program, &spans)
    }

    /// The nodes a box from `min` to `max` selects on the scene of `src`.
    fn boxed(src: &str, min: (f64, f64), max: (f64, f64)) -> Vec<Span> {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let area = (Point::new(min.0, min.1), Point::new(max.0, max.1));
        in_box(&program, &scene, 0.0, area)
    }

    /// The span of the first node in `src` that starts with `node` and
    /// holds no braces.
    fn span_of(src: &str, node: &str) -> Span {
        let start = src.find(node).unwrap();
        let end = start + src[start..].find('}').unwrap() + 1;
        Span { start, end }
    }

    #[test]
    fn a_click_selects_the_node_under_it_and_again_keeps_it() {
        let first = click(SRC, Point::new(20.0, 20.0));
        assert_eq!(first.spans(), [span_of(SRC, "box a")]);
        assert_eq!(click(SRC, Point::new(20.0, 20.0)), first);
    }

    #[test]
    fn a_click_on_empty_canvas_or_another_object_moves_the_selection() {
        assert_eq!(click(SRC, Point::new(150.0, 150.0)), Selection::default());
        let other = click(SRC, Point::new(210.0, 110.0));
        assert_eq!(other.spans(), [span_of(SRC, "box b")]);
    }

    #[test]
    fn the_selection_follows_its_nodes_when_an_edit_shifts_spans() {
        let program = parse(SRC).unwrap();
        let both = [span_of(SRC, "box b"), span_of(SRC, "box a")];
        let selected = Selection::of(&program, &both);
        let edited = SRC.replace("at 10 10", "at 10.5 10.5");
        let followed = selected.follow(&parse(&edited).unwrap());
        assert_eq!(
            followed.spans(),
            [span_of(&edited, "box a"), span_of(&edited, "box b")]
        );
    }

    #[test]
    fn a_node_that_is_gone_leaves_the_selection() {
        let program = parse(SRC).unwrap();
        let both = [span_of(SRC, "box a"), span_of(SRC, "box b")];
        let selected = Selection::of(&program, &both);
        let edited = "canvas { size 300 200; box a { at 10 10; size 40 40; } }";
        let followed = selected.follow(&parse(edited).unwrap());
        assert_eq!(followed.spans(), [span_of(edited, "box a")]);
    }

    #[test]
    fn a_box_selects_every_node_wholly_inside_it() {
        assert_eq!(boxed(SRC, (0.0, 0.0), (300.0, 200.0)).len(), 2);
        assert_eq!(
            boxed(SRC, (0.0, 0.0), (100.0, 100.0)),
            [span_of(SRC, "box a")]
        );

        // b reaches 240, past the box
        assert_eq!(
            boxed(SRC, (0.0, 0.0), (239.0, 200.0)),
            [span_of(SRC, "box a")]
        );
        assert!(boxed(SRC, (100.0, 0.0), (150.0, 200.0)).is_empty());
    }

    const GROUP: &str = "canvas { size 300 200; \
                         group g { at 100 100; box c { at 0 0; size 20 20; } \
                         box d { at 60 0; size 20 20; } } }";

    #[test]
    fn a_box_around_a_group_selects_the_group_and_around_a_child_the_child() {
        let whole = boxed(GROUP, (90.0, 90.0), (190.0, 130.0));
        assert_eq!(whole.len(), 1);
        assert!(GROUP[whole[0].start..].starts_with("group g"));
        assert_eq!(
            boxed(GROUP, (90.0, 90.0), (130.0, 130.0)),
            [span_of(GROUP, "box c")]
        );
    }

    #[test]
    fn a_press_on_a_selected_node_or_inside_one_drags_the_selection() {
        let a = span_of(SRC, "box a");
        let b = span_of(SRC, "box b");
        assert_eq!(dragged(&[a, b], b), [a, b]);
        assert_eq!(dragged(&[a], b), [b]);
        assert_eq!(dragged(&[], b), [b]);
        let program = parse(GROUP).unwrap();
        let group = program.nodes[0].children[0].span;
        let c = span_of(GROUP, "box c");
        assert_eq!(dragged(&[group], c), [group]);
    }
}

use acre::lang::{Node, Program, Span};
use acre::scene::edit;
use acre::scene::{Point, Scene};

use crate::snap::Bounds;

/// The nodes picked out by a click, a selection box or the editor's
/// cursor, in source order. Each is kept by its place in the source
/// tree, so an edit that shifts spans leaves it selected.
#[derive(Debug, Default, PartialEq)]
pub struct Selection {
    paths: Vec<Vec<usize>>,
    /// The nodes' spans in the program the shown scene was built from,
    /// one for each of `paths`.
    spans: Vec<Span>,
    /// Whether a click, a box or a new node in the viewer made the
    /// selection, rather than the editor's cursor.
    made_in_viewer: bool,
}

impl Selection {
    /// The selection made in the viewer of the nodes at `spans` in
    /// `program`, leaving out a span no node has.
    pub fn of(program: &Program, spans: &[Span]) -> Selection {
        let mut found: Vec<(Vec<usize>, Span)> = spans
            .iter()
            .filter(|s| s.start < s.end)
            .filter_map(|&s| Some((edit::node_path(program, s)?, s)))
            .collect();
        found.sort_by_key(|(_, s)| (s.start, std::cmp::Reverse(s.end)));
        found.dedup_by_key(|(_, s)| *s);
        let (paths, spans) = found.into_iter().unzip();
        Selection {
            paths,
            spans,
            made_in_viewer: true,
        }
    }

    /// The selection made in the viewer of the nodes at `paths` in
    /// `program`, leaving out a path no node has.
    pub fn at(program: &Program, paths: Vec<Vec<usize>>) -> Selection {
        Selection {
            paths,
            spans: Vec::new(),
            made_in_viewer: true,
        }
        .follow(program)
    }

    /// The selection a click at canvas `point` on `scene` at time `t`
    /// makes: the node that drew the object there, or none on empty
    /// canvas.
    pub fn clicked(program: &Program, scene: &Scene, t: f64, point: Point) -> Selection {
        match scene.object_at(point, t) {
            Some(i) => Selection::of(program, &[scene.objects[i].source]),
            None => Selection::default(),
        }
    }

    /// The selection the editor's cursor at byte `offset` of `source`
    /// makes, where `program` was parsed from `source`: the innermost
    /// node under the canvas that holds the offset and drew an object
    /// whose source is in `drawn`, else none.
    pub fn at_cursor(program: &Program, source: &str, drawn: &[Span], offset: usize) -> Selection {
        let offset = on_line(source, offset);
        let spans: Vec<Span> = innermost(program, drawn, offset).into_iter().collect();
        Selection {
            made_in_viewer: false,
            ..Selection::of(program, &spans)
        }
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
        Selection {
            paths,
            spans,
            made_in_viewer: self.made_in_viewer,
        }
    }

    pub fn spans(&self) -> &[Span] {
        &self.spans
    }

    pub fn paths(&self) -> &[Vec<usize>] {
        &self.paths
    }

    /// Whether the node at `node` is selected.
    pub fn contains(&self, node: Span) -> bool {
        self.spans.contains(&node)
    }

    pub fn made_in_viewer(&self) -> bool {
        self.made_in_viewer
    }
}

/// Whether the node at `span` holds byte `offset`: from the first byte
/// of its kind up to its closing brace, both included.
fn holds(span: Span, offset: usize) -> bool {
    span.start <= offset && offset < span.end
}

/// The offset that byte `offset` of `source` stands for on its line: the
/// line's first non-blank byte when only blanks come before it, its last
/// when only blanks come after it, and `offset` itself on a blank line.
fn on_line(source: &str, offset: usize) -> usize {
    let bytes = source.as_bytes();
    let offset = offset.min(bytes.len());
    let start = bytes[..offset]
        .iter()
        .rposition(|&b| b == b'\n')
        .map_or(0, |i| i + 1);
    let end = bytes[offset..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |i| offset + i);
    let line = &bytes[start..end];
    let blank = |b: &u8| matches!(b, b' ' | b'\t' | b'\r');
    let (Some(first), Some(last)) = (
        line.iter().position(|b| !blank(b)),
        line.iter().rposition(|b| !blank(b)),
    ) else {
        return offset;
    };
    (offset - start).clamp(first, last) + start
}

/// The innermost node under the canvas of `program` that holds byte
/// `offset` and holds the source of an object in `drawn`.
fn innermost(program: &Program, drawn: &[Span], offset: usize) -> Option<Span> {
    let draws = |node: Span| {
        drawn
            .iter()
            .any(|s| s.start < s.end && node.start <= s.start && s.end <= node.end)
    };
    let mut found = None;
    let mut nodes = &program.nodes.first()?.children[..];
    while let Some(node) = nodes.iter().find(|n| holds(n.span, offset)) {
        if draws(node.span) {
            found = Some(node.span);
        }
        nodes = &node.children;
    }
    found
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
    use acre::scene;

    const SRC: &str = "canvas { size 300 200; box a { at 10 10; size 40 40; } \
                       box b { at 200 100; size 40 40; } }";

    /// What a click at `point` on the scene of `src` selects.
    fn click(src: &str, point: Point) -> Selection {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        Selection::clicked(&program, &scene, 0.0, point)
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
    fn the_nodes_at_paths_are_selected_and_a_path_no_node_has_is_not() {
        let program = parse(SRC).unwrap();
        let selected = Selection::at(&program, vec![vec![0, 1], vec![0, 5]]);
        assert_eq!(selected.spans(), [span_of(SRC, "box b")]);
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

    /// The selection the editor's cursor at `offset` in `src` makes.
    fn cursor(src: &str, offset: usize) -> Selection {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let drawn: Vec<Span> = scene.objects.iter().map(|o| o.source).collect();
        Selection::at_cursor(&program, src, &drawn, offset)
    }

    #[test]
    fn the_cursor_selects_the_innermost_node_that_holds_it() {
        let program = parse(GROUP).unwrap();
        let group = program.nodes[0].children[0].span;
        let c = span_of(GROUP, "box c");
        let in_c = GROUP.find("size 20").unwrap();
        assert_eq!(cursor(GROUP, in_c).spans(), [c]);
        let in_group = GROUP.find("at 100").unwrap();
        assert_eq!(cursor(GROUP, in_group).spans(), [group]);
    }

    #[test]
    fn a_node_holds_the_cursor_from_its_kind_to_its_closing_brace() {
        let a = span_of(SRC, "box a");
        let on = |offset| cursor(SRC, offset);
        assert_eq!(on(a.start).spans(), [a]);
        assert_eq!(on(a.end - 1).spans(), [a]);

        // the space after the brace, between a and b
        assert!(on(a.end).spans().is_empty());
        assert!(on(a.start - 1).spans().is_empty());
    }

    #[test]
    fn the_cursor_on_the_canvas_or_in_a_node_that_draws_nothing_selects_nothing() {
        let on = |offset| cursor(SRC, offset);
        assert!(on(SRC.find("size 300").unwrap()).spans().is_empty());
        assert!(on(0).spans().is_empty());

        let styled = "canvas { size 300 200; style s { fill red; } \
                      box a { at 10 10; size 40 40; } }";
        let in_style = styled.find("fill").unwrap();
        assert!(cursor(styled, in_style).spans().is_empty());
    }

    // the group's line is indented with tabs, and the lines that
    // end in two spaces test the blanks after a node's last byte
    const NESTED: &str = concat!(
        "canvas {\n",
        "  size 300 200;\n",
        "  stack s {\n",
        "    at 10 10;\n",
        "\n",
        "    box a { size 20 20; }\n",
        "\t\tgroup g {\n",
        "      at 0 0;\n",
        "      box c { size 10 10; }  \n",
        "    }  \n",
        "  }\n",
        "}\n",
    );

    /// The byte at column `col` of line `line` of `NESTED`, both from 0.
    fn at(line: usize, col: usize) -> usize {
        NESTED
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>()
            + col
    }

    #[test]
    fn the_cursor_in_a_lines_indentation_selects_the_node_the_line_starts() {
        let program = parse(NESTED).unwrap();
        let stack = &program.nodes[0].children[0];
        let (a, group) = (stack.children[0].span, &stack.children[1]);
        let c = group.children[0].span;
        let on = |line, col| cursor(NESTED, at(line, col)).spans().to_vec();
        assert_eq!(on(5, 0), [a]);
        assert_eq!(on(5, 3), [a]);
        assert_eq!(on(6, 1), [group.span]);
        assert_eq!(on(8, 2), [c]);

        // the closing brace's line belongs to the node it closes
        assert_eq!(on(9, 0), [group.span]);
        assert_eq!(on(10, 1), [stack.span]);
    }

    #[test]
    fn the_cursor_on_a_containers_own_lines_or_a_blank_line_selects_it() {
        let program = parse(NESTED).unwrap();
        let stack = &program.nodes[0].children[0];
        let group = stack.children[1].span;
        let on = |line, col| cursor(NESTED, at(line, col)).spans().to_vec();
        assert_eq!(on(2, 0), [stack.span]);
        assert_eq!(on(2, 10), [stack.span]);
        assert_eq!(on(3, 2), [stack.span]);
        assert_eq!(on(3, 6), [stack.span]);
        assert_eq!(on(4, 0), [stack.span]);
        assert_eq!(on(7, 0), [group]);
        assert!(on(1, 0).is_empty());
        assert!(on(12, 0).is_empty());
    }

    #[test]
    fn the_cursor_after_a_lines_last_byte_selects_the_node_that_byte_is_in() {
        let program = parse(NESTED).unwrap();
        let group = &program.nodes[0].children[0].children[1];
        let c = group.children[0].span;
        let on = |line, col| cursor(NESTED, at(line, col)).spans().to_vec();
        assert_eq!(on(8, 28), [c]);
        assert_eq!(on(8, 29), [c]);
        assert_eq!(on(9, 6), [group.span]);
        assert_eq!(on(9, 7), [group.span]);
    }

    #[test]
    fn the_cursor_moving_the_selection_makes_it_the_editors() {
        let program = parse(SRC).unwrap();
        let b = span_of(SRC, "box b");
        let moved = cursor(SRC, b.start + 1);
        assert_eq!(moved.spans(), [b]);
        assert!(!moved.made_in_viewer());

        // a click on the node the cursor picked hands it back to the viewer
        assert!(Selection::of(&program, moved.spans()).made_in_viewer());
        let left = cursor(SRC, span_of(SRC, "box a").end);
        assert!(left.spans().is_empty());
        assert!(!left.made_in_viewer());
    }

    #[test]
    fn the_editors_selection_stays_the_editors_after_an_edit() {
        let a = span_of(SRC, "box a");
        let picked = cursor(SRC, a.start);
        let edited = SRC.replace("at 10 10", "at 10.5 10.5");
        let followed = picked.follow(&parse(&edited).unwrap());
        assert_eq!(followed.spans(), [span_of(&edited, "box a")]);
        assert!(!followed.made_in_viewer());
    }

    #[test]
    fn a_click_on_empty_canvas_clears_a_selection_the_cursor_made() {
        let a = span_of(SRC, "box a");
        assert_eq!(cursor(SRC, a.start).spans(), [a]);
        assert_eq!(click(SRC, Point::new(150.0, 150.0)), Selection::default());
        assert_eq!(cursor(SRC, a.start).spans(), [a]);
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

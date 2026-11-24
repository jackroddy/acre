use acre::lang::{Program, Span};
use acre::scene::edit;

/// The node a click picked out. It is kept by its place in the source
/// tree, so an edit that shifts spans leaves it selected.
#[derive(Debug, PartialEq)]
pub struct Selection {
    path: Vec<usize>,
    /// The node's span in the program the shown scene was built from.
    span: Span,
}

impl Selection {
    /// The selection of the node at `span` in `program`, or `None` when
    /// no node has that span.
    pub fn of(program: &Program, span: Span) -> Option<Selection> {
        if span.start >= span.end {
            return None;
        }
        let path = edit::node_path(program, span)?;
        Some(Selection { path, span })
    }

    /// The same node in `program`, built after an edit, or `None` when
    /// it is gone.
    pub fn follow(self, program: &Program) -> Option<Selection> {
        let span = edit::node_span(program, &self.path)?;
        Some(Selection {
            path: self.path,
            span,
        })
    }

    pub fn span(&self) -> Span {
        self.span
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
    fn click(src: &str, point: Point) -> Option<Selection> {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let index = scene.object_at(point, 0.0)?;
        Selection::of(&program, scene.objects[index].source)
    }

    fn span_of(src: &str, node: &str) -> Span {
        let start = src.find(node).unwrap();
        let end = start + src[start..].find('}').unwrap() + 1;
        Span { start, end }
    }

    #[test]
    fn a_click_selects_the_node_under_it_and_again_keeps_it() {
        let first = click(SRC, Point::new(20.0, 20.0)).unwrap();
        assert_eq!(first.span(), span_of(SRC, "box a"));
        assert_eq!(click(SRC, Point::new(20.0, 20.0)), Some(first));
    }

    #[test]
    fn a_click_on_empty_canvas_or_another_object_moves_the_selection() {
        assert_eq!(click(SRC, Point::new(150.0, 150.0)), None);
        let other = click(SRC, Point::new(210.0, 110.0)).unwrap();
        assert_eq!(other.span(), span_of(SRC, "box b"));
    }

    #[test]
    fn the_selection_follows_its_node_when_an_edit_shifts_spans() {
        let selected = click(SRC, Point::new(210.0, 110.0)).unwrap();
        let edited = SRC.replace("at 10 10", "at 10.5 10.5");
        let followed = selected.follow(&parse(&edited).unwrap()).unwrap();
        assert_eq!(followed.span(), span_of(&edited, "box b"));
    }

    #[test]
    fn the_selection_ends_when_its_node_is_gone() {
        let selected = click(SRC, Point::new(210.0, 110.0)).unwrap();
        let edited = "canvas { size 300 200; box a { at 10 10; size 40 40; } }";
        assert_eq!(selected.follow(&parse(edited).unwrap()), None);
    }
}

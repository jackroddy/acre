use acre::lang::Span;
use acre::scene::Point;
use acre::scene::edit::Siblings;

use crate::handle::{Grip, Target};
use crate::reorder::moved_index;
use crate::screen;

/// How far, in pixels, the pointer goes from where it was
/// pressed before the press is a drag and not a click.
const THRESHOLD: f32 = 4.0;

/// A primary press in the render pane, from press to release.
pub struct Drag {
    /// The nodes the drag moves, or none on empty canvas, where the drag
    /// draws a selection box.
    nodes: Vec<Span>,
    /// Whether the drag draws a new node, and so neither moves nor
    /// selects.
    create: bool,
    /// The handle the drag pulls and what it sits on, when the press was
    /// on one.
    grip: Option<(Target, Grip)>,
    /// The children of the stack or grid that holds the one node the
    /// drag moves, and the index among them of the child the node goes
    /// before, or their count for after the last.
    reorder: Option<(Siblings, usize)>,
    /// Whether the press only clicks, however far the pointer goes.
    still: bool,
    /// Where the press was, on screen.
    from: screen::Point,
    /// Where the pointer is, on screen.
    to: screen::Point,
    /// How far the pointer has moved, in canvas pixels, once it has gone
    /// past the threshold.
    by: Option<Point>,
}

/// What a press comes to when the pointer is let go.
#[derive(Debug, PartialEq)]
pub enum Release {
    /// The pointer stayed within the threshold of where it was pressed.
    Click(screen::Point),
    /// Move `nodes` by `by` canvas pixels.
    Move { nodes: Vec<Span>, by: Point },
    /// Select what lies in the box between two corners on screen.
    Select(screen::Point, screen::Point),
    /// Draw a new node from one point on screen to another.
    Create(screen::Point, screen::Point),
    /// Drag the handle `grip` on `target` by `by` canvas pixels.
    Grip {
        target: Target,
        grip: Grip,
        by: Point,
    },
    /// Move `node` before its sibling `before`, or after the last when
    /// `None`, to end up at `index` among them.
    Reorder {
        node: Span,
        before: Option<Span>,
        index: usize,
    },
}

impl Drag {
    pub fn new(nodes: Vec<Span>, from: screen::Point) -> Self {
        Drag {
            nodes,
            create: false,
            grip: None,
            reorder: None,
            still: false,
            from,
            to: from,
            by: None,
        }
    }

    /// A press that draws a new node.
    pub fn create(from: screen::Point) -> Self {
        Drag {
            create: true,
            ..Drag::new(Vec::new(), from)
        }
    }

    /// A press on the handle `grip` of `target`.
    pub fn grip(target: Target, grip: Grip, from: screen::Point) -> Self {
        Drag {
            grip: Some((target, grip)),
            ..Drag::new(Vec::new(), from)
        }
    }

    /// A press that comes to a click wherever it is let go, and draws
    /// nothing meanwhile.
    pub fn click(from: screen::Point) -> Self {
        Drag {
            still: true,
            ..Drag::new(Vec::new(), from)
        }
    }

    /// Make the drag of one child of a stack or grid, whose siblings
    /// are `siblings`, reorder it among them in place of moving it.
    pub fn reorder(&mut self, siblings: Siblings) {
        let [node] = self.nodes[..] else { return };
        if let Some(index) = siblings.children.iter().position(|&c| c == node) {
            self.reorder = Some((siblings, index));
        }
    }

    /// The node being reordered, its siblings and how far it has been
    /// dragged, once the pointer has gone past the threshold.
    pub fn reordering(&self) -> Option<(Span, &Siblings, Point)> {
        let (siblings, _) = self.reorder.as_ref()?;
        Some((self.nodes[0], siblings, self.by?))
    }

    /// Put the node being reordered before the sibling at `slot`, or
    /// after the last when `slot` is their count.
    pub fn aim(&mut self, slot: usize) {
        if let Some((_, aimed)) = &mut self.reorder {
            *aimed = slot;
        }
    }

    /// Follow the pointer to `to`, at `zoom` pixels on screen per canvas
    /// pixel.
    pub fn follow(&mut self, to: screen::Point, zoom: f32) {
        self.to = to;
        if self.by.is_none() && to.distance(self.from) <= THRESHOLD {
            return;
        }
        let by = (to - self.from) / zoom;
        self.by = Some(Point::new(by.x as f64, by.y as f64));
    }

    /// Put the nodes at `by` canvas pixels from where they were, in place
    /// of the pointer's offset, until the pointer moves again.
    pub fn place(&mut self, by: Point) {
        if self.by.is_some() {
            self.by = Some(by);
        }
    }

    /// The nodes being moved and how far, once the pointer has gone past
    /// the threshold.
    pub fn moving(&self) -> Option<(&[Span], Point)> {
        let by = self.by?;
        let moves = !self.nodes.is_empty() && self.reorder.is_none();
        moves.then_some((self.nodes.as_slice(), by))
    }

    /// The corners on screen of the selection box, once the pointer has
    /// gone past the threshold on empty canvas.
    pub fn boxed(&self) -> Option<(screen::Point, screen::Point)> {
        let selecting = self.nodes.is_empty() && !self.create && self.grip.is_none() && !self.still;
        (selecting && self.by.is_some()).then_some((self.from, self.to))
    }

    /// Where on screen a drag that draws a new node runs from and to,
    /// once the pointer has gone past the threshold.
    pub fn creating(&self) -> Option<(screen::Point, screen::Point)> {
        (self.create && self.by.is_some()).then_some((self.from, self.to))
    }

    /// The handle being dragged, what it sits on, and how far it has
    /// moved, once the pointer has gone past the threshold.
    pub fn gripping(&self) -> Option<(&Target, Grip, Point)> {
        let (target, grip) = self.grip.as_ref()?;
        Some((target, *grip, self.by?))
    }

    pub fn release(self) -> Release {
        let Some(by) = self.by.filter(|_| !self.still) else {
            return Release::Click(self.from);
        };
        if let Some((target, grip)) = self.grip {
            return Release::Grip { target, grip, by };
        }
        if let Some((siblings, slot)) = self.reorder {
            let from = siblings
                .children
                .iter()
                .position(|&c| c == self.nodes[0])
                .expect("`reorder` takes the node's siblings");
            return Release::Reorder {
                node: self.nodes[0],
                before: siblings.children.get(slot).copied(),
                index: moved_index(from, slot),
            };
        }
        if self.create {
            Release::Create(self.from, self.to)
        } else if self.nodes.is_empty() {
            Release::Select(self.from, self.to)
        } else {
            Release::Move {
                nodes: self.nodes,
                by,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre::scene::edit::Layout;

    fn span() -> Span {
        Span { start: 10, end: 20 }
    }

    #[test]
    fn a_press_that_stays_put_is_a_click() {
        let mut drag = Drag::new(vec![span()], screen::Point::new(100.0, 100.0));
        drag.follow(screen::Point::new(103.0, 102.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(
            drag.release(),
            Release::Click(screen::Point::new(100.0, 100.0))
        );
    }

    #[test]
    fn a_drag_on_an_object_moves_its_nodes_in_canvas_pixels() {
        let other = Span { start: 30, end: 40 };
        let mut drag = Drag::new(vec![span(), other], screen::Point::new(100.0, 100.0));
        drag.follow(screen::Point::new(120.0, 90.0), 2.0);
        let by = Point::new(10.0, -5.0);
        assert_eq!(drag.moving(), Some((&[span(), other][..], by)));
        assert_eq!(drag.boxed(), None);
        assert_eq!(
            drag.release(),
            Release::Move {
                nodes: vec![span(), other],
                by,
            }
        );
    }

    #[test]
    fn a_drag_back_inside_the_threshold_still_moves() {
        let mut drag = Drag::new(vec![span()], screen::Point::new(0.0, 0.0));
        drag.follow(screen::Point::new(30.0, 0.0), 1.0);
        drag.follow(screen::Point::new(2.0, 0.0), 1.0);
        assert_eq!(drag.moving(), Some((&[span()][..], Point::new(2.0, 0.0))));
    }

    #[test]
    fn a_placed_drag_lets_go_where_it_was_placed() {
        let mut drag = Drag::new(vec![span()], screen::Point::new(0.0, 0.0));
        drag.place(Point::new(9.0, 9.0));
        assert_eq!(drag.moving(), None);
        drag.follow(screen::Point::new(30.0, 0.0), 1.0);
        drag.place(Point::new(32.5, 0.0));
        assert_eq!(
            drag.release(),
            Release::Move {
                nodes: vec![span()],
                by: Point::new(32.5, 0.0),
            }
        );
    }

    #[test]
    fn a_drag_on_empty_canvas_draws_a_selection_box() {
        let mut drag = Drag::new(Vec::new(), screen::Point::new(10.0, 10.0));
        drag.follow(screen::Point::new(12.0, 12.0), 1.0);
        assert_eq!(drag.boxed(), None);
        drag.follow(screen::Point::new(50.0, 5.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(
            drag.boxed(),
            Some((
                screen::Point::new(10.0, 10.0),
                screen::Point::new(50.0, 5.0)
            ))
        );
        assert_eq!(
            drag.release(),
            Release::Select(
                screen::Point::new(10.0, 10.0),
                screen::Point::new(50.0, 5.0)
            )
        );
    }

    #[test]
    fn a_drag_that_creates_neither_selects_nor_moves() {
        let mut drag = Drag::create(screen::Point::new(10.0, 10.0));
        drag.follow(screen::Point::new(12.0, 12.0), 1.0);
        assert_eq!(drag.creating(), None);
        drag.follow(screen::Point::new(60.0, 40.0), 2.0);
        assert_eq!(drag.boxed(), None);
        assert_eq!(drag.moving(), None);
        assert_eq!(
            drag.creating(),
            Some((
                screen::Point::new(10.0, 10.0),
                screen::Point::new(60.0, 40.0)
            ))
        );
        assert_eq!(
            drag.release(),
            Release::Create(
                screen::Point::new(10.0, 10.0),
                screen::Point::new(60.0, 40.0)
            )
        );
        assert_eq!(
            Drag::create(screen::Point::new(5.0, 5.0)).release(),
            Release::Click(screen::Point::new(5.0, 5.0))
        );
    }

    #[test]
    fn a_drag_on_a_handle_pulls_it_and_neither_selects_nor_moves() {
        let target = Target::Path {
            node: span(),
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)],
            open: true,
        };
        let grip = Grip::Vertex(1);
        let mut drag = Drag::grip(target.clone(), grip, screen::Point::new(10.0, 10.0));
        drag.follow(screen::Point::new(30.0, 10.0), 2.0);
        assert_eq!(drag.boxed(), None);
        assert_eq!(drag.moving(), None);
        let by = Point::new(10.0, 0.0);
        assert_eq!(drag.gripping(), Some((&target, grip, by)));
        drag.place(Point::new(11.0, 0.0));
        assert_eq!(
            drag.release(),
            Release::Grip {
                target,
                grip,
                by: Point::new(11.0, 0.0),
            }
        );
    }

    #[test]
    fn a_press_that_only_clicks_draws_nothing_and_clicks_where_pressed() {
        let mut drag = Drag::click(screen::Point::new(10.0, 10.0));
        drag.follow(screen::Point::new(80.0, 40.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(drag.boxed(), None);
        assert_eq!(drag.creating(), None);
        assert_eq!(drag.gripping(), None);
        assert_eq!(
            drag.release(),
            Release::Click(screen::Point::new(10.0, 10.0))
        );
    }

    #[test]
    fn a_drag_on_a_child_of_a_stack_reorders_it_and_does_not_move_it() {
        let a = Span { start: 0, end: 5 };
        let siblings = Siblings {
            layout: Layout::Down,
            children: vec![a, span(), Span { start: 30, end: 40 }],
        };
        let mut drag = Drag::new(vec![span()], screen::Point::new(0.0, 0.0));
        drag.reorder(siblings.clone());
        assert_eq!(drag.reordering(), None);
        drag.follow(screen::Point::new(0.0, 30.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(
            drag.reordering(),
            Some((span(), &siblings, Point::new(0.0, 30.0)))
        );
        drag.aim(0);
        assert_eq!(
            drag.release(),
            Release::Reorder {
                node: span(),
                before: Some(a),
                index: 0,
            }
        );
    }

    #[test]
    fn a_reorder_to_the_end_goes_before_nothing_and_several_nodes_move() {
        let siblings = Siblings {
            layout: Layout::Down,
            children: vec![span(), Span { start: 30, end: 40 }],
        };
        let mut drag = Drag::new(vec![span()], screen::Point::new(0.0, 0.0));
        drag.reorder(siblings.clone());
        drag.follow(screen::Point::new(0.0, 30.0), 1.0);
        drag.aim(2);
        assert_eq!(
            drag.release(),
            Release::Reorder {
                node: span(),
                before: None,
                index: 1,
            }
        );

        let other = Span { start: 30, end: 40 };
        let mut drag = Drag::new(vec![span(), other], screen::Point::new(0.0, 0.0));
        drag.reorder(siblings);
        drag.follow(screen::Point::new(0.0, 30.0), 1.0);
        assert!(drag.moving().is_some());
    }
}

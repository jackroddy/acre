use acre::lang::Span;
use acre::scene::Point;
use eframe::egui::Pos2;

/// How far, in screen points, the pointer goes from where it was
/// pressed before the press is a drag and not a click.
const THRESHOLD: f32 = 4.0;

/// A primary press in the render pane, from press to release.
pub struct Drag {
    /// The nodes the drag moves, or none on empty canvas, where the drag
    /// draws a selection box.
    nodes: Vec<Span>,
    /// Where the press was, on screen.
    from: Pos2,
    /// Where the pointer is, on screen.
    to: Pos2,
    /// How far the pointer has moved, in canvas pixels, once it has gone
    /// past the threshold.
    by: Option<Point>,
}

/// What a press comes to when the pointer is let go.
#[derive(Debug, PartialEq)]
pub enum Release {
    /// The pointer stayed within the threshold of where it was pressed.
    Click(Pos2),
    /// Move `nodes` by `by` canvas pixels.
    Move { nodes: Vec<Span>, by: Point },
    /// Select what lies in the box between two corners on screen.
    Select(Pos2, Pos2),
}

impl Drag {
    pub fn new(nodes: Vec<Span>, from: Pos2) -> Self {
        Drag {
            nodes,
            from,
            to: from,
            by: None,
        }
    }

    /// Follow the pointer to `to`, at `zoom` screen points per canvas
    /// pixel.
    pub fn follow(&mut self, to: Pos2, zoom: f32) {
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
        (!self.nodes.is_empty()).then_some((self.nodes.as_slice(), by))
    }

    /// The corners on screen of the selection box, once the pointer has
    /// gone past the threshold on empty canvas.
    pub fn boxed(&self) -> Option<(Pos2, Pos2)> {
        (self.nodes.is_empty() && self.by.is_some()).then_some((self.from, self.to))
    }

    pub fn release(self) -> Release {
        match self.by {
            None => Release::Click(self.from),
            Some(_) if self.nodes.is_empty() => Release::Select(self.from, self.to),
            Some(by) => Release::Move {
                nodes: self.nodes,
                by,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    fn span() -> Span {
        Span { start: 10, end: 20 }
    }

    #[test]
    fn a_press_that_stays_put_is_a_click() {
        let mut drag = Drag::new(vec![span()], pos2(100.0, 100.0));
        drag.follow(pos2(103.0, 102.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(drag.release(), Release::Click(pos2(100.0, 100.0)));
    }

    #[test]
    fn a_drag_on_an_object_moves_its_nodes_in_canvas_pixels() {
        let other = Span { start: 30, end: 40 };
        let mut drag = Drag::new(vec![span(), other], pos2(100.0, 100.0));
        drag.follow(pos2(120.0, 90.0), 2.0);
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
        let mut drag = Drag::new(vec![span()], pos2(0.0, 0.0));
        drag.follow(pos2(30.0, 0.0), 1.0);
        drag.follow(pos2(2.0, 0.0), 1.0);
        assert_eq!(drag.moving(), Some((&[span()][..], Point::new(2.0, 0.0))));
    }

    #[test]
    fn a_placed_drag_lets_go_where_it_was_placed() {
        let mut drag = Drag::new(vec![span()], pos2(0.0, 0.0));
        drag.place(Point::new(9.0, 9.0));
        assert_eq!(drag.moving(), None);
        drag.follow(pos2(30.0, 0.0), 1.0);
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
        let mut drag = Drag::new(Vec::new(), pos2(10.0, 10.0));
        drag.follow(pos2(12.0, 12.0), 1.0);
        assert_eq!(drag.boxed(), None);
        drag.follow(pos2(50.0, 5.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(drag.boxed(), Some((pos2(10.0, 10.0), pos2(50.0, 5.0))));
        assert_eq!(
            drag.release(),
            Release::Select(pos2(10.0, 10.0), pos2(50.0, 5.0))
        );
    }
}

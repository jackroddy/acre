use acre::lang::Span;
use acre::scene::Point;
use eframe::egui::Pos2;

/// How far, in screen points, the pointer goes from where it was
/// pressed before the press is a drag and not a click.
const THRESHOLD: f32 = 4.0;

/// A primary press in the render pane, from press to release.
pub struct Drag {
    /// The node that drew the object pressed on, or `None` on empty
    /// canvas, where the drag pans.
    node: Option<Span>,
    /// Where the press was, on screen.
    from: Pos2,
    /// How far the pointer has moved, in canvas pixels, once it has gone
    /// past the threshold.
    by: Option<Point>,
}

/// What a press comes to when the pointer is let go.
#[derive(Debug, PartialEq)]
pub enum Release {
    /// The pointer stayed within the threshold of where it was pressed.
    Click(Pos2),
    /// Move `node` by `by` canvas pixels.
    Move { node: Span, by: Point },
    /// A pan, already made while the pointer moved.
    Pan,
}

impl Drag {
    pub fn new(node: Option<Span>, from: Pos2) -> Self {
        Drag {
            node,
            from,
            by: None,
        }
    }

    /// Whether the drag pans the view rather than moving an object.
    pub fn pans(&self) -> bool {
        self.node.is_none()
    }

    /// Follow the pointer to `to`, at `zoom` screen points per canvas
    /// pixel.
    pub fn follow(&mut self, to: Pos2, zoom: f32) {
        if self.by.is_none() && to.distance(self.from) <= THRESHOLD {
            return;
        }
        let by = (to - self.from) / zoom;
        self.by = Some(Point::new(by.x as f64, by.y as f64));
    }

    /// The node being moved and how far, once the pointer has gone past
    /// the threshold.
    pub fn moving(&self) -> Option<(Span, Point)> {
        Some((self.node?, self.by?))
    }

    pub fn release(self) -> Release {
        match (self.node, self.by) {
            (_, None) => Release::Click(self.from),
            (Some(node), Some(by)) => Release::Move { node, by },
            (None, Some(_)) => Release::Pan,
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
        let mut drag = Drag::new(Some(span()), pos2(100.0, 100.0));
        drag.follow(pos2(103.0, 102.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(drag.release(), Release::Click(pos2(100.0, 100.0)));
    }

    #[test]
    fn a_drag_on_an_object_moves_it_in_canvas_pixels() {
        let mut drag = Drag::new(Some(span()), pos2(100.0, 100.0));
        drag.follow(pos2(120.0, 90.0), 2.0);
        assert_eq!(drag.moving(), Some((span(), Point::new(10.0, -5.0))));
        assert_eq!(
            drag.release(),
            Release::Move {
                node: span(),
                by: Point::new(10.0, -5.0),
            }
        );
    }

    #[test]
    fn a_drag_back_inside_the_threshold_still_moves() {
        let mut drag = Drag::new(Some(span()), pos2(0.0, 0.0));
        drag.follow(pos2(30.0, 0.0), 1.0);
        drag.follow(pos2(2.0, 0.0), 1.0);
        assert_eq!(drag.moving(), Some((span(), Point::new(2.0, 0.0))));
    }

    #[test]
    fn a_drag_on_empty_canvas_pans() {
        let mut drag = Drag::new(None, pos2(0.0, 0.0));
        assert!(drag.pans());
        drag.follow(pos2(50.0, 0.0), 1.0);
        assert_eq!(drag.moving(), None);
        assert_eq!(drag.release(), Release::Pan);
    }
}

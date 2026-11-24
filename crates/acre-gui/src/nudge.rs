/// The arrow-key moves of the selection not yet written. While an edit
/// is with the editor, the moves add up here and go out as one edit
/// once the editor sends its text back.
#[derive(Debug, Default)]
pub struct Nudge {
    dx: f64,
    dy: f64,
    /// Whether an edit is with the editor.
    //
    // an edit carries the tick of the text it was worked out on, and
    // the editor drops an edit whose tick is not its own, so a second
    // edit sent before the first comes back would be dropped
    waiting: bool,
}

impl Nudge {
    pub fn press(&mut self, dx: f64, dy: f64) {
        self.dx += dx;
        self.dy += dy;
    }

    /// The move to write now, cleared here, or `None` while an edit is
    /// with the editor or when there is none.
    pub fn ready(&mut self) -> Option<(f64, f64)> {
        if self.waiting || (self.dx == 0.0 && self.dy == 0.0) {
            return None;
        }
        Some((std::mem::take(&mut self.dx), std::mem::take(&mut self.dy)))
    }

    /// Whether an edit is with the editor.
    pub fn waiting(&self) -> bool {
        self.waiting
    }

    /// Note that an edit went to the editor.
    pub fn sent(&mut self) {
        self.waiting = true;
    }

    /// Note that the editor sent its text, after the edit it made or
    /// dropped.
    pub fn answered(&mut self) {
        self.waiting = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_while_an_edit_is_out_go_as_one_move_once_it_is_answered() {
        let mut nudge = Nudge::default();
        assert_eq!(nudge.ready(), None);
        nudge.press(1.0, 0.0);
        assert_eq!(nudge.ready(), Some((1.0, 0.0)));
        nudge.sent();

        nudge.press(1.0, 0.0);
        nudge.press(1.0, 0.0);
        nudge.press(0.0, -1.0);
        assert_eq!(nudge.ready(), None);
        nudge.answered();
        assert_eq!(nudge.ready(), Some((2.0, -1.0)));
        assert_eq!(nudge.ready(), None);
    }

    #[test]
    fn presses_that_cancel_write_nothing() {
        let mut nudge = Nudge::default();
        nudge.press(1.0, 0.0);
        nudge.press(-1.0, 0.0);
        assert_eq!(nudge.ready(), None);
    }

    #[test]
    fn without_an_editor_each_frames_presses_go_at_once() {
        let mut nudge = Nudge::default();
        nudge.press(0.0, 1.0);
        assert_eq!(nudge.ready(), Some((0.0, 1.0)));
        nudge.press(0.0, 1.0);
        assert_eq!(nudge.ready(), Some((0.0, 1.0)));
    }
}

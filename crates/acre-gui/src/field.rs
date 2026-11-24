use acre::lang::{Program, Span};
use acre::scene::edit::{self, TextEdit};
use acre::scene::{Point, Scene};
use eframe::egui::{self, Event, Key, KeyboardShortcut, Modifiers};

/// A field over an object that edits the text of the node that drew it.
pub struct Field {
    /// The node's place in the source tree. It is kept by place, as a
    /// selection is, so a change of source that shifts spans leaves the
    /// field on its node.
    path: Vec<usize>,
    /// The text when the field opened.
    original: String,
    /// The text in the field, markup included.
    pub text: String,
    /// Whether the field has had the keyboard focus.
    focused: bool,
}

impl Field {
    /// A field for the text of the node at `node` in `source`, which
    /// parses as `program`, empty when the node has none yet, or the
    /// reason it cannot take text for the status line.
    pub fn open(source: &str, program: &Program, node: Span) -> Result<Field, String> {
        let text = match edit::text_of(source, node) {
            Ok(Some(text)) => text,
            Ok(None) => String::new(),
            Err(e) => {
                let (line, col) = e.line_col(source);
                return Err(format!("cannot edit text: {line}:{col}: {}", e.message));
            }
        };
        let path = edit::node_path(program, node).ok_or("no node has this span")?;
        Ok(Field {
            path,
            original: text.clone(),
            text,
            focused: false,
        })
    }

    /// The span of the node in `program`, or `None` when it is gone.
    pub fn node(&self, program: &Program) -> Option<Span> {
        edit::node_span(program, &self.path)
    }

    /// Draw the field `width` points wide in `ui`, and say whether it
    /// has just been left by Enter, Escape, Tab or a click elsewhere: the
    /// moment to write its text. Shift+Enter starts a new line.
    pub fn ui(&mut self, ui: &mut egui::Ui, width: f32) -> bool {
        // read before the field takes its keys: Shift+Enter is its
        // return key, so it leaves a plain Enter in the events
        let enter = ui.input(|i| {
            i.events.iter().any(|e| {
                matches!(e, Event::Key { key: Key::Enter, pressed: true, modifiers, .. }
                    if !modifiers.shift)
            })
        });
        let edit = egui::TextEdit::multiline(&mut self.text)
            .desired_rows(1)
            .desired_width(width)
            .return_key(KeyboardShortcut::new(Modifiers::SHIFT, Key::Enter));
        let response = ui.add(edit);
        if !self.focused {
            response.request_focus();
        }
        let mut focus = response.has_focus();
        if focus && enter {
            response.surrender_focus();
            focus = false;
        }
        self.left(focus)
    }

    /// Note whether the field has the focus this frame, and say whether
    /// it has just lost the focus it had.
    fn left(&mut self, has_focus: bool) -> bool {
        if has_focus {
            self.focused = true;
            return false;
        }
        self.focused
    }

    /// The edits that write the field's text into `source`, which parses
    /// as `program`, none when the text is unchanged, or the reason they
    /// cannot be made.
    pub fn edits(&self, source: &str, program: &Program) -> Result<Vec<TextEdit>, String> {
        if self.text == self.original {
            return Ok(Vec::new());
        }
        let node = self.node(program).ok_or("the node edited is gone")?;
        edit::text_edits(source, node, &self.text).map_err(|e| {
            let (line, col) = e.line_col(source);
            format!("cannot edit text: {line}:{col}: {}", e.message)
        })
    }
}

/// The box around the objects that the node at `node` drew itself, at
/// time `t`, or `None` when none shows.
pub fn bounds(scene: &Scene, node: Span, t: f64) -> Option<(Point, Point)> {
    (0..scene.objects.len())
        .filter(|&i| scene.objects[i].source == node)
        .filter_map(|i| scene.bounds(i, t))
        .reduce(|(a, b), (min, max)| {
            (
                Point::new(a.x.min(min.x), a.y.min(min.y)),
                Point::new(b.x.max(max.x), b.y.max(max.y)),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use acre::lang::parse;
    use acre::scene;

    const SRC: &str = "canvas { size 300 200; box a { at 10 10; size 40 20; text \"one\"; } \
                       box b { at 100 100; size 40 40; } \
                       text c { at 200 20; text \"two\\nlines\"; } }";

    /// A field opened on the object at `point` of `src`.
    fn open(src: &str, point: Point) -> Result<Field, String> {
        let program = parse(src).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let index = scene.object_at(point, 0.0).unwrap();
        Field::open(src, &program, scene.objects[index].source)
    }

    #[test]
    fn a_field_opens_with_the_nodes_text_and_writes_nothing_unchanged() {
        let field = open(SRC, Point::new(20.0, 20.0)).unwrap();
        assert_eq!(field.text, "one");
        assert_eq!(field.edits(SRC, &parse(SRC).unwrap()), Ok(Vec::new()));

        let field = open(SRC, Point::new(205.0, 25.0)).unwrap();
        assert_eq!(field.text, "two\nlines");
    }

    #[test]
    fn a_node_without_text_opens_an_empty_field_that_writes_nothing_while_empty() {
        let mut field = open(SRC, Point::new(110.0, 110.0)).unwrap();
        assert_eq!(field.text, "");
        let program = parse(SRC).unwrap();
        assert_eq!(field.edits(SRC, &program), Ok(Vec::new()));

        field.text = "hi".into();
        let out = edit::apply(SRC, &field.edits(SRC, &program).unwrap());
        assert!(out.contains(r#"text "hi";"#), "{out}");
    }

    #[test]
    fn a_connector_without_a_label_opens_an_empty_field() {
        let src = "canvas { size 300 200; line l { from 10 10; to 100 100; } }";
        let field = open(src, Point::new(55.0, 55.0)).unwrap();
        assert_eq!(field.text, "");
    }

    #[test]
    fn a_node_that_cannot_take_text_or_has_text_not_its_own_opens_no_field() {
        let src = "canvas { size 300 200; let t \"x\"; box a { at 10 10; size 40 20; text t; } }";
        let why = open(src, Point::new(20.0, 20.0)).err().unwrap();
        assert!(why.starts_with("cannot edit text: 1:"), "{why}");
    }

    #[test]
    fn a_changed_text_is_written_to_the_node_after_a_shift_of_spans() {
        let mut field = open(SRC, Point::new(20.0, 20.0)).unwrap();
        field.text = "say \"one\"".into();
        let shifted = SRC.replace("size 300 200", "size 3000 2000");
        let program = parse(&shifted).unwrap();
        let out = edit::apply(&shifted, &field.edits(&shifted, &program).unwrap());
        assert!(out.contains(r#"text "say \"one\"";"#), "{out}");
    }

    #[test]
    fn a_field_is_left_once_it_loses_the_focus_it_had() {
        let mut field = open(SRC, Point::new(20.0, 20.0)).unwrap();
        assert!(!field.left(false));
        assert!(!field.left(true));
        assert!(field.left(false));
    }

    /// Whether `field` is left after a frame that gives it the focus and
    /// a frame with `key` pressed with `modifiers` held.
    fn press(field: &mut Field, key: Key, modifiers: Modifiers) -> bool {
        let ctx = egui::Context::default();
        let event = Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        let mut left = false;
        for events in [vec![], vec![event]] {
            let input = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(input, |ui| left = field.ui(ui, 200.0));
            output.textures_delta.clear();
        }
        left
    }

    #[test]
    fn shift_enter_starts_a_new_line_and_enter_leaves_the_field() {
        let mut field = open(SRC, Point::new(20.0, 20.0)).unwrap();
        assert!(!press(&mut field, Key::Enter, Modifiers::SHIFT));
        assert_eq!(field.text, "one\n");

        let mut field = open(SRC, Point::new(20.0, 20.0)).unwrap();
        assert!(press(&mut field, Key::Enter, Modifiers::NONE));
        assert_eq!(field.text, "one");
    }

    #[test]
    fn a_new_line_typed_in_the_field_is_written_as_an_escape() {
        let mut field = open(SRC, Point::new(20.0, 20.0)).unwrap();
        field.text = "one\ntwo".into();
        let program = parse(SRC).unwrap();
        let out = edit::apply(SRC, &field.edits(SRC, &program).unwrap());
        assert!(out.contains(r#"text "one\ntwo";"#), "{out}");
    }

    #[test]
    fn the_bounds_cover_what_the_node_drew() {
        let program = parse(SRC).unwrap();
        let scene = scene::build(&program, &[], &[]).unwrap();
        let a = scene.objects[0].source;
        assert_eq!(bounds(&scene, a, 0.0), scene.bounds(0, 0.0));
        assert_eq!(bounds(&scene, Span { start: 0, end: 1 }, 0.0), None);
    }
}

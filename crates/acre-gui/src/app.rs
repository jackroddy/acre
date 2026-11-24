use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use acre::lang;
use acre::scene::edit::{self, NewNode, TextEdit};
use acre::scene::{self, Scene, Section};
use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{
    self, Color32, Event, Key, Modifiers, Pos2, Rect, Sense, Stroke, TextStyle, Vec2,
    ViewportCommand,
};

use crate::field::{self, Field};
use crate::link::{Incoming, Link, Outgoing, Replace};
use crate::nudge::Nudge;
use crate::playback::{self, Playback};
use crate::properties::Properties;
use crate::recent::Recent;
use crate::selection::{self, Selection};
use crate::tool::{self, Step, Tool};
use crate::view::{MenuItem, View};

/// How often an open file is checked for a change on disk.
const DISK_CHECK: Duration = Duration::from_millis(500);

/// A viewer for one drawing. Its source comes from a file it watches, or
/// from an editor over the link.
pub struct App {
    recent: Recent,
    /// The file the source belongs to.
    path: Option<PathBuf>,
    text: String,
    /// When the watched file last changed, as the disk reports it.
    modified: Option<SystemTime>,
    last_check: Instant,
    /// The font and data files the last source listed, with when each
    /// last changed on disk. A change reloads the source.
    files: Vec<(PathBuf, Option<SystemTime>)>,
    /// The editor that sends the source, when acre-gui runs with
    /// `--stdio`. acre-gui then watches no file.
    link: Option<Link>,
    /// The editor's tick for `text`.
    tick: u64,
    error: Option<lang::Error>,
    /// The last scene that built, kept while the source has an error.
    scene: Option<Scene>,
    /// The program `scene` was built from.
    program: Option<lang::Program>,
    /// The nodes clicked, dragged or boxed last, outlined until a click
    /// elsewhere.
    selection: Selection,
    /// The field open over an object to edit its text.
    field: Option<Field>,
    /// The panel that changes the selected nodes' colors and strokes.
    properties: Properties,
    nudge: Nudge,
    /// The nodes added by the edit with the editor, to select once the
    /// editor sends its text back.
    added: Option<Added>,
    /// Counts the scenes built, so the view draws again on a new one.
    version: u64,
    /// Whether the editor heard about the current error, or that there
    /// is none.
    reported: bool,
    view: View,
    playback: Playback,
    /// Whether the panel that lists the scene's sections is open.
    sections_open: bool,
    /// The time drawn this frame.
    time: f64,
    /// The render pane's size at the last frame.
    pane: Vec2,
    /// The editor's cursor, as a byte offset into the source.
    cursor: Option<usize>,
    /// The canvas point under the pointer.
    hover: Option<scene::Point>,
    /// The latest message that is not a source error.
    notice: Option<String>,
    /// The characters in the scene's text that no font has.
    missing: Vec<char>,
    title: String,
}

impl App {
    /// A viewer of `file`, or of the last file opened. With `link`, a
    /// viewer of what the editor at its other end sends.
    pub fn new(file: Option<PathBuf>, link: Option<Link>) -> Self {
        let mut app = App {
            recent: Recent::load(),
            path: None,
            text: String::new(),
            modified: None,
            last_check: Instant::now(),
            files: Vec::new(),
            link,
            tick: 0,
            error: None,
            scene: None,
            program: None,
            selection: Selection::default(),
            field: None,
            properties: Properties::default(),
            nudge: Nudge::default(),
            added: None,
            version: 0,
            reported: false,
            view: View::new(),
            playback: Playback::new(),
            sections_open: true,
            time: 0.0,
            pane: Vec2::ZERO,
            cursor: None,
            hover: None,
            notice: None,
            missing: Vec::new(),
            title: String::new(),
        };
        if app.link.is_none() {
            let file = file.or_else(|| app.recent.files.first().filter(|f| f.exists()).cloned());
            if let Some(file) = file {
                app.open(&file);
            }
        }
        app
    }

    fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("untitled".into(), |n| n.to_string_lossy().into_owned())
    }

    fn open(&mut self, path: &Path) {
        match fs::read_to_string(path) {
            Ok(text) => {
                self.path = Some(path.to_path_buf());
                self.modified = modified(path);
                let now = Instant::now();
                self.playback.pause(now);
                self.playback.seek(now, 0.0);
                self.selection = Selection::default();
                self.field = None;
                self.show(text);
                self.notice = self.recent.touch(path).err();
            }
            Err(e) => self.notice = Some(format!("cannot open {}: {e}", path.display())),
        }
    }

    /// Show `text` as the source: build its scene, and keep the last one
    /// that built if it has an error.
    fn show(&mut self, text: String) {
        self.view.end_move();
        self.text = text;
        let built = lang::parse(&self.text)
            .and_then(|program| self.build(&program).map(|scene| (program, scene)));
        match built {
            Ok((program, scene)) => {
                self.playback
                    .set_duration(Instant::now(), scene.canvas.duration);
                self.error = None;
                self.notice = None;
                let missing = scene.missing_glyphs();
                if missing != self.missing {
                    for c in &missing {
                        eprintln!("{}: {}", self.name(), missing_glyph(*c));
                    }
                    self.missing = missing;
                }
                self.selection = std::mem::take(&mut self.selection).follow(&program);
                self.scene = Some(scene);
                self.program = Some(program);
                self.version += 1;
            }
            Err(e) => self.error = Some(e),
        }
        self.reported = false;
    }

    /// The scene `program` describes, with the font and data files it
    /// lists read from the folder of the source's file.
    fn build(&mut self, program: &lang::Program) -> Result<Scene, lang::Error> {
        let fonts = scene::font_files(program)?;
        let data = scene::data_files(program)?;
        let listed: Vec<(&str, &str, lang::Span)> = fonts
            .iter()
            .map(|f| ("font", f.path.as_str(), f.span))
            .chain(data.iter().map(|d| ("data file", d.path.as_str(), d.span)))
            .collect();
        let dir = self.path.as_deref().and_then(Path::parent);
        self.files = listed
            .iter()
            .map(|(_, path, _)| {
                let path = dir.map_or(PathBuf::from(path), |d| d.join(path));
                let changed = modified(&path);
                (path, changed)
            })
            .collect();
        let mut bytes = Vec::new();
        for ((what, written, span), (path, _)) in listed.iter().zip(&self.files) {
            let read = fs::read(path).map_err(|e| {
                lang::Error::new(*span, format!("cannot read {what} {written}: {e}"))
            })?;
            bytes.push(read);
        }
        let data_bytes = bytes.split_off(fonts.len());
        scene::build(program, &bytes, &data_bytes)
    }

    /// Build the source again if a font or data file it lists changed on
    /// disk.
    fn check_files(&mut self) {
        if self.files.iter().any(|(path, at)| modified(path) != *at) {
            self.show(self.text.clone());
        }
    }

    /// Read the watched file again if it changed on disk.
    fn check_disk(&mut self) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let on_disk = modified(&path);
        if on_disk.is_none() || on_disk == self.modified {
            return;
        }
        self.modified = on_disk;
        if let Ok(text) = fs::read_to_string(&path)
            && text != self.text
        {
            self.show(text);
        }
    }

    /// Take what the editor sent, and tell it about the source's error
    /// when that changed. False once the editor has gone.
    fn talk(&mut self) -> bool {
        let Some(link) = &self.link else { return true };
        let mut newest = None;
        let mut open = true;
        for message in link.received() {
            match message {
                Some(Incoming::Text { text, path, tick }) => newest = Some((text, path, tick)),
                Some(Incoming::Cursor { offset }) => self.cursor = Some(offset),
                None => open = false,
            }
        }

        // only the newest text matters when several came at once
        if let Some((text, path, tick)) = newest {
            // the editor sends its text after an edit it makes or
            // drops alike, so a drag it dropped snaps back here, and
            // this text is taken as the answer to the edit sent last
            self.nudge.answered();
            self.view.end_move();
            let path = path.map(PathBuf::from);
            if path != self.path {
                self.selection = Selection::default();
                self.field = None;
            }
            self.path = path;

            // an undo can bring back the text already shown under a
            // new tick, and an edit must carry the new one
            self.tick = tick;
            if text != self.text || self.version == 0 {
                self.show(text);
            }
            if let Some(added) = self.added.take() {
                self.select(added);
            }
        }
        if !self.reported
            && let Some(link) = &self.link
        {
            link.send(&match &self.error {
                Some(e) => Outgoing::Error {
                    offset: e.span.start,
                    message: e.message.clone(),
                },
                None => Outgoing::Ok,
            });
            self.reported = true;
        }
        open
    }

    /// Ask the editor to make `edits` to the source it last sent.
    fn send_edits(&self, edits: &[TextEdit]) {
        let Some(link) = &self.link else { return };
        link.send(&Outgoing::Edit {
            tick: self.tick,
            edits: edits.iter().map(Replace::from).collect(),
        });
    }

    /// Move the nodes at `nodes` by `by` canvas pixels: ask the editor to
    /// make the edits, or make them in the file and show it again. A
    /// refusal of any node snaps them all back and names the reason on
    /// the status line.
    fn move_nodes(&mut self, nodes: &[lang::Span], by: scene::Point) {
        if !nodes.iter().all(|&n| self.selection.contains(n))
            && let Some(program) = &self.program
        {
            self.selection = Selection::of(program, nodes);
        }

        // with an error, the scene shown was built from an earlier
        // text, so its spans do not point into `text`
        let edits = match &self.error {
            Some(_) => Err("fix the source's error before moving things".to_string()),
            None => edit::move_edits(&self.text, nodes, by.x, by.y).map_err(|e| {
                let (line, col) = e.line_col(&self.text);
                format!("cannot move: {line}:{col}: {}", e.message)
            }),
        };
        let edits = match edits {
            Ok(edits) => edits,
            Err(why) => {
                self.view.end_move();
                self.notice = Some(why);
                return;
            }
        };
        self.notice = None;
        if let Err(why) = self.write(&edits) {
            self.view.end_move();
            self.notice = Some(why);
        }
    }

    /// Ask the editor to make `edits`, or make them in the file and show
    /// it again.
    fn write(&mut self, edits: &[TextEdit]) -> Result<(), String> {
        if self.link.is_some() {
            self.send_edits(edits);
            self.nudge.sent();
            return Ok(());
        }
        let path = self.path.clone().ok_or("no file to write the edit to")?;
        let text = edit::apply(&self.text, edits);
        fs::write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        self.modified = modified(&path);
        self.show(text);
        Ok(())
    }

    /// Open a field over the objects the node at `node` drew to edit its
    /// text, and select the node, or name on the status line why it has
    /// no text to edit.
    fn open_field(&mut self, node: lang::Span) {
        let Some(program) = &self.program else { return };
        self.selection = Selection::of(program, &[node]);

        // with an error, the scene shown was built from an earlier
        // text, so its spans do not point into `text`
        let opened = match &self.error {
            Some(_) => Err("fix the source's error before editing text".to_string()),
            None => Field::open(&self.text, program, node),
        };
        match opened {
            Ok(f) => {
                self.notice = None;
                self.field = Some(f);
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Draw the open field over its node's objects in the pane at
    /// `rect`, and write its text once it loses the focus.
    fn edit_field(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let (Some(f), Some(scene), Some(program)) = (&mut self.field, &self.scene, &self.program)
        else {
            return;
        };
        let bounds = f
            .node(program)
            .and_then(|n| field::bounds(scene, n, self.time));
        let left = match bounds {
            Some((min, max)) => {
                let (min, max) = (
                    self.view.to_screen(rect, min),
                    self.view.to_screen(rect, max),
                );
                let width = (max.x - min.x).max(160.0);
                egui::Area::new(ui.scope_id().with("text field"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(min)
                    .show(ui.ctx(), |ui| f.ui(ui, width))
                    .inner
            }

            // the node draws nothing now, so the field has nothing to
            // sit over
            None => true,
        };
        if !left {
            return;
        }
        let f = self.field.take().expect("checked above");
        let edits = match &self.error {
            Some(_) => Err("fix the source's error before editing text".to_string()),
            None => f.edits(&self.text, self.program.as_ref().expect("checked above")),
        };
        match edits {
            Ok(edits) if edits.is_empty() => {}
            Ok(edits) => {
                self.notice = None;
                if let Err(why) = self.write(&edits) {
                    self.notice = Some(why);
                }
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Select every node whose objects lie inside `area`, in canvas
    /// pixels, at the time shown.
    fn select_in(&mut self, area: (scene::Point, scene::Point)) {
        let (Some(scene), Some(program)) = (&self.scene, &self.program) else {
            return;
        };
        let spans = selection::in_box(program, scene, self.time, area);
        self.selection = Selection::of(program, &spans);
    }

    /// The boxes of the objects the node under the editor's cursor drew.
    /// The innermost node holding the cursor wins, so a cursor inside a
    /// group's child marks that child.
    fn marks(&self) -> Vec<(scene::Point, scene::Point)> {
        let (Some(scene), Some(cursor)) = (&self.scene, self.cursor) else {
            return Vec::new();
        };
        let holds =
            |span: &lang::Span| span.start < span.end && span.start <= cursor && cursor <= span.end;
        let Some(innermost) = scene
            .objects
            .iter()
            .map(|o| o.source)
            .filter(holds)
            .min_by_key(|s| s.end - s.start)
        else {
            return Vec::new();
        };
        (0..scene.objects.len())
            .filter(|&i| scene.objects[i].source == innermost)
            .filter_map(|i| scene.bounds(i, self.time))
            .collect()
    }

    /// Select the node that drew the object at canvas `point`, and point
    /// the editor at it, or name its line when there is no editor. A
    /// point on no object ends the selection.
    fn go_to_object_at(&mut self, point: scene::Point) {
        let (Some(scene), Some(program)) = (&self.scene, &self.program) else {
            return;
        };
        let Some(index) = scene.object_at(point, self.time) else {
            self.selection = Selection::default();
            return;
        };
        let source = scene.objects[index].source;
        self.selection = Selection::of(program, &[source]);
        let offset = source.start;
        match &self.link {
            Some(link) => {
                link.send(&Outgoing::Goto { offset });
                self.cursor = Some(offset);
            }
            None => {
                let line = self.text[..offset.min(self.text.len())]
                    .matches('\n')
                    .count()
                    + 1;
                self.notice = Some(format!("drawn by line {line}"));
            }
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…").clicked() {
                    let dialog = rfd::FileDialog::new().add_filter("acre drawing", &["acre"]);
                    if let Some(path) = dialog.pick_file() {
                        self.open(&path);
                    }
                    ui.close();
                }
                ui.menu_button("Open Recent", |ui| {
                    if self.recent.files.is_empty() {
                        ui.label("nothing yet");
                    }
                    let mut chosen = None;
                    for file in &self.recent.files {
                        if ui.button(file.to_string_lossy()).clicked() {
                            chosen = Some(file.clone());
                        }
                    }
                    if let Some(file) = chosen {
                        self.open(&file);
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
        });
    }

    /// The buttons that zoom the render, the snap and grid toggles, and
    /// the tools, with Done and Cancel while a path is being drawn.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("−").on_hover_text("zoom out").clicked() {
                self.view.zoom_by(0.8, self.pane);
            }
            if ui.button("+").on_hover_text("zoom in").clicked() {
                self.view.zoom_by(1.25, self.pane);
            }
            if ui.button("Fit").clicked() {
                self.view.fit();
            }
            if ui
                .button("100%")
                .on_hover_text("one screen point to a canvas pixel")
                .clicked()
            {
                self.view.actual_size(self.pane);
            }
            ui.separator();
            ui.toggle_value(&mut self.view.snap, "Snap")
                .on_hover_text("snap a dragged object to other objects and the grid");
            ui.toggle_value(&mut self.view.grid, "Grid")
                .on_hover_text("draw the grid and snap to it");
            ui.add(
                egui::DragValue::new(&mut self.view.spacing)
                    .range(1.0..=1000.0)
                    .suffix(" px"),
            )
            .on_hover_text("the grid's spacing in canvas pixels");
            ui.separator();
            for t in Tool::ALL {
                if ui
                    .selectable_label(self.view.tool == t, t.label())
                    .on_hover_text(t.hint())
                    .clicked()
                {
                    self.view.tool = t;
                    self.view.path.clear();
                }
            }
            if self.view.tool.draws_path() && !self.view.path.is_empty() {
                ui.separator();
                if ui.button("Done").clicked() {
                    self.finish_path();
                }
                if ui.button("Cancel").clicked() {
                    self.view.tool = Tool::Select;
                    self.view.path.clear();
                }
            }
        });
    }

    /// Space plays and pauses. With a selection the arrow keys move it a
    /// canvas pixel, and with none Left and Right step a frame. Delete
    /// deletes the selection, and Ctrl+C and Ctrl+V copy and paste. No
    /// key acts while a text field has the keys.
    fn keys(&mut self, ctx: &egui::Context) {
        if !ctx.text_edit_focused() {
            self.take_keys(ctx);
        }
        if let Some((dx, dy)) = self.nudge.ready() {
            let nodes = self.selection.spans().to_vec();
            if !nodes.is_empty() {
                self.move_nodes(&nodes, scene::Point::new(dx, dy));
            }
        }
    }

    fn take_keys(&mut self, ctx: &egui::Context) {
        // consumed before the bar is laid out, so a focused button does
        // not take the same press
        let count = |key| ctx.input_mut(|i| i.count_and_consume_key(Modifiers::NONE, key)) as i64;
        let space = count(Key::Space);
        let across = count(Key::ArrowRight) - count(Key::ArrowLeft);
        let down = count(Key::ArrowDown) - count(Key::ArrowUp);
        let delete = count(Key::Delete) > 0;

        // the windowing layer turns Ctrl+C and Ctrl+V into these events,
        // reading the clipboard for a paste
        let (copy, pasted) = ctx.input_mut(|i| {
            let copy = i.events.contains(&Event::Copy);
            let pasted = i.events.iter().find_map(|e| match e {
                Event::Paste(text) => Some(text.clone()),
                _ => None,
            });
            i.events
                .retain(|e| !matches!(e, Event::Copy | Event::Paste(_)));
            (copy, pasted)
        });

        let now = Instant::now();
        let fps = self.scene.as_ref().map(|s| s.canvas.fps);
        if self.playback.duration() > 0.0 && space % 2 == 1 {
            self.playback.toggle(now);
        }
        if !self.selection.spans().is_empty() {
            self.nudge.press(across as f64, down as f64);
        } else if let Some(fps) = fps
            && self.playback.duration() > 0.0
            && across != 0
        {
            self.playback.step(now, across, fps);
        }
        if delete {
            self.delete();
        }
        if copy {
            self.copy(ctx);
        }
        if let Some(text) = pasted {
            self.paste(&text);
        }
    }

    /// What `work` makes of the source, or the reason for the status line
    /// that it cannot, with `verb` naming the work.
    fn on_source<T>(
        &self,
        verb: &str,
        work: impl FnOnce(&str) -> Result<T, lang::Error>,
    ) -> Result<T, String> {
        // with an error, the scene shown was built from an earlier
        // text, so the selection's spans do not point into `text`
        if self.error.is_some() {
            return Err(format!("fix the source's error before you {verb}"));
        }
        work(&self.text).map_err(|e| {
            let (line, col) = e.line_col(&self.text);
            format!("cannot {verb}: {line}:{col}: {}", e.message)
        })
    }

    /// Delete the selected nodes, or name on the status line why not.
    fn delete(&mut self) {
        let nodes = self.selection.spans().to_vec();
        if nodes.is_empty() {
            return;
        }
        let deleted = self
            .on_source("delete", |text| edit::delete_edits(text, &nodes))
            .and_then(|edits| self.write(&edits));
        match deleted {
            Ok(()) => {
                self.notice = None;
                self.selection = Selection::default();
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Put the selected nodes' source on the clipboard, or name on the
    /// status line why not.
    fn copy(&mut self, ctx: &egui::Context) {
        let nodes = self.selection.spans().to_vec();
        if nodes.is_empty() {
            return;
        }
        match self.on_source("copy", |text| edit::copy_text(text, &nodes)) {
            Ok(text) => {
                self.notice = None;
                ctx.copy_text(text);
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Add the nodes in `text` to the canvas, 10 pixels right of and
    /// below where they were, and select them, or name on the status
    /// line why not.
    fn paste(&mut self, text: &str) {
        // the error's place may be in `text` or in the source, so the
        // reason names no line
        let edits = match &self.error {
            Some(_) => Err("fix the source's error before you paste".to_string()),
            None => edit::paste_edits(&self.text, text, 10.0, 10.0)
                .map_err(|e| format!("cannot paste: {}", e.message)),
        };
        let pasted = edits.and_then(|(edits, paths)| self.write(&edits).map(|()| paths));
        let paths = match pasted {
            Ok(paths) => paths,
            Err(why) => {
                self.notice = Some(why);
                return;
            }
        };
        self.notice = None;
        self.select_added(Added {
            paths,
            open_field: false,
        });
    }

    /// Select the nodes `added` names now, or once the editor sends back
    /// the text the edit with it makes.
    fn select_added(&mut self, added: Added) {
        match &self.link {
            Some(_) => self.added = Some(added),
            None => self.select(added),
        }
    }

    fn select(&mut self, added: Added) {
        let Some(program) = &self.program else { return };
        self.selection = Selection::at(program, added.paths);
        if added.open_field
            && let Some(&node) = self.selection.spans().first()
        {
            self.open_field(node);
        }
    }

    /// Add the duplicate of the selected nodes that a copy and a paste
    /// make, or name on the status line why not.
    fn duplicate(&mut self) {
        let nodes = self.selection.spans().to_vec();
        if nodes.is_empty() {
            return;
        }
        match self.on_source("duplicate", |text| edit::copy_text(text, &nodes)) {
            Ok(text) => self.paste(&text),
            Err(why) => self.notice = Some(why),
        }
    }

    /// Canvas point `p` snapped as the view snaps a new node's corner.
    fn snapped(&self, p: scene::Point) -> scene::Point {
        match &self.scene {
            Some(scene) => self.view.snap_point(scene, p, self.time).0,
            None => p,
        }
    }

    /// Where the source's 0 0 falls on the canvas.
    fn origin(&self) -> scene::Point {
        self.scene.as_ref().map_or(scene::Point::ZERO, |s| s.origin)
    }

    /// Take a click at canvas point `point` with the tool in use.
    fn click(&mut self, point: scene::Point) {
        match self.view.tool {
            Tool::Select => self.go_to_object_at(point),
            Tool::Box | Tool::Ellipse => self.add_box(point, None),
            Tool::Text => {
                let at = self.snapped(point) - self.origin();
                let text = "text".to_string();
                self.add_node(NewNode::Text { at, text }, true);
            }
            Tool::Line | Tool::Arrow => {
                self.notice = Some("drag from one end to the other".to_string());
            }
            Tool::Polygon | Tool::Polyline => {
                let p = self.snapped(point);
                let close = (tool::CLOSE / self.view.zoom()) as f64;
                let closed = self.view.tool == Tool::Polygon;
                if tool::click_path(&mut self.view.path, p, close, closed) == Step::Done {
                    self.finish_path();
                }
            }
        }
    }

    /// Take a drag from canvas point `from` to `to` with the tool in use.
    fn drawn(&mut self, from: scene::Point, to: scene::Point) {
        match self.view.tool {
            Tool::Box | Tool::Ellipse => self.add_box(from, Some(to)),
            Tool::Line | Tool::Arrow => self.add_line(from, to),
            Tool::Text | Tool::Polygon | Tool::Polyline => self.click(to),
            Tool::Select => {}
        }
    }

    /// Add a box, or an ellipse with that tool, from canvas point `from`
    /// to `to`, or of the size a click gives with no `to`.
    fn add_box(&mut self, from: scene::Point, to: Option<scene::Point>) {
        let corner = self.snapped(from);
        let (at, size) = match to {
            None => (corner, tool::CLICKED_SIZE),
            Some(to) => match tool::corners(corner, self.snapped(to)) {
                Some(c) => c,
                None => {
                    self.notice = Some("a box needs a width and a height".to_string());
                    return;
                }
            },
        };
        let at = at - self.origin();
        let node = match self.view.tool {
            Tool::Ellipse => NewNode::Ellipse { at, size },
            _ => NewNode::Box { at, size },
        };
        self.add_node(node, false);
    }

    /// Add a line, or an arrow with that tool, from canvas point `from`
    /// to `to`, each end on the thing there when it can name it.
    fn add_line(&mut self, from: scene::Point, to: scene::Point) {
        let (Some(scene), Some(program)) = (&self.scene, &self.program) else {
            return;
        };
        let end = |p| tool::end_at(program, scene, p, self.snapped(p), self.time);
        let (from, to) = (end(from), end(to));
        let edits = match &self.error {
            Some(_) => Err("fix the source's error before you add a node".to_string()),
            None => {
                let arrow = self.view.tool == Tool::Arrow;
                tool::line_edits(&self.text, program, from, to, scene.origin, arrow)
            }
        };
        self.add(edits, false);
    }

    /// Add the polygon or polyline through the points placed so far, or
    /// name on the status line why not.
    fn finish_path(&mut self) {
        let closed = self.view.tool == Tool::Polygon;
        if !tool::enough(&self.view.path, closed) {
            let fewest = if closed { 3 } else { 2 };
            self.notice = Some(format!("place at least {fewest} points"));
            return;
        }
        let origin = self.origin();
        let points = self.view.path.iter().map(|&p| p - origin).collect();
        let node = if closed {
            NewNode::Polygon { points }
        } else {
            NewNode::Polyline { points }
        };
        self.add_node(node, false);
    }

    /// Add `node` to the canvas and select it, opening its text field
    /// when `open_field`, or name on the status line why not.
    fn add_node(&mut self, node: NewNode, open_field: bool) {
        let edits = self.on_source("add a node", |text| edit::insert_edits(text, &node.text()?));
        self.add(edits, open_field);
    }

    /// Make `edits`, which add a node at the end of the canvas, if the
    /// source they make builds, then select the node, open its text field
    /// when `open_field`, and go back to the Select tool. Otherwise name
    /// on the status line why not.
    fn add(&mut self, edits: Result<Vec<TextEdit>, String>, open_field: bool) {
        let added = edits.and_then(|edits| {
            self.check(&edits)?;
            let program = self.program.as_ref().ok_or("no drawing to add to")?;
            let path = tool::added_path(program);
            self.write(&edits)?;
            Ok(path)
        });
        match added {
            Ok(path) => {
                self.notice = None;
                self.view.tool = Tool::Select;
                self.view.path.clear();
                self.select_added(Added {
                    paths: vec![path],
                    open_field,
                });
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Whether the source with `edits` made parses and builds, and the
    /// error for the status line when not.
    fn check(&mut self, edits: &[TextEdit]) -> Result<(), String> {
        if self.nudge.waiting() {
            return Err("wait for the editor to take the last edit".to_string());
        }
        let text = edit::apply(&self.text, edits);

        // `build` notes the files the source lists, and these are the
        // files of a source not yet shown
        let files = std::mem::take(&mut self.files);
        let built = lang::parse(&text).and_then(|program| self.build(&program).map(|_| ()));
        self.files = files;
        built.map_err(|e| {
            let (line, col) = e.line_col(&text);
            format!("cannot add the node: {line}:{col}: {}", e.message)
        })
    }

    /// The bar that plays, pauses and scrubs the scene's time.
    fn transport(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let duration = self.playback.duration();
        ui.horizontal(|ui| {
            let label = if self.playback.playing() {
                "Pause"
            } else {
                "Play"
            };
            if ui.button(label).on_hover_text("space").clicked() {
                self.playback.toggle(now);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sections = self.scene.as_ref().map_or(&[][..], |s| &s.sections);
                if !sections.is_empty() {
                    ui.toggle_value(&mut self.sections_open, "Sections");
                }
                let mut looping = self.playback.looping();
                if ui.toggle_value(&mut looping, "Loop").changed() {
                    self.playback.set_looping(now, looping);
                }
                ui.label(format!("{:.2} s / {duration:.2} s", self.time));
                if let Some(t) = time_bar(ui, sections, duration, self.time) {
                    self.playback.seek(now, t);
                }
            });
        });
        self.time = self.playback.time(now);
    }

    /// The scene's sections, each with its start, the current one
    /// marked. Clicking one goes to its start.
    fn sections(&mut self, ui: &mut egui::Ui) {
        let Some(scene) = &self.scene else { return };
        let current = playback::current(&scene.sections, self.time);
        let mut chosen = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, section) in scene.sections.iter().enumerate() {
                let here = current == Some(i);
                let text = format!("{:.2} s  {}", section.start, section.title);
                if section.subsections.is_empty() {
                    ui.horizontal(|ui| {
                        // the width of a fold arrow, so titles line up
                        ui.add_space(ui.spacing().indent);
                        if ui.selectable_label(here, text).clicked() {
                            chosen = Some(section.start);
                        }
                    });
                    continue;
                }

                // keyed by title, so a section stays open or folded
                // when an edit moves its start
                let id = ui.make_persistent_id(("section", &section.title));
                let sub = playback::current_subsection(section, self.time).filter(|_| here);
                CollapsingState::load_with_default_open(ui.ctx(), id, false)
                    .show_header(ui, |ui| {
                        if ui.selectable_label(here, text).clicked() {
                            chosen = Some(section.start);
                        }
                    })
                    .body(|ui| {
                        for (j, s) in section.subsections.iter().enumerate() {
                            let text = format!("{:.2} s  {}", s.start, s.title);
                            if ui.selectable_label(sub == Some(j), text).clicked() {
                                chosen = Some(s.start);
                            }
                        }
                    });
            }
        });
        if let Some(t) = chosen {
            self.playback.seek(Instant::now(), t);
        }
    }

    /// The panel that changes the selected nodes' colors, strokes and
    /// fonts.
    fn properties(&mut self, ui: &mut egui::Ui) {
        // with an error, the scene shown was built from an earlier
        // text, so the selection's spans do not point into `text`
        if self.error.is_some() {
            ui.label("fix the source's error before changing properties");
            return;
        }
        let fonts = self
            .program
            .as_ref()
            .and_then(|p| scene::font_files(p).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.name)
            .filter(|name| name != "math")
            .collect();
        self.properties
            .read(self.version, &self.text, self.selection.spans(), fonts);
        self.properties.ui(ui, self.selection.paths());
    }

    /// Write the first change in the properties panel not yet written,
    /// once no pointer button is held, so that a drag in a color picker
    /// writes once, and once the editor has taken the last edit.
    fn write_property(&mut self, ctx: &egui::Context) {
        if self.nudge.waiting() || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let Some((key, value)) = self.properties.take(self.selection.paths()) else {
            return;
        };
        let nodes = self.selection.spans().to_vec();
        let written = self
            .on_source("change it", |text| {
                edit::property_edits(text, &nodes, key, &value)
            })
            .and_then(|edits| {
                if edits.is_empty() {
                    return Ok(false);
                }
                self.write(&edits).map(|()| true)
            });

        // a refusal or a change that made no edit leaves the panel
        // showing a value the source does not hold
        match written {
            Ok(true) => self.notice = None,
            Ok(false) => self.properties.clear(),
            Err(why) => {
                self.notice = Some(why);
                self.properties.clear();
            }
        }
    }

    fn status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let where_ = self
                .path
                .as_ref()
                .map_or("untitled".into(), |p| p.display().to_string());
            ui.label(where_);
            ui.separator();
            ui.label(format!("{:.0}%", self.view.zoom() * 100.0));
            ui.separator();
            if let Some(p) = self.hover {
                ui.label(format!("{:.0}, {:.0}", p.x, p.y));
                ui.separator();
            }
            if let Some(e) = &self.error {
                let (line, col) = e.line_col(&self.text);
                ui.colored_label(
                    Color32::from_rgb(0xf7, 0x76, 0x8e),
                    format!("{line}:{col}: {}", e.message),
                );
            } else if let Some(notice) = &self.notice {
                ui.label(notice);
            } else if let Some(&c) = self.missing.first() {
                let more = match self.missing.len() - 1 {
                    0 => String::new(),
                    n => format!(", and {n} more"),
                };
                ui.colored_label(
                    Color32::from_rgb(0xe0, 0xaf, 0x68),
                    format!("{}{more}", missing_glyph(c)),
                );
            }
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.talk() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if self.last_check.elapsed() >= DISK_CHECK {
            self.last_check = Instant::now();
            self.check_files();
            if self.link.is_none() {
                self.check_disk();
            }
        }
        ctx.request_repaint_after(DISK_CHECK);
        let title = format!("{} - acre", self.name());
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.keys(ui.ctx());
        self.time = self.playback.time(Instant::now());
        if self.link.is_none() {
            egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.playback.duration() > 0.0 {
            egui::Panel::bottom("transport").show(ui, |ui| self.transport(ui));
        }
        let has_sections = self.scene.as_ref().is_some_and(|s| !s.sections.is_empty());
        if self.sections_open && has_sections {
            egui::Panel::left("sections").show(ui, |ui| self.sections(ui));
        }
        if !self.selection.spans().is_empty() {
            self.write_property(ui.ctx());
            egui::Panel::right("properties").show(ui, |ui| self.properties(ui));
        }
        if self.playback.playing() {
            ui.ctx().request_repaint();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let marks = self.marks();
                let shown = self.view.show(
                    ui,
                    self.scene.as_mut(),
                    self.version,
                    self.time,
                    &marks,
                    self.selection.spans(),
                );
                self.pane = shown.rect.size();
                self.hover = shown.hover;
                if let Some(point) = shown.clicked {
                    self.click(point);
                }
                if let Some((from, to)) = shown.created {
                    self.drawn(from, to);
                }
                if let Some(node) = shown.right_clicked
                    && let Some(program) = &self.program
                {
                    self.selection = Selection::of(program, &[node]);
                }
                match shown.chosen {
                    Some(MenuItem::Delete) => self.delete(),
                    Some(MenuItem::Duplicate) => self.duplicate(),
                    None => {}
                }
                if let Some(node) = shown.double_clicked {
                    self.open_field(node);
                }
                if let Some((nodes, by)) = shown.moved {
                    self.move_nodes(&nodes, by);
                }
                if let Some(area) = shown.boxed {
                    self.select_in(area);
                }
                self.edit_field(ui, shown.rect);
            });
    }
}

/// Nodes an edit adds, to select.
struct Added {
    /// Their places as [`edit::node_path`] gives them.
    paths: Vec<Vec<usize>>,
    /// Whether to open a field on the first to edit its text.
    open_field: bool,
}

/// Draw the bar that scrubs over `0..=duration`, divided into
/// `sections`, with the playhead at `time`. Returns the time the pointer
/// presses or drags to.
fn time_bar(ui: &mut egui::Ui, sections: &[Section], duration: f64, time: f64) -> Option<f64> {
    let size = Vec2::new(ui.available_width(), ui.spacing().interact_size.y * 1.4);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let x = |t: f64| rect.left() + (t / duration) as f32 * rect.width();
    let painter = ui.painter_at(rect);
    let visuals = ui.visuals();
    painter.rect_filled(rect, 3.0, visuals.extreme_bg_color);
    let played = Rect::from_min_max(rect.min, Pos2::new(x(time), rect.max.y));
    painter.rect_filled(played, 3.0, visuals.selection.bg_fill.gamma_multiply(0.5));

    let font = TextStyle::Small.resolve(ui.style());
    let current = playback::current(sections, time);
    for (i, section) in sections.iter().enumerate() {
        let left = x(section.start);
        let right = sections.get(i + 1).map_or(rect.right(), |s| x(s.start));
        if section.start > 0.0 {
            painter.vline(
                left,
                rect.y_range(),
                visuals.widgets.noninteractive.bg_stroke,
            );
        }
        let color = if current == Some(i) {
            visuals.strong_text_color()
        } else {
            visuals.weak_text_color()
        };
        let tick = (rect.bottom() - rect.height() * 0.3)..=rect.bottom();
        for s in section
            .subsections
            .iter()
            .filter(|s| s.start > section.start)
        {
            painter.vline(
                x(s.start),
                tick.clone(),
                visuals.widgets.noninteractive.bg_stroke,
            );
        }
        let mut job = LayoutJob::simple_singleline(section.title.clone(), font.clone(), color);
        job.wrap = TextWrapping::truncate_at_width(right - left - 8.0);
        let galley = painter.layout_job(job);
        let at = Pos2::new(
            (left + right - galley.size().x) / 2.0,
            rect.center().y - galley.size().y / 2.0,
        );
        painter.galley(at, galley, color);
    }
    painter.vline(
        x(time),
        rect.y_range(),
        Stroke::new(2.0, visuals.strong_text_color()),
    );

    let time_at = |p: Pos2| ((p.x - rect.left()) / rect.width()) as f64 * duration;
    if rect.width() <= 0.0 {
        return None;
    }
    let pressed = response.interact_pointer_pos().map(time_at);
    if let Some(t) = response.hover_pos().map(time_at)
        && let Some(i) = playback::current(sections, t)
    {
        let section = &sections[i];
        let place = match playback::current_subsection(section, t) {
            Some(j) => format!("{} › {}", section.title, section.subsections[j].title),
            None => section.title.clone(),
        };
        response.on_hover_text_at_pointer(place);
    }
    pressed
}

/// When the file at `path` last changed, or `None` when the disk cannot
/// say.
fn missing_glyph(c: char) -> String {
    format!(
        "warning: no font has a glyph for '{c}' (U+{:04X})",
        c as u32
    )
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

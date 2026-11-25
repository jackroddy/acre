use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use acre::lang;
use acre::scene::edit::{self, NewNode, TextEdit};
use acre::scene::{self, Section};
use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{
    self, Align2, Color32, Event, FontId, Key, Modifiers, Pos2, Rect, Sense, Stroke, TextStyle,
    Vec2, ViewportCommand,
};

use crate::anchor;
use crate::arrange::{self, Arrange};
use crate::document::{self, Added, Document};
use crate::field::{self, Field};
use crate::handle::{self, Reshape, Target};
use crate::link::{Editor, Incoming, Link, Outgoing, Replace};
use crate::neovim;
use crate::nudge::Nudge;
use crate::playback;
use crate::preview::Previews;
use crate::recent::Recent;
use crate::selection::{self, Selection};
use crate::snap;
use crate::tabs;
use crate::tool::{self, Step, Tool};
use crate::view::{MenuItem, Toolbar};

/// How often an open file is checked for a change on disk.
const DISK_CHECK: Duration = Duration::from_millis(500);

/// The status line's reason for an edit refused while the view is locked.
const LOCKED: &str = "locked";

/// A viewer of drawings. Each drawing's source comes from a file it
/// watches, or from an editor over the link.
pub struct App {
    recent: Recent,
    previews: Previews,
    /// What the open dialog asks to close.
    closing: Option<Closing>,
    /// The drawings open, one to a tab.
    docs: Vec<Document>,
    /// The index in `docs` of the drawing shown.
    active: usize,
    last_check: Instant,
    /// The link an editor sends the source over. While an editor is
    /// connected, acre-gui watches no file.
    link: Option<Link>,
    /// The offset of the last goto sent to the editor.
    //
    // the next batch of messages that holds a text or a cursor
    // clears it. A cursor at this offset in that batch is the
    // goto's echo and changes nothing. The editor sends no echo
    // when its cursor is already at the offset. The offset then
    // holds back at most the next cursor message, and only when
    // that message is at this offset
    goto: Option<usize>,
    nudge: Nudge,
    bar: Toolbar,
    /// Whether the panel that lists the scene's sections is open.
    sections_open: bool,
    /// The render pane's size at the last frame.
    pane: Vec2,
    /// The canvas point under the pointer.
    hover: Option<scene::Point>,
    /// The latest message that is not a source error.
    notice: Option<String>,
    title: String,
}

impl App {
    /// A viewer of `file`, or of the last file in `recent`. With `link`,
    /// a viewer of what an editor at its other end sends, and with a
    /// link over stdin and stdout, of that alone. With `locked`, it
    /// refuses edits until the toolbar unlocks it.
    pub fn new(file: Option<PathBuf>, recent: Recent, link: Option<Link>, locked: bool) -> Self {
        let mut app = App {
            recent,
            previews: Previews::default(),
            closing: None,
            docs: vec![Document::blank()],
            active: 0,
            last_check: Instant::now(),
            link,
            goto: None,
            nudge: Nudge::default(),
            bar: Toolbar::new(),
            sections_open: true,
            pane: Vec2::ZERO,
            hover: None,
            notice: None,
            title: String::new(),
        };
        app.bar.locked = locked;
        if !app.stdio() {
            let file = file.or_else(|| app.recent.files.first().filter(|f| f.exists()).cloned());
            if let Some(file) = file {
                app.open(&file);
            }
        }
        app
    }

    /// Whether the link is over stdin and stdout, which leaves no file
    /// to open and no menu.
    fn stdio(&self) -> bool {
        self.link.as_ref().is_some_and(Link::is_stdio)
    }

    /// The editor connected over the link.
    fn editor(&self) -> Option<&Editor> {
        self.link.as_ref().and_then(Link::editor)
    }

    /// The drawing shown.
    fn doc(&self) -> &Document {
        &self.docs[self.active]
    }

    fn doc_mut(&mut self) -> &mut Document {
        &mut self.docs[self.active]
    }

    /// Show the file at `path` in its tab, or in a new tab when none
    /// has it.
    fn open(&mut self, path: &Path) {
        let i = match tabs::find(&self.docs, Some(path)) {
            Some(i) => i,
            None => {
                let mut doc = Document::new();
                if let Err(why) = doc.open(path) {
                    self.notice = Some(why);
                    return;
                }
                tabs::add(&mut self.docs, doc)
            }
        };
        self.activate(i);
        self.notice = self.recent.touch(path).err();
    }

    /// Show the drawing in tab `i`. With a link, ask the editor to show
    /// its file when the editor shows another.
    fn activate(&mut self, i: usize) {
        if i == self.active {
            return;
        }

        // the points placed and the field belong to the drawing left,
        // and a field's text is written only while it is shown
        let left = self.doc_mut();
        left.view.path.clear();
        left.field = None;
        self.active = i;
        self.shown_changed();
    }

    /// Clear the status line for the drawing now shown, and with a link,
    /// ask the editor to show its file when the editor shows another.
    fn shown_changed(&mut self) {
        self.notice = None;
        let doc = &self.docs[self.active];
        if let Some(editor) = self.editor()
            && !doc.linked
            && let Some(path) = &doc.path
        {
            editor.send(&Outgoing::Open {
                path: path.to_string_lossy().into_owned(),
            });
        }
    }

    /// Close tab `i`.
    fn close_tab(&mut self, i: usize) {
        let shown = i == self.active;
        self.active = tabs::close(&mut self.docs, self.active, i);
        if shown {
            self.shown_changed();
        }
    }

    /// Build each drawing again whose font or data files changed on
    /// disk, and without an editor read each watched file again that
    /// changed.
    fn check_disk(&mut self) {
        let watch = self.editor().is_none();
        for (i, doc) in self.docs.iter_mut().enumerate() {
            let mut built = doc.check_files();
            if watch {
                built |= doc.check_disk();
            }
            if built && i == self.active {
                self.notice = None;
            }
        }
    }

    /// Take what the editor sent, and tell it about the source's error
    /// when that changed. False once the editor at the other end of
    /// stdin and stdout has gone.
    fn talk(&mut self) -> bool {
        let Some(link) = &mut self.link else {
            return true;
        };
        let received = link.received();
        if received.closed {
            if link.is_stdio() {
                return false;
            }
            self.unlink();
        }
        let mut texts: Vec<(String, Option<PathBuf>, u64)> = Vec::new();
        let mut cursor = None;
        for message in received.messages {
            match message {
                Incoming::Text { text, path, tick } => {
                    // an unnamed buffer comes with an empty path
                    let path = path.filter(|p| !p.is_empty()).map(PathBuf::from);

                    // only the newest text of a file matters when
                    // several came at once
                    texts.retain(|(_, p, _)| *p != path);
                    texts.push((text, path, tick));
                }
                Incoming::Cursor { offset } => cursor = Some(offset),
            }
        }

        let goto = if texts.is_empty() && cursor.is_none() {
            None
        } else {
            self.goto.take()
        };
        for (text, path, tick) in texts {
            self.take_text(text, path, tick);
        }

        // after the text, so the offset is read against the source
        // of the newest program
        let Some(doc) = self.docs.iter_mut().find(|d| d.linked) else {
            return true;
        };
        if let Some(offset) = cursor
            && Some(offset) != goto
            && let (Some(scene), Some(program)) = (&doc.scene, &doc.program)
        {
            let drawn: Vec<lang::Span> = scene.objects.iter().map(|o| o.source).collect();
            doc.selection = Selection::at_cursor(program, &doc.program_text, &drawn, offset);
        }
        if !doc.reported
            && let Some(editor) = self.link.as_ref().and_then(Link::editor)
        {
            editor.send(&match &doc.error {
                Some(e) => Outgoing::Error {
                    offset: e.span.start,
                    message: e.message.clone(),
                },
                None => Outgoing::Ok,
            });
            doc.reported = true;
        }
        true
    }

    /// Watch each drawing's file again, once the editor has gone.
    fn unlink(&mut self) {
        for doc in &mut self.docs {
            doc.linked = false;

            // the editor's text can differ from the file, so the
            // next check reads every file again
            doc.modified = None;
        }
        self.goto = None;
        self.nudge.answered();
    }

    /// Show `text`, the editor's source of tick `tick` for the file at
    /// `path`, in that file's tab. A file other than the one the editor
    /// showed means the editor moved, and its tab is shown, opened when
    /// no tab has it.
    fn take_text(&mut self, text: String, path: Option<PathBuf>, tick: u64) {
        // the editor sends its text after an edit it makes or drops
        // alike, so a drag it dropped snaps back here, and this text
        // is taken as the answer to the edit sent last
        self.nudge.answered();
        let linked = self.docs.iter().position(|d| d.linked);
        let i = match linked {
            Some(i) if self.docs[i].path == path => i,
            _ => {
                if let Some(i) = linked {
                    self.docs[i].linked = false;
                }
                let i = tabs::find(&self.docs, path.as_deref()).unwrap_or_else(|| {
                    let mut doc = Document::new();
                    doc.path = path;
                    tabs::add(&mut self.docs, doc)
                });
                self.docs[i].linked = true;
                self.activate(i);
                i
            }
        };
        let doc = &mut self.docs[i];
        doc.view.end_move();

        // an undo can bring back the text already shown under a new
        // tick, and an edit must carry the new one
        doc.tick = tick;
        if (text != doc.text || doc.version == 0) && doc.show(text) && i == self.active {
            self.notice = None;
        }
        if let Some(added) = doc.added.take()
            && i == self.active
        {
            self.select(added);
        }
    }

    /// Ask the editor to make `edits` to the source it last sent.
    fn send_edits(&self, edits: &[TextEdit]) {
        let Some(editor) = self.editor() else { return };
        editor.send(&Outgoing::Edit {
            tick: self.doc().tick,
            edits: edits.iter().map(Replace::from).collect(),
        });
    }

    /// Move the nodes at `nodes` by `by` canvas pixels: ask the editor to
    /// make the edits, or make them in the file and show it again. A
    /// refusal of any node snaps them all back and names the reason on
    /// the status line.
    fn move_nodes(&mut self, nodes: &[lang::Span], by: scene::Point) {
        let doc = &mut self.docs[self.active];
        if !nodes.iter().all(|&n| doc.selection.contains(n))
            && let Some(program) = &doc.program
        {
            doc.selection = Selection::of(program, nodes);
        }

        // with an error, the scene shown was built from an earlier
        // text, so its spans do not point into `text`
        let outer = arrange::outermost(nodes);
        let stacked = outer.len() > 1
            && doc.error.is_none()
            && outer.iter().any(|&n| edit::siblings(&doc.text, n).is_ok());
        let edits = match &doc.error {
            Some(_) => Err("fix the source's error before moving things".to_string()),
            None if stacked => Err(
                "cannot move: drag one child of a stack or a grid at a time to reorder it"
                    .to_string(),
            ),
            None => edit::move_edits(&doc.text, nodes, by.x, by.y).map_err(|e| {
                let (line, col) = e.line_col(&doc.text);
                format!("cannot move: {line}:{col}: {}", e.message)
            }),
        };
        let edits = match edits {
            Ok(edits) => edits,
            Err(why) => {
                doc.view.end_move();
                self.notice = Some(why);
                return;
            }
        };
        self.notice = None;
        if let Err(why) = self.write(&edits) {
            self.doc_mut().view.end_move();
            self.notice = Some(why);
        }
    }

    /// Move the child of a stack or grid at `node` before its sibling
    /// `before`, or after the last when `None`, and keep it selected at
    /// `index` among them, or name on the status line why not.
    fn reorder(&mut self, node: lang::Span, before: Option<lang::Span>, index: usize) {
        let path = self
            .doc()
            .program
            .as_ref()
            .and_then(|p| edit::node_path(p, node));
        let Some(mut path) = path else { return };
        let edits = self.on_source("reorder", |text| edit::reorder_edits(text, node, before));
        let written = edits.and_then(|edits| {
            if edits.is_empty() {
                return Ok(false);
            }
            self.write(&edits).map(|()| true)
        });
        match written {
            Ok(true) => {
                self.notice = None;
                *path.last_mut().expect("a path names a node") = index;
                self.select_added(Added {
                    paths: vec![path],
                    open_field: false,
                });
            }
            Ok(false) => {
                self.notice = None;
                let doc = self.doc_mut();
                if let Some(program) = &doc.program {
                    doc.selection = Selection::at(program, vec![path]);
                }
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Resize a box or an ellipse, move a vertex, or move an end of a
    /// line, as a handle was dragged, or name on the status line why not.
    fn reshape(&mut self, reshape: Reshape) {
        let edits = match reshape {
            Reshape::End { node, end, to } => return self.move_end(node, end, to),
            Reshape::Resize { node, from, to } => {
                self.on_source("resize", |text| edit::resize_edits(text, node, from, to))
            }
            Reshape::Vertex { node, index, by } => self.on_source("move the vertex", |text| {
                edit::vertex_edits(text, node, index, by.x, by.y)
            }),
        };
        let written = edits.and_then(|edits| {
            if edits.is_empty() {
                Ok(())
            } else {
                self.write(&edits)
            }
        });
        self.notice = written.err();
    }

    /// Put end `end`, 0 for `from` and 1 for `to`, of the line or arrow
    /// at `node` on the thing at canvas point `to`, as the Line tool puts
    /// an end there, or at `to` snapped, or name on the status line why
    /// not.
    fn move_end(&mut self, node: lang::Span, end: usize, to: scene::Point) {
        let doc = self.doc();
        let (Some(scene), Some(program)) = (&doc.scene, &doc.program) else {
            return;
        };
        let reach = doc.view.anchor_reach();
        let at = tool::end_at(&doc.view.outlines, to, self.snapped(to), reach);
        let edits = match &doc.error {
            Some(_) => Err("fix the source's error before moving an end".to_string()),
            None => tool::end_edits(&doc.text, program, node, end, at, scene.origin),
        };
        let written = edits.and_then(|edits| {
            if edits.is_empty() {
                return Ok(());
            }
            self.check(&edits, "move the end")?;
            self.write(&edits)
        });
        self.notice = written.err();
    }

    /// Line up or space out the selected nodes as `how` says, by the
    /// boxes of what they draw at the time shown, or name on the status
    /// line why not. A node inside another selected node moves with it.
    fn arrange(&mut self, how: Arrange) {
        let doc = self.doc();
        let Some(scene) = &doc.scene else { return };
        let drawn: Vec<(lang::Span, snap::Bounds)> = arrange::outermost(doc.selection.spans())
            .into_iter()
            .filter_map(|node| Some((node, arrange::bounds(scene, node, doc.time)?)))
            .collect();
        let boxes: Vec<snap::Bounds> = drawn.iter().map(|&(_, b)| b).collect();
        let moves: Vec<(lang::Span, f64, f64)> = drawn
            .iter()
            .zip(arrange::offsets(how, &boxes))
            .filter(|&(_, by)| by != scene::Point::ZERO)
            .map(|(&(node, _), by)| (node, by.x, by.y))
            .collect();
        if moves.is_empty() {
            self.notice = None;
            return;
        }
        let written = self
            .on_source(how.verb(), |text| edit::arrange_edits(text, &moves))
            .and_then(|edits| {
                if edits.is_empty() {
                    Ok(())
                } else {
                    self.write(&edits)
                }
            });
        self.notice = written.err();
    }

    /// Ask the editor to make `edits` to the drawing shown, or make them
    /// in its file and show it again.
    fn write(&mut self, edits: &[TextEdit]) -> Result<(), String> {
        if self.bar.locked {
            return Err(LOCKED.to_string());
        }
        if self.editor().is_some() {
            if !self.doc().linked {
                return Err("wait for the editor to show this drawing".to_string());
            }
            self.nudge.admit()?;
            self.send_edits(edits);
            self.nudge.sent();
            return Ok(());
        }
        let doc = &mut self.docs[self.active];
        let path = doc.path.clone().ok_or("no file to write the edit to")?;
        let text = edit::apply(&doc.text, edits);
        fs::write(&path, &text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        doc.modified = document::modified(&path);
        if doc.show(text) {
            self.notice = None;
        }
        Ok(())
    }

    /// Open a field over the objects the node at `node` drew to edit its
    /// text, and select the node, or name on the status line why it has
    /// no text to edit.
    fn open_field(&mut self, node: lang::Span) {
        let locked = self.bar.locked;
        let doc = &mut self.docs[self.active];
        let Some(program) = &doc.program else { return };
        doc.selection = Selection::of(program, &[node]);
        if locked {
            self.notice = Some(LOCKED.to_string());
            return;
        }

        // with an error, the scene shown was built from an earlier
        // text, so its spans do not point into `text`
        let opened = match &doc.error {
            Some(_) => Err("fix the source's error before editing text".to_string()),
            None => Field::open(&doc.text, program, node),
        };
        match opened {
            Ok(f) => {
                self.notice = None;
                doc.field = Some(f);
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Draw the open field over its node's objects in the pane at
    /// `rect`, and write its text once it loses the focus.
    fn edit_field(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let doc = &mut self.docs[self.active];
        let (Some(f), Some(scene), Some(program)) = (&mut doc.field, &doc.scene, &doc.program)
        else {
            return;
        };
        let bounds = f
            .node(program)
            .and_then(|n| field::bounds(scene, n, doc.time));
        let left = match bounds {
            Some((min, max)) => {
                let (min, max) = (doc.view.to_screen(rect, min), doc.view.to_screen(rect, max));
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
        let f = doc.field.take().expect("checked above");
        let edits = match &doc.error {
            Some(_) => Err("fix the source's error before editing text".to_string()),
            None => f.edits(&doc.text, doc.program.as_ref().expect("checked above")),
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
        let doc = self.doc_mut();
        let (Some(scene), Some(program)) = (&doc.scene, &doc.program) else {
            return;
        };
        let spans = selection::in_box(program, scene, doc.time, area);
        doc.selection = Selection::of(program, &spans);
    }

    /// Select the node that drew the object at canvas `point`, and point
    /// the editor at it, or name its line when the editor does not
    /// show this drawing. A
    /// point on no object ends the selection.
    fn go_to_object_at(&mut self, point: scene::Point) {
        let doc = &mut self.docs[self.active];
        let (Some(scene), Some(program)) = (&doc.scene, &doc.program) else {
            return;
        };
        doc.selection = Selection::clicked(program, scene, doc.time, point);
        let Some(offset) = doc.selection.spans().first().map(|s| s.start) else {
            return;
        };
        match self.link.as_ref().and_then(Link::editor) {
            Some(editor) if doc.linked => {
                editor.send(&Outgoing::Goto { offset });
                self.goto = Some(offset);
            }
            _ => {
                let line = doc.text[..offset.min(doc.text.len())].matches('\n').count() + 1;
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
                        let button = ui
                            .button(file.to_string_lossy())
                            .on_hover_ui(|ui| self.previews.show(ui, file));
                        if button.clicked() {
                            chosen = Some(file.clone());
                        }
                    }
                    if let Some(file) = chosen {
                        self.open(&file);
                        ui.close();
                    }
                });
                self.edit_in_neovim(ui);
                ui.separator();
                if ui.button("Quit").clicked() {
                    ui.ctx().send_viewport_cmd(ViewportCommand::Close);
                }
            });
        });
    }

    /// The menu item that opens neovim on the drawing shown, linked to
    /// this viewer. It is disabled while an editor is connected.
    fn edit_in_neovim(&mut self, ui: &mut egui::Ui) {
        let file = self.doc().path.clone();
        let socket = self.link.as_ref().and_then(Link::socket);
        let why_not = match (&file, socket) {
            _ if self.editor().is_some() => Some("an editor is linked already"),
            (None, _) => Some("this drawing has no file"),
            (_, None) => Some("acre-gui has no socket for neovim to link to"),
            _ => None,
        };
        let button = ui
            .add_enabled(why_not.is_none(), egui::Button::new("Edit in neovim"))
            .on_disabled_hover_text(why_not.unwrap_or_default());
        if button.clicked()
            && let (Some(file), Some(socket)) = (file, socket)
        {
            self.notice = neovim::open(&file, socket).err();
            ui.close();
        }
    }

    /// Take up `tool`, dropping the points placed of any polygon or
    /// polyline being drawn.
    fn set_tool(&mut self, tool: Tool) {
        self.bar.tool = tool;
        for doc in &mut self.docs {
            doc.view.path.clear();
        }
    }

    /// The buttons that zoom the render, the snap, grid and lock toggles,
    /// and the tools, with Done and Cancel while a path is being drawn,
    /// and the buttons that align two or more selected nodes and space
    /// out three or more. While locked, only the Select tool is on and
    /// no arrange button is.
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let pane = self.pane;
            if ui.button("−").on_hover_text("zoom out").clicked() {
                self.doc_mut().view.zoom_by(0.8, pane);
            }
            if ui.button("+").on_hover_text("zoom in").clicked() {
                self.doc_mut().view.zoom_by(1.25, pane);
            }
            if ui.button("Fit").clicked() {
                self.doc_mut().view.fit();
            }
            if ui
                .button("100%")
                .on_hover_text("one screen point to a canvas pixel")
                .clicked()
            {
                self.doc_mut().view.actual_size(pane);
            }
            ui.separator();
            ui.toggle_value(&mut self.bar.snap, "Snap")
                .on_hover_text("snap a dragged object to other objects and the grid");
            ui.toggle_value(&mut self.bar.grid, "Grid")
                .on_hover_text("draw the grid and snap to it");
            ui.add(
                egui::DragValue::new(&mut self.bar.spacing)
                    .range(1.0..=1000.0)
                    .suffix(" px"),
            )
            .on_hover_text("the grid's spacing in canvas pixels");
            if ui
                .toggle_value(&mut self.bar.locked, "🔒 Lock")
                .on_hover_text("refuse every edit to the source")
                .clicked()
                && self.bar.locked
            {
                self.set_tool(Tool::Select);
            }
            let locked = self.bar.locked;
            ui.separator();
            for t in Tool::ALL {
                let enabled = !locked || t == Tool::Select;
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::selectable(self.bar.tool == t, t.label()),
                    )
                    .on_hover_text(t.hint())
                    .clicked()
                {
                    self.set_tool(t);
                }
            }
            if self.bar.tool.draws_path() && !self.doc().view.path.is_empty() {
                ui.separator();
                if ui.button("Done").clicked() {
                    self.finish_path();
                }
                if ui.button("Cancel").clicked() {
                    self.set_tool(Tool::Select);
                }
            }
            let selected = arrange::outermost(self.doc().selection.spans()).len();
            if selected >= 2 {
                ui.separator();
                let space: &[Arrange] = if selected >= 3 { &Arrange::SPACE } else { &[] };
                for &how in Arrange::ALIGN.iter().chain(space) {
                    if ui
                        .add_enabled(!locked, egui::Button::new(how.label()))
                        .on_hover_text(how.hint())
                        .clicked()
                    {
                        self.arrange(how);
                    }
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("×").on_hover_text("close acre-gui").clicked() {
                    self.closing = Some(Closing::Window);
                }
            });
        });
    }

    /// Ask whether to close what `closing` names, and close it on
    /// Close. A click on Cancel or outside the dialog keeps it open.
    fn confirm_close(&mut self, ctx: &egui::Context, closing: Closing) {
        let question = match closing {
            Closing::Window => "Close acre-gui?".to_string(),
            Closing::Tab(i) => format!("Close {}?", self.docs[i].name()),
        };
        let shown = egui::Modal::new(egui::Id::unique("close")).show(ctx, |ui| {
            ui.label(question);
            ui.add_space(8.0);
            ui.horizontal(|ui| (ui.button("Close").clicked(), ui.button("Cancel").clicked()))
                .inner
        });
        let (close, cancel) = shown.inner;
        if close || cancel || shown.backdrop_response.clicked() {
            self.closing = None;
        }
        match closing {
            Closing::Window if close => ctx.send_viewport_cmd(ViewportCommand::Close),
            Closing::Tab(i) if close => self.close_tab(i),
            _ => {}
        }
    }

    /// One tab for each drawing open, by its file's name, with its path
    /// on hover and a × that asks to close it. A click on a tab shows its
    /// drawing. The blank drawing has no tab.
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let mut chosen = None;
        ui.horizontal(|ui| {
            for (i, doc) in self.docs.iter().enumerate().filter(|(_, d)| !d.blank) {
                let path = doc
                    .path
                    .as_ref()
                    .map_or("untitled".into(), |p| p.display().to_string());
                let tab = ui.selectable_label(i == self.active, doc.name());
                if tab.on_hover_text(path).clicked() {
                    chosen = Some(i);
                }
                if ui
                    .small_button("×")
                    .on_hover_text("close this tab")
                    .clicked()
                {
                    self.closing = Some(Closing::Tab(i));
                }
                ui.separator();
            }
        });
        if let Some(i) = chosen {
            self.activate(i);
        }
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
            let nodes = self.doc().selection.spans().to_vec();
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
        let doc = &mut self.docs[self.active];
        let fps = doc.scene.as_ref().map(|s| s.canvas.fps);
        if doc.playback.duration() > 0.0 && space % 2 == 1 {
            doc.playback.toggle(now);
        }
        if !doc.selection.spans().is_empty() {
            if (across, down) != (0, 0) && !self.refuse_locked() {
                self.nudge.press(across as f64, down as f64);
            }
        } else if let Some(fps) = fps
            && doc.playback.duration() > 0.0
            && across != 0
        {
            doc.playback.step(now, across, fps);
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

    /// Whether the view is locked, with the reason on the status line
    /// when it is.
    fn refuse_locked(&mut self) -> bool {
        if self.bar.locked {
            self.notice = Some(LOCKED.to_string());
        }
        self.bar.locked
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
        let doc = self.doc();
        if doc.error.is_some() {
            return Err(format!("fix the source's error before you {verb}"));
        }
        work(&doc.text).map_err(|e| {
            let (line, col) = e.line_col(&doc.text);
            format!("cannot {verb}: {line}:{col}: {}", e.message)
        })
    }

    /// Delete the selected nodes, or name on the status line why not.
    fn delete(&mut self) {
        let nodes = self.doc().selection.spans().to_vec();
        if nodes.is_empty() || self.refuse_locked() {
            return;
        }
        let deleted = self
            .on_source("delete", |text| edit::delete_edits(text, &nodes))
            .and_then(|edits| self.write(&edits));
        match deleted {
            Ok(()) => {
                self.notice = None;
                self.doc_mut().selection = Selection::default();
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Put the selected nodes' source on the clipboard, or name on the
    /// status line why not.
    fn copy(&mut self, ctx: &egui::Context) {
        let nodes = self.doc().selection.spans().to_vec();
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
        if self.refuse_locked() {
            return;
        }

        // the error's place may be in `text` or in the source, so the
        // reason names no line
        let doc = self.doc();
        let edits = match &doc.error {
            Some(_) => Err("fix the source's error before you paste".to_string()),
            None => edit::paste_edits(&doc.text, text, 10.0, 10.0)
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
        match self.editor() {
            Some(_) => self.doc_mut().added = Some(added),
            None => self.select(added),
        }
    }

    fn select(&mut self, added: Added) {
        let doc = self.doc_mut();
        let Some(program) = &doc.program else { return };
        doc.selection = Selection::at(program, added.paths);
        if added.open_field
            && let Some(&node) = doc.selection.spans().first()
        {
            self.open_field(node);
        }
    }

    /// Add the duplicate of the selected nodes that a copy and a paste
    /// make, or name on the status line why not.
    fn duplicate(&mut self) {
        let nodes = self.doc().selection.spans().to_vec();
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
        let doc = self.doc();
        match &doc.scene {
            Some(scene) => doc.view.snap_point(&self.bar, scene, p, doc.time).0,
            None => p,
        }
    }

    /// Where the source's 0 0 falls on the canvas.
    fn origin(&self) -> scene::Point {
        self.doc()
            .scene
            .as_ref()
            .map_or(scene::Point::ZERO, |s| s.origin)
    }

    /// Take a click at canvas point `point` with the tool in use.
    fn click(&mut self, point: scene::Point) {
        match self.bar.tool {
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
                let closed = self.bar.tool == Tool::Polygon;
                let view = &mut self.docs[self.active].view;
                let close = (tool::CLOSE / view.zoom()) as f64;
                if tool::click_path(&mut view.path, p, close, closed) == Step::Done {
                    self.finish_path();
                }
            }
        }
    }

    /// Take a drag from canvas point `from` to `to` with the tool in use.
    fn drawn(&mut self, from: scene::Point, to: scene::Point) {
        match self.bar.tool {
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
        let node = match self.bar.tool {
            Tool::Ellipse => NewNode::Ellipse { at, size },
            _ => NewNode::Box { at, size },
        };
        self.add_node(node, false);
    }

    /// Add a line, or an arrow with that tool, from canvas point `from`
    /// to `to`, each end on the thing there when it can name it.
    fn add_line(&mut self, from: scene::Point, to: scene::Point) {
        let doc = self.doc();
        let (Some(scene), Some(program)) = (&doc.scene, &doc.program) else {
            return;
        };
        let reach = doc.view.anchor_reach();
        let end = |p| tool::end_at(&doc.view.outlines, p, self.snapped(p), reach);
        let (from, to) = (end(from), end(to));
        let edits = match &doc.error {
            Some(_) => Err("fix the source's error before you add a node".to_string()),
            None => {
                let arrow = self.bar.tool == Tool::Arrow;
                tool::line_edits(&doc.text, program, from, to, scene.origin, arrow)
            }
        };
        self.add(edits, false);
    }

    /// Add the polygon or polyline through the points placed so far, or
    /// name on the status line why not.
    fn finish_path(&mut self) {
        let closed = self.bar.tool == Tool::Polygon;
        let path = &self.doc().view.path;
        if !tool::enough(path, closed) {
            let fewest = if closed { 3 } else { 2 };
            self.notice = Some(format!("place at least {fewest} points"));
            return;
        }
        let origin = self.origin();
        let points = path.iter().map(|&p| p - origin).collect();
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
            self.check(&edits, "add the node")?;
            let program = self.doc().program.as_ref().ok_or("no drawing to add to")?;
            let path = tool::added_path(program);
            self.write(&edits)?;
            Ok(path)
        });
        match added {
            Ok(path) => {
                self.notice = None;
                self.set_tool(Tool::Select);
                self.select_added(Added {
                    paths: vec![path],
                    open_field,
                });
            }
            Err(why) => self.notice = Some(why),
        }
    }

    /// Whether the source with `edits` made parses and builds, and the
    /// error for the status line when not, which says it cannot `verb`.
    fn check(&mut self, edits: &[TextEdit], verb: &str) -> Result<(), String> {
        if self.nudge.waiting() {
            return Err("wait for the editor to take the last edit".to_string());
        }
        let doc = self.doc_mut();
        let text = edit::apply(&doc.text, edits);

        // `build` notes the files the source lists, and these are the
        // files of a source not yet shown
        let files = std::mem::take(&mut doc.files);
        let built = lang::parse(&text).and_then(|program| doc.build(&program).map(|_| ()));
        doc.files = files;
        built.map_err(|e| {
            let (line, col) = e.line_col(&text);
            format!("cannot {verb}: {line}:{col}: {}", e.message)
        })
    }

    /// The bar that plays, pauses and scrubs the scene's time.
    fn transport(&mut self, ui: &mut egui::Ui) {
        let now = Instant::now();
        let doc = &mut self.docs[self.active];
        let duration = doc.playback.duration();
        ui.horizontal(|ui| {
            let label = if doc.playback.playing() {
                "Pause"
            } else {
                "Play"
            };
            if ui.button(label).on_hover_text("space").clicked() {
                doc.playback.toggle(now);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let sections = doc.scene.as_ref().map_or(&[][..], |s| &s.sections);
                if !sections.is_empty() {
                    ui.toggle_value(&mut self.sections_open, "Sections");
                }
                let mut looping = doc.playback.looping();
                if ui.toggle_value(&mut looping, "Loop").changed() {
                    doc.playback.set_looping(now, looping);
                }
                ui.label(format!("{:.2} s / {duration:.2} s", doc.time));
                if let Some(t) = time_bar(ui, sections, duration, doc.time) {
                    doc.playback.seek(now, t);
                }
            });
        });
        doc.time = doc.playback.time(now);
    }

    /// The scene's sections, each with its start, the current one
    /// marked. Clicking one goes to its start.
    fn sections(&mut self, ui: &mut egui::Ui) {
        let doc = self.doc_mut();
        let Some(scene) = &doc.scene else { return };
        let current = playback::current(&scene.sections, doc.time);
        let mut chosen = None;
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
            let sub = playback::current_subsection(section, doc.time).filter(|_| here);
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
        if let Some(t) = chosen {
            doc.playback.seek(Instant::now(), t);
        }
    }

    /// The panel that changes the selected nodes' colors, strokes and
    /// fonts.
    fn properties(&mut self, ui: &mut egui::Ui) {
        let locked = self.bar.locked;
        let doc = self.doc_mut();

        // with an error, the scene shown was built from an earlier
        // text, so the selection's spans do not point into `text`
        if doc.error.is_some() {
            ui.label("fix the source's error before changing properties");
            return;
        }
        let fonts = doc
            .program
            .as_ref()
            .and_then(|p| scene::font_files(p).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.name)
            .filter(|name| name != "math")
            .collect();
        doc.properties
            .read(doc.version, &doc.text, doc.selection.spans(), fonts);
        if locked {
            ui.disable();
        }
        doc.properties.ui(ui, doc.selection.paths());
    }

    /// Write the first change in the properties panel not yet written,
    /// once no pointer button is held, so that a drag in a color picker
    /// writes once, and once the editor has taken the last edit.
    fn write_property(&mut self, ctx: &egui::Context) {
        if self.nudge.waiting() || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        let doc = self.doc_mut();
        let Some((key, value)) = doc.properties.take(doc.selection.paths()) else {
            return;
        };
        let nodes = doc.selection.spans().to_vec();
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
            Ok(false) => self.doc_mut().properties.clear(),
            Err(why) => {
                self.notice = Some(why);
                self.doc_mut().properties.clear();
            }
        }
    }

    fn status(&self, ui: &mut egui::Ui) {
        let doc = self.doc();
        ui.horizontal(|ui| {
            let where_ = doc
                .path
                .as_ref()
                .map_or("untitled".into(), |p| p.display().to_string());
            ui.label(where_);
            ui.separator();
            ui.label(format!("{:.0}%", doc.view.zoom() * 100.0));
            ui.separator();
            if let Some(p) = self.hover {
                ui.label(format!("{:.0}, {:.0}", p.x, p.y));
                ui.separator();
            }
            if let Some(e) = &doc.error {
                let (line, col) = e.line_col(&doc.text);
                ui.colored_label(
                    Color32::from_rgb(0xf7, 0x76, 0x8e),
                    format!("{line}:{col}: {}", e.message),
                );
            } else if let Some(notice) = &self.notice {
                ui.label(notice);
            } else if let Some(&c) = doc.missing.first() {
                let more = match doc.missing.len() - 1 {
                    0 => String::new(),
                    n => format!(", and {n} more"),
                };
                ui.colored_label(
                    Color32::from_rgb(0xe0, 0xaf, 0x68),
                    format!("{}{more}", document::missing_glyph(c)),
                );
            }
        });
    }

    /// The render pane: the drawing shown, and what the person did to it
    /// this frame.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let doc = &mut self.docs[self.active];
        doc.view.target = match (&doc.program, &doc.scene) {
            _ if self.bar.locked => None,
            (Some(program), Some(scene)) => handle::target(program, scene, doc.selection.spans()),
            _ => None,
        };
        let ends = matches!(self.bar.tool, Tool::Line | Tool::Arrow)
            || matches!(doc.view.target, Some(Target::Line { .. }));
        doc.view.outlines = match (&doc.program, &doc.scene) {
            (Some(program), Some(scene)) if ends => anchor::outlines(program, scene, doc.time),
            _ => Vec::new(),
        };
        let shown = doc.view.show(
            ui,
            &self.bar,
            doc.scene.as_mut(),
            doc.version,
            doc.time,
            doc.selection.spans(),
        );
        if doc.blank && !self.stdio() {
            ui.painter().text(
                shown.rect.center(),
                Align2::CENTER_CENTER,
                "Open a drawing with File › Open",
                FontId::proportional(16.0),
                ui.visuals().weak_text_color(),
            );
        }
        self.pane = shown.rect.size();
        self.hover = shown.hover;
        if let Some(point) = shown.clicked {
            self.click(point);
        }
        if let Some((from, to)) = shown.created {
            self.drawn(from, to);
        }
        if let Some(node) = shown.right_clicked {
            let doc = self.doc_mut();
            if let Some(program) = &doc.program {
                doc.selection = Selection::of(program, &[node]);
            }
        }
        match shown.chosen {
            Some(MenuItem::Delete) => self.delete(),
            Some(MenuItem::Duplicate) => self.duplicate(),
            None => {}
        }
        if let Some(node) = shown.double_clicked {
            self.open_field(node);
        }
        if let Some(node) = shown.pressed {
            let doc = self.doc_mut();
            if doc.error.is_none()
                && let Ok(siblings) = edit::siblings(&doc.text, node)
            {
                doc.view.reorder(siblings);
            }
        }
        if let Some((nodes, by)) = shown.moved {
            self.move_nodes(&nodes, by);
        }
        if let Some((node, before, index)) = shown.reordered {
            self.reorder(node, before, index);
        }
        if let Some(reshape) = shown.reshaped {
            self.reshape(reshape);
        }
        if let Some(area) = shown.boxed {
            self.select_in(area);
        }
        self.edit_field(ui, shown.rect);
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        // dropping the link removes its socket
        self.link = None;
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.talk() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if self.last_check.elapsed() >= DISK_CHECK {
            self.last_check = Instant::now();
            self.check_disk();
        }
        ctx.request_repaint_after(DISK_CHECK);
        let title = format!("{} - acre", self.doc().name());
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.keys(ui.ctx());
        let doc = self.doc_mut();
        doc.time = doc.playback.time(Instant::now());
        if !self.stdio() {
            egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        }
        egui::Panel::top("tabs").show(ui, |ui| self.tabs(ui));
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        if let Some(closing) = self.closing {
            self.confirm_close(ui.ctx(), closing);
        }
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        if self.doc().playback.duration() > 0.0 {
            egui::Panel::bottom("transport").show(ui, |ui| self.transport(ui));
        }
        let pane = ui.available_rect_before_wrap();
        let doc = self.doc();
        let has_sections = doc.scene.as_ref().is_some_and(|s| !s.sections.is_empty());
        if self.sections_open && has_sections {
            side_panel(ui, "sections", pane, Align2::LEFT_TOP, |ui| {
                self.sections(ui)
            });
        }
        let doc = self.doc();
        if !doc.selection.spans().is_empty() && doc.selection.made_in_viewer() {
            self.write_property(ui.ctx());
            side_panel(ui, "properties", pane, Align2::RIGHT_TOP, |ui| {
                self.properties(ui)
            });
        }
        if self.doc().playback.playing() {
            ui.ctx().request_repaint();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.canvas(ui));
    }
}

/// What the dialog that asks before closing is about.
#[derive(Clone, Copy)]
enum Closing {
    /// The viewer's window.
    Window,
    /// The tab at this index.
    Tab(usize),
}

/// The width of a side panel, as egui gives a side panel by default.
const PANEL_WIDTH: f32 = 200.0;

/// Draw `contents` in a panel over the `side` edge of the render pane at
/// `pane`, as tall as the pane and scrolled when taller. The pane keeps
/// its size, and the pointer over the panel reaches the panel alone.
fn side_panel(
    ui: &egui::Ui,
    id: &str,
    pane: Rect,
    side: Align2,
    contents: impl FnOnce(&mut egui::Ui),
) {
    egui::Area::new(ui.scope_id().with(id))
        .order(egui::Order::Middle)
        .movable(false)
        .fade_in(false)
        .constrain_to(pane)
        .anchor(side, Vec2::ZERO)
        .show(ui.ctx(), |ui| {
            let frame = egui::Frame::side_top_panel(ui.style());
            let margin = frame.total_margin().sum();
            let height = pane.height() - margin.y;
            let rect = frame
                .show(ui, |ui| {
                    ui.set_min_size(Vec2::new(PANEL_WIDTH - margin.x, height));
                    egui::ScrollArea::vertical()
                        .max_height(height)
                        .show(ui, contents);
                })
                .response
                .rect;
            let edge = if side == Align2::LEFT_TOP {
                rect.right()
            } else {
                rect.left()
            };
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            ui.painter().vline(edge, rect.y_range(), stroke);
        });
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::os::unix::net::UnixStream;

    /// The next line the viewer sends to `editor`, with the viewer
    /// taking messages meanwhile.
    fn answer(app: &mut App, editor: &mut BufReader<UnixStream>) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut line = String::new();
        while Instant::now() < deadline {
            assert!(app.talk());
            match editor.read_line(&mut line) {
                Ok(_) if line.ends_with('\n') => return line,
                Err(e) if e.kind() != ErrorKind::WouldBlock && e.kind() != ErrorKind::TimedOut => {
                    panic!("{e}")
                }
                _ => {}
            }
        }
        panic!("no answer, only {line:?}");
    }

    #[test]
    fn an_editor_on_the_socket_gets_answers_and_leaves_the_viewer_running() {
        let path = std::env::temp_dir().join(format!("acre-gui-test-{}.sock", std::process::id()));
        let link = Link::listen(egui::Context::default(), path.clone()).unwrap();
        let mut app = App::new(None, Recent::default(), Some(link), false);
        assert!(app.editor().is_none());

        let stream = UnixStream::connect(&path).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        let mut editor = BufReader::new(stream);
        let text = |text: &str, tick: u64| {
            let message =
                serde_json::json!({"type": "text", "text": text, "path": "", "tick": tick});
            format!("{message}\n")
        };
        editor
            .get_mut()
            .write_all(text("canvas { size 40 40; box { at 10; } }\n", 1).as_bytes())
            .unwrap();
        let error: serde_json::Value =
            serde_json::from_str(&answer(&mut app, &mut editor)).unwrap();
        assert_eq!(error["type"], "error");
        assert!(error["message"].as_str().unwrap().contains("`at`"));
        assert!(app.editor().is_some());
        assert!(app.doc().linked);

        editor
            .get_mut()
            .write_all(text("canvas { size 40 40; }\n", 2).as_bytes())
            .unwrap();
        assert_eq!(answer(&mut app, &mut editor), "{\"type\":\"ok\"}\n");

        drop(editor);
        let deadline = Instant::now() + Duration::from_secs(5);
        while app.editor().is_some() && Instant::now() < deadline {
            assert!(app.talk(), "the viewer stays open when the editor goes");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.editor().is_none());
        assert!(!app.doc().linked);

        drop(app);
        assert!(!path.exists(), "the socket goes with the viewer");
    }
}

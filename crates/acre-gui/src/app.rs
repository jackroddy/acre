use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use acre::lang;
use acre::scene;
use acre::scene::edit::{self, NewNode, TextEdit};
use una::{
    Align, Answer, Choice, Event, Flow, Id, Key, Layer, Length, Look, MenuEntry, PickFile,
    PickMode, Picked, Rgba, Spec, Style, Surface, TreeRow, TreeSignal, Ui,
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
use crate::properties::number_text;
use crate::recent::Recent;
use crate::selection::{self, Selection};
use crate::snap;
use crate::tabs;
use crate::time_bar::TimeBar;
use crate::tool::{self, Step, Tool};
use crate::view::{MenuItem, Toolbar};

/// How often, in milliseconds, an open file is checked for a change on
/// disk.
const DISK_CHECK: u64 = 500;

/// How often, in milliseconds, a frame is drawn while the scene plays.
const PLAYING: u64 = 16;

/// The status line's reason for an edit refused while the view is locked.
const LOCKED: &str = "locked";

/// The columns of the sections and properties columns.
const COLUMN: u16 = 32;

/// The width of the field over a node that edits its text, at least,
/// in pixels at the chrome style's mockup size.
const FIELD_WIDTH: i32 = 160;

/// The lines of text the field over a node shows before it scrolls.
const FIELD_LINES: i32 = 4;

/// The glyph of the button that closes acre-gui, from the icon font.
const CLOSE_ICON: char = '\u{f0156}';

/// The arrow keys' commands with a selection, and how far each moves it.
const MOVES: [(&str, &str, f64, f64); 4] = [
    ("Move left", "left", -1.0, 0.0),
    ("Move right", "right", 1.0, 0.0),
    ("Move up", "up", 0.0, -1.0),
    ("Move down", "down", 0.0, 1.0),
];

/// The arrow keys' commands with no selection, and the frames each steps.
const STEPS: [(&str, &str, i64); 2] = [
    ("Step back a frame", "left", -1),
    ("Step forward a frame", "right", 1),
];

/// The entries of the File menu.
#[derive(Clone, Copy)]
enum FileItem {
    Open,
    /// The recent file at this index.
    Recent(usize),
    EditInNeovim,
    Quit,
}

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
    /// When the files are next checked, in the milliseconds of
    /// `Ui::now_ms`.
    next_check: u64,
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
    /// The grid's spacing as typed in the toolbar's field.
    spacing: String,
    /// Whether the spacing field had the keys at the last frame.
    spacing_focused: bool,
    /// Whether the panel that lists the scene's sections is open.
    sections_open: bool,
    /// The titles of the sections whose subsections show.
    unfolded: HashSet<String>,
    /// The pane the drawing is drawn in.
    canvas: Surface,
    time_bar: TimeBar,
    /// Whether the file picker shows.
    picking: bool,
    /// Whether the File menu was open at the end of the last frame.
    file_menu_open: bool,
    /// The canvas point under the pointer.
    hover: Option<scene::Point>,
    /// Whether the left button went down on the canvas this frame.
    canvas_pressed: bool,
    /// The latest message that is not a source error.
    notice: Option<String>,
    title: String,
}

impl App {
    /// A viewer of `files` in tabs, or of the last file in `recent`.
    /// With `link`, a viewer of what an editor at its other end sends,
    /// and with a link over stdin and stdout, of that alone. With
    /// `locked`, it refuses edits until the toolbar unlocks it.
    pub fn new(mut files: Vec<PathBuf>, recent: Recent, link: Option<Link>, locked: bool) -> Self {
        let bar = Toolbar::new();
        let spacing = number_text(bar.spacing);
        let mut app = App {
            recent,
            previews: Previews::default(),
            closing: None,
            docs: vec![Document::blank()],
            active: 0,
            next_check: 0,
            link,
            goto: None,
            nudge: Nudge::default(),
            bar,
            spacing,
            spacing_focused: false,
            sections_open: true,
            unfolded: HashSet::new(),
            canvas: Surface::new(),
            time_bar: TimeBar::new(),
            picking: false,
            file_menu_open: false,
            hover: None,
            canvas_pressed: false,
            notice: None,
            title: String::new(),
        };
        app.bar.locked = locked;
        if !app.stdio() {
            if files.is_empty() {
                files.extend(app.recent.files.first().filter(|f| f.exists()).cloned());
            }
            // each file shown clears the status line, so the
            // first failure waits until all are open
            let mut failed = None;
            for file in &files {
                app.open(file);
                failed = failed.or(app.notice.take());
            }
            app.notice = failed;
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
        self.docs[self.active].view.forget();
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
    /// when that changed.
    fn talk(&mut self) {
        let Some(link) = &mut self.link else {
            return;
        };
        let received = link.received();
        if received.closed {
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
            return;
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

    /// Build the open field over its node's objects, in the pane at
    /// `pane` in the window, and write its text once it is left by
    /// Ctrl+Enter, Tab or a click elsewhere. Escape closes it and writes
    /// nothing.
    fn edit_field(&mut self, ui: &mut Ui, pane: crate::screen::Rect) {
        let doc = &mut self.docs[self.active];
        let (Some(f), Some(scene), Some(program)) = (&mut doc.field, &doc.scene, &doc.program)
        else {
            return;
        };
        let bounds = f
            .node(program)
            .and_then(|n| field::bounds(scene, n, doc.time));

        // with no bounds the node draws nothing now, so the field has
        // nothing to sit over and is left
        let mut left = true;
        let mut cancelled = false;
        if let Some((min, max)) = bounds {
            let (min, max) = (doc.view.to_screen(min), doc.view.to_screen(max));
            let style = Style::small(ui.palette());
            let width = ((max.x - min.x) as i32).max(style.px(ui, FIELD_WIDTH));

            // one row holds the editor's padding and its first line
            let line = ui.text_size(" ", style.face).height;
            let height = style.px(ui, style.row) + (FIELD_LINES - 1) * line;
            let at = una::Point {
                x: (pane.min.x + min.x) as i32,
                y: (pane.min.y + min.y) as i32,
            };
            let id = ui.id("text field");
            ui.layer(Layer::Popup, |ui| {
                let spec = Spec {
                    width: Length::Px(width),
                    height: Length::Px(height),
                    float: Some(at),
                    look: Look {
                        fill: Some(ui.palette().background),
                        ..Look::default()
                    },
                    ..Spec::default()
                };
                ui.group(spec, |ui| {
                    if !f.has_had_focus() {
                        ui.focus(id);
                    }
                    let signal = ui.editor(id, &mut f.text);
                    left = f.left(signal.signal.focused) || signal.submitted;
                    cancelled = signal.cancelled;
                });
            });
        }
        if cancelled {
            doc.field = None;
            return;
        }
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

    /// The File menu, with the recent files between Open and Edit in
    /// neovim, and what its entries do. The recent file highlighted shows
    /// its preview beside the menu.
    fn file_menu(&mut self, ui: &mut Ui) {
        let id = ui.id("file menu");
        let button = ui.button("File");

        // on the press, so the press that closes an open menu, which
        // lands outside it, does not open it again on release
        if (button.pressed || button.activated) && !self.file_menu_open {
            let at = una::Point {
                x: button.rect.x,
                y: button.rect.bottom(),
            };
            ui.open_menu(id, at);
        }
        let recent = (0..self.recent.files.len()).map(FileItem::Recent);
        let items: Vec<FileItem> = [FileItem::Open]
            .into_iter()
            .chain(recent)
            .chain([FileItem::EditInNeovim, FileItem::Quit])
            .collect();
        let labels: Vec<String> = (items.iter())
            .map(|item| match item {
                FileItem::Open => "Open…".to_string(),
                FileItem::Recent(i) => self.recent.files[*i].to_string_lossy().into_owned(),
                FileItem::EditInNeovim => "Edit in neovim".to_string(),
                FileItem::Quit => "Quit".to_string(),
            })
            .collect();
        let entries: Vec<MenuEntry> = labels.iter().map(|l| MenuEntry::new(l)).collect();
        let menu = ui.menu(id, &entries);
        self.file_menu_open = menu.selected.is_some();
        if let Some(FileItem::Recent(i)) = menu.selected.map(|i| items[i])
            && let Some(rect) = ui.rect(id)
        {
            let file = self.recent.files[i].clone();
            self.preview_beside(ui, rect, &file);
        }
        match menu.ran.map(|i| items[i]) {
            Some(FileItem::Open) => self.picking = true,
            Some(FileItem::Recent(i)) => {
                let file = self.recent.files[i].clone();
                self.open(&file);
            }
            Some(FileItem::EditInNeovim) => self.edit_in_neovim(),
            Some(FileItem::Quit) => ui.close(),
            None => {}
        }
    }

    /// The preview of `file` in a panel beside `menu`, a rectangle in
    /// the window.
    fn preview_beside(&mut self, ui: &mut Ui, menu: una::Rect, file: &Path) {
        let palette = ui.palette().clone();
        let style = Style::small(&palette);
        let pad = style.px(ui, style.padding);
        let panel = Spec {
            pad: [pad; 4],
            float: Some(una::Point {
                x: menu.right() + pad,
                y: menu.y,
            }),
            radius: style.px(ui, style.radius),
            look: Look {
                fill: Some(palette.raised_background),
                border: Some(palette.border),
                ..Look::default()
            },
            ..Spec::default()
        };
        ui.layer(Layer::Popup, |ui| {
            ui.group(panel, |ui| self.previews.show(ui, file));
        });
    }

    /// The commands of the File menu, which the command palette lists.
    fn file_commands(&mut self, ui: &mut Ui) {
        if self.stdio() {
            return;
        }
        if ui.command("Open…", None) {
            self.picking = true;
        }
        if ui.command("Edit in neovim", None) {
            self.edit_in_neovim();
        }
        if ui.command("Quit", None) {
            ui.close();
        }
    }

    /// Ask for a drawing to open in the file picker while it shows, and
    /// open the one chosen.
    fn pick(&mut self, ui: &mut Ui) {
        if !self.picking {
            return;
        }
        let start = (self.doc().path.as_deref())
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_default();
        let request = PickFile {
            title: "Open a drawing",
            mode: PickMode::Open,
            start: &start,
            line: "",
        };
        let picked = ui.pick_file(ui.id("open"), &request);
        match picked {
            Some(Picked::Chosen(paths)) => {
                self.picking = false;
                if let Some(path) = paths.first() {
                    self.open(path);
                }
            }
            Some(Picked::Cancelled) => self.picking = false,
            None => {}
        }
    }

    /// Open neovim on the drawing shown, linked to this viewer, or say on
    /// the status line why not.
    fn edit_in_neovim(&mut self) {
        let file = self.doc().path.clone();
        let socket = self.link.as_ref().and_then(Link::socket);
        self.notice = match (&file, socket) {
            _ if self.editor().is_some() => Some("an editor is linked already".to_string()),
            (None, _) => Some("this drawing has no file".to_string()),
            (_, None) => Some("acre-gui has no socket for neovim to link to".to_string()),
            (Some(file), Some(socket)) => neovim::open(file, socket).err(),
        };
    }

    /// Take up `tool`, dropping the points placed of any polygon or
    /// polyline being drawn.
    fn set_tool(&mut self, tool: Tool) {
        self.bar.tool = tool;
        for doc in &mut self.docs {
            doc.view.path.clear();
        }
    }

    /// The File menu unless linked over stdio, the buttons that zoom the
    /// render, the snap, grid and lock toggles, and the tools, with Done
    /// and Cancel while a path is being drawn, and the buttons that align
    /// two or more selected nodes and space out three or more. While
    /// locked, only the Select tool is on and no arrange button is.
    fn toolbar(&mut self, ui: &mut Ui) {
        let now = ui.now_ms();
        let gap = |ui: &mut Ui| ui.space(Length::Cells(1));
        let rule = |ui: &mut Ui| {
            ui.space(Length::Cells(1));
            ui.separator();
            ui.space(Length::Cells(1));
        };
        ui.keyed("toolbar", |ui| {
            ui.row(|ui| {
                if !self.stdio() {
                    self.file_menu(ui);
                    rule(ui);
                }
                let out = ui.button("−");
                ui.tooltip(out, "zoom out");
                if out.clicked {
                    self.doc_mut().view.zoom_by(0.8, now);
                }
                let zoom_in = ui.button("+");
                ui.tooltip(zoom_in, "zoom in");
                if zoom_in.clicked {
                    self.doc_mut().view.zoom_by(1.25, now);
                }
                if ui.button("Fit").clicked {
                    self.doc_mut().view.fit();
                }
                let actual = ui.button("100%");
                ui.tooltip(actual, "one pixel on screen to a canvas pixel");
                if actual.clicked {
                    self.doc_mut().view.actual_size(now);
                }
                rule(ui);
                let snap = ui.toggle("Snap", &mut self.bar.snap);
                ui.tooltip(snap, "snap a dragged object to other objects and the grid");
                let grid = ui.toggle("Grid", &mut self.bar.grid);
                ui.tooltip(grid, "draw the grid and snap to it");
                gap(ui);
                self.spacing_field(ui);
                gap(ui);
                let lock = ui.toggle("Lock", &mut self.bar.locked);
                ui.tooltip(lock, "refuse every edit to the source");
                if lock.clicked && self.bar.locked {
                    self.set_tool(Tool::Select);
                }
                rule(ui);
                self.tools(ui);
                let selected = arrange::outermost(self.doc().selection.spans()).len();
                if selected >= 2 {
                    rule(ui);
                    let space: &[Arrange] = if selected >= 3 { &Arrange::SPACE } else { &[] };
                    ui.disabled(self.bar.locked, |ui| {
                        for &how in Arrange::ALIGN.iter().chain(space) {
                            let button = ui.button(how.label());
                            ui.tooltip(button, how.hint());
                            if button.clicked {
                                self.arrange(how);
                            }
                        }
                    });
                }
                ui.space(Length::Fill(1));
                let close = ui.icon_button(ui.id("close"), CLOSE_ICON);
                ui.tooltip(close, "close acre-gui");
                if close.clicked {
                    self.closing = Some(Closing::Window);
                }
            });
        });
    }

    /// The tools, one toggle each, with Done and Cancel while a path is
    /// being drawn.
    fn tools(&mut self, ui: &mut Ui) {
        let locked = self.bar.locked;
        for t in Tool::ALL {
            let mut on = self.bar.tool == t;
            let mut clicked = false;
            ui.disabled(locked && t != Tool::Select, |ui| {
                let toggle = ui.toggle(t.label(), &mut on);
                ui.tooltip(toggle, t.hint());
                clicked = toggle.clicked;
            });
            if clicked {
                self.set_tool(t);
            }
        }
        if self.bar.tool.draws_path() && !self.doc().view.path.is_empty() {
            ui.space(Length::Cells(1));
            if ui.button("Done").clicked {
                self.finish_path();
            }
            if ui.button("Cancel").clicked {
                self.set_tool(Tool::Select);
            }
        }
    }

    /// The field that sets the grid's spacing in canvas pixels. It takes
    /// a number from 1 to 1000 on Enter or once it loses the keys, and
    /// puts back the spacing for anything else.
    fn spacing_field(&mut self, ui: &mut Ui) {
        let spec = Spec {
            width: Length::Cells(6),
            ..Spec::default()
        };
        let mut signal = None;
        ui.group(spec, |ui| {
            signal = Some(ui.field(ui.id("spacing"), &mut self.spacing, "px"));
        });
        let Some(signal) = signal else { return };
        ui.tooltip(signal.signal, "the grid's spacing in canvas pixels");
        ui.label(" px");
        let focused = signal.signal.focused;
        let left = self.spacing_focused && !focused;
        self.spacing_focused = focused;
        if signal.submitted || left || signal.cancelled {
            let typed = self.spacing.trim().parse::<f64>().ok();
            if let Some(spacing) = typed.filter(|s| (1.0..=1000.0).contains(s) && !signal.cancelled)
            {
                self.bar.spacing = spacing;
            }
            self.spacing = number_text(self.bar.spacing);
        }
    }

    /// Ask whether to close what `closing` names, and close it on
    /// Close. Cancel or Escape keeps it open.
    fn confirm_close(&mut self, ui: &mut Ui) {
        let Some(closing) = self.closing else { return };
        let question = match closing {
            Closing::Window => "Close acre-gui?".to_string(),
            Closing::Tab(i) => format!("Close {}?", self.docs[i].name()),
        };
        let choices = [
            Choice {
                label: "Close",
                destructive: true,
            },
            Choice {
                label: "Cancel",
                destructive: false,
            },
        ];
        let Some(answer) = ui.dialog(ui.id("close dialog"), &question, "", &choices) else {
            return;
        };
        self.closing = None;
        match closing {
            Closing::Window if answer == Answer::Choice(0) => ui.close(),
            Closing::Tab(i) if answer == Answer::Choice(0) => self.close_tab(i),
            _ => {}
        }
    }

    /// One tab for each drawing open, by its file's name, with a close
    /// mark that asks to close it, and the file's path in a tooltip. A
    /// click on a tab shows its drawing.
    /// The blank drawing has no tab.
    fn tabs(&mut self, ui: &mut Ui) {
        let open: Vec<usize> = (0..self.docs.len())
            .filter(|&i| !self.docs[i].blank)
            .collect();
        if open.is_empty() {
            return;
        }
        let names: Vec<String> = open.iter().map(|&i| self.docs[i].name()).collect();
        let labels: Vec<&str> = names.iter().map(String::as_str).collect();
        let mut shown = open.iter().position(|&i| i == self.active).unwrap_or(0);
        let signal = ui.tabs(ui.id("tabs"), &labels, &mut shown, true);
        if let Some((tab, pill)) = signal.hovered
            && let Some(path) = &self.docs[open[tab]].path
        {
            ui.tooltip(pill, &path.to_string_lossy());
        }
        if let Some(closed) = signal.closed {
            self.closing = Some(Closing::Tab(open[closed]));
        } else if open[shown] != self.active {
            self.activate(open[shown]);
        }
    }

    /// The keys, as commands that una keeps from a text field, a menu, a
    /// dialog or the file picker with the keys: Space plays and pauses,
    /// with a selection the arrow keys move it a canvas pixel, and with
    /// none Left and Right step a frame. Delete and Backspace delete the
    /// selection.
    fn keys(&mut self, ui: &mut Ui) {
        self.commands(ui);
        if let Some((dx, dy)) = self.nudge.ready() {
            let nodes = self.doc().selection.spans().to_vec();
            if !nodes.is_empty() {
                self.move_nodes(&nodes, scene::Point::new(dx, dy));
            }
        }
    }

    fn commands(&mut self, ui: &mut Ui) {
        let now = Instant::now();
        if ui.command("Play or pause", Some("space")) {
            let playback = &mut self.doc_mut().playback;
            if playback.duration() > 0.0 {
                playback.toggle(now);
            }
        }
        if self.doc().selection.spans().is_empty() {
            for (title, chord, frames) in STEPS {
                let doc = self.doc_mut();
                if ui.command(title, Some(chord))
                    && let Some(fps) = doc.scene.as_ref().map(|s| s.canvas.fps)
                    && doc.playback.duration() > 0.0
                {
                    doc.playback.step(now, frames, fps);
                }
            }
        } else {
            for (title, chord, dx, dy) in MOVES {
                if ui.command(title, Some(chord)) && !self.refuse_locked() {
                    self.nudge.press(dx, dy);
                }
            }
        }

        // both declared every frame, so neither hides the other's chord
        let delete = ui.command("Delete", Some("delete"));
        let backspace = ui.command("Delete with Backspace", Some("backspace"));
        if delete || backspace {
            self.delete();
        }
    }

    /// Ctrl+C copies the selection and Ctrl+V pastes, read from the raw
    /// keys, since a command's chord would take them from a text field.
    /// They do nothing while a text field has the keys or a menu, dialog
    /// or picker is open.
    fn clipboard_keys(&mut self, ui: &mut Ui) {
        // before the build, so both answer for the boxes the keys
        // were routed to
        if ui.typing() || ui.overlay_open() {
            return;
        }
        let pressed: Vec<Key> = (ui.events().iter())
            .filter_map(|e| match e {
                Event::Key(key)
                    if key.pressed
                        && key.mods.ctrl
                        && !key.mods.shift
                        && !key.mods.alt
                        && !key.mods.logo =>
                {
                    Some(key.key)
                }
                _ => None,
            })
            .collect();
        for key in pressed {
            match key {
                Key::Char('c') => self.copy(ui),
                Key::Char('v') => {
                    if let Some(text) = ui.clipboard().map(str::to_string) {
                        self.paste(&text);
                    }
                }
                _ => {}
            }
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
    fn copy(&mut self, ui: &mut Ui) {
        let nodes = self.doc().selection.spans().to_vec();
        if nodes.is_empty() {
            return;
        }
        match self.on_source("copy", |text| edit::copy_text(text, &nodes)) {
            Ok(text) => {
                self.notice = None;
                ui.set_clipboard(&text);
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
                let close = view.reach(tool::CLOSE);
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
    fn transport(&mut self, ui: &mut Ui) {
        let now = Instant::now();
        let doc = &mut self.docs[self.active];
        let duration = doc.playback.duration();
        let sections = doc.scene.as_ref().map_or(&[][..], |s| &s.sections);
        ui.keyed("transport", |ui| {
            ui.row(|ui| {
                let label = if doc.playback.playing() {
                    "Pause"
                } else {
                    "Play"
                };
                let play = ui.button(label);
                ui.tooltip(play, "space");
                if play.clicked {
                    doc.playback.toggle(now);
                }
                ui.space(Length::Cells(1));
                if let Some(t) = self.time_bar.show(ui, sections, duration, doc.time) {
                    doc.playback.seek(now, t);
                }
                ui.space(Length::Cells(1));
                ui.label(&format!("{:.2} s / {duration:.2} s", doc.time));
                ui.space(Length::Cells(1));
                let mut looping = doc.playback.looping();
                if ui.toggle("Loop", &mut looping).clicked {
                    doc.playback.set_looping(now, looping);
                }
                if !sections.is_empty() {
                    ui.toggle("Sections", &mut self.sections_open);
                }
            });
        });
        doc.time = doc.playback.time(now);
    }

    /// The scene's sections in a tree, each with its start, the current
    /// one selected. Selecting another, or Enter or a double click on
    /// one, goes to its start. A section with subsections folds them
    /// away.
    fn sections(&mut self, ui: &mut Ui) {
        let doc = &mut self.docs[self.active];
        let Some(scene) = &doc.scene else { return };
        let current = playback::current(&scene.sections, doc.time);

        // the rows shown, and for each its section and its start and text
        let mut rows = Vec::new();
        let mut shown: Vec<(usize, f64, String)> = Vec::new();
        let mut here = usize::MAX;
        for (i, section) in scene.sections.iter().enumerate() {
            let open = self.unfolded.contains(&section.title);
            if current == Some(i) {
                here = rows.len();
            }
            rows.push(TreeRow {
                depth: 0,
                children: !section.subsections.is_empty(),
                open,
            });
            let text = format!("{:.2} s  {}", section.start, section.title);
            shown.push((i, section.start, text));
            if !open {
                continue;
            }
            let sub =
                playback::current_subsection(section, doc.time).filter(|_| current == Some(i));
            for (j, s) in section.subsections.iter().enumerate() {
                if sub == Some(j) {
                    here = rows.len();
                }
                rows.push(TreeRow {
                    depth: 1,
                    ..TreeRow::default()
                });
                shown.push((i, s.start, format!("{:.2} s  {}", s.start, s.title)));
            }
        }

        let style = Style::small(ui.palette());
        let pad = style.px(ui, style.padding);
        let spec = Spec {
            id: ui.id("sections"),
            width: Length::Cells(COLUMN),
            height: Length::Fill(1),
            pad: [pad; 4],
            ..Spec::default()
        };
        let mut selected = here;
        let mut tree = TreeSignal::default();
        ui.scroll(spec, |ui| {
            let height = rows.len() as u16;
            tree = ui.tree(ui.id("tree"), &rows, height, &mut selected, |ui, i| {
                ui.label(&shown[i].2);
            });
        });
        let chosen = tree.activated.or((selected != here).then_some(selected));
        if let Some(&(_, start, _)) = chosen.and_then(|i| shown.get(i)) {
            doc.playback.seek(Instant::now(), start);
        }

        // by title, so a section stays open or folded when an edit moves
        // its start
        if let Some(i) = tree.toggled {
            let title = &scene.sections[shown[i].0].title;
            if !self.unfolded.remove(title) {
                self.unfolded.insert(title.clone());
            }
        }
    }

    /// The column that changes the selected nodes' colors, strokes and
    /// fonts.
    fn properties(&mut self, ui: &mut Ui) {
        let locked = self.bar.locked;
        let doc = &mut self.docs[self.active];
        let style = Style::small(ui.palette());
        let pad = style.px(ui, style.padding);
        let spec = Spec {
            id: ui.id("properties"),
            width: Length::Cells(COLUMN),
            height: Length::Fill(1),
            pad: [pad; 4],
            ..Spec::default()
        };
        ui.scroll(spec, |ui| {
            // with an error, the scene shown was built from an earlier
            // text, so the selection's spans do not point into `text`
            if doc.error.is_some() {
                ui.label("fix the source's error first");
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
            doc.properties.ui(ui, doc.selection.paths(), locked);
        });
    }

    /// Write the first change in the properties column not yet written,
    /// once the editor has taken the last edit.
    fn write_property(&mut self) {
        if self.nudge.waiting() {
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

        // a refusal or a change that made no edit leaves the column
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

    fn status(&self, ui: &mut Ui) {
        let doc = self.doc();
        let palette = ui.palette().clone();
        let gap = |ui: &mut Ui| {
            ui.space(Length::Cells(1));
            ui.separator();
            ui.space(Length::Cells(1));
        };
        ui.keyed("status", |ui| {
            ui.row(|ui| {
                let where_ = doc
                    .path
                    .as_ref()
                    .map_or("untitled".into(), |p| p.display().to_string());
                ui.label(&where_);
                gap(ui);
                ui.label(&format!("{:.0}%", doc.view.zoom() * 100.0));
                gap(ui);
                if let Some(p) = self.hover {
                    ui.label(&format!("{:.0}, {:.0}", p.x, p.y));
                    gap(ui);
                }
                if let Some(e) = &doc.error {
                    let (line, col) = e.line_col(&doc.text);
                    colored(ui, &format!("{line}:{col}: {}", e.message), palette.red);
                } else if let Some(notice) = &self.notice {
                    ui.label(notice);
                } else if let Some(&c) = doc.missing.first() {
                    let more = match doc.missing.len() - 1 {
                        0 => String::new(),
                        n => format!(", and {n} more"),
                    };
                    let text = format!("{}{more}", document::missing_glyph(c));
                    colored(ui, &text, palette.yellow);
                }
            });
        });
    }

    /// The render pane: the drawing shown, and what the person did to it
    /// this frame. The blank drawing shows how to open one.
    fn canvas(&mut self, ui: &mut Ui) {
        let doc = &mut self.docs[self.active];
        if doc.blank {
            let text = if self.stdio() {
                ""
            } else {
                "Open a drawing with File › Open"
            };
            let muted = ui.palette().muted;
            ui.leaf(Spec {
                text,
                width: Length::Fill(1),
                height: Length::Fill(1),
                align: Align::Center,
                pad: [0, 0, 0, 0],
                look: Look {
                    text: Some(muted),
                    ..Look::default()
                },
                ..Spec::default()
            });
            self.hover = None;
            self.canvas_pressed = false;
            return;
        }
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
        let style = Style::small(ui.palette());
        let unit = style.px(ui, style.mockup) as f32 / style.mockup as f32;
        let shown = doc.view.show(
            ui,
            &mut self.canvas,
            unit,
            &self.bar,
            doc.scene.as_mut(),
            doc.version,
            doc.time,
            doc.selection.spans(),
        );
        self.hover = shown.hover;
        self.canvas_pressed = shown.pane_pressed;
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

    /// Build one frame of the window.
    pub fn frame(&mut self, ui: &mut Ui) {
        self.talk();
        let now = ui.now_ms();
        if now >= self.next_check {
            self.next_check = now + DISK_CHECK;
            self.check_disk();
        }
        ui.wake_at(self.next_check);
        let title = format!("{} - acre", self.doc().name());
        if title != self.title {
            ui.set_title(&title);
            self.title = title;
        }
        self.file_commands(ui);
        self.keys(ui);
        self.clipboard_keys(ui);
        let doc = self.doc_mut();
        doc.time = doc.playback.time(Instant::now());

        let fill = Spec {
            width: Length::Fill(1),
            height: Length::Fill(1),
            ..Spec::default()
        };
        ui.group(fill, |ui| {
            self.toolbar(ui);
            self.tabs(ui);
            ui.separator();
            let body = Spec {
                flow: Flow::Row,
                ..fill
            };
            ui.group(body, |ui| {
                let doc = self.doc();
                let has_sections = doc.scene.as_ref().is_some_and(|s| !s.sections.is_empty());
                if self.sections_open && has_sections {
                    self.sections(ui);
                    ui.separator();
                }
                self.canvas(ui);
                let doc = self.doc();
                if !doc.selection.spans().is_empty() && doc.selection.made_in_viewer() {
                    self.write_property();
                    ui.separator();
                    self.properties(ui);
                }
            });
            ui.separator();
            if self.doc().playback.duration() > 0.0 {
                self.transport(ui);
            }
            self.status(ui);
        });
        self.pick(ui);
        self.confirm_close(ui);

        // after the build, since the colour picker hands focus back
        // to its swatch when a press outside closes it
        if self.canvas_pressed {
            ui.focus(Id::NONE);
        }
        if self.doc().playback.playing() {
            ui.wake_at(ui.now_ms() + PLAYING);
        }
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

/// A label of `text` in `color`, one row of the small style high.
pub fn colored(ui: &mut Ui, text: &str, color: Rgba) {
    let style = Style::small(ui.palette());
    let room = (style.px(ui, style.row) - style.px(ui, style.mockup)).max(0);
    ui.leaf(Spec {
        text,
        width: Length::Text,
        height: Length::Text,
        pad: [0, room / 2, 0, room - room / 2],
        look: Look {
            text: Some(color),
            ..Look::default()
        },
        ..Spec::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, ErrorKind, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use std::time::Duration;

    /// The next line the viewer sends to `editor`, with the viewer
    /// taking messages meanwhile.
    fn answer(app: &mut App, editor: &mut BufReader<UnixStream>) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut line = String::new();
        while Instant::now() < deadline {
            app.talk();
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
    fn each_file_named_opens_in_a_tab_with_the_last_shown_and_a_failure_named() {
        let dir = std::env::temp_dir().join(format!("acre-gui-test-tabs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let [a, missing, b] = ["a.acre", "missing.acre", "b.acre"].map(|f| dir.join(f));
        fs::write(&a, "canvas { size 40 40; }\n").unwrap();
        fs::write(&b, "canvas { size 40 40; }\n").unwrap();

        let app = App::new(
            vec![a.clone(), missing, b.clone()],
            Recent::default(),
            None,
            false,
        );
        fs::remove_dir_all(&dir).unwrap();
        let open: Vec<_> = app.docs.iter().filter_map(|d| d.path.clone()).collect();
        assert_eq!(open, [a, b.clone()]);
        assert_eq!(app.doc().path.as_ref(), Some(&b));
        assert!(app.notice.unwrap().contains("missing.acre"));
    }

    #[test]
    fn an_editor_on_the_socket_gets_answers_and_leaves_the_viewer_running() {
        let path = std::env::temp_dir().join(format!("acre-gui-test-{}.sock", std::process::id()));
        let link = Link::listen(Arc::new(|| {}), path.clone()).unwrap();
        let mut app = App::new(Vec::new(), Recent::default(), Some(link), false);
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
            app.talk();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            app.editor().is_none(),
            "the viewer stays open when the editor goes"
        );
        assert!(!app.doc().linked);

        drop(app);
        assert!(!path.exists(), "the socket goes with the viewer");
    }
}

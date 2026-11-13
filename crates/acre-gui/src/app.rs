use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use acre::lang::{self, Highlight, Span};
use acre::scene::{self, Scene};
use eframe::egui::{
    self, Color32, FontId, Key, KeyboardShortcut, Modifiers, Stroke, Vec2, ViewportCommand,
    text::{LayoutJob, TextFormat},
};

use crate::config::Config;
use crate::editor;
use crate::recent::Recent;
use crate::view::View;

const STARTER: &str = "canvas {
    size 640 360;

    box hello { at 240 150; size 160 60; text \"hello\"; }
}
";

const NEW: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::N);
const OPEN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::O);
const SAVE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::S);
const SAVE_AS: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::S);
const QUIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Q);
const FORMAT: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::F);
const ZOOM_IN: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Equals);
const ZOOM_OUT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Minus);
const FIT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);
const ACTUAL_SIZE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num1);
const COMMENT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Slash);
const FIND: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::F);
const COMPLETE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Space);
const REFERENCE: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F1);

/// How often the open file is checked for a change on disk.
const DISK_CHECK: Duration = Duration::from_millis(500);

/// Something that replaces the open drawing, held while the person
/// decides what to do with unsaved changes.
#[derive(Clone)]
enum Replace {
    New,
    Open(Option<PathBuf>),
    Quit,
}

pub struct App {
    config: Config,
    recent: Recent,
    path: Option<PathBuf>,
    text: String,
    /// The text as last opened or saved.
    saved: String,
    /// When the file was last read or written, as the disk reports it.
    modified: Option<SystemTime>,
    last_check: Instant,
    error: Option<lang::Error>,
    /// The last scene that built, kept while the text has an error.
    scene: Option<Scene>,
    /// Counts the scenes built, so the view draws again on a new one.
    version: u64,
    view: View,
    /// The render pane's size at the last frame.
    pane: Vec2,
    /// The editor cursor's byte offset into the text.
    cursor: Option<usize>,
    /// The editor's selection, in bytes, from its start to its end.
    selection: Option<std::ops::Range<usize>>,
    /// The find bar's text and replacement, while it is open.
    find: Option<(String, String)>,
    /// Whether the reference panel is open.
    reference: bool,
    /// The canvas point under the pointer.
    hover: Option<scene::Point>,
    /// The latest message that is not a source error.
    notice: Option<String>,
    pending: Option<Replace>,
    rendered: bool,
    quitting: bool,
    title: String,
}

impl App {
    pub fn new(config: Config, config_error: Option<String>, file: Option<PathBuf>) -> Self {
        let mut app = App {
            config,
            recent: Recent::load(),
            path: None,
            text: STARTER.to_string(),
            saved: STARTER.to_string(),
            modified: None,
            last_check: Instant::now(),
            error: None,
            scene: None,
            version: 0,
            view: View::new(),
            pane: Vec2::ZERO,
            cursor: None,
            selection: None,
            find: None,
            reference: false,
            hover: None,
            notice: config_error,
            pending: None,
            rendered: false,
            quitting: false,
            title: String::new(),
        };
        match file {
            Some(file) if !file.exists() => {
                app.path = Some(file);
                app.saved = String::new();
            }
            Some(file) => app.open(&file),
            None => {
                if let Some(last) = app.recent.files.first().filter(|f| f.exists()).cloned() {
                    app.open(&last);
                }
            }
        }
        app
    }

    fn dirty(&self) -> bool {
        self.text != self.saved
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
                self.saved = text.clone();
                self.text = text;
                self.path = Some(path.to_path_buf());
                self.modified = modified(path);
                self.rendered = false;
                self.notice = self.recent.touch(path).err();
            }
            Err(e) => self.notice = Some(format!("cannot open {}: {e}", path.display())),
        }
    }

    /// Write the drawing to its file, asking for one if it has none.
    /// False when nothing was written.
    fn save(&mut self, choose: bool) -> bool {
        let path = match (&self.path, choose) {
            (Some(path), false) => path.clone(),
            _ => {
                let mut dialog = rfd::FileDialog::new()
                    .add_filter("acre drawing", &["acre"])
                    .set_file_name(format!("{}.acre", self.name().trim_end_matches(".acre")));
                if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
                    dialog = dialog.set_directory(dir);
                }
                match dialog.save_file() {
                    Some(path) => path,
                    None => return false,
                }
            }
        };
        if let Err(e) = fs::write(&path, &self.text) {
            self.notice = Some(format!("cannot write {}: {e}", path.display()));
            return false;
        }
        self.saved = self.text.clone();
        self.modified = modified(&path);
        self.notice = self.recent.touch(&path).err();
        self.path = Some(path);
        true
    }

    fn request(&mut self, replace: Replace) {
        if self.dirty() {
            self.pending = Some(replace);
        } else {
            self.replace(replace);
        }
    }

    fn replace(&mut self, replace: Replace) {
        match replace {
            Replace::New => {
                self.path = None;
                self.text = STARTER.to_string();
                self.saved = STARTER.to_string();
                self.rendered = false;
            }
            Replace::Open(Some(path)) => self.open(&path),
            Replace::Open(None) => {
                let mut dialog = rfd::FileDialog::new().add_filter("acre drawing", &["acre"]);
                if let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) {
                    dialog = dialog.set_directory(dir);
                }
                if let Some(path) = dialog.pick_file() {
                    self.open(&path);
                }
            }
            Replace::Quit => self.quitting = true,
        }
    }

    fn render(&mut self) {
        self.rendered = true;
        match lang::parse(&self.text).and_then(|program| scene::build(&program)) {
            Ok(scene) => {
                self.error = None;
                self.scene = Some(scene);
                self.version += 1;
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Read the file again if it changed on disk. With unsaved edits it
    /// stays as it is, and the status line says so.
    fn check_disk(&mut self) {
        self.last_check = Instant::now();
        let Some(path) = self.path.clone() else {
            return;
        };
        let on_disk = modified(&path);
        if on_disk.is_none() || on_disk == self.modified {
            return;
        }
        if self.dirty() {
            self.modified = on_disk;
            self.notice = Some("the file changed on disk; saving will overwrite it".into());
            return;
        }
        if let Ok(text) = fs::read_to_string(&path) {
            self.modified = on_disk;
            if text != self.text {
                self.saved = text.clone();
                self.text = text;
                self.rendered = false;
                self.notice = Some("reloaded from disk".into());
            }
        }
    }

    /// Rewrite the text in acre's one layout, unless it does not parse,
    /// when the status line already shows why.
    fn format(&mut self) {
        if let Ok(formatted) = lang::format(&self.text)
            && formatted != self.text
        {
            self.text = formatted;
            self.rendered = false;
        }
    }

    /// The source editor: line numbers beside the text, with the
    /// editor's own touches after each edit.
    fn editor(&mut self, ui: &mut egui::Ui) {
        let error = self.error.as_ref().map(|e| e.span);
        let size = self.config.editor_font_size;
        let braces = self
            .cursor
            .and_then(|c| editor::matching_brace(&self.text, c));
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let mut job = layout(buf.as_str(), error, braces, size);
            job.wrap.max_width = f32::INFINITY;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };
        let old = self.text.clone();
        egui::ScrollArea::both().show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let lines = self.text.split('\n').count();
                let width = lines.to_string().len();
                let numbers: String = (1..=lines)
                    .map(|n| format!("{n:>width$}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(numbers)
                            .font(FontId::monospace(size))
                            .color(Color32::from_rgb(0x56, 0x5f, 0x89)),
                    )
                    .selectable(false),
                );
                let output = egui::TextEdit::multiline(&mut self.text)
                    .id(editor_id())
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(40)
                    .layouter(&mut layouter)
                    .show(ui);
                let byte = |text: &str, chars: usize| {
                    text.char_indices()
                        .nth(chars)
                        .map_or(text.len(), |(b, _)| b)
                };
                self.cursor = output
                    .cursor_range
                    .map(|range| byte(&self.text, range.primary.index.0));
                self.selection = output.cursor_range.map(|range| {
                    let r = range.as_sorted_char_range();
                    byte(&self.text, r.start.0)..byte(&self.text, r.end.0)
                });
                if output.response.changed() {
                    self.rendered = false;
                    if let Some(cursor) = self.cursor
                        && let Some((text, cursor)) = editor::after_edit(&old, &self.text, cursor)
                    {
                        self.text = text;
                        self.select(ui.ctx(), cursor..cursor);
                    }
                }
            });
        });
    }

    /// Set the editor's selection to the bytes `range` and give it the
    /// keyboard.
    fn select(&mut self, ctx: &egui::Context, range: std::ops::Range<usize>) {
        let chars = |byte: usize| self.text[..byte.min(self.text.len())].chars().count();
        let id = editor_id();
        let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
        state
            .cursor
            .set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(chars(range.start)),
                egui::text::CCursor::new(chars(range.end)),
            )));
        state.store(ctx, id);
        ctx.memory_mut(|m| m.request_focus(id));
        self.cursor = Some(range.end);
        self.selection = Some(range);
    }

    /// The find bar: a text to find, its replacement, and buttons to step
    /// through matches and replace them.
    fn find_bar(&mut self, ui: &mut egui::Ui) {
        let Some((mut needle, mut replacement)) = self.find.take() else {
            return;
        };
        let mut close = false;
        let mut next = false;
        let mut replace_one = false;
        let mut replace_all = false;
        ui.horizontal(|ui| {
            let find = ui.add(egui::TextEdit::singleline(&mut needle).hint_text("find"));
            if find.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                next = true;
                find.request_focus();
            }
            next |= ui.button("Next").clicked();
            ui.add(egui::TextEdit::singleline(&mut replacement).hint_text("replace with"));
            replace_one = ui.button("Replace").clicked();
            replace_all = ui.button("All").clicked();
            close = ui.button("×").clicked() || ui.input(|i| i.key_pressed(Key::Escape));
        });
        let ctx = ui.ctx().clone();
        if !needle.is_empty() {
            let selected = self
                .selection
                .clone()
                .filter(|r| self.text.get(r.clone()) == Some(needle.as_str()));
            if replace_one && let Some(r) = selected {
                self.text.replace_range(r.clone(), &replacement);
                self.rendered = false;
                self.select(&ctx, r.start..r.start + replacement.len());
                next = true;
            }
            if replace_all {
                let count = self.text.matches(needle.as_str()).count();
                self.text = self.text.replace(needle.as_str(), &replacement);
                self.rendered = false;
                self.notice = Some(format!("replaced {count}"));
            }
            if next {
                let from = self.selection.as_ref().map_or(0, |r| r.end);
                let found = self.text[from..]
                    .find(needle.as_str())
                    .map(|i| from + i)
                    .or_else(|| self.text.find(needle.as_str()));
                match found {
                    Some(at) => self.select(&ctx, at..at + needle.len()),
                    None => self.notice = Some(format!("`{needle}` is not in the text")),
                }
            }
        }
        if !close {
            self.find = Some((needle, replacement));
        }
    }

    /// The reference panel: the node kind the cursor sits in and every
    /// property it takes, with the one under the cursor in bold.
    fn reference(&self, ui: &mut egui::Ui) {
        let ctx = self
            .cursor
            .map(|c| lang::context_at(&self.text, c))
            .unwrap_or_default();
        let kind = ctx
            .nodes
            .last()
            .and_then(|n| scene::schema::node(n))
            .or_else(|| scene::schema::node("canvas"));
        egui::ScrollArea::vertical().show(ui, |ui| {
            let Some(kind) = kind else { return };
            ui.heading(kind.name);
            ui.label(kind.about);
            ui.separator();
            let current = ctx
                .key
                .clone()
                .or_else(|| ctx.word.as_ref().map(|(_, w)| w.clone()));
            for p in kind.properties {
                let mut name = egui::RichText::new(format!("{} {}", p.name, p.takes)).monospace();
                if current.as_deref() == Some(p.name) {
                    name = name.strong().color(Color32::from_rgb(0xff, 0x9e, 0x64));
                }
                ui.label(name);
                ui.label(egui::RichText::new(p.about).weak());
                ui.add_space(4.0);
            }
            if !kind.children.is_empty() {
                ui.separator();
                ui.label(format!("holds {}", kind.children.join(", ")));
            }
        });
    }

    /// Complete the word before the cursor as far as every candidate
    /// agrees, and list the candidates when more than one is left.
    fn complete(&mut self, ctx: &egui::Context) {
        let Some(cursor) = self.cursor else { return };
        let Some(completion) = editor::complete(&self.text, cursor) else {
            self.notice = Some("nothing to complete".into());
            return;
        };
        let prefix = editor::common_prefix(&completion.candidates);
        let start = completion.replace.start;
        if prefix.len() > completion.replace.len() {
            self.text.replace_range(completion.replace.clone(), prefix);
            self.rendered = false;
            self.select(ctx, start + prefix.len()..start + prefix.len());
        }
        if completion.candidates.len() > 1 {
            self.notice = Some(completion.candidates.join("  "));
        }
    }

    /// Comment the selected lines out, or back in.
    fn toggle_comment(&mut self, ctx: &egui::Context) {
        let Some(selection) = self.selection.clone() else {
            return;
        };
        let (text, selection) = editor::toggle_comment(&self.text, selection);
        self.text = text;
        self.rendered = false;
        self.select(ctx, selection);
    }

    /// The boxes of the objects the node under the editor's cursor drew.
    /// The innermost node holding the cursor wins, so a cursor inside a
    /// group's child marks that child.
    fn marks(&self) -> Vec<(scene::Point, scene::Point)> {
        let (Some(scene), Some(cursor)) = (&self.scene, self.cursor) else {
            return Vec::new();
        };
        let holds =
            |span: &Span| span.start < span.end && span.start <= cursor && cursor <= span.end;
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
            .filter_map(|i| scene.bounds(i, 0.0))
            .collect()
    }

    /// Select the node that drew the object at canvas `point` in the
    /// editor.
    fn select_object_at(&mut self, ctx: &egui::Context, point: scene::Point) {
        let Some(scene) = &self.scene else { return };
        let Some(index) = scene.object_at(point, 0.0) else {
            return;
        };
        let span = scene.objects[index].source;
        if span.end > self.text.len() || !self.text.is_char_boundary(span.start) {
            return;
        }
        self.select(ctx, span.start..span.end);
    }

    fn view_shortcut(&mut self, shortcut: KeyboardShortcut) {
        if shortcut == ZOOM_IN {
            self.view.zoom_by(1.25, self.pane);
        } else if shortcut == ZOOM_OUT {
            self.view.zoom_by(0.8, self.pane);
        } else if shortcut == FIT {
            self.view.fit();
        } else {
            self.view.actual_size(self.pane);
        }
    }

    fn menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New").clicked() {
                    self.request(Replace::New);
                    ui.close();
                }
                if ui.button("Open…").clicked() {
                    self.request(Replace::Open(None));
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
                        self.request(Replace::Open(Some(file)));
                        ui.close();
                    }
                });
                ui.separator();
                if ui.button("Save").clicked() {
                    self.save(false);
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    self.save(true);
                    ui.close();
                }
                ui.separator();
                if ui.button("Quit").clicked() {
                    self.request(Replace::Quit);
                    ui.close();
                }
            });
            ui.menu_button("View", |ui| {
                let entries = [
                    ("Zoom In", ZOOM_IN),
                    ("Zoom Out", ZOOM_OUT),
                    ("Fit", FIT),
                    ("Actual Size", ACTUAL_SIZE),
                ];
                let label = format!("Reference ({})", ui.ctx().format_shortcut(&REFERENCE));
                if ui.checkbox(&mut self.reference, label).clicked() {
                    ui.close();
                }
                for (name, shortcut) in entries {
                    let label = format!("{name} ({})", ui.ctx().format_shortcut(&shortcut));
                    if ui.button(label).clicked() {
                        self.view_shortcut(shortcut);
                        ui.close();
                    }
                }
            });
            ui.menu_button("Edit", |ui| {
                let label = format!("Format ({})", ui.ctx().format_shortcut(&FORMAT));
                if ui.button(label).clicked() {
                    self.format();
                    ui.close();
                }
                let label = format!("Find ({})", ui.ctx().format_shortcut(&FIND));
                if ui.button(label).clicked() {
                    self.find.get_or_insert_default();
                    ui.close();
                }
                let label = format!("Toggle Comment ({})", ui.ctx().format_shortcut(&COMMENT));
                if ui.button(label).clicked() {
                    let ctx = ui.ctx().clone();
                    self.toggle_comment(&ctx);
                    ui.close();
                }
                let label = format!("Complete ({})", ui.ctx().format_shortcut(&COMPLETE));
                if ui.button(label).clicked() {
                    let ctx = ui.ctx().clone();
                    self.complete(&ctx);
                    ui.close();
                }
            });
        });
    }

    fn status(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let modified = if self.dirty() { " (modified)" } else { "" };
            let where_ = self
                .path
                .as_ref()
                .map_or("untitled".into(), |p| p.display().to_string());
            ui.label(format!("{where_}{modified}"));
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
            }
        });
    }

    fn unsaved_prompt(&mut self, ctx: &egui::Context) {
        let Some(replace) = self.pending.clone() else {
            return;
        };
        let name = self.name();
        let mut choice = None;
        let modal = egui::Modal::new(egui::Id::unique("unsaved")).show(ctx, |ui| {
            ui.label(format!("Save changes to {name}?"));
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    choice = Some(true);
                }
                if ui.button("Discard").clicked() {
                    choice = Some(false);
                }
                if ui.button("Cancel").clicked() {
                    ui.close();
                }
            });
        });
        match choice {
            Some(save) => {
                self.pending = None;
                if !save || self.save(false) {
                    self.replace(replace);
                }
            }
            None if modal.should_close() => self.pending = None,
            None => {}
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let (new, open, save_as, save, quit, format) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&NEW),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&SAVE_AS),
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&QUIT),
                i.consume_shortcut(&FORMAT),
            )
        });
        if format {
            self.format();
        }
        if ctx.input_mut(|i| i.consume_shortcut(&COMMENT)) {
            self.toggle_comment(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&COMPLETE)) {
            self.complete(ctx);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&FIND)) {
            let selected = self
                .selection
                .clone()
                .and_then(|r| self.text.get(r).map(str::to_string))
                .filter(|s| !s.is_empty() && !s.contains('\n'));
            let (needle, replacement) = self.find.take().unwrap_or_default();
            self.find = Some((selected.unwrap_or(needle), replacement));
        }
        if ctx.input_mut(|i| i.consume_shortcut(&REFERENCE)) {
            self.reference = !self.reference;
        }
        for shortcut in [ZOOM_IN, ZOOM_OUT, FIT, ACTUAL_SIZE] {
            if ctx.input_mut(|i| i.consume_shortcut(&shortcut)) {
                self.view_shortcut(shortcut);
            }
        }
        if self.last_check.elapsed() >= DISK_CHECK {
            self.check_disk();
        }
        ctx.request_repaint_after(DISK_CHECK);
        if new {
            self.request(Replace::New);
        }
        if open {
            self.request(Replace::Open(None));
        }
        if save_as {
            self.save(true);
        } else if save {
            self.save(false);
        }
        if quit {
            self.request(Replace::Quit);
        }

        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.request(Replace::Quit);
        }
        if self.quitting {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        if !self.rendered {
            self.render();
        }
        let title = format!(
            "{}{} - acre",
            self.name(),
            if self.dirty() { "*" } else { "" }
        );
        if title != self.title {
            ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.title = title;
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));

        if self.reference {
            egui::Panel::right("reference")
                .resizable(true)
                .default_size(320.0)
                .show(ui, |ui| self.reference(ui));
        }

        let half = ui.available_width() / 2.0;
        egui::Panel::left("editor")
            .resizable(true)
            .default_size(half)
            .show(ui, |ui| {
                if self.find.is_some() {
                    self.find_bar(ui);
                }
                self.editor(ui);
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let marks = self.marks();
                let shown = self
                    .view
                    .show(ui, self.scene.as_ref(), self.version, &marks);
                self.pane = shown.pane;
                self.hover = shown.hover;
                if let Some(point) = shown.clicked {
                    self.select_object_at(ui.ctx(), point);
                }
            });

        let ctx = ui.ctx().clone();
        self.unsaved_prompt(&ctx);
    }
}

/// The editor's id, which its cursor state is stored under.
fn editor_id() -> egui::Id {
    egui::Id::unique("acre source editor")
}

/// When the file at `path` last changed, or `None` when the disk cannot
/// say.
fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// The editor's text split into colored runs, with the error span
/// underlined.
fn layout(src: &str, error: Option<Span>, braces: Option<(usize, usize)>, size: f32) -> LayoutJob {
    let error = error.map(|e| widen(src, e));
    let mut cuts = vec![0, src.len()];
    if let Some((open, close)) = braces {
        cuts.extend([open, open + 1, close, close + 1]);
    }
    let mut runs: Vec<(usize, Option<Highlight>)> = Vec::new();
    for (span, class) in lang::highlight(src) {
        cuts.extend([span.start, span.end]);
        runs.push((span.start, Some(class)));
        runs.push((span.end, None));
    }
    if let Some(e) = error {
        cuts.extend([e.start, e.end]);
    }
    cuts.sort_unstable();
    cuts.dedup();

    let mut job = LayoutJob::default();
    let mut next_run = 0;
    let mut class = None;
    for pair in cuts.windows(2) {
        let (start, end) = (pair[0], pair[1]);
        // runs are in source order, and a token's end comes
        // before the next token's start at the same offset,
        // so the last boundary at or before `start` sets the class
        while let Some(&(at, c)) = runs.get(next_run)
            && at <= start
        {
            class = c;
            next_run += 1;
        }
        let underline = match error {
            Some(e) if e.start <= start && end <= e.end => {
                Stroke::new(1.5, Color32::from_rgb(0xf7, 0x76, 0x8e))
            }
            _ => Stroke::NONE,
        };
        let (color, background) = match class {
            Some(Highlight::Color([r, g, b, a])) => {
                // a swatch: the text on its own color, in black or
                // white by the color's luma
                let luma = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
                let text = if luma > 140.0 || a < 128 {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                };
                (text, Color32::from_rgba_unmultiplied(r, g, b, a))
            }
            _ => {
                let brace = braces.is_some_and(|(o, c)| start == o || start == c);
                let background = if brace {
                    Color32::from_rgb(0x3b, 0x42, 0x61)
                } else {
                    Color32::TRANSPARENT
                };
                (color(class), background)
            }
        };
        job.append(
            &src[start..end],
            0.0,
            TextFormat {
                font_id: FontId::monospace(size),
                color,
                background,
                underline,
                ..Default::default()
            },
        );
    }
    job
}

/// `span`, or the character at its start when it is empty, so an error
/// at a single point still shows.
fn widen(src: &str, span: Span) -> Span {
    if span.start < span.end || span.start >= src.len() {
        return span;
    }
    let width = src[span.start..].chars().next().map_or(0, char::len_utf8);
    Span {
        start: span.start,
        end: span.start + width,
    }
}

fn color(class: Option<Highlight>) -> Color32 {
    let hex = match class {
        None | Some(Highlight::Word) => 0xc0caf5,
        Some(Highlight::Key) => 0x7aa2f7,
        Some(Highlight::Number) => 0xff9e64,
        Some(Highlight::Str) => 0x9ece6a,
        Some(Highlight::Name) => 0x2ac3de,
        Some(Highlight::Markup) => 0xe0af68,
        Some(Highlight::Color(_)) => 0xbb9af7,
        Some(Highlight::Punct) => 0x9aa5ce,
        Some(Highlight::Comment) => 0x565f89,
        Some(Highlight::Invalid) => 0xf7768e,
    };
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text<'a>(job: &'a LayoutJob, s: &egui::text::LayoutSection) -> &'a str {
        &job.text[s.byte_range.start.0..s.byte_range.end.0]
    }

    fn runs(job: &LayoutJob) -> Vec<&str> {
        job.sections.iter().map(|s| text(job, s)).collect()
    }

    #[test]
    fn the_layout_keeps_every_byte_of_the_source() {
        let src = "canvas { size 1 2; } // é\n";
        let job = layout(src, None, None, 14.0);
        assert_eq!(job.text, src);
        assert_eq!(runs(&job).concat(), src);
    }

    #[test]
    fn the_error_span_is_its_own_underlined_run() {
        let src = "box { colour red; }";
        let job = layout(src, Some(Span { start: 6, end: 12 }), None, 14.0);
        let underlined: Vec<&str> = job
            .sections
            .iter()
            .filter(|s| s.format.underline != Stroke::NONE)
            .map(|s| text(&job, s))
            .collect();
        assert_eq!(underlined, ["colour"]);
    }

    #[test]
    fn whitespace_between_tokens_takes_the_plain_color() {
        let src = "a  1";
        let job = layout(src, None, None, 14.0);
        let gap = job.sections.iter().find(|s| text(&job, s) == "  ").unwrap();
        assert_eq!(gap.format.color, color(None));
    }
}

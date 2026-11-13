use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use acre::lang;
use acre::scene::{self, Scene};
use eframe::egui::{self, Color32, Vec2, ViewportCommand};

use crate::link::{Incoming, Link, Outgoing};
use crate::recent::Recent;
use crate::view::View;

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
    /// The font files the last source listed, with when each last
    /// changed on disk. A change reloads the source.
    fonts: Vec<(PathBuf, Option<SystemTime>)>,
    /// The editor that sends the source, when acre-gui runs with
    /// `--stdio`. acre-gui then watches no file.
    link: Option<Link>,
    error: Option<lang::Error>,
    /// The last scene that built, kept while the source has an error.
    scene: Option<Scene>,
    /// Counts the scenes built, so the view draws again on a new one.
    version: u64,
    /// Whether the editor heard about the current error, or that there
    /// is none.
    reported: bool,
    view: View,
    /// The render pane's size at the last frame.
    pane: Vec2,
    /// The editor's cursor, as a byte offset into the source.
    cursor: Option<usize>,
    /// The canvas point under the pointer.
    hover: Option<scene::Point>,
    /// The latest message that is not a source error.
    notice: Option<String>,
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
            fonts: Vec::new(),
            link,
            error: None,
            scene: None,
            version: 0,
            reported: false,
            view: View::new(),
            pane: Vec2::ZERO,
            cursor: None,
            hover: None,
            notice: None,
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
                self.notice = self.recent.touch(path).err();
                self.show(text);
            }
            Err(e) => self.notice = Some(format!("cannot open {}: {e}", path.display())),
        }
    }

    /// Show `text` as the source: build its scene, and keep the last one
    /// that built if it has an error.
    fn show(&mut self, text: String) {
        self.text = text;
        match lang::parse(&self.text).and_then(|program| self.build(&program)) {
            Ok(scene) => {
                self.error = None;
                self.scene = Some(scene);
                self.version += 1;
            }
            Err(e) => self.error = Some(e),
        }
        self.reported = false;
    }

    /// The scene `program` describes, with the font files it lists read
    /// from the folder of the source's file.
    fn build(&mut self, program: &lang::Program) -> Result<Scene, lang::Error> {
        let files = scene::font_files(program)?;
        let dir = self.path.as_deref().and_then(Path::parent);
        self.fonts = files
            .iter()
            .map(|f| {
                let path = dir.map_or(PathBuf::from(&f.path), |d| d.join(&f.path));
                let changed = modified(&path);
                (path, changed)
            })
            .collect();
        let mut bytes = Vec::new();
        for (file, (path, _)) in files.iter().zip(&self.fonts) {
            let read = fs::read(path).map_err(|e| {
                lang::Error::new(file.span, format!("cannot read font {}: {e}", file.path))
            })?;
            bytes.push(read);
        }
        scene::build(program, &bytes)
    }

    /// Build the source again if a font file it lists changed on disk.
    fn check_fonts(&mut self) {
        if self.fonts.iter().any(|(path, at)| modified(path) != *at) {
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
                Some(Incoming::Text { text, path }) => newest = Some((text, path)),
                Some(Incoming::Cursor { offset }) => self.cursor = Some(offset),
                None => open = false,
            }
        }

        // only the newest text matters when several came at once
        if let Some((text, path)) = newest {
            self.path = path.map(PathBuf::from);
            if text != self.text || self.version == 0 {
                self.show(text);
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
            .filter_map(|i| scene.bounds(i, 0.0))
            .collect()
    }

    /// Point the editor at the node that drew the object at canvas
    /// `point`, or name the node's line when there is no editor.
    fn go_to_object_at(&mut self, point: scene::Point) {
        let Some(scene) = &self.scene else { return };
        let Some(index) = scene.object_at(point, 0.0) else {
            return;
        };
        let offset = scene.objects[index].source.start;
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

    /// The buttons that zoom the render.
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
        });
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
            self.check_fonts();
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
        if self.link.is_none() {
            egui::Panel::top("menu").show(ui, |ui| self.menu(ui));
        }
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
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
                    self.go_to_object_at(point);
                }
            });
    }
}

/// When the file at `path` last changed, or `None` when the disk cannot
/// say.
fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

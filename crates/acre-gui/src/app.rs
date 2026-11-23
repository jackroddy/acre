use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use acre::lang;
use acre::scene::{self, Scene, Section};
use eframe::egui::collapsing_header::CollapsingState;
use eframe::egui::text::{LayoutJob, TextWrapping};
use eframe::egui::{
    self, Color32, Key, Modifiers, Pos2, Rect, Sense, Stroke, TextStyle, Vec2, ViewportCommand,
};

use crate::link::{Incoming, Link, Outgoing};
use crate::playback::{self, Playback};
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
    /// The font and data files the last source listed, with when each
    /// last changed on disk. A change reloads the source.
    files: Vec<(PathBuf, Option<SystemTime>)>,
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
            error: None,
            scene: None,
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
                self.notice = self.recent.touch(path).err();
                let now = Instant::now();
                self.playback.pause(now);
                self.playback.seek(now, 0.0);
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
                self.playback
                    .set_duration(Instant::now(), scene.canvas.duration);
                self.error = None;
                let missing = scene.missing_glyphs();
                if missing != self.missing {
                    for c in &missing {
                        eprintln!("{}: {}", self.name(), missing_glyph(*c));
                    }
                    self.missing = missing;
                }
                self.scene = Some(scene);
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
            .filter_map(|i| scene.bounds(i, self.time))
            .collect()
    }

    /// Point the editor at the node that drew the object at canvas
    /// `point`, or name the node's line when there is no editor.
    fn go_to_object_at(&mut self, point: scene::Point) {
        let Some(scene) = &self.scene else { return };
        let Some(index) = scene.object_at(point, self.time) else {
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

    /// Space plays and pauses, and Left and Right step a frame, unless a
    /// text field has the keys.
    fn keys(&mut self, ctx: &egui::Context) {
        let Some(scene) = &self.scene else { return };
        if self.playback.duration() <= 0.0 || ctx.text_edit_focused() {
            return;
        }

        // consumed before the bar is laid out, so a focused button does
        // not take the same press
        let (space, left, right) = ctx.input_mut(|i| {
            (
                i.count_and_consume_key(Modifiers::NONE, Key::Space),
                i.count_and_consume_key(Modifiers::NONE, Key::ArrowLeft),
                i.count_and_consume_key(Modifiers::NONE, Key::ArrowRight),
            )
        });
        let now = Instant::now();
        if space % 2 == 1 {
            self.playback.toggle(now);
        }
        let frames = right as i64 - left as i64;
        if frames != 0 {
            self.playback.step(now, frames, scene.canvas.fps);
        }
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
            } else if let Some(&c) = self.missing.first() {
                let more = match self.missing.len() - 1 {
                    0 => String::new(),
                    n => format!(", and {n} more"),
                };
                ui.colored_label(
                    Color32::from_rgb(0xe0, 0xaf, 0x68),
                    format!("{}{more}", missing_glyph(c)),
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
        if self.playback.playing() {
            ui.ctx().request_repaint();
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                let marks = self.marks();
                let shown =
                    self.view
                        .show(ui, self.scene.as_ref(), self.version, self.time, &marks);
                self.pane = shown.pane;
                self.hover = shown.hover;
                if let Some(point) = shown.clicked {
                    self.go_to_object_at(point);
                }
            });
    }
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

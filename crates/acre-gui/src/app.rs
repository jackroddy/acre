use std::fs;
use std::path::{Path, PathBuf};

use acre::lang::{self, Highlight, Span};
use acre::{render, scene};
use eframe::egui::{
    self, Color32, FontId, Key, KeyboardShortcut, Modifiers, Stroke, TextureHandle, TextureOptions,
    ViewportCommand,
    text::{LayoutJob, TextFormat},
};

use crate::config::Config;
use crate::recent::Recent;

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
    error: Option<lang::Error>,
    texture: Option<TextureHandle>,
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
            error: None,
            texture: None,
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

    fn render(&mut self, ctx: &egui::Context) {
        self.rendered = true;
        let built = lang::parse(&self.text).and_then(|program| scene::build(&program));
        match built {
            Ok(scene) => {
                self.error = None;
                let pixmap = render::draw_frame(&scene, 0.0);
                let image = egui::ColorImage::from_rgba_premultiplied(
                    [pixmap.width() as usize, pixmap.height() as usize],
                    pixmap.data(),
                );
                match &mut self.texture {
                    Some(texture) => texture.set(image, TextureOptions::LINEAR),
                    None => {
                        self.texture =
                            Some(ctx.load_texture("render", image, TextureOptions::LINEAR));
                    }
                }
            }
            Err(e) => self.error = Some(e),
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
        let (new, open, save_as, save, quit) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&NEW),
                i.consume_shortcut(&OPEN),
                i.consume_shortcut(&SAVE_AS),
                i.consume_shortcut(&SAVE),
                i.consume_shortcut(&QUIT),
            )
        });
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
            self.render(ctx);
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

        let half = ui.available_width() / 2.0;
        egui::Panel::left("editor")
            .resizable(true)
            .default_size(half)
            .show(ui, |ui| {
                let error = self.error.as_ref().map(|e| e.span);
                let size = self.config.editor_font_size;
                let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
                    let mut job = layout(buf.as_str(), error, size);
                    job.wrap.max_width = wrap;
                    ui.ctx().fonts_mut(|f| f.layout_job(job))
                };
                egui::ScrollArea::both().show(ui, |ui| {
                    let edit = egui::TextEdit::multiline(&mut self.text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(40)
                        .layouter(&mut layouter);
                    if ui.add(edit).changed() {
                        self.rendered = false;
                    }
                });
            });

        egui::CentralPanel::default().show(ui, |ui| {
            if let Some(texture) = &self.texture {
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Image::new(texture).shrink_to_fit());
                });
            }
        });

        let ctx = ui.ctx().clone();
        self.unsaved_prompt(&ctx);
    }
}

/// The editor's text split into colored runs, with the error span
/// underlined.
fn layout(src: &str, error: Option<Span>, size: f32) -> LayoutJob {
    let error = error.map(|e| widen(src, e));
    let mut cuts = vec![0, src.len()];
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
        job.append(
            &src[start..end],
            0.0,
            TextFormat {
                font_id: FontId::monospace(size),
                color: color(class),
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
        Some(Highlight::Color) => 0xbb9af7,
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
        let job = layout(src, None, 14.0);
        assert_eq!(job.text, src);
        assert_eq!(runs(&job).concat(), src);
    }

    #[test]
    fn the_error_span_is_its_own_underlined_run() {
        let src = "box { colour red; }";
        let job = layout(src, Some(Span { start: 6, end: 12 }), 14.0);
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
        let job = layout(src, None, 14.0);
        let gap = job.sections.iter().find(|s| text(&job, s) == "  ").unwrap();
        assert_eq!(gap.format.color, color(None));
    }
}

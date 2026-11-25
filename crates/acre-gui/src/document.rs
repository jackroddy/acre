//! One drawing open in the viewer: its source, the scene built from it,
//! and what the person did to it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use acre::lang;
use acre::scene::{self, Scene};

use crate::field::Field;
use crate::playback::Playback;
use crate::properties::Properties;
use crate::selection::Selection;
use crate::view::View;

/// A drawing, from a file the viewer watches or from an editor over the
/// link.
pub struct Document {
    /// The file the source belongs to.
    pub path: Option<PathBuf>,
    pub text: String,
    /// When the watched file last changed, as the disk reports it.
    pub modified: Option<SystemTime>,
    /// The font and data files the last source listed, with when each
    /// last changed on disk. A change reloads the source.
    pub files: Vec<(PathBuf, Option<SystemTime>)>,
    /// The editor's tick for `text`.
    pub tick: u64,
    pub error: Option<lang::Error>,
    /// The last scene that built, kept while the source has an error.
    pub scene: Option<Scene>,
    /// The program `scene` was built from.
    pub program: Option<lang::Program>,
    /// The source `program` was parsed from.
    pub program_text: String,
    /// The nodes clicked, dragged or boxed last, or under the editor's
    /// cursor, outlined until a click elsewhere.
    pub selection: Selection,
    /// The field open over an object to edit its text.
    pub field: Option<Field>,
    /// The panel that changes the selected nodes' colors and strokes.
    pub properties: Properties,
    /// The nodes added by the edit with the editor, to select once the
    /// editor sends its text back.
    pub added: Option<Added>,
    /// Counts the scenes built, so the view draws again on a new one.
    pub version: u64,
    /// Whether the editor heard about the current error, or that there
    /// is none.
    pub reported: bool,
    pub view: View,
    pub playback: Playback,
    /// The time drawn this frame.
    pub time: f64,
    /// The characters in the scene's text that no font has.
    pub missing: Vec<char>,
}

/// Nodes an edit adds, to select.
pub struct Added {
    /// Their places as [`acre::scene::edit::node_path`] gives them.
    pub paths: Vec<Vec<usize>>,
    /// Whether to open a field on the first to edit its text.
    pub open_field: bool,
}

impl Document {
    pub fn new() -> Self {
        Document {
            path: None,
            text: String::new(),
            modified: None,
            files: Vec::new(),
            tick: 0,
            error: None,
            scene: None,
            program: None,
            program_text: String::new(),
            selection: Selection::default(),
            field: None,
            properties: Properties::default(),
            added: None,
            version: 0,
            reported: false,
            view: View::new(),
            playback: Playback::new(),
            time: 0.0,
            missing: Vec::new(),
        }
    }

    pub fn name(&self) -> String {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map_or("untitled".into(), |n| n.to_string_lossy().into_owned())
    }

    /// Show the file at `path` from its start, with nothing selected,
    /// or give the reason for the status line that it cannot be read.
    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        let text =
            fs::read_to_string(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        self.path = Some(path.to_path_buf());
        self.modified = modified(path);
        self.rewind();
        self.show(text);
        Ok(())
    }

    /// Pause at the start, with nothing selected and no field open.
    pub fn rewind(&mut self) {
        let now = Instant::now();
        self.playback.pause(now);
        self.playback.seek(now, 0.0);
        self.selection = Selection::default();
        self.field = None;
    }

    /// Show `text` as the source: build its scene, and keep the last one
    /// that built if it has an error. True when it built.
    pub fn show(&mut self, text: String) -> bool {
        self.view.end_move();
        self.text = text;
        let built = lang::parse(&self.text)
            .and_then(|program| self.build(&program).map(|scene| (program, scene)));
        self.reported = false;
        match built {
            Ok((program, scene)) => {
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
                self.selection = std::mem::take(&mut self.selection).follow(&program);
                self.scene = Some(scene);
                self.program = Some(program);
                self.program_text = self.text.clone();
                self.version += 1;
                true
            }
            Err(e) => {
                self.error = Some(e);
                false
            }
        }
    }

    /// The scene `program` describes, with the font and data files it
    /// lists read from the folder of the source's file.
    pub fn build(&mut self, program: &lang::Program) -> Result<Scene, lang::Error> {
        let dir = self.path.as_deref().and_then(Path::parent);
        build(program, dir, &mut self.files)
    }

    /// Build the source again if a font or data file it lists changed on
    /// disk. True when it built again.
    pub fn check_files(&mut self) -> bool {
        self.files.iter().any(|(path, at)| modified(path) != *at) && self.show(self.text.clone())
    }

    /// Read the watched file again if it changed on disk. True when it
    /// built again.
    pub fn check_disk(&mut self) -> bool {
        let Some(path) = self.path.clone() else {
            return false;
        };
        let on_disk = modified(&path);
        if on_disk.is_none() || on_disk == self.modified {
            return false;
        }
        self.modified = on_disk;
        match fs::read_to_string(&path) {
            Ok(text) if text != self.text => self.show(text),
            _ => false,
        }
    }
}

/// The scene `program` describes, with the font and data files it lists
/// read from `dir`, or from the working directory without one. `files`
/// gets each listed file with when it last changed on disk, once the
/// program lists them without an error.
pub fn build(
    program: &lang::Program,
    dir: Option<&Path>,
    files: &mut Vec<(PathBuf, Option<SystemTime>)>,
) -> Result<Scene, lang::Error> {
    let fonts = scene::font_files(program)?;
    let data = scene::data_files(program)?;
    let listed: Vec<(&str, &str, lang::Span)> = fonts
        .iter()
        .map(|f| ("font", f.path.as_str(), f.span))
        .chain(data.iter().map(|d| ("data file", d.path.as_str(), d.span)))
        .collect();
    *files = listed
        .iter()
        .map(|(_, path, _)| {
            let path = dir.map_or(PathBuf::from(path), |d| d.join(path));
            let changed = modified(&path);
            (path, changed)
        })
        .collect();
    let mut bytes = Vec::new();
    for ((what, written, span), (path, _)) in listed.iter().zip(files.iter()) {
        let read = fs::read(path)
            .map_err(|e| lang::Error::new(*span, format!("cannot read {what} {written}: {e}")))?;
        bytes.push(read);
    }
    let data_bytes = bytes.split_off(fonts.len());
    scene::build(program, &bytes, &data_bytes)
}

pub fn missing_glyph(c: char) -> String {
    format!(
        "warning: no font has a glyph for '{c}' (U+{:04X})",
        c as u32
    )
}

/// When the file at `path` last changed, or `None` when the disk cannot
/// say.
pub fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

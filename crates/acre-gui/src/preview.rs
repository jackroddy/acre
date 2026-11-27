use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::SystemTime;

use acre::{lang, render};
use una::{Image, Ui};

use crate::document;

/// The box a preview fits into, in pixels at the chrome style's mockup
/// size.
const SIZE: [i32; 2] = [240, 160];

/// The most rows an error takes in a preview.
const ERROR_ROWS: usize = 4;

/// The columns of an error's rows in a preview.
const ERROR_COLUMNS: usize = 30;

/// How often, in milliseconds, the list draws again while a preview is
/// being built.
const POLL: u64 = 50;

/// Small renders of drawings at time 0, built on a background thread
/// and kept for the session by path and modification time.
#[derive(Default)]
pub struct Previews {
    cache: HashMap<PathBuf, Entry>,
    /// The worker's ends of the channels, made on the first request.
    worker: Option<(Sender<Request>, Receiver<Reply>)>,
}

struct Entry {
    modified: Option<SystemTime>,
    state: State,
}

enum State {
    Pending,
    Pixels(Pixels),
    Image(Image),
    Failed(String),
}

/// A render as straight-alpha RGBA bytes.
#[derive(Debug)]
struct Pixels {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

struct Request {
    path: PathBuf,
    modified: Option<SystemTime>,
    /// The width and height in pixels the render fits into.
    fit: [f64; 2],
}

struct Reply {
    path: PathBuf,
    modified: Option<SystemTime>,
    image: Result<Pixels, String>,
}

impl Previews {
    /// Ask for the preview of `path` as it was at `modified`. False when
    /// the cache already holds it or is building it.
    fn request(&mut self, path: &Path, modified: Option<SystemTime>, fit: [f64; 2]) -> bool {
        if self.cache.get(path).is_some_and(|e| e.modified == modified) {
            return false;
        }
        let (to_worker, _) = self.worker.get_or_insert_with(start);
        let request = Request {
            path: path.to_path_buf(),
            modified,
            fit,
        };
        if to_worker.send(request).is_err() {
            return false;
        }
        let state = State::Pending;
        self.cache
            .insert(path.to_path_buf(), Entry { modified, state });
        true
    }

    /// Take the previews the worker finished, dropping any for a version
    /// of a file the cache no longer wants.
    fn receive(&mut self) {
        let Some((_, from_worker)) = &self.worker else {
            return;
        };
        for reply in from_worker.try_iter() {
            if let Some(entry) = self.cache.get_mut(&reply.path)
                && entry.modified == reply.modified
            {
                entry.state = match reply.image {
                    Ok(pixels) => State::Pixels(pixels),
                    Err(e) => State::Failed(e),
                };
            }
        }
    }

    /// Show the preview of `path`: the render, a note while it is
    /// built, or the start of the error that stopped it.
    pub fn show(&mut self, ui: &mut Ui, path: &Path) {
        let style = una::Style::small(ui.palette());
        let fit = SIZE.map(|side| style.px(ui, side) as f64);
        self.request(path, document::modified(path), fit);
        self.receive();
        let Some(entry) = self.cache.get_mut(path) else {
            return;
        };
        if let State::Pixels(pixels) = &entry.state {
            let image = ui.load_image(pixels.width, pixels.height, &pixels.rgba);
            entry.state = State::Image(image);
        }
        match &entry.state {
            State::Pending | State::Pixels(_) => {
                ui.label("drawing…");
                ui.wake_at(ui.now_ms() + POLL);
            }
            State::Image(image) => {
                ui.image(image);
            }
            State::Failed(e) => {
                let first = e.lines().next().unwrap_or_default();
                for row in rows(first, ERROR_COLUMNS, ERROR_ROWS) {
                    ui.label(&row);
                }
            }
        }
    }
}

/// `text` broken into rows of `columns` characters, anywhere, since a
/// path has no spaces to break at, and cut to `most` rows.
fn rows(text: &str, columns: usize, most: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut rows: Vec<String> = chars
        .chunks(columns.max(1))
        .map(|row| row.iter().collect())
        .collect();
    if rows.len() > most {
        rows.truncate(most);
        if let Some(last) = rows.last_mut() {
            last.pop();
            last.push('…');
        }
    }
    rows
}

fn start() -> (Sender<Request>, Receiver<Reply>) {
    let (to_worker, requests) = mpsc::channel::<Request>();
    let (replies, from_worker) = mpsc::channel();
    thread::spawn(move || {
        for request in requests {
            let image = draw(&request.path, request.fit);
            let reply = Reply {
                path: request.path,
                modified: request.modified,
                image,
            };
            if replies.send(reply).is_err() {
                return;
            }
        }
    });
    (to_worker, from_worker)
}

/// The drawing at `path` at time 0, scaled to fit `fit` pixels, with its
/// font and data files read from its folder.
fn draw(path: &Path, fit: [f64; 2]) -> Result<Pixels, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let scene = lang::parse(&text)
        .and_then(|program| document::build(&program, path.parent(), &mut Vec::new()))
        .map_err(|e| {
            let (line, col) = e.line_col(&text);
            format!("{line}:{col}: {}", e.message)
        })?;
    let canvas = &scene.canvas;
    let scale = (fit[0] / canvas.width as f64).min(fit[1] / canvas.height as f64);
    if (canvas.width as f64 * scale).round() < 1.0 || (canvas.height as f64 * scale).round() < 1.0 {
        return Err("the canvas is too thin to preview".into());
    }
    let pixmap = render::draw_frame(&scene, 0.0, scale);
    let rgba = (pixmap.pixels().iter())
        .flat_map(|pixel| {
            let c = pixel.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(Pixels {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIT: [f64; 2] = [240.0, 160.0];

    #[test]
    fn the_same_version_of_a_file_is_built_once() {
        let mut previews = Previews::default();
        let path = Path::new("no such file.acre");
        let at = Some(SystemTime::UNIX_EPOCH);
        assert!(previews.request(path, at, FIT));
        assert!(!previews.request(path, at, FIT));
    }

    #[test]
    fn a_changed_modification_time_builds_again() {
        let mut previews = Previews::default();
        let path = Path::new("no such file.acre");
        assert!(previews.request(path, Some(SystemTime::UNIX_EPOCH), FIT));
        let later = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1);
        assert!(previews.request(path, Some(later), FIT));
        assert!(!previews.request(path, Some(later), FIT));
    }

    #[test]
    fn a_missing_file_previews_as_an_error() {
        let error = draw(Path::new("no such file.acre"), FIT).unwrap_err();
        assert!(
            error.starts_with("cannot read no such file.acre:"),
            "{error}"
        );
    }

    #[test]
    fn a_drawing_is_scaled_to_fit_the_box() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../acre/examples/layers.acre");
        let image = draw(&path, FIT).unwrap();
        // the canvas is 640 by 200, so its width sets the scale
        assert_eq!((image.width, image.height), (240, 75));
        assert_eq!(image.rgba.len(), 240 * 75 * 4);
    }

    #[test]
    fn an_error_breaks_anywhere_and_is_cut_to_its_rows() {
        assert_eq!(rows("abcdefg", 3, 4), ["abc", "def", "g"]);
        assert_eq!(rows("abcdefghij", 3, 2), ["abc", "de…"]);
    }
}

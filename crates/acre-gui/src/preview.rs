use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, SystemTime};

use acre::{lang, render};
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions, Vec2};

use crate::document;

/// The box in points a preview fits into.
const SIZE: Vec2 = Vec2::new(240.0, 160.0);

/// How often the menu draws again while a preview is being built.
const POLL: Duration = Duration::from_millis(50);

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
    Image(ColorImage),
    Texture(TextureHandle),
    Failed(String),
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
    image: Result<ColorImage, String>,
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
                    Ok(image) => State::Image(image),
                    Err(e) => State::Failed(e),
                };
            }
        }
    }

    /// Draw the preview of `path`: the render, a spinner while it is
    /// built, or the first line of the error that stopped it.
    pub fn show(&mut self, ui: &mut egui::Ui, path: &Path) {
        let ppp = ui.ctx().pixels_per_point();
        let fit = [(SIZE.x * ppp) as f64, (SIZE.y * ppp) as f64];
        self.request(path, document::modified(path), fit);
        self.receive();
        let Some(entry) = self.cache.get_mut(path) else {
            return;
        };
        if let State::Image(image) = &entry.state {
            let texture = ui
                .ctx()
                .load_texture("preview", image.clone(), TextureOptions::LINEAR);
            entry.state = State::Texture(texture);
        }
        match &entry.state {
            State::Pending | State::Image(_) => {
                ui.spinner();
                ui.ctx().request_repaint_after(POLL);
            }
            State::Texture(texture) => {
                let size = texture.size_vec2() / ppp;
                ui.image((texture.id(), size));
            }
            State::Failed(e) => {
                ui.label(e.lines().next().unwrap_or_default());
            }
        }
    }
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
fn draw(path: &Path, fit: [f64; 2]) -> Result<ColorImage, String> {
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
    let pixels = [pixmap.width() as usize, pixmap.height() as usize];
    Ok(ColorImage::from_rgba_premultiplied(pixels, pixmap.data()))
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
        let later = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
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
        assert_eq!(image.size, [240, 75]);
    }
}

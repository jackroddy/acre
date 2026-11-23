//! MP4 export: draws every frame of a scene and pipes it to `ffmpeg`.

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;

use anyhow::{Context, anyhow};

use acre::render;
use acre::scene::{Color, Scene};

/// How many frames to draw: one every `1/fps` seconds from 0 through
/// `duration` inclusive.
pub fn frame_count(duration: f64, fps: u32) -> usize {
    // the nudge keeps 4.9999999999 frames from losing the last one to rounding
    (duration * fps as f64 + 1e-9).floor() as usize + 1
}

/// A length rounded up to the next even number, as yuv420p needs.
fn even(n: u32) -> u32 {
    n + n % 2
}

/// The size in pixels of a canvas drawn at `scale`, as `draw_frame` makes it.
fn pixel_size(scene: &Scene, scale: f64) -> (u32, u32) {
    let size = |n: u32| (n as f64 * scale).round() as u32;
    (size(scene.canvas.width), size(scene.canvas.height))
}

/// Fills `out` with the RGB bytes of premultiplied RGBA `pixels`, `width`
/// across, flattened onto `background`. A column and a row of `background`
/// are added where the size is odd.
fn flatten(pixels: &[u8], width: usize, background: Color, out: &mut Vec<u8>) {
    let bg = [background.r, background.g, background.b].map(|c| (c * 255.0).round() as u32);
    let height = pixels.len() / 4 / width;
    let odd_width = width % 2 == 1;
    out.clear();
    for row in pixels.chunks_exact(width * 4) {
        for px in row.as_chunks::<4>().0 {
            // the pixel is premultiplied, so the background shows through
            // in proportion to the alpha it leaves uncovered
            let hidden = 255 - px[3] as u32;
            for c in 0..3 {
                let covered = px[c] as u32 + (hidden * bg[c] + 127) / 255;
                out.push(covered.min(255) as u8);
            }
        }
        if odd_width {
            out.extend(bg.map(|c| c as u8));
        }
    }
    if height % 2 == 1 {
        let padded_width = even(width as u32) as usize;
        for _ in 0..padded_width {
            out.extend(bg.map(|c| c as u8));
        }
    }
}

/// Draws frames `0..count` on `jobs` threads and gives each to `sink` in
/// order. Thread `k` draws the frames numbered `k` modulo `jobs`, and holds at
/// most two drawn frames ahead of `sink`.
fn draw_frames(
    scene: &Scene,
    scale: f64,
    fps: u32,
    count: usize,
    jobs: usize,
    mut sink: impl FnMut(usize, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    thread::scope(|s| {
        let mut receivers = Vec::new();
        for k in 0..jobs {
            let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(2);
            receivers.push(receiver);
            s.spawn(move || {
                for frame in (k..count).step_by(jobs) {
                    let pixmap = render::draw_frame(scene, frame as f64 / fps as f64, scale);
                    let mut bytes = Vec::new();
                    flatten(
                        pixmap.data(),
                        pixmap.width() as usize,
                        scene.canvas.background,
                        &mut bytes,
                    );
                    // the receiver is gone when the sink failed
                    if sender.send(bytes).is_err() {
                        return;
                    }
                }
            });
        }
        for frame in 0..count {
            let bytes = receivers[frame % jobs]
                .recv()
                .expect("a drawing thread ends only after sending its frames");
            sink(frame, &bytes)?;
        }
        Ok(())
    })
}

/// The path of the `ffmpeg` on `PATH`, if there is one.
fn find_ffmpeg() -> Option<std::path::PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join("ffmpeg"))
        .find(|candidate| candidate.is_file())
}

/// Writes the animation in `scene` to `out` as H.264 video.
pub fn export(
    scene: &Scene,
    out: &Path,
    scale: f64,
    fps: Option<u32>,
    jobs: usize,
) -> anyhow::Result<()> {
    if scene.canvas.duration <= 0.0 {
        return Err(anyhow!(
            "the drawing has no animation: give the canvas a duration to export MP4"
        ));
    }
    let ffmpeg = find_ffmpeg()
        .ok_or_else(|| anyhow!("MP4 export needs ffmpeg, and there is none on the PATH"))?;
    let fps = fps.unwrap_or(scene.canvas.fps);
    let count = frame_count(scene.canvas.duration, fps);
    let (width, height) = pixel_size(scene, scale);

    let mut child = Command::new(&ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-f", "rawvideo", "-pix_fmt", "rgb24"])
        .args(["-s", &format!("{}x{}", even(width), even(height))])
        .args(["-r", &fps.to_string(), "-i", "-"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .args(["-movflags", "+faststart", "-f", "mp4"])
        .arg(out)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("cannot start {}", ffmpeg.display()))?;
    let mut stdin = child.stdin.take().expect("stdin was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");
    // ffmpeg's messages are read as they come so a full pipe cannot stall it
    let messages = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let progress = io::stderr().is_terminal();
    let written = draw_frames(scene, scale, fps, count, jobs, |frame, bytes| {
        if progress && frame % 10 == 0 {
            eprint!("\rframe {}/{count}", frame + 1);
        }
        stdin.write_all(bytes)
    });
    if progress {
        eprint!("\r\x1b[K");
    }
    // closing stdin tells ffmpeg the video is complete
    drop(stdin);
    let status = child.wait().context("cannot wait for ffmpeg")?;
    let messages = messages.join().unwrap_or_default();

    if !status.success() || written.is_err() {
        let _ = fs::remove_file(out);
        let detail = match messages.trim() {
            "" => String::from("ffmpeg gave no message"),
            text => text.to_string(),
        };
        return Err(anyhow!(
            "ffmpeg failed ({status}), so {} was not written:\n{detail}",
            out.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(width: usize, height: usize, rgba: [u8; 4]) -> (Vec<u8>, usize) {
        (rgba.repeat(width * height), width)
    }

    fn flatten_of((data, width): (Vec<u8>, usize), bg: Color, out: &mut Vec<u8>) {
        flatten(&data, width, bg, out);
    }

    const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);

    #[test]
    fn frames_run_from_zero_through_the_duration() {
        assert_eq!(frame_count(5.0, 30), 151);
        assert_eq!(frame_count(1.0, 24), 25);
        assert_eq!(frame_count(0.1, 30), 4);
        // 0.1 * 3 is 0.30000000000000004 and 0.7 * 10 is 7.000000000000001
        assert_eq!(frame_count(0.7, 10), 8);
        assert_eq!(frame_count(1.05, 10), 11);
    }

    #[test]
    fn odd_sizes_round_up() {
        assert_eq!((even(640), even(361), even(1)), (640, 362, 2));
    }

    #[test]
    fn an_even_pixmap_keeps_its_size() {
        let mut out = Vec::new();
        flatten_of(pixels(4, 2, [10, 20, 30, 255]), WHITE, &mut out);
        assert_eq!(out.len(), 4 * 2 * 3);
        assert_eq!(&out[..3], &[10, 20, 30]);
    }

    #[test]
    fn an_odd_pixmap_is_padded_with_the_background() {
        let mut out = Vec::new();
        let bg = Color::rgb(1.0, 0.0, 0.0);
        flatten_of(pixels(3, 3, [0, 0, 255, 255]), bg, &mut out);
        assert_eq!(out.len(), 4 * 4 * 3);
        // the first row ends with the padding column
        assert_eq!(&out[9..15], &[255, 0, 0, 0, 0, 255]);
        // the last row is all padding
        assert!(out[36..].chunks(3).all(|px| px == [255, 0, 0]));
    }

    #[test]
    fn translucent_pixels_are_flattened_onto_the_background() {
        let mut out = Vec::new();
        // black at half alpha, premultiplied, over white
        flatten_of(pixels(2, 2, [0, 0, 0, 128]), WHITE, &mut out);
        assert_eq!(&out[..3], &[127, 127, 127]);
        // a fully transparent pixel is the background
        flatten_of(pixels(2, 2, [0, 0, 0, 0]), WHITE, &mut out);
        assert_eq!(&out[..3], &[255, 255, 255]);
    }

    #[test]
    fn frames_reach_the_sink_in_order_on_any_number_of_threads() {
        let src = "canvas { size 64 48; duration 1s; box a { at 4 4; size 20 20; } \
                   timeline { 0s to 1s { move a 30 0; } } }";
        let program = acre::lang::parse(src).unwrap();
        let scene = acre::scene::build(&program, &[], &[]).unwrap();
        let count = frame_count(scene.canvas.duration, 8);
        assert_eq!(count, 9);
        let run = |jobs| {
            let mut frames = Vec::new();
            draw_frames(&scene, 1.0, 8, count, jobs, |n, bytes| {
                assert_eq!(n, frames.len());
                frames.push(bytes.to_vec());
                Ok(())
            })
            .unwrap();
            frames
        };
        let one = run(1);
        assert_eq!(one.len(), count);
        assert_ne!(one[0], one[8]);
        assert_eq!(one, run(4));
        assert_eq!(one, run(16));
    }
}

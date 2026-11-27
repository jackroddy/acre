//! The bar that shows and scrubs the scene's time: one app-drawn pane,
//! with the played part filled and the sections marked on it.

use acre::scene::Section;
use tiny_skia as sk;
use una::{Buffer, Format, Length, Palette, Rgba, Signal, Spec, Style, Surface, Ui};

use crate::playback;

pub struct TimeBar {
    surface: Surface,
    /// What the buffer on the surface shows.
    drawn: Option<Drawn>,
}

/// What the bar was drawn for.
#[derive(Clone, PartialEq)]
struct Drawn {
    size: (u32, u32),
    duration: f64,
    time: f64,
    /// The start of each section, with the starts of its subsections.
    starts: Vec<(f64, Vec<f64>)>,
    current: Option<usize>,
    /// The background, the current section, the played part, the
    /// lines between sections and the playhead.
    colors: [Rgba; 5],
    /// The playhead's width in pixels.
    head: f32,
}

impl TimeBar {
    pub fn new() -> Self {
        TimeBar {
            surface: Surface::new(),
            drawn: None,
        }
    }

    /// Build the bar over `0..=duration`, divided into `sections`, with
    /// the playhead at `time`, and name the section under the pointer in
    /// a tooltip. Returns the time the pointer presses or drags to.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        sections: &[Section],
        duration: f64,
        time: f64,
    ) -> Option<f64> {
        let style = Style::small(ui.palette());
        let height = style.px(ui, style.row);
        let spec = Spec {
            width: Length::Fill(1),
            height: Length::Px(height),
            ..Spec::default()
        };
        let mut pane = None;
        ui.group(spec, |ui| pane = Some(ui.surface(&mut self.surface)));
        let pane = pane?;
        let signal = pane.signal;
        let width = pane.size.0 as f64;
        if width <= 0.0 || duration <= 0.0 {
            return None;
        }
        let palette = ui.palette();
        let wanted = Drawn {
            size: pane.size,
            duration,
            time,
            starts: (sections.iter())
                .map(|s| (s.start, s.subsections.iter().map(|s| s.start).collect()))
                .collect(),
            current: playback::current(sections, time),
            colors: colors(palette),
            head: (style.px(ui, 2)) as f32,
        };
        if self.drawn.as_ref() != Some(&wanted) && pane.size.1 > 0 {
            self.surface.present(draw(&wanted));
            self.drawn = Some(wanted);
        }

        let time_at = |x: f64| (x / width).clamp(0.0, 1.0) * duration;
        if let Some((x, _)) = signal.at.filter(|_| signal.hovered) {
            tooltip(ui, signal, sections, duration, time_at(x));
        }
        let (x, _) = signal.at.filter(|_| signal.held)?;
        Some(time_at(x))
    }
}

fn colors(palette: &Palette) -> [Rgba; 5] {
    let [red, green, blue, _] = palette.selection;
    [
        palette.dark_background,
        palette.lighter_background,
        [red, green, blue, 0.8],
        palette.border,
        palette.foreground,
    ]
}

/// Name the section, and the subsection, at time `t` in a tooltip by
/// that section's part of the bar.
fn tooltip(ui: &mut Ui, bar: Signal, sections: &[Section], duration: f64, t: f64) {
    let Some(i) = playback::current(sections, t) else {
        return;
    };
    let section = &sections[i];
    let place = match playback::current_subsection(section, t) {
        Some(j) => format!("{} › {}", section.title, section.subsections[j].title),
        None => section.title.clone(),
    };
    let x = |t: f64| bar.rect.x + (t / duration * bar.rect.width as f64) as i32;
    let (left, right) = (
        x(section.start),
        sections.get(i + 1).map_or(bar.rect.right(), |s| x(s.start)),
    );
    let part = Signal {
        id: ui.id(&format!("section {i}")),
        rect: una::Rect {
            x: left,
            width: right - left,
            ..bar.rect
        },
        ..bar
    };
    ui.tooltip(part, &place);
}

/// The pixels of the bar as `drawn` describes it.
fn draw(drawn: &Drawn) -> Buffer {
    let (width, height) = drawn.size;
    let mut buffer = Buffer::new(width, height, Format::Rgba);
    let Some(mut pixmap) = sk::PixmapMut::from_bytes(buffer.pixels(), width, height) else {
        return buffer;
    };
    let [background, current, played, line, head] = drawn
        .colors
        .map(|[r, g, b, a]| sk::Color::from_rgba(r, g, b, a).unwrap_or(sk::Color::BLACK));
    pixmap.fill(background);
    let (w, h) = (width as f32, height as f32);
    let x = |t: f64| (t / drawn.duration) as f32 * w;
    let mut fill = |left: f32, top: f32, right: f32, bottom: f32, color: sk::Color| {
        let mut paint = sk::Paint::default();
        paint.set_color(color);
        if let Some(rect) = sk::Rect::from_ltrb(left, top, right, bottom) {
            pixmap.fill_rect(rect, &paint, sk::Transform::identity(), None);
        }
    };
    if let Some(i) = drawn.current {
        let right = drawn.starts.get(i + 1).map_or(w, |s| x(s.0));
        fill(x(drawn.starts[i].0), 0.0, right, h, current);
    }
    fill(0.0, 0.0, x(drawn.time), h, played);
    for (start, subsections) in &drawn.starts {
        if *start > 0.0 {
            fill(x(*start), 0.0, x(*start) + 1.0, h, line);
        }
        for sub in subsections.iter().filter(|s| *s > start) {
            fill(x(*sub), h * 0.7, x(*sub) + 1.0, h, line);
        }
    }
    let at = x(drawn.time);
    fill(at - drawn.head / 2.0, 0.0, at + drawn.head / 2.0, h, head);
    buffer
}

//! The bar that shows and scrubs the scene's time: one app-drawn pane,
//! with the played part filled and the sections marked and titled on it.

use acre::scene::Section;
use tiny_skia as sk;
use una::{
    Buffer, Face, Format, Length, Palette, Point, Rgba, Signal, Size, Spec, Style, Surface, Ui,
};

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
    titles: Vec<String>,
    current: Option<usize>,
    /// The background, the current section, the played part, the
    /// lines between sections, the playhead, a title and the current
    /// section's title.
    colors: [Rgba; 7],
    /// The playhead's width in pixels.
    head: f32,
    /// A cell of the titles' face, in pixels.
    cell: Size,
    /// The room kept clear each side of a title, in pixels.
    inset: i32,
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
            titles: sections.iter().map(|s| s.title.clone()).collect(),
            current: playback::current(sections, time),
            colors: colors(palette),
            head: (style.px(ui, 2)) as f32,
            cell: ui.text_size(" ", style.face),
            inset: style.px(ui, style.inset),
        };
        if self.drawn.as_ref() != Some(&wanted) && pane.size.1 > 0 {
            let mut buffer = draw(&wanted);
            titles(ui, &mut buffer, &wanted, style.face);
            head(&mut buffer, &wanted);
            self.surface.present(buffer);
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

fn colors(palette: &Palette) -> [Rgba; 7] {
    let [red, green, blue, _] = palette.selection;
    [
        palette.dark_background,
        palette.lighter_background,
        [red, green, blue, 0.8],
        palette.border,
        palette.foreground,
        palette.muted,
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
    let x = |t: f64| (t / duration * bar.rect.width as f64) as i32;
    let (left, right) = (
        x(section.start),
        sections.get(i + 1).map_or(bar.rect.width, |s| x(s.start)),
    );
    let part = una::Rect {
        x: left,
        y: 0,
        width: right - left,
        height: bar.rect.height,
    };
    ui.tooltip_part(bar, part, i, &place);
}

/// The pixels of the bar as `drawn` describes it, but for the titles
/// and the playhead.
fn draw(drawn: &Drawn) -> Buffer {
    let (width, height) = drawn.size;
    let mut buffer = Buffer::new(width, height, Format::Rgba);
    let Some(mut pixmap) = sk::PixmapMut::from_bytes(buffer.pixels(), width, height) else {
        return buffer;
    };
    let [background, current, played, line, ..] = drawn.colors.map(sk_color);
    pixmap.fill(background);
    let (w, h) = (width as f32, height as f32);
    let x = |t: f64| (t / drawn.duration) as f32 * w;
    let mut fill = |left, top, right, bottom, color| {
        fill_rect(&mut pixmap, left, top, right, bottom, color);
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
    buffer
}

/// Write each section's title centred in its part of `buffer`, cut to
/// fit, or left out where not even two cells fit.
fn titles(ui: &mut Ui, buffer: &mut Buffer, drawn: &Drawn, face: Face) {
    let (width, height) = drawn.size;
    let x = |t: f64| (t / drawn.duration * width as f64) as i32;
    let y = (height as i32 - drawn.cell.height) / 2;
    for (i, title) in drawn.titles.iter().enumerate() {
        let left = x(drawn.starts[i].0);
        let right = drawn.starts.get(i + 1).map_or(width as i32, |s| x(s.0));
        let room = (right - left - 2 * drawn.inset) / drawn.cell.width.max(1);
        if room < 2 {
            continue;
        }
        let title = una::fit(title, room as usize);
        let color = drawn.colors[if drawn.current == Some(i) { 6 } else { 5 }];
        let text_width = ui.text_size(&title, face).width;
        let at = Point {
            x: left + (right - left - text_width) / 2,
            y,
        };
        ui.draw_text(buffer, at, face, color, &title);
    }
}

/// Draw the playhead on `buffer`, over the titles.
fn head(buffer: &mut Buffer, drawn: &Drawn) {
    let (width, height) = drawn.size;
    let Some(mut pixmap) = sk::PixmapMut::from_bytes(buffer.pixels(), width, height) else {
        return;
    };
    let at = (drawn.time / drawn.duration) as f32 * width as f32;
    let half = drawn.head / 2.0;
    let color = sk_color(drawn.colors[4]);
    fill_rect(&mut pixmap, at - half, 0.0, at + half, height as f32, color);
}

fn sk_color([r, g, b, a]: Rgba) -> sk::Color {
    sk::Color::from_rgba(r, g, b, a).unwrap_or(sk::Color::BLACK)
}

fn fill_rect(
    pixmap: &mut sk::PixmapMut,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    color: sk::Color,
) {
    let mut paint = sk::Paint::default();
    paint.set_color(color);
    if let Some(rect) = sk::Rect::from_ltrb(left, top, right, bottom) {
        pixmap.fill_rect(rect, &paint, sk::Transform::identity(), None);
    }
}

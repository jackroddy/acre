//! Text set in acre's embedded fonts: shaped into glyphs, broken into
//! lines, and measured. `scene` measures labels with this module and
//! `render` draws them from it, so the two agree on every position.

use std::sync::LazyLock;

use rustybuzz::{Face, UnicodeBuffer};

use crate::scene::{Color, Font, HAlign, Point, Run, VAlign};

static MONO: &[u8] = include_bytes!("../assets/fonts/JetBrainsMonoNL-Regular.ttf");
static SANS: &[u8] = include_bytes!("../assets/fonts/NotoSans-Regular.ttf");
static SANS_BOLD: &[u8] = include_bytes!("../assets/fonts/NotoSans-Bold.ttf");
static SANS_ITALIC: &[u8] = include_bytes!("../assets/fonts/NotoSans-Italic.ttf");

static FACES: LazyLock<[Face<'static>; 4]> = LazyLock::new(|| {
    let face = |data| Face::from_slice(data, 0).expect("the embedded font parses");
    [face(MONO), face(SANS), face(SANS_BOLD), face(SANS_ITALIC)]
});

/// The distance from one baseline to the next, in multiples of the
/// text size.
pub const LINE_HEIGHT: f64 = 1.3;

/// The parsed face of `font`.
pub fn face(font: Font) -> &'static Face<'static> {
    &FACES[match font {
        Font::Mono => 0,
        Font::Sans => 1,
        Font::SansBold => 2,
        Font::SansItalic => 3,
    }]
}

/// One shaped glyph, placed from the start of its line's baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub font: Font,
    pub id: u16,
    pub x: f64,
    /// Up from the baseline is negative, as on the canvas.
    pub y: f64,
    pub color: Color,
}

/// One line of shaped text.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub glyphs: Vec<Glyph>,
    /// The sum of the glyphs' advances.
    pub width: f64,
}

/// Runs shaped into lines at one size.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub lines: Vec<Line>,
    pub size: f64,
    /// The height above the baseline of the first run's font.
    pub ascent: f64,
    /// The depth below the baseline of the first run's font, positive.
    pub descent: f64,
    /// The height of a capital in the first run's font.
    pub cap_height: f64,
}

impl Layout {
    pub fn width(&self) -> f64 {
        self.lines.iter().map(|l| l.width).fold(0.0, f64::max)
    }

    pub fn line_height(&self) -> f64 {
        LINE_HEIGHT * self.size
    }

    /// The left end of each line's baseline when the text's `origin`
    /// sits where `h_align` and `v_align` say: `Top` at the first line's
    /// ascent, `Middle` at half the capital height of the middle of the
    /// lines, `Baseline` on the first baseline.
    pub fn place(&self, origin: Point, h_align: HAlign, v_align: VAlign) -> Vec<Point> {
        let lines = self.lines.len().max(1) as f64;
        let first_baseline = origin.y
            + match v_align {
                VAlign::Top => self.ascent,

                // half the capital height rather than half the ascent
                // and descent, so a digit or a capital sits in the
                // middle of a grid cell
                VAlign::Middle => self.cap_height / 2.0 - (lines - 1.0) * self.line_height() / 2.0,
                VAlign::Baseline => 0.0,
            };
        self.lines
            .iter()
            .enumerate()
            .map(|(k, line)| {
                let x = origin.x
                    - match h_align {
                        HAlign::Left => 0.0,
                        HAlign::Center => line.width / 2.0,
                        HAlign::Right => line.width,
                    };
                Point::new(x, first_baseline + k as f64 * self.line_height())
            })
            .collect()
    }

    /// The box the text covers when placed as in [`Layout::place`], from
    /// the first line's ascent to the last line's descent, as its
    /// top-left and bottom-right corners.
    pub fn extent(&self, origin: Point, h_align: HAlign, v_align: VAlign) -> (Point, Point) {
        let starts = self.place(origin, h_align, v_align);
        let left = starts.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
        let right = starts
            .iter()
            .zip(&self.lines)
            .map(|(p, l)| p.x + l.width)
            .fold(f64::NEG_INFINITY, f64::max);
        let top = starts.first().map_or(origin.y, |p| p.y - self.ascent);
        let bottom = starts.last().map_or(origin.y, |p| p.y + self.descent);
        (Point::new(left, top), Point::new(right, bottom))
    }
}

/// `runs` shaped at `size` pixels and broken into lines at each `\n`.
pub fn layout(runs: &[Run], size: f64) -> Layout {
    let first = face(runs.first().map_or(Font::Mono, |r| r.font));
    let per_unit = |face: &Face| size / face.units_per_em() as f64;
    let mut lines = vec![Line {
        glyphs: Vec::new(),
        width: 0.0,
    }];
    for run in runs {
        let face = face(run.font);
        let scale = per_unit(face);
        for (k, piece) in run.text.split('\n').enumerate() {
            if k > 0 {
                lines.push(Line {
                    glyphs: Vec::new(),
                    width: 0.0,
                });
            }
            if piece.is_empty() {
                continue;
            }
            let line = lines.last_mut().expect("there is always a line");
            let mut buffer = UnicodeBuffer::new();
            buffer.push_str(piece);
            let shaped = rustybuzz::shape(face, &[], buffer);
            for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                line.glyphs.push(Glyph {
                    font: run.font,
                    id: info.glyph_id as u16,
                    x: line.width + pos.x_offset as f64 * scale,
                    y: -pos.y_offset as f64 * scale,
                    color: run.color,
                });
                line.width += pos.x_advance as f64 * scale;
            }
        }
    }
    let scale = per_unit(first);
    Layout {
        lines,
        size,
        ascent: first.ascender() as f64 * scale,
        descent: -(first.descender() as f64) * scale,
        cap_height: first.capital_height().unwrap_or(first.ascender()) as f64 * scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(text: &str, font: Font) -> Run {
        Run {
            text: text.into(),
            font,
            color: Color::BLACK,
        }
    }

    /// The sum of each character's advance with no shaping, in pixels.
    fn unkerned(text: &str, font: Font, size: f64) -> f64 {
        let face = face(font);
        let scale = size / face.units_per_em() as f64;
        text.chars()
            .map(|c| {
                let g = face.glyph_index(c).unwrap_or_default();
                face.glyph_hor_advance(g).unwrap_or(0) as f64 * scale
            })
            .sum()
    }

    #[test]
    fn a_kerned_pair_is_narrower_than_its_advances() {
        let width = layout(&[run("AV", Font::Sans)], 100.0).width();
        assert!(
            width < unkerned("AV", Font::Sans, 100.0) - 1.0,
            "AV is {width} wide"
        );
    }

    #[test]
    fn runs_follow_one_another_and_lines_break_at_newlines() {
        let l = layout(&[run("ab", Font::Mono), run("c\nde", Font::Sans)], 20.0);
        assert_eq!(l.lines.len(), 2);
        let first = &l.lines[0];
        assert_eq!(first.glyphs.len(), 3);
        assert_eq!(first.glyphs[2].font, Font::Sans);
        assert!((first.glyphs[2].x - unkerned("ab", Font::Mono, 20.0)).abs() < 1e-9);
        assert_eq!(l.lines[1].glyphs.len(), 2);
    }

    #[test]
    fn placing_centers_each_line_and_steps_down_one_line_height() {
        let l = layout(&[run("aaaa\naa", Font::Mono)], 10.0);
        let starts = l.place(Point::new(100.0, 50.0), HAlign::Center, VAlign::Baseline);
        assert!((starts[0].x - (100.0 - l.lines[0].width / 2.0)).abs() < 1e-9);
        assert!((starts[1].x - (100.0 - l.lines[1].width / 2.0)).abs() < 1e-9);
        assert_eq!(starts[1].y - starts[0].y, 13.0);
    }
}

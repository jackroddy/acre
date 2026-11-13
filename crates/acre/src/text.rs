//! Text set in acre's embedded fonts: shaped into glyphs, broken into
//! lines, and measured. `scene` measures labels with this module and
//! `render` draws them from it, so the two agree on every position.

use std::sync::LazyLock;

use rustybuzz::{Face, UnicodeBuffer};

use crate::math::MathFont;
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
    match font {
        Font::Mono => &FACES[0],
        Font::Sans => &FACES[1],
        Font::SansBold => &FACES[2],
        Font::SansItalic => &FACES[3],
        Font::Math => &MathFont::noto().face,
    }
}

/// One shaped glyph, placed from the start of its line's baseline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Glyph {
    pub font: Font,
    pub id: u16,
    pub x: f64,
    /// Up from the baseline is negative, as on the canvas.
    pub y: f64,
    /// How far the line moves on after this glyph.
    pub advance: f64,
    /// Whether the glyph sets a space, where a line may break.
    pub space: bool,
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

/// `runs` shaped at `size` pixels and broken into lines at each `\n`,
/// and at spaces to keep lines within `width` when it is given. A word
/// wider than `width` takes a line of its own.
pub fn layout(runs: &[Run], size: f64, width: Option<f64>) -> Layout {
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
                let advance = pos.x_advance as f64 * scale;
                line.glyphs.push(Glyph {
                    font: run.font,
                    id: info.glyph_id as u16,
                    x: line.width + pos.x_offset as f64 * scale,
                    y: -pos.y_offset as f64 * scale,
                    advance,
                    space: piece[info.cluster as usize..].starts_with(' '),
                    color: run.color,
                });
                line.width += advance;
            }
        }
    }
    if let Some(width) = width {
        lines = lines.into_iter().flat_map(|l| wrap(l, width)).collect();
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

/// `line` broken at spaces into lines no wider than `width`, greedily.
/// The space at a break is dropped. A space between mono glyphs in a
/// line that also holds other fonts is inside a code span, and a break
/// goes there only when no other space is on the line.
fn wrap(line: Line, width: f64) -> Vec<Line> {
    let glyphs = line.glyphs;
    let has_code =
        glyphs.iter().any(|g| g.font == Font::Mono) && glyphs.iter().any(|g| g.font != Font::Mono);
    let in_code = |i: usize| {
        has_code
            && i > 0
            && glyphs.get(i + 1).is_some()
            && glyphs[i - 1].font == Font::Mono
            && glyphs[i + 1].font == Font::Mono
    };
    let mut out = Vec::new();
    let mut start = 0;
    let mut last_space = None;
    let mut last_code_space = None;
    let mut i = 0;
    while i < glyphs.len() {
        let left = glyphs[start].x;
        let end = glyphs[i].x + glyphs[i].advance - left;
        if end > width
            && !glyphs[i].space
            && let Some(space) = last_space.or(last_code_space)
        {
            out.push(line_of(&glyphs[start..space]));
            start = space + 1;
            last_space = None;
            last_code_space = None;
            i = start;
            continue;
        }
        if glyphs[i].space {
            if in_code(i) {
                last_code_space = Some(i);
            } else {
                last_space = Some(i);
            }
        }
        i += 1;
    }
    out.push(line_of(&glyphs[start..]));
    out
}

/// A line of `glyphs` moved to start at 0, without trailing spaces in
/// its width.
fn line_of(glyphs: &[Glyph]) -> Line {
    let left = glyphs.first().map_or(0.0, |g| g.x);
    let glyphs: Vec<Glyph> = glyphs
        .iter()
        .map(|g| Glyph {
            x: g.x - left,
            ..*g
        })
        .collect();
    let width = glyphs
        .iter()
        .rev()
        .find(|g| !g.space)
        .map_or(0.0, |g| g.x + g.advance);
    Line { glyphs, width }
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
        let width = layout(&[run("AV", Font::Sans)], 100.0, None).width();
        assert!(
            width < unkerned("AV", Font::Sans, 100.0) - 1.0,
            "AV is {width} wide"
        );
    }

    #[test]
    fn runs_follow_one_another_and_lines_break_at_newlines() {
        let l = layout(
            &[run("ab", Font::Mono), run("c\nde", Font::Sans)],
            20.0,
            None,
        );
        assert_eq!(l.lines.len(), 2);
        let first = &l.lines[0];
        assert_eq!(first.glyphs.len(), 3);
        assert_eq!(first.glyphs[2].font, Font::Sans);
        assert!((first.glyphs[2].x - unkerned("ab", Font::Mono, 20.0)).abs() < 1e-9);
        assert_eq!(l.lines[1].glyphs.len(), 2);
    }

    #[test]
    fn lines_wrap_at_spaces_within_the_width_and_keep_their_runs() {
        // each mono character is 6 pixels wide at size 10
        let char = unkerned("a", Font::Mono, 10.0);
        let l = layout(
            &[run("aaa bbb ", Font::Mono), run("ccc dddddddd", Font::Sans)],
            10.0,
            Some(char * 7.5),
        );
        let texts: Vec<usize> = l.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(texts, [7, 3, 8]);
        assert!(l.lines[0].width <= char * 7.5);
        assert_eq!(l.lines[1].glyphs[0].font, Font::Sans);
        assert_eq!(l.lines[1].glyphs[0].x, 0.0);
        // the last word is wider than the width, so it overflows alone
        assert!(l.lines[2].width > char * 7.5);
    }

    #[test]
    fn a_line_breaks_outside_a_code_span_before_inside_one() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [run("xx ", Font::Sans), run("a b c", Font::Mono)];
        // "xx a b" would fit, but the break goes before the span
        let l = layout(&runs, 10.0, Some(char * 6.0));
        let widths: Vec<usize> = l.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(widths, [2, 5]);
    }

    #[test]
    fn a_code_span_wider_than_the_width_breaks_at_its_own_spaces() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [run("xx ", Font::Sans), run("aa bb cc", Font::Mono)];
        let l = layout(&runs, 10.0, Some(char * 4.0));
        let widths: Vec<usize> = l.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(widths, [2, 2, 2, 2]);
    }

    #[test]
    fn placing_centers_each_line_and_steps_down_one_line_height() {
        let l = layout(&[run("aaaa\naa", Font::Mono)], 10.0, None);
        let starts = l.place(Point::new(100.0, 50.0), HAlign::Center, VAlign::Baseline);
        assert!((starts[0].x - (100.0 - l.lines[0].width / 2.0)).abs() < 1e-9);
        assert!((starts[1].x - (100.0 - l.lines[1].width / 2.0)).abs() < 1e-9);
        assert_eq!(starts[1].y - starts[0].y, 13.0);
    }
}

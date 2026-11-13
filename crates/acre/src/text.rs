//! Text set in acre's embedded fonts: shaped into glyphs, broken into
//! lines, and measured. `scene` measures labels with this module and
//! `render` draws them from it, so the two agree on every position.

use std::ops::Range;

use rustybuzz::{Face, UnicodeBuffer};

use crate::fonts::Fonts;
use crate::math;
use crate::scene::{Color, Font, HAlign, Point, Run, VAlign};

/// The distance from one baseline to the next, in multiples of the
/// text size.
pub const LINE_HEIGHT: f64 = 1.3;

/// What a glyph draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mark {
    /// Glyph `id` of `font`, set at `size` pixels to the em.
    Outline { font: Font, id: u16, size: f64 },
    /// A filled rectangle with its bottom-left corner on the glyph's
    /// position, such as the bar of a fraction.
    Rule { width: f64, height: f64 },
}

/// One shaped glyph, placed from the start of its line's baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct Glyph {
    pub mark: Mark,
    /// The bytes of [`Layout::text`] the glyph sets, empty for a glyph
    /// of a formula.
    pub text: Range<usize>,
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
    /// The distance from the start of the line to the end of its last
    /// glyph's advance.
    pub width: f64,
}

/// Runs shaped into lines at one size.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub lines: Vec<Line>,
    /// The text of every run, one after another.
    pub text: String,
    pub size: f64,
    /// The height above the baseline of the first run's font, or of the
    /// formulas on the first line where they are taller.
    pub ascent: f64,
    /// The depth below the baseline of the first run's font, positive,
    /// or of the formulas on the last line where they are deeper.
    pub descent: f64,
    /// The height of a capital in the first run's font.
    pub cap_height: f64,
    /// Whether the lines were wrapped and justified, so they align as
    /// one block.
    pub justify: bool,
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
    /// lines, `Baseline` on the first baseline. Justified lines share
    /// one left edge.
    pub fn place(&self, origin: Point, h_align: HAlign, v_align: VAlign) -> Vec<Point> {
        let block = self.width();
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
                let width = if self.justify { block } else { line.width };
                let x = origin.x
                    - match h_align {
                        HAlign::Left => 0.0,
                        HAlign::Center => width / 2.0,
                        HAlign::Right => width,
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

/// `runs` set in `fonts`, shaped at `size` pixels and broken into lines at each `\n`,
/// and at spaces to keep lines within `width` when it is given. A word
/// wider than `width` takes a line of its own. With `justify`, the
/// wrapped lines are stretched to fill `width`, as [`wrap`] says.
pub fn layout(fonts: &Fonts, runs: &[Run], size: f64, width: Option<f64>, justify: bool) -> Layout {
    let first = match runs.first().map_or(Font::Mono, |r| r.font) {
        Font::Math => fonts.face(Font::Sans),
        font => fonts.face(font),
    };
    let per_unit = |face: &Face| size / face.units_per_em() as f64;
    let mut lines = vec![Line {
        glyphs: Vec::new(),
        width: 0.0,
    }];
    let mut text = String::new();
    for run in runs {
        let run_start = text.len();
        text.push_str(&run.text);
        if run.font == Font::Math
            && let Ok(formula) = math::layout(&run.text, size, fonts)
        {
            let line = lines.last_mut().expect("there is always a line");
            push_formula(line, formula, run_start, run.color);
            continue;
        }

        // a formula that does not parse is set as its source, which
        // markup has already refused, so only a run made in code gets
        // here
        let font = if run.font == Font::Math {
            Font::Sans
        } else {
            run.font
        };
        let face = fonts.face(font);
        let scale = per_unit(face);
        let mut piece_start = run_start;
        for (k, piece) in run.text.split('\n').enumerate() {
            let start = piece_start;
            piece_start += piece.len() + 1;
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
            let mut clusters: Vec<usize> = shaped
                .glyph_infos()
                .iter()
                .map(|info| info.cluster as usize)
                .collect();
            clusters.sort_unstable();
            clusters.dedup();
            for (info, pos) in shaped.glyph_infos().iter().zip(shaped.glyph_positions()) {
                let advance = pos.x_advance as f64 * scale;
                let cluster = info.cluster as usize;
                let end = clusters
                    .iter()
                    .find(|&&c| c > cluster)
                    .map_or(piece.len(), |&c| c);
                line.glyphs.push(Glyph {
                    mark: Mark::Outline {
                        font,
                        id: info.glyph_id as u16,
                        size,
                    },
                    text: start + cluster..start + end,
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
        lines = lines
            .into_iter()
            .flat_map(|l| wrap(l, width, justify))
            .collect();
    }
    let scale = per_unit(first);
    let ascent = lines.first().map_or(0.0, |l| -ink(fonts, l).0);
    let descent = lines.last().map_or(0.0, |l| ink(fonts, l).1);
    Layout {
        lines,
        text,
        size,
        ascent: (first.ascender() as f64 * scale).max(ascent),
        descent: (-(first.descender() as f64) * scale).max(descent),
        cap_height: first.capital_height().unwrap_or(first.ascender()) as f64 * scale,
        justify: justify && width.is_some(),
    }
}

/// Append `formula` to `line` as one unbreakable stretch of glyphs.
fn push_formula(line: &mut Line, formula: math::MathBox, text_start: usize, color: Color) {
    let start = line.width;
    let end = start + formula.width;

    // every glyph of the formula advances to its right edge, so a line
    // that ends in it is measured to there whatever order its glyphs
    // are in, and an empty rule at its left edge comes first, so a line
    // that starts with it starts there
    let marker = math::Piece {
        x: 0.0,
        y: 0.0,
        mark: Mark::Rule {
            width: 0.0,
            height: 0.0,
        },
    };
    for piece in std::iter::once(marker).chain(formula.pieces) {
        let x = start + piece.x;
        line.glyphs.push(Glyph {
            mark: piece.mark,
            text: text_start..text_start,
            x,
            y: piece.y,
            advance: end - x,
            space: false,
            color,
        });
    }
    line.width = end;
}

/// The top and bottom of the ink of the formulas in `line`, from its
/// baseline with down positive, or zeros when it holds none.
fn ink(fonts: &Fonts, line: &Line) -> (f64, f64) {
    let mut top: f64 = 0.0;
    let mut bottom: f64 = 0.0;
    for g in &line.glyphs {
        let (high, low) = match g.mark {
            Mark::Rule { height, .. } => (height, 0.0),
            Mark::Outline {
                font: Font::Math,
                id,
                size,
            } => {
                let face = fonts.face(Font::Math);
                let scale = size / face.units_per_em() as f64;
                match face.glyph_bounding_box(ttf_parser::GlyphId(id)) {
                    Some(b) => (b.y_max as f64 * scale, b.y_min as f64 * scale),
                    None => continue,
                }
            }
            Mark::Outline { .. } => continue,
        };
        top = top.min(g.y - high);
        bottom = bottom.max(g.y - low);
    }
    (top, bottom)
}

/// `line` broken at spaces into lines no wider than `width`. The space
/// at a break is dropped. A space between mono glyphs in a line that
/// also holds other fonts is inside a code span, and a break goes there
/// only when the line has no other place to break.
///
/// Without `justify` the breaks are greedy. With it they are chosen
/// for the whole line at once by [`even_breaks`], and every line but
/// the last has its spaces outside code spans stretched or shrunk to
/// fill `width`.
fn wrap(line: Line, width: f64, justify: bool) -> Vec<Line> {
    let glyphs = line.glyphs;
    let mono = |g: &Glyph| {
        matches!(
            g.mark,
            Mark::Outline {
                font: Font::Mono,
                ..
            }
        )
    };
    let has_code = glyphs.iter().any(mono) && !glyphs.iter().all(mono);
    let code: Vec<bool> = (0..glyphs.len())
        .map(|i| {
            has_code
                && glyphs[i].space
                && i > 0
                && i + 1 < glyphs.len()
                && mono(&glyphs[i - 1])
                && mono(&glyphs[i + 1])
        })
        .collect();
    let breaks = if justify {
        even_breaks(&glyphs, &code, width)
    } else {
        greedy_breaks(&glyphs, &code, width)
    };
    let mut out = Vec::new();
    let mut start = 0;
    for space in breaks {
        let mut line = line_of(&glyphs[start..space]);
        if justify {
            stretch(&mut line, &code[start..space], width);
        }
        out.push(line);
        start = space + 1;
    }
    out.push(line_of(&glyphs[start..]));
    out
}

/// The spaces to break `glyphs` at, each line filled as far as it fits
/// in `width` before the next starts.
fn greedy_breaks(glyphs: &[Glyph], code: &[bool], width: f64) -> Vec<usize> {
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
            out.push(space);
            start = space + 1;
            last_space = None;
            last_code_space = None;
            i = start;
            continue;
        }
        if glyphs[i].space {
            if code[i] {
                last_code_space = Some(i);
            } else {
                last_space = Some(i);
            }
        }
        i += 1;
    }
    out
}

/// How far a space may stretch, as a share of its width.
//
// TeX's Computer Modern sets a space of 1/3 em with a stretch of 1/6 em
const STRETCH: f64 = 0.5;

/// How far a space may shrink, as a share of its width.
//
// TeX's Computer Modern sets a space of 1/3 em with a shrink of 1/9 em
const SHRINK: f64 = 1.0 / 3.0;

/// The badness of a line that cannot stretch to fill the width, or
/// that overflows it.
//
// TeX's `inf_bad` is 10000, but here a stretched line's badness is not
// capped there, and a line of one word set ragged inside a justified
// paragraph reads worse than a line whose spaces are stretched to
// about 12 times their width, where 100 r^3 = 1e6
const INF_BAD: f64 = 1e6;

/// The demerits every line adds, as TeX's `\linepenalty`, so that of
/// two sets of breaks equally even the one with fewer lines costs less.
const LINE_PENALTY: f64 = 10.0;

/// The penalty for a break inside a code span, far above any number of
/// lines at [`INF_BAD`].
const CODE_BREAK: f64 = 1e9;

/// The share of the width below which a last line is short.
const SHORT_LAST: f64 = 1.0 / 3.0;

/// The penalty for an empty last line under longer lines, falling to
/// nothing as the line grows to [`SHORT_LAST`] of the width.
//
// at 150 the breaker leaves one short word on the last line rather
// than stretch an earlier line's spaces to about 1.6 times their width
const SHORT_LAST_PENALTY: f64 = 150.0;

/// The spaces to break `glyphs` at so the lines come out about as long
/// as one another within `width`, by the method of Knuth and Plass,
/// "Breaking Paragraphs into Lines" (1981).
///
/// Spaces outside code spans may stretch by [`STRETCH`] and shrink by
/// [`SHRINK`] of their width. The last line costs nothing for being
/// short, but one shorter than [`SHORT_LAST`] of `width` pays up to
/// [`SHORT_LAST_PENALTY`]. A line wider than `width` once shrunk is
/// allowed only when no break inside it exists.
fn even_breaks(glyphs: &[Glyph], code: &[bool], width: f64) -> Vec<usize> {
    let n = glyphs.len();

    // a line ends at the first space of a run of spaces, or at the end
    let mut ends: Vec<usize> = (1..n)
        .filter(|&i| glyphs[i].space && !glyphs[i - 1].space)
        .collect();
    ends.push(n);

    // what: the fewest demerits that reach each end, and the end the
    //       line before it ended at, or `None` when it is the first
    // why: every line runs forward, so `best[k]` is final by the time
    //      the lines that start after end `k` are tried
    let mut best = vec![f64::INFINITY; ends.len()];
    let mut before: Vec<Option<usize>> = vec![None; ends.len()];
    for from in std::iter::once(None).chain((0..ends.len() - 1).map(Some)) {
        let (start, reached) = match from {
            None => (0, 0.0),
            Some(k) => (ends[k] + 1, best[k]),
        };
        if reached.is_infinite() {
            continue;
        }
        for (k, &end) in ends.iter().enumerate().skip(from.map_or(0, |k| k + 1)) {
            let line = &glyphs[start..end];
            let natural = natural_width(line);
            let last = end == n;
            let gaps: f64 = gaps(line, &code[start..end]).map(|i| line[i].advance).sum();
            let shortfall = width - natural;
            let overfull = if last {
                natural > width
            } else {
                -shortfall > gaps * SHRINK + 1e-9
            };
            let first = from.map_or(k == 0, |f| k == f + 1);
            if overfull && !first {
                break;
            }
            let badness = if overfull {
                INF_BAD
            } else if last || shortfall.abs() <= 1e-9 {
                0.0
            } else if shortfall < 0.0 {
                100.0 * (-shortfall / (gaps * SHRINK)).powi(3)
            } else if gaps > 0.0 {
                // uncapped, unlike TeX, so that of two loose lines the
                // looser costs more: a narrow width can leave no break
                // without one
                100.0 * (shortfall / (gaps * STRETCH)).powi(3)
            } else {
                INF_BAD
            };
            let mut demerits = (LINE_PENALTY + badness).powi(2);
            if !last && code[end] {
                demerits += CODE_BREAK * CODE_BREAK;
            }
            let short = width * SHORT_LAST;
            if last && from.is_some() && natural < short {
                let penalty = SHORT_LAST_PENALTY * (1.0 - natural / short);
                demerits += penalty * penalty;
            }
            if reached + demerits < best[k] {
                best[k] = reached + demerits;
                before[k] = from;
            }
        }
    }
    let mut out = Vec::new();
    let mut k = before[ends.len() - 1];
    while let Some(b) = k {
        out.push(ends[b]);
        k = before[b];
    }
    out.reverse();
    out
}

/// The width of `glyphs` set from the first one's place to the end of
/// the last one that is not a space.
fn natural_width(glyphs: &[Glyph]) -> f64 {
    let left = glyphs.first().map_or(0.0, |g| g.x);
    glyphs
        .iter()
        .rev()
        .find(|g| !g.space)
        .map_or(0.0, |g| g.x + g.advance - left)
}

/// The indices of the spaces in `glyphs` that justifying stretches:
/// those between its first and last glyphs that are not spaces, and
/// not inside a code span.
fn gaps<'a>(glyphs: &'a [Glyph], code: &'a [bool]) -> impl Iterator<Item = usize> + 'a {
    let first = glyphs.iter().position(|g| !g.space).unwrap_or(0);
    let last = glyphs.iter().rposition(|g| !g.space).unwrap_or(0);
    (first..last).filter(move |&i| glyphs[i].space && !code[i])
}

/// Widen or narrow the spaces of `line` that [`gaps`] picks, each in
/// proportion to its width, so the line fills `width`. A line that
/// would need its spaces narrowed past [`SHRINK`] is left as it is.
fn stretch(line: &mut Line, code: &[bool], width: f64) {
    let gaps: Vec<usize> = gaps(&line.glyphs, code).collect();
    let total: f64 = gaps.iter().map(|&i| line.glyphs[i].advance).sum();
    let extra = width - line.width;
    if total <= 0.0 || extra == 0.0 || -extra > total * SHRINK + 1e-9 {
        return;
    }
    let mut shift = 0.0;
    let mut next = gaps.iter().peekable();
    for (i, g) in line.glyphs.iter_mut().enumerate() {
        g.x += shift;
        if next.next_if_eq(&&i).is_some() {
            let more = extra * g.advance / total;
            g.advance += more;
            shift += more;
        }
    }
    line.width = width;
}

/// A line of `glyphs` moved to start at 0, without trailing spaces in
/// its width.
fn line_of(glyphs: &[Glyph]) -> Line {
    let left = glyphs.first().map_or(0.0, |g| g.x);
    let glyphs: Vec<Glyph> = glyphs
        .iter()
        .map(|g| Glyph {
            x: g.x - left,
            ..g.clone()
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

    fn layout(runs: &[Run], size: f64, width: Option<f64>) -> Layout {
        super::layout(&Fonts::default(), runs, size, width, false)
    }

    fn justified(runs: &[Run], size: f64, width: f64) -> Layout {
        super::layout(&Fonts::default(), runs, size, Some(width), true)
    }

    fn run(text: &str, font: Font) -> Run {
        Run {
            text: text.into(),
            font,
            color: Color::BLACK,
        }
    }

    /// The sum of each character's advance with no shaping, in pixels.
    fn unkerned(text: &str, font: Font, size: f64) -> f64 {
        let fonts = Fonts::default();
        let face = fonts.face(font);
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
        assert!(matches!(
            first.glyphs[2].mark,
            Mark::Outline {
                font: Font::Sans,
                ..
            }
        ));
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
        assert!(matches!(
            l.lines[1].glyphs[0].mark,
            Mark::Outline {
                font: Font::Sans,
                ..
            }
        ));
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
    fn a_formula_takes_its_width_in_the_line_and_never_breaks() {
        let math = Run {
            text: r"\displaystyle \frac{a+b}{c} + d".into(),
            font: Font::Math,
            color: Color::BLACK,
        };
        let formula = math::layout(&math.text, 20.0, &Fonts::default()).unwrap();
        let alone = layout(&[run("x ", Font::Sans)], 20.0, None);
        let l = layout(&[run("x ", Font::Sans), math.clone()], 20.0, None);
        assert!((l.width() - (alone.lines[0].width + formula.width)).abs() < 1e-9);
        assert!(
            l.ascent > alone.ascent,
            "the fraction is taller than the text"
        );
        let narrow = layout(&[run("x ", Font::Sans), math], 20.0, Some(1.0));
        assert_eq!(narrow.lines.len(), 2);
        assert!((narrow.lines[1].width - formula.width).abs() < 1e-9);
    }

    #[test]
    fn the_glyphs_carry_their_runs_text_once_per_cluster() {
        let l = layout(
            &[run("ab", Font::Mono), run("c\nfi ffl", Font::Sans)],
            20.0,
            None,
        );
        let mut copied = String::new();
        let mut last = None;
        for g in l.lines.iter().flat_map(|l| &l.glyphs) {
            if last != Some(g.text.clone()) {
                copied.push_str(&l.text[g.text.clone()]);
            }
            last = Some(g.text.clone());
        }
        assert_eq!(copied, "abcfi ffl");
    }

    #[test]
    fn placing_centers_each_line_and_steps_down_one_line_height() {
        let l = layout(&[run("aaaa\naa", Font::Mono)], 10.0, None);
        let starts = l.place(Point::new(100.0, 50.0), HAlign::Center, VAlign::Baseline);
        assert!((starts[0].x - (100.0 - l.lines[0].width / 2.0)).abs() < 1e-9);
        assert!((starts[1].x - (100.0 - l.lines[1].width / 2.0)).abs() < 1e-9);
        assert_eq!(starts[1].y - starts[0].y, 13.0);
    }

    /// The right end of the last glyph of `line` that is not a space.
    fn ink_end(line: &Line) -> f64 {
        let g = line.glyphs.iter().rev().find(|g| !g.space).unwrap();
        g.x + g.advance
    }

    #[test]
    fn justified_lines_fill_the_width_but_the_last() {
        let text = "the quick brown fox jumps over the lazy dog and then runs far away";
        let l = justified(&[run(text, Font::Sans)], 10.0, 120.0);
        assert!(l.lines.len() > 2);
        let (last, full) = l.lines.split_last().unwrap();
        for line in full {
            assert!((line.width - 120.0).abs() < 1e-9);
            assert!((ink_end(line) - 120.0).abs() < 1e-9);
        }
        assert!(last.width < 120.0);
        let words = l.text.split(' ').collect::<Vec<_>>();
        let tail =
            words[words.len() - last.glyphs.iter().filter(|g| g.space).count() - 1..].join(" ");
        let natural = layout(&[run(&tail, Font::Sans)], 10.0, None).width();
        assert!((last.width - natural).abs() < 1e-9);
    }

    #[test]
    fn justifying_leaves_the_spaces_in_a_code_span_alone() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [
            run("see ", Font::Sans),
            run("a b", Font::Mono),
            run(" for it and more words after it", Font::Sans),
        ];
        let l = justified(&runs, 10.0, 100.0);
        let first = &l.lines[0];
        assert!((first.width - 100.0).abs() < 1e-9);
        let space = first.glyphs.iter().position(|g| g.space).unwrap();
        let code = space + 2;
        assert!(first.glyphs[code].space);
        assert!((first.glyphs[code].advance - char).abs() < 1e-9);
        assert!((first.glyphs[code + 1].x - first.glyphs[code].x - char).abs() < 1e-9);
        assert!(first.glyphs[space].advance > unkerned(" ", Font::Sans, 10.0));
    }

    #[test]
    fn the_even_breaker_keeps_a_short_word_off_the_last_line() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [run("aa bb cc dd ee ff gg hh ii j", Font::Mono)];

        // the greedy breaks fill the first line and leave `j` alone
        let ragged = layout(&runs, 10.0, Some(char * 26.0));
        let counts: Vec<usize> = ragged.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(counts, [26, 1]);

        // spreading three characters over seven spaces costs less
        let even = justified(&runs, 10.0, char * 26.0);
        let counts: Vec<usize> = even.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(counts, [23, 4]);
        assert!((even.lines[0].width - char * 26.0).abs() < 1e-9);
    }

    #[test]
    fn a_paragraph_on_one_line_pays_nothing_for_being_short() {
        let char = unkerned("a", Font::Mono, 10.0);
        let l = justified(&[run("aa b", Font::Mono)], 10.0, char * 26.0);
        assert_eq!(l.lines.len(), 1);
        assert!((l.lines[0].width - 4.0 * char).abs() < 1e-9);
    }

    #[test]
    fn a_justified_line_shrinks_its_spaces_rather_than_stretch_the_one_before() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [run("aa bb cc dd ee ff gg hhhhhhhhhh", Font::Mono)];

        // the first 20 characters overflow 19 by one, which their six
        // spaces take by shrinking to 5/6 of their width
        let even = justified(&runs, 10.0, char * 19.0);
        let counts: Vec<usize> = even.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(counts, [20, 10]);
        let first = &even.lines[0];
        assert!((first.width - char * 19.0).abs() < 1e-9);
        assert!((ink_end(first) - char * 19.0).abs() < 1e-9);
        let space = first.glyphs.iter().find(|g| g.space).unwrap();
        assert!((space.advance - char * 5.0 / 6.0).abs() < 1e-9);

        // the greedy breaks never shrink
        let ragged = layout(&runs, 10.0, Some(char * 19.0));
        let counts: Vec<usize> = ragged.lines.iter().map(|l| l.glyphs.len()).collect();
        assert_eq!(counts, [17, 13]);
    }

    #[test]
    fn a_shrunk_line_leaves_the_spaces_in_a_code_span_alone() {
        let char = unkerned("a", Font::Mono, 10.0);
        let sans_space = unkerned(" ", Font::Sans, 10.0);
        let runs = [
            run("see ", Font::Sans),
            run("a b", Font::Mono),
            run(
                " for it and more words after it, and then a good many more",
                Font::Sans,
            ),
        ];

        // the width falls just short of the end of `words`
        let natural = layout(&runs, 10.0, None);
        let words = natural.text.find(" after").unwrap();
        let end = natural.lines[0]
            .glyphs
            .iter()
            .find(|g| g.text.start == words)
            .unwrap()
            .x;
        let l = justified(&runs, 10.0, end - 1.0);
        let first = &l.lines[0];
        assert!((first.width - (end - 1.0)).abs() < 1e-9);
        assert_eq!(&l.text[first.glyphs.last().unwrap().text.clone()], "s");
        let space = first.glyphs.iter().position(|g| g.space).unwrap();
        assert!(first.glyphs[space].advance < sans_space);
        let code = space + 2;
        assert!(first.glyphs[code].space);
        assert!((first.glyphs[code].advance - char).abs() < 1e-9);
    }

    #[test]
    fn the_even_breaker_breaks_inside_a_code_span_only_when_it_must() {
        let char = unkerned("a", Font::Mono, 10.0);
        let counts = |runs: &[Run], width: f64| -> Vec<usize> {
            let l = justified(runs, 10.0, width);
            l.lines.iter().map(|l| l.glyphs.len()).collect()
        };
        let fits = [run("xx ", Font::Sans), run("a b c", Font::Mono)];
        assert_eq!(counts(&fits, char * 6.0), [2, 5]);
        let wide = [run("xx ", Font::Sans), run("aa bb cc", Font::Mono)];
        assert_eq!(counts(&wide, char * 4.0), [2, 2, 2, 2]);
    }

    #[test]
    fn a_justified_line_ending_at_a_newline_keeps_its_spacing() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [run("aa bb\ncc dd ee ff gg hh", Font::Mono)];
        let l = justified(&runs, 10.0, char * 12.0);
        let widths: Vec<f64> = l.lines.iter().map(|l| l.width / char).collect();
        assert_eq!(l.lines.len(), 3);
        assert!((widths[0] - 5.0).abs() < 1e-9);
        assert!((widths[1] - 12.0).abs() < 1e-9);
        assert!(widths[2] < 12.0);
        assert!((l.lines[2].glyphs[3].x - 3.0 * char).abs() < 1e-9);
    }

    #[test]
    fn a_justified_line_of_one_word_stays_at_its_width() {
        let char = unkerned("a", Font::Mono, 10.0);
        let l = justified(&[run("aaaaaa bbbbbbbbb", Font::Mono)], 10.0, char * 10.0);
        assert_eq!(l.lines.len(), 2);
        assert!((l.lines[0].width - 6.0 * char).abs() < 1e-9);
    }

    #[test]
    fn justified_lines_share_a_left_edge_when_centered() {
        let char = unkerned("a", Font::Mono, 10.0);
        let l = justified(&[run("aa bb cc dd ee ff", Font::Mono)], 10.0, char * 9.0);
        let starts = l.place(Point::new(100.0, 0.0), HAlign::Center, VAlign::Baseline);
        assert!(
            starts
                .iter()
                .all(|p| (p.x - (100.0 - char * 4.5)).abs() < 1e-9)
        );
    }
}

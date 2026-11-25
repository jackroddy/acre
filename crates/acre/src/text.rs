//! Text set in acre's embedded fonts: shaped into glyphs, broken into
//! lines, and measured. `scene` measures labels with this module and
//! `render` draws them from it, so the two agree on every position.

use std::ops::Range;
use std::sync::LazyLock;

use hyphenation::{Hyphenator, Language, Load, Standard};

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
    /// of a formula or a hyphen added where a word breaks.
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
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Line {
    pub glyphs: Vec<Glyph>,
    /// The distance from the start of the line to the end of its last
    /// glyph's advance.
    pub width: f64,
    /// The space between this line and the one before it beyond one
    /// line height.
    pub gap: f64,
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
    /// Whether the lines align as one block: wrapped and justified, or
    /// holding a list item.
    pub block: bool,
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
    /// lines, `Baseline` on the first baseline. The lines of a block
    /// share one left edge.
    pub fn place(&self, origin: Point, h_align: HAlign, v_align: VAlign) -> Vec<Point> {
        let block = self.width();
        let lines = self.lines.len().max(1) as f64;
        let gaps: f64 = self.lines.iter().map(|l| l.gap).sum();
        let first_baseline = origin.y
            + match v_align {
                VAlign::Top => self.ascent,

                // half the capital height rather than half the ascent
                // and descent, so a digit or a capital sits in the
                // middle of a grid cell
                VAlign::Middle => {
                    self.cap_height / 2.0 - ((lines - 1.0) * self.line_height() + gaps) / 2.0
                }
                VAlign::Baseline => 0.0,
            };
        let mut below = 0.0;
        self.lines
            .iter()
            .enumerate()
            .map(|(k, line)| {
                below += line.gap;
                let width = if self.block { block } else { line.width };
                let x = origin.x
                    - match h_align {
                        HAlign::Left => 0.0,
                        HAlign::Center => width / 2.0,
                        HAlign::Right => width,
                    };
                Point::new(x, first_baseline + k as f64 * self.line_height() + below)
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
/// wrapped lines are stretched to fill `width`, as [`wrap`] says. A line
/// that starts with a bullet run is a list item, set as [`item`] says.
/// A character its run's font lacks is set in the font
/// [`Fonts::cover`] gives.
pub fn layout(fonts: &Fonts, runs: &[Run], size: f64, width: Option<f64>, justify: bool) -> Layout {
    let first_run = runs.iter().find(|r| !r.bullet);
    let first = match first_run.map_or(Font::Mono, |r| r.font) {
        Font::Math => fonts.face(Font::Sans),
        font => fonts.face(font),
    };
    let per_unit = |face: &Face| size / face.units_per_em() as f64;
    let mut lines = vec![Line::default()];

    // the number of bullet glyphs that start each line, 0 for a line
    // that is not a list item
    let mut bullets = vec![0];
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
        let mut piece_start = run_start;
        for (k, piece) in run.text.split('\n').enumerate() {
            let start = piece_start;
            piece_start += piece.len() + 1;
            if k > 0 {
                lines.push(Line::default());
                bullets.push(0);
            }
            let line = lines.last_mut().expect("there is always a line");
            for (range, font) in segments(fonts, font, piece) {
                shape(
                    fonts,
                    font,
                    &piece[range.clone()],
                    start + range.start,
                    size,
                    run.color,
                    line,
                );
            }
        }
        if run.bullet {
            let line = lines.last().expect("there is always a line");
            *bullets.last_mut().expect("one per line") = line.glyphs.len();
        }
    }
    let has_items = bullets.iter().any(|&b| b > 0);
    if width.is_some() || has_items {
        let mut out = Vec::with_capacity(lines.len());
        let mut after_item = false;
        for (line, marks) in lines.into_iter().zip(bullets) {
            let mut set = if marks > 0 {
                item(fonts, &text, line, marks, size, width, justify)
            } else if let Some(width) = width {
                wrap(fonts, &text, line, width, justify)
            } else {
                vec![line]
            };
            if let (true, Some(first)) = (after_item, set.first_mut()) {
                first.gap = ITEM_GAP * size;
            }
            out.extend(set);
            after_item = marks > 0;
        }
        lines = out;
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
        block: (justify && width.is_some()) || has_items,
    }
}

/// `piece` cut where the font that sets it changes, each stretch with
/// the font [`Fonts::cover`] gives for text in `font`. A character no
/// font has stays in the stretch before it.
fn segments(fonts: &Fonts, font: Font, piece: &str) -> Vec<(Range<usize>, Font)> {
    let mut out: Vec<(Range<usize>, Font)> = Vec::new();
    for (i, c) in piece.char_indices() {
        let end = i + c.len_utf8();
        let set = fonts
            .cover(font, c)
            .or(out.last().map(|s| s.1))
            .unwrap_or(font);
        match out.last_mut() {
            Some((range, last)) if *last == set => range.end = end,
            _ => out.push((i..end, set)),
        }
    }
    out
}

/// Append `text` shaped in `font` at `size` to `line`. `start` is where
/// `text` starts in [`Layout::text`].
fn shape(
    fonts: &Fonts,
    font: Font,
    text: &str,
    start: usize,
    size: f64,
    color: Color,
    line: &mut Line,
) {
    let face = fonts.face(font);
    let scale = size / face.units_per_em() as f64;
    let mut buffer = UnicodeBuffer::new();
    buffer.push_str(text);
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
            .map_or(text.len(), |&c| c);
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
            space: text[cluster..].starts_with(' '),
            color,
        });
        line.width += advance;
    }
}

/// How far a list item's text starts from its bullet's left edge, in
/// multiples of the text size.
const INDENT: f64 = 1.0;

/// The space after a list item beyond one line height, in multiples
/// of the text size.
const ITEM_GAP: f64 = 0.3;

/// The list item `line`, whose first `marks` glyphs are its bullet, set
/// with its text [`INDENT`] from the bullet. The text wraps within what
/// is left of `width`, as [`wrap`] says, and its lines hang at the
/// indent.
fn item(
    fonts: &Fonts,
    text: &str,
    line: Line,
    marks: usize,
    size: f64,
    width: Option<f64>,
    justify: bool,
) -> Vec<Line> {
    let indent = INDENT * size;
    let mut bullet = line.glyphs;
    let body = bullet.split_off(marks);
    let left = body.first().map_or(line.width, |g| g.x);
    let body = Line {
        glyphs: body
            .into_iter()
            .map(|g| Glyph { x: g.x - left, ..g })
            .collect(),
        width: line.width - left,
        gap: 0.0,
    };
    let mut lines = match width {
        Some(width) => wrap(fonts, text, body, (width - indent).max(0.0), justify),
        None => vec![body],
    };
    for line in &mut lines {
        for g in &mut line.glyphs {
            g.x += indent;
        }
        line.width += indent;
    }
    lines[0].glyphs.splice(0..0, bullet);
    lines
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
/// for the whole line at once by [`even_breaks`], among the spaces and
/// the places [`breaks`] finds inside words, and every line but the
/// last has its spaces outside code spans stretched or shrunk to fill
/// `width`.
fn wrap(fonts: &Fonts, text: &str, line: Line, width: f64, justify: bool) -> Vec<Line> {
    let glyphs = line.glyphs;
    let mono = |g: &Glyph| {
        matches!(
            g.mark,
            Mark::Outline {
                font: Font::Mono | Font::MonoBold,
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
    if !justify {
        let mut out = Vec::new();
        let mut start = 0;
        for space in greedy_breaks(&glyphs, &code, width) {
            out.push(line_of(&glyphs[start..space]));
            start = space + 1;
        }
        out.push(line_of(&glyphs[start..]));
        return out;
    }
    even_lines(&glyphs, &code, &breaks(fonts, text, &glyphs), width)
}

/// `glyphs` broken at the `breaks` [`even_breaks`] chooses, every line
/// but the last stretched or shrunk to fill `width`.
fn even_lines(glyphs: &[Glyph], code: &[bool], breaks: &[Break], width: f64) -> Vec<Line> {
    let mut out = Vec::new();
    let mut from = &breaks[0];
    for k in even_breaks(glyphs, code, breaks, width) {
        let to = &breaks[k];
        let (set, set_code) = joined(glyphs, code, from, to);
        let mut line = line_of(&set);
        if k + 1 < breaks.len() {
            stretch(&mut line, &set_code, width);
        }
        out.push(line);
        from = to;
    }
    out
}

/// A place a line may end, and where the line after it starts.
struct Break {
    /// The glyph the line's own glyphs end before.
    end: usize,
    /// The glyph the next line's own glyphs start at.
    next: usize,
    /// The first part of a word broken here, with its hyphen, set
    /// after the line's own glyphs.
    tail: Vec<Glyph>,
    /// The rest of a word broken here, set before the next line's own
    /// glyphs.
    head: Vec<Glyph>,
    /// Whether the break is inside a word.
    hyphen: bool,
}

impl Break {
    fn space(end: usize, next: usize) -> Break {
        Break {
            end,
            next,
            tail: Vec::new(),
            head: Vec::new(),
            hyphen: false,
        }
    }
}

/// Where `glyphs` may break, in order: the start, the first space of
/// each run of spaces, the places [`hyphens`] finds inside each word,
/// and the end.
fn breaks(fonts: &Fonts, text: &str, glyphs: &[Glyph]) -> Vec<Break> {
    let n = glyphs.len();
    let mut out = vec![Break::space(0, 0)];
    let mut word = 0;
    for i in 0..=n {
        if i < n && !glyphs[i].space {
            continue;
        }
        if word < i {
            out.extend(hyphens(fonts, text, glyphs, word..i));
            if i < n {
                out.push(Break::space(i, i + 1));
            }
        }
        word = i + 1;
    }
    out.push(Break::space(n, n));
    out
}

/// The fewest letters a word breaks inside.
const SHORTEST_BROKEN: usize = 5;

/// English hyphenation patterns, with at least 2 letters before a
/// hyphen and 3 after it.
//
// 2 and 3 are the left and right minima the en-us patterns give for
// typesetting, as plain TeX's `\lefthyphenmin` and `\righthyphenmin`
static ENGLISH: LazyLock<Standard> = LazyLock::new(|| {
    let mut patterns =
        Standard::from_embedded(Language::EnglishUS).expect("the English patterns are embedded");
    patterns.minima = (2, 3);
    patterns
});

/// The places the word `glyphs[word]` breaks: after each hyphen it
/// holds, or else where the English patterns allow, with a hyphen
/// added to the first part. A word breaks only when one font, size and
/// color set it and that font is not mono or math, and only between
/// letters: a word with a digit or an apostrophe in it stays whole.
fn hyphens(fonts: &Fonts, text: &str, glyphs: &[Glyph], word: Range<usize>) -> Vec<Break> {
    let set = &glyphs[word.clone()];
    let Mark::Outline { font, size, .. } = set[0].mark else {
        return Vec::new();
    };
    let color = set[0].color;
    let one_face = set.iter().all(|g| {
        g.color == color
            && matches!(g.mark, Mark::Outline { font: f, size: s, .. } if f == font && s == size)
    });
    if !one_face || matches!(font, Font::Mono | Font::MonoBold | Font::Math) {
        return Vec::new();
    }
    let start = set.iter().map(|g| g.text.start).min().unwrap_or(0);
    let end = set.iter().map(|g| g.text.end).max().unwrap_or(0);
    let source = &text[start..end];
    let (Some(first), Some(last)) = (
        source.find(char::is_alphanumeric),
        source.rfind(char::is_alphanumeric),
    ) else {
        return Vec::new();
    };
    let core_start = start + first;
    let last_end = last + source[last..].chars().next().map_or(0, char::len_utf8);
    let core = &source[first..last_end];
    if !core.chars().all(|c| c.is_alphabetic() || c == '-') {
        return Vec::new();
    }
    if core.contains('-') {
        return core
            .match_indices('-')
            .filter_map(|(i, _)| {
                let at = core_start + i + 1;
                let c = word.start + set.iter().position(|g| g.text.start == at)?;
                Some(Break {
                    end: word.start,
                    next: word.end,
                    tail: line_of(&glyphs[word.start..c]).glyphs,
                    head: line_of(&glyphs[c..word.end]).glyphs,
                    hyphen: true,
                })
            })
            .collect();
    }
    if core.chars().count() < SHORTEST_BROKEN {
        return Vec::new();
    }
    ENGLISH
        .hyphenate(core)
        .breaks
        .into_iter()
        .map(|b| {
            let at = core_start + b;
            let mut tail = Line::default();
            let first = format!("{}-", &text[start..at]);
            shape(fonts, font, &first, start, size, color, &mut tail);
            for g in &mut tail.glyphs {
                if g.text.start >= at {
                    g.text = at..at;
                }
            }
            let mut head = Line::default();
            shape(fonts, font, &text[at..end], at, size, color, &mut head);
            Break {
                end: word.start,
                next: word.end,
                tail: tail.glyphs,
                head: head.glyphs,
                hyphen: true,
            }
        })
        .collect()
}

/// The line from break `from` to break `to`: the head of `from`, the
/// glyphs between them, and the tail of `to`, set one after another,
/// with whether each glyph is a space inside a code span.
fn joined(glyphs: &[Glyph], code: &[bool], from: &Break, to: &Break) -> (Vec<Glyph>, Vec<bool>) {
    let own = &glyphs[from.next..to.end];
    let head = pen(&from.head);
    let left = own.first().map_or(0.0, |g| g.x);

    // a break with a tail is inside a word, so `to.end` is a glyph
    let tail_at = match own.first() {
        Some(g) if !to.tail.is_empty() => head + glyphs[to.end].x - g.x,
        _ => head,
    };
    let mut set = from.head.clone();
    set.extend(own.iter().map(|g| Glyph {
        x: g.x - left + head,
        ..g.clone()
    }));
    set.extend(to.tail.iter().map(|g| Glyph {
        x: g.x + tail_at,
        ..g.clone()
    }));
    let mut set_code = vec![false; from.head.len()];
    set_code.extend_from_slice(&code[from.next..to.end]);
    set_code.resize(set.len(), false);
    (set, set_code)
}

/// The distance `glyphs` move the pen on.
fn pen(glyphs: &[Glyph]) -> f64 {
    glyphs.iter().map(|g| g.advance).sum()
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

/// The penalty for a break inside a word, as plain TeX's
/// `\hyphenpenalty`.
const HYPHEN_PENALTY: f64 = 50.0;

/// The demerits for two lines in a row that end inside a word, as plain
/// TeX's `\doublehyphendemerits`.
const DOUBLE_HYPHEN_DEMERITS: f64 = 10000.0;

/// The indices of the `breaks` that end the lines of `glyphs`, the
/// last line's included, chosen so the lines come out about as long as
/// one another within `width`, by the method of Knuth and Plass,
/// "Breaking Paragraphs into Lines" (1981).
///
/// Spaces outside code spans may stretch by [`STRETCH`] and shrink by
/// [`SHRINK`] of their width. The last line costs nothing for being
/// short, but one shorter than [`SHORT_LAST`] of `width` pays up to
/// [`SHORT_LAST_PENALTY`]. A break inside a word pays
/// [`HYPHEN_PENALTY`], and [`DOUBLE_HYPHEN_DEMERITS`] more when the line
/// before also ends inside a word, and never comes before the last
/// line. A line wider than `width` once shrunk is allowed only when no
/// break inside it exists.
fn even_breaks(glyphs: &[Glyph], code: &[bool], breaks: &[Break], width: f64) -> Vec<usize> {
    let count = breaks.len();

    // what: the fewest demerits that reach each break, and the break
    //       the line before it started at
    // why: every line runs forward, so `best[k]` is final by the time
    //      the lines that start at break `k` are tried
    let mut best = vec![f64::INFINITY; count];
    let mut before = vec![0; count];
    best[0] = 0.0;
    for from in 0..count - 1 {
        let reached = best[from];
        if reached.is_infinite() {
            continue;
        }
        let a = &breaks[from];
        let mut first = true;
        for (k, b) in breaks.iter().enumerate().skip(from + 1) {
            // two breaks inside one word, or a last line that starts
            // inside one
            if a.next > b.end || (a.hyphen && k == count - 1) {
                continue;
            }
            let own = &glyphs[a.next..b.end];
            let own_width = match own.first() {
                Some(g) if !b.tail.is_empty() => glyphs[b.end].x - g.x,
                _ => natural_width(own),
            };
            let natural = pen(&a.head) + own_width + pen(&b.tail);
            let last = k == count - 1;
            let gaps: f64 = gaps(
                own,
                &code[a.next..b.end],
                !a.head.is_empty(),
                !b.tail.is_empty(),
            )
            .map(|i| own[i].advance)
            .sum();
            let shortfall = width - natural;
            let overfull = if last {
                natural > width
            } else {
                -shortfall > gaps * SHRINK + 1e-9
            };
            if overfull && !first {
                break;
            }
            first = false;
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
            if !last && !b.hyphen && code[b.end] {
                demerits += CODE_BREAK * CODE_BREAK;
            }
            if b.hyphen {
                demerits += HYPHEN_PENALTY * HYPHEN_PENALTY;
                if a.hyphen {
                    demerits += DOUBLE_HYPHEN_DEMERITS;
                }
            }
            let short = width * SHORT_LAST;
            if last && from > 0 && natural < short {
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
    let mut k = count - 1;
    while k > 0 {
        out.push(k);
        k = before[k];
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
/// not inside a code span. With `open_start` or `open_end` more glyphs
/// that are not spaces come before or after `glyphs` in the line.
fn gaps<'a>(
    glyphs: &'a [Glyph],
    code: &'a [bool],
    open_start: bool,
    open_end: bool,
) -> impl Iterator<Item = usize> + 'a {
    let first = match open_start {
        true => 0,
        false => glyphs.iter().position(|g| !g.space).unwrap_or(0),
    };
    let last = match open_end {
        true => glyphs.len(),
        false => glyphs.iter().rposition(|g| !g.space).unwrap_or(0),
    };
    (first..last).filter(move |&i| glyphs[i].space && !code[i])
}

/// Widen or narrow the spaces of `line` that [`gaps`] picks, each in
/// proportion to its width, so the line fills `width`. A line that
/// would need its spaces narrowed past [`SHRINK`] is left as it is.
fn stretch(line: &mut Line, code: &[bool], width: f64) {
    let gaps: Vec<usize> = gaps(&line.glyphs, code, false, false).collect();
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
    Line {
        glyphs,
        width,
        gap: 0.0,
    }
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
            bullet: false,
        }
    }

    fn bullet() -> Run {
        Run {
            text: "\u{2022}".into(),
            font: Font::Sans,
            color: Color::BLACK,
            bullet: true,
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

    /// The font each glyph of the first line of `l` is drawn from.
    fn drawn_from(l: &Layout) -> Vec<Font> {
        l.lines[0]
            .glyphs
            .iter()
            .map(|g| match g.mark {
                Mark::Outline { font, .. } => font,
                Mark::Rule { .. } => panic!("no formula here"),
            })
            .collect()
    }

    #[test]
    fn a_character_its_font_lacks_comes_from_the_next_font_that_has_it() {
        // sans has no infinity sign, and mono has no superscript a
        let fonts = Fonts::default();
        assert_eq!(fonts.cover(Font::Sans, '\u{221e}'), Some(Font::Mono));
        assert_eq!(fonts.cover(Font::Mono, '\u{1d43}'), Some(Font::Sans));
        let l = layout(&[run("-\u{221e}x", Font::Sans)], 20.0, None);
        assert_eq!(drawn_from(&l), [Font::Sans, Font::Mono, Font::Sans]);
    }

    #[test]
    fn a_bold_character_its_font_lacks_comes_from_a_bold_face() {
        // sans has no arrow, mono has no superscript a, and only the
        // math font, which has no bold face, has a triple integral
        let fonts = Fonts::default();
        assert_eq!(
            fonts.cover(Font::SansBold, '\u{2192}'),
            Some(Font::MonoBold)
        );
        assert_eq!(
            fonts.cover(Font::MonoBold, '\u{1d43}'),
            Some(Font::SansBold)
        );
        assert_eq!(fonts.cover(Font::SansBold, '\u{222d}'), Some(Font::Math));
        let l = layout(&[run("a\u{2192}b", Font::SansBold)], 20.0, None);
        assert_eq!(
            drawn_from(&l),
            [Font::SansBold, Font::MonoBold, Font::SansBold]
        );
    }

    #[test]
    fn a_fallback_glyph_takes_its_own_fonts_advance() {
        let l = layout(&[run("a\u{221e}b", Font::Sans)], 20.0, None);
        let glyphs = &l.lines[0].glyphs;
        let infinity = unkerned("\u{221e}", Font::Mono, 20.0);
        assert!((glyphs[1].advance - infinity).abs() < 1e-9);
        assert!((glyphs[2].x - glyphs[1].x - infinity).abs() < 1e-9);
        assert_eq!(&l.text[glyphs[1].text.clone()], "\u{221e}");
        let a_and_b = unkerned("ab", Font::Sans, 20.0);
        assert!((l.width() - (a_and_b + infinity)).abs() < 1e-9);
    }

    #[test]
    fn a_character_no_font_has_stays_in_the_font_before_it() {
        let fonts = Fonts::default();
        assert_eq!(fonts.cover(Font::Sans, '\u{10fffd}'), None);
        let l = layout(&[run("\u{221e}\u{10fffd}a", Font::Sans)], 20.0, None);
        assert_eq!(drawn_from(&l), [Font::Mono, Font::Mono, Font::Sans]);
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
    fn a_bold_code_span_breaks_like_a_code_span() {
        let char = unkerned("a", Font::MonoBold, 10.0);
        let runs = [run("xx ", Font::Sans), run("a b c", Font::MonoBold)];
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
            bullet: false,
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

    #[test]
    fn a_list_item_hangs_its_wrapped_lines_under_its_first_word() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [bullet(), run("aa bb cc dd", Font::Mono)];

        // the text wraps within 8 characters less the indent of 10
        let l = layout(&runs, 10.0, Some(char * 8.0));
        assert!(l.block);
        assert_eq!(l.lines.len(), 2);
        let first = &l.lines[0];
        assert_eq!(&l.text[first.glyphs[0].text.clone()], "\u{2022}");
        assert_eq!(first.glyphs[0].x, 0.0);
        assert!((first.glyphs[1].x - 10.0).abs() < 1e-9);
        assert!((first.width - (10.0 + 5.0 * char)).abs() < 1e-9);
        assert!((l.lines[1].glyphs[0].x - 10.0).abs() < 1e-9);
        assert!(l.lines.iter().all(|l| l.width <= char * 8.0));
    }

    #[test]
    fn a_list_sits_apart_from_its_items_and_the_line_after_it() {
        let runs = [
            run("head\n", Font::Sans),
            bullet(),
            run("one\n", Font::Mono),
            bullet(),
            run("two\nafter", Font::Mono),
        ];
        let l = layout(&runs, 10.0, None);
        assert_eq!(l.lines.len(), 4);
        assert_eq!(l.lines[0].glyphs[0].x, 0.0);
        assert_eq!(l.lines[3].glyphs[0].x, 0.0);
        let starts = l.place(Point::ZERO, HAlign::Left, VAlign::Baseline);
        let steps: Vec<f64> = starts.windows(2).map(|p| p[1].y - p[0].y).collect();
        assert!((steps[0] - 13.0).abs() < 1e-9);
        assert!((steps[1] - 16.0).abs() < 1e-9);
        assert!((steps[2] - 16.0).abs() < 1e-9);
        let (top, bottom) = l.extent(Point::ZERO, HAlign::Left, VAlign::Top);
        assert!((bottom.y - top.y - (l.ascent + 3.0 * 13.0 + 6.0 + l.descent)).abs() < 1e-9);
    }

    #[test]
    fn a_justified_list_item_fills_the_width_but_its_last_line() {
        let char = unkerned("a", Font::Mono, 10.0);
        let runs = [bullet(), run("aa bb cc dd ee ff gg hh", Font::Mono)];
        let width = 10.0 + char * 12.0;
        let l = justified(&runs, 10.0, width);
        let (last, full) = l.lines.split_last().unwrap();
        assert!(!full.is_empty());
        for line in full {
            assert!((line.width - width).abs() < 1e-9);
            assert!((ink_end(line) - width).abs() < 1e-9);
        }
        assert!((full[0].glyphs[1].x - 10.0).abs() < 1e-9);
        assert!((last.glyphs[0].x - 10.0).abs() < 1e-9);
        assert!(last.width < width);
        let spaces = last.glyphs.iter().filter(|g| g.space).count();
        assert!((last.width - 10.0 - char * (3 * spaces + 2) as f64).abs() < 1e-9);
    }

    /// `text` set in `font` with a `-` at each place it may break inside
    /// a word that adds a hyphen, and a `|` after each hyphen it may
    /// break after.
    fn hyphenated(text: &str, font: Font) -> String {
        let l = layout(&[run(text, font)], 20.0, None);
        let breaks = breaks(&Fonts::default(), &l.text, &l.lines[0].glyphs);
        let mut out = text.to_string();
        for b in breaks.iter().rev().filter(|b| b.hyphen) {
            let added = b.tail.last().unwrap().text.is_empty();
            out.insert(b.head[0].text.start, if added { '-' } else { '|' });
        }
        out
    }

    #[test]
    fn a_word_breaks_where_the_english_patterns_allow() {
        assert_eq!(hyphenated("hyphenation", Font::Sans), "hy-phen-a-tion");
        assert_eq!(
            hyphenated("a paragraph, justified.", Font::Sans),
            "a para-graph, jus-ti-fied."
        );
        assert_eq!(hyphenated("Partition", Font::SansBold), "Par-ti-tion");
    }

    #[test]
    fn a_break_leaves_two_letters_before_it_and_three_after() {
        // the patterns break "a-bout" and "ev-ery", and "in-to" and
        // "al-so" are shorter than five letters
        for word in ["about", "every", "into", "also", "sky", "it"] {
            assert_eq!(hyphenated(word, Font::Sans), word);
        }
        assert_eq!(hyphenated("ignores", Font::Sans), "ig-nores");
        assert_eq!(hyphenated("suffix", Font::Sans), "suf-fix");
    }

    #[test]
    fn mono_text_and_words_with_digits_never_break() {
        assert_eq!(hyphenated("hyphenation", Font::Mono), "hyphenation");
        assert_eq!(hyphenated("hyphenation", Font::MonoBold), "hyphenation");
        assert_eq!(
            hyphenated("hyphenation2 don't", Font::Sans),
            "hyphenation2 don't"
        );
        let runs = [run("the ", Font::Sans), run("hyphenation", Font::Mono)];
        let l = layout(&runs, 20.0, None);
        let breaks = breaks(&Fonts::default(), &l.text, &l.lines[0].glyphs);
        assert!(breaks.iter().all(|b| !b.hyphen));
    }

    #[test]
    fn a_hyphenated_word_breaks_only_after_its_hyphens() {
        assert_eq!(hyphenated("well-known", Font::Sans), "well-|known");
        assert_eq!(
            hyphenated("partition-based hyphenation-aware", Font::Sans),
            "partition-|based hyphenation-|aware"
        );

        let text = "a well-known and long-standing state-of-the-art method";
        let mut at_hyphens = 0;
        for width in (60..=200).step_by(5) {
            let l = justified(&[run(text, Font::Sans)], 14.0, width as f64);
            let (_, full) = l.lines.split_last().unwrap();
            for line in full {
                let end = line.glyphs.last().unwrap();
                if &l.text[end.text.clone()] != "-" || !line.glyphs.iter().any(|g| g.space) {
                    continue;
                }
                at_hyphens += 1;
                assert!(line.glyphs.windows(2).all(|g| g[0].x < g[1].x));
                assert!((ink_end(line) - width as f64).abs() < 1e-9, "at {width}");
            }
        }
        assert!(at_hyphens > 0);
    }

    #[test]
    fn a_word_broken_inside_a_ligature_is_shaped_again_in_two_parts() {
        // the sans face sets "ffi" as one glyph
        let l = layout(&[run("efficient", Font::Sans)], 20.0, None);
        assert_eq!(l.lines[0].glyphs[1].text, 1..4);
        assert_eq!(
            hyphenated("efficient office", Font::Sans),
            "ef-fi-cient of-fice"
        );
        let breaks = breaks(&Fonts::default(), &l.text, &l.lines[0].glyphs);
        let ef = breaks.iter().find(|b| b.hyphen).unwrap();

        // "ef-" sets e, f and a hyphen that copies out no text, and
        // "ficient" starts with the "fi" ligature
        let texts: Vec<Range<usize>> = ef.tail.iter().map(|g| g.text.clone()).collect();
        assert_eq!(texts, [0..1, 1..2, 2..2]);
        assert_eq!(ef.head[0].text, 2..4);
        assert_eq!(ef.head.len(), 6);
        let hyphen = layout(&[run("-", Font::Sans)], 20.0, None).lines[0].glyphs[0].mark;
        assert_eq!(ef.tail[2].mark, hyphen);
    }

    /// The most any space outside the last line of `lines` is stretched,
    /// as a multiple of its width `space`.
    fn worst_stretch(lines: &[Line], space: f64) -> f64 {
        let (_, full) = lines.split_last().unwrap();
        full.iter()
            .flat_map(|l| l.glyphs.iter().filter(|g| g.space))
            .map(|g| g.advance / space)
            .fold(0.0, f64::max)
    }

    #[test]
    fn breaking_inside_words_evens_out_a_narrow_paragraph() {
        let text = "The suffix array of a text in memory lists every suffix \
            of the text in sorted order, so a search for a pattern finds every \
            place it occurs with two binary searches.";
        let fonts = Fonts::default();
        let l = layout(&[run(text, Font::Sans)], 14.0, None);
        let glyphs = &l.lines[0].glyphs;
        let space = glyphs.iter().find(|g| g.space).unwrap().advance;
        let code = vec![false; glyphs.len()];
        let at_spaces: Vec<Break> = breaks(&fonts, &l.text, glyphs)
            .into_iter()
            .filter(|b| !b.hyphen)
            .collect();
        let before = worst_stretch(&even_lines(glyphs, &code, &at_spaces, 225.0), space);
        let after = worst_stretch(
            &justified(&[run(text, Font::Sans)], 14.0, 225.0).lines,
            space,
        );

        // at spaces alone the first line stretches its spaces to 3.3
        // times their width, and "mem-" and "pat-" bring every line
        // under 1.5
        assert!(before > 3.0, "{before}");
        assert!(after < 1.5, "{after}");
    }

    /// Whether `line` ends in a hyphen the layout added.
    fn ends_broken(line: &Line) -> bool {
        line.glyphs
            .last()
            .is_some_and(|g| g.text.is_empty() && matches!(g.mark, Mark::Outline { .. }))
    }

    #[test]
    fn the_line_before_the_last_never_ends_inside_a_word() {
        let text = "Justified text hyphenates words to even out its spacing, \
            and every place a word may break adds one more choice of where a line ends.";
        let mut broken = 0;
        for width in (100..=400).step_by(4) {
            let l = justified(&[run(text, Font::Sans)], 14.0, width as f64);
            let (last, full) = l.lines.split_last().unwrap();
            assert!(!ends_broken(last));
            assert!(full.last().is_none_or(|l| !ends_broken(l)), "at {width}");
            broken += full.iter().filter(|l| ends_broken(l)).count();
        }
        assert!(broken > 0);
    }
}

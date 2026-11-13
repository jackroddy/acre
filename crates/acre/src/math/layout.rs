use ttf_parser::GlyphId;
use ttf_parser::math::MathValue;

use super::font::{MathFont, Variant};
use super::parse::{self, Atom, Class, Noad, Nucleus, Style};
use crate::scene::Font;
use crate::text::{self, Mark};

/// A formula set into glyphs and rules, and the box around it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MathBox {
    pub width: f64,
    /// The height above the baseline.
    pub height: f64,
    /// The depth below the baseline, positive.
    pub depth: f64,
    /// What it draws, from the left end of its baseline, down positive.
    pub pieces: Vec<Piece>,
    /// The italic correction of its last glyph, which a superscript
    /// after it moves right by.
    italic: f64,
}

/// One glyph or rule of a formula.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Piece {
    pub x: f64,
    pub y: f64,
    pub mark: Mark,
}

impl MathBox {
    fn kern(width: f64) -> MathBox {
        MathBox {
            width,
            ..MathBox::default()
        }
    }

    /// Draw `other` with its origin at `x`, `y` in this box. The box
    /// keeps its size.
    fn put(&mut self, other: MathBox, x: f64, y: f64) {
        self.pieces.extend(other.pieces.into_iter().map(|p| Piece {
            x: p.x + x,
            y: p.y + y,
            ..p
        }));
    }

    /// This box moved down by `shift`, which a negative shift moves up.
    fn lowered(mut self, shift: f64) -> MathBox {
        for p in &mut self.pieces {
            p.y += shift;
        }
        self.height -= shift;
        self.depth += shift;
        self
    }
}

/// `src`, LaTeX math, set in text style at `size` pixels to the em of
/// `font`.
pub fn layout(src: &str, size: f64, font: &MathFont) -> Result<MathBox, String> {
    let list = parse::parse(src)?;
    let mode = Mode {
        style: Style::Text,
        cramped: false,
    };
    Ok(Setter { font, size }.list(&list, mode))
}

/// A style, and whether it is cramped: a cramped style raises
/// superscripts less, as under a fraction bar or a radical.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Mode {
    style: Style,
    cramped: bool,
}

impl Mode {
    fn display(self) -> bool {
        self.style == Style::Display
    }

    fn sup(self) -> Mode {
        let style = match self.style {
            Style::Display | Style::Text => Style::Script,
            Style::Script | Style::ScriptScript => Style::ScriptScript,
        };
        Mode { style, ..self }
    }

    fn sub(self) -> Mode {
        Mode {
            cramped: true,
            ..self.sup()
        }
    }

    fn num(self) -> Mode {
        let style = match self.style {
            Style::Display => Style::Text,
            Style::Text => Style::Script,
            Style::Script | Style::ScriptScript => Style::ScriptScript,
        };
        Mode { style, ..self }
    }

    fn den(self) -> Mode {
        Mode {
            cramped: true,
            ..self.num()
        }
    }

    fn cramp(self) -> Mode {
        Mode {
            cramped: true,
            ..self
        }
    }
}

struct Setter<'f> {
    font: &'f MathFont,
    /// Pixels to the em in text style.
    size: f64,
}

/// TeX's `\nulldelimiterspace`, 1.2pt at 10pt, in ems of text style: the
/// space either side of a fraction, and the width of a `.` delimiter.
const NULL_DELIMITER: f64 = 0.12;

/// The space between atoms of each pair of classes, by the class on the
/// left and then the class on the right, from *The TeXbook*, chapter 18,
/// with the classes in the order of [`Class`]. 1, 2 and 3 are thin,
/// medium and thick spaces, and a negative number is that space in
/// display and text style and none in script styles. A pair TeX never
/// meets, such as two Bin atoms in a row, is 0.
const SPACING: [[i8; 8]; 8] = [
    [0, 1, -2, -3, 0, 0, 0, -1],
    [1, 1, 0, -3, 0, 0, 0, -1],
    [-2, -2, 0, 0, -2, 0, 0, -2],
    [-3, -3, 0, 0, -3, 0, 0, -3],
    [0, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, -2, -3, 0, 0, 0, -1],
    [-1, -1, 0, -1, -1, -1, -1, -1],
    [-1, 1, -2, -3, -1, 0, -1, -1],
];

/// The space between an atom of class `left` and one of class `right`,
/// in mu.
fn spacing(left: Class, right: Class, mode: Mode) -> f64 {
    let entry = SPACING[left as usize][right as usize];
    let script = matches!(mode.style, Style::Script | Style::ScriptScript);
    match (entry.abs(), entry < 0 && script) {
        (_, true) | (0, _) => 0.0,
        (1, _) => 3.0,
        (2, _) => 4.0,
        _ => 5.0,
    }
}

impl Setter<'_> {
    /// Pixels to the em in `mode`.
    fn em(&self, mode: Mode) -> f64 {
        let k = &self.font.constants;
        let percent = match mode.style {
            Style::Display | Style::Text => 100,
            Style::Script => k.script_percent_scale_down(),
            Style::ScriptScript => k.script_script_percent_scale_down(),
        };
        self.size * percent as f64 / 100.0
    }

    /// `units` of the font, in pixels in `mode`.
    fn px(&self, mode: Mode, units: f64) -> f64 {
        units * self.em(mode) / self.font.units_per_em()
    }

    fn value(&self, mode: Mode, v: MathValue) -> f64 {
        self.px(mode, v.value as f64)
    }

    fn axis(&self, mode: Mode) -> f64 {
        self.value(mode, self.font.constants.axis_height())
    }

    /// `list` set in `mode`, its atoms spaced by their classes, as TeX's
    /// rules 5, 6 and 20 do.
    fn list(&self, list: &[Noad], mut mode: Mode) -> MathBox {
        let mut items: Vec<(Option<Class>, MathBox, Mode)> = Vec::new();
        for noad in list {
            match noad {
                Noad::Style(style) => mode.style = *style,
                Noad::Space(mu) => {
                    items.push((None, MathBox::kern(mu * self.em(mode) / 18.0), mode))
                }
                Noad::Atom(atom) => items.push((Some(atom.class), self.atom(atom, mode), mode)),
            }
        }

        // a Bin with no operand on its left, or none on its right, is
        // an Ord, as the minus in `-x` or `(-)`
        let mut prev: Option<usize> = None;
        for i in 0..items.len() {
            let Some(class) = items[i].0 else {
                continue;
            };
            let before = prev.and_then(|p| items[p].0);
            if class == Class::Bin
                && before.is_none_or(|b| {
                    matches!(
                        b,
                        Class::Bin | Class::Op | Class::Rel | Class::Open | Class::Punct
                    )
                })
            {
                items[i].0 = Some(Class::Ord);
            }
            if matches!(class, Class::Rel | Class::Close | Class::Punct)
                && before == Some(Class::Bin)
            {
                items[prev.unwrap()].0 = Some(Class::Ord);
            }
            prev = Some(i);
        }
        if let Some(p) = prev
            && items[p].0 == Some(Class::Bin)
        {
            items[p].0 = Some(Class::Ord);
        }

        let mut out = MathBox::default();
        let mut before: Option<Class> = None;
        for (class, b, mode) in items {
            if let Some(class) = class {
                if let Some(left) = before {
                    out.width += spacing(left, class, mode) * self.em(mode) / 18.0;
                }
                before = Some(class);
            }
            out.height = out.height.max(b.height);
            out.depth = out.depth.max(b.depth);
            out.italic = b.italic;
            let x = out.width;
            out.width += b.width;
            out.put(b, x, 0.0);
        }
        out
    }

    fn atom(&self, atom: &Atom, mode: Mode) -> MathBox {
        let base = self.nucleus(&atom.nucleus, mode);
        if atom.sup.is_none() && atom.sub.is_none() {
            return base;
        }
        let sup = atom.sup.as_ref().map(|l| self.list(l, mode.sup()));
        let sub = atom.sub.as_ref().map(|l| self.list(l, mode.sub()));
        if atom.class == Class::Op && atom.limits && mode.display() {
            return self.limits(base, sup, sub, mode);
        }
        let large = matches!(atom.nucleus, Nucleus::LargeOp(_));
        self.scripts(base, sup, sub, large, mode)
    }

    fn nucleus(&self, nucleus: &Nucleus, mode: Mode) -> MathBox {
        match nucleus {
            Nucleus::Empty => MathBox::default(),
            Nucleus::Char(c) => self.glyph(Font::Math, self.font.glyph(*c), self.em(mode)),
            Nucleus::LargeOp(c) => self.large_op(*c, mode),
            Nucleus::Text(text, font) => self.text(text, *font, mode),
            Nucleus::Group(list) => self.list(list, mode),
            Nucleus::Frac(num, den) => self.fraction(num, den, mode),
            Nucleus::Sqrt { index, radicand } => self.radical(index.as_deref(), radicand, mode),
            Nucleus::Fenced { left, body, right } => self.fenced(*left, body, *right, mode),
        }
    }

    /// Glyph `id` of `font` at `em` pixels to the em, in a box from its
    /// advance and its ink above and below the baseline.
    fn glyph(&self, font: Font, id: GlyphId, em: f64) -> MathBox {
        let face = match font {
            Font::Math => &self.font.face,
            font => text::face(font),
        };
        let scale = em / face.units_per_em() as f64;
        let (top, bottom) = face
            .glyph_bounding_box(id)
            .map_or((0.0, 0.0), |b| (b.y_max as f64, b.y_min as f64));
        let italic = match font {
            Font::Math => self.font.italic_correction(id) * scale,
            _ => 0.0,
        };
        MathBox {
            width: face.glyph_hor_advance(id).unwrap_or(0) as f64 * scale,
            height: (top * scale).max(0.0),
            depth: (-bottom * scale).max(0.0),
            pieces: vec![Piece {
                x: 0.0,
                y: 0.0,
                mark: Mark::Outline {
                    font,
                    id: id.0,
                    size: em,
                },
            }],
            italic,
        }
    }

    /// `text` set upright in `font`, one glyph to a character.
    fn text(&self, text: &str, font: Font, mode: Mode) -> MathBox {
        let em = self.em(mode);
        let face = match font {
            Font::Math => &self.font.face,
            font => text::face(font),
        };
        let mut out = MathBox::default();
        for c in text.chars() {
            let g = self.glyph(font, face.glyph_index(c).unwrap_or_default(), em);
            out.height = out.height.max(g.height);
            out.depth = out.depth.max(g.depth);
            let x = out.width;
            out.width += g.width;
            out.put(g, x, 0.0);
        }
        out
    }

    /// The glyph for `c` at least `height` pixels from top to bottom, a
    /// larger variant or an assembly of parts, with its baseline at the
    /// bottom of an assembly.
    fn vertical(&self, c: char, height: f64, em: f64) -> MathBox {
        let units = height * self.font.units_per_em() / em;
        let parts = match self.font.vertical(self.font.glyph(c), units) {
            Variant::Glyph(g) => return self.glyph(Font::Math, g, em),
            Variant::Assembly(parts) => parts,
        };
        let scale = em / self.font.units_per_em();
        let mut out = MathBox::default();
        for (g, at) in parts {
            let (top, bottom) = self
                .font
                .face
                .glyph_bounding_box(g)
                .map_or((0.0, 0.0), |b| (b.y_max as f64, b.y_min as f64));

            // a part's ink starts `at` up from the bottom of the
            // assembly
            let rise = (at - bottom) * scale;
            out.height = out.height.max(rise + top * scale);
            let part = self.glyph(Font::Math, g, em);
            out.width = out.width.max(part.width);
            out.put(part, 0.0, -rise);
        }
        out
    }

    /// `b` moved up or down so its middle is on the math axis, as TeX's
    /// rule 13 sets a large operator and rule 19 a delimiter.
    fn on_axis(&self, b: MathBox, mode: Mode) -> MathBox {
        let shift = (b.height - b.depth) / 2.0 - self.axis(mode);
        b.lowered(shift)
    }

    /// A large operator, grown in display style to the font's minimum
    /// height for display operators.
    fn large_op(&self, c: char, mode: Mode) -> MathBox {
        let em = self.em(mode);
        let b = if mode.display() {
            let min = self.font.constants.display_operator_min_height() as f64;
            self.vertical(c, self.px(mode, min), em)
        } else {
            self.glyph(Font::Math, self.font.glyph(c), em)
        };
        self.on_axis(b, mode)
    }

    /// Scripts after `base`, as TeX's rule 18 places them, with the
    /// constants of the MATH table.
    fn scripts(
        &self,
        base: MathBox,
        sup: Option<MathBox>,
        sub: Option<MathBox>,
        large: bool,
        mode: Mode,
    ) -> MathBox {
        let k = &self.font.constants;
        let v = |value| self.value(mode, value);
        let mut sup_shift = 0.0;
        if let Some(sup) = &sup {
            let up = if mode.cramped {
                v(k.superscript_shift_up_cramped())
            } else {
                v(k.superscript_shift_up())
            };
            sup_shift = up
                .max(base.height - v(k.superscript_baseline_drop_max()))
                .max(v(k.superscript_bottom_min()) + sup.depth);
        }
        let mut sub_shift = 0.0;
        if let Some(sub) = &sub {
            sub_shift = v(k.subscript_shift_down())
                .max(base.depth + v(k.subscript_baseline_drop_min()))
                .max(sub.height - v(k.subscript_top_max()));
        }
        if let (Some(sup), Some(sub)) = (&sup, &sub) {
            let gap = (sup_shift - sup.depth) - (sub.height - sub_shift);
            let min = v(k.sub_superscript_gap_min());
            if gap < min {
                sub_shift += min - gap;
                let lift = v(k.superscript_bottom_max_with_subscript()) - (sup_shift - sup.depth);
                if lift > 0.0 {
                    sup_shift += lift;
                    sub_shift -= lift;
                }
            }
        }

        // a large operator's italic correction moves its subscript
        // left, under the slant, and anything else's moves its
        // superscript right
        let (sup_x, sub_x) = if large {
            (base.width, base.width - base.italic)
        } else {
            (base.width + base.italic, base.width)
        };
        let mut out = MathBox {
            width: base.width,
            height: base.height,
            depth: base.depth,
            ..MathBox::default()
        };
        out.put(base, 0.0, 0.0);
        if let Some(sup) = sup {
            out.height = out.height.max(sup_shift + sup.height);
            out.width = out.width.max(sup_x + sup.width);
            out.put(sup, sup_x, -sup_shift);
        }
        if let Some(sub) = sub {
            out.depth = out.depth.max(sub_shift + sub.depth);
            out.width = out.width.max(sub_x + sub.width);
            out.put(sub, sub_x, sub_shift);
        }
        out.width += v(k.space_after_script());
        out
    }

    /// Scripts above and below `base`, as TeX's rule 13a sets the limits
    /// of an operator in display style.
    fn limits(
        &self,
        base: MathBox,
        sup: Option<MathBox>,
        sub: Option<MathBox>,
        mode: Mode,
    ) -> MathBox {
        let k = &self.font.constants;
        let v = |value| self.value(mode, value);
        let width = [&sup, &sub]
            .into_iter()
            .flatten()
            .map(|b| b.width)
            .fold(base.width, f64::max);
        let italic = base.italic;
        let mut out = MathBox {
            width,
            height: base.height,
            depth: base.depth,
            ..MathBox::default()
        };
        if let Some(sup) = sup {
            let rise =
                v(k.upper_limit_baseline_rise_min()).max(v(k.upper_limit_gap_min()) + sup.depth);
            let y = -(base.height + rise);
            out.height = base.height + rise + sup.height;
            let x = (width - sup.width + italic) / 2.0;
            out.put(sup, x, y);
        }
        if let Some(sub) = sub {
            let drop =
                v(k.lower_limit_baseline_drop_min()).max(v(k.lower_limit_gap_min()) + sub.height);
            let y = base.depth + drop;
            out.depth = base.depth + drop + sub.depth;
            let x = (width - sub.width - italic) / 2.0;
            out.put(sub, x, y);
        }
        let x = (width - base.width) / 2.0;
        out.put(base, x, 0.0);
        out
    }

    /// A fraction with its bar on the math axis, as TeX's rule 15 sets
    /// it.
    fn fraction(&self, num: &[Noad], den: &[Noad], mode: Mode) -> MathBox {
        let k = &self.font.constants;
        let v = |value| self.value(mode, value);
        let num = self.list(num, mode.num());
        let den = self.list(den, mode.den());
        let axis = self.axis(mode);
        let bar = v(k.fraction_rule_thickness());
        let (shift_up, gap_up, shift_down, gap_down) = if mode.display() {
            (
                v(k.fraction_numerator_display_style_shift_up()),
                v(k.fraction_num_display_style_gap_min()),
                v(k.fraction_denominator_display_style_shift_down()),
                v(k.fraction_denom_display_style_gap_min()),
            )
        } else {
            (
                v(k.fraction_numerator_shift_up()),
                v(k.fraction_numerator_gap_min()),
                v(k.fraction_denominator_shift_down()),
                v(k.fraction_denominator_gap_min()),
            )
        };
        let up = shift_up.max(num.depth + axis + bar / 2.0 + gap_up);
        let down = shift_down.max(den.height - axis + bar / 2.0 + gap_down);
        let pad = NULL_DELIMITER * self.size;
        let width = num.width.max(den.width);
        let mut out = MathBox {
            width: width + 2.0 * pad,
            height: up + num.height,
            depth: down + den.depth,
            ..MathBox::default()
        };
        out.pieces.push(Piece {
            x: pad,
            y: -(axis - bar / 2.0),
            mark: Mark::Rule { width, height: bar },
        });
        let (num_x, den_x) = ((width - num.width) / 2.0, (width - den.width) / 2.0);
        out.put(num, pad + num_x, -up);
        out.put(den, pad + den_x, down);
        out
    }

    /// A radical sign grown to cover `radicand`, with a bar over it and
    /// `index` in its crook, as TeX's rule 11 sets it.
    fn radical(&self, index: Option<&[Noad]>, radicand: &[Noad], mode: Mode) -> MathBox {
        let k = &self.font.constants;
        let v = |value| self.value(mode, value);
        let inner = self.list(radicand, mode.cramp());
        let mut gap = if mode.display() {
            v(k.radical_display_style_vertical_gap())
        } else {
            v(k.radical_vertical_gap())
        };
        let bar = v(k.radical_rule_thickness());
        let target = inner.height + inner.depth + gap + bar;
        let sign = self.vertical('√', target, self.em(mode));
        let total = sign.height + sign.depth;
        if total > target {
            gap += (total - target) / 2.0;
        }

        // the sign's top meets the top of the bar
        let top = inner.height + gap + bar;
        let shift = sign.height - top;
        let sign = sign.lowered(shift);
        let mut out = MathBox {
            height: top + v(k.radical_extra_ascender()),
            depth: inner.depth.max(sign.depth),
            ..MathBox::default()
        };
        let mut x = 0.0;
        if let Some(index) = index {
            let index = self.list(
                index,
                Mode {
                    style: Style::ScriptScript,
                    cramped: false,
                },
            );
            let before = v(k.radical_kern_before_degree());
            let after = v(k.radical_kern_after_degree());
            let raise = k.radical_degree_bottom_raise_percent() as f64 / 100.0 * total;
            let y = sign.depth - raise;
            out.height = out.height.max(index.height - y);
            x = (before + index.width + after).max(0.0);
            out.put(index, before, y);
        }
        let sign_width = sign.width;
        out.put(sign, x, 0.0);
        x += sign_width;
        out.pieces.push(Piece {
            x,
            y: -(inner.height + gap),
            mark: Mark::Rule {
                width: inner.width,
                height: bar,
            },
        });
        out.width = x + inner.width;
        out.put(inner, x, 0.0);
        out
    }

    /// `body` between delimiters grown to its height about the math
    /// axis, as TeX's rule 19 sets `\left` and `\right`.
    fn fenced(
        &self,
        left: Option<char>,
        body: &[Noad],
        right: Option<char>,
        mode: Mode,
    ) -> MathBox {
        let inner = self.list(body, mode);
        let em = self.em(mode);
        let axis = self.axis(mode);
        let half = (inner.height - axis).max(inner.depth + axis);

        // TeX's \delimiterfactor of 901 and \delimitershortfall of 5pt
        // at 10pt: a delimiter covers at least 90.1% of the body, and
        // falls short of it by at most half an em
        let target = (2.0 * half * 0.901).max(2.0 * half - 0.5 * em);
        let delimiter = |c: Option<char>| match c {
            Some(c) => self.on_axis(self.vertical(c, target, em), mode),
            None => MathBox::kern(NULL_DELIMITER * self.size),
        };
        let mut out = MathBox::default();
        for b in [delimiter(left), inner, delimiter(right)] {
            out.height = out.height.max(b.height);
            out.depth = out.depth.max(b.depth);
            let x = out.width;
            out.width += b.width;
            out.put(b, x, 0.0);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: f64 = 20.0;

    fn set(src: &str) -> MathBox {
        layout(src, SIZE, MathFont::noto()).unwrap()
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    fn mu(n: f64) -> f64 {
        n * SIZE / 18.0
    }

    fn rules(b: &MathBox) -> Vec<(Piece, f64, f64)> {
        b.pieces
            .iter()
            .filter_map(|p| match p.mark {
                Mark::Rule { width, height } => Some((*p, width, height)),
                Mark::Outline { .. } => None,
            })
            .collect()
    }

    fn axis() -> f64 {
        let font = MathFont::noto();
        font.constants.axis_height().value as f64 * SIZE / font.units_per_em()
    }

    #[test]
    fn a_binary_operator_is_spaced_unless_it_has_no_left_operand() {
        let w = |s| set(s).width;
        assert!(close(w("a+b"), w("a") + w("+") + w("b") + 2.0 * mu(4.0)));
        assert!(close(w("a=b"), w("a") + w("=") + w("b") + 2.0 * mu(5.0)));
        assert!(close(w("-b"), w("-") + w("b")));
        assert!(close(w("a,b"), w("a") + w(",") + w("b") + mu(3.0)));
    }

    #[test]
    fn script_styles_drop_all_but_thin_spaces() {
        let script = Mode {
            style: Style::Script,
            cramped: false,
        };
        assert_eq!(spacing(Class::Ord, Class::Bin, script), 0.0);
        assert_eq!(spacing(Class::Ord, Class::Rel, script), 0.0);
        assert_eq!(spacing(Class::Op, Class::Ord, script), 3.0);
    }

    #[test]
    fn a_superscript_is_smaller_and_raised_and_a_subscript_lowered() {
        let b = set("x_i^2");
        let [base, sup, sub] = &b.pieces[..] else {
            panic!("expected three glyphs, got {:?}", b.pieces);
        };
        let size = |p: &Piece| match p.mark {
            Mark::Outline { size, .. } => size,
            Mark::Rule { .. } => panic!("expected a glyph"),
        };
        assert_eq!(size(base), SIZE);
        assert!(size(sup) < SIZE && size(sub) < SIZE);
        assert!(sup.y < 0.0 && sub.y > 0.0);
        assert!(
            sup.x >= sub.x,
            "the superscript starts at the italic correction"
        );
    }

    #[test]
    fn a_fraction_bar_sits_on_the_axis_between_its_parts() {
        let b = set(r"\frac{a}{b}");
        let [(bar, width, height)] = rules(&b)[..] else {
            panic!("expected one rule");
        };
        assert!(close(bar.y - height / 2.0, -axis()));
        let pad = NULL_DELIMITER * SIZE;
        assert!(close(b.width, width + 2.0 * pad));
        let glyphs: Vec<&Piece> = b
            .pieces
            .iter()
            .filter(|p| matches!(p.mark, Mark::Outline { .. }))
            .collect();
        assert!(glyphs[0].y < bar.y - height && glyphs[1].y > bar.y);
        assert!(b.height > axis() && b.depth > 0.0);
    }

    #[test]
    fn a_display_operator_is_larger_and_takes_its_limits_above_and_below() {
        let text = set(r"\sum_{i=1}^n");
        let display = set(r"\displaystyle \sum_{i=1}^n");
        assert!(display.height + display.depth > text.height + text.depth);
        let op = display
            .pieces
            .iter()
            .find(|p| p.y > -SIZE && p.y < SIZE)
            .unwrap();
        let top = display
            .pieces
            .iter()
            .min_by(|a, b| a.y.total_cmp(&b.y))
            .unwrap();
        assert!(top.y < -display.height / 2.0 && top.x > op.x);
        assert!(
            display.width < text.width,
            "limits stack instead of trailing"
        );
    }

    #[test]
    fn delimiters_grow_to_cover_what_they_fence() {
        let plain = set("(x)");
        let fenced = set(r"\left( \frac{\frac{a}{b}}{\frac{c}{d}} \right)");
        let inner = set(r"\frac{\frac{a}{b}}{\frac{c}{d}}");
        assert!(fenced.height + fenced.depth > plain.height + plain.depth);
        assert!(fenced.height + fenced.depth >= 0.9 * (inner.height + inner.depth));
        let mid = (fenced.height - fenced.depth) / 2.0;
        assert!(
            (mid - axis()).abs() < 1.0,
            "a delimiter is centered on the axis"
        );
    }

    #[test]
    fn a_radical_bar_runs_from_the_sign_over_the_radicand() {
        let radicand = set("x+y");
        let b = set(r"\sqrt{x+y}");
        let [(bar, width, height)] = rules(&b)[..] else {
            panic!("expected one rule");
        };
        assert!(close(width, radicand.width));
        assert!(close(bar.x + width, b.width));
        assert!(bar.y - height < -radicand.height);
        assert!(b.height > radicand.height + height);
        let indexed = set(r"\sqrt[3]{x+y}");
        let index = indexed.pieces.iter().find(|p| p.y < 0.0).unwrap();
        let [(bar, ..)] = rules(&indexed)[..] else {
            panic!("expected one rule");
        };
        assert!(index.x < bar.x, "the index sits in the crook of the sign");
    }
}

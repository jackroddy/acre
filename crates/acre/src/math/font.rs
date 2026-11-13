use std::sync::LazyLock;

use rustybuzz::Face;
use ttf_parser::GlyphId;
use ttf_parser::math::{Constants, GlyphAssembly, Table};

static NOTO_SANS_MATH: &[u8] = include_bytes!("../../assets/fonts/NotoSansMath-Regular.ttf");

static NOTO: LazyLock<MathFont> =
    LazyLock::new(|| MathFont::new(NOTO_SANS_MATH).expect("the embedded math font parses"));

/// A font with an OpenType MATH table: its glyphs, and the constants,
/// size variants and assemblies math is set with.
pub struct MathFont {
    pub face: Face<'static>,
    pub constants: Constants<'static>,
    table: Table<'static>,
}

/// The way to draw a character at a given size: one glyph, or parts
/// stacked bottom to top.
#[derive(Debug, Clone, PartialEq)]
pub enum Variant {
    Glyph(GlyphId),
    /// Each part's glyph and its offset up from the first's, in font
    /// units.
    Assembly(Vec<(GlyphId, f64)>),
}

impl MathFont {
    /// Noto Sans Math, which acre embeds.
    pub fn noto() -> &'static MathFont {
        &NOTO
    }

    fn new(data: &'static [u8]) -> Option<MathFont> {
        let face = Face::from_slice(data, 0)?;
        let table = face.tables().math?;
        Some(MathFont {
            constants: table.constants?,
            face,
            table,
        })
    }

    pub fn units_per_em(&self) -> f64 {
        self.face.units_per_em() as f64
    }

    pub fn glyph(&self, c: char) -> GlyphId {
        self.face.glyph_index(c).unwrap_or_default()
    }

    /// The italic correction of `glyph`, in font units.
    pub fn italic_correction(&self, glyph: GlyphId) -> f64 {
        self.table
            .glyph_info
            .and_then(|i| i.italic_corrections)
            .and_then(|c| c.get(glyph))
            .map_or(0.0, |v| v.value as f64)
    }

    /// The first vertical variant of `glyph` at least `height` font units
    /// tall from top to bottom, or an assembly of parts when none is,
    /// or the largest variant when the font has no assembly.
    pub fn vertical(&self, glyph: GlyphId, height: f64) -> Variant {
        let Some(variants) = self.table.variants else {
            return Variant::Glyph(glyph);
        };
        let Some(construction) = variants.vertical_constructions.get(glyph) else {
            return Variant::Glyph(glyph);
        };
        let mut largest = glyph;
        for v in construction.variants {
            largest = v.variant_glyph;
            if v.advance_measurement as f64 >= height {
                return Variant::Glyph(v.variant_glyph);
            }
        }
        match construction.assembly {
            Some(assembly) => Variant::Assembly(assemble(
                assembly,
                variants.min_connector_overlap as f64,
                height,
            )),
            None => Variant::Glyph(largest),
        }
    }
}

/// The parts of `assembly`, with its extenders repeated as few times as
/// make it at least `height` tall, and the overlap between parts spread
/// evenly so it is as close to `height` as the connectors allow.
fn assemble(assembly: GlyphAssembly, min_overlap: f64, height: f64) -> Vec<(GlyphId, f64)> {
    let parts: Vec<_> = assembly.parts.into_iter().collect();
    let length = |repeats: usize| -> (Vec<_>, f64) {
        let chosen: Vec<_> = parts
            .iter()
            .flat_map(|p| {
                let n = if p.part_flags.extender() { repeats } else { 1 };
                std::iter::repeat_n(*p, n)
            })
            .collect();
        let advances: f64 = chosen.iter().map(|p| p.full_advance as f64).sum();
        let joints = chosen.len().saturating_sub(1) as f64;
        (chosen, advances - joints * min_overlap)
    };
    let mut repeats = 0;
    let (chosen, longest) = loop {
        let (chosen, len) = length(repeats);
        if len >= height || repeats >= 64 {
            break (chosen, len);
        }
        repeats += 1;
    };

    // each joint overlaps by at most the shorter of the two connectors
    // that meet there
    let max_overlap = chosen
        .windows(2)
        .map(|w| w[0].end_connector_length.min(w[1].start_connector_length) as f64)
        .fold(f64::INFINITY, f64::min);
    let joints = chosen.len().saturating_sub(1) as f64;
    let overlap = if joints > 0.0 {
        (min_overlap + (longest - height).max(0.0) / joints).min(max_overlap.max(min_overlap))
    } else {
        0.0
    };
    let mut at = 0.0;
    chosen
        .iter()
        .map(|p| {
            let placed = (p.glyph_id, at);
            at += p.full_advance as f64 - overlap;
            placed
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taller_request_takes_a_larger_variant_and_then_an_assembly() {
        let font = MathFont::noto();
        let paren = font.glyph('(');
        assert_eq!(font.vertical(paren, 0.0), Variant::Glyph(paren));
        let Variant::Glyph(larger) = font.vertical(paren, 1500.0) else {
            panic!("expected a variant");
        };
        assert_ne!(larger, paren);
        let Variant::Assembly(parts) = font.vertical(paren, 20_000.0) else {
            panic!("expected an assembly");
        };
        let (last, at) = *parts.last().unwrap();
        let top = at + font.face.glyph_bounding_box(last).unwrap().height() as f64;
        assert!(top >= 20_000.0 * 0.95, "the parts reach {top}");
        assert!(parts.windows(2).all(|w| w[0].1 < w[1].1));
    }
}

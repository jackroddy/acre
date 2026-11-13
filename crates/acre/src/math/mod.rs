//! Math written in a subset of LaTeX, typeset after the rules of *The
//! TeXbook*, appendix G, with the constants and glyph variants of an
//! OpenType MATH table.
//!
//! [`parse`] reads a formula into a list of atoms. [`layout`] sets the
//! list into a [`MathBox`] of glyphs and rules. `text` places a formula
//! inside a line of text, and `render` draws it.

mod font;
mod layout;
mod parse;

pub use font::MathFont;
pub use layout::{MathBox, Piece, layout};
pub use parse::{Atom, Class, Noad, Nucleus, Style, parse};

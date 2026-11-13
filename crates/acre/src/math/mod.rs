//! Math written in a subset of LaTeX, typeset after the rules of *The
//! TeXbook*, appendix G, with the constants and glyph variants of an
//! OpenType MATH table.
//!
//! [`parse`] reads a formula into a list of atoms.

mod font;
mod parse;

pub use font::MathFont;
pub use parse::{Atom, Class, Noad, Nucleus, Style, parse};

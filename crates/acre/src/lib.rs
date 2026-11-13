//! acre: animation creation and rendering engine.
//!
//! `lang` parses source text into a tree of nodes. `scene` builds a
//! function of time from that tree and lowers it to drawing primitives.
//! `render` paints those primitives into pixels.
//! `text` shapes and measures text in the fonts of `fonts`, for both,
//! and `math` typesets the formulas in it.

pub mod fonts;
pub mod lang;
pub mod math;
pub mod render;
pub mod scene;
pub mod text;

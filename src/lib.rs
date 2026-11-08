//! acre: animation creation and rendering engine.
//!
//! `lang` parses source text into a tree of nodes. `scene` builds a
//! function of time from that tree and lowers it to drawing primitives.
//! `render` paints those primitives into pixels.

pub mod lang;
pub mod render;
pub mod scene;

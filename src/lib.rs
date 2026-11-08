//! acre: animation creation and rendering engine.
//!
//! `scene` turns an animation into a function of time and lowers it to
//! drawing primitives, and `render` paints those primitives into pixels.
//! The language that builds a scene is not written yet.

pub mod render;
pub mod scene;

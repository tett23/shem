//! shem: a deterministic PDF rasterizer.
//!
//! v0.1 reads the subset of PDF that the current Golem writes
//! (docs/adr/0001, docs/adr/0002). The rendering uses integer and
//! fixed-point arithmetic only, so that its output is the same on every
//! machine.

pub mod cli;
pub mod color;
pub mod content;
pub mod error;
pub mod fixed;
pub mod flate;
pub mod font;
pub mod parallel;
pub mod pdf;
pub mod png;
pub mod raster;
pub mod render;

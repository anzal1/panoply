//! The Panoply image pipeline.
#![warn(missing_docs)]
// Pixel loops walk runtime-sized rows in fixed-size groups (3, 4 and 2 samples); the plain
// `chunks_exact` form reads the same and is kept in hot loops and test code.
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod color;
pub mod decode;
pub mod encode;
pub mod error;
pub mod format;
pub mod meta;
pub mod modules;
pub mod ops;
pub mod orient;
pub mod pipeline;
pub mod presets;
pub mod preview;
pub mod quant;
pub mod resample;
pub mod ssim;
pub mod types;

//! ID, forms and document photos: passport and visa photo, print sheet, signature cleaner
//! (docs/specs/id-and-image.md section 2, docs/FLOWS.md worked example 2).
//!
//! The faces come from the OS engine where there is one (Apple Vision on macOS, ADR 0004) and
//! from the small YuNet model through ONNX Runtime everywhere else, both fetched once with pinned
//! checksums. The pixel work (crop, tilt, background, thresholds) is ours, on the image crate's
//! pipeline. Output sizes reuse the `image.resize_to_kb` presets and its KB search.
#![warn(missing_docs)]
// Pixel loops walk runtime-sized rows in fixed-size groups; the plain `chunks_exact` form reads
// the same and is kept in hot loops and test code.
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod face;
pub mod imageio;
pub mod modules;
pub mod passport;
pub mod rules;
pub mod runtime;
pub mod segment;
pub mod sheet;
pub mod signature;
pub mod warp;
pub mod yunet;

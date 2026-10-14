//! The Panoply video and audio engine.
//!
//! Stream copy first. Where pictures must be encoded the OS hardware encoder is tried before
//! any software one. The heavy codec work runs in a helper process, an LGPL ffmpeg build
//! (see [`engine`] for why), and everything that decides what to do is plain Rust here: the
//! probe profile, per-stream routing, MP4 index work, size targets, loudness and HDR tables.
#![warn(missing_docs)]
// Sample loops walk interleaved audio in fixed-size groups; the plain `chunks_exact` form reads the
// same and is kept in hot loops and test code.
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod encoders;
pub mod engine;
pub mod error;
pub mod hdr;
pub mod io;
pub mod loudness;
pub mod modules;
pub mod mp4;
pub mod ops;
pub mod opts;
pub mod plan;
pub mod presets;
pub mod probe;
pub mod proc;
pub mod units;

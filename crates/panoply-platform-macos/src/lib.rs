//! Panoply macOS platform crate: HEIC, HEIF, AVIF and every other format ImageIO reads, with
//! HEIC and AVIF encode, behind a safe API.
//!
//! # Unsafe policy
//!
//! This is the only crate in the workspace that is allowed to use `unsafe` (its `Cargo.toml`
//! sets `unsafe_code = "allow"` instead of inheriting the workspace `forbid`). Nearly all of it is
//! in `sys.rs` (the ImageIO and CoreGraphics calls). A few type casts live in `cf.rs`,
//! `props.rs`, `encode.rs` and `decode.rs`. Every block carries a `// SAFETY:` comment.
//! Callers never see a raw pointer, a CF object or an Objective-C type.
//!
//! The whole crate is empty on every platform except macOS, so the workspace builds everywhere.
//!
//! # What ImageIO does, measured on macOS 26.6
//!
//! **Orientation.** For both JPEG and HEIC, `CGImageSourceCreateImageAtIndex` returns the
//! stored, unrotated pixels. The orientation (EXIF for JPEG, `irot` and `imir` mapped into the
//! same 1 to 8 scheme for HEIC) is only reported in the property dictionary. This crate applies
//! it itself with an exact integer transform on each band, so rows come out upright and no
//! resampling happens.
//!
//! **Color.** Each band is drawn into a bitmap context in sRGB, Display P3, or the image's own
//! RGB space, with default rendering intent and no interpolation. Opaque images use
//! `NoneSkipLast` and get alpha 255, images with alpha use premultiplied last. 16 bit reads use
//! 16 bits per component, little endian (native on every Mac).
//!
//! **HDR and gain maps.** The decoder asks for the SDR rendition explicitly
//! (`kCGImageSourceDecodeToSDR`). That is also what ImageIO does by default (the mean colour of
//! an Apple gain map HEIC is identical either way), so no tone mapping is ever applied.
//! `Info::has_gain_map` is set from ImageIO's auxiliary data, plus a byte scan for the vendor
//! markers ImageIO does not report (the Samsung HEIC gain map in the 200 MP corpus file).
//!
//! **Memory.** ImageIO decodes lazily, but drawing a whole `CGImage` into a context decodes the
//! whole picture first (1.6 GB for 200 MP). Cutting the wanted band out with
//! `CGImageCreateWithImageInRect` before drawing makes ImageIO decode only the tiles or strips
//! under the band. On top of that the decoder keeps one block of rows, a tile high, so small
//! sequential bands do not decode the same tile again and again. HEIF and AVIF grids and JPEG
//! are banded this way. Files ImageIO cannot decode in bands (PNG, WebP, GIF, plain AVIF and
//! others) are decoded whole on the first read, so `MAX_UNTILED_PIXELS` rejects the big ones.
//! `Decoder::is_banded` says which kind a file is. Known gaps: a hardware HEVC tile decode
//! keeps roughly 100 to 200 KB of IOKit memory per tile for the life of the process, and a
//! grid with very large tiles (the 33 MP Canon HIF with 3520 by 1600 tiles, 10 bit) needs
//! about 500 MB inside ImageIO whatever we do, because one tile is the smallest unit it decodes.

#![cfg(target_os = "macos")]

mod cf;
mod decode;
mod encode;
mod error;
mod props;
mod raw;
mod sys;

pub use decode::{
    Decoder, FrameGeometry, Info, MAX_DIMENSION, MAX_PIXELS, MAX_UNTILED_PIXELS, Reduce, Space,
    Thumb, can_decode_uti,
};
pub use encode::{
    EncodeInput, EncodeOptions, Format, MAX_ENCODE_PIXELS, Pixels, can_encode, encode,
};
pub use error::Error;
pub use props::{PropValue, Properties};

//! The Panoply PDF engine. See README.md.

pub mod compress;
pub mod copy;
pub mod dests;
pub mod doc;
pub mod error;
pub mod filters;
pub mod images_in;
pub mod jpeg;
pub mod obj;
pub mod pageops;
pub mod pdfium;
pub mod ranges;
pub mod render;
pub mod resample;
pub mod source;
pub mod ssim;
pub mod writer;

#[cfg(test)]
pub(crate) mod testutil;

pub use doc::{Doc, Opened, open};
pub use error::{PdfError, Result};

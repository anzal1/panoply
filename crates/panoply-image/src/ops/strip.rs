//! `image.strip_metadata`: remove location, camera details, or everything, without touching a
//! single pixel (docs/specs/id-and-image.md 3.17).
//!
//! The work is segment surgery in `crate::meta`: pixel data is copied byte for byte, so there is
//! no second JPEG generation. Orientation and the color profile always survive, because removing
//! them changes how the picture looks (the classic "my portrait turned sideways" bug).

use std::path::PathBuf;

use crate::error::{ImageError, Result};
use crate::meta::{self, StripLevel};

use super::common::*;
use super::options::Opts;
use super::{Io, NameSpec};

/// Strip each input.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let level = match opts.str("remove").unwrap_or("private") {
        "gps" => StripLevel::Gps,
        "everything" => StripLevel::Everything,
        _ => StripLevel::Private,
    };
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        check_cancel(io)?;
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Cleaning {}", display_name(path)),
        );
        match strip_one(io, path, level) {
            Ok(p) => outputs.push(p),
            Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
            Err(e) if inputs.len() > 1 => {
                io.note(format!("{}: skipped. {e}", display_name(path)));
                first_err.get_or_insert(e);
            }
            Err(e) => return Err(e),
        }
    }
    if outputs.is_empty()
        && let Some(e) = first_err
    {
        return Err(e);
    }
    Ok(outputs)
}

fn strip_one(io: &dyn Io, path: &std::path::Path, level: StripLevel) -> Result<PathBuf> {
    let name = display_name(path);
    let bytes = source_bytes(io, path).ok_or_else(|| {
        ImageError::TooLarge(format!(
            "{name} is too large to clean in one pass (the limit is 96 MB). Try a smaller file."
        ))
    })?;
    let before = meta::inspect(&bytes).map_err(|e| ImageError::Unsupported(e.to_string()))?;
    let out = meta::strip(&bytes, level).map_err(|e| ImageError::Failed(e.to_string()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg")
        .to_ascii_lowercase();
    let out_path = write_output(io, path, &NameSpec::Label("clean".into()), &ext, &out.bytes)?;
    io.note(format!("{name}: {}", before.summary));
    if out.removed.is_empty() {
        io.note(format!("{name}: nothing to remove; image data untouched"));
    } else {
        io.note(format!(
            "{name}: removed {}; image data untouched",
            out.removed.join(", ")
        ));
    }
    if !out.kept.is_empty() {
        io.note(format!("{name}: kept {}", out.kept.join(", ")));
    }
    for w in &out.warnings {
        io.note(format!("{name}: {w}"));
    }
    io.note(format!(
        "{name}: wrote {}",
        out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    Ok(out_path)
}

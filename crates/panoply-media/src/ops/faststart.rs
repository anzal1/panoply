//! Moving the index of an MP4 or MOV to the front without touching a sample (spec MOOV): the
//! file starts playing and uploading at once. Panoply's own writer does it in one pass over the
//! media data, so it costs disk speed and flat memory, and every stream stays bit-identical.

use std::path::{Path, PathBuf};

use crate::error::{MediaError, Result};
use crate::mp4::boxes::{self, bx};
use crate::mp4::movie::parse_movie;
use crate::mp4::source::Source;
use crate::mp4::write::{self, Output};

use super::Job;

/// True when `path` is an MP4 or MOV whose index sits after the media data.
pub fn needs_faststart(path: &Path) -> bool {
    let Ok(src) = Source::open(path) else {
        return false;
    };
    let Ok(l) = boxes::scan(&src) else {
        return false;
    };
    l.moov_first() == Some(false)
        && !l.is_fragmented()
        && l.find(b"moov").is_some_and(|m| !m.truncated)
}

/// Rewrite `input` with the index first. Returns the temporary result and its folder.
pub fn rewrite(job: &mut Job, input: &Path, ext: &str) -> Result<(PathBuf, PathBuf)> {
    let src = Source::open(input)?;
    let layout = boxes::scan(&src)?;
    let moov = layout.find(b"moov").ok_or_else(|| {
        MediaError::Corrupt(
            "This file looks cut off: its index is missing. Try Repair video.".into(),
        )
    })?;
    let body = src.read_exact_at(
        moov.payload(),
        (moov.size - u64::from(moov.header_len)) as usize,
    )?;
    let movie = parse_movie(&body)?;
    let ftyp = match layout.find(b"ftyp") {
        Some(f) => src.read_exact_at(f.start, f.size as usize)?,
        None => bx(b"ftyp", b"isom\0\0\x02\0isomiso2mp41"),
    };
    let (dir, part) = job.part(input, ext)?;
    let out = Output {
        ftyp,
        timescale: movie.timescale,
        mvhd: movie.mvhd.clone(),
        extra: movie.extra.clone(),
    };
    let tracks = movie.out_tracks(0);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&part)?;
    let io = job.io;
    let last = std::cell::Cell::new(-1.0f32);
    let (lo, hi) = (job.lo, job.hi);
    let r = write::write(
        file,
        &out,
        &tracks,
        &[&src],
        &|| io.cancel().is_cancelled(),
        &|f| {
            if (f - last.get()).abs() >= 0.02 || f >= 1.0 {
                last.set(f);
                io.progress(lo + (hi - lo) * f, "Moving the index to the front");
            }
        },
    );
    match r {
        Ok(_) => Ok((part, dir)),
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => Err(MediaError::Cancelled),
        Err(e) => Err(e.into()),
    }
}

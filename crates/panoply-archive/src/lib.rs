//! The Panoply archive engine.
//!
//! * [`create`] packs files and folders into a zip, tar.gz or tar.zst, streaming each file, so
//!   memory stays flat for a 20 GB folder.
//! * [`extract`] unpacks zip, tar, tar.gz, tar.zst and gz into a new folder. It is built for
//!   archives from strangers: entries that leave the folder (zip-slip, absolute paths, `..`),
//!   links that point outside it and archives that unpack far bigger than they are (zip bombs) are
//!   refused, and nothing already on disk is ever overwritten.
//!
//! Both calls poll a [`panoply_core::CancelToken`] between chunks and report progress as a fraction.
//! Safe Rust only.

mod create;
mod extract;
mod safe;

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

pub use create::{CreateOptions, CreateReport, create};
pub use extract::{ExtractReport, Skipped, extract};

/// An archive format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `.zip`
    Zip,
    /// `.tar`
    Tar,
    /// `.tar.gz` and `.tgz`
    TarGz,
    /// `.tar.zst`
    TarZst,
    /// A single gzip-compressed file, `.gz`
    Gz,
    /// A single zstd-compressed file, `.zst`
    Zst,
}

impl Format {
    /// The name shown to a person.
    pub fn label(self) -> &'static str {
        match self {
            Format::Zip => "ZIP",
            Format::Tar => "TAR",
            Format::TarGz => "TAR.GZ",
            Format::TarZst => "TAR.ZST",
            Format::Gz => "GZ",
            Format::Zst => "ZST",
        }
    }

    /// The file extension an archive of this format gets, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
            Format::TarZst => "tar.zst",
            Format::Gz => "gz",
            Format::Zst => "zst",
        }
    }
}

/// The ceilings that stop a zip bomb. Every one of them ends in a plain message that says what to
/// change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Stop when the unpacked bytes are more than this many times the archive's own size.
    pub max_ratio: u64,
    /// The ratio is only judged once this many bytes are unpacked, so a small, very compressible
    /// file (a text log, a blank image) is never taken for a bomb.
    pub ratio_floor: u64,
    /// Stop when more than this many bytes are unpacked, whatever the ratio.
    pub max_total: u64,
    /// Stop after this many entries (an archive of millions of empty files is a bomb too).
    pub max_entries: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_ratio: 1000,
            ratio_floor: 64 * 1024 * 1024,
            max_total: 50 * 1024 * 1024 * 1024,
            max_entries: 1_000_000,
        }
    }
}

/// Why an archive job stopped. Every message says what happened and what to do next.
#[derive(Debug, thiserror::Error)]
pub enum ArchiveError {
    /// The user cancelled.
    #[error("cancelled")]
    Cancelled,
    /// A file operation failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file involved, empty when it is not known.
        path: PathBuf,
        /// The system error.
        source: std::io::Error,
    },
    /// Not a format Panoply opens.
    #[error("{0}")]
    Unsupported(String),
    /// The archive is damaged or not what it says it is.
    #[error("{0}")]
    Corrupt(String),
    /// The archive needs a password.
    #[error("{0}")]
    Encrypted(String),
    /// The archive looks like a zip bomb.
    #[error("{0}")]
    Bomb(String),
    /// Something the caller asked for cannot be done.
    #[error("{0}")]
    Failed(String),
}

pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> ArchiveError {
    ArchiveError::Io {
        path: path.into(),
        source,
    }
}

/// The largest window a zstd frame may ask the decoder to hold: 2^27 bytes, 128 MiB. A frame that
/// wants more is refused instead of allocating what the sender chose.
pub(crate) const ZSTD_WINDOW_LOG_MAX: u32 = 27;

/// A zstd decoder with the window cap set.
pub(crate) fn zstd_decoder<R: Read>(
    r: R,
) -> std::io::Result<zstd::stream::read::Decoder<'static, std::io::BufReader<R>>> {
    let mut d = zstd::stream::read::Decoder::new(r)?;
    d.window_log_max(ZSTD_WINDOW_LOG_MAX)?;
    Ok(d)
}

/// A limit as a person reads it in the options: binary units, so 50 GiB is what the field says.
pub fn limit_text(bytes: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    if bytes >= GIB {
        let v = bytes as f64 / GIB as f64;
        if (v - v.round()).abs() < 0.05 {
            format!("{} GiB", v.round() as u64)
        } else {
            format!("{v:.1} GiB")
        }
    } else {
        format!("{:.0} MiB", bytes as f64 / MIB as f64)
    }
}

/// A short size for messages: `1.5 MB`.
pub fn size_text(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1000.0 && i < UNITS.len() - 1 {
        v /= 1000.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Look at the first bytes of a file (and, for gzip and zstd, at the first block inside) to tell
/// which format it is. `name` only settles the case of a tar with no magic number.
pub fn detect(file: &mut File, name: &str) -> Result<Format, ArchiveError> {
    let mut head = [0u8; 512];
    file.seek(SeekFrom::Start(0)).map_err(|e| io("", e))?;
    let n = read_up_to(&mut *file, &mut head).map_err(|e| io("", e))?;
    file.seek(SeekFrom::Start(0)).map_err(|e| io("", e))?;
    let head = &head[..n];
    let lower = name.to_ascii_lowercase();
    let tar_name =
        lower.ends_with(".tar.gz") || lower.ends_with(".tgz") || lower.ends_with(".tar.zst");
    let plain_tar_name = lower.ends_with(".tar");
    if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
        return Ok(Format::Zip);
    }
    if head.starts_with(&[0x1f, 0x8b]) {
        let mut inner = [0u8; 512];
        let m = read_up_to(flate2::read::MultiGzDecoder::new(&mut *file), &mut inner)
            .map_err(|e| io("", e))?;
        file.seek(SeekFrom::Start(0)).map_err(|e| io("", e))?;
        return Ok(if looks_like_tar(&inner[..m]) || tar_name {
            Format::TarGz
        } else {
            Format::Gz
        });
    }
    if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        let mut inner = [0u8; 512];
        let m = match zstd_decoder(&mut *file) {
            Ok(d) => read_up_to(d, &mut inner).unwrap_or(0),
            Err(_) => 0,
        };
        file.seek(SeekFrom::Start(0)).map_err(|e| io("", e))?;
        return Ok(if looks_like_tar(&inner[..m]) || tar_name {
            Format::TarZst
        } else {
            Format::Zst
        });
    }
    if looks_like_tar(head) || (plain_tar_name && head.len() == 512) {
        return Ok(Format::Tar);
    }
    if head.starts_with(&[0x37, 0x7a, 0xbc, 0xaf, 0x27, 0x1c]) {
        return Err(ArchiveError::Unsupported(
            "7z archives cannot be opened yet. Open it with the 7-Zip app, or save it as a ZIP, \
             then try again."
                .into(),
        ));
    }
    Err(ArchiveError::Unsupported(
        "This is not an archive Panoply can open. It opens ZIP, TAR, TAR.GZ, TAR.ZST and GZ. \
         Check that the file is not damaged or only partly downloaded."
            .into(),
    ))
}

fn looks_like_tar(block: &[u8]) -> bool {
    block.len() >= 262 && &block[257..262] == b"ustar"
}

fn read_up_to(mut r: impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

/// The file name without the archive extension: `photos.tar.gz` becomes `photos`.
pub fn archive_stem(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    for ext in [".tar.gz", ".tar.zst", ".tgz", ".tzst"] {
        if lower.ends_with(ext) {
            return name[..name.len() - ext.len()].to_string();
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

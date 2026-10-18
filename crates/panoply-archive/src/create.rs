//! Packing. Files are read once, in chunks, straight into the compressor and the output file.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use panoply_core::CancelToken;

use crate::extract::days_from_civil;
use crate::{ArchiveError, Format, io as io_err};

/// How to pack.
#[derive(Debug, Clone, Copy)]
pub struct CreateOptions {
    /// Zip, tar.gz or tar.zst.
    pub format: Format,
    /// 1 fastest to 9 smallest for zip and gzip; mapped to zstd levels 1, 3 and 19 in three steps.
    pub level: u32,
}

/// What a pack did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateReport {
    /// Files stored.
    pub files: u64,
    /// Folders stored (empty ones included).
    pub folders: u64,
    /// Links stored as links.
    pub links: u64,
    /// Bytes read from the files.
    pub bytes_in: u64,
    /// Things left out, with the reason.
    pub skipped: Vec<(String, String)>,
}

struct Item {
    path: PathBuf,
    name: String,
    kind: Kind,
    size: u64,
    mode: u32,
    mtime: Option<SystemTime>,
}

enum Kind {
    File,
    Dir,
    Link(String),
}

/// Walk every input in a stable order. `visit` gets each file, folder and link with its name inside
/// the archive. Memory is one folder listing at a time.
fn walk(
    inputs: &[PathBuf],
    skip: &Path,
    skipped: &mut Vec<(String, String)>,
    visit: &mut dyn FnMut(Item) -> Result<(), ArchiveError>,
) -> Result<(), ArchiveError> {
    let mut used: HashSet<String> = HashSet::new();
    for input in inputs {
        let base = input
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "files".to_string());
        // Two inputs with the same name get a suffix, so neither hides the other.
        let mut top = base.clone();
        let mut k = 2;
        while !used.insert(top.clone()) {
            top = format!("{base} ({k})");
            k += 1;
        }
        walk_one(input, &top, skip, skipped, visit)?;
    }
    Ok(())
}

fn walk_one(
    path: &Path,
    name: &str,
    skip: &Path,
    skipped: &mut Vec<(String, String)>,
    visit: &mut dyn FnMut(Item) -> Result<(), ArchiveError>,
) -> Result<(), ArchiveError> {
    // The skipped file is told apart by name: the caller's path may spell the folder differently.
    if skip.file_name().is_some() && path.file_name() == skip.file_name() {
        return Ok(());
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) => return Err(io_err(path, e)),
    };
    let mode = mode_of(&meta);
    let mtime = meta.modified().ok();
    let ft = meta.file_type();
    if ft.is_symlink() {
        match fs::read_link(path) {
            Ok(t) => visit(Item {
                path: path.to_path_buf(),
                name: name.to_string(),
                kind: Kind::Link(t.to_string_lossy().into_owned()),
                size: 0,
                mode,
                mtime,
            })?,
            Err(_) => skipped.push((name.to_string(), "the link could not be read".into())),
        }
    } else if ft.is_dir() {
        visit(Item {
            path: path.to_path_buf(),
            name: name.to_string(),
            kind: Kind::Dir,
            size: 0,
            mode,
            mtime,
        })?;
        let mut names: Vec<std::ffi::OsString> = match fs::read_dir(path) {
            Ok(rd) => rd.filter_map(|e| e.ok().map(|e| e.file_name())).collect(),
            Err(e) => return Err(io_err(path, e)),
        };
        names.sort();
        for n in names {
            let child = format!("{name}/{}", n.to_string_lossy());
            walk_one(&path.join(&n), &child, skip, skipped, visit)?;
        }
    } else if ft.is_file() {
        visit(Item {
            path: path.to_path_buf(),
            name: name.to_string(),
            kind: Kind::File,
            size: meta.len(),
            mode,
            mtime,
        })?;
    } else {
        skipped.push((name.to_string(), "it is a device or pipe".into()));
    }
    Ok(())
}

#[cfg(unix)]
fn mode_of(m: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn mode_of(m: &fs::Metadata) -> u32 {
    if m.is_dir() { 0o755 } else { 0o644 }
}

/// Reads a file, counting and polling cancel between chunks. A cancel comes out as an
/// `Interrupted` read, and the caller turns that into [`ArchiveError::Cancelled`].
struct Feed<'a, R> {
    inner: R,
    cancel: &'a CancelToken,
    done: &'a mut u64,
    total: u64,
    last: &'a mut Instant,
    progress: &'a mut dyn FnMut(f32, &str),
    label: &'a str,
}

impl<R: Read> Read for Feed<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = self.inner.read(buf)?;
        *self.done += n as u64;
        if self.last.elapsed() >= Duration::from_millis(50) {
            *self.last = Instant::now();
            (self.progress)(
                (*self.done as f64 / self.total.max(1) as f64).min(0.999) as f32,
                self.label,
            );
        }
        Ok(n)
    }
}

/// Extensions of files that are already compressed. Zip stores them as they are, which is faster
/// and no bigger.
fn already_compressed(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(
        ext,
        "zip"
            | "gz"
            | "tgz"
            | "zst"
            | "bz2"
            | "xz"
            | "7z"
            | "rar"
            | "jpg"
            | "jpeg"
            | "png"
            | "webp"
            | "heic"
            | "avif"
            | "gif"
            | "mp3"
            | "aac"
            | "m4a"
            | "ogg"
            | "opus"
            | "flac"
            | "mp4"
            | "m4v"
            | "mov"
            | "mkv"
            | "webm"
            | "pdf"
            | "docx"
            | "xlsx"
            | "pptx"
            | "epub"
            | "jar"
            | "apk"
    )
}

/// Pack `inputs` into `out`. `open` opens a file for reading (the host's capability guard sits
/// behind it). `skip` is a path never to store, so an archive made next to its sources does not
/// swallow itself.
pub fn create(
    inputs: &[PathBuf],
    out: &mut File,
    opts: &CreateOptions,
    skip: &Path,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
    open: &mut dyn FnMut(&Path) -> Result<File, ArchiveError>,
) -> Result<CreateReport, ArchiveError> {
    if !matches!(opts.format, Format::Zip | Format::TarGz | Format::TarZst) {
        return Err(ArchiveError::Failed(
            "Choose ZIP, TAR.GZ or TAR.ZST to pack into.".into(),
        ));
    }
    let skip = skip.to_path_buf();
    // Pass one counts the bytes so progress is a real fraction.
    let mut total = 0u64;
    let mut count_skipped = Vec::new();
    walk(inputs, &skip, &mut count_skipped, &mut |it| {
        if cancel.is_cancelled() {
            return Err(ArchiveError::Cancelled);
        }
        total += it.size;
        Ok(())
    })?;
    let mut report = CreateReport {
        files: 0,
        folders: 0,
        links: 0,
        bytes_in: 0,
        skipped: Vec::new(),
    };
    let mut done = 0u64;
    let mut last = Instant::now() - Duration::from_secs(1);
    let writer = BufWriter::with_capacity(256 * 1024, &mut *out);
    let result = match opts.format {
        Format::Zip => zip_pack(
            writer,
            inputs,
            &skip,
            opts,
            total,
            &mut done,
            &mut last,
            &mut report,
            cancel,
            progress,
            open,
        ),
        Format::TarGz => {
            let level = flate2::Compression::new(opts.level.clamp(1, 9));
            let enc = flate2::write::GzEncoder::new(writer, level);
            tar_pack(
                enc,
                inputs,
                &skip,
                total,
                &mut done,
                &mut last,
                &mut report,
                cancel,
                progress,
                open,
            )
            .and_then(|enc| {
                enc.finish()
                    .and_then(|mut w| w.flush())
                    .map_err(|e| io_err("", e))
            })
        }
        _ => {
            let level = match opts.level {
                0..=3 => 1,
                4..=7 => 3,
                _ => 19,
            };
            let enc =
                zstd::stream::write::Encoder::new(writer, level).map_err(|e| io_err("", e))?;
            tar_pack(
                enc,
                inputs,
                &skip,
                total,
                &mut done,
                &mut last,
                &mut report,
                cancel,
                progress,
                open,
            )
            .and_then(|enc| {
                enc.finish()
                    .and_then(|mut w| w.flush())
                    .map_err(|e| io_err("", e))
            })
        }
    };
    result?;
    progress(1.0, "Done");
    Ok(report)
}

fn cancel_or(cancel: &CancelToken, e: io::Error, path: &Path) -> ArchiveError {
    if cancel.is_cancelled() {
        ArchiveError::Cancelled
    } else {
        io_err(path, e)
    }
}

#[allow(clippy::too_many_arguments)]
fn tar_pack<W: Write>(
    sink: W,
    inputs: &[PathBuf],
    skip: &Path,
    total: u64,
    done: &mut u64,
    last: &mut Instant,
    report: &mut CreateReport,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
    open: &mut dyn FnMut(&Path) -> Result<File, ArchiveError>,
) -> Result<W, ArchiveError> {
    let mut b = tar::Builder::new(sink);
    b.follow_symlinks(false);
    let mut skipped = Vec::new();
    walk(inputs, skip, &mut skipped, &mut |it| {
        if cancel.is_cancelled() {
            return Err(ArchiveError::Cancelled);
        }
        let mut h = tar::Header::new_gnu();
        h.set_mode(it.mode);
        h.set_mtime(epoch_secs(it.mtime));
        match &it.kind {
            Kind::Dir => {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_size(0);
                let name = format!("{}/", it.name);
                b.append_data(&mut h, &name, io::empty())
                    .map_err(|e| cancel_or(cancel, e, &it.path))?;
                report.folders += 1;
            }
            Kind::Link(target) => {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                b.append_link(&mut h, &it.name, target)
                    .map_err(|e| cancel_or(cancel, e, &it.path))?;
                report.links += 1;
            }
            Kind::File => {
                let f = open(&it.path)?;
                h.set_entry_type(tar::EntryType::Regular);
                h.set_size(it.size);
                let feed = Feed {
                    inner: f,
                    cancel,
                    done,
                    total,
                    last,
                    progress,
                    label: &it.name,
                };
                b.append_data(&mut h, &it.name, feed)
                    .map_err(|e| cancel_or(cancel, e, &it.path))?;
                report.files += 1;
                report.bytes_in += it.size;
            }
        }
        Ok(())
    })?;
    report.skipped.extend(skipped);
    b.into_inner().map_err(|e| io_err("", e))
}

#[allow(clippy::too_many_arguments)]
fn zip_pack<W: Write + Seek>(
    sink: W,
    inputs: &[PathBuf],
    skip: &Path,
    opts: &CreateOptions,
    total: u64,
    done: &mut u64,
    last: &mut Instant,
    report: &mut CreateReport,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
    open: &mut dyn FnMut(&Path) -> Result<File, ArchiveError>,
) -> Result<(), ArchiveError> {
    use zip::CompressionMethod;
    use zip::write::SimpleFileOptions;
    let zerr = |e: zip::result::ZipError| match e {
        zip::result::ZipError::Io(e) => io_err("", e),
        other => ArchiveError::Failed(format!("The ZIP could not be written ({other}).")),
    };
    let mut z = zip::ZipWriter::new(sink);
    let mut skipped = Vec::new();
    walk(inputs, skip, &mut skipped, &mut |it| {
        if cancel.is_cancelled() {
            return Err(ArchiveError::Cancelled);
        }
        let mut o = SimpleFileOptions::default()
            .unix_permissions(it.mode & 0o777)
            .large_file(it.size >= u64::from(u32::MAX));
        if let Some(t) = zip_time(it.mtime) {
            o = o.last_modified_time(t);
        }
        match &it.kind {
            Kind::Dir => {
                z.add_directory(format!("{}/", it.name), o).map_err(zerr)?;
                report.folders += 1;
            }
            Kind::Link(target) => {
                z.add_symlink(&it.name, target, o).map_err(zerr)?;
                report.links += 1;
            }
            Kind::File => {
                let method = if already_compressed(&it.name) {
                    CompressionMethod::Stored
                } else {
                    CompressionMethod::Deflated
                };
                let mut o = o.compression_method(method);
                if method == CompressionMethod::Deflated {
                    o = o.compression_level(Some(i64::from(opts.level.clamp(1, 9))));
                }
                z.start_file(&it.name, o).map_err(zerr)?;
                let mut feed = Feed {
                    inner: open(&it.path)?,
                    cancel,
                    done,
                    total,
                    last,
                    progress,
                    label: &it.name,
                };
                io::copy(&mut feed, &mut z).map_err(|e| cancel_or(cancel, e, &it.path))?;
                report.files += 1;
                report.bytes_in += it.size;
            }
        }
        Ok(())
    })?;
    report.skipped.extend(skipped);
    let mut w = z.finish().map_err(zerr)?;
    w.flush().map_err(|e| io_err("", e))
}

fn epoch_secs(t: Option<SystemTime>) -> u64 {
    t.and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// A system time as the date and time a zip entry holds (UTC). Dates before 1980 or after 2107
/// cannot be stored and leave the default.
fn zip_time(t: Option<SystemTime>) -> Option<zip::DateTime> {
    let secs = i64::try_from(epoch_secs(t)).ok()?;
    if secs == 0 {
        return None;
    }
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since the epoch, the inverse of days_from_civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    debug_assert_eq!(days_from_civil(y, m, d), days);
    zip::DateTime::from_date_and_time(
        u16::try_from(y).ok()?,
        u8::try_from(m).ok()?,
        u8::try_from(d).ok()?,
        u8::try_from(rem / 3600).ok()?,
        u8::try_from(rem % 3600 / 60).ok()?,
        u8::try_from(rem % 60).ok()?,
    )
    .ok()
}

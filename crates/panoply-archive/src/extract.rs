//! Unpacking. One pass over the archive, one buffer, nothing held in memory but the buffer.

use std::cell::Cell;
use std::ffi::OsStr;
use std::fs;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

use panoply_core::CancelToken;

use crate::safe::{Cleaned, Reject, clean_name, create_unique, link_target_ok};
use crate::{ArchiveError, Format, Limits, archive_stem, detect, io, size_text};

const CHUNK: usize = 256 * 1024;

/// One entry that was left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// The name the archive gave it.
    pub name: String,
    /// What is wrong with it, as a phrase that follows the name.
    pub reason: &'static str,
}

/// What an unpack did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractReport {
    /// The format that was read.
    pub format: Format,
    /// Files written.
    pub files: u64,
    /// Folders made.
    pub folders: u64,
    /// Links made (all of them inside the folder).
    pub links: u64,
    /// Bytes written.
    pub bytes: u64,
    /// Files that got a ` (2)` suffix because the name was taken.
    pub renamed: u64,
    /// Entries refused for safety.
    pub skipped: Vec<Skipped>,
    /// Set when the archive turned out damaged part way: what was readable is on disk.
    pub damaged: Option<String>,
}

enum Fail {
    Reject(Reject),
    Fatal(ArchiveError),
}

impl From<Reject> for Fail {
    fn from(r: Reject) -> Self {
        Fail::Reject(r)
    }
}

impl From<ArchiveError> for Fail {
    fn from(e: ArchiveError) -> Self {
        Fail::Fatal(e)
    }
}

/// Counts the bytes read from the archive, so the ratio and the progress bar know how far in
/// the compressed stream we are.
struct Counting<R> {
    inner: R,
    count: Rc<Cell<u64>>,
}

impl<R: Read> Read for Counting<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.count.set(self.count.get() + n as u64);
        Ok(n)
    }
}

impl<R: Seek> Seek for Counting<R> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        self.inner.seek(pos)
    }
}

/// Unpack `archive` into `dest`, which must be an existing, empty folder the caller just made.
///
/// Unsafe entries are skipped and listed in the report. A zip bomb, a cancel or a disk error
/// stops the job with an error and leaves the cleanup of `dest` to the caller. An archive that
/// turns out damaged part way keeps what was readable and says so in `damaged`.
pub fn extract(
    archive: fs::File,
    archive_name: &str,
    dest: &Path,
    limits: &Limits,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<ExtractReport, ArchiveError> {
    let mut file = archive;
    let in_len = file.metadata().map_err(|e| io("", e))?.len();
    let format = detect(&mut file, archive_name)?;
    let root = dest.canonicalize().map_err(|e| io(dest, e))?;
    let count = Rc::new(Cell::new(0u64));
    let reader = Counting {
        inner: file,
        count: count.clone(),
    };
    let mut run = Run {
        root,
        limits,
        cancel,
        progress,
        compressed: count,
        in_len,
        report: ExtractReport {
            format,
            files: 0,
            folders: 0,
            links: 0,
            bytes: 0,
            renamed: 0,
            skipped: Vec::new(),
            damaged: None,
        },
        entries: 0,
        last_tick: Instant::now() - Duration::from_secs(1),
        verified: None,
    };
    run.tick("Reading the archive");
    let result = match format {
        Format::Zip => run.zip(reader),
        Format::Tar => run.tar(reader),
        Format::TarGz => run.tar(flate2::read::MultiGzDecoder::new(reader)),
        Format::TarZst => match zstd::stream::read::Decoder::new(reader) {
            Ok(d) => run.tar(d),
            Err(e) => Err(read_err(e)),
        },
        Format::Gz => run.single(flate2::read::MultiGzDecoder::new(reader), archive_name),
        Format::Zst => match zstd::stream::read::Decoder::new(reader) {
            Ok(d) => run.single(d, archive_name),
            Err(e) => Err(read_err(e)),
        },
    };
    match result {
        Ok(()) => {}
        Err(ArchiveError::Corrupt(m)) if run.report.files > 0 => run.report.damaged = Some(m),
        Err(e) => return Err(e),
    }
    run.force_tick("Done", 1.0);
    Ok(run.report)
}

fn read_err(e: std::io::Error) -> ArchiveError {
    match e.kind() {
        ErrorKind::InvalidData | ErrorKind::UnexpectedEof | ErrorKind::InvalidInput => {
            ArchiveError::Corrupt(format!(
                "The archive is damaged or was cut short ({}). Download it again, or ask whoever \
                 sent it for a fresh copy.",
                panoply_core::naming::plain_io(&e)
            ))
        }
        _ => io("", e),
    }
}

fn zip_err(e: zip::result::ZipError) -> ArchiveError {
    use zip::result::ZipError as Z;
    match e {
        Z::Io(e) => read_err(e),
        Z::UnsupportedArchive(m) => ArchiveError::Unsupported(format!(
            "This ZIP uses something Panoply cannot open ({m}). Open it with another tool, or \
             ask for it as a plain ZIP."
        )),
        other => ArchiveError::Corrupt(format!(
            "The ZIP is damaged or was cut short ({other}). Download it again, or ask whoever \
             sent it for a fresh copy."
        )),
    }
}

struct Run<'a> {
    root: PathBuf,
    limits: &'a Limits,
    cancel: &'a CancelToken,
    progress: &'a mut dyn FnMut(f32, &str),
    compressed: Rc<Cell<u64>>,
    in_len: u64,
    report: ExtractReport,
    entries: u64,
    last_tick: Instant,
    /// The parent folder (as `a/b`) already checked this pass, so a folder with 10,000 files is
    /// checked once.
    verified: Option<String>,
}

impl Run<'_> {
    fn fraction(&self) -> f32 {
        (self.compressed.get() as f64 / self.in_len.max(1) as f64).min(0.999) as f32
    }

    fn tick(&mut self, label: &str) {
        if self.last_tick.elapsed() >= Duration::from_millis(50) {
            self.force_tick(label, self.fraction());
        }
    }

    fn force_tick(&mut self, label: &str, fraction: f32) {
        self.last_tick = Instant::now();
        (self.progress)(fraction, label);
    }

    fn check_cancel(&self) -> Result<(), ArchiveError> {
        if self.cancel.is_cancelled() {
            Err(ArchiveError::Cancelled)
        } else {
            Ok(())
        }
    }

    fn next_entry(&mut self) -> Result<(), ArchiveError> {
        self.check_cancel()?;
        self.entries += 1;
        if self.entries > self.limits.max_entries {
            return Err(ArchiveError::Bomb(format!(
                "This archive holds more than {} entries, which is how a zip bomb hides. Panoply \
                 stopped and removed what it had unpacked. If you trust the file, unpack it with \
                 another tool.",
                self.limits.max_entries
            )));
        }
        Ok(())
    }

    fn skip(&mut self, name: &str, why: Reject) {
        self.report.skipped.push(Skipped {
            name: name.to_string(),
            reason: why.reason(),
        });
    }

    /// The ratio and total checks, run after every chunk.
    fn check_bomb(&self) -> Result<(), ArchiveError> {
        let out = self.report.bytes;
        if out > self.limits.max_total {
            return Err(ArchiveError::Bomb(format!(
                "This archive unpacks to more than {}, which is over the limit. Panoply stopped \
                 and removed what it had unpacked. If you trust the file, raise the size limit \
                 in the options.",
                size_text(self.limits.max_total)
            )));
        }
        self.check_ratio(out)
    }

    fn check_ratio(&self, out: u64) -> Result<(), ArchiveError> {
        if out > self.limits.ratio_floor {
            let inp = self.compressed.get().max(1);
            if out / inp > self.limits.max_ratio {
                return Err(ArchiveError::Bomb(format!(
                    "This archive unpacks far bigger than it is: {} from {} so far, more than {} \
                     to 1. That is how a zip bomb works, so Panoply stopped and removed what it \
                     had unpacked. If you trust the file, raise the ratio limit in the options.",
                    size_text(out),
                    size_text(inp),
                    self.limits.max_ratio
                )));
            }
        }
        Ok(())
    }

    /// Make every folder of `comps` under the root, refusing to step through a link or a file.
    fn dir_chain(&mut self, comps: &[String]) -> Result<PathBuf, Fail> {
        let key = comps.join("/");
        let mut cur = self.root.clone();
        if self.verified.as_deref() == Some(key.as_str()) {
            cur.extend(comps);
            return Ok(cur);
        }
        for c in comps {
            cur.push(c);
            match fs::symlink_metadata(&cur) {
                Ok(m) if m.is_dir() => {}
                Ok(_) => return Err(Reject::BlockedParent.into()),
                Err(e) if e.kind() == ErrorKind::NotFound => match fs::create_dir(&cur) {
                    Ok(()) => self.report.folders += 1,
                    Err(e) if e.kind() == ErrorKind::AlreadyExists => {
                        return Err(Reject::BlockedParent.into());
                    }
                    Err(e) => return Err(io(&cur, e).into()),
                },
                Err(e) => return Err(io(&cur, e).into()),
            }
        }
        self.verified = Some(key);
        Ok(cur)
    }

    fn write_file(
        &mut self,
        name: &str,
        comps: &[String],
        reader: &mut dyn Read,
        mode: Option<u32>,
        mtime: Option<SystemTime>,
    ) -> Result<(), Fail> {
        let (last, parent) = comps.split_last().expect("a file has a name");
        let dir = self.dir_chain(parent)?;
        let (path, mut f, renamed) =
            create_unique(&dir, OsStr::new(last)).map_err(|e| io(dir.join(last), e))?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            self.check_cancel()?;
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(read_err(e).into()),
            };
            self.report.bytes += n as u64;
            self.check_bomb()?;
            std::io::Write::write_all(&mut f, &buf[..n]).map_err(|e| io(&path, e))?;
            self.tick(name);
        }
        drop_buffer(buf);
        #[cfg(unix)]
        if let Some(m) = mode {
            use std::os::unix::fs::PermissionsExt;
            // Owner can always read and write; setuid, setgid and sticky never come from an archive.
            let _ = f.set_permissions(fs::Permissions::from_mode((m & 0o777) | 0o600));
        }
        #[cfg(not(unix))]
        let _ = mode;
        if let Some(t) = mtime {
            let _ = f.set_modified(t);
        }
        self.report.files += 1;
        if renamed {
            self.report.renamed += 1;
        }
        Ok(())
    }

    fn make_link(&mut self, comps: &[String], target: &str) -> Result<(), Fail> {
        let (last, parent) = comps.split_last().expect("a link has a name");
        link_target_ok(target, parent.len())?;
        #[cfg(unix)]
        {
            let dir = self.dir_chain(parent)?;
            let mut k = 1u32;
            loop {
                let name = if k == 1 {
                    last.clone()
                } else {
                    format!("{last} ({k})")
                };
                let full = dir.join(&name);
                match std::os::unix::fs::symlink(target, &full) {
                    Ok(()) => break,
                    Err(e) if e.kind() == ErrorKind::AlreadyExists && k < 10_000 => k += 1,
                    Err(e) => return Err(io(full, e).into()),
                }
            }
            self.verified = None;
            self.report.links += 1;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = (last, target);
            Err(Reject::LinkUnsupported.into())
        }
    }

    /// Route one refusal into the report and let a fatal error out.
    fn settle(&mut self, name: &str, r: Result<(), Fail>) -> Result<(), ArchiveError> {
        match r {
            Ok(()) => Ok(()),
            Err(Fail::Reject(why)) => {
                self.skip(name, why);
                Ok(())
            }
            Err(Fail::Fatal(e)) => Err(e),
        }
    }

    fn add_dir(&mut self, comps: &[String]) -> Result<(), Fail> {
        self.dir_chain(comps).map(|_| ())
    }

    fn zip<R: Read + Seek>(&mut self, reader: R) -> Result<(), ArchiveError> {
        let mut z = zip::ZipArchive::new(reader).map_err(zip_err)?;
        let n = z.len();
        if n as u64 > self.limits.max_entries {
            return Err(ArchiveError::Bomb(format!(
                "This ZIP holds {n} entries, more than the limit of {}. Panoply did not unpack \
                 it. If you trust the file, unpack it with another tool.",
                self.limits.max_entries
            )));
        }
        // The central directory is read first and costs nothing: refuse a lying or huge ZIP
        // before a single byte is written.
        let mut declared = 0u64;
        for i in 0..n {
            let f = z.by_index_raw(i).map_err(zip_err)?;
            if f.encrypted() {
                return Err(ArchiveError::Encrypted(
                    "This ZIP needs a password, and Panoply cannot open password-protected ZIPs \
                     yet. Open it with another tool, or ask for it without a password."
                        .into(),
                ));
            }
            declared = declared.saturating_add(f.size());
        }
        if declared > self.limits.max_total {
            return Err(ArchiveError::Bomb(format!(
                "This ZIP says it unpacks to {}, which is over the limit of {}. Panoply did not \
                 unpack it. If you trust the file, raise the size limit in the options.",
                size_text(declared),
                size_text(self.limits.max_total)
            )));
        }
        if declared > self.limits.ratio_floor
            && declared / self.in_len.max(1) > self.limits.max_ratio
        {
            return Err(ArchiveError::Bomb(format!(
                "This ZIP is {} and says it unpacks to {}, more than {} to 1. That is how a zip \
                 bomb works, so Panoply did not unpack it. If you trust the file, raise the ratio \
                 limit in the options.",
                size_text(self.in_len),
                size_text(declared),
                self.limits.max_ratio
            )));
        }
        for i in 0..n {
            self.next_entry()?;
            let mut f = z.by_index(i).map_err(zip_err)?;
            let name = f
                .name()
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(f.name_raw()).into_owned());
            let is_dir = f.is_dir();
            let mode = f.unix_mode();
            let is_link = mode.is_some_and(|m| m & 0o170000 == 0o120000);
            let mtime = f.last_modified().and_then(|d| {
                dos_to_time(
                    d.year(),
                    d.month(),
                    d.day(),
                    d.hour(),
                    d.minute(),
                    d.second(),
                )
            });
            let comps = match clean_name(&name, true) {
                Ok(Cleaned::Root) => continue,
                Ok(Cleaned::Path(c)) => c,
                Err(why) => {
                    self.skip(&name, why);
                    continue;
                }
            };
            let r = if is_dir {
                self.add_dir(&comps)
            } else if is_link {
                let mut target = Vec::new();
                match (&mut f).take(4097).read_to_end(&mut target) {
                    Ok(_) => {}
                    Err(e) => return Err(read_err(e)),
                }
                self.make_link(&comps, &String::from_utf8_lossy(&target))
            } else {
                self.write_file(&name, &comps, &mut f, mode, mtime)
            };
            self.settle(&name, r)?;
            self.tick(&name);
        }
        Ok(())
    }

    fn tar<R: Read>(&mut self, reader: R) -> Result<(), ArchiveError> {
        let mut ar = tar::Archive::new(reader);
        for entry in ar.entries().map_err(read_err)? {
            let mut e = entry.map_err(read_err)?;
            self.next_entry()?;
            let kind = e.header().entry_type();
            let name = String::from_utf8_lossy(&e.path_bytes()).into_owned();
            use tar::EntryType as T;
            if matches!(
                kind,
                T::XGlobalHeader | T::XHeader | T::GNULongName | T::GNULongLink
            ) {
                continue;
            }
            let comps = match clean_name(&name, false) {
                Ok(Cleaned::Root) => continue,
                Ok(Cleaned::Path(c)) => c,
                Err(why) => {
                    self.skip(&name, why);
                    continue;
                }
            };
            let mode = e.header().mode().ok();
            let mtime = e
                .header()
                .mtime()
                .ok()
                .filter(|&s| s > 0)
                .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s));
            let r = match kind {
                T::Directory => self.add_dir(&comps),
                T::Regular | T::Continuous | T::GNUSparse => {
                    self.write_file(&name, &comps, &mut e, mode, mtime)
                }
                T::Symlink => match e.link_name_bytes() {
                    Some(t) => {
                        let target = String::from_utf8_lossy(&t).into_owned();
                        self.make_link(&comps, &target)
                    }
                    None => Err(Reject::LinkOutside.into()),
                },
                _ => Err(Reject::SpecialFile.into()),
            };
            self.settle(&name, r)?;
            self.tick(&name);
        }
        Ok(())
    }

    fn single<R: Read>(&mut self, mut reader: R, archive_name: &str) -> Result<(), ArchiveError> {
        self.next_entry()?;
        let mut name = archive_stem(archive_name);
        if name.is_empty() || name == archive_name {
            name = format!("{archive_name}.unpacked");
        }
        let comps = match clean_name(&name, false) {
            Ok(Cleaned::Path(c)) => c,
            _ => vec!["unpacked".to_string()],
        };
        let r = self.write_file(&name, &comps, &mut reader, None, None);
        self.settle(&name, r)
    }
}

fn drop_buffer(_b: Vec<u8>) {}

/// A zip timestamp (local time in the file, read here as UTC) as a system time.
pub(crate) fn dos_to_time(y: u16, mo: u8, d: u8, h: u8, mi: u8, s: u8) -> Option<SystemTime> {
    let days = days_from_civil(i64::from(y), i64::from(mo), i64::from(d));
    let secs = days * 86_400 + i64::from(h) * 3600 + i64::from(mi) * 60 + i64::from(s);
    u64::try_from(secs)
        .ok()
        .map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s))
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub(crate) fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

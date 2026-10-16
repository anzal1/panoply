//! The parallel walker that fills a [`Segment`].
//!
//! Design, in one paragraph. Each folder is one task on a work-stealing pool: read the folder, append its
//! entries to the running thread's private arena (no shared state, no per-entry allocation beyond the
//! name `std` hands back), and spawn one task per sub-folder. A folder's listing gets a *batch id* from an
//! atomic counter when its parent decides to descend, so ids always grow from parent to child. When the
//! pool drains, one pass walks the batches in id order, which puts every parent before its children,
//! copies the entries into the final columns, and fills in `parent`, `depth` and the inherited hidden
//! flag without any lookup table keyed by path. File types come from the directory read itself (no
//! `stat`); the modified time costs one `lstat` per entry and can be switched off.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::exclude::Exclusions;
use crate::fold::fold_into;
use crate::segment::{F_DIR, F_HIDDEN, F_ROOT, NONE, Segment, SegmentBuilder};

/// How a scan runs.
#[derive(Clone, Debug)]
pub struct WalkOptions {
    /// Worker threads (0 means the number of logical CPUs).
    pub threads: usize,
    /// Read the modified time of every entry (one `lstat` each). Off makes the scan faster but recency
    /// ranking has nothing to work with until the entries are refreshed.
    pub with_mtime: bool,
    /// What to skip.
    pub exclude: Exclusions,
    /// Read folders with `std::fs::read_dir` on every platform instead of the Unix fast path (open the
    /// folder, read names without allocating, stat relative to it). For comparing the two and as an
    /// escape hatch.
    pub portable_reader: bool,
    /// macOS: read folders with `getattrlistbulk`, which returns the modified time with the names. Off
    /// falls back to a read plus one `lstat` per entry (for comparing the two, and for volumes that do
    /// not support bulk reads, which fall back by themselves). Ignored elsewhere.
    pub bulk_reader: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            threads: 0,
            with_mtime: true,
            exclude: Exclusions::defaults(),
            portable_reader: false,
            bulk_reader: true,
        }
    }
}

/// What a scan did.
#[derive(Clone, Debug, Default)]
pub struct WalkStats {
    /// Entries stored (files, folders and symlinks, roots included).
    pub entries: u64,
    /// Folders read.
    pub dirs_read: u64,
    /// Folders skipped by the exclusion policy (their names are still indexed).
    pub dirs_excluded: u64,
    /// Folders or entries that could not be read (permissions, races).
    pub errors: u64,
    /// Time spent walking and filling arenas.
    pub walk: Duration,
    /// Time spent merging arenas into the final columns.
    pub merge: Duration,
}

struct Ent {
    f_len: u32,
    o_len: u32,
    pub(crate) mtime: u32,
    mask: u32,
    flags: u8,
    child: u32,
}

struct BatchRec {
    id: u32,
    ent_lo: u32,
    ent_hi: u32,
    f_lo: u32,
    o_lo: u32,
}

#[derive(Default)]
struct Arena {
    f: Vec<u8>,
    o: Vec<u8>,
    ents: Vec<Ent>,
    batches: Vec<BatchRec>,
}

struct Ctx<'a> {
    opts: &'a WalkOptions,
    arenas: Vec<Mutex<Arena>>,
    next_id: AtomicU32,
    dirs_read: AtomicU64,
    dirs_excluded: AtomicU64,
    errors: AtomicU64,
    root_dev: u64,
    /// Entries read so far, for a caller that shows progress.
    progress: Option<&'a AtomicU64>,
}

#[cfg(all(unix, target_os = "macos"))]
fn dev_of(md: &fs::Metadata) -> u64 {
    // macOS device ids are 32 bits; getattrlistbulk reports them unsigned.
    u64::from(std::os::unix::fs::MetadataExt::dev(md) as u32)
}
#[cfg(all(unix, not(target_os = "macos")))]
fn dev_of(md: &fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::dev(md)
}
#[cfg(not(unix))]
fn dev_of(_md: &fs::Metadata) -> u64 {
    0
}

#[cfg(unix)]
fn mtime_of(md: &fs::Metadata) -> u32 {
    std::os::unix::fs::MetadataExt::mtime(md).clamp(0, u32::MAX as i64) as u32
}
#[cfg(not(unix))]
fn mtime_of(md: &fs::Metadata) -> u32 {
    md.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32)
}

/// One directory entry as the reader hands it over.
pub(crate) struct Raw<'a> {
    pub(crate) name: &'a std::ffi::OsStr,
    pub(crate) is_dir: bool,
    pub(crate) mtime: u32,
    pub(crate) dev: u64,
    pub(crate) stat_failed: bool,
}

/// Reads one folder and calls `f` for every entry (not `.` or `..`). File types come from the directory
/// read itself. When `want_stat` is set each entry is also `lstat`ed, relative to the open folder on Unix
/// so the kernel resolves one path component instead of the whole path. Returns `false` when the folder
/// itself could not be opened.
#[cfg(unix)]
fn read_entries_native(
    path: &Path,
    want_stat: bool,
    dirs_only_stat: bool,
    f: &mut dyn FnMut(Raw<'_>),
) -> bool {
    use rustix::fs::{AtFlags, Dir, FileType, Mode, OFlags, statat};
    use std::os::unix::ffi::OsStrExt;
    let Ok(fd) = rustix::fs::openat(
        rustix::fs::CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    ) else {
        return false;
    };
    let Ok(dir) = Dir::read_from(&fd) else {
        return false;
    };
    for ent in dir {
        let Ok(ent) = ent else { continue };
        let cname = ent.file_name();
        let bytes = cname.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        let ft = ent.file_type();
        let mut is_dir = ft == FileType::Directory;
        let (mut mtime, mut dev, mut stat_failed) = (0u32, 0u64, false);
        let need_stat = ft == FileType::Unknown
            || if is_dir {
                want_stat || dirs_only_stat
            } else {
                want_stat
            };
        if need_stat {
            match statat(&fd, cname, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(st) => {
                    // The integer types of `stat` fields differ by platform.
                    #[allow(clippy::unnecessary_cast)]
                    {
                        mtime = (st.st_mtime as i64).clamp(0, u32::MAX as i64) as u32;
                        dev = if cfg!(target_os = "macos") {
                            u64::from(st.st_dev as u32)
                        } else {
                            st.st_dev as u64
                        };
                    }
                    if ft == FileType::Unknown {
                        // `st_mode` is a u16 on macOS and a u32 on Linux.
                        #[allow(clippy::unnecessary_cast)]
                        let mode = st.st_mode as u32;
                        is_dir = (mode & 0o170000) == 0o040000;
                    }
                }
                Err(_) => stat_failed = true,
            }
        }
        f(Raw {
            name: std::ffi::OsStr::from_bytes(bytes),
            is_dir,
            mtime,
            dev,
            stat_failed,
        });
    }
    true
}

fn read_entries_std(
    path: &Path,
    want_stat: bool,
    dirs_only_stat: bool,
    f: &mut dyn FnMut(Raw<'_>),
) -> bool {
    let Ok(rd) = fs::read_dir(path) else {
        return false;
    };
    for ent in rd {
        let Ok(ent) = ent else { continue };
        let name = ent.file_name();
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        let (mut mtime, mut dev, mut stat_failed) = (0u32, 0u64, false);
        if want_stat || (is_dir && dirs_only_stat) {
            match ent.metadata() {
                Ok(md) => {
                    mtime = mtime_of(&md);
                    dev = dev_of(&md);
                }
                Err(_) => stat_failed = true,
            }
        }
        f(Raw {
            name: &name,
            is_dir,
            mtime,
            dev,
            stat_failed,
        });
    }
    true
}

pub(crate) fn read_entries(
    portable: bool,
    bulk: bool,
    path: &Path,
    want_stat: bool,
    dirs_only_stat: bool,
    f: &mut dyn FnMut(Raw<'_>),
) -> bool {
    #[cfg(target_os = "macos")]
    if !portable && bulk && want_stat {
        // One getattrlistbulk call returns name, type and modified time for a few hundred entries.
        // A file system that does not support it (some network and FAT volumes) falls back below,
        // unless entries were already delivered.
        use std::os::unix::ffi::OsStrExt;
        let mut delivered = 0usize;
        let r = panoply_platform_macos::read_dir_bulk(path, &mut |e| {
            delivered += 1;
            f(Raw {
                name: std::ffi::OsStr::from_bytes(e.name),
                is_dir: e.kind == panoply_platform_macos::EntryKind::Dir,
                mtime: e.mtime.clamp(0, i64::from(u32::MAX)) as u32,
                dev: u64::from(e.dev),
                stat_failed: false,
            });
        });
        if r.is_ok() || delivered > 0 {
            return true;
        }
    }
    #[cfg(unix)]
    if !portable {
        return read_entries_native(path, want_stat, dirs_only_stat, f);
    }
    let _ = (portable, bulk);
    read_entries_std(path, want_stat, dirs_only_stat, f)
}

/// Walks `roots` and returns the finished segment. Roots that cannot be read are counted as errors.
pub fn walk(roots: &[PathBuf], opts: &WalkOptions) -> io::Result<(Segment, WalkStats)> {
    walk_with_progress(roots, opts, None)
}

/// [`walk`], adding the number of entries read to `progress` as the scan goes (a count that only
/// grows, safe to poll from another thread, for an "indexing, 412,000 files so far" line).
pub fn walk_with_progress(
    roots: &[PathBuf],
    opts: &WalkOptions,
    progress: Option<&AtomicU64>,
) -> io::Result<(Segment, WalkStats)> {
    let threads = if opts.threads == 0 {
        std::thread::available_parallelism().map_or(4, |n| n.get())
    } else {
        opts.threads
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .thread_name(|i| format!("panoply-find-walk-{i}"))
        .build()
        .map_err(|e| io::Error::other(e.to_string()))?;
    let root_dev = roots
        .first()
        .and_then(|r| fs::metadata(r).ok())
        .map_or(0, |m| dev_of(&m));
    let ctx = Ctx {
        opts,
        arenas: (0..threads).map(|_| Mutex::new(Arena::default())).collect(),
        next_id: AtomicU32::new(roots.len() as u32),
        dirs_read: AtomicU64::new(0),
        dirs_excluded: AtomicU64::new(0),
        errors: AtomicU64::new(0),
        root_dev,
        progress,
    };
    let t0 = Instant::now();
    pool.scope(|s| {
        for (i, r) in roots.iter().enumerate() {
            let ctx = &ctx;
            let r = r.clone();
            s.spawn(move |s| visit(ctx, s, r, i as u32));
        }
    });
    let walk_time = t0.elapsed();

    // Merge.
    let t1 = Instant::now();
    let n_batches = ctx.next_id.load(Ordering::Relaxed) as usize;
    let mut arenas: Vec<Arena> = ctx
        .arenas
        .into_iter()
        .map(|m| m.into_inner().unwrap_or_else(|e| e.into_inner()))
        .collect();
    let mut by_id: Vec<Option<(usize, usize)>> = vec![None; n_batches];
    let mut total_ents = 0usize;
    let mut total_bytes = 0usize;
    for (ti, a) in arenas.iter().enumerate() {
        for (bi, b) in a.batches.iter().enumerate() {
            by_id[b.id as usize] = Some((ti, bi));
        }
        total_ents += a.ents.len();
        total_bytes += a.o.len();
    }
    let mut builder = SegmentBuilder::new(0);
    builder.reserve(total_ents + roots.len(), total_bytes + roots.len() * 64);
    let mut parent_of_batch: Vec<u32> = vec![NONE; n_batches];
    for (i, r) in roots.iter().enumerate() {
        let name = r.to_string_lossy();
        let mut f = Vec::new();
        let mask = fold_into(&name.replace('\\', "/"), &mut f);
        if f.is_empty() {
            f.push(b'/');
        }
        builder.push(name.as_bytes(), &f, NONE, 0, mask, F_DIR | F_ROOT, 0);
        parent_of_batch[i] = i as u32;
    }
    for id in 0..n_batches {
        let Some((ti, bi)) = by_id[id] else { continue };
        let parent = parent_of_batch[id];
        if parent == NONE {
            continue;
        }
        let pdepth = builder.depth_of(parent as usize);
        let phidden = builder.flags_of(parent as usize) & F_HIDDEN;
        let a = &arenas[ti];
        let rec = &a.batches[bi];
        let (mut fo, mut oo) = (rec.f_lo as usize, rec.o_lo as usize);
        let block_start = builder.len();
        for e in &a.ents[rec.ent_lo as usize..rec.ent_hi as usize] {
            let f = &a.f[fo..fo + e.f_len as usize];
            let o = &a.o[oo..oo + e.o_len as usize];
            fo += e.f_len as usize + 1;
            oo += e.o_len as usize + 1;
            let gid = builder.len() as u32;
            builder.push(
                o,
                f,
                parent,
                e.mtime,
                e.mask,
                e.flags | phidden,
                pdepth.saturating_add(1),
            );
            if e.child != NONE {
                parent_of_batch[e.child as usize] = gid;
            }
        }
        builder.add_block(parent, block_start, builder.len() - block_start);
    }
    let entries = builder.len() as u64;
    let seg = builder.finish();
    arenas.clear();
    let stats = WalkStats {
        entries,
        dirs_read: ctx.dirs_read.load(Ordering::Relaxed),
        dirs_excluded: ctx.dirs_excluded.load(Ordering::Relaxed),
        errors: ctx.errors.load(Ordering::Relaxed),
        walk: walk_time,
        merge: t1.elapsed(),
    };
    Ok((seg, stats))
}

fn visit<'s>(ctx: &'s Ctx<'s>, scope: &rayon::Scope<'s>, path: PathBuf, batch_id: u32) {
    let tidx = rayon::current_thread_index()
        .unwrap_or(0)
        .min(ctx.arenas.len() - 1);
    let mut guard = ctx.arenas[tidx].lock().unwrap_or_else(|e| e.into_inner());
    let a = &mut *guard;
    let (e0, f0, o0) = (a.ents.len(), a.f.len(), a.o.len());
    let ex = &ctx.opts.exclude;
    let mut pending: Vec<(usize, PathBuf, bool)> = Vec::new();
    let mut has_cargo = false;
    let mut never_index = false;
    let opened = read_entries(
        ctx.opts.portable_reader,
        ctx.opts.bulk_reader,
        &path,
        ctx.opts.with_mtime,
        ex.one_file_system,
        &mut |raw| {
            let name = raw.name.to_string_lossy();
            if name.is_empty() {
                return;
            }
            if raw.stat_failed {
                ctx.errors.fetch_add(1, Ordering::Relaxed);
            }
            let o_start = a.o.len();
            a.o.extend_from_slice(name.as_bytes());
            a.o.push(0);
            let f_start = a.f.len();
            let mask = fold_into(&name, &mut a.f);
            if a.f.len() == f_start {
                // A name made only of combining marks folds to nothing; keep the entry findable.
                a.f.push(b'?');
            }
            let f_len = a.f.len() - f_start;
            a.f.push(0);
            let folded = &a.f[f_start..f_start + f_len];
            if folded == b".metadata_never_index" {
                never_index = true;
            }
            if folded == b"cargo.toml" {
                has_cargo = true;
            }
            let mut flags = 0u8;
            if name.starts_with('.') {
                flags |= F_HIDDEN;
            }
            let ent_idx = a.ents.len();
            if raw.is_dir {
                flags |= F_DIR;
                let is_target = folded == b"target";
                let child_path = path.join(raw.name);
                let other_fs = ex.one_file_system && raw.dev != ctx.root_dev && !raw.stat_failed;
                if other_fs || ex.name_excluded(folded) || ex.path_excluded(&child_path) {
                    ctx.dirs_excluded.fetch_add(1, Ordering::Relaxed);
                } else {
                    pending.push((ent_idx, child_path, is_target && ex.cargo_target));
                }
            }
            a.ents.push(Ent {
                f_len: f_len as u32,
                o_len: (a.o.len() - o_start - 1) as u32,
                mtime: raw.mtime,
                mask,
                flags,
                child: NONE,
            });
        },
    );
    if !opened {
        ctx.errors.fetch_add(1, Ordering::Relaxed);
        return;
    }
    ctx.dirs_read.fetch_add(1, Ordering::Relaxed);
    if let Some(p) = ctx.progress {
        p.fetch_add((a.ents.len() - e0) as u64, Ordering::Relaxed);
    }
    if never_index {
        // The folder asked not to be indexed: drop what was read and do not descend.
        a.ents.truncate(e0);
        a.f.truncate(f0);
        a.o.truncate(o0);
        a.batches.push(BatchRec {
            id: batch_id,
            ent_lo: e0 as u32,
            ent_hi: e0 as u32,
            f_lo: f0 as u32,
            o_lo: o0 as u32,
        });
        ctx.dirs_excluded.fetch_add(1, Ordering::Relaxed);
        return;
    }
    a.batches.push(BatchRec {
        id: batch_id,
        ent_lo: e0 as u32,
        ent_hi: a.ents.len() as u32,
        f_lo: f0 as u32,
        o_lo: o0 as u32,
    });
    for (ent_idx, child_path, cargo_target) in pending {
        if cargo_target && has_cargo {
            ctx.dirs_excluded.fetch_add(1, Ordering::Relaxed);
            continue;
        }
        let id = ctx.next_id.fetch_add(1, Ordering::Relaxed);
        a.ents[ent_idx].child = id;
        scope.spawn(move |s| visit(ctx, s, child_path, id));
    }
}

/// A walk with the `ignore` crate's parallel walker, for the benchmark baseline. It resolves parents by
/// hashing paths (the visitor sees entries in no useful order), so it is slower to assemble than
/// [`walk`]; it exists to answer "what would the stock walker give us".
#[cfg(feature = "ignore-walker")]
pub fn walk_ignore(root: &Path, opts: &WalkOptions) -> io::Result<(Segment, Duration)> {
    use std::collections::HashMap;
    use std::hash::Hasher;

    struct Row {
        o: Vec<u8>,
        f: Vec<u8>,
        mask: u32,
        mtime: u32,
        flags: u8,
        depth: u8,
        parent_hash: u64,
        self_hash: u64,
    }
    fn h(p: &Path) -> u64 {
        let mut s = std::hash::DefaultHasher::new();
        s.write(p.as_os_str().as_encoded_bytes());
        s.finish()
    }
    let t0 = Instant::now();
    let sink: Mutex<Vec<Vec<Row>>> = Mutex::new(Vec::new());
    struct Local<'a> {
        rows: Vec<Row>,
        sink: &'a Mutex<Vec<Vec<Row>>>,
    }
    impl Drop for Local<'_> {
        fn drop(&mut self) {
            let rows = std::mem::take(&mut self.rows);
            self.sink.lock().unwrap().push(rows);
        }
    }
    let ex = &opts.exclude;
    let want_mtime = opts.with_mtime;
    let root_owned = root.to_path_buf();
    let mut b = ignore::WalkBuilder::new(root);
    b.standard_filters(false)
        .follow_links(false)
        .threads(if opts.threads == 0 { 0 } else { opts.threads });
    let ex2 = ex.clone();
    b.filter_entry(move |e| {
        if e.depth() == 0 {
            return true;
        }
        if e.file_type().is_some_and(|t| t.is_dir()) {
            let name = e.file_name().to_string_lossy();
            let (f, _) = crate::fold::fold(&name);
            if ex2.name_excluded(&f) || ex2.path_excluded(e.path()) {
                return false;
            }
        }
        true
    });
    b.build_parallel().run(|| {
        let mut local = Local {
            rows: Vec::new(),
            sink: &sink,
        };
        let root = root_owned.clone();
        Box::new(move |res| {
            let Ok(e) = res else {
                return ignore::WalkState::Continue;
            };
            let name = e.file_name().to_string_lossy().into_owned();
            let depth = e.depth();
            let mut f = Vec::new();
            let (mask, o) = if depth == 0 {
                let full = e.path().to_string_lossy().into_owned();
                (fold_into(&full, &mut f), full.into_bytes())
            } else {
                (fold_into(&name, &mut f), name.clone().into_bytes())
            };
            if f.is_empty() {
                f.push(b'?');
            }
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            let mtime = if want_mtime {
                e.metadata().ok().map_or(0, |m| mtime_of(&m))
            } else {
                0
            };
            let mut flags = if is_dir { F_DIR } else { 0 };
            if depth == 0 {
                flags |= F_ROOT;
            } else if let Ok(rel) = e.path().strip_prefix(&root)
                && rel
                    .components()
                    .any(|c| c.as_os_str().as_encoded_bytes().first() == Some(&b'.'))
            {
                flags |= F_HIDDEN;
            }
            local.rows.push(Row {
                o,
                f,
                mask,
                mtime,
                flags,
                depth: depth.min(255) as u8,
                parent_hash: e.path().parent().map_or(0, h),
                self_hash: if is_dir { h(e.path()) } else { 0 },
            });
            ignore::WalkState::Continue
        })
    });
    let chunks = sink.into_inner().unwrap();
    let total: usize = chunks.iter().map(Vec::len).sum();
    let mut dir_ids: HashMap<u64, u32> = HashMap::new();
    let mut gid = 0u32;
    for c in &chunks {
        for r in c {
            if r.flags & F_DIR != 0 {
                dir_ids.insert(r.self_hash, gid);
            }
            gid += 1;
        }
    }
    let mut builder = SegmentBuilder::new(0);
    builder.reserve(total, total * 24);
    for c in &chunks {
        for r in c {
            let parent = if r.flags & F_ROOT != 0 {
                NONE
            } else {
                dir_ids.get(&r.parent_hash).copied().unwrap_or(NONE)
            };
            builder.push(&r.o, &r.f, parent, r.mtime, r.mask, r.flags, r.depth);
        }
    }
    Ok((builder.finish(), t0.elapsed()))
}

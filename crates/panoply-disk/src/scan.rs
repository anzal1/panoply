//! The parallel scanner.
//!
//! Workers share one stack of folders to read (last in, first out, so the queue stays short) and one tree
//! builder. A worker reads a folder through the platform backend into a private batch, then appends the
//! batch to the tree under a short lock, which keeps every folder's children next to each other. When the
//! stack is empty and no worker is busy the walk is over; hard links and clones are then counted once and
//! sizes are added up the tree in one backwards pass.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::backend::{self, Backend, BackendChoice, DirFd, Entry, Scratch, SkipReason};
use crate::tree::{Kind, Node, NodeId, Tree, flags};
use crate::volume::{self, VolumeInfo};

/// How to scan.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Worker threads. 0 means twice the cores, between 8 and 32.
    pub threads: usize,
    /// Which backend (default: the fastest the platform has).
    pub backend: BackendChoice,
    /// Enter other local volumes mounted below the root. Off by default.
    pub cross_volumes: bool,
    /// Enter network mounts (only when `cross_volumes` is on). Off by default.
    pub include_network: bool,
    /// Count APFS clones once (macOS only, small cost). On by default.
    pub clone_aware: bool,
    /// Fold the small plain files of each folder (16 KiB or less on disk, no other names, no clone) into
    /// one "N small files" entry. Sizes and counts stay exact; the tree gets about five times smaller on a
    /// typical disk. On by default; off keeps every file as its own entry.
    pub collapse_small_files: bool,
    /// Set to true to stop. The scan returns what it has, flagged as cancelled.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Live counters for a progress display.
    pub progress: Option<Arc<Progress>>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            threads: 0,
            backend: BackendChoice::Auto,
            cross_volumes: false,
            include_network: false,
            clone_aware: true,
            collapse_small_files: true,
            cancel: None,
            progress: None,
        }
    }
}

/// Live counters, updated as folders are read. Read them from any thread.
#[derive(Debug, Default)]
pub struct Progress {
    /// Files and links seen so far.
    pub files: AtomicU64,
    /// Folders read so far.
    pub dirs: AtomicU64,
    /// Allocated bytes seen so far (before hard link and clone corrections).
    pub bytes: AtomicU64,
}

/// A folder the scan could not read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Skipped {
    /// The folder.
    pub node: NodeId,
    /// Why.
    pub reason: SkipReason,
}

/// Why a scan failed outright (nothing to show).
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The root does not exist or cannot be opened.
    #[error("cannot open {0}: {1}")]
    Root(PathBuf, std::io::Error),
}

/// What a scan found and what it left out.
#[derive(Clone, Debug, Default)]
pub struct ScanReport {
    /// The backend that read the folders.
    pub backend: &'static str,
    /// Worker threads used.
    pub threads: usize,
    /// Wall time of the walk and the final passes.
    pub elapsed: Duration,
    /// Allocated bytes under the root, hard links and clones counted once.
    pub total_bytes: u64,
    /// Files, links and devices counted (small files folded into one entry still count one each).
    pub files: u64,
    /// Entries stored in the tree: what memory is spent on.
    pub nodes: u64,
    /// Folders read (including unreadable ones).
    pub dirs: u64,
    /// Bytes not counted because a hard link points at a file already counted.
    pub hard_link_bytes_saved: u64,
    /// Number of hard link entries that were not counted again.
    pub hard_link_entries: u64,
    /// Bytes not counted because an APFS clone shares them with a file already counted.
    pub clone_bytes_saved: u64,
    /// Files stored in less space than their length (sparse or compressed).
    pub sparse_or_compressed: u64,
    /// Folders that could not be read, with the reason for each.
    pub skipped: Vec<Skipped>,
    /// Mount points of other volumes that were not entered.
    pub other_volumes: u64,
    /// True when the scan was stopped early. The tree is partial.
    pub cancelled: bool,
    /// True when the tree reached its size limit (4 billion entries or name bytes). It is partial.
    pub truncated: bool,
    /// The volume the root lives on: size, free and (macOS) purgeable space.
    pub volume: Option<VolumeInfo>,
}

impl ScanReport {
    /// True when at least one folder was refused in a way Full Disk Access would fix.
    pub fn needs_full_disk_access(&self) -> bool {
        self.skipped
            .iter()
            .any(|s| s.reason == SkipReason::FullDiskAccess)
    }

    /// Folders skipped for one reason.
    pub fn skipped_count(&self, reason: SkipReason) -> usize {
        self.skipped.iter().filter(|s| s.reason == reason).count()
    }
}

/// A finished scan.
#[derive(Debug)]
pub struct Scan {
    /// The tree.
    pub tree: Tree,
    /// What was counted and what was left out.
    pub report: ScanReport,
}

struct Task {
    node: u32,
    path: PathBuf,
    /// The open parent folder, so this one opens with a single name lookup.
    parent: Option<Arc<DirFd>>,
}

/// One folder's worth of entries, before it is appended to the tree.
#[derive(Default)]
struct Batch {
    names: Vec<u8>,
    recs: Vec<Rec>,
    links: Vec<LinkRec>,
    clones: Vec<CloneRec>,
    /// Small plain files folded together: bytes and count.
    small_bytes: u64,
    small_files: u32,
}

impl Batch {
    fn clear(&mut self) {
        self.names.clear();
        self.recs.clear();
        self.links.clear();
        self.clones.clear();
        self.small_bytes = 0;
        self.small_files = 0;
    }
}

/// Marks a clone record that belongs to the folder's folded entry, whose position is known only at the merge.
const IN_SMALL: u32 = u32::MAX;

/// Files up to this many bytes on disk are folded into their folder's "small files" entry.
const SMALL_MAX: u64 = 16 * 1024;

#[derive(Clone, Copy)]
struct Rec {
    size: u64,
    items: u32,
    name_off: u32,
    name_len: u16,
    kind: u8,
    flags: u8,
}

/// Identity of a file that may have several names. Sorted to find the repeats.
#[derive(Clone, Copy)]
struct LinkRec {
    dev: u64,
    id: u64,
    id_hi: u64,
    /// Index within the batch, then the absolute node.
    node: u32,
}

#[derive(Clone, Copy)]
struct CloneRec {
    clone_id: u64,
    shared: u64,
    node: u32,
}

#[derive(Default)]
struct Builder {
    nodes: Vec<Node>,
    names: Vec<u8>,
    links: Vec<LinkRec>,
    clones: Vec<CloneRec>,
    skipped: Vec<Skipped>,
    full: bool,
}

#[derive(Default)]
struct Queue {
    stack: Vec<Task>,
    busy: usize,
    done: bool,
}

struct Shared<'a> {
    backend: &'a dyn Backend,
    queue: Mutex<Queue>,
    cv: Condvar,
    builder: Mutex<Builder>,
    root_devs: Vec<u64>,
    /// Mount points below the root, by parent folder: name and whether it is a network filesystem.
    mounts: std::collections::HashMap<PathBuf, Vec<(Vec<u8>, bool)>>,
    cross: bool,
    network_ok: bool,
    collapse: bool,
    cancel: Option<&'a AtomicBool>,
    progress: Option<&'a Progress>,
    cancelled: AtomicBool,
    root_is_slash: bool,
}

#[cfg(unix)]
fn path_from_bytes(parent: &Path, name: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    parent.join(std::ffi::OsStr::from_bytes(name))
}

#[cfg(not(unix))]
fn path_from_bytes(parent: &Path, name: &[u8]) -> PathBuf {
    parent.join(String::from_utf8_lossy(name).as_ref())
}

#[cfg(unix)]
fn root_bytes(p: &Path) -> Vec<u8> {
    use std::os::unix::ffi::OsStrExt;
    p.as_os_str().as_bytes().to_vec()
}

#[cfg(not(unix))]
fn root_bytes(p: &Path) -> Vec<u8> {
    p.to_string_lossy().into_owned().into_bytes()
}

/// The mount points strictly below `root`, grouped by the folder that holds them.
fn mounts_below(root: &Path) -> std::collections::HashMap<PathBuf, Vec<(Vec<u8>, bool)>> {
    let mut map: std::collections::HashMap<PathBuf, Vec<(Vec<u8>, bool)>> =
        std::collections::HashMap::new();
    for (mount, network) in volume::mount_points() {
        if mount == root || !mount.starts_with(root) {
            continue;
        }
        if let (Some(parent), Some(name)) = (mount.parent(), mount.file_name()) {
            map.entry(parent.to_path_buf())
                .or_default()
                .push((root_bytes(Path::new(name)), network));
        }
    }
    map
}

/// Scan `root` and everything below it.
pub fn scan(root: &Path, opts: &ScanOptions) -> Result<Scan, ScanError> {
    let started = Instant::now();
    let root = std::fs::canonicalize(root).map_err(|e| ScanError::Root(root.to_path_buf(), e))?;
    let backend = backend::pick(opts.backend, opts.clone_aware);
    let info = backend
        .root_info(&root)
        .map_err(|e| ScanError::Root(root.clone(), e))?;

    // Reading metadata waits on the disk far more than it computes, so twice the cores keeps the disk's queue
    // full (8 threads took 10 s on a million files where 16 to 32 took 6 s on an 18-core Mac).
    let threads = if opts.threads == 0 {
        std::thread::available_parallelism().map_or(8, |n| (n.get() * 2).clamp(8, 32))
    } else {
        opts.threads
    }
    .clamp(1, 64);

    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut root_devs = vec![info.dev];
    #[cfg(target_os = "macos")]
    let root_is_slash = root == Path::new("/");
    #[cfg(not(target_os = "macos"))]
    let root_is_slash = false;
    #[cfg(target_os = "macos")]
    if root_is_slash && let Ok(d) = backend.root_info(Path::new("/System/Volumes/Data")) {
        // "/" is the sealed system volume. Its firmlinks (/Users, /Applications, ...) lead into the Data
        // volume, which is the same Mac as far as a person is concerned.
        root_devs.push(d.dev);
    }

    let mut builder = Builder::default();
    builder.names.extend_from_slice(&root_bytes(&root));
    builder.nodes.push(Node {
        size: info.alloc,
        items: u32::from(info.kind != Kind::Dir),
        name_off: 0,
        parent: 0,
        first_child: 1,
        child_count: 0,
        name_len: builder.names.len().min(u16::MAX as usize) as u16,
        kind: info.kind as u8,
        flags: 0,
    });
    // A root path longer than 65535 bytes cannot happen on any supported OS.

    let shared = Shared {
        backend: backend.as_ref(),
        queue: Mutex::new(Queue::default()),
        cv: Condvar::new(),
        builder: Mutex::new(builder),
        root_devs,
        mounts: mounts_below(&root),
        cross: opts.cross_volumes,
        network_ok: opts.include_network,
        collapse: opts.collapse_small_files,
        cancel: opts.cancel.as_deref(),
        progress: opts.progress.as_deref(),
        cancelled: AtomicBool::new(false),
        root_is_slash,
    };

    if info.kind == Kind::Dir {
        #[cfg(unix)]
        backend::fds::raise_limit();
        shared.queue.lock().unwrap().stack.push(Task {
            node: 0,
            path: root.clone(),
            parent: None,
        });
        std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| worker(&shared));
            }
        });
    }

    let Shared {
        builder, cancelled, ..
    } = shared;
    let b = builder.into_inner().unwrap();
    let mut report = ScanReport {
        backend: backend.name(),
        threads,
        cancelled: cancelled.load(Ordering::Relaxed),
        truncated: b.full,
        ..ScanReport::default()
    };
    let Builder {
        nodes,
        names,
        links,
        clones,
        skipped,
        ..
    } = b;
    let mut tree = Tree { nodes, names };
    finish(
        &mut tree,
        links,
        clones,
        threads,
        opts.clone_aware && backend::PROBES_CLONES,
        &mut report,
    );
    report.skipped = skipped;
    report.skipped.sort_by_key(|s| s.node.0);
    report.elapsed = started.elapsed();
    report.volume = volume::info(&root).ok();
    // Giving back slack copies the whole array, which is itself the peak; only worth it for real waste.
    if tree.nodes.capacity() > tree.nodes.len() + tree.nodes.len() / 4 {
        tree.nodes.shrink_to_fit();
    }
    if tree.names.capacity() > tree.names.len() + tree.names.len() / 4 {
        tree.names.shrink_to_fit();
    }
    Ok(Scan { tree, report })
}

fn worker(sh: &Shared<'_>) {
    let mut scratch = Scratch::new(scratch_bytes());
    let mut batch = Batch::default();
    loop {
        let task = {
            let mut q = sh.queue.lock().unwrap();
            loop {
                if q.done {
                    return;
                }
                if let Some(t) = q.stack.pop() {
                    q.busy += 1;
                    break t;
                }
                if q.busy == 0 {
                    q.done = true;
                    sh.cv.notify_all();
                    return;
                }
                q = sh.cv.wait(q).unwrap();
            }
        };

        let stop = sh.cancel.is_some_and(|c| c.load(Ordering::Relaxed));
        let mut subdirs: Vec<Task> = Vec::new();
        if stop {
            sh.cancelled.store(true, Ordering::Relaxed);
        } else {
            read_one(sh, &task, &mut scratch, &mut batch, &mut subdirs);
        }

        let mut q = sh.queue.lock().unwrap();
        let n = subdirs.len();
        q.stack.append(&mut subdirs);
        q.busy -= 1;
        if stop {
            q.stack.clear();
        }
        if q.stack.is_empty() && q.busy == 0 {
            q.done = true;
            sh.cv.notify_all();
        } else if n > 1 {
            sh.cv.notify_all();
        } else if n == 1 {
            sh.cv.notify_one();
        }
    }
}

fn scratch_bytes() -> usize {
    #[cfg(target_os = "macos")]
    return backend::macos::BUF;
    #[cfg(target_os = "linux")]
    return backend::linux::BUF;
    #[cfg(windows)]
    return backend::windows::BUF;
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    return 1;
}

fn read_one(
    sh: &Shared<'_>,
    task: &Task,
    scratch: &mut Scratch,
    batch: &mut Batch,
    out: &mut Vec<Task>,
) {
    batch.clear();
    let mut enter: Vec<u32> = Vec::new(); // batch indices of folders to descend into
    let mut files = 0u64;
    let mut bytes = 0u64;
    let res = {
        let batch = &mut *batch;
        let enter = &mut enter;
        let mounts_here = sh.mounts.get(task.path.as_path());
        sh.backend.read_dir(
            &task.path,
            task.parent.as_deref(),
            scratch,
            &mut |e: &Entry<'_>| {
                // A small plain file with no other name and no clone partner costs nothing worth a node.
                if sh.collapse
                    && e.kind == Kind::File
                    && e.nlink == 1
                    && e.flags == 0
                    && e.alloc <= SMALL_MAX
                    && e.apparent < e.alloc.saturating_add(64 * 1024)
                    && !e.is_mount_hint
                {
                    files += 1;
                    bytes += e.alloc;
                    batch.small_bytes += e.alloc;
                    batch.small_files += 1;
                    if e.clone_shared > 0 {
                        // A folded clone still gives its bytes back, from the folder's folded entry.
                        batch.clones.push(CloneRec {
                            clone_id: e.clone_id,
                            shared: e.clone_shared,
                            node: IN_SMALL,
                        });
                    }
                    return;
                }
                let idx = batch.recs.len() as u32;
                let name_off = batch.names.len() as u32;
                let take = e.name.len().min(u16::MAX as usize);
                batch.names.extend_from_slice(&e.name[..take]);
                let mut fl = e.flags;
                let kind = e.kind;
                if kind == Kind::Dir {
                    let mut skip_other = false;
                    if let Some(ms) = mounts_here
                        && let Some((_, network)) = ms.iter().find(|(n, _)| n.as_slice() == e.name)
                        && (!sh.cross || (*network && !sh.network_ok))
                    {
                        skip_other = true;
                        if *network {
                            fl |= flags::NETWORK_VOLUME;
                        }
                    }
                    if e.dev != 0 && !sh.root_devs.contains(&e.dev) {
                        skip_other = !sh.cross;
                        if sh.cross && !sh.network_ok && is_network(&task.path, e.name) {
                            fl |= flags::NETWORK_VOLUME;
                            skip_other = true;
                        }
                    }
                    if sh.root_is_slash
                        && task.path == Path::new("/System/Volumes")
                        && e.name == b"Data"
                    {
                        skip_other = true; // reached through the firmlinks already
                    }
                    if skip_other {
                        fl |= flags::OTHER_VOLUME;
                    } else {
                        enter.push(idx);
                    }
                } else {
                    files += 1;
                    bytes += e.alloc;
                    if e.is_mount_hint {
                        fl |= flags::OTHER_VOLUME;
                    }
                    if kind == Kind::File
                        && fl & flags::DATALESS == 0
                        && e.apparent >= e.alloc.saturating_add(64 * 1024)
                    {
                        fl |= flags::SPARSE_OR_COMPRESSED;
                    }
                    // Link candidates: macOS and Linux report the link count; Windows does not (0 = unknown).
                    if kind == Kind::File && e.alloc > 0 && e.nlink != 1 && e.id != 0 {
                        batch.links.push(LinkRec {
                            dev: e.dev,
                            id: e.id,
                            id_hi: e.id_hi,
                            node: idx,
                        });
                    }
                    if e.clone_shared > 0 {
                        batch.clones.push(CloneRec {
                            clone_id: e.clone_id,
                            shared: e.clone_shared,
                            node: idx,
                        });
                    }
                }
                batch.recs.push(Rec {
                    size: e.alloc,
                    items: u32::from(kind != Kind::Dir),
                    name_off,
                    name_len: take as u16,
                    kind: kind as u8,
                    flags: fl,
                });
            },
        )
    };

    if res.is_ok() && batch.small_files > 0 {
        batch.recs.push(Rec {
            size: batch.small_bytes,
            items: batch.small_files,
            name_off: batch.names.len() as u32,
            name_len: 0,
            kind: Kind::Small as u8,
            flags: 0,
        });
    }
    let mut b = sh.builder.lock().unwrap();
    let held = match res {
        Ok(fd) => fd.map(Arc::new),
        Err(reason) => {
            let node = &mut b.nodes[task.node as usize];
            node.flags |= flags::UNREADABLE;
            b.skipped.push(Skipped {
                node: NodeId(task.node),
                reason,
            });
            return;
        }
    };
    let count = batch.recs.len();
    let base = b.nodes.len();
    if base + count >= u32::MAX as usize - 1
        || b.names.len() + batch.names.len() >= u32::MAX as usize - 1
    {
        b.full = true;
        return;
    }
    let name_base = b.names.len() as u32;
    b.names.extend_from_slice(&batch.names);
    b.nodes.reserve(count);
    for r in &batch.recs {
        b.nodes.push(Node {
            size: r.size,
            items: r.items,
            name_off: name_base + r.name_off,
            parent: task.node,
            first_child: 0,
            child_count: 0,
            name_len: r.name_len,
            kind: r.kind,
            flags: r.flags,
        });
    }
    {
        let parent = &mut b.nodes[task.node as usize];
        parent.first_child = base as u32;
        parent.child_count = count as u32;
    }
    for l in &batch.links {
        b.links.push(LinkRec {
            node: base as u32 + l.node,
            ..*l
        });
    }
    for c in &batch.clones {
        b.clones.push(CloneRec {
            // The folded entry is the last one of the batch.
            node: base as u32
                + if c.node == IN_SMALL {
                    count as u32 - 1
                } else {
                    c.node
                },
            ..*c
        });
    }
    drop(b);

    if let Some(p) = sh.progress {
        p.files.fetch_add(files, Ordering::Relaxed);
        p.dirs.fetch_add(1, Ordering::Relaxed);
        p.bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    for idx in enter {
        let r = batch.recs[idx as usize];
        let name = &batch.names[r.name_off as usize..r.name_off as usize + r.name_len as usize];
        out.push(Task {
            node: base as u32 + idx,
            path: path_from_bytes(&task.path, name),
            parent: held.clone(),
        });
    }
}

/// Network mounts below a folder: only asked when scanning across volumes.
fn is_network(parent: &Path, name: &[u8]) -> bool {
    volume::is_network_mount(&path_from_bytes(parent, name))
}

/// After the walk: count each hard-linked file and each clone once, then add sizes up the tree.
fn finish(
    tree: &mut Tree,
    mut links: Vec<LinkRec>,
    clones: Vec<CloneRec>,
    threads: usize,
    probe_big_files: bool,
    report: &mut ScanReport,
) {
    // Hard links: every file with a link count above 1 (or unknown) is listed with its identity. Sorted
    // by identity then position, the first of each run keeps its size and the rest become 0.
    links.sort_unstable_by(|a, c| {
        (a.dev, a.id, a.id_hi, a.node).cmp(&(c.dev, c.id, c.id_hi, c.node))
    });
    let mut i = 0;
    while i < links.len() {
        let mut j = i + 1;
        while j < links.len()
            && (links[j].dev, links[j].id, links[j].id_hi)
                == (links[i].dev, links[i].id, links[i].id_hi)
        {
            let n = &mut tree.nodes[links[j].node as usize];
            report.hard_link_bytes_saved += n.size;
            report.hard_link_entries += 1;
            n.size = 0;
            n.flags |= flags::HARD_LINK_DUPLICATE;
            n.flags &= !flags::CLONE_SHARED;
            j += 1;
        }
        i = j;
    }
    drop(links);

    resolve_clones(tree, clones, threads, probe_big_files, report);

    // Add sizes and item counts up the tree. A child always has a higher index than its parent.
    for i in (1..tree.nodes.len()).rev() {
        let (size, items, parent) = {
            let n = &tree.nodes[i];
            (n.size, n.items, n.parent as usize)
        };
        let p = &mut tree.nodes[parent];
        p.size += size;
        p.items += items;
    }
    if let Some(root) = tree.nodes.first() {
        report.total_bytes = root.size;
    }
    report.nodes = tree.nodes.len() as u64;
    for n in &tree.nodes {
        if n.kind == Kind::Dir as u8 {
            report.dirs += 1;
            if n.flags & flags::OTHER_VOLUME != 0 {
                report.other_volumes += 1;
            }
        } else if n.flags & flags::SPARSE_OR_COMPRESSED != 0 {
            report.sparse_or_compressed += 1;
        }
    }
    // The root's item count is every file and link below it, folded ones included.
    report.files = tree.nodes.first().map_or(0, |r| u64::from(r.items));
}

/// Files this big are checked for shared blocks by their physical extents (see `resolve_clones`).
const PROBE_MIN_BYTES: u64 = 1 << 20;

/// APFS clones.
///
/// - Small files (under 1 MiB) that still name clone partners (same clone ID, count above 1) are unedited
///   copies: they hold exactly the same blocks, so one member of each group keeps its size and the others
///   give theirs back. This needs nothing but what the walk already read.
/// - Big files are settled by their physical extents, which is exact also for a clone that was edited after
///   the copy (it forgets its partners but keeps sharing the untouched blocks). Asking the OS for every
///   file's private size doubles a scan, so only files of 1 MiB or more are asked; a small edited clone can
///   still be counted twice for the blocks it shares, under 1 MiB per file.
/// - A big file that cannot be opened falls back to the group rule when it still names a group.
fn resolve_clones(
    tree: &mut Tree,
    clones: Vec<CloneRec>,
    threads: usize,
    probe_big_files: bool,
    report: &mut ScanReport,
) {
    let threads = threads.max(1);
    let (small, mut big): (Vec<CloneRec>, Vec<CloneRec>) = clones
        .into_iter()
        .partition(|c| tree.nodes[c.node as usize].size < PROBE_MIN_BYTES);
    give_back_groups(tree, small, report);

    if probe_big_files {
        let known: std::collections::HashSet<u32> = big.iter().map(|c| c.node).collect();
        let candidates: Vec<u32> = tree
            .nodes
            .iter()
            .enumerate()
            .filter(|(i, n)| {
                n.kind == Kind::File as u8
                    && n.size >= PROBE_MIN_BYTES
                    && n.flags & flags::HARD_LINK_DUPLICATE == 0
                    && !known.contains(&(*i as u32))
            })
            .map(|(i, _)| i as u32)
            .collect();
        let t: &Tree = tree;
        let hits = parallel_map(threads, candidates.len(), |i| {
            backend::private_size(&t.path(NodeId(candidates[i])))
                .is_some_and(|(alloc, private)| private < alloc)
        });
        for (i, hit) in hits.into_iter().enumerate() {
            if hit {
                big.push(CloneRec {
                    clone_id: 0,
                    shared: tree.nodes[candidates[i] as usize].size,
                    node: candidates[i],
                });
            }
        }
    }
    if big.is_empty() {
        return;
    }

    let t: &Tree = tree;
    let extents = parallel_map(threads, big.len(), |i| {
        let node = big[i].node;
        backend::file_extents(&t.path(NodeId(node)), t.nodes[node as usize].size)
    });

    // Sweep all ranges in physical order; the first file to reach a block owns it.
    let mut runs: Vec<(u64, u64, u64, u32)> = Vec::new(); // (dev, start, end, candidate)
    for (i, ex) in extents.iter().enumerate() {
        if let Some((dev, ex)) = ex {
            for &(start, len) in ex {
                runs.push((*dev, start, start + len, i as u32));
            }
        }
    }
    runs.sort_unstable();
    let mut claimed = vec![0u64; big.len()];
    let mut own = vec![0u64; big.len()];
    let (mut cur_dev, mut covered) = (u64::MAX, 0u64);
    for &(dev, start, end, cand) in &runs {
        own[cand as usize] += end - start;
        if dev != cur_dev {
            cur_dev = dev;
            covered = 0;
        }
        let from = start.max(covered);
        if end > from {
            claimed[cand as usize] += end - from;
        }
        covered = covered.max(end);
    }
    drop(runs);
    for (i, ex) in extents.iter().enumerate() {
        if ex.is_none() {
            continue;
        }
        let n = &mut tree.nodes[big[i].node as usize];
        if n.flags & flags::HARD_LINK_DUPLICATE != 0 {
            continue;
        }
        let give = own[i].saturating_sub(claimed[i]).min(n.size);
        if give > 0 {
            n.size -= give;
            n.flags |= flags::CLONE_SHARED;
            report.clone_bytes_saved += give;
        }
    }

    let failed: Vec<CloneRec> = big
        .iter()
        .zip(&extents)
        .filter(|(_, ex)| ex.is_none())
        .map(|(c, _)| *c)
        .collect();
    give_back_groups(tree, failed, report);
}

/// Each clone group keeps the bytes once: the member with the most (ties: the lowest position) is counted,
/// every other member gives its shared bytes back. Files without a group ID are left alone.
fn give_back_groups(tree: &mut Tree, mut recs: Vec<CloneRec>, report: &mut ScanReport) {
    recs.retain(|c| c.clone_id != 0);
    recs.sort_unstable_by(|a, c| {
        a.clone_id
            .cmp(&c.clone_id)
            .then(c.shared.cmp(&a.shared))
            .then(a.node.cmp(&c.node))
    });
    let mut i = 0;
    while i < recs.len() {
        let mut j = i + 1;
        while j < recs.len() && recs[j].clone_id == recs[i].clone_id {
            let n = &mut tree.nodes[recs[j].node as usize];
            if n.flags & flags::HARD_LINK_DUPLICATE == 0 {
                let give = recs[j].shared.min(n.size);
                n.size -= give;
                n.flags |= flags::CLONE_SHARED;
                report.clone_bytes_saved += give;
            }
            j += 1;
        }
        i = j;
    }
}

/// Run `f(0..n)` on up to `threads` threads and collect the results in order.
fn parallel_map<T: Send>(threads: usize, n: usize, f: impl Fn(usize) -> T + Sync) -> Vec<T> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<T>>> = (0..n).map(|_| Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..threads.min(n).max(1) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= n {
                        break;
                    }
                    *slots[i].lock().unwrap() = Some(f(i));
                }
            });
        }
    });
    slots
        .into_iter()
        .map(|m| m.into_inner().unwrap().expect("every slot is filled"))
        .collect()
}

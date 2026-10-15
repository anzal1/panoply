//! Keeping the index fresh: applying file system changes to a built index.
//!
//! A watcher reports "something at this path changed". [`LiveIndex::apply_path`] answers by asking the
//! file system what is there now (one `lstat`), never by trusting the kind of event, so a lost or
//! reordered event can not corrupt anything: a missing path is removed, a new path is appended, a known
//! path gets its modified time refreshed, a new folder is scanned. Appended entries go into a small
//! delta segment next to the big one and removed entries get a tombstone flag; a removed folder takes
//! everything below it with it in one linear pass at the end of a batch. When the delta or the
//! tombstones grow, [`LiveIndex::compact`] folds everything back into one segment in breadth-first
//! order, which keeps each folder's listing contiguous (that is what makes finding a child by name a
//! short scan instead of a path hash table, and keeps the memory at about 57 bytes per file).

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::exclude::Exclusions;
use crate::fold::fold_into;
use crate::index::Index;
use crate::segment::{F_DEAD, F_DIR, F_HIDDEN, F_ROOT, NONE, Segment, SegmentBuilder};
use crate::walk::{Raw, read_entries};

/// What [`LiveIndex::apply_path`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// A new entry (and, for a folder, everything below it) was added.
    Added,
    /// A known entry was refreshed.
    Updated,
    /// The path is gone and its entry (with everything below it) was removed.
    Removed,
    /// The path is outside the roots or inside an excluded folder, or could not be read.
    Ignored,
}

/// An [`Index`] plus what is needed to update it in place.
pub struct LiveIndex {
    index: Index,
    roots: Vec<(PathBuf, u32)>,
    delta_children: HashMap<u32, Vec<u32>>,
    exclude: Exclusions,
    dir_removed: bool,
    /// Entries added since the last compaction.
    pub added: usize,
    /// Entries removed since the last compaction.
    pub removed: usize,
}

impl LiveIndex {
    /// Wraps a freshly built or loaded index. The roots are the entries flagged as scan roots.
    pub fn new(index: Index, exclude: Exclusions) -> Self {
        let mut roots = Vec::new();
        for seg in index.segments() {
            for e in 0..seg.len() {
                if seg.flags[e] & F_ROOT != 0 && !seg.is_dead(e) {
                    roots.push((
                        PathBuf::from(String::from_utf8_lossy(seg.o_name(e)).into_owned()),
                        seg.base + e as u32,
                    ));
                }
            }
        }
        LiveIndex {
            index,
            roots,
            delta_children: HashMap::new(),
            exclude,
            dir_removed: false,
            added: 0,
            removed: 0,
        }
    }

    /// The index to search.
    pub fn index(&self) -> &Index {
        &self.index
    }

    /// The exclusion policy in force.
    pub fn exclusions(&self) -> &Exclusions {
        &self.exclude
    }

    /// The scan roots.
    pub fn roots(&self) -> impl Iterator<Item = &Path> {
        self.roots.iter().map(|r| r.0.as_path())
    }

    /// Looks a path up: the id of its live entry.
    pub fn lookup(&self, path: &Path) -> Option<u32> {
        let (root_path, root_id) = self.roots.iter().find(|(r, _)| path.starts_with(r))?;
        let rel = path.strip_prefix(root_path).ok()?;
        let mut cur = *root_id;
        for c in rel.components() {
            let Component::Normal(name) = c else {
                return None;
            };
            cur = self.find_child(cur, &name.to_string_lossy())?;
        }
        Some(cur)
    }

    fn find_child(&self, dir: u32, name: &str) -> Option<u32> {
        let want = name.as_bytes();
        for seg in self.index.segments() {
            for (start, count) in seg.blocks_of(dir) {
                for e in start..start + count {
                    if !seg.is_dead(e) && seg.o_name(e) == want {
                        return Some(seg.base + e as u32);
                    }
                }
            }
        }
        for &g in self.delta_children.get(&dir)? {
            if let Some((seg, l)) = self.index.locate(g)
                && !seg.is_dead(l)
                && seg.o_name(l) == want
            {
                return Some(g);
            }
        }
        None
    }

    fn mark_dead(&mut self, id: u32) {
        let Some(i) = self
            .index
            .segs
            .partition_point(|s| s.base <= id)
            .checked_sub(1)
        else {
            return;
        };
        let seg = &mut self.index.segs[i];
        let l = (id - seg.base) as usize;
        if l < seg.len() && !seg.is_dead(l) {
            if seg.flags[l] & F_DIR != 0 {
                self.dir_removed = true;
            }
            seg.mark_dead(l);
            self.removed += 1;
        }
    }

    /// Tombstones everything below a removed folder. One pass: a parent always comes before its children.
    fn propagate_deaths(&mut self) {
        if !self.dir_removed {
            return;
        }
        self.dir_removed = false;
        for si in 0..self.index.segs.len() {
            for e in 0..self.index.segs[si].len() {
                if self.index.segs[si].is_dead(e) {
                    continue;
                }
                let p = self.index.segs[si].parent[e];
                if p == NONE {
                    continue;
                }
                let dead_parent = self.index.locate(p).is_some_and(|(s, l)| s.is_dead(l));
                if dead_parent {
                    self.index.segs[si].flags[e] |= F_DEAD;
                    self.removed += 1;
                }
            }
        }
    }

    fn delta(&mut self) -> &mut Segment {
        let total = self.index.len() as u32;
        let needs_new = self.index.segs.len() < 2;
        if needs_new {
            self.index.segs.push(Segment::with_base(total));
        }
        self.index.segs.last_mut().expect("a delta segment exists")
    }

    fn push_entry(&mut self, parent: u32, name: &str, mtime: u32, is_dir: bool) -> Option<u32> {
        let (pdepth, phidden) = {
            let (s, l) = self.index.locate(parent)?;
            (s.depth[l], s.flags[l] & F_HIDDEN)
        };
        let mut folded = Vec::new();
        let mask = fold_into(name, &mut folded);
        if folded.is_empty() {
            folded.push(b'?');
        }
        let mut flags = phidden;
        if name.starts_with('.') {
            flags |= F_HIDDEN;
        }
        if is_dir {
            flags |= F_DIR;
        }
        let seg = self.delta();
        let gid = seg.base + seg.len() as u32;
        seg.push(
            name.as_bytes(),
            &folded,
            parent,
            mtime,
            mask,
            flags,
            pdepth.saturating_add(1),
        );
        self.delta_children.entry(parent).or_default().push(gid);
        self.added += 1;
        Some(gid)
    }

    /// Brings the index in line with what is on disk at `path`.
    pub fn apply_path(&mut self, path: &Path) -> ApplyOutcome {
        let Some((root, _)) = self
            .roots
            .iter()
            .find(|(r, _)| path.starts_with(r))
            .cloned()
        else {
            return ApplyOutcome::Ignored;
        };
        if path == root || self.exclude.has_excluded_ancestor(&root, path) {
            return ApplyOutcome::Ignored;
        }
        let existing = self.lookup(path);
        let md = match fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return match existing {
                    Some(id) => {
                        self.mark_dead(id);
                        ApplyOutcome::Removed
                    }
                    None => ApplyOutcome::Ignored,
                };
            }
            Err(_) => return ApplyOutcome::Ignored,
        };
        let is_dir = md.is_dir();
        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs().min(u32::MAX as u64) as u32);
        if let Some(id) = existing {
            let (kind_matches, i, l) = {
                let i = self.index.segs.partition_point(|s| s.base <= id) - 1;
                let l = (id - self.index.segs[i].base) as usize;
                (((self.index.segs[i].flags[l] & F_DIR) != 0) == is_dir, i, l)
            };
            if kind_matches {
                self.index.segs[i].set_mtime(l, mtime);
                return ApplyOutcome::Updated;
            }
            self.mark_dead(id);
            self.propagate_deaths();
        }
        // New entry. Make sure its parent exists first (a deep new folder reports its parts in any order).
        let Some(parent_path) = path.parent() else {
            return ApplyOutcome::Ignored;
        };
        let parent = if parent_path == root {
            self.roots.iter().find(|(r, _)| *r == root).map(|r| r.1)
        } else {
            match self.lookup(parent_path) {
                Some(p) => Some(p),
                None => {
                    self.apply_path(parent_path);
                    self.lookup(parent_path)
                }
            }
        };
        let Some(parent) = parent else {
            return ApplyOutcome::Ignored;
        };
        // The parent may just have scanned this very path as part of a new subtree.
        if self
            .find_child(
                parent,
                &path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
            .is_some()
        {
            return ApplyOutcome::Added;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(gid) = self.push_entry(parent, &name, mtime, is_dir) else {
            return ApplyOutcome::Ignored;
        };
        if is_dir {
            let (folded, _) = crate::fold::fold(&name);
            if !self.exclude.name_excluded(&folded) && !self.exclude.path_excluded(path) {
                self.scan_into(path, gid);
            }
        }
        ApplyOutcome::Added
    }

    /// Reads a whole folder tree below `dir` (already in the index as `dir_id`) and appends it.
    fn scan_into(&mut self, dir: &Path, dir_id: u32) {
        let mut stack = vec![(dir.to_path_buf(), dir_id)];
        let root_dev = dir_device(dir);
        while let Some((path, id)) = stack.pop() {
            let mut found: Vec<(String, bool, u32, u64)> = Vec::new();
            read_entries(false, &path, true, true, &mut |raw: Raw<'_>| {
                found.push((
                    raw.name.to_string_lossy().into_owned(),
                    raw.is_dir,
                    raw.mtime,
                    raw.dev,
                ));
            });
            for (name, is_dir, mtime, dev) in found {
                if name.is_empty() {
                    continue;
                }
                let Some(gid) = self.push_entry(id, &name, mtime, is_dir) else {
                    continue;
                };
                if is_dir {
                    let child = path.join(&name);
                    let (folded, _) = crate::fold::fold(&name);
                    let other_fs =
                        self.exclude.one_file_system && root_dev.is_some_and(|d| d != dev);
                    if !other_fs
                        && !self.exclude.name_excluded(&folded)
                        && !self.exclude.path_excluded(&child)
                    {
                        stack.push((child, gid));
                    }
                }
            }
        }
    }

    /// Forgets everything below `dir` and reads it again: the answer to an event that says changes were
    /// coalesced or dropped ("must scan subdirectories").
    pub fn rescan_dir(&mut self, dir: &Path) {
        let Some(id) = self.lookup(dir) else {
            self.apply_path(dir);
            return;
        };
        let mut children: Vec<u32> = Vec::new();
        for seg in self.index.segments() {
            for (start, count) in seg.blocks_of(id) {
                children.extend((start..start + count).map(|e| seg.base + e as u32));
            }
        }
        if let Some(c) = self.delta_children.remove(&id) {
            children.extend(c);
        }
        for c in children {
            self.mark_dead(c);
        }
        self.propagate_deaths();
        if dir_device(dir).is_some() {
            self.scan_into(dir, id);
        }
    }

    /// Call after a batch of [`LiveIndex::apply_path`]: finishes folder removals.
    pub fn finish_batch(&mut self) {
        self.propagate_deaths();
    }

    /// True when the delta or the tombstones are big enough that compacting pays for itself.
    pub fn wants_compaction(&self) -> bool {
        let total = self.index.len().max(1);
        self.added > 50_000.max(total / 20) || self.removed > 50_000.max(total / 10)
    }

    /// Folds the delta and the tombstones into one fresh segment, breadth first, so every folder's
    /// listing is contiguous again. Ids change; nothing outside this struct may hold on to one.
    pub fn compact(&mut self) {
        let total = self.index.len();
        let mut by_depth: Vec<Vec<u32>> = Vec::new();
        for seg in self.index.segments() {
            for e in 0..seg.len() {
                if seg.is_dead(e) {
                    continue;
                }
                let d = seg.depth[e] as usize;
                if by_depth.len() <= d {
                    by_depth.resize_with(d + 1, Vec::new);
                }
                by_depth[d].push(seg.base + e as u32);
            }
        }
        let mut newid = vec![NONE; total];
        let mut b = SegmentBuilder::new(0);
        b.reserve(total - self.removed.min(total), self.index.heap_bytes() / 2);
        for mut level in by_depth {
            level.sort_by_key(|&g| {
                let (s, l) = self.index.locate(g).expect("live id");
                let p = s.parent[l];
                if p == NONE { 0 } else { newid[p as usize] }
            });
            let mut run_parent = NONE;
            let mut run_start = 0usize;
            for g in level {
                let (s, l) = self.index.locate(g).expect("live id");
                let p = s.parent[l];
                let np = if p == NONE { NONE } else { newid[p as usize] };
                if np != run_parent {
                    if run_parent != NONE {
                        b.add_block(run_parent, run_start, b.len() - run_start);
                    }
                    run_parent = np;
                    run_start = b.len();
                }
                newid[g as usize] = b.len() as u32;
                b.push(
                    s.o_name(l),
                    s.f_name(l),
                    np,
                    s.mtime[l],
                    s.mask[l],
                    s.flags[l] & !F_DEAD,
                    s.depth[l],
                );
            }
            if run_parent != NONE {
                b.add_block(run_parent, run_start, b.len() - run_start);
            }
        }
        for r in &mut self.roots {
            r.1 = newid[r.1 as usize];
        }
        self.index = Index::from_segment(b.finish());
        self.delta_children.clear();
        self.added = 0;
        self.removed = 0;
    }

    /// Compacts and writes the index to `path` (written to a sibling file and renamed, so a crash never
    /// leaves half a file).
    pub fn save(&mut self, path: &Path) -> std::io::Result<()> {
        if self.index.segments().len() > 1 || self.removed > 0 {
            self.compact();
        }
        let tmp = path.with_extension("tmp");
        {
            let f = fs::File::create(&tmp)?;
            self.index.segments()[0].write_to(std::io::BufWriter::new(f))?;
        }
        fs::rename(tmp, path)
    }
}

#[cfg(unix)]
fn dir_device(p: &Path) -> Option<u64> {
    let md = fs::symlink_metadata(p).ok()?;
    #[cfg(target_os = "macos")]
    {
        Some(u64::from(std::os::unix::fs::MetadataExt::dev(&md) as u32))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Some(std::os::unix::fs::MetadataExt::dev(&md))
    }
}
#[cfg(not(unix))]
fn dir_device(p: &Path) -> Option<u64> {
    fs::symlink_metadata(p).ok().map(|_| 0)
}

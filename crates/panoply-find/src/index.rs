//! The index: a list of segments and the ways to read an entry back out of them.

use std::io;
use std::path::{MAIN_SEPARATOR, PathBuf};

use crate::segment::{F_DIR, F_HIDDEN, NONE, Segment};
use crate::walk::{WalkOptions, WalkStats, walk};

/// One entry read back from the index.
#[derive(Clone, Debug)]
pub struct EntryInfo {
    /// Global id.
    pub id: u32,
    /// The file or folder name as the file system spells it.
    pub name: String,
    /// The full path.
    pub path: PathBuf,
    /// True for a folder.
    pub is_dir: bool,
    /// True when the name or an ancestor starts with a dot.
    pub hidden: bool,
    /// Modified time, seconds since the Unix epoch (0 when it was not read).
    pub mtime: u32,
}

/// All entries of a scan. Today one big immutable segment; the watcher adds small delta segments and
/// tombstones next to it and a background merge folds them back in.
#[derive(Default, Clone)]
pub struct Index {
    pub(crate) segs: Vec<Segment>,
}

impl Index {
    /// An index over one finished segment.
    pub fn from_segment(seg: Segment) -> Self {
        Index { segs: vec![seg] }
    }

    /// Scans `roots` and indexes them.
    pub fn build(roots: &[PathBuf], opts: &WalkOptions) -> io::Result<(Index, WalkStats)> {
        let (seg, stats) = walk(roots, opts)?;
        Ok((Index::from_segment(seg), stats))
    }

    /// The segments, oldest first.
    pub fn segments(&self) -> &[Segment] {
        &self.segs
    }

    /// Entries stored, tombstones included.
    pub fn len(&self) -> usize {
        self.segs.iter().map(Segment::len).sum()
    }

    /// True when nothing is indexed.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Bytes the columns hold on the heap.
    pub fn heap_bytes(&self) -> usize {
        self.segs.iter().map(Segment::heap_bytes).sum()
    }

    fn locate(&self, id: u32) -> Option<(&Segment, usize)> {
        let i = self.segs.partition_point(|s| s.base <= id).checked_sub(1)?;
        let s = &self.segs[i];
        let local = (id - s.base) as usize;
        (local < s.len()).then_some((s, local))
    }

    /// Reads one entry back, or `None` for an unknown id.
    pub fn entry(&self, id: u32) -> Option<EntryInfo> {
        let (s, l) = self.locate(id)?;
        Some(EntryInfo {
            id,
            name: String::from_utf8_lossy(s.o_name(l)).into_owned(),
            path: self.path(id)?,
            is_dir: s.flags[l] & F_DIR != 0,
            hidden: s.flags[l] & F_HIDDEN != 0,
            mtime: s.mtime[l],
        })
    }

    /// The full path of an entry: the root's path, then each folder name down to the entry.
    pub fn path(&self, id: u32) -> Option<PathBuf> {
        let mut chain = [0u32; 260];
        let mut n = 0;
        let mut cur = id;
        while cur != NONE && n < chain.len() {
            chain[n] = cur;
            n += 1;
            let (s, l) = self.locate(cur)?;
            cur = s.parent[l];
        }
        let mut out = String::new();
        for &c in chain[..n].iter().rev() {
            let (s, l) = self.locate(c)?;
            let name = String::from_utf8_lossy(s.o_name(l));
            if !out.is_empty() && !out.ends_with(MAIN_SEPARATOR) && !out.ends_with('/') {
                out.push(MAIN_SEPARATOR);
            }
            out.push_str(&name);
        }
        Some(PathBuf::from(out))
    }

    /// True when the folded name of the folder `parent` or of any folder above it contains `needle`.
    pub(crate) fn ancestor_contains(&self, mut parent: u32, needle: &[u8]) -> bool {
        let finder = memchr::memmem::Finder::new(needle);
        while parent != NONE {
            let Some((s, l)) = self.locate(parent) else {
                return false;
            };
            if finder.find(s.f_name(l)).is_some() {
                return true;
            }
            parent = s.parent[l];
        }
        false
    }

    /// The folded full path (`/` between parts), used to check path tokens such as `src/lib`.
    pub(crate) fn folded_path(&self, id: u32) -> Vec<u8> {
        let mut chain = [0u32; 260];
        let mut n = 0;
        let mut cur = id;
        while cur != NONE && n < chain.len() {
            let Some((s, l)) = self.locate(cur) else {
                break;
            };
            chain[n] = cur;
            n += 1;
            cur = s.parent[l];
        }
        let mut out = Vec::with_capacity(96);
        for &c in chain[..n].iter().rev() {
            if let Some((s, l)) = self.locate(c) {
                if !out.is_empty() && out.last() != Some(&b'/') {
                    out.push(b'/');
                }
                out.extend_from_slice(s.f_name(l));
            }
        }
        out
    }
}

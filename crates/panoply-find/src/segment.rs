//! The compact name table.
//!
//! A [`Segment`] is a struct of arrays, one slot per file or folder:
//!
//! | column | bytes | what |
//! |---|---|---|
//! | folded names | name length + 1 | lowercase, accent-free copy, each name ends in a zero byte |
//! | original names | name length + 1 | the name as the file system spells it, zero terminated |
//! | `f_off`, `o_off` | 4 + 4 | where each name starts in the two buffers (plus one end sentinel) |
//! | `parent` | 4 | id of the folder that holds it (interned: a folder is stored once, not once per child) |
//! | `mtime` | 4 | modified time, seconds since the Unix epoch |
//! | `mask` | 4 | [`CharMask`](crate::fold::CharMask), the fuzzy prefilter |
//! | `first` | 1 | first byte of the folded name, so a prefix scan reads one byte per file |
//! | `depth`, `flags` | 1 + 1 | levels below the root; folder, hidden, root, dead bits |
//!
//! No path is stored anywhere: a path is rebuilt by following `parent` and joining original names, and
//! that happens only for the handful of rows on screen. About 23 bytes of columns plus two copies of the
//! name per file.

use std::io::{self, Read, Write};

/// "No parent" and "no entry".
pub const NONE: u32 = u32::MAX;
/// The entry is a folder.
pub const F_DIR: u8 = 1;
/// The entry or one of its ancestors starts with a dot.
pub const F_HIDDEN: u8 = 2;
/// The entry is a scan root; its name is the whole path. Never returned as a result.
pub const F_ROOT: u8 = 4;
/// The entry was removed after the segment was built (a tombstone).
pub const F_DEAD: u8 = 8;

/// One immutable, contiguous block of entries. See the module docs for the layout.
#[derive(Default, Clone)]
pub struct Segment {
    pub(crate) base: u32,
    pub(crate) f_names: Vec<u8>,
    pub(crate) o_names: Vec<u8>,
    pub(crate) f_off: Vec<u32>,
    pub(crate) o_off: Vec<u32>,
    pub(crate) parent: Vec<u32>,
    pub(crate) mtime: Vec<u32>,
    pub(crate) mask: Vec<u32>,
    pub(crate) first: Vec<u8>,
    pub(crate) depth: Vec<u8>,
    pub(crate) flags: Vec<u8>,
    /// Where each folder's listing sits: `(folder id, first local index, count)`, sorted by folder id.
    /// A walker writes a folder's entries as one block, so finding a child by name is a short scan.
    pub(crate) blocks: Vec<(u32, u32, u32)>,
}

impl Segment {
    /// Number of entries, dead ones included.
    pub fn len(&self) -> usize {
        self.parent.len()
    }

    /// True when the segment holds no entries.
    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }

    /// Global id of the first entry.
    pub fn base(&self) -> u32 {
        self.base
    }

    #[inline]
    pub(crate) fn f_name(&self, i: usize) -> &[u8] {
        &self.f_names[self.f_off[i] as usize..self.f_off[i + 1] as usize - 1]
    }

    #[inline]
    pub(crate) fn o_name(&self, i: usize) -> &[u8] {
        &self.o_names[self.o_off[i] as usize..self.o_off[i + 1] as usize - 1]
    }

    /// Bytes held by the columns (capacity, which is what the allocator charged).
    pub fn heap_bytes(&self) -> usize {
        self.f_names.capacity()
            + self.o_names.capacity()
            + 4 * (self.f_off.capacity()
                + self.o_off.capacity()
                + self.parent.capacity()
                + self.mtime.capacity()
                + self.mask.capacity())
            + self.first.capacity()
            + self.depth.capacity()
            + self.flags.capacity()
            + 12 * self.blocks.capacity()
    }

    /// An empty segment whose first entry will have global id `base`.
    pub fn with_base(base: u32) -> Self {
        let mut seg = Segment {
            base,
            ..Segment::default()
        };
        seg.f_off.push(0);
        seg.o_off.push(0);
        seg
    }

    /// Appends one entry. `folded` and `original` are the name without its terminator; `folded` must be
    /// non-empty and neither may contain a zero byte.
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        original: &[u8],
        folded: &[u8],
        parent: u32,
        mtime: u32,
        mask: u32,
        flags: u8,
        depth: u8,
    ) {
        debug_assert!(!folded.is_empty() && !folded.contains(&0) && !original.contains(&0));
        self.f_names.extend_from_slice(folded);
        self.f_names.push(0);
        self.o_names.extend_from_slice(original);
        self.o_names.push(0);
        self.f_off.push(self.f_names.len() as u32);
        self.o_off.push(self.o_names.len() as u32);
        self.parent.push(parent);
        self.mtime.push(mtime);
        self.mask.push(mask);
        self.first.push(folded[0]);
        self.depth.push(depth);
        self.flags.push(flags);
    }

    /// Records that the entries `start..start + count` (local indexes) are the listing of folder `dir`.
    pub fn add_block(&mut self, dir: u32, start: usize, count: usize) {
        if count > 0 {
            self.blocks.push((dir, start as u32, count as u32));
        }
    }

    /// Sorts the block table; call once after the last `add_block`.
    pub fn seal_blocks(&mut self) {
        self.blocks.sort_unstable();
    }

    /// The listing blocks of folder `dir` in this segment (a folder can have more than one after updates).
    pub(crate) fn blocks_of(&self, dir: u32) -> impl Iterator<Item = (usize, usize)> + '_ {
        let lo = self.blocks.partition_point(|b| b.0 < dir);
        self.blocks[lo..]
            .iter()
            .take_while(move |b| b.0 == dir)
            .map(|b| (b.1 as usize, b.2 as usize))
    }

    /// True for a removed entry.
    pub fn is_dead(&self, i: usize) -> bool {
        self.flags[i] & F_DEAD != 0
    }

    /// Replaces an entry's modified time.
    pub fn set_mtime(&mut self, i: usize, mtime: u32) {
        self.mtime[i] = mtime;
    }

    /// Marks an entry removed. The bytes stay until the segment is rebuilt.
    pub fn mark_dead(&mut self, i: usize) {
        self.flags[i] |= F_DEAD;
    }

    /// Writes the columns in a flat little-endian format (see [`Segment::read_from`]).
    pub fn write_to<W: Write>(&self, mut w: W) -> io::Result<()> {
        w.write_all(MAGIC)?;
        w.write_all(&(self.len() as u64).to_le_bytes())?;
        w.write_all(&self.base.to_le_bytes())?;
        w.write_all(&(self.f_names.len() as u64).to_le_bytes())?;
        w.write_all(&(self.o_names.len() as u64).to_le_bytes())?;
        w.write_all(&self.f_names)?;
        w.write_all(&self.o_names)?;
        w.write_all(&self.first)?;
        w.write_all(&self.depth)?;
        w.write_all(&self.flags)?;
        w.write_all(&(self.blocks.len() as u64).to_le_bytes())?;
        let mut bytes = Vec::with_capacity(self.blocks.len() * 12);
        for b in &self.blocks {
            bytes.extend_from_slice(&b.0.to_le_bytes());
            bytes.extend_from_slice(&b.1.to_le_bytes());
            bytes.extend_from_slice(&b.2.to_le_bytes());
        }
        w.write_all(&bytes)?;
        for col in [
            &self.f_off,
            &self.o_off,
            &self.parent,
            &self.mtime,
            &self.mask,
        ] {
            let mut bytes = Vec::with_capacity(col.len() * 4);
            for v in col.iter() {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            w.write_all(&bytes)?;
        }
        Ok(())
    }

    /// Reads what [`Segment::write_to`] wrote. Rejects a file whose magic or sizes do not fit.
    pub fn read_from<R: Read>(mut r: R) -> io::Result<Segment> {
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(bad("not a panoply-find segment"));
        }
        let mut b8 = [0u8; 8];
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b8)?;
        let n = u64::from_le_bytes(b8) as usize;
        r.read_exact(&mut b4)?;
        let base = u32::from_le_bytes(b4);
        r.read_exact(&mut b8)?;
        let fl = u64::from_le_bytes(b8) as usize;
        r.read_exact(&mut b8)?;
        let ol = u64::from_le_bytes(b8) as usize;
        if n >= NONE as usize || fl > u32::MAX as usize || ol > u32::MAX as usize {
            return Err(bad("sizes out of range"));
        }
        let mut seg = Segment {
            base,
            ..Segment::default()
        };
        let read_vec = |r: &mut R, len: usize| -> io::Result<Vec<u8>> {
            let mut v = vec![0u8; len];
            r.read_exact(&mut v)?;
            Ok(v)
        };
        seg.f_names = read_vec(&mut r, fl)?;
        seg.o_names = read_vec(&mut r, ol)?;
        seg.first = read_vec(&mut r, n)?;
        seg.depth = read_vec(&mut r, n)?;
        seg.flags = read_vec(&mut r, n)?;
        let col = |r: &mut R, len: usize| -> io::Result<Vec<u32>> {
            let bytes = read_vec(r, len * 4)?;
            Ok(bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|c| u32::from_le_bytes(*c))
                .collect())
        };
        r.read_exact(&mut b8)?;
        let nb = u64::from_le_bytes(b8) as usize;
        if nb > n {
            return Err(bad("too many blocks"));
        }
        let raw = read_vec(&mut r, nb * 12)?;
        seg.blocks = raw
            .as_chunks::<12>()
            .0
            .iter()
            .map(|c| {
                (
                    u32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                    u32::from_le_bytes([c[4], c[5], c[6], c[7]]),
                    u32::from_le_bytes([c[8], c[9], c[10], c[11]]),
                )
            })
            .collect();
        if seg.blocks.iter().any(|b| b.1 as usize + b.2 as usize > n) {
            return Err(bad("block out of range"));
        }
        seg.f_off = col(&mut r, n + 1)?;
        seg.o_off = col(&mut r, n + 1)?;
        seg.parent = col(&mut r, n)?;
        seg.mtime = col(&mut r, n)?;
        seg.mask = col(&mut r, n)?;
        // Cheap structural checks so a damaged file cannot make a query index out of bounds.
        if seg.f_off.last().copied() != Some(fl as u32)
            || seg.o_off.last().copied() != Some(ol as u32)
        {
            return Err(bad("offset tables do not match buffers"));
        }
        if !seg.f_off.windows(2).all(|w| w[0] < w[1]) || !seg.o_off.windows(2).all(|w| w[0] < w[1])
        {
            return Err(bad("offsets not increasing"));
        }
        Ok(seg)
    }
}

const MAGIC: &[u8; 8] = b"PFIXSEG2";

/// Appends entries and produces a [`Segment`].
#[derive(Default)]
pub struct SegmentBuilder {
    seg: Segment,
}

impl SegmentBuilder {
    /// A builder whose first entry will have global id `base`.
    pub fn new(base: u32) -> Self {
        SegmentBuilder {
            seg: Segment::with_base(base),
        }
    }

    /// Reserves room for `entries` rows and `name_bytes` of names in each buffer.
    pub fn reserve(&mut self, entries: usize, name_bytes: usize) {
        let s = &mut self.seg;
        s.f_names.reserve(name_bytes);
        s.o_names.reserve(name_bytes);
        s.f_off.reserve(entries);
        s.o_off.reserve(entries);
        s.parent.reserve(entries);
        s.mtime.reserve(entries);
        s.mask.reserve(entries);
        s.first.reserve(entries);
        s.depth.reserve(entries);
        s.flags.reserve(entries);
    }

    /// Number of entries so far.
    pub fn len(&self) -> usize {
        self.seg.len()
    }

    /// True before the first push.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Depth of an entry already pushed (local index).
    pub(crate) fn depth_of(&self, local: usize) -> u8 {
        self.seg.depth[local]
    }

    /// Flags of an entry already pushed (local index).
    pub(crate) fn flags_of(&self, local: usize) -> u8 {
        self.seg.flags[local]
    }

    /// Appends one entry (see [`Segment::push`]).
    #[allow(clippy::too_many_arguments)]
    pub fn push(
        &mut self,
        original: &[u8],
        folded: &[u8],
        parent: u32,
        mtime: u32,
        mask: u32,
        flags: u8,
        depth: u8,
    ) {
        self.seg
            .push(original, folded, parent, mtime, mask, flags, depth);
    }

    /// Records a folder listing block (see [`Segment::add_block`]).
    pub fn add_block(&mut self, dir: u32, start: usize, count: usize) {
        self.seg.add_block(dir, start, count);
    }

    /// Finishes the segment, releasing spare capacity.
    pub fn finish(mut self) -> Segment {
        let s = &mut self.seg;
        s.seal_blocks();
        s.f_names.shrink_to_fit();
        s.o_names.shrink_to_fit();
        s.f_off.shrink_to_fit();
        s.o_off.shrink_to_fit();
        s.parent.shrink_to_fit();
        s.mtime.shrink_to_fit();
        s.mask.shrink_to_fit();
        s.first.shrink_to_fit();
        s.depth.shrink_to_fit();
        s.flags.shrink_to_fit();
        s.blocks.shrink_to_fit();
        self.seg
    }
}

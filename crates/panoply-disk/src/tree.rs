//! The aggregated tree: 32 bytes per entry, names in one shared byte buffer.
//!
//! Children of a folder are stored next to each other, so a folder is `first_child` and `child_count`
//! and walking down is slicing. Sizes are allocated bytes, already added up the tree when a scan
//! finishes. Nothing here allocates per node.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::ffi::OsString;
use std::path::PathBuf;

/// An entry in a [`Tree`]. Index 0 is the root.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

impl NodeId {
    /// The scan root.
    pub const ROOT: NodeId = NodeId(0);
    /// The index into the tree's node array.
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What an entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// A folder.
    Dir = 0,
    /// A regular file.
    File = 1,
    /// A symbolic link or Windows reparse point. Never followed.
    Symlink = 2,
    /// A device, socket, pipe or anything else without data of its own.
    Other = 3,
    /// All the small plain files of one folder, folded into one entry: its size is their total and
    /// [`Tree::items`] says how many there are. It has no name and no path of its own.
    Small = 4,
}

impl Kind {
    fn from_u8(v: u8) -> Kind {
        match v {
            0 => Kind::Dir,
            1 => Kind::File,
            2 => Kind::Symlink,
            4 => Kind::Small,
            _ => Kind::Other,
        }
    }
}

/// Bit flags on a [`Node`].
pub mod flags {
    /// The folder could not be read (permission, Full Disk Access, vanished). Its size is what was
    /// counted before the refusal, usually 0.
    pub const UNREADABLE: u8 = 1;
    /// A hard link to a file already counted elsewhere in the scan: its size here is 0.
    pub const HARD_LINK_DUPLICATE: u8 = 1 << 1;
    /// Shares data with an APFS clone elsewhere: only the part not counted for another clone is here.
    pub const CLONE_SHARED: u8 = 1 << 2;
    /// Stored in less space than its length: a sparse or compressed file.
    pub const SPARSE_OR_COMPRESSED: u8 = 1 << 3;
    /// Content lives in the cloud (iCloud, OneDrive placeholder): costs nothing on this disk.
    pub const DATALESS: u8 = 1 << 4;
    /// The mount point of another volume that the scan did not enter.
    pub const OTHER_VOLUME: u8 = 1 << 5;
    /// A network mount that the scan did not enter.
    pub const NETWORK_VOLUME: u8 = 1 << 6;
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(crate) struct Node {
    /// Allocated bytes, folders include everything below.
    pub size: u64,
    /// Files (and links) below, 1 for a file itself.
    pub items: u32,
    pub name_off: u32,
    pub parent: u32,
    pub first_child: u32,
    pub child_count: u32,
    pub name_len: u16,
    pub kind: u8,
    pub flags: u8,
}

const _: () = assert!(std::mem::size_of::<Node>() == 32);

/// A scanned folder tree.
#[derive(Debug, Default)]
pub struct Tree {
    pub(crate) nodes: Vec<Node>,
    pub(crate) names: Vec<u8>,
}

impl Tree {
    /// A tree built from `(path below the root, bytes)` pairs, with no disk read at all. For renders and
    /// tests (the launch film draws a believable home folder this way); a real scan never calls it.
    /// `root` is the name of the root entry and also the start of every path ("/Users/maya").
    pub fn from_files(root: &str, files: &[(&str, u64)]) -> Tree {
        use std::collections::BTreeMap;
        #[derive(Default)]
        struct D {
            dirs: BTreeMap<String, D>,
            files: Vec<(String, u64)>,
        }
        let mut top = D::default();
        for (path, bytes) in files {
            let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
            let Some((name, dirs)) = parts.split_last() else {
                continue;
            };
            let mut cur = &mut top;
            for d in dirs {
                cur = cur.dirs.entry((*d).to_string()).or_default();
            }
            cur.files.push(((*name).to_string(), *bytes));
        }
        let mut tree = Tree::default();
        let push = |tree: &mut Tree, name: &str, kind: Kind, parent: u32| -> u32 {
            let off = tree.names.len() as u32;
            tree.names.extend_from_slice(name.as_bytes());
            tree.nodes.push(Node {
                size: 0,
                items: if kind == Kind::File { 1 } else { 0 },
                name_off: off,
                parent,
                first_child: 0,
                child_count: 0,
                name_len: name.len() as u16,
                kind: kind as u8,
                flags: 0,
            });
            (tree.nodes.len() - 1) as u32
        };
        let r = push(&mut tree, root, Kind::Dir, 0);
        let mut queue = std::collections::VecDeque::new();
        queue.push_back((&top, r));
        while let Some((d, idx)) = queue.pop_front() {
            let first = tree.nodes.len() as u32;
            tree.nodes[idx as usize].first_child = first;
            tree.nodes[idx as usize].child_count = (d.dirs.len() + d.files.len()) as u32;
            for (name, sub) in &d.dirs {
                let ci = push(&mut tree, name, Kind::Dir, idx);
                queue.push_back((sub, ci));
            }
            for (name, bytes) in &d.files {
                let ci = push(&mut tree, name, Kind::File, idx);
                tree.nodes[ci as usize].size = *bytes;
            }
        }
        for i in (1..tree.nodes.len()).rev() {
            let p = tree.nodes[i].parent as usize;
            let (sz, it) = (tree.nodes[i].size, tree.nodes[i].items);
            tree.nodes[p].size += sz;
            tree.nodes[p].items += it;
        }
        tree
    }

    /// Number of entries, root included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// True for a tree that was never scanned.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Bytes of heap this tree uses: nodes plus names. Room that was reserved for a scan and never written
    /// is not counted, because it holds no memory.
    pub fn heap_bytes(&self) -> usize {
        self.nodes.len() * std::mem::size_of::<Node>() + self.names.len()
    }

    /// The root.
    pub fn root(&self) -> NodeId {
        NodeId::ROOT
    }

    fn n(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    /// Allocated bytes of this entry, everything below it included.
    pub fn size(&self, id: NodeId) -> u64 {
        self.n(id).size
    }

    /// Number of files and links at or below this entry.
    pub fn items(&self, id: NodeId) -> u32 {
        self.n(id).items
    }

    /// What this entry is.
    pub fn kind(&self, id: NodeId) -> Kind {
        Kind::from_u8(self.n(id).kind)
    }

    /// The [`flags`] on this entry.
    pub fn flags(&self, id: NodeId) -> u8 {
        self.n(id).flags
    }

    /// The parent folder, `None` for the root.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        if id == NodeId::ROOT {
            None
        } else {
            Some(NodeId(self.n(id).parent))
        }
    }

    /// The name as stored (the full path for the root). Raw bytes: UTF-8 on macOS and Windows, anything
    /// on Linux.
    pub fn name_bytes(&self, id: NodeId) -> &[u8] {
        let n = self.n(id);
        &self.names[n.name_off as usize..n.name_off as usize + n.name_len as usize]
    }

    /// The name for display (lossy for bytes that are not text). A folded entry reads "42 small files".
    pub fn name(&self, id: NodeId) -> String {
        if self.kind(id) == Kind::Small {
            let n = self.items(id);
            return format!("{n} small file{}", if n == 1 { "" } else { "s" });
        }
        String::from_utf8_lossy(self.name_bytes(id)).into_owned()
    }

    /// The children of a folder, in storage order.
    pub fn children(&self, id: NodeId) -> impl ExactSizeIterator<Item = NodeId> + use<> {
        let n = self.n(id);
        (n.first_child..n.first_child + n.child_count).map(NodeId)
    }

    /// Number of direct children.
    pub fn child_count(&self, id: NodeId) -> usize {
        self.n(id).child_count as usize
    }

    /// The children of a folder, largest first (ties by name, so the order is stable).
    pub fn children_by_size(&self, id: NodeId) -> Vec<NodeId> {
        let mut v: Vec<NodeId> = self.children(id).collect();
        v.sort_by(|a, b| {
            self.size(*b)
                .cmp(&self.size(*a))
                .then_with(|| self.name_bytes(*a).cmp(self.name_bytes(*b)))
        });
        v
    }

    /// The full path of an entry. A folded "small files" entry has no path of its own: this is its folder.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let id = if self.kind(id) == Kind::Small {
            self.parent(id).unwrap_or(id)
        } else {
            id
        };
        let mut chain = Vec::new();
        let mut cur = id;
        loop {
            chain.push(cur);
            match self.parent(cur) {
                Some(p) => cur = p,
                None => break,
            }
        }
        let mut out = PathBuf::from(os_from_bytes(self.name_bytes(NodeId::ROOT)));
        for id in chain.iter().rev().skip(1) {
            out.push(os_from_bytes(self.name_bytes(*id)));
        }
        out
    }

    /// The `n` largest entries of one kind anywhere in the tree, largest first. One pass, a heap of `n`.
    pub fn largest(&self, n: usize, kind: Kind) -> Vec<NodeId> {
        self.largest_where(n, |node| node.kind == kind as u8)
    }

    fn largest_where(&self, n: usize, f: impl Fn(&Node) -> bool) -> Vec<NodeId> {
        if n == 0 {
            return Vec::new();
        }
        let mut heap: BinaryHeap<Reverse<(u64, u32)>> = BinaryHeap::with_capacity(n + 1);
        for (i, node) in self.nodes.iter().enumerate() {
            if !f(node) {
                continue;
            }
            if heap.len() < n {
                heap.push(Reverse((node.size, i as u32)));
            } else if let Some(Reverse((min, _))) = heap.peek()
                && node.size > *min
            {
                heap.pop();
                heap.push(Reverse((node.size, i as u32)));
            }
        }
        let mut v: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(x)| x).collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
        v.into_iter().map(|(_, i)| NodeId(i)).collect()
    }
}

#[cfg(unix)]
pub(crate) fn os_from_bytes(b: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(b.to_vec())
}

#[cfg(not(unix))]
pub(crate) fn os_from_bytes(b: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(b).into_owned())
}

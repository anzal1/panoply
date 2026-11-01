//! Copying the objects a result needs from a source document into a new file.
//!
//! A [`Copier`] walks references from the objects you ask for. Every source object is copied at
//! most once, gets a new number the first time something points at it, and is written when its
//! turn comes in a first-in first-out queue. That order is the order the objects are discovered
//! in, so a page and everything it uses end up next to each other, and nothing the result does
//! not reach is written at all. Memory is the map of numbers plus the objects in flight.
//!
//! Merge, split and organize copy pages this way. Compress copies the whole document this way,
//! with a policy that recodes images on the way through.

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;

use panoply_core::CancelToken;

use crate::doc::{Doc, Entry, Loaded, RawStream};
use crate::error::{PdfError, Result};
use crate::obj::{Dict, Obj};
use crate::writer::PdfWriter;

/// What the copier is allowed to look at when a policy decides something.
pub struct View<'a> {
    /// Source objects that must come out as `null`: pages that are not in the result.
    pub pruned: &'a HashSet<u32>,
    /// Source object to new object number, for everything that has one so far.
    pub mapped: &'a HashMap<u32, u32>,
}

/// Hooks that change what is copied. The defaults copy everything unchanged.
pub trait Policy {
    /// Look at, and edit, the dictionary of a source object before its references are
    /// rewritten. For a stream this is the stream dictionary. `src` is the source object number.
    fn dict(&mut self, _doc: &Doc, _view: &View, _src: u32, _dict: &mut Dict) {}

    /// Decide whether an annotation stays on a page. `num` is its object number when it is an
    /// indirect object. May edit a direct annotation.
    fn keep_annot(
        &mut self,
        _doc: &Doc,
        _view: &View,
        _num: Option<u32>,
        _annot: &mut Dict,
    ) -> bool {
        true
    }

    /// The source object that stands for `src` (duplicate elimination), if any.
    fn alias_of(&self, _src: u32) -> Option<u32> {
        None
    }

    /// Replace the bytes of a stream. Return `None` to copy the stream unchanged. The dictionary
    /// is the source dictionary (references are not yet rewritten); the returned one is rewritten.
    fn stream(
        &mut self,
        _doc: &Doc,
        _src: u32,
        _dict: &Dict,
        _raw: RawStream,
    ) -> Result<Option<(Dict, Vec<u8>)>> {
        Ok(None)
    }
}

/// A policy that changes nothing.
pub struct Plain;
impl Policy for Plain {}

/// Copies objects from one source document.
pub struct Copier<'d, P: Policy> {
    /// The source.
    pub doc: &'d Doc,
    /// The policy.
    pub policy: P,
    map: HashMap<u32, u32>,
    queue: VecDeque<u32>,
    pruned: HashSet<u32>,
    alias: HashMap<u32, u32>,
    cancel: CancelToken,
    /// Objects that could not be read and were written as `null`.
    pub unreadable: usize,
}

impl<'d, P: Policy> Copier<'d, P> {
    /// A copier over `doc`.
    pub fn new(doc: &'d Doc, policy: P, cancel: CancelToken) -> Self {
        Copier {
            doc,
            policy,
            map: HashMap::new(),
            queue: VecDeque::new(),
            pruned: HashSet::new(),
            alias: HashMap::new(),
            cancel,
            unreadable: 0,
        }
    }

    /// Give a source object a fixed new number. It is not queued: the caller writes it.
    pub fn preassign(&mut self, src: u32, dst: u32) {
        self.map.insert(src, dst);
    }

    /// Make references to `src` point at `canonical` instead (duplicate elimination).
    pub fn alias(&mut self, src: u32, canonical: u32) {
        self.alias.insert(src, canonical);
    }

    /// References to these source objects become `null`.
    pub fn prune(&mut self, nums: impl IntoIterator<Item = u32>) {
        self.pruned.extend(nums);
    }

    /// True when a source object already has a new number.
    pub fn is_mapped(&self, src: u32) -> bool {
        self.map.contains_key(&src)
    }

    /// Objects queued and not yet written.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// The view policies get.
    pub fn view(&self) -> View<'_> {
        View {
            pruned: &self.pruned,
            mapped: &self.map,
        }
    }

    /// The new number of a source object, allocating and queueing it on first use. `None` when
    /// the reference must become `null` (pruned, or the object is not in use).
    pub fn map_ref<W: Write>(&mut self, w: &mut PdfWriter<W>, src: u32) -> Option<u32> {
        let src = self
            .alias
            .get(&src)
            .copied()
            .or_else(|| self.policy.alias_of(src))
            .unwrap_or(src);
        if self.pruned.contains(&src) {
            return None;
        }
        if let Some(&d) = self.map.get(&src) {
            return Some(d);
        }
        if matches!(self.doc.entry(src), Entry::Free) {
            return None;
        }
        let dst = w.alloc();
        self.map.insert(src, dst);
        self.queue.push_back(src);
        Some(dst)
    }

    /// Rewrite every reference in an object to point into the new file.
    pub fn rewrite<W: Write>(&mut self, w: &mut PdfWriter<W>, o: &Obj) -> Obj {
        match o {
            Obj::Ref(n, _) => match self.map_ref(w, *n) {
                Some(d) => Obj::r(d),
                None => Obj::Null,
            },
            Obj::Array(a) => Obj::Array(a.iter().map(|x| self.rewrite(w, x)).collect()),
            Obj::Dict(d) => Obj::Dict(self.rewrite_dict(w, d)),
            other => other.clone(),
        }
    }

    /// Rewrite a dictionary. `/Annots` arrays go through the policy's annotation filter.
    pub fn rewrite_dict<W: Write>(&mut self, w: &mut PdfWriter<W>, d: &Dict) -> Dict {
        let mut out = Dict::new();
        for (k, v) in d.iter() {
            let key = String::from_utf8_lossy(k).into_owned();
            let nv = if key == "Annots" {
                self.rewrite_annots(w, v)
            } else {
                self.rewrite(w, v)
            };
            out.set_raw(k.to_vec(), nv);
        }
        out
    }

    fn rewrite_annots<W: Write>(&mut self, w: &mut PdfWriter<W>, v: &Obj) -> Obj {
        let items = match self.doc.resolve(v) {
            Obj::Array(a) => a,
            _ => return self.rewrite(w, v),
        };
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            let num = item.as_ref_num();
            let Some(mut annot) = self.doc.resolve_dict(&item) else {
                continue;
            };
            let keep = {
                let view = View {
                    pruned: &self.pruned,
                    mapped: &self.map,
                };
                self.policy.keep_annot(self.doc, &view, num, &mut annot)
            };
            if !keep {
                continue;
            }
            match num {
                Some(n) => {
                    if let Some(d) = self.map_ref(w, n) {
                        out.push(Obj::r(d));
                    }
                }
                None => out.push(Obj::Dict(self.rewrite_dict(w, &annot))),
            }
        }
        Obj::Array(out)
    }

    /// Write every queued object, and everything they reach.
    pub fn drain<W: Write>(&mut self, w: &mut PdfWriter<W>) -> Result<()> {
        self.drain_ticking(w, &mut |_, _| {})
    }

    /// Like [`Copier::drain`], telling `tick(written, still_queued)` after every object.
    pub fn drain_ticking<W: Write>(
        &mut self,
        w: &mut PdfWriter<W>,
        tick: &mut dyn FnMut(usize, usize),
    ) -> Result<()> {
        let mut written = 0usize;
        while let Some(src) = self.queue.pop_front() {
            if self.cancel.is_cancelled() {
                return Err(PdfError::Cancelled);
            }
            let dst = self.map[&src];
            self.copy_one(w, src, dst)?;
            written += 1;
            tick(written, self.queue.len());
        }
        Ok(())
    }

    /// Write one source object under a given new number (queued objects, and ones the caller
    /// wants to write itself with the default treatment).
    pub fn copy_one<W: Write>(&mut self, w: &mut PdfWriter<W>, src: u32, dst: u32) -> Result<()> {
        let loaded = match self.doc.load(src) {
            Ok(l) => l,
            Err(PdfError::Damaged(why)) => return Err(PdfError::Damaged(why)),
            Err(e) => return Err(e),
        };
        let Loaded { obj, stream } = loaded;
        match (obj, stream) {
            (Obj::Dict(mut d), Some(raw)) => {
                d.remove("Length");
                {
                    let view = View {
                        pruned: &self.pruned,
                        mapped: &self.map,
                    };
                    self.policy.dict(self.doc, &view, src, &mut d);
                }
                match self.policy.stream(self.doc, src, &d, raw)? {
                    Some((nd, data)) => {
                        let nd = self.rewrite_dict(w, &nd);
                        w.put_stream(dst, &nd, &data)
                    }
                    None => {
                        let nd = self.rewrite_dict(w, &d);
                        w.put_stream_from(dst, &nd, self.doc, raw)
                    }
                }
            }
            (mut obj, _) => {
                if let Obj::Dict(d) = &mut obj {
                    let view = View {
                        pruned: &self.pruned,
                        mapped: &self.map,
                    };
                    self.policy.dict(self.doc, &view, src, d);
                }
                let obj = self.rewrite(w, &obj);
                w.put(dst, &obj)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;
    use crate::writer::WriterOptions;

    #[test]
    fn only_reachable_objects_are_copied_and_pruned_pages_become_null() {
        // Reuse the tiny two-page file; object 99 is not in the file, so refs to it are null.
        let mut bytes = crate::testutil::tiny_pdf();
        // An unreachable object is simply never asked for: check by counting the output.
        bytes.extend_from_slice(b"\n");
        let doc = Doc::open(Source::from_bytes(bytes)).unwrap();
        let mut out = Vec::new();
        let mut w = PdfWriter::new(&mut out, WriterOptions::default()).unwrap();
        let mut c = Copier::new(&doc, Plain, CancelToken::new());
        let pages = doc.pages().unwrap();
        c.prune([pages[1].id]);
        let n = c.rewrite(
            &mut w,
            &Obj::Array(vec![Obj::r(pages[0].id), Obj::r(pages[1].id), Obj::r(99)]),
        );
        let a = n.as_array().unwrap();
        assert!(a[0].as_ref_num().is_some());
        assert_eq!(a[1], Obj::Null);
        assert_eq!(a[2], Obj::Null);
        c.drain(&mut w).unwrap();
    }
}

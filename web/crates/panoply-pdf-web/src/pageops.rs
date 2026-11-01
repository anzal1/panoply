//! Building a new PDF out of pages of existing ones: merge, split, extract, reorder, delete and
//! rotate are all the same operation with a different list of pages.
//!
//! Two modes decide what else comes along:
//!
//! - [`CatalogMode::Full`]: one source, every page kept (reorder, reverse, rotate). The whole
//!   catalog is copied, so bookmarks, named destinations, form fields, tags and metadata survive.
//! - [`CatalogMode::Subset`]: pages chosen from one or several sources (merge, split, extract,
//!   delete). Pages, their annotations and what they use are copied. Links keep working when
//!   their target page is in the result and are removed when it is not. Form fields on the kept
//!   pages are rebuilt into a fresh form (duplicate names across sources get `_2`, `_3`).
//!   Merge adds one bookmark per source file with that file's own bookmarks under it.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;

use panoply_core::CancelToken;

use crate::copy::{Copier, Policy, View};
use crate::dests::{DestPage, NameDests};
use crate::doc::{Doc, PageRef};
use crate::error::{PdfError, Result};
use crate::obj::{Dict, Obj};
use crate::writer::{PdfWriter, WriterOptions};

/// One source document with its pages listed.
pub struct Input<'a> {
    /// The document.
    pub doc: &'a Doc,
    /// Its pages in order, from [`Doc::pages`].
    pub pages: std::borrow::Cow<'a, [PageRef]>,
    /// Name used for this file's bookmark in a merge.
    pub title: String,
}

/// One page of the result.
#[derive(Debug, Clone, Copy)]
pub struct OutPage {
    /// Index into the inputs.
    pub input: usize,
    /// Zero-based page index in that input.
    pub index: usize,
    /// Extra clockwise rotation in degrees, a multiple of 90.
    pub rotate: i32,
}

/// What else is carried over besides the pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogMode {
    /// Copy the whole catalog. One input, every page kept exactly once.
    Full,
    /// Copy only the pages and what they use.
    Subset,
}

/// Settings for [`build`].
#[derive(Debug, Clone)]
pub struct BuildOptions {
    /// What to carry over.
    pub mode: CatalogMode,
    /// Merge: add a bookmark per input.
    pub bookmarks: bool,
    /// Full mode: drop `/PageLabels` (page order changed, so the labels would be wrong).
    pub drop_page_labels: bool,
    /// Which input's document information to keep.
    pub info_from: Option<usize>,
    /// Writer settings.
    pub writer: WriterOptions,
}

impl Default for BuildOptions {
    fn default() -> Self {
        BuildOptions {
            mode: CatalogMode::Subset,
            bookmarks: false,
            drop_page_labels: false,
            info_from: Some(0),
            writer: WriterOptions::default(),
        }
    }
}

/// What [`build`] did, for the result card.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuildReport {
    /// Pages written.
    pub pages: usize,
    /// Links removed because their target page is not in the result.
    pub links_removed: usize,
    /// Form fields kept (top-level fields in the new form).
    pub fields_kept: usize,
    /// Fields renamed to avoid a duplicate name.
    pub fields_renamed: usize,
    /// Bookmarks written by the merge.
    pub bookmarks: usize,
    /// Objects that could not be read and were written as `null`.
    pub unreadable: usize,
    /// Size of the file.
    pub bytes: u64,
}

struct Shared {
    names: HashSet<Vec<u8>>,
    renamed: usize,
}

struct PagePolicy {
    subset: bool,
    names: NameDests,
    keep: HashSet<u32>,
    roots: Vec<u32>,
    links_removed: usize,
    shared: Rc<RefCell<Shared>>,
}

impl PagePolicy {
    fn new(subset: bool, shared: Rc<RefCell<Shared>>) -> Self {
        PagePolicy {
            subset,
            names: NameDests::new(),
            keep: HashSet::new(),
            roots: Vec::new(),
            links_removed: 0,
            shared,
        }
    }

    /// Note a widget on a kept page and every field above it.
    fn register_widget(&mut self, doc: &Doc, num: u32) {
        let mut cur = num;
        let mut hops = 0;
        loop {
            self.keep.insert(cur);
            hops += 1;
            let parent = doc.load(cur).ok().and_then(|l| {
                l.obj
                    .as_dict()
                    .and_then(|d| d.get("Parent").and_then(Obj::as_ref_num))
            });
            match parent {
                Some(p) if hops < 64 && !self.keep.contains(&p) => cur = p,
                Some(_) if hops < 64 => return, // already known: its chain is registered
                _ => break,
            }
        }
        if !self.roots.contains(&cur) {
            self.roots.push(cur);
        }
    }
}

/// The page policy plus a caller's own, which sees everything second.
struct Composed<P: Policy> {
    page: PagePolicy,
    extra: P,
}

impl<P: Policy> Policy for Composed<P> {
    fn dict(&mut self, doc: &Doc, view: &View, src: u32, d: &mut Dict) {
        self.page.dict(doc, view, src, d);
        self.extra.dict(doc, view, src, d);
    }

    fn keep_annot(&mut self, doc: &Doc, view: &View, num: Option<u32>, annot: &mut Dict) -> bool {
        self.page.keep_annot(doc, view, num, annot) && self.extra.keep_annot(doc, view, num, annot)
    }

    fn alias_of(&self, src: u32) -> Option<u32> {
        self.extra.alias_of(src)
    }

    fn stream(
        &mut self,
        doc: &Doc,
        src: u32,
        dict: &Dict,
        raw: crate::doc::RawStream,
    ) -> Result<Option<(Dict, Vec<u8>)>> {
        self.extra.stream(doc, src, dict, raw)
    }
}

impl Policy for PagePolicy {
    fn keep_annot(&mut self, doc: &Doc, view: &View, num: Option<u32>, annot: &mut Dict) -> bool {
        match annot.name("Subtype") {
            Some(b"Widget") if self.subset => {
                if let Some(n) = num {
                    self.register_widget(doc, n);
                }
                true
            }
            Some(b"Link") => {
                let keep = match self.names.explicit(doc, annot) {
                    DestPage::None => true,
                    DestPage::Page(p) => !view.pruned.contains(&p),
                    DestPage::Unresolved => !self.subset,
                };
                if !keep {
                    self.links_removed += 1;
                }
                keep
            }
            _ => true,
        }
    }

    fn dict(&mut self, doc: &Doc, view: &View, src: u32, d: &mut Dict) {
        // Indirect link annotations: make their destination explicit.
        if d.name("Subtype") == Some(b"Link") {
            let _ = self.names.explicit(doc, d);
            return;
        }
        // Bookmarks: explicit destinations, and no destination when the page is gone.
        if d.has("Title") && d.has("Parent") && (d.has("Dest") || d.has("A")) {
            if let DestPage::Page(p) = self.names.explicit(doc, d)
                && view.pruned.contains(&p)
            {
                d.remove("Dest");
                d.remove("A");
            }
            return;
        }
        if !self.subset || !self.keep.contains(&src) {
            return;
        }
        // A field or widget in the rebuilt form: prune its kids to the ones that stayed.
        if let Some(kids) = d.get("Kids").and_then(Obj::as_array) {
            let kept: Vec<Obj> = kids
                .iter()
                .filter(|k| k.as_ref_num().is_some_and(|n| self.keep.contains(&n)))
                .cloned()
                .collect();
            d.set("Kids", Obj::Array(kept));
        }
        // A top-level field: keep names unique across merged files.
        if !d.has("Parent")
            && let Some(Obj::Str(t)) = d.get("T").cloned()
        {
            let mut sh = self.shared.borrow_mut();
            let mut name = t.clone();
            let mut n = 1;
            while sh.names.contains(&name) {
                n += 1;
                name = t.clone();
                name.extend_from_slice(format!("_{n}").as_bytes());
            }
            if name != t {
                sh.renamed += 1;
                d.set("T", Obj::Str(name.clone()));
            }
            sh.names.insert(name);
        }
    }
}

fn pages_per_node(n: usize) -> usize {
    if n <= 64 {
        n.max(1)
    } else {
        ((n as f64).sqrt().ceil() as usize).max(32)
    }
}

/// Build a new PDF from the listed pages and write it to `out`.
pub fn build<W: Write>(
    out: W,
    inputs: &[Input<'_>],
    pages: &[OutPage],
    opts: &BuildOptions,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
) -> Result<BuildReport> {
    build_with(
        out,
        inputs,
        pages,
        opts,
        cancel,
        progress,
        |_| crate::copy::Plain,
        &|_| {},
    )
    .map(|(r, _)| r)
}

/// [`build`] with a policy of the caller's (one per input, made by `make`) that also sees every
/// object, and an edit applied to the catalog in [`CatalogMode::Full`] before it is copied.
/// Returns the report and the policies, so the caller can read what they collected.
#[allow(clippy::too_many_arguments)]
pub fn build_with<W: Write, P: Policy>(
    out: W,
    inputs: &[Input<'_>],
    pages: &[OutPage],
    opts: &BuildOptions,
    cancel: &CancelToken,
    progress: &mut dyn FnMut(f32, &str),
    mut make: impl FnMut(usize) -> P,
    catalog_edit: &dyn Fn(&mut Dict),
) -> Result<(BuildReport, Vec<P>)> {
    if pages.is_empty() {
        return Err(PdfError::BadInput(
            "There are no pages to put in the result.".into(),
        ));
    }
    for p in pages {
        let inp = inputs
            .get(p.input)
            .ok_or_else(|| PdfError::BadInput("A page points at a missing file.".into()))?;
        if p.index >= inp.pages.len() {
            return Err(PdfError::BadInput(
                "A page number is past the end of its file.".into(),
            ));
        }
    }
    let subset = opts.mode == CatalogMode::Subset;
    if !subset && inputs.len() != 1 {
        return Err(PdfError::BadInput(
            "Keeping the whole catalog works on one file.".into(),
        ));
    }
    let mut w = PdfWriter::new(out, opts.writer)?;
    let shared = Rc::new(RefCell::new(Shared {
        names: HashSet::new(),
        renamed: 0,
    }));
    let mut copiers: Vec<Copier<'_, Composed<P>>> = inputs
        .iter()
        .enumerate()
        .map(|(k, i)| {
            Copier::new(
                i.doc,
                Composed {
                    page: PagePolicy::new(subset, shared.clone()),
                    extra: make(k),
                },
                cancel.clone(),
            )
        })
        .collect();

    // Numbers for the structure: catalog, page tree, page nodes, then one per output page.
    let catalog_id = w.alloc();
    let root_pages = w.alloc();
    let per_node = pages_per_node(pages.len());
    let node_count = if pages.len() <= 64 {
        0
    } else {
        pages.len().div_ceil(per_node)
    };
    let nodes: Vec<u32> = (0..node_count).map(|_| w.alloc()).collect();
    let page_ids: Vec<u32> = pages.iter().map(|_| w.alloc()).collect();

    // Which source pages are used, and which are not (those become `null` in links).
    let mut first_use: Vec<HashMap<usize, u32>> = vec![HashMap::new(); inputs.len()];
    for (i, p) in pages.iter().enumerate() {
        first_use[p.input].entry(p.index).or_insert(page_ids[i]);
    }
    for (k, c) in copiers.iter_mut().enumerate() {
        let mut pruned = Vec::new();
        for (idx, pr) in inputs[k].pages.iter().enumerate() {
            match first_use[k].get(&idx) {
                Some(&dst) => c.preassign(pr.id, dst),
                None => pruned.push(pr.id),
            }
        }
        c.prune(pruned);
    }

    // The bookmark of each file in a merge takes the place of that file's own bookmark root.
    let mut outline_items: Vec<(usize, u32)> = Vec::new();
    let outlines_id = (subset && opts.bookmarks).then(|| w.alloc());
    if outlines_id.is_some() {
        for (k, c) in copiers.iter_mut().enumerate() {
            if !first_use[k].is_empty() {
                let item = w.alloc();
                if let Some(src_root) = inputs[k]
                    .doc
                    .catalog()
                    .ok()
                    .and_then(|cat| cat.get("Outlines").and_then(Obj::as_ref_num))
                {
                    c.preassign(src_root, item);
                }
                outline_items.push((k, item));
            }
        }
    }

    // Write the pages.
    let n = pages.len();
    for (i, p) in pages.iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(PdfError::Cancelled);
        }
        let parent = if node_count == 0 {
            root_pages
        } else {
            nodes[i / per_node]
        };
        write_page(
            &mut w,
            &mut copiers[p.input],
            &inputs[p.input],
            p,
            page_ids[i],
            parent,
        )?;
        if i % 16 == 0 {
            progress(0.45 * (i as f32 / n as f32), "Writing pages");
        }
    }

    // Page tree.
    if node_count == 0 {
        let mut d = Dict::new();
        d.set("Type", Obj::name("Pages"));
        d.set(
            "Kids",
            Obj::Array(page_ids.iter().map(|&p| Obj::r(p)).collect()),
        );
        d.set("Count", Obj::Int(n as i64));
        w.put(root_pages, &Obj::Dict(d))?;
    } else {
        for (ni, &node) in nodes.iter().enumerate() {
            let lo = ni * per_node;
            let hi = ((ni + 1) * per_node).min(n);
            let mut d = Dict::new();
            d.set("Type", Obj::name("Pages"));
            d.set("Parent", Obj::r(root_pages));
            d.set(
                "Kids",
                Obj::Array(page_ids[lo..hi].iter().map(|&p| Obj::r(p)).collect()),
            );
            d.set("Count", Obj::Int((hi - lo) as i64));
            w.put(node, &Obj::Dict(d))?;
        }
        let mut d = Dict::new();
        d.set("Type", Obj::name("Pages"));
        d.set(
            "Kids",
            Obj::Array(nodes.iter().map(|&p| Obj::r(p)).collect()),
        );
        d.set("Count", Obj::Int(n as i64));
        w.put(root_pages, &Obj::Dict(d))?;
    }

    // Info.
    let info = match opts.info_from {
        Some(k) if k < inputs.len() => inputs[k]
            .doc
            .trailer()
            .get("Info")
            .and_then(Obj::as_ref_num)
            .and_then(|n| copiers[k].map_ref(&mut w, n)),
        _ => None,
    };

    // The catalog.
    let mut report = BuildReport {
        pages: n,
        ..Default::default()
    };
    let mut cat = Dict::new();
    if subset {
        cat.set("Type", Obj::name("Catalog"));
        cat.set("Pages", Obj::r(root_pages));
        if let Ok(src_cat) = inputs[0].doc.catalog()
            && let Some(lang) = src_cat.get("Lang")
        {
            cat.set("Lang", lang.clone());
        }
        // Form fields on the kept pages.
        let mut fields: Vec<Obj> = Vec::new();
        let mut dr = Dict::new();
        let mut need_appearances = false;
        let mut da: Option<Obj> = None;
        let mut q: Option<Obj> = None;
        for k in 0..copiers.len() {
            let roots = copiers[k].policy.page.roots.clone();
            if roots.is_empty() {
                continue;
            }
            for r in roots {
                if let Some(d) = copiers[k].map_ref(&mut w, r) {
                    fields.push(Obj::r(d));
                }
            }
            let acro = inputs[k].doc.catalog().ok().and_then(|c| {
                c.get("AcroForm")
                    .and_then(|a| inputs[k].doc.resolve_dict(a))
            });
            if let Some(acro) = acro {
                need_appearances |= matches!(
                    acro.get("NeedAppearances")
                        .map(|o| inputs[k].doc.resolve(o)),
                    Some(Obj::Bool(true))
                );
                if da.is_none() {
                    da = acro.get("DA").cloned();
                }
                if q.is_none() {
                    q = acro.get("Q").cloned();
                }
                if let Some(src_dr) = acro.get("DR").and_then(|d| inputs[k].doc.resolve_dict(d)) {
                    for (cat_name, v) in src_dr.iter() {
                        let key = String::from_utf8_lossy(cat_name).into_owned();
                        let sub = inputs[k].doc.resolve(v);
                        let mut target = match dr.get(&key) {
                            Some(Obj::Dict(d)) => d.clone(),
                            _ => Dict::new(),
                        };
                        match sub {
                            Obj::Dict(entries) => {
                                for (name, val) in entries.iter() {
                                    let nm = String::from_utf8_lossy(name).into_owned();
                                    if !target.has(&nm) {
                                        let nv = copiers[k].rewrite(&mut w, val);
                                        target.set(&nm, nv);
                                    }
                                }
                                dr.set(&key, Obj::Dict(target));
                            }
                            Obj::Array(_) if !dr.has(&key) => {
                                let nv = copiers[k].rewrite(&mut w, v);
                                dr.set(&key, nv);
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if !fields.is_empty() {
            report.fields_kept = fields.len();
            let mut af = Dict::new();
            af.set("Fields", Obj::Array(fields));
            if !dr.is_empty() {
                af.set("DR", Obj::Dict(dr));
            }
            if let Some(da) = da {
                // The default appearance string is plain text: no references to rewrite.
                af.set("DA", da);
            }
            if let Some(q) = q {
                af.set("Q", q);
            }
            if need_appearances {
                af.set("NeedAppearances", Obj::Bool(true));
            }
            cat.set("AcroForm", Obj::Dict(af));
        }
        // Bookmarks.
        if let Some(oid) = outlines_id {
            let total = write_outlines(
                &mut w,
                &mut copiers,
                inputs,
                &first_use,
                &page_ids,
                oid,
                &outline_items,
                &mut report,
            )?;
            let _ = total;
            cat.set("Outlines", Obj::r(oid));
            cat.set("PageMode", Obj::name("UseOutlines"));
        }
    } else {
        let mut src_cat = inputs[0].doc.catalog()?;
        src_cat.remove("Pages");
        if opts.drop_page_labels {
            src_cat.remove("PageLabels");
        }
        catalog_edit(&mut src_cat);
        cat = copiers[0].rewrite_dict(&mut w, &src_cat);
        // Set after the rewrite: this is already a number in the new file.
        cat.set("Pages", Obj::r(root_pages));
    }
    w.put(catalog_id, &Obj::Dict(cat))?;

    // Everything the pages and the catalog pointed at.
    let total = copiers.len();
    for (k, c) in copiers.iter_mut().enumerate() {
        c.drain(&mut w)?;
        progress(
            0.45 + 0.5 * ((k + 1) as f32 / total as f32),
            "Copying page content",
        );
    }
    for c in &copiers {
        report.links_removed += c.policy.page.links_removed;
        report.unreadable += c.unreadable;
    }
    report.fields_renamed = shared.borrow().renamed;
    let seed: Vec<u8> = pages
        .iter()
        .flat_map(|p| {
            [
                p.input as u8,
                (p.index & 0xff) as u8,
                ((p.index >> 8) & 0xff) as u8,
            ]
        })
        .chain(inputs.iter().flat_map(|i| i.doc.file_len().to_le_bytes()))
        .collect();
    report.bytes = w.finish(catalog_id, info, &seed)?;
    progress(1.0, "Done");
    let extras = copiers.into_iter().map(|c| c.policy.extra).collect();
    Ok((report, extras))
}

fn write_page<W: Write, P: Policy>(
    w: &mut PdfWriter<W>,
    c: &mut Copier<'_, Composed<P>>,
    input: &Input<'_>,
    p: &OutPage,
    dst: u32,
    parent: u32,
) -> Result<()> {
    let pr = &input.pages[p.index];
    let doc = input.doc;
    let mut d = match doc.load(pr.id)?.obj {
        Obj::Dict(d) => d,
        _ => return Err(PdfError::damaged("a page is not a dictionary")),
    };
    // Inherited attributes are written on the page, because its parent is not coming along.
    for (key, inh) in [
        ("Resources", &pr.inherited.resources),
        ("MediaBox", &pr.inherited.media_box),
        ("CropBox", &pr.inherited.crop_box),
    ] {
        if !d.has(key)
            && let Some(v) = inh
        {
            d.set(key, v.clone());
        }
    }
    let base = doc.page_geom(pr).rotate;
    let rot = (((base + p.rotate) % 360) + 360) % 360;
    if rot != 0 || d.has("Rotate") || pr.inherited.rotate.is_some() {
        d.set("Rotate", Obj::Int(i64::from(rot)));
    }
    d.remove("Parent");
    d.remove("B");
    if c.policy.page.subset {
        d.remove("StructParents");
    }
    d.set("Type", Obj::name("Page"));
    let mut out = c.rewrite_dict(w, &d);
    out.set("Parent", Obj::r(parent));
    w.put(dst, &Obj::Dict(out))
}

/// Write the outline root and one item per merged file. Returns the visible item count.
#[allow(clippy::too_many_arguments)]
fn write_outlines<W: Write, P: Policy>(
    w: &mut PdfWriter<W>,
    copiers: &mut [Copier<'_, Composed<P>>],
    inputs: &[Input<'_>],
    first_use: &[HashMap<usize, u32>],
    page_ids: &[u32],
    root: u32,
    items: &[(usize, u32)],
    report: &mut BuildReport,
) -> Result<i64> {
    let _ = page_ids;
    let mut visible = 0i64;
    for (pos, &(k, item)) in items.iter().enumerate() {
        let first_page = first_use[k]
            .iter()
            .min_by_key(|(idx, _)| **idx)
            .map(|(_, dst)| *dst);
        let mut d = Dict::new();
        d.set("Title", Obj::Str(inputs[k].title.clone().into_bytes()));
        d.set("Parent", Obj::r(root));
        if let Some(fp) = first_page {
            d.set(
                "Dest",
                Obj::Array(vec![
                    Obj::r(fp),
                    Obj::name("XYZ"),
                    Obj::Null,
                    Obj::Null,
                    Obj::Null,
                ]),
            );
        }
        if pos > 0 {
            d.set("Prev", Obj::r(items[pos - 1].1));
        }
        if pos + 1 < items.len() {
            d.set("Next", Obj::r(items[pos + 1].1));
        }
        // The file's own bookmarks hang under its item.
        let src_cat = inputs[k].doc.catalog().ok();
        let src_root = src_cat
            .as_ref()
            .and_then(|c| c.get("Outlines"))
            .and_then(|o| inputs[k].doc.resolve_dict(o));
        let mut count = 0i64;
        if let Some(sr) = src_root {
            for key in ["First", "Last"] {
                if let Some(v) = sr.get(key) {
                    let nv = copiers[k].rewrite(w, v);
                    if !matches!(nv, Obj::Null) {
                        d.set(key, nv);
                    }
                }
            }
            if d.has("First") {
                // Collapsed: the file's bookmarks are one click away, the list stays short.
                count = -sr.int("Count").unwrap_or(0).abs().max(1);
            }
        }
        if count != 0 {
            d.set("Count", Obj::Int(count));
        }
        visible += 1;
        w.put(item, &Obj::Dict(d))?;
        report.bookmarks += 1;
    }
    let mut r = Dict::new();
    r.set("Type", Obj::name("Outlines"));
    r.set("First", Obj::r(items[0].1));
    r.set("Last", Obj::r(items[items.len() - 1].1));
    r.set("Count", Obj::Int(visible));
    w.put(root, &Obj::Dict(r))?;
    Ok(visible)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;
    use crate::testutil::{TextPage, text_pdf};

    fn open(bytes: Vec<u8>) -> Doc {
        Doc::open(Source::from_bytes(bytes)).unwrap()
    }

    fn input<'a>(doc: &'a Doc, title: &str) -> Input<'a> {
        Input {
            doc,
            pages: doc.pages().unwrap().into(),
            title: title.into(),
        }
    }

    fn run(inputs: &[Input<'_>], pages: &[OutPage], opts: &BuildOptions) -> (Doc, BuildReport) {
        let mut out = Vec::new();
        let rep = build(
            &mut out,
            inputs,
            pages,
            opts,
            &CancelToken::new(),
            &mut |_, _| {},
        )
        .unwrap();
        (open(out), rep)
    }

    fn five() -> Doc {
        open(text_pdf(&[
            TextPage::new(200, 300, 0, "one"),
            TextPage::new(200, 300, 90, "two"),
            TextPage::new(300, 200, 0, "three"),
            TextPage::new(200, 300, 180, "four"),
            TextPage::new(100, 100, 0, "five"),
        ]))
    }

    fn texts(doc: &Doc) -> Vec<String> {
        doc.pages()
            .unwrap()
            .iter()
            .map(|p| {
                let d = doc.resolve_dict(&Obj::r(p.id)).unwrap();
                let c = String::from_utf8_lossy(&doc.page_content(&d)).into_owned();
                let a = c.find('(').unwrap() + 1;
                let b = c.find(')').unwrap();
                c[a..b].to_string()
            })
            .collect()
    }

    #[test]
    fn subset_picks_orders_and_repeats_pages() {
        let d = five();
        let inp = [input(&d, "five")];
        let pick = |i: usize| OutPage {
            input: 0,
            index: i,
            rotate: 0,
        };
        let (out, rep) = run(
            &inp,
            &[pick(4), pick(0), pick(0), pick(2)],
            &BuildOptions::default(),
        );
        assert_eq!(texts(&out), ["five", "one", "one", "three"]);
        assert_eq!(rep.pages, 4);
        // Sizes and rotation stay per page.
        let pages = out.pages().unwrap();
        assert_eq!(out.page_geom(&pages[0]).media, [0.0, 0.0, 100.0, 100.0]);
        assert_eq!(out.page_geom(&pages[3]).media, [0.0, 0.0, 300.0, 200.0]);
    }

    #[test]
    fn rotation_adds_to_the_existing_angle_and_wraps() {
        let d = five();
        let inp = [input(&d, "x")];
        let rot = |i, r| OutPage {
            input: 0,
            index: i,
            rotate: r,
        };
        let (out, _) = run(
            &inp,
            &[rot(0, 90), rot(1, 90), rot(3, 180), rot(4, 270)],
            &BuildOptions::default(),
        );
        let angles: Vec<i32> = out
            .pages()
            .unwrap()
            .iter()
            .map(|p| out.page_geom(p).rotate)
            .collect();
        assert_eq!(angles, [90, 180, 0, 270]);
    }

    #[test]
    fn merge_two_files_keeps_every_page_and_adds_bookmarks() {
        let a = open(text_pdf(&[
            TextPage::new(200, 200, 0, "a1"),
            TextPage::new(200, 200, 0, "a2"),
        ]));
        let b = open(text_pdf(&[TextPage::new(300, 300, 0, "b1")]));
        let inputs = [input(&a, "first"), input(&b, "second")];
        let mut plan = Vec::new();
        for (k, inp) in inputs.iter().enumerate() {
            for i in 0..inp.pages.len() {
                plan.push(OutPage {
                    input: k,
                    index: i,
                    rotate: 0,
                });
            }
        }
        let opts = BuildOptions {
            bookmarks: true,
            ..Default::default()
        };
        let (out, rep) = run(&inputs, &plan, &opts);
        assert_eq!(texts(&out), ["a1", "a2", "b1"]);
        assert_eq!(rep.bookmarks, 2);
        let cat = out.catalog().unwrap();
        let outlines = out.resolve_dict(cat.get("Outlines").unwrap()).unwrap();
        assert_eq!(outlines.int("Count"), Some(2));
        let first = out.resolve_dict(outlines.get("First").unwrap()).unwrap();
        assert_eq!(
            first.get("Title").unwrap().as_str_bytes().unwrap(),
            b"first"
        );
    }

    #[test]
    fn many_pages_get_a_two_level_tree() {
        let pages: Vec<TextPage> = (0..300)
            .map(|i| TextPage::new(100, 100, 0, &format!("p{i}")))
            .collect();
        let d = open(text_pdf(&pages));
        let inp = [input(&d, "x")];
        let plan: Vec<OutPage> = (0..300)
            .map(|i| OutPage {
                input: 0,
                index: i,
                rotate: 0,
            })
            .collect();
        let (out, _) = run(&inp, &plan, &BuildOptions::default());
        assert_eq!(out.pages().unwrap().len(), 300);
        let cat = out.catalog().unwrap();
        let root = out.resolve_dict(cat.get("Pages").unwrap()).unwrap();
        assert!(root.get("Kids").unwrap().as_array().unwrap().len() < 300);
        assert_eq!(texts(&out)[299], "p299");
    }

    #[test]
    fn full_mode_keeps_the_catalog_and_reorders() {
        // A file with a bookmark that points at page two.
        let mut b = crate::testutil::Builder::new();
        b.obj(
            1,
            "<< /Type /Catalog /Pages 2 0 R /Outlines 6 0 R /Lang (en) >>",
        );
        b.obj(
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 50 50] >>",
        );
        b.obj(3, "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>");
        b.obj(4, "<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>");
        b.stream(5, "", b"BT (x) Tj ET");
        b.obj(6, "<< /Type /Outlines /First 7 0 R /Last 7 0 R /Count 1 >>");
        b.obj(
            7,
            "<< /Title (to page two) /Parent 6 0 R /Dest [4 0 R /Fit] >>",
        );
        let d = open(b.build(1, ""));
        let inp = [input(&d, "x")];
        let plan = [
            OutPage {
                input: 0,
                index: 1,
                rotate: 0,
            },
            OutPage {
                input: 0,
                index: 0,
                rotate: 0,
            },
        ];
        let opts = BuildOptions {
            mode: CatalogMode::Full,
            ..Default::default()
        };
        let (out, _) = run(&inp, &plan, &opts);
        let cat = out.catalog().unwrap();
        assert_eq!(cat.get("Lang").unwrap().as_str_bytes().unwrap(), b"en");
        let outlines = out.resolve_dict(cat.get("Outlines").unwrap()).unwrap();
        let item = out.resolve_dict(outlines.get("First").unwrap()).unwrap();
        let dest = item.get("Dest").unwrap().as_array().unwrap();
        // The bookmark followed its page, which is now first.
        let pages = out.pages().unwrap();
        assert_eq!(dest[0].as_ref_num().unwrap(), pages[0].id);
    }

    #[test]
    fn links_to_removed_pages_are_dropped_and_others_survive() {
        let mut b = crate::testutil::Builder::new();
        b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
        b.obj(
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 50 50] >>",
        );
        b.obj(
            3,
            "<< /Type /Page /Parent 2 0 R /Annots [5 0 R 6 0 R 7 0 R] >>",
        );
        b.obj(4, "<< /Type /Page /Parent 2 0 R >>");
        b.obj(
            5,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 5 5] /Dest [4 0 R /Fit] >>",
        );
        b.obj(
            6,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 5 5] /Dest [3 0 R /Fit] >>",
        );
        b.obj(7, "<< /Type /Annot /Subtype /Link /Rect [0 0 5 5] /A << /S /URI /URI (https://example.com) >> >>");
        let d = open(b.build(1, ""));
        let inp = [input(&d, "x")];
        let (out, rep) = run(
            &inp,
            &[OutPage {
                input: 0,
                index: 0,
                rotate: 0,
            }],
            &BuildOptions::default(),
        );
        assert_eq!(rep.links_removed, 1);
        let p = out.pages().unwrap();
        let page = out.resolve_dict(&Obj::r(p[0].id)).unwrap();
        assert_eq!(page.get("Annots").unwrap().as_array().unwrap().len(), 2);
    }

    #[test]
    fn fields_are_rebuilt_and_duplicate_names_renamed_in_a_merge() {
        let form = |label: &str| {
            let mut b = crate::testutil::Builder::new();
            b.obj(1, "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] /DA (/Helv 0 Tf) >> >>");
            b.obj(
                2,
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 50 50] >>",
            );
            b.obj(3, "<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>");
            b.obj(
                5,
                &format!("<< /Type /Annot /Subtype /Widget /FT /Tx /T (name) /V ({label}) /Rect [0 0 10 10] /P 3 0 R >>"),
            );
            open(b.build(1, ""))
        };
        let a = form("A");
        let b = form("B");
        let inputs = [input(&a, "a"), input(&b, "b")];
        let plan = [
            OutPage {
                input: 0,
                index: 0,
                rotate: 0,
            },
            OutPage {
                input: 1,
                index: 0,
                rotate: 0,
            },
        ];
        let (out, rep) = run(&inputs, &plan, &BuildOptions::default());
        assert_eq!(rep.fields_kept, 2);
        assert_eq!(rep.fields_renamed, 1);
        let cat = out.catalog().unwrap();
        let acro = out.resolve_dict(cat.get("AcroForm").unwrap()).unwrap();
        let names: Vec<Vec<u8>> = acro
            .get("Fields")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                out.resolve_dict(f)
                    .unwrap()
                    .get("T")
                    .unwrap()
                    .as_str_bytes()
                    .unwrap()
                    .to_vec()
            })
            .collect();
        assert_eq!(names, [b"name".to_vec(), b"name_2".to_vec()]);
    }

    #[test]
    fn bad_plans_are_refused_plainly() {
        let d = five();
        let inp = [input(&d, "x")];
        let mut sink = Vec::new();
        let r = build(
            &mut sink,
            &inp,
            &[OutPage {
                input: 0,
                index: 99,
                rotate: 0,
            }],
            &BuildOptions::default(),
            &CancelToken::new(),
            &mut |_, _| {},
        );
        assert!(matches!(r, Err(PdfError::BadInput(_))));
        let r = build(
            &mut sink,
            &inp,
            &[],
            &BuildOptions::default(),
            &CancelToken::new(),
            &mut |_, _| {},
        );
        assert!(matches!(r, Err(PdfError::BadInput(_))));
    }

    #[test]
    fn cancel_stops_the_build() {
        let d = five();
        let inp = [input(&d, "x")];
        let c = CancelToken::new();
        c.cancel();
        let mut sink = Vec::new();
        let r = build(
            &mut sink,
            &inp,
            &[OutPage {
                input: 0,
                index: 0,
                rotate: 0,
            }],
            &BuildOptions::default(),
            &c,
            &mut |_, _| {},
        );
        assert!(matches!(r, Err(PdfError::Cancelled)));
    }
}

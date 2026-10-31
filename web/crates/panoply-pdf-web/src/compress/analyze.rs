//! What compression needs to know before it copies anything.
//!
//! One look over the document finds:
//!
//! - every image, with its stored size, and which images are the soft mask of another (those are
//!   only ever recoded without loss);
//! - the highest resolution each image is shown at, by following the content streams of every
//!   page (and the forms they draw) and tracking the current transformation matrix;
//! - streams and shared dictionaries (fonts, font descriptors, graphics states, encodings) that
//!   are exact duplicates of another object, so the copier writes each only once;
//! - how many bytes of embedded fonts there are, and how many belong to full fonts rather than
//!   subsets.
//!
//! Nothing here holds a stream longer than it takes to hash it or parse one page.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::rc::Rc;

use panoply_core::CancelToken;
use sha2::{Digest, Sha256};

use crate::doc::{Doc, Entry, Loaded, PageRef};
use crate::error::{PdfError, Result};
use crate::obj::{Dict, Lexer, Obj, Tok, to_bytes};

/// Streams under this size are not worth deduplicating.
const MIN_DEDUPE_BYTES: u64 = 128;
/// Deepest nesting of forms followed when measuring image resolution.
const MAX_FORM_DEPTH: usize = 8;
/// Most `Do` operators followed in one document, a guard against forms that draw each other.
const MAX_DO_OPS: usize = 400_000;
/// Decoded form content kept for reuse, in total.
const FORM_CACHE_BYTES: usize = 32 * 1024 * 1024;
/// Rounds of duplicate elimination (references to merged objects can merge their owners).
const DEDUPE_ROUNDS: usize = 5;

/// The result of looking at a document.
#[derive(Debug, Default)]
pub struct Analysis {
    /// Duplicate object to the object that stands for it.
    pub alias: HashMap<u32, u32>,
    /// Highest resolution each image is shown at, in pixels per inch (the limiting axis).
    pub dpi: HashMap<u32, f64>,
    /// Images that are the mask or soft mask of another image.
    pub smasks: HashSet<u32>,
    /// Every image stream: object number and stored size.
    pub images: Vec<(u32, u64)>,
    /// Bytes of embedded font programs.
    pub font_bytes: u64,
    /// Bytes of font programs that are the whole font, not a subset.
    pub full_font_bytes: u64,
}

/// Dictionary text and, for a stream, the hash of its bytes.
type DupKey = (Vec<u8>, Option<[u8; 32]>);

/// What kind of object is a candidate for duplicate elimination.
struct Cand {
    num: u32,
    dict: Dict,
    /// Stored length when the object is a stream.
    stream: Option<crate::doc::RawStream>,
}

/// Look at `doc`. `pages` is `doc.pages()`.
pub fn analyze(doc: &Doc, pages: &[PageRef], cancel: &CancelToken) -> Result<Analysis> {
    let mut a = Analysis::default();
    let cands = scan_objects(doc, &mut a, cancel)?;
    dedupe(doc, &cands, &mut a, cancel)?;
    measure_dpi(doc, pages, &mut a, cancel)?;
    merge_aliased(&mut a);
    Ok(a)
}

fn is_subset_name(name: &[u8]) -> bool {
    name.len() > 7 && name[6] == b'+' && name[..6].iter().all(u8::is_ascii_uppercase)
}

/// Object numbers in an order that reads each object stream once: plain objects by file offset,
/// then members of each object stream together.
fn scan_order(doc: &Doc) -> Vec<u32> {
    let mut plain: Vec<(u64, u32)> = Vec::new();
    let mut packed: Vec<(u32, u32, u32)> = Vec::new();
    for n in 0..doc.object_count() as u32 {
        match doc.entry(n) {
            Entry::InFile { offset, .. } => plain.push((offset, n)),
            Entry::InStream { stream, index } => packed.push((stream, index, n)),
            Entry::Free => {}
        }
    }
    plain.sort_unstable();
    packed.sort_unstable();
    plain
        .into_iter()
        .map(|p| p.1)
        .chain(packed.into_iter().map(|p| p.2))
        .collect()
}

fn scan_objects(doc: &Doc, a: &mut Analysis, cancel: &CancelToken) -> Result<Vec<Cand>> {
    let mut cands = Vec::new();
    let mut font_files: HashSet<u32> = HashSet::new();
    for (i, n) in scan_order(doc).into_iter().enumerate() {
        if i % 512 == 0 && cancel.is_cancelled() {
            return Err(PdfError::Cancelled);
        }
        let Ok(Loaded {
            obj: Obj::Dict(d),
            stream,
        }) = doc.load(n)
        else {
            continue;
        };
        let ty = d.name("Type").map(<[u8]>::to_vec);
        match stream {
            Some(raw) => {
                if matches!(ty.as_deref(), Some(b"XRef") | Some(b"ObjStm")) {
                    continue;
                }
                if d.name("Subtype") == Some(b"Image") {
                    a.images.push((n, raw.len));
                    for key in ["SMask", "Mask"] {
                        if let Some(m) = d.get(key).and_then(Obj::as_ref_num) {
                            a.smasks.insert(m);
                        }
                    }
                }
                if raw.len >= MIN_DEDUPE_BYTES {
                    cands.push(Cand {
                        num: n,
                        dict: d,
                        stream: Some(raw),
                    });
                }
            }
            None => match ty.as_deref() {
                Some(b"FontDescriptor") => {
                    let name = d.get("FontName").map(|o| doc.resolve(o));
                    let subset = name
                        .as_ref()
                        .and_then(Obj::as_name)
                        .is_some_and(is_subset_name);
                    for key in ["FontFile", "FontFile2", "FontFile3"] {
                        if let Some(f) = d.get(key).and_then(Obj::as_ref_num)
                            && font_files.insert(f)
                            && let Ok(Loaded {
                                stream: Some(raw), ..
                            }) = doc.load(f)
                        {
                            a.font_bytes += raw.len;
                            if !subset {
                                a.full_font_bytes += raw.len;
                            }
                        }
                    }
                    cands.push(Cand {
                        num: n,
                        dict: d,
                        stream: None,
                    });
                }
                Some(b"Font") | Some(b"ExtGState") | Some(b"Encoding") => {
                    cands.push(Cand {
                        num: n,
                        dict: d,
                        stream: None,
                    });
                }
                _ => {}
            },
        }
    }
    Ok(cands)
}

/// A writer that hashes.
struct HashSink(Sha256);
impl Write for HashSink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.update(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Follow `alias` to the end of the chain.
fn canon(alias: &HashMap<u32, u32>, mut n: u32) -> u32 {
    for _ in 0..16 {
        match alias.get(&n) {
            Some(&m) if m != n => n = m,
            _ => break,
        }
    }
    n
}

/// A copy of `o` with every reference replaced by its representative.
fn canon_obj(alias: &HashMap<u32, u32>, o: &Obj) -> Obj {
    match o {
        Obj::Ref(n, _) => Obj::Ref(canon(alias, *n), 0),
        Obj::Array(v) => Obj::Array(v.iter().map(|x| canon_obj(alias, x)).collect()),
        Obj::Dict(d) => {
            let mut out = Dict::new();
            for (k, v) in d.iter() {
                if k != b"Length" {
                    out.set_raw(k.to_vec(), canon_obj(alias, v));
                }
            }
            Obj::Dict(out)
        }
        other => other.clone(),
    }
}

fn dedupe(doc: &Doc, cands: &[Cand], a: &mut Analysis, cancel: &CancelToken) -> Result<()> {
    // Streams of the same length are the only ones that can match, so only those are hashed.
    let mut by_len: HashMap<u64, usize> = HashMap::new();
    for c in cands {
        if let Some(s) = c.stream {
            *by_len.entry(s.len).or_insert(0) += 1;
        }
    }
    let mut hashes: HashMap<u32, [u8; 32]> = HashMap::new();
    for _round in 0..DEDUPE_ROUNDS {
        let mut groups: BTreeMap<DupKey, Vec<u32>> = BTreeMap::new();
        for c in cands {
            if cancel.is_cancelled() {
                return Err(PdfError::Cancelled);
            }
            if a.alias.contains_key(&c.num) {
                continue;
            }
            let content = match c.stream {
                Some(raw) => {
                    if by_len.get(&raw.len).copied().unwrap_or(0) < 2 {
                        continue;
                    }
                    let h = match hashes.get(&c.num) {
                        Some(h) => *h,
                        None => {
                            let mut sink = HashSink(Sha256::new());
                            if doc.copy_raw(raw, &mut sink).is_err() {
                                continue;
                            }
                            let h: [u8; 32] = sink.0.finalize().into();
                            hashes.insert(c.num, h);
                            h
                        }
                    };
                    Some(h)
                }
                None => None,
            };
            let key = to_bytes(&canon_obj(&a.alias, &Obj::Dict(c.dict.clone())));
            groups.entry((key, content)).or_default().push(c.num);
        }
        let mut changed = false;
        for (_, mut nums) in groups {
            if nums.len() < 2 {
                continue;
            }
            nums.sort_unstable();
            let first = nums[0];
            for n in nums.into_iter().skip(1) {
                a.alias.insert(n, first);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    Ok(())
}

/// A duplicate image inherits the resolution and mask role of the image that stands for it, so
/// the representative is recoded for the most demanding use.
fn merge_aliased(a: &mut Analysis) {
    let pairs: Vec<(u32, u32)> = a
        .alias
        .iter()
        .map(|(&k, &v)| (k, canon(&a.alias, v)))
        .collect();
    for (dup, rep) in pairs {
        if let Some(d) = a.dpi.get(&dup).copied() {
            let e = a.dpi.entry(rep).or_insert(d);
            if d > *e {
                *e = d;
            }
        }
        if a.smasks.contains(&dup) {
            a.smasks.insert(rep);
        }
    }
    // The aliases point at representatives directly.
    let flat: Vec<(u32, u32)> = a.alias.keys().map(|&k| (k, canon(&a.alias, k))).collect();
    for (k, v) in flat {
        a.alias.insert(k, v);
    }
}

type Matrix = [f64; 6];

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// `m` applied first, then `n` (the PDF `cm` operator: CTM' = M x CTM).
fn mul(m: &Matrix, n: &Matrix) -> Matrix {
    [
        m[0] * n[0] + m[1] * n[2],
        m[0] * n[1] + m[1] * n[3],
        m[2] * n[0] + m[3] * n[2],
        m[2] * n[1] + m[3] * n[3],
        m[4] * n[0] + m[5] * n[2] + n[4],
        m[4] * n[1] + m[5] * n[3] + n[5],
    ]
}

struct Walker<'a> {
    doc: &'a Doc,
    dpi: HashMap<u32, f64>,
    dims: HashMap<u32, Option<(f64, f64)>>,
    forms: HashMap<u32, Option<Rc<Vec<u8>>>>,
    form_cache_bytes: usize,
    ops: usize,
}

impl Walker<'_> {
    fn image_dims(&mut self, n: u32, d: &Dict) -> Option<(f64, f64)> {
        *self.dims.entry(n).or_insert_with(|| {
            let w = d.get("Width").and_then(|o| self.doc.resolve_int(o))?;
            let h = d.get("Height").and_then(|o| self.doc.resolve_int(o))?;
            (w > 0 && h > 0).then_some((w as f64, h as f64))
        })
    }

    fn note_image(&mut self, n: u32, d: &Dict, ctm: &Matrix) {
        let Some((w, h)) = self.image_dims(n, d) else {
            return;
        };
        // The unit square maps to a parallelogram whose sides are the shown width and height.
        let sw = ctm[0].hypot(ctm[1]);
        let sh = ctm[2].hypot(ctm[3]);
        if sw < 1e-6 || sh < 1e-6 || !sw.is_finite() || !sh.is_finite() {
            return;
        }
        let eff = (w / (sw / 72.0)).min(h / (sh / 72.0));
        if eff.is_finite() {
            let e = self.dpi.entry(n).or_insert(eff);
            if eff > *e {
                *e = eff;
            }
        }
    }

    fn form_content(
        &mut self,
        n: u32,
        d: &Dict,
        raw: crate::doc::RawStream,
    ) -> Option<Rc<Vec<u8>>> {
        if let Some(c) = self.forms.get(&n) {
            return c.clone();
        }
        let data = self
            .doc
            .decode(d, raw, crate::doc::DECODE_LIMIT, true)
            .ok()
            .map(Rc::new);
        let keep = data.as_ref().is_some_and(|v| v.len() <= 1 << 20)
            && self.form_cache_bytes < FORM_CACHE_BYTES;
        if keep {
            self.form_cache_bytes += data.as_ref().map_or(0, |v| v.len());
            self.forms.insert(n, data.clone());
        }
        data
    }

    fn walk(&mut self, content: &[u8], res: &Dict, base: Matrix, depth: usize) {
        let doc = self.doc;
        let xobjects = res.get("XObject").and_then(|x| doc.resolve_dict(x));
        let mut lx = Lexer::content(content);
        let mut operands: Vec<Obj> = Vec::new();
        let mut ctm = base;
        let mut stack: Vec<Matrix> = Vec::new();
        loop {
            match lx.next_tok() {
                Ok(Tok::Obj(o)) => {
                    if operands.len() < 64 {
                        operands.push(o);
                    }
                }
                Ok(Tok::Word(w)) => {
                    match w {
                        b"q" => {
                            if stack.len() < 256 {
                                stack.push(ctm);
                            }
                        }
                        b"Q" => {
                            if let Some(m) = stack.pop() {
                                ctm = m;
                            }
                        }
                        b"cm" if operands.len() >= 6 => {
                            let v: Vec<f64> = operands[operands.len() - 6..]
                                .iter()
                                .filter_map(Obj::as_f64)
                                .collect();
                            if v.len() == 6 {
                                ctm = mul(&[v[0], v[1], v[2], v[3], v[4], v[5]], &ctm);
                            }
                        }
                        b"Do" => {
                            self.ops += 1;
                            if self.ops > MAX_DO_OPS {
                                return;
                            }
                            if let Some(name) = operands.last().and_then(Obj::as_name)
                                && let Some(xo) = &xobjects
                                && let Some(n) = xo
                                    .get(&String::from_utf8_lossy(name))
                                    .and_then(Obj::as_ref_num)
                            {
                                self.draw(n, &ctm, res, depth);
                            }
                        }
                        b"BI" => {
                            // Inline image: skip its dictionary and data.
                            skip_inline_image(&mut lx);
                        }
                        _ => {}
                    }
                    operands.clear();
                }
                Ok(Tok::End) => return,
                Err(_) => return,
            }
        }
    }

    fn draw(&mut self, n: u32, ctm: &Matrix, res: &Dict, depth: usize) {
        let Ok(Loaded {
            obj: Obj::Dict(d),
            stream: Some(raw),
        }) = self.doc.load(n)
        else {
            return;
        };
        match d.name("Subtype") {
            Some(b"Image") => self.note_image(n, &d, ctm),
            Some(b"Form") if depth < MAX_FORM_DEPTH => {
                let m = d
                    .get("Matrix")
                    .and_then(|m| self.doc.resolve_array(m))
                    .map(|a| {
                        a.iter()
                            .filter_map(|o| self.doc.resolve(o).as_f64())
                            .collect::<Vec<_>>()
                    })
                    .filter(|v| v.len() == 6)
                    .map_or(IDENTITY, |v| [v[0], v[1], v[2], v[3], v[4], v[5]]);
                let inner = mul(&m, ctm);
                let own_res = d.get("Resources").and_then(|r| self.doc.resolve_dict(r));
                let Some(content) = self.form_content(n, &d, raw) else {
                    return;
                };
                self.walk(&content, own_res.as_ref().unwrap_or(res), inner, depth + 1);
            }
            _ => {}
        }
    }
}

/// Step the lexer past an inline image: key/value pairs up to `ID`, then data up to `EI`.
fn skip_inline_image(lx: &mut Lexer<'_>) {
    loop {
        match lx.next_tok() {
            Ok(Tok::Word(b"ID")) => break,
            Ok(Tok::End) | Err(_) => return,
            _ => {}
        }
    }
    // One white space byte follows `ID`.
    let buf = lx.buf;
    let mut p = lx.pos + 1;
    while p + 1 < buf.len() {
        if buf[p] == b'E'
            && buf[p + 1] == b'I'
            && crate::obj::is_ws(buf[p - 1])
            && buf.get(p + 2).is_none_or(|&c| crate::obj::is_ws(c))
        {
            lx.pos = p + 2;
            return;
        }
        p += 1;
    }
    lx.pos = buf.len();
}

fn measure_dpi(doc: &Doc, pages: &[PageRef], a: &mut Analysis, cancel: &CancelToken) -> Result<()> {
    let mut w = Walker {
        doc,
        dpi: HashMap::new(),
        dims: HashMap::new(),
        forms: HashMap::new(),
        form_cache_bytes: 0,
        ops: 0,
    };
    for p in pages {
        if cancel.is_cancelled() {
            return Err(PdfError::Cancelled);
        }
        if w.ops > MAX_DO_OPS {
            break;
        }
        let Ok(Loaded {
            obj: Obj::Dict(page),
            ..
        }) = doc.load(p.id)
        else {
            continue;
        };
        let res = page
            .get("Resources")
            .or(p.inherited.resources.as_ref())
            .and_then(|r| doc.resolve_dict(r))
            .unwrap_or_default();
        let content = doc.page_content(&page);
        w.walk(&content, &res, IDENTITY, 0);
    }
    a.dpi = w.dpi;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;
    use crate::testutil::Builder;

    fn doc_with(ctm: &str, same_image: bool) -> Doc {
        let mut b = Builder::new();
        b.obj(1, "<< /Type /Catalog /Pages 2 0 R >>");
        b.obj(2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
        b.obj(
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /XObject << /A 5 0 R /B 6 0 R /F 8 0 R >> /Font << /F1 7 0 R /F2 9 0 R >> >> >>",
        );
        let content = format!(
            "q {ctm} cm /A Do Q q 100 0 0 100 0 0 cm /F Do Q BI /W 1 /H 1 /BPC 8 /CS /G ID \x00 EI Q"
        );
        b.stream(4, "", content.as_bytes());
        let px = vec![7u8; 600 * 600];
        let img = "/Type /XObject /Subtype /Image /Width 600 /Height 600 /ColorSpace /DeviceGray /BitsPerComponent 8";
        b.stream(5, img, &px);
        if same_image {
            b.stream(6, img, &px);
        } else {
            b.stream(6, img, &vec![9u8; 600 * 600]);
        }
        b.obj(7, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        b.stream(8, "/Type /XObject /Subtype /Form /BBox [0 0 1 1] /Matrix [0.5 0 0 0.5 0 0] /Resources << /XObject << /B 6 0 R >> >>", b"q 1 0 0 1 0 0 cm /B Do Q q 1 0 0 1 0 0 cm /B Do Q q 1 0 0 1 0 0 cm /B Do Q q 1 0 0 1 0 0 cm /B Do Q q 1 0 0 1 0 0 cm /B Do Q");
        b.obj(9, "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>");
        Doc::open(Source::from_bytes(b.build(1, ""))).unwrap()
    }

    #[test]
    fn dpi_follows_the_matrix_and_forms_and_duplicates_merge() {
        let doc = doc_with("200 0 0 200 0 0", true);
        let pages = doc.pages().unwrap();
        let a = analyze(&doc, &pages, &CancelToken::new()).unwrap();
        // 600 px over 200 pt (2.78 in) is 216 dpi. Image B is drawn by the form at 50 pt: 864 dpi.
        let rep = canon(&a.alias, 6);
        assert_eq!(rep, 5);
        let d5 = a.dpi[&5];
        assert!((d5 - 864.0).abs() < 1.0, "{d5}");
        // Both fonts are the same object after merging.
        assert_eq!(canon(&a.alias, 9), 7);
        assert_eq!(a.images.len(), 2);
    }

    #[test]
    fn different_images_are_not_merged() {
        let doc = doc_with("300 0 0 150 0 0", false);
        let pages = doc.pages().unwrap();
        let a = analyze(&doc, &pages, &CancelToken::new()).unwrap();
        assert!(!a.alias.contains_key(&6));
        // The limiting axis is the lower dpi: 600 px over 300 pt.
        assert!((a.dpi[&5] - 144.0).abs() < 1.0, "{}", a.dpi[&5]);
    }
}

//! A PDF opened lazily.
//!
//! Opening a [`Doc`] reads the end of the file, the cross-reference data and the trailer, and
//! nothing else. Objects are parsed on demand with positioned reads, stream bytes are read only
//! when asked for, and nothing is cached except recently used object streams (at most about
//! 24 MB of them). That is the memory
//! model of docs/specs/pdf.md ("a whole file is never loaded"): peak memory is the page tree and
//! the objects in flight, whatever the file size.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;

use crate::error::{PdfError, Result};
use crate::filters;
use crate::obj::{Dict, Lexer, Obj, ParseErr, Tok};
use crate::source::Source;

/// Largest decoded size of any one stream the engine will hold (object streams, page content,
/// raw images). Above it the stream is refused, which is what stops decompression bombs.
pub const DECODE_LIMIT: usize = 192 * 1024 * 1024;

/// Decoded object streams kept for reuse, in bytes (never fewer than four streams).
const OBJSTM_CACHE_BYTES: usize = 24 * 1024 * 1024;

/// Smallest and largest window read when parsing the head of an object.
const HEAD_WINDOW: usize = 8 * 1024;
const HEAD_WINDOW_MAX: usize = 128 * 1024 * 1024;

/// Where an object lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// Not in use.
    Free,
    /// At a byte offset in the file.
    InFile {
        /// Offset from the start of the header.
        offset: u64,
        /// Generation number.
        generation: u16,
    },
    /// Inside an object stream.
    InStream {
        /// Object number of the object stream.
        stream: u32,
        /// Index inside it.
        index: u32,
    },
}

/// Where the raw bytes of a stream are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawStream {
    /// Absolute offset in the source.
    pub offset: u64,
    /// Length in bytes.
    pub len: u64,
}

/// An indirect object as stored: its value, and for a stream the location of its bytes (the
/// value is then the stream dictionary).
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The object, or the dictionary of a stream.
    pub obj: Obj,
    /// The stream bytes, when this is a stream.
    pub stream: Option<RawStream>,
}

/// Attributes a page inherits from its ancestors in the page tree.
#[derive(Debug, Clone, Default)]
pub struct Inherited {
    /// `/Resources`.
    pub resources: Option<Obj>,
    /// `/MediaBox`.
    pub media_box: Option<Obj>,
    /// `/CropBox`.
    pub crop_box: Option<Obj>,
    /// `/Rotate`.
    pub rotate: Option<Obj>,
}

/// One leaf of the page tree.
#[derive(Debug, Clone)]
pub struct PageRef {
    /// Object number of the page.
    pub id: u32,
    /// Attributes inherited from the tree above it.
    pub inherited: Inherited,
}

/// Effective page geometry in points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageGeom {
    /// Media box `[x0 y0 x1 y1]`, normalised.
    pub media: [f64; 4],
    /// Crop box, intersected with the media box.
    pub crop: [f64; 4],
    /// Rotation, 0, 90, 180 or 270.
    pub rotate: i32,
}

impl PageGeom {
    /// Displayed width and height in points, after rotation.
    pub fn display_size(&self) -> (f64, f64) {
        let w = self.crop[2] - self.crop[0];
        let h = self.crop[3] - self.crop[1];
        if self.rotate % 180 == 0 {
            (w, h)
        } else {
            (h, w)
        }
    }
}

/// What the file says about encryption.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptInfo {
    /// Security handler version (`/V`).
    pub v: i64,
    /// Revision (`/R`).
    pub r: i64,
}

/// What the file says about digital signatures.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SignatureInfo {
    /// Signature fields that carry a signature.
    pub count: usize,
    /// True for a certified document (author signature with DocMDP).
    pub certified: bool,
}

struct ObjStm {
    num: u32,
    data: Vec<u8>,
    first: usize,
    /// (object number, offset relative to `first`).
    members: Vec<(u32, usize)>,
}

/// A PDF opened for reading.
pub struct Doc {
    src: Source,
    base: u64,
    entries: Vec<Entry>,
    trailer: Dict,
    version: (u8, u8),
    objstms: RefCell<VecDeque<ObjStm>>,
}

impl Doc {
    /// Open a PDF from a source. Reads the header, the tail and the cross-reference data only.
    /// A file the cross-reference data of which cannot be followed is [`PdfError::Damaged`], and
    /// [`crate::open`] repairs those.
    pub fn open(src: Source) -> Result<Doc> {
        let head = src.read_range(0, 1024)?;
        let hdr = find(&head, b"%PDF-").ok_or(PdfError::NotPdf)?;
        let version = parse_version(&head[hdr + 5..]);
        let base = hdr as u64;
        let (entries, trailer) = read_xref_chain(&src, base)?;
        let doc = Doc {
            src,
            base,
            entries,
            trailer,
            version,
            objstms: RefCell::new(VecDeque::new()),
        };
        // An encrypted file may keep its catalog in an encrypted object stream, which cannot be
        // read here. Hand it back as it is: callers refuse it with the plain password message.
        if doc.trailer.has("Encrypt") {
            return Ok(doc);
        }
        // The catalog and the page tree root must load, or this file needs repair.
        let root = doc
            .catalog()
            .map_err(|_| PdfError::damaged("the catalog is missing"))?;
        if root.get("Pages").is_none() {
            return Err(PdfError::damaged("the catalog has no page tree"));
        }
        let pages = doc.resolve(root.get("Pages").unwrap_or(&Obj::Null));
        if pages.as_dict().is_none() {
            return Err(PdfError::damaged("the page tree is missing"));
        }
        Ok(doc)
    }

    /// The file size in bytes.
    pub fn file_len(&self) -> u64 {
        self.src.len()
    }

    /// The `%PDF-x.y` version from the header.
    pub fn version(&self) -> (u8, u8) {
        self.version
    }

    /// The trailer dictionary.
    pub fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// Highest object number plus one.
    pub fn object_count(&self) -> usize {
        self.entries.len()
    }

    /// The cross-reference entry of an object.
    pub fn entry(&self, num: u32) -> Entry {
        self.entries
            .get(num as usize)
            .copied()
            .unwrap_or(Entry::Free)
    }

    /// Object numbers that are in use, in file order for plain objects and then those in object
    /// streams.
    pub fn used_objects(&self) -> Vec<u32> {
        let mut v: Vec<(u64, u32)> = Vec::new();
        for (i, e) in self.entries.iter().enumerate() {
            match e {
                Entry::InFile { offset, .. } => v.push((*offset, i as u32)),
                Entry::InStream { .. } => v.push((u64::MAX, i as u32)),
                Entry::Free => {}
            }
        }
        v.sort();
        v.into_iter().map(|(_, n)| n).collect()
    }

    /// Resolve a reference chain to a value. Missing objects are `null`. A stream resolves to its
    /// dictionary.
    pub fn resolve(&self, o: &Obj) -> Obj {
        let mut cur = o.clone();
        for _ in 0..32 {
            match cur {
                Obj::Ref(n, _) => match self.load(n) {
                    Ok(l) => cur = l.obj,
                    Err(_) => return Obj::Null,
                },
                other => return other,
            }
        }
        Obj::Null
    }

    /// Resolve and return the integer, if there is one.
    pub fn resolve_int(&self, o: &Obj) -> Option<i64> {
        self.resolve(o).as_i64()
    }

    /// Resolve to a dictionary (a stream gives its dictionary).
    pub fn resolve_dict(&self, o: &Obj) -> Option<Dict> {
        match self.resolve(o) {
            Obj::Dict(d) => Some(d),
            _ => None,
        }
    }

    /// Resolve to an array.
    pub fn resolve_array(&self, o: &Obj) -> Option<Vec<Obj>> {
        match self.resolve(o) {
            Obj::Array(a) => Some(a),
            _ => None,
        }
    }

    /// Load an indirect object.
    pub fn load(&self, num: u32) -> Result<Loaded> {
        self.load_depth(num, 0)
    }

    fn load_depth(&self, num: u32, depth: usize) -> Result<Loaded> {
        match self.entry(num) {
            Entry::Free => Ok(Loaded {
                obj: Obj::Null,
                stream: None,
            }),
            Entry::InFile { offset, .. } => {
                let (n, l) = self.read_indirect(self.base + offset, depth)?;
                if n != num {
                    return Err(PdfError::damaged(format!(
                        "object {num} is not where the cross-reference table says"
                    )));
                }
                Ok(l)
            }
            Entry::InStream { stream, index } => {
                let obj = self.object_in_stream(stream, index, num)?;
                Ok(Loaded { obj, stream: None })
            }
        }
    }

    /// Parse an indirect object at an absolute offset. Returns its number too.
    fn read_indirect(&self, abs: u64, depth: usize) -> Result<(u32, Loaded)> {
        let mut win = HEAD_WINDOW;
        loop {
            let buf = self.src.read_range(abs, win)?;
            let mut lx = Lexer::new(&buf);
            let (Some(num), Some(_gen)) = (lx.uint(), lx.uint()) else {
                return Err(PdfError::damaged("an object header is missing"));
            };
            if !lx.expect_word(b"obj") {
                return Err(PdfError::damaged("an object header is missing"));
            }
            let obj = match lx.object() {
                Ok(o) => o,
                Err(ParseErr::Eof) if buf.len() == win && win < HEAD_WINDOW_MAX => {
                    win *= 8;
                    continue;
                }
                Err(_) => return Err(PdfError::damaged("an object could not be read")),
            };
            // `endobj` directly, or the keyword `stream` for a stream.
            lx.skip_ws();
            let rest = &buf[lx.pos.min(buf.len())..];
            if matches!(obj, Obj::Dict(_)) && rest.starts_with(b"stream") {
                let mut p = lx.pos + 6;
                match buf.get(p) {
                    Some(b'\r') if buf.get(p + 1) == Some(&b'\n') => p += 2,
                    Some(b'\n') | Some(b'\r') => p += 1,
                    None if buf.len() == win && win < HEAD_WINDOW_MAX => {
                        win *= 8;
                        continue;
                    }
                    _ => {}
                }
                let start = abs + p as u64;
                let dict = obj.as_dict().cloned().unwrap_or_default();
                let len = self.stream_length(&dict, start, depth)?;
                return Ok((
                    num as u32,
                    Loaded {
                        obj,
                        stream: Some(RawStream { offset: start, len }),
                    },
                ));
            }
            return Ok((num as u32, Loaded { obj, stream: None }));
        }
    }

    /// The true length of a stream whose bytes start at `start`: the declared `/Length` when
    /// `endstream` follows it, otherwise found by searching for `endstream`.
    fn stream_length(&self, dict: &Dict, start: u64, depth: usize) -> Result<u64> {
        let declared = match dict.get("Length") {
            Some(Obj::Int(n)) if *n >= 0 => Some(*n as u64),
            Some(Obj::Ref(n, _)) if depth < 3 => match self.load_depth(*n, depth + 1) {
                Ok(Loaded {
                    obj: Obj::Int(v), ..
                }) if v >= 0 => Some(v as u64),
                _ => None,
            },
            _ => None,
        };
        if let Some(len) = declared
            && start + len <= self.src.len()
        {
            let tail = self.src.read_range(start + len, 32)?;
            let t = tail
                .iter()
                .position(|b| !crate::obj::is_ws(*b))
                .unwrap_or(0);
            if tail[t..].starts_with(b"endstream") || tail[t..].starts_with(b"endobj") {
                return Ok(len);
            }
        }
        // Search for the end marker, 1 MiB at a time.
        let mut pos = start;
        let total = self.src.len();
        let chunk = 1024 * 1024usize;
        while pos < total {
            let buf = self.src.read_range(pos, chunk + 9)?;
            if let Some(i) = find(&buf, b"endstream") {
                let mut end = pos + i as u64;
                // Drop the end-of-line that precedes the marker.
                let before = self.src.read_range(end.saturating_sub(2).max(start), 2)?;
                if before.ends_with(b"\r\n") {
                    end -= 2;
                } else if before.ends_with(b"\n") || before.ends_with(b"\r") {
                    end -= 1;
                }
                return Ok(end.saturating_sub(start));
            }
            if buf.len() <= 9 {
                break;
            }
            pos += chunk as u64;
        }
        // No end marker anywhere: a truncated file. The stream is whatever is left.
        Ok(total.saturating_sub(start))
    }

    fn object_in_stream(&self, stm: u32, index: u32, num: u32) -> Result<Obj> {
        let mut cache = self.objstms.borrow_mut();
        let pos = match cache.iter().position(|s| s.num == stm) {
            Some(p) => p,
            None => {
                drop(cache);
                let parsed = self.parse_objstm(stm)?;
                cache = self.objstms.borrow_mut();
                cache.push_front(parsed);
                // Copying visits objects in discovery order, which hops between object streams,
                // so keep as many decoded streams as fit a modest byte budget (at least four).
                let mut bytes = 0usize;
                let mut keep = 0usize;
                for s in cache.iter() {
                    bytes += s.data.len();
                    if keep >= 4 && bytes > OBJSTM_CACHE_BYTES {
                        break;
                    }
                    keep += 1;
                }
                cache.truncate(keep);
                0
            }
        };
        let s = &cache[pos];
        let (_, off) = s
            .members
            .iter()
            .find(|(n, _)| *n == num)
            .or_else(|| s.members.get(index as usize))
            .copied()
            .ok_or_else(|| PdfError::damaged("an object stream is missing a member"))?;
        let at = s.first + off;
        if at > s.data.len() {
            return Err(PdfError::damaged("an object stream is cut short"));
        }
        Lexer::new(&s.data[at..])
            .object()
            .map_err(|_| PdfError::damaged("an object inside an object stream is unreadable"))
    }

    fn parse_objstm(&self, stm: u32) -> Result<ObjStm> {
        let l = self.load(stm)?;
        let (Some(raw), Obj::Dict(d)) = (l.stream, &l.obj) else {
            return Err(PdfError::damaged("an object stream is missing"));
        };
        let n = self
            .resolve_int(d.get("N").unwrap_or(&Obj::Null))
            .unwrap_or(0)
            .clamp(0, 1 << 22) as usize;
        let first = self
            .resolve_int(d.get("First").unwrap_or(&Obj::Null))
            .unwrap_or(0)
            .max(0) as usize;
        let data = self.decode(d, raw, DECODE_LIMIT, true)?;
        let mut lx = Lexer::new(&data[..first.min(data.len())]);
        let mut members = Vec::with_capacity(n);
        for _ in 0..n {
            let (Some(a), Some(b)) = (lx.uint(), lx.uint()) else {
                break;
            };
            members.push((a as u32, b as usize));
        }
        Ok(ObjStm {
            num: stm,
            data,
            first,
            members,
        })
    }

    /// The filter chain of a stream dictionary with references resolved.
    pub fn filters(&self, dict: &Dict) -> Vec<(filters::Filter, Dict)> {
        filters::chain(dict, &|o| self.resolve(o))
    }

    /// Read the raw (still filtered) bytes of a stream.
    pub fn raw(&self, s: RawStream) -> Result<Vec<u8>> {
        if s.len > DECODE_LIMIT as u64 * 4 {
            return Err(PdfError::Unsupported(
                "A stream in this PDF is too large to hold in memory.".into(),
            ));
        }
        let v = self.src.read_range(s.offset, s.len as usize)?;
        if (v.len() as u64) < s.len {
            return Err(PdfError::damaged("a stream is cut short"));
        }
        Ok(v)
    }

    /// Copy the raw bytes of a stream to a writer in 64 KiB pieces.
    pub fn copy_raw(&self, s: RawStream, out: &mut dyn Write) -> Result<()> {
        let mut buf = vec![0u8; 64 * 1024];
        let mut done = 0u64;
        while done < s.len {
            let want = ((s.len - done) as usize).min(buf.len());
            let n = self.src.read_at(s.offset + done, &mut buf[..want])?;
            if n == 0 {
                return Err(PdfError::damaged("a stream is cut short"));
            }
            out.write_all(&buf[..n])?;
            done += n as u64;
        }
        Ok(())
    }

    /// Read and decode a stream through its lossless filters.
    pub fn decode(
        &self,
        dict: &Dict,
        s: RawStream,
        limit: usize,
        lenient: bool,
    ) -> Result<Vec<u8>> {
        let raw = self.raw(s)?;
        let chain = self.filters(dict);
        filters::decode(&chain, &raw, limit, lenient)
    }

    /// Load object `num` and decode its stream (lenient: a corrupt Flate stream gives a prefix).
    pub fn stream_data(&self, num: u32) -> Result<Option<(Dict, Vec<u8>)>> {
        let l = self.load(num)?;
        match (l.obj, l.stream) {
            (Obj::Dict(d), Some(s)) => {
                let data = self.decode(&d, s, DECODE_LIMIT, true)?;
                Ok(Some((d, data)))
            }
            _ => Ok(None),
        }
    }

    /// The document catalog.
    pub fn catalog(&self) -> Result<Dict> {
        let root = self
            .trailer
            .get("Root")
            .ok_or_else(|| PdfError::damaged("the trailer has no catalog"))?;
        self.resolve_dict(root)
            .ok_or_else(|| PdfError::damaged("the catalog is not a dictionary"))
    }

    /// Object number of the catalog.
    pub fn catalog_ref(&self) -> Option<u32> {
        self.trailer.get("Root").and_then(Obj::as_ref_num)
    }

    /// Encryption, if the file has any.
    pub fn encryption(&self) -> Option<EncryptInfo> {
        let e = self.trailer.get("Encrypt")?;
        let d = self.resolve_dict(e).unwrap_or_default();
        Some(EncryptInfo {
            v: d.int("V").unwrap_or(0),
            r: d.int("R").unwrap_or(0),
        })
    }

    /// Fail with the plain "password-protected" error when the file is encrypted.
    pub fn require_unencrypted(&self) -> Result<()> {
        if self.encryption().is_some() {
            Err(PdfError::Encrypted)
        } else {
            Ok(())
        }
    }

    /// Digital signatures in the file.
    pub fn signatures(&self) -> SignatureInfo {
        let mut info = SignatureInfo::default();
        let Ok(cat) = self.catalog() else {
            return info;
        };
        if let Some(perms) = cat.get("Perms").and_then(|p| self.resolve_dict(p))
            && perms.has("DocMDP")
        {
            info.certified = true;
        }
        let Some(acro) = cat.get("AcroForm").and_then(|a| self.resolve_dict(a)) else {
            return info;
        };
        let Some(fields) = acro.get("Fields").and_then(|f| self.resolve_array(f)) else {
            return info;
        };
        let mut stack: Vec<Obj> = fields;
        let mut seen = HashSet::new();
        let mut visited = 0;
        while let Some(f) = stack.pop() {
            visited += 1;
            if visited > 50_000 {
                break;
            }
            if let Obj::Ref(n, _) = f
                && !seen.insert(n)
            {
                continue;
            }
            let Some(d) = self.resolve_dict(&f) else {
                continue;
            };
            if d.name("FT") == Some(b"Sig") {
                if let Some(v) = d.get("V").and_then(|v| self.resolve_dict(v))
                    && (v.has("ByteRange") || v.has("Contents"))
                {
                    info.count += 1;
                }
            } else if let Some(kids) = d.get("Kids").and_then(|k| self.resolve_array(k)) {
                stack.extend(kids);
            }
            // Signature fields can also sit under a parent that carries /FT: check kids too.
            if d.name("FT") == Some(b"Sig")
                && let Some(kids) = d.get("Kids").and_then(|k| self.resolve_array(k))
            {
                stack.extend(kids);
            }
        }
        info
    }

    /// Every page in order, with inherited attributes.
    pub fn pages(&self) -> Result<Vec<PageRef>> {
        let cat = self.catalog()?;
        let root = cat
            .get("Pages")
            .cloned()
            .ok_or_else(|| PdfError::damaged("no page tree"))?;
        let mut out = Vec::new();
        let mut seen: HashSet<u32> = HashSet::new();
        // (node, inherited, depth)
        let mut stack: Vec<(Obj, Inherited, usize)> = vec![(root, Inherited::default(), 0)];
        while let Some((node, inh, depth)) = stack.pop() {
            let Some(num) = node.as_ref_num() else {
                continue;
            };
            if depth > 64 || !seen.insert(num) {
                continue;
            }
            let Ok(Loaded {
                obj: Obj::Dict(d), ..
            }) = self.load(num)
            else {
                continue;
            };
            let mut here = inh.clone();
            for (key, slot) in [
                ("Resources", &mut here.resources),
                ("MediaBox", &mut here.media_box),
                ("CropBox", &mut here.crop_box),
                ("Rotate", &mut here.rotate),
            ] {
                if let Some(v) = d.get(key) {
                    *slot = Some(v.clone());
                }
            }
            let kids = d.get("Kids").and_then(|k| self.resolve_array(k));
            let is_page = d.name("Type") == Some(b"Page") || kids.is_none();
            if is_page {
                out.push(PageRef {
                    id: num,
                    // A page's own values are on its dictionary: inherit only from above.
                    inherited: inh,
                });
                if out.len() > 10_000_000 {
                    break;
                }
            } else if let Some(kids) = kids {
                for k in kids.into_iter().rev() {
                    stack.push((k, here.clone(), depth + 1));
                }
            }
        }
        Ok(out)
    }

    /// The effective geometry of a page.
    pub fn page_geom(&self, p: &PageRef) -> PageGeom {
        let d = match self.load(p.id) {
            Ok(Loaded {
                obj: Obj::Dict(d), ..
            }) => d,
            _ => Dict::new(),
        };
        let pick = |key: &str, inh: &Option<Obj>| -> Option<[f64; 4]> {
            d.get(key).or(inh.as_ref()).and_then(|o| self.rect(o))
        };
        let media = pick("MediaBox", &p.inherited.media_box).unwrap_or([0.0, 0.0, 612.0, 792.0]);
        let crop = pick("CropBox", &p.inherited.crop_box)
            .map(|c| {
                [
                    c[0].max(media[0]),
                    c[1].max(media[1]),
                    c[2].min(media[2]),
                    c[3].min(media[3]),
                ]
            })
            .filter(|c| c[2] > c[0] && c[3] > c[1])
            .unwrap_or(media);
        let rot = d
            .get("Rotate")
            .or(p.inherited.rotate.as_ref())
            .and_then(|r| self.resolve(r).as_i64())
            .unwrap_or(0);
        PageGeom {
            media,
            crop,
            rotate: (((rot % 360) + 360) % 360 / 90 * 90) as i32,
        }
    }

    /// A rectangle array as `[x0 y0 x1 y1]`, normalised so x0 <= x1 and y0 <= y1.
    pub fn rect(&self, o: &Obj) -> Option<[f64; 4]> {
        let a = self.resolve_array(o)?;
        if a.len() < 4 {
            return None;
        }
        let mut v = [0.0; 4];
        for (i, x) in a.iter().take(4).enumerate() {
            v[i] = self.resolve(x).as_f64()?;
        }
        Some([
            v[0].min(v[2]),
            v[1].min(v[3]),
            v[0].max(v[2]),
            v[1].max(v[3]),
        ])
    }

    /// Stream bytes of a page: the concatenated, decoded `/Contents`.
    pub fn page_content(&self, page: &Dict) -> Vec<u8> {
        let mut out = Vec::new();
        let Some(c) = page.get("Contents") else {
            return out;
        };
        let parts: Vec<Obj> = match self.resolve(c) {
            Obj::Array(a) => a,
            _ => vec![c.clone()],
        };
        for p in parts {
            if let Some(n) = p.as_ref_num()
                && let Ok(Some((_, data))) = self.stream_data(n)
            {
                out.extend_from_slice(&data);
                out.push(b'\n');
                if out.len() > DECODE_LIMIT {
                    break;
                }
            }
        }
        out
    }

    /// A map of object number to the dictionary's `/Type`, for objects that are cheap to read.
    /// Not used on big files, only in tests and tools.
    #[doc(hidden)]
    pub fn debug_types(&self) -> HashMap<u32, String> {
        let mut m = HashMap::new();
        for n in self.used_objects() {
            if let Ok(Loaded {
                obj: Obj::Dict(d), ..
            }) = self.load(n)
                && let Some(t) = d.name("Type")
            {
                m.insert(n, String::from_utf8_lossy(t).into_owned());
            }
        }
        m
    }
}

// ---------------------------------------------------------------------------------------------
// Cross-reference reading

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn rfind(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).rposition(|w| w == needle)
}

fn parse_version(rest: &[u8]) -> (u8, u8) {
    let maj = rest
        .first()
        .filter(|b| b.is_ascii_digit())
        .map(|b| b - b'0');
    let min = rest.get(2).filter(|b| b.is_ascii_digit()).map(|b| b - b'0');
    (maj.unwrap_or(1), min.unwrap_or(4))
}

fn read_xref_chain(src: &Source, base: u64) -> Result<(Vec<Entry>, Dict)> {
    let len = src.len();
    let tail_len = 4096u64.min(len);
    let tail = src.read_range(len - tail_len, tail_len as usize)?;
    let at = rfind(&tail, b"startxref").ok_or_else(|| PdfError::damaged("startxref is missing"))?;
    let mut lx = Lexer::new(&tail[at + 9..]);
    let start = lx
        .uint()
        .ok_or_else(|| PdfError::damaged("startxref has no offset"))?;

    let mut entries: Vec<Option<Entry>> = Vec::new();
    let mut trailer = Dict::new();
    let mut queue: Vec<u64> = vec![start];
    let mut visited = HashSet::new();
    let mut first_trailer = true;
    while let Some(off) = queue.pop() {
        if !visited.insert(off) {
            continue;
        }
        if visited.len() > 4096 {
            return Err(PdfError::damaged("the cross-reference chain is too long"));
        }
        let abs = base + off;
        let head = src.read_range(abs, 32)?;
        let mut hl = Lexer::new(&head);
        hl.skip_ws();
        let (section_trailer, extra_stm) = if head[hl.pos.min(head.len())..].starts_with(b"xref") {
            let t = parse_xref_table(src, abs, &mut entries)?;
            let stm = t.get("XRefStm").and_then(Obj::as_i64);
            (t, stm)
        } else {
            (parse_xref_stream(src, abs, &mut entries)?, None)
        };
        // Older sections go last: the stack pops the newest first, so push /Prev first and a
        // hybrid /XRefStm after it so the stream is read before the older table.
        if let Some(prev) = section_trailer.get("Prev").and_then(Obj::as_i64)
            && prev >= 0
        {
            queue.push(prev as u64);
        }
        if let Some(stm) = extra_stm
            && stm >= 0
        {
            queue.push(stm as u64);
        }
        if first_trailer {
            trailer = section_trailer;
            first_trailer = false;
        } else {
            for key in ["Root", "Info", "Encrypt", "ID"] {
                if !trailer.has(key)
                    && let Some(v) = section_trailer.get(key)
                {
                    trailer.set(key, v.clone());
                }
            }
        }
    }
    if !trailer.has("Root") {
        return Err(PdfError::damaged("the trailer has no catalog"));
    }
    let entries = entries
        .into_iter()
        .map(|e| e.unwrap_or(Entry::Free))
        .collect();
    Ok((entries, trailer))
}

/// Record an entry unless a newer section already did.
fn put(entries: &mut Vec<Option<Entry>>, num: u64, e: Entry) -> Result<()> {
    if num > 32_000_000 {
        return Err(PdfError::damaged("the object count is not believable"));
    }
    let i = num as usize;
    if entries.len() <= i {
        entries.resize(i + 1, None);
    }
    if entries[i].is_none() {
        entries[i] = Some(e);
    }
    Ok(())
}

fn parse_xref_table(src: &Source, abs: u64, entries: &mut Vec<Option<Entry>>) -> Result<Dict> {
    // Read the section in growing windows until the trailer dictionary parses.
    let mut win = 64 * 1024usize;
    loop {
        let buf = src.read_range(abs, win)?;
        match parse_table_buf(&buf, entries) {
            Ok(Some(t)) => return Ok(t),
            Ok(None) | Err(PdfError::Damaged(_)) if buf.len() == win && win < 64 << 20 => {
                win *= 8;
            }
            Ok(None) => {
                return Err(PdfError::damaged(
                    "the cross-reference table has no trailer",
                ));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Parse `xref ... trailer << >>` from a buffer. `Ok(None)` when the buffer ended too early.
/// Entries are only recorded when the whole section parsed, so a retry never double counts.
fn parse_table_buf(buf: &[u8], entries: &mut Vec<Option<Entry>>) -> Result<Option<Dict>> {
    let mut lx = Lexer::new(buf);
    if !lx.expect_word(b"xref") {
        return Err(PdfError::damaged("the cross-reference table is missing"));
    }
    let mut found: Vec<(u64, Entry)> = Vec::new();
    loop {
        lx.skip_ws();
        if lx.expect_word(b"trailer") {
            let t = match lx.object() {
                Ok(Obj::Dict(d)) => d,
                Err(ParseErr::Eof) => return Ok(None),
                _ => return Err(PdfError::damaged("the trailer is unreadable")),
            };
            for (n, e) in found {
                put(entries, n, e)?;
            }
            return Ok(Some(t));
        }
        let (Some(first), Some(count)) = (lx.uint(), lx.uint()) else {
            return if lx.pos >= buf.len() {
                Ok(None)
            } else {
                Err(PdfError::damaged("the cross-reference table is unreadable"))
            };
        };
        if count > 32_000_000 {
            return Err(PdfError::damaged(
                "the cross-reference table is not believable",
            ));
        }
        for i in 0..count {
            let (Some(off), Some(generation)) = (lx.uint(), lx.uint()) else {
                return if lx.pos >= buf.len() {
                    Ok(None)
                } else {
                    Err(PdfError::damaged("the cross-reference table is unreadable"))
                };
            };
            let e = match lx.next_tok() {
                Ok(Tok::Word(b"n")) => Entry::InFile {
                    offset: off,
                    generation: generation.min(65535) as u16,
                },
                Ok(Tok::Word(b"f")) => Entry::Free,
                Ok(Tok::End) => return Ok(None),
                _ => return Err(PdfError::damaged("the cross-reference table is unreadable")),
            };
            found.push((first + i, e));
        }
    }
}

fn parse_xref_stream(src: &Source, abs: u64, entries: &mut Vec<Option<Entry>>) -> Result<Dict> {
    // Header and dictionary, then the stream bytes.
    let mut win = 16 * 1024usize;
    let (dict, data_start) = loop {
        let buf = src.read_range(abs, win)?;
        let mut lx = Lexer::new(&buf);
        let ok = lx.uint().is_some() && lx.uint().is_some() && lx.expect_word(b"obj");
        if !ok {
            return Err(PdfError::damaged("the cross-reference stream is missing"));
        }
        match lx.object() {
            Ok(Obj::Dict(d)) => {
                lx.skip_ws();
                if !buf[lx.pos.min(buf.len())..].starts_with(b"stream") {
                    return Err(PdfError::damaged("the cross-reference stream has no data"));
                }
                let mut p = lx.pos + 6;
                match buf.get(p) {
                    Some(b'\r') if buf.get(p + 1) == Some(&b'\n') => p += 2,
                    Some(b'\n') | Some(b'\r') => p += 1,
                    _ => {}
                }
                break (d, abs + p as u64);
            }
            Err(ParseErr::Eof) if buf.len() == win && win < 1 << 26 => win *= 8,
            _ => {
                return Err(PdfError::damaged(
                    "the cross-reference stream is unreadable",
                ));
            }
        }
    };
    if dict.name("Type") != Some(b"XRef") {
        return Err(PdfError::damaged(
            "the cross-reference stream has the wrong type",
        ));
    }
    // /Length must be direct in an xref stream.
    let len = dict
        .int("Length")
        .filter(|l| *l >= 0)
        .ok_or_else(|| PdfError::damaged("the cross-reference stream has no length"))?
        as u64;
    if len > 512 * 1024 * 1024 {
        return Err(PdfError::damaged(
            "the cross-reference stream is not believable",
        ));
    }
    let raw = src.read_range(data_start, len as usize)?;
    let chain = filters::chain(&dict, &|o| o.clone());
    let data = filters::decode(&chain, &raw, DECODE_LIMIT, true)?;
    let w: Vec<usize> = dict
        .get("W")
        .and_then(Obj::as_array)
        .map(|a| {
            a.iter()
                .map(|x| x.as_i64().unwrap_or(0).clamp(0, 8) as usize)
                .collect()
        })
        .unwrap_or_default();
    if w.len() < 3 {
        return Err(PdfError::damaged(
            "the cross-reference stream has no field widths",
        ));
    }
    let size = dict.int("Size").unwrap_or(0).max(0);
    let index: Vec<(u64, u64)> = match dict.get("Index").and_then(Obj::as_array) {
        Some(a) => a
            .chunks(2)
            .filter(|c| c.len() == 2)
            .map(|c| {
                (
                    c[0].as_i64().unwrap_or(0).max(0) as u64,
                    c[1].as_i64().unwrap_or(0).max(0) as u64,
                )
            })
            .collect(),
        None => vec![(0, size as u64)],
    };
    let row = w[0] + w[1] + w[2];
    if row == 0 {
        return Err(PdfError::damaged(
            "the cross-reference stream has empty rows",
        ));
    }
    let mut pos = 0usize;
    let field = |bytes: &[u8]| bytes.iter().fold(0u64, |a, &b| a << 8 | u64::from(b));
    for (first, count) in index {
        for i in 0..count {
            if pos + row > data.len() {
                break;
            }
            let r = &data[pos..pos + row];
            pos += row;
            let t = if w[0] == 0 { 1 } else { field(&r[..w[0]]) };
            let f2 = field(&r[w[0]..w[0] + w[1]]);
            let f3 = field(&r[w[0] + w[1]..]);
            let e = match t {
                0 => Entry::Free,
                1 => Entry::InFile {
                    offset: f2,
                    generation: f3.min(65535) as u16,
                },
                2 => Entry::InStream {
                    stream: f2 as u32,
                    index: f3 as u32,
                },
                _ => continue,
            };
            put(entries, first + i, e)?;
        }
    }
    Ok(dict)
}

// ---------------------------------------------------------------------------------------------
// Opening with repair

/// Files up to this size are repaired in memory when their cross-reference data is broken.
pub const REPAIR_LIMIT: u64 = 64 * 1024 * 1024;

/// A document and whether it had to be repaired to open.
pub struct Opened {
    /// The document.
    pub doc: Doc,
    /// True when the file was damaged and was rebuilt in memory (the file on disk is untouched).
    pub repaired: bool,
}

/// Open a file as a [`Doc`]. A file with broken cross-reference data is repaired in memory: the
/// engine first scans the file for objects (any size), then, up to [`REPAIR_LIMIT`], hands it to
/// lopdf and then pdfium. The file on disk is never changed.
pub fn open(file: &std::fs::File) -> Result<Opened> {
    let src = Source::from_file(file.try_clone()?)?;
    match Doc::open(src) {
        Ok(doc) => Ok(Opened {
            doc,
            repaired: false,
        }),
        Err(PdfError::Damaged(why)) => repair(file, &why),
        Err(e) => Err(e),
    }
}

/// Rebuild a damaged file in memory. Tries lopdf first, which scans the file for objects when
/// the cross-reference data is wrong, then pdfium, which also recovers files that end early.
pub fn repair(file: &std::fs::File, why: &str) -> Result<Opened> {
    let len = file.metadata()?.len();
    if mentions_encrypt(file)? {
        return Err(PdfError::Encrypted);
    }
    match reconstruct(Source::from_file(file.try_clone()?)?) {
        Ok(doc) => {
            return Ok(Opened {
                doc,
                repaired: true,
            });
        }
        Err(PdfError::Encrypted) => return Err(PdfError::Encrypted),
        Err(_) => {}
    }
    if len > REPAIR_LIMIT {
        return Err(PdfError::DamagedTooLarge);
    }
    let src = Source::from_file(file.try_clone()?)?;
    let bytes = src.read_range(0, len as usize)?;
    let first = repair_with_lopdf(&bytes);
    drop(bytes);
    let doc = match first {
        Ok(doc) => doc,
        Err(PdfError::Encrypted) => return Err(PdfError::Encrypted),
        Err(e1) => match repair_with_pdfium(file) {
            Ok(doc) => doc,
            Err(PdfError::Encrypted) => return Err(PdfError::Encrypted),
            Err(e2) => {
                // Name the first reason, which is about the file; the second may be about the engine.
                let _ = e2;
                return Err(PdfError::damaged(format!("{why}; {}", reason(&e1))));
            }
        },
    };
    Ok(Opened {
        doc,
        repaired: true,
    })
}

/// Rebuild the cross-reference data of a damaged file by scanning it for `n g obj`. Works on a
/// file of any size, one megabyte at a time.
pub fn reconstruct(src: Source) -> Result<Doc> {
    let head = src.read_range(0, 1024)?;
    if find(&head, b"%PDF-").is_none() {
        return Err(PdfError::NotPdf);
    }
    let version = parse_version(&head[find(&head, b"%PDF-").unwrap_or(0) + 5..]);
    let total = src.len();
    // Pass 1: object headers and trailer keywords.
    let mut found: std::collections::BTreeMap<u32, u64> = std::collections::BTreeMap::new();
    let mut trailers: Vec<u64> = Vec::new();
    let chunk = 1024 * 1024usize;
    let overlap = 48usize;
    let mut pos = 0u64;
    while pos < total {
        // Re-read the end of the previous chunk so headers across the boundary are seen once.
        let start = pos.saturating_sub(overlap as u64);
        let buf = src.read_range(start, chunk + overlap)?;
        let skip = (pos - start) as usize;
        let mut i = skip.saturating_sub(2);
        while i + 3 <= buf.len() {
            if buf[i] == b'o' && buf[i + 1] == b'b' && buf[i + 2] == b'j' {
                let after_ok = buf.get(i + 3).is_none_or(|c| !c.is_ascii_alphanumeric());
                if after_ok && let Some((num, off_in_buf)) = header_before(&buf, i) {
                    // The overlap is scanned twice, which records the same offset twice.
                    found.insert(num, start + off_in_buf as u64);
                }
                i += 3;
            } else if buf[i] == b't' && buf[i..].starts_with(b"trailer") {
                trailers.push(start + i as u64);
                i += 7;
            } else {
                i += 1;
            }
        }
        pos += chunk as u64;
    }
    trailers.sort_unstable();
    trailers.dedup();
    if found.is_empty() {
        return Err(PdfError::damaged("no objects could be found"));
    }
    let max = *found.keys().next_back().unwrap_or(&0) as usize;
    if max > 32_000_000 {
        return Err(PdfError::damaged("the object count is not believable"));
    }
    let mut entries = vec![Entry::Free; max + 1];
    for (&n, &off) in &found {
        entries[n as usize] = Entry::InFile {
            offset: off,
            generation: 0,
        };
    }
    let mut doc = Doc {
        src,
        base: 0,
        entries,
        trailer: Dict::new(),
        version,
        objstms: RefCell::new(VecDeque::new()),
    };
    // Trailer: the last one that names a catalog, else an xref stream's dictionary, else any
    // catalog object.
    let mut trailer = Dict::new();
    for &t in trailers.iter().rev() {
        let buf = doc.src.read_range(t + 7, 16 * 1024)?;
        if let Ok(Obj::Dict(d)) = Lexer::new(&buf).object()
            && d.has("Root")
        {
            if trailer.is_empty() {
                trailer = d;
            } else {
                for key in ["Info", "Encrypt", "ID"] {
                    if !trailer.has(key)
                        && let Some(v) = d.get(key)
                    {
                        trailer.set(key, v.clone());
                    }
                }
            }
        }
    }
    let mut objstms: Vec<u32> = Vec::new();
    let mut catalog: Option<u32> = None;
    if found.len() <= 2_000_000 {
        for (&n, &off) in found.iter().rev() {
            // Cheap look at the start of each object for the three kinds that matter here.
            let h = doc.src.read_range(off, 400)?;
            let has = |needle: &[u8]| find(&h, needle).is_some();
            if has(b"/ObjStm") {
                objstms.push(n);
            }
            if trailer.is_empty()
                && has(b"/XRef")
                && let Ok((_, l)) = doc.read_indirect(off, 0)
                && let Obj::Dict(d) = &l.obj
                && d.name("Type") == Some(b"XRef")
                && d.has("Root")
            {
                trailer = d.clone();
            }
            if catalog.is_none()
                && has(b"/Catalog")
                && let Ok((_, l)) = doc.read_indirect(off, 0)
                && let Obj::Dict(d) = &l.obj
                && d.name("Type") == Some(b"Catalog")
            {
                catalog = Some(n);
            }
        }
    }
    // Objects that live inside object streams are only visible once those streams are read.
    for stm in objstms {
        if let Ok(o) = doc.parse_objstm(stm) {
            for (i, (n, _)) in o.members.iter().enumerate() {
                let slot = *n as usize;
                if slot > 32_000_000 {
                    continue;
                }
                if doc.entries.len() <= slot {
                    doc.entries.resize(slot + 1, Entry::Free);
                }
                if matches!(doc.entries[slot], Entry::Free) {
                    doc.entries[slot] = Entry::InStream {
                        stream: stm,
                        index: i as u32,
                    };
                }
            }
            if trailer.is_empty() {
                // The catalog may be inside an object stream.
                for (n, _) in &o.members {
                    if let Ok(Obj::Dict(d)) = doc.load(*n).map(|l| l.obj)
                        && d.name("Type") == Some(b"Catalog")
                    {
                        catalog = Some(*n);
                    }
                }
            }
        }
    }
    if !trailer.has("Root") {
        let root = catalog.ok_or_else(|| PdfError::damaged("no catalog could be found"))?;
        trailer.set("Root", Obj::r(root));
    }
    doc.trailer = trailer;
    if doc.trailer.has("Encrypt") {
        return Ok(doc);
    }
    // The result must be usable: a catalog with a page tree.
    let cat = doc
        .catalog()
        .map_err(|_| PdfError::damaged("the catalog could not be read"))?;
    if doc
        .resolve_dict(cat.get("Pages").unwrap_or(&Obj::Null))
        .is_none()
    {
        return Err(PdfError::damaged("the page tree could not be found"));
    }
    Ok(doc)
}

/// If `buf[at..]` is the `obj` of `<num> <gen> obj` at the start of a line or after a token,
/// return the object number and the offset in `buf` where the number starts.
fn header_before(buf: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut j = at;
    // whitespace between the generation and `obj`
    while j > 0 && crate::obj::is_ws(buf[j - 1]) {
        j -= 1;
    }
    let gen_end = j;
    while j > 0 && buf[j - 1].is_ascii_digit() {
        j -= 1;
    }
    if j == gen_end || gen_end - j > 5 {
        return None;
    }
    let ws2_end = j;
    while j > 0 && crate::obj::is_ws(buf[j - 1]) {
        j -= 1;
    }
    if j == ws2_end {
        return None;
    }
    let num_end = j;
    while j > 0 && buf[j - 1].is_ascii_digit() {
        j -= 1;
    }
    if j == num_end || num_end - j > 10 {
        return None;
    }
    // The number must start a token: start of data, or after white space, or after `>`/`]`.
    if j > 0 && !crate::obj::is_ws(buf[j - 1]) && !matches!(buf[j - 1], b'>' | b']' | b')') {
        return None;
    }
    let num: u64 = std::str::from_utf8(&buf[j..num_end]).ok()?.parse().ok()?;
    (num < u64::from(u32::MAX)).then_some((num as u32, j))
}

/// True when `/Encrypt` appears near the start or the end of the file, where trailers live. A
/// damaged encrypted file must never be "repaired" into an unprotected copy.
fn mentions_encrypt(file: &std::fs::File) -> Result<bool> {
    let src = Source::from_file(file.try_clone()?)?;
    let len = src.len();
    let head = src.read_range(0, 64 * 1024)?;
    let tail_len = (64 * 1024u64).min(len);
    let tail = src.read_range(len - tail_len, tail_len as usize)?;
    Ok(find(&head, b"/Encrypt").is_some() || find(&tail, b"/Encrypt").is_some())
}

fn reason(e: &PdfError) -> String {
    match e {
        PdfError::Damaged(w) => w.clone(),
        other => other.to_string(),
    }
}

fn repair_with_lopdf(bytes: &[u8]) -> Result<Doc> {
    let mut lo = lopdf::Document::load_mem(bytes).map_err(|e| PdfError::damaged(e.to_string()))?;
    if lo.is_encrypted() {
        return Err(PdfError::Encrypted);
    }
    let mut out = Vec::with_capacity(bytes.len());
    lo.save_to(&mut out)
        .map_err(|e| PdfError::damaged(e.to_string()))?;
    Doc::open(Source::from_bytes(out))
}

fn repair_with_pdfium(file: &std::fs::File) -> Result<Doc> {
    let engine = crate::pdfium::engine()?;
    let _guard = crate::pdfium::lock();
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (engine, file);
        return Err(PdfError::damaged("pdfium repair is not available in the browser build"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let pd = engine
            .load_pdf_from_reader(file.try_clone()?, None)
            .map_err(|e| match e {
                pdfium_render::prelude::PdfiumError::PdfiumLibraryInternalError(
                    pdfium_render::prelude::PdfiumInternalError::PasswordError,
                ) => PdfError::Encrypted,
                other => PdfError::damaged(format!("pdfium: {other:?}")),
            })?;
        let mut out = Vec::new();
        pd.save_to_writer(&mut out)
            .map_err(|e| PdfError::damaged(format!("pdfium could not rewrite the file ({e:?})")))?;
        Doc::open(Source::from_bytes(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::testutil::tiny_pdf;

    #[test]
    fn opens_pages_and_inherits() {
        let doc = Doc::open(Source::from_bytes(tiny_pdf())).unwrap();
        let pages = doc.pages().unwrap();
        assert_eq!(pages.len(), 2);
        let g0 = doc.page_geom(&pages[0]);
        assert_eq!(g0.media, [0.0, 0.0, 300.0, 400.0]);
        assert_eq!(g0.rotate, 90);
        assert_eq!(g0.display_size(), (400.0, 300.0));
        let g1 = doc.page_geom(&pages[1]);
        assert_eq!(g1.media, [0.0, 0.0, 100.0, 200.0]);
        assert_eq!(g1.rotate, 0);
    }

    #[test]
    fn stream_length_and_content() {
        let doc = Doc::open(Source::from_bytes(tiny_pdf())).unwrap();
        let (d, data) = doc.stream_data(5).unwrap().unwrap();
        assert_eq!(d.int("Length"), Some(4));
        assert_eq!(data, b"q Q ");
    }

    #[test]
    fn wrong_length_is_recovered_by_searching_endstream() {
        let mut bytes = tiny_pdf();
        let at = bytes.windows(9).position(|w| w == b"/Length 4").unwrap();
        bytes[at + 8] = b'9'; // same width, so every offset in the table stays valid
        let doc = Doc::open(Source::from_bytes(bytes)).unwrap();
        let (_, data) = doc.stream_data(5).unwrap().unwrap();
        assert_eq!(data, b"q Q ");
    }

    #[test]
    fn not_a_pdf_and_missing_xref() {
        assert!(matches!(
            Doc::open(Source::from_bytes(b"hello".to_vec())),
            Err(PdfError::NotPdf)
        ));
        let cut = tiny_pdf();
        let cut = &cut[..cut.len() / 2];
        assert!(matches!(
            Doc::open(Source::from_bytes(cut.to_vec())),
            Err(PdfError::Damaged(_))
        ));
    }

    #[test]
    fn no_encryption_and_no_signatures_in_a_plain_file() {
        let doc = Doc::open(Source::from_bytes(tiny_pdf())).unwrap();
        assert!(doc.encryption().is_none());
        assert_eq!(doc.signatures(), SignatureInfo::default());
        doc.require_unencrypted().unwrap();
    }

    #[test]
    fn junk_before_the_header_shifts_offsets() {
        let mut bytes = b"Received: by example\r\n".to_vec();
        bytes.extend_from_slice(&tiny_pdf());
        let doc = Doc::open(Source::from_bytes(bytes)).unwrap();
        assert_eq!(doc.pages().unwrap().len(), 2);
    }
}

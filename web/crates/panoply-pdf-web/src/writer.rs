//! Writing PDF files one object at a time.
//!
//! Objects go straight to the output as they are produced, so the writer holds only the offsets
//! of what it has written and a small buffer of objects waiting to be packed into an object
//! stream. Finishing writes a compact cross-reference stream (or a classic table when object
//! streams are off).

use std::io::Write;

use sha2::{Digest, Sha256};

use crate::doc::{Doc, RawStream};
use crate::error::{PdfError, Result};
use crate::filters;
use crate::obj::{Dict, Obj, write_dict, write_obj};

/// Objects per object stream, and the size at which a pending stream is flushed.
const OBJSTM_MAX_OBJECTS: usize = 200;
const OBJSTM_MAX_BYTES: usize = 192 * 1024;

#[derive(Debug, Clone, Copy)]
enum Loc {
    Offset(u64),
    InStream { stream: u32, index: u32 },
}

/// Settings for a writer.
#[derive(Debug, Clone, Copy)]
pub struct WriterOptions {
    /// PDF version for the header, `(major, minor)`. Object streams need 1.5.
    pub version: (u8, u8),
    /// Pack small objects into object streams and write a cross-reference stream.
    pub object_streams: bool,
    /// Deflate level for object streams and the cross-reference stream (1 to 10).
    pub level: u8,
}

impl Default for WriterOptions {
    fn default() -> Self {
        WriterOptions {
            version: (1, 7),
            object_streams: true,
            level: 3,
        }
    }
}

/// A PDF being written.
pub struct PdfWriter<W: Write> {
    out: W,
    pos: u64,
    locs: Vec<Option<Loc>>,
    next: u32,
    opts: WriterOptions,
    pending: Vec<(u32, Vec<u8>)>,
    pending_bytes: usize,
    scratch: Vec<u8>,
}

impl<W: Write> PdfWriter<W> {
    /// Start a file: writes the header.
    pub fn new(out: W, opts: WriterOptions) -> Result<Self> {
        let mut w = PdfWriter {
            out,
            pos: 0,
            locs: vec![None],
            next: 1,
            opts,
            pending: Vec::new(),
            pending_bytes: 0,
            scratch: Vec::new(),
        };
        let (maj, min) = opts.version;
        w.raw(format!("%PDF-{maj}.{min}\n").as_bytes())?;
        w.raw(b"%\xE2\xE3\xCF\xD3\n")?;
        Ok(w)
    }

    fn raw(&mut self, b: &[u8]) -> Result<()> {
        self.out.write_all(b).map_err(PdfError::from)?;
        self.pos += b.len() as u64;
        Ok(())
    }

    /// Bytes written so far.
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Reserve the next object number.
    pub fn alloc(&mut self) -> u32 {
        let n = self.next;
        self.next += 1;
        if self.locs.len() <= n as usize {
            self.locs.resize(n as usize + 1, None);
        }
        n
    }

    /// Write a non-stream object. With object streams on it is buffered and packed.
    pub fn put(&mut self, num: u32, obj: &Obj) -> Result<()> {
        if self.opts.object_streams {
            let mut bytes = Vec::new();
            write_obj(&mut bytes, obj);
            self.pending_bytes += bytes.len();
            self.pending.push((num, bytes));
            if self.pending.len() >= OBJSTM_MAX_OBJECTS || self.pending_bytes >= OBJSTM_MAX_BYTES {
                self.flush_objects()?;
            }
            Ok(())
        } else {
            self.put_direct(num, obj)
        }
    }

    /// Write a non-stream object as its own indirect object, never packed.
    pub fn put_direct(&mut self, num: u32, obj: &Obj) -> Result<()> {
        self.locs[num as usize] = Some(Loc::Offset(self.pos));
        let mut b = std::mem::take(&mut self.scratch);
        b.clear();
        b.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        write_obj(&mut b, obj);
        b.extend_from_slice(b"\nendobj\n");
        let r = self.raw(&b);
        self.scratch = b;
        r
    }

    fn stream_head(&mut self, num: u32, dict: &Dict, len: u64) -> Result<()> {
        self.locs[num as usize] = Some(Loc::Offset(self.pos));
        let mut d = dict.clone();
        d.set("Length", Obj::Int(len as i64));
        let mut b = Vec::with_capacity(128);
        b.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
        write_dict(&mut b, &d);
        b.extend_from_slice(b"\nstream\n");
        self.raw(&b)
    }

    /// Write a stream whose bytes are already encoded to match `dict`'s filters.
    pub fn put_stream(&mut self, num: u32, dict: &Dict, data: &[u8]) -> Result<()> {
        self.stream_head(num, dict, data.len() as u64)?;
        self.raw(data)?;
        self.raw(b"\nendstream\nendobj\n")
    }

    /// Write a stream by copying its raw bytes from a source document, untouched.
    pub fn put_stream_from(
        &mut self,
        num: u32,
        dict: &Dict,
        doc: &Doc,
        raw: RawStream,
    ) -> Result<()> {
        self.stream_head(num, dict, raw.len)?;
        struct Counting<'a, W: Write> {
            w: &'a mut W,
            n: u64,
        }
        impl<W: Write> Write for Counting<'_, W> {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                let k = self.w.write(b)?;
                self.n += k as u64;
                Ok(k)
            }
            fn flush(&mut self) -> std::io::Result<()> {
                self.w.flush()
            }
        }
        let mut c = Counting {
            w: &mut self.out,
            n: 0,
        };
        doc.copy_raw(raw, &mut c)?;
        self.pos += c.n;
        self.raw(b"\nendstream\nendobj\n")
    }

    /// Pack the pending objects into an object stream now.
    pub fn flush_objects(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        let pending = std::mem::take(&mut self.pending);
        self.pending_bytes = 0;
        let stm = self.alloc();
        let mut head = Vec::new();
        let mut body = Vec::new();
        for (num, bytes) in &pending {
            head.extend_from_slice(format!("{num} {} ", body.len()).as_bytes());
            body.extend_from_slice(bytes);
            body.push(b'\n');
        }
        let first = head.len();
        head.extend_from_slice(&body);
        let z = filters::deflate(&head, self.opts.level);
        let mut d = Dict::new();
        d.set("Type", Obj::name("ObjStm"));
        d.set("N", Obj::Int(pending.len() as i64));
        d.set("First", Obj::Int(first as i64));
        d.set("Filter", Obj::name("FlateDecode"));
        self.put_stream(stm, &d, &z)?;
        for (i, (num, _)) in pending.iter().enumerate() {
            self.locs[*num as usize] = Some(Loc::InStream {
                stream: stm,
                index: i as u32,
            });
        }
        Ok(())
    }

    /// Finish the file: pending objects, cross-reference data, trailer. `root` is the catalog,
    /// `info` the optional document information dictionary, `id_seed` feeds the file identifier.
    /// Returns the size of the file.
    pub fn finish(mut self, root: u32, info: Option<u32>, id_seed: &[u8]) -> Result<u64> {
        self.flush_objects()?;
        let digest = Sha256::digest(id_seed);
        let id_hex: Vec<u8> = digest[..16].to_vec();
        let id = Obj::Array(vec![Obj::Str(id_hex.clone()), Obj::Str(id_hex)]);
        let size_before = self.next;
        if self.opts.object_streams {
            let xref_num = self.alloc();
            let size = self.next;
            let max_off = self
                .locs
                .iter()
                .flatten()
                .map(|l| match l {
                    Loc::Offset(o) => *o,
                    Loc::InStream { stream, .. } => u64::from(*stream),
                })
                .max()
                .unwrap_or(0)
                .max(self.pos);
            let w2 = bytes_for(max_off);
            let max_idx = self
                .locs
                .iter()
                .flatten()
                .filter_map(|l| match l {
                    Loc::InStream { index, .. } => Some(u64::from(*index)),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            let w3 = bytes_for(max_idx).max(1);
            let row = 1 + w2 + w3;
            let xref_pos = self.pos;
            self.locs[xref_num as usize] = Some(Loc::Offset(xref_pos));
            let mut data = Vec::with_capacity(size as usize * row);
            for n in 0..size {
                let loc = self.locs.get(n as usize).copied().flatten();
                let (t, a, b): (u8, u64, u64) = match loc {
                    Some(Loc::Offset(o)) => (1, o, 0),
                    Some(Loc::InStream { stream, index }) => {
                        (2, u64::from(stream), u64::from(index))
                    }
                    None => (0, 0, if n == 0 { 65535 } else { 0 }),
                };
                data.push(t);
                data.extend_from_slice(&a.to_be_bytes()[8 - w2..]);
                data.extend_from_slice(&b.to_be_bytes()[8 - w3..]);
            }
            let predicted = filters::predict_up(&data, row);
            let z = filters::deflate(&predicted, self.opts.level);
            let mut d = Dict::new();
            d.set("Type", Obj::name("XRef"));
            d.set("Size", Obj::Int(i64::from(size)));
            d.set(
                "W",
                Obj::Array(vec![Obj::Int(1), Obj::Int(w2 as i64), Obj::Int(w3 as i64)]),
            );
            d.set("Root", Obj::r(root));
            if let Some(i) = info {
                d.set("Info", Obj::r(i));
            }
            d.set("ID", id);
            d.set("Filter", Obj::name("FlateDecode"));
            let mut parms = Dict::new();
            parms.set("Predictor", Obj::Int(12));
            parms.set("Columns", Obj::Int(row as i64));
            d.set("DecodeParms", Obj::Dict(parms));
            self.stream_head(xref_num, &d, z.len() as u64)?;
            self.raw(&z)?;
            self.raw(b"\nendstream\nendobj\n")?;
            self.raw(format!("startxref\n{xref_pos}\n%%EOF\n").as_bytes())?;
        } else {
            let size = size_before;
            let xref_pos = self.pos;
            let free: Vec<u32> = (1..size)
                .filter(|n| self.locs.get(*n as usize).copied().flatten().is_none())
                .collect();
            let mut t = String::with_capacity(size as usize * 20 + 32);
            t.push_str(&format!("xref\n0 {size}\n"));
            let next_free = |after: u32| free.iter().copied().find(|f| *f > after).unwrap_or(0);
            t.push_str(&format!("{:010} 65535 f \n", next_free(0)));
            for n in 1..size {
                match self.locs.get(n as usize).copied().flatten() {
                    Some(Loc::Offset(o)) => t.push_str(&format!("{o:010} 00000 n \n")),
                    _ => t.push_str(&format!("{:010} 00000 f \n", next_free(n))),
                }
            }
            let mut d = Dict::new();
            d.set("Size", Obj::Int(i64::from(size)));
            d.set("Root", Obj::r(root));
            if let Some(i) = info {
                d.set("Info", Obj::r(i));
            }
            d.set("ID", id);
            let mut tb = t.into_bytes();
            tb.extend_from_slice(b"trailer\n");
            write_dict(&mut tb, &d);
            tb.extend_from_slice(format!("\nstartxref\n{xref_pos}\n%%EOF\n").as_bytes());
            self.raw(&tb)?;
        }
        self.out.flush().map_err(PdfError::from)?;
        Ok(self.pos)
    }
}

fn bytes_for(v: u64) -> usize {
    let mut n = 1;
    let mut x = v >> 8;
    while x > 0 {
        n += 1;
        x >>= 8;
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;

    fn build(object_streams: bool) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut w = PdfWriter::new(
                &mut out,
                WriterOptions {
                    object_streams,
                    ..Default::default()
                },
            )
            .unwrap();
            let cat = w.alloc();
            let pages = w.alloc();
            let page = w.alloc();
            let content = w.alloc();
            let mut c = Dict::new();
            c.set("Type", Obj::name("Catalog"));
            c.set("Pages", Obj::r(pages));
            w.put(cat, &Obj::Dict(c)).unwrap();
            let mut p = Dict::new();
            p.set("Type", Obj::name("Pages"));
            p.set("Kids", Obj::Array(vec![Obj::r(page)]));
            p.set("Count", Obj::Int(1));
            w.put(pages, &Obj::Dict(p)).unwrap();
            let mut pg = Dict::new();
            pg.set("Type", Obj::name("Page"));
            pg.set("Parent", Obj::r(pages));
            pg.set(
                "MediaBox",
                Obj::Array(vec![Obj::Int(0), Obj::Int(0), Obj::Int(200), Obj::Int(100)]),
            );
            pg.set("Contents", Obj::r(content));
            w.put(page, &Obj::Dict(pg)).unwrap();
            w.put_stream(content, &Dict::new(), b"0 0 10 10 re f")
                .unwrap();
            w.finish(cat, None, b"seed").unwrap();
        }
        out
    }

    #[test]
    fn both_output_styles_read_back() {
        for style in [true, false] {
            let bytes = build(style);
            let doc = Doc::open(Source::from_bytes(bytes)).unwrap();
            let pages = doc.pages().unwrap();
            assert_eq!(pages.len(), 1, "object streams = {style}");
            let g = doc.page_geom(&pages[0]);
            assert_eq!(g.media, [0.0, 0.0, 200.0, 100.0]);
            let page = doc.resolve_dict(&Obj::r(pages[0].id)).unwrap();
            assert_eq!(doc.page_content(&page), b"0 0 10 10 re f\n");
        }
    }

    #[test]
    fn object_streams_make_a_smaller_file_for_many_small_objects() {
        let mut sizes = Vec::new();
        for style in [false, true] {
            let mut out = Vec::new();
            let mut w = PdfWriter::new(
                &mut out,
                WriterOptions {
                    object_streams: style,
                    ..Default::default()
                },
            )
            .unwrap();
            let cat = w.alloc();
            let pages = w.alloc();
            let mut kids = Vec::new();
            for _ in 0..300 {
                let pg = w.alloc();
                let mut d = Dict::new();
                d.set("Type", Obj::name("Page"));
                d.set("Parent", Obj::r(pages));
                d.set(
                    "MediaBox",
                    Obj::Array(vec![Obj::Int(0), Obj::Int(0), Obj::Int(612), Obj::Int(792)]),
                );
                w.put(pg, &Obj::Dict(d)).unwrap();
                kids.push(Obj::r(pg));
            }
            let mut c = Dict::new();
            c.set("Type", Obj::name("Catalog"));
            c.set("Pages", Obj::r(pages));
            w.put(cat, &Obj::Dict(c)).unwrap();
            let mut p = Dict::new();
            p.set("Type", Obj::name("Pages"));
            p.set("Kids", Obj::Array(kids));
            p.set("Count", Obj::Int(300));
            w.put(pages, &Obj::Dict(p)).unwrap();
            w.finish(cat, None, b"x").unwrap();
            let doc = Doc::open(Source::from_bytes(out.clone())).unwrap();
            assert_eq!(doc.pages().unwrap().len(), 300);
            sizes.push(out.len());
        }
        assert!(sizes[1] * 3 < sizes[0], "{sizes:?}");
    }
}

//! The invisible text layer of a searchable PDF: page content, and the font that carries it.
//!
//! Each recognized word is drawn in text render mode 3 (neither filled nor stroked), at the
//! word's box, with a font of our own. That font is a Type 0 composite font with Identity-H
//! encoding, one code (CID) per distinct character used in the document, and a `/ToUnicode`
//! map from codes back to characters. That is what makes the text searchable and copyable in
//! any language: the viewer asks the map, not the glyph shapes. The embedded font program is a
//! tiny TrueType file with one glyph (every code maps to it), generated here, so no font license
//! is involved. The glyph is a plain box, never drawn because the text is invisible; it is not
//! empty because pdfium (and so Chrome) drops the characters of a font whose glyphs have empty
//! bounds when the text runs along a rotated page.
//!
//! Positions: a word's box (pixels of the rendered page) is converted to points, the font size
//! is the box height and horizontal scaling (`Tz`) stretches the word's characters to the
//! box's width, which is what keeps selection aligned with the printed word. Pages with
//! `/Rotate` and crop boxes are handled by mapping displayed coordinates back into the page's
//! own coordinate system.

use std::collections::HashMap;
use std::io::Write;

use panoply_pdf::error::Result as PdfResult;
use panoply_pdf::filters::deflate;
use panoply_pdf::obj::{Dict, Obj};
use panoply_pdf::writer::PdfWriter;

use crate::types::OcrPage;

/// Name of the font in page resources.
pub const FONT_NAME: &str = "PanoplyOcr";
/// Advance of every glyph, in thousandths of the font size.
const ADVANCE: f64 = 500.0;
/// Most distinct characters one document's text layer can hold (2 byte codes, 0 unused).
const MAX_CIDS: usize = 65_000;

/// Assigns a code to each distinct character, in order of first use.
#[derive(Debug, Default)]
pub struct FontBuilder {
    cids: HashMap<char, u16>,
    order: Vec<char>,
}

impl FontBuilder {
    /// A builder with no characters.
    pub fn new() -> Self {
        Self::default()
    }

    /// The code for a character, assigned on first use. `None` when the document has more
    /// distinct characters than a font can hold.
    pub fn cid(&mut self, c: char) -> Option<u16> {
        if let Some(&id) = self.cids.get(&c) {
            return Some(id);
        }
        if self.order.len() >= MAX_CIDS {
            return None;
        }
        self.order.push(c);
        let id = self.order.len() as u16;
        self.cids.insert(c, id);
        Some(id)
    }

    /// Number of distinct characters.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// True when no character was used.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The characters in code order: code `i + 1` is `chars()[i]`.
    pub fn chars(&self) -> &[char] {
        &self.order
    }
}

/// A page's geometry in the PDF's own terms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PageBox {
    /// The visible box `[x0 y0 x1 y1]` in default user space (the crop box).
    pub crop: [f64; 4],
    /// The page's `/Rotate`: 0, 90, 180 or 270.
    pub rotate: i32,
}

impl PageBox {
    /// Map a point of the displayed page (points, origin top left, y down) to user space.
    pub fn to_user(&self, u: f64, v: f64) -> (f64, f64) {
        let [x0, y0, x1, y1] = self.crop;
        let (w, h) = (x1 - x0, y1 - y0);
        match self.rotate {
            90 => (x0 + v, y0 + u),
            180 => (x0 + w - u, y0 + v),
            270 => (x0 + w - v, y0 + h - u),
            _ => (x0 + u, y1 - v),
        }
    }

    /// The text matrix's linear part `(a, b, c, d)`: where the displayed page's right and up
    /// directions point in user space.
    pub fn matrix(&self) -> (f64, f64, f64, f64) {
        match self.rotate {
            90 => (0.0, 1.0, -1.0, 0.0),
            180 => (-1.0, 0.0, 0.0, -1.0),
            270 => (0.0, -1.0, 1.0, 0.0),
            _ => (1.0, 0.0, 0.0, 1.0),
        }
    }

    /// Size of the displayed page in points.
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

fn num(out: &mut String, v: f64) {
    use std::fmt::Write as _;
    let _ = write!(out, "{v:.2}");
    // Trim trailing zeros to keep the stream small.
    if out.ends_with('0') && out.contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
}

fn push_num(buf: &mut String, v: f64) {
    let mut s = String::new();
    num(&mut s, v);
    if s == "-0" {
        s = "0".into();
    }
    buf.push_str(&s);
}

/// The content stream that draws a page's recognized words invisibly. `px_per_pt` is how many
/// pixels of the OCR image make a point of the displayed page.
pub fn page_content(
    page: &OcrPage,
    px_per_pt: f32,
    geom: &PageBox,
    fonts: &mut FontBuilder,
) -> Vec<u8> {
    let mut s = String::from("q\nBT\n3 Tr\n0 Tc 0 Tw 0 Ts\n");
    let k = f64::from(px_per_pt.max(1e-3));
    let (a, b, c, d) = geom.matrix();
    let mut any = false;
    for line in &page.lines {
        for word in &line.words {
            let mut codes = String::new();
            let mut n = 0usize;
            let mut ok = true;
            for ch in word.text.chars() {
                match fonts.cid(ch) {
                    Some(id) => {
                        use std::fmt::Write as _;
                        let _ = write!(codes, "{id:04X}");
                        n += 1;
                    }
                    None => ok = false,
                }
            }
            if !ok || n == 0 {
                continue;
            }
            // A trailing space code makes extracted text come out with spaces between words.
            if let Some(sp) = fonts.cid(' ') {
                use std::fmt::Write as _;
                let _ = write!(codes, "{sp:04X}");
            }
            let bx = f64::from(word.bbox.x) / k;
            let bw = (f64::from(word.bbox.w) / k).max(0.1);
            let bh = (f64::from(word.bbox.h) / k).max(1.0);
            let by_bottom = (f64::from(word.bbox.y) + f64::from(word.bbox.h)) / k;
            // The font has its ascent at 0.8 and its descent at 0.2 of the size, so the
            // baseline sits a fifth of the box height above the box's bottom.
            let baseline_v = by_bottom - 0.2 * bh;
            let (e, f) = geom.to_user(bx, baseline_v);
            let tz = (bw / (n as f64 * ADVANCE / 1000.0 * bh) * 100.0).clamp(1.0, 4000.0);
            s.push('/');
            s.push_str(FONT_NAME);
            s.push(' ');
            push_num(&mut s, bh);
            s.push_str(" Tf ");
            push_num(&mut s, tz);
            s.push_str(" Tz ");
            for v in [a, b, c, d, e, f] {
                push_num(&mut s, v);
                s.push(' ');
            }
            s.push_str("Tm <");
            s.push_str(&codes);
            s.push_str("> Tj\n");
            any = true;
        }
    }
    s.push_str("ET\nQ\n");
    if any { s.into_bytes() } else { Vec::new() }
}

/// The ToUnicode map for the characters used.
pub fn to_unicode(fonts: &FontBuilder) -> Vec<u8> {
    let mut s = String::new();
    s.push_str("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n");
    s.push_str("/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n");
    s.push_str("/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n");
    s.push_str("1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    for (chunk_no, chunk) in fonts.chars().chunks(100).enumerate() {
        s.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (i, ch) in chunk.iter().enumerate() {
            let code = chunk_no * 100 + i + 1;
            let mut buf = [0u16; 2];
            let hex: String = ch
                .encode_utf16(&mut buf)
                .iter()
                .map(|u| format!("{u:04X}"))
                .collect();
            s.push_str(&format!("<{code:04X}> <{hex}>\n"));
        }
        s.push_str("endbfchar\n");
    }
    s.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    s.into_bytes()
}

/// A TrueType font with one box glyph and a 500 unit advance, units per em 1000.
pub fn glyphless_ttf() -> Vec<u8> {
    fn be16(v: &mut Vec<u8>, x: i16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    fn beu16(v: &mut Vec<u8>, x: u16) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    fn be32(v: &mut Vec<u8>, x: u32) {
        v.extend_from_slice(&x.to_be_bytes());
    }
    let mut head = Vec::new();
    be32(&mut head, 0x0001_0000); // version
    be32(&mut head, 0x0001_0000); // fontRevision
    be32(&mut head, 0); // checkSumAdjustment, patched below
    be32(&mut head, 0x5F0F_3CF5); // magic
    beu16(&mut head, 0x000B); // flags: baseline at 0, lsb at 0, instructions may depend on size
    beu16(&mut head, 1000); // unitsPerEm
    head.extend_from_slice(&[0; 16]); // created, modified
    be16(&mut head, 0); // xMin
    be16(&mut head, -200); // yMin
    be16(&mut head, 500); // xMax
    be16(&mut head, 800); // yMax
    beu16(&mut head, 0); // macStyle
    beu16(&mut head, 8); // lowestRecPPEM
    be16(&mut head, 2); // fontDirectionHint
    be16(&mut head, 0); // indexToLocFormat: short
    be16(&mut head, 0); // glyphDataFormat

    let mut hhea = Vec::new();
    be32(&mut hhea, 0x0001_0000);
    be16(&mut hhea, 800); // ascender
    be16(&mut hhea, -200); // descender
    be16(&mut hhea, 0); // lineGap
    beu16(&mut hhea, 500); // advanceWidthMax
    be16(&mut hhea, 0); // minLeftSideBearing
    be16(&mut hhea, 0); // minRightSideBearing
    be16(&mut hhea, 500); // xMaxExtent
    be16(&mut hhea, 1); // caretSlopeRise
    be16(&mut hhea, 0); // caretSlopeRun
    be16(&mut hhea, 0); // caretOffset
    hhea.extend_from_slice(&[0; 8]); // reserved
    be16(&mut hhea, 0); // metricDataFormat
    beu16(&mut hhea, 1); // numberOfHMetrics

    let mut maxp = Vec::new();
    be32(&mut maxp, 0x0001_0000);
    beu16(&mut maxp, 1); // numGlyphs
    for v in [4u16, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
        beu16(&mut maxp, v);
    }

    let mut hmtx = Vec::new();
    beu16(&mut hmtx, 500);
    be16(&mut hmtx, 0);

    // One glyph: a single rectangle covering the whole character cell (0, -200) to (500, 800),
    // so that its measured bounds are not empty. It is never drawn (text render mode 3), but
    // pdfium drops the characters of a font whose glyphs have empty bounds when the text is
    // rotated a quarter turn, which made scans on pages with /Rotate 90 or 270 unsearchable in
    // pdfium based viewers (Chrome). Short loca stores offset / 2.
    let mut glyf = Vec::new();
    be16(&mut glyf, 1); // numberOfContours
    be16(&mut glyf, 0); // xMin
    be16(&mut glyf, -200); // yMin
    be16(&mut glyf, 500); // xMax
    be16(&mut glyf, 800); // yMax
    beu16(&mut glyf, 3); // endPtsOfContours[0]
    beu16(&mut glyf, 0); // instructionLength
    glyf.extend_from_slice(&[0x01; 4]); // flags: on curve, 16 bit deltas
    for dx in [0i16, 500, 0, -500] {
        be16(&mut glyf, dx);
    }
    for dy in [-200i16, 0, 1000, 0] {
        be16(&mut glyf, dy);
    }
    let mut loca = Vec::new();
    beu16(&mut loca, 0);
    beu16(&mut loca, (glyf.len() / 2) as u16);

    // cmap: one Windows Unicode BMP subtable (format 4) with only the closing segment.
    let mut cmap = Vec::new();
    beu16(&mut cmap, 0); // version
    beu16(&mut cmap, 1); // numTables
    beu16(&mut cmap, 3); // platformID
    beu16(&mut cmap, 1); // encodingID
    be32(&mut cmap, 12); // offset
    beu16(&mut cmap, 4); // format
    beu16(&mut cmap, 24); // length
    beu16(&mut cmap, 0); // language
    beu16(&mut cmap, 2); // segCountX2
    beu16(&mut cmap, 2); // searchRange
    beu16(&mut cmap, 0); // entrySelector
    beu16(&mut cmap, 0); // rangeShift
    beu16(&mut cmap, 0xFFFF); // endCode
    beu16(&mut cmap, 0); // reservedPad
    beu16(&mut cmap, 0xFFFF); // startCode
    be16(&mut cmap, 1); // idDelta
    beu16(&mut cmap, 0); // idRangeOffset

    let mut post = Vec::new();
    be32(&mut post, 0x0003_0000); // format 3: no glyph names
    post.extend_from_slice(&[0; 28]);

    let mut name = Vec::new();
    beu16(&mut name, 0); // format
    beu16(&mut name, 0); // count
    beu16(&mut name, 6); // stringOffset

    let mut tables: Vec<([u8; 4], Vec<u8>)> = vec![
        (*b"cmap", cmap),
        (*b"glyf", glyf),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", hmtx),
        (*b"loca", loca),
        (*b"maxp", maxp),
        (*b"name", name),
        (*b"post", post),
    ];
    tables.sort_by_key(|t| t.0);
    let n = tables.len() as u16;
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= u32::from(n) {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let mut out = Vec::new();
    be32(&mut out, 0x0001_0000);
    beu16(&mut out, n);
    beu16(&mut out, search_range);
    beu16(&mut out, entry_selector);
    beu16(&mut out, n * 16 - search_range);
    let mut offset = 12 + 16 * tables.len();
    let mut head_offset = 0usize;
    let mut body = Vec::new();
    for (tag, data) in &tables {
        let checksum = table_checksum(data);
        out.extend_from_slice(tag);
        be32(&mut out, checksum);
        be32(&mut out, offset as u32);
        be32(&mut out, data.len() as u32);
        if tag == b"head" {
            head_offset = offset;
        }
        body.extend_from_slice(data);
        while body.len() % 4 != 0 {
            body.push(0);
        }
        offset = 12 + 16 * tables.len() + body.len();
    }
    out.extend_from_slice(&body);
    let whole = table_checksum(&out);
    let adjust = 0xB1B0_AFBAu32.wrapping_sub(whole);
    out[head_offset + 8..head_offset + 12].copy_from_slice(&adjust.to_be_bytes());
    out
}

fn table_checksum(data: &[u8]) -> u32 {
    let mut sum = 0u32;
    for chunk in data.chunks(4) {
        let mut b = [0u8; 4];
        b[..chunk.len()].copy_from_slice(chunk);
        sum = sum.wrapping_add(u32::from_be_bytes(b));
    }
    sum
}

/// Object numbers reserved for the font before the pages are written.
#[derive(Debug, Clone, Copy)]
pub struct FontIds {
    /// The Type 0 font, which pages refer to.
    pub font: u32,
    /// Its descendant CIDFont.
    pub cid_font: u32,
    /// Its descriptor.
    pub descriptor: u32,
    /// The embedded font program.
    pub program: u32,
    /// The code to glyph map.
    pub cid_to_gid: u32,
    /// The code to Unicode map.
    pub to_unicode: u32,
}

impl FontIds {
    /// Reserve the numbers.
    pub fn alloc<W: Write>(w: &mut PdfWriter<W>) -> FontIds {
        FontIds {
            font: w.alloc(),
            cid_font: w.alloc(),
            descriptor: w.alloc(),
            program: w.alloc(),
            cid_to_gid: w.alloc(),
            to_unicode: w.alloc(),
        }
    }
}

fn flate_dict() -> Dict {
    let mut d = Dict::new();
    d.set("Filter", Obj::name("FlateDecode"));
    d
}

/// Write the font objects, once every page has been drawn (so the map holds every character).
pub fn write_font<W: Write>(
    w: &mut PdfWriter<W>,
    ids: FontIds,
    fonts: &FontBuilder,
) -> PdfResult<()> {
    let name = "PanoplyGlyphless";
    let mut font = Dict::new();
    font.set("Type", Obj::name("Font"));
    font.set("Subtype", Obj::name("Type0"));
    font.set("BaseFont", Obj::name(name));
    font.set("Encoding", Obj::name("Identity-H"));
    font.set("DescendantFonts", Obj::Array(vec![Obj::r(ids.cid_font)]));
    font.set("ToUnicode", Obj::r(ids.to_unicode));
    w.put(ids.font, &Obj::Dict(font))?;

    let mut info = Dict::new();
    info.set("Registry", Obj::Str(b"Adobe".to_vec()));
    info.set("Ordering", Obj::Str(b"Identity".to_vec()));
    info.set("Supplement", Obj::Int(0));
    let mut cid = Dict::new();
    cid.set("Type", Obj::name("Font"));
    cid.set("Subtype", Obj::name("CIDFontType2"));
    cid.set("BaseFont", Obj::name(name));
    cid.set("CIDSystemInfo", Obj::Dict(info));
    cid.set("FontDescriptor", Obj::r(ids.descriptor));
    cid.set("DW", Obj::Int(ADVANCE as i64));
    cid.set("CIDToGIDMap", Obj::r(ids.cid_to_gid));
    w.put(ids.cid_font, &Obj::Dict(cid))?;

    let mut desc = Dict::new();
    desc.set("Type", Obj::name("FontDescriptor"));
    desc.set("FontName", Obj::name(name));
    desc.set("Flags", Obj::Int(4));
    desc.set(
        "FontBBox",
        Obj::Array(vec![
            Obj::Int(0),
            Obj::Int(-200),
            Obj::Int(500),
            Obj::Int(800),
        ]),
    );
    desc.set("ItalicAngle", Obj::Int(0));
    desc.set("Ascent", Obj::Int(800));
    desc.set("Descent", Obj::Int(-200));
    desc.set("CapHeight", Obj::Int(700));
    desc.set("StemV", Obj::Int(80));
    desc.set("FontFile2", Obj::r(ids.program));
    w.put(ids.descriptor, &Obj::Dict(desc))?;

    let ttf = glyphless_ttf();
    let mut d = flate_dict();
    d.set("Length1", Obj::Int(ttf.len() as i64));
    w.put_stream(ids.program, &d, &deflate(&ttf, 6))?;

    // Every code maps to glyph 0, the one glyph.
    let zeros = vec![0u8; (fonts.len() + 1) * 2];
    w.put_stream(ids.cid_to_gid, &flate_dict(), &deflate(&zeros, 6))?;

    w.put_stream(
        ids.to_unicode,
        &flate_dict(),
        &deflate(&to_unicode(fonts), 6),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{EngineKind, Line, Rect, Word};

    fn page(words: &[(&str, f32, f32, f32, f32)]) -> OcrPage {
        let ws: Vec<Word> = words
            .iter()
            .map(|(t, x, y, w, h)| Word {
                text: (*t).into(),
                bbox: Rect {
                    x: *x,
                    y: *y,
                    w: *w,
                    h: *h,
                },
                confidence: 0.9,
            })
            .collect();
        OcrPage {
            width: 1000,
            height: 1000,
            lines: vec![Line::from_words(ws).unwrap()],
            engine: EngineKind::PpOcr,
            languages: vec![],
        }
    }

    #[test]
    fn characters_get_codes_in_order_of_first_use() {
        let mut f = FontBuilder::new();
        assert_eq!(f.cid('a'), Some(1));
        assert_eq!(f.cid('b'), Some(2));
        assert_eq!(f.cid('a'), Some(1));
        assert_eq!(f.len(), 2);
        assert_eq!(f.chars(), &['a', 'b']);
    }

    #[test]
    fn the_glyphless_font_has_a_valid_directory_and_checksum() {
        let t = glyphless_ttf();
        assert_eq!(&t[..4], &[0, 1, 0, 0]);
        let n = u16::from_be_bytes([t[4], t[5]]) as usize;
        assert_eq!(n, 9);
        // Tables are sorted by tag, each starts on a 4 byte boundary inside the file.
        let mut last = [0u8; 4];
        let mut head = None;
        for i in 0..n {
            let e = &t[12 + 16 * i..28 + 16 * i];
            let tag: [u8; 4] = e[..4].try_into().unwrap();
            assert!(tag > last);
            last = tag;
            let off = u32::from_be_bytes(e[8..12].try_into().unwrap()) as usize;
            let len = u32::from_be_bytes(e[12..16].try_into().unwrap()) as usize;
            assert_eq!(off % 4, 0);
            assert!(off + len <= t.len());
            let sum = u32::from_be_bytes(e[4..8].try_into().unwrap());
            if &tag == b"head" {
                head = Some(off);
            } else {
                assert_eq!(table_checksum(&t[off..off + len]), sum, "{tag:?}");
            }
        }
        // The whole file sums to the magic value once checkSumAdjustment is in.
        assert_eq!(table_checksum(&t), 0xB1B0_AFBA);
        let h = head.unwrap();
        assert_eq!(&t[h + 12..h + 16], &0x5F0F_3CF5u32.to_be_bytes());
    }

    #[test]
    fn a_word_is_drawn_invisibly_at_its_box() {
        // 300 DPI: 4.1667 px per point. A word at x 300 px (72 pt), 600 px down.
        let p = page(&[("Hi", 300.0, 600.0, 100.0, 40.0)]);
        let geom = PageBox {
            crop: [0.0, 0.0, 612.0, 792.0],
            rotate: 0,
        };
        let mut f = FontBuilder::new();
        let s = String::from_utf8(page_content(&p, 300.0 / 72.0, &geom, &mut f)).unwrap();
        assert!(s.contains("3 Tr"), "{s}");
        assert!(s.contains("/PanoplyOcr 9.6 Tf"), "{s}");
        // Word box 24 pt wide, two characters of 500/1000 em at 9.6 pt: Tz = 24 / (2 * 0.5 * 9.6).
        assert!(s.contains(" 250 Tz "), "{s}");
        // x = 72 pt; baseline is 0.2 * 9.6 pt above the box bottom: v = 144 + 9.6 - 1.92.
        assert!(s.contains("1 0 0 1 72 "), "{s}");
        assert!(s.contains("Tm <00010002") && s.contains("0003> Tj"), "{s}");
        assert_eq!(f.chars(), &['H', 'i', ' ']);
    }

    #[test]
    fn rotation_maps_displayed_points_into_user_space() {
        let crop = [0.0, 0.0, 600.0, 800.0];
        let g0 = PageBox { crop, rotate: 0 };
        assert_eq!(g0.to_user(10.0, 20.0), (10.0, 780.0));
        // Rotate 90: the displayed page is 800 wide and 600 tall; its top left corner is the
        // unrotated bottom left.
        let g90 = PageBox { crop, rotate: 90 };
        assert_eq!(g90.to_user(0.0, 0.0), (0.0, 0.0));
        assert_eq!(g90.to_user(800.0, 0.0), (0.0, 800.0));
        assert_eq!(g90.display_size(), (800.0, 600.0));
        let g180 = PageBox { crop, rotate: 180 };
        assert_eq!(g180.to_user(0.0, 0.0), (600.0, 0.0));
        let g270 = PageBox { crop, rotate: 270 };
        assert_eq!(g270.to_user(0.0, 0.0), (600.0, 800.0));
        assert_eq!(g270.matrix(), (0.0, -1.0, 1.0, 0.0));
    }

    #[test]
    fn the_unicode_map_covers_every_code_and_surrogate_pairs() {
        let mut f = FontBuilder::new();
        f.cid('a');
        f.cid('\u{1F600}');
        f.cid('न');
        let m = String::from_utf8(to_unicode(&f)).unwrap();
        assert!(m.contains("3 beginbfchar"), "{m}");
        assert!(m.contains("<0001> <0061>"));
        assert!(m.contains("<0002> <D83DDE00>"));
        assert!(m.contains("<0003> <0928>"));
    }

    #[test]
    fn a_page_with_no_usable_words_draws_nothing() {
        let geom = PageBox {
            crop: [0.0, 0.0, 100.0, 100.0],
            rotate: 0,
        };
        let empty = OcrPage {
            width: 10,
            height: 10,
            lines: vec![],
            engine: EngineKind::PpOcr,
            languages: vec![],
        };
        assert!(page_content(&empty, 1.0, &geom, &mut FontBuilder::new()).is_empty());
    }
}

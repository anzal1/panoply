//! Lossless metadata inspection and stripping for images.
//!
//! Everything here works on whole files held in memory. Pixel data is never
//! decoded or re-encoded: the container is walked, metadata segments are
//! dropped or edited, and every other byte is copied through untouched.
//!
//! Supported containers: JPEG, PNG, WebP, classic TIFF and the ISO BMFF family
//! (HEIC, HEIF, AVIF). The module uses `std` only.
//!
//! Removal rules, so the result card can be trusted:
//!
//! * An EXIF entry that is removed is deleted from its directory (the entry
//!   array is shifted, the count and next-directory pointer are fixed) and
//!   every byte of its value, and of any sub-directory it owned, is overwritten
//!   with zeros. A raw byte scan of the output finds nothing.
//! * Directory offsets are never rebuilt, so a kept MakerNote stays valid.
//! * All offsets and lengths come from untrusted input and are bounds checked.
//!   Loops between directories are detected and reported as
//!   [`MetaError::Corrupt`].

mod bmff;
mod exif;
mod jpeg;
mod png;
mod tiff;
mod webp;
mod xmp;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod testutil;

use std::fmt;

/// The container format of an image file, found from its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// JPEG (JFIF or Exif flavour).
    Jpeg,
    /// PNG, including APNG.
    Png,
    /// WebP, simple or extended, still or animated.
    WebP,
    /// TIFF, including TIFF based raw files such as DNG.
    Tiff,
    /// ISO base media file format images: HEIC, HEIF and AVIF.
    Bmff,
    /// Anything else.
    Unknown,
}

impl Container {
    /// A short plain name for messages, such as "JPEG" or "HEIC/AVIF".
    pub fn name(self) -> &'static str {
        match self {
            Container::Jpeg => "JPEG",
            Container::Png => "PNG",
            Container::WebP => "WebP",
            Container::Tiff => "TIFF",
            Container::Bmff => "HEIC/AVIF",
            Container::Unknown => "unknown",
        }
    }
}

/// Why metadata work failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetaError {
    /// The file type or feature is not handled. The text says what to do next.
    Unsupported(String),
    /// The file is damaged or hostile. The text says what looked wrong.
    Corrupt(String),
}

impl fmt::Display for MetaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MetaError::Unsupported(s) => write!(f, "{s}"),
            MetaError::Corrupt(s) => write!(
                f,
                "This file looks damaged: {s}. Open it in a photo app and save a fresh copy, then try again."
            ),
        }
    }
}

impl std::error::Error for MetaError {}

pub(crate) fn corrupt(s: &str) -> MetaError {
    MetaError::Corrupt(s.to_string())
}

/// A GPS fix in decimal degrees.
#[derive(Debug, Clone, PartialEq)]
pub struct Gps {
    /// Latitude, positive north.
    pub lat: f64,
    /// Longitude, positive east.
    pub lon: f64,
    /// Altitude in metres above sea level, when recorded.
    pub alt: Option<f64>,
}

/// Everything found in a file that is not pixels.
#[derive(Debug, Clone)]
pub struct Report {
    /// The container the file was read as.
    pub container: Container,
    /// An EXIF block exists.
    pub exif_present: bool,
    /// EXIF orientation, 1 to 8. It is 1 when absent.
    pub orientation: u8,
    /// A valid GPS fix, or `None` when there is none.
    pub gps: Option<Gps>,
    /// How many GPS tag entries exist, even without a valid fix.
    pub gps_tags: usize,
    /// "Make Model" of the camera.
    pub camera: Option<String>,
    /// The lens model.
    pub lens: Option<String>,
    /// Serial numbers and unique IDs as (tag name, value).
    pub serials: Vec<(String, String)>,
    /// Artist, copyright, camera owner and XMP creator as (tag name, value).
    pub owner: Vec<(String, String)>,
    /// The software that wrote or edited the file.
    pub software: Option<String>,
    /// When the photo was taken, as written in the file.
    pub date_taken: Option<String>,
    /// An XMP packet exists.
    pub xmp: bool,
    /// IPTC data exists.
    pub iptc: bool,
    /// Content credentials (C2PA) exist.
    pub c2pa: bool,
    /// An ICC color profile exists.
    pub icc: bool,
    /// Embedded thumbnails: EXIF IFD1, MPF previews, HEIC thumbnail items.
    pub thumbnails: usize,
    /// Size of the MakerNote in bytes, 0 when absent.
    pub maker_note_bytes: usize,
    /// Extra images inside the file, such as "gain map (MPF)" or "depth map".
    pub secondary_images: Vec<String>,
    /// The Apple Live Photo content identifier, when found.
    pub live_photo_id: Option<String>,
    /// One plain sentence about what is hidden in the file.
    pub summary: String,
}

impl Report {
    pub(crate) fn new(container: Container) -> Report {
        Report {
            container,
            exif_present: false,
            orientation: 1,
            gps: None,
            gps_tags: 0,
            camera: None,
            lens: None,
            serials: Vec::new(),
            owner: Vec::new(),
            software: None,
            date_taken: None,
            xmp: false,
            iptc: false,
            c2pa: false,
            icc: false,
            thumbnails: 0,
            maker_note_bytes: 0,
            secondary_images: Vec::new(),
            live_photo_id: None,
            summary: String::new(),
        }
    }

    pub(crate) fn finish(&mut self) {
        self.summary = summarize(self);
    }
}

fn join_phrase(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        n => format!("{} and {}", items[..n - 1].join(", "), items[n - 1]),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

fn summarize(r: &Report) -> String {
    let mut items: Vec<String> = Vec::new();
    if r.gps.is_some() || r.gps_tags > 0 {
        items.push("location".into());
    }
    if !r.serials.is_empty() {
        items.push(
            if r.serials.len() == 1 {
                "serial number"
            } else {
                "serial numbers"
            }
            .into(),
        );
    }
    if !r.owner.is_empty() {
        items.push("owner or copyright details".into());
    }
    if r.thumbnails > 0 {
        items.push(plural(r.thumbnails, "thumbnail", "thumbnails"));
    }
    if r.maker_note_bytes > 0 {
        items.push("maker notes".into());
    }
    if r.xmp {
        items.push("XMP data".into());
    }
    if r.iptc {
        items.push("IPTC data".into());
    }
    if r.c2pa {
        items.push("content credentials".into());
    }
    if r.live_photo_id.is_some() {
        items.push("a Live Photo ID".into());
    }
    if !r.secondary_images.is_empty() {
        let n = r
            .secondary_images
            .iter()
            .filter(|s| !s.starts_with("preview"))
            .count();
        if n > 0 {
            items.push(plural(n, "extra image", "extra images"));
        }
    }
    if items.is_empty() {
        "no private metadata found".to_string()
    } else {
        format!("contains {}", join_phrase(&items))
    }
}

/// How much metadata [`strip`] removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StripLevel {
    /// Remove location only.
    Gps,
    /// Remove location, serial numbers, owner and copyright, thumbnails,
    /// maker notes, XMP, IPTC and C2PA. Camera make and model, dates,
    /// exposure, orientation and the color profile stay.
    Private,
    /// Remove all metadata except what changes how the pixels look:
    /// orientation, the color profile and decoder flags.
    Everything,
}

/// The result of [`strip`].
#[derive(Debug, Clone)]
pub struct StripOutcome {
    /// The cleaned file.
    pub bytes: Vec<u8>,
    /// Plain lines for the result card, such as "GPS location".
    pub removed: Vec<String>,
    /// What was deliberately kept, such as "orientation".
    pub kept: Vec<String>,
    /// Things the user should know about, such as an in-place edit.
    pub warnings: Vec<String>,
}

/// What a strip pass did. Containers fill this and [`Log::removed`] makes the
/// card lines.
#[derive(Default)]
pub(crate) struct Log {
    pub tally: exif::Tally,
    pub exif_all: bool,
    pub xmp: bool,
    pub xmp_gps: usize,
    pub iptc: bool,
    pub c2pa: bool,
    pub comments: bool,
    pub thumbs_extra: usize,
    pub extra: Vec<String>,
    pub warnings: Vec<String>,
    pub kept_extra: Vec<String>,
}

impl Log {
    pub(crate) fn warn(&mut self, s: &str) {
        if !self.warnings.iter().any(|w| w == s) {
            self.warnings.push(s.to_string());
        }
    }

    pub(crate) fn extra(&mut self, s: &str) {
        if !self.extra.iter().any(|w| w == s) {
            self.extra.push(s.to_string());
        }
    }

    /// At level Everything the container code drops EXIF whole, so the lines
    /// for what it held come from a read of the source file.
    pub(crate) fn merge_report(&mut self, r: &Report) {
        let t = &mut self.tally;
        t.gps_tags = t
            .gps_tags
            .max(r.gps_tags + usize::from(r.gps.is_some() && r.gps_tags == 0));
        if t.serials == 0 {
            t.serials = r.serials.len();
        }
        if t.owner == 0 {
            t.owner = r.owner.len();
        }
        if t.thumbnails + self.thumbs_extra == 0 {
            t.thumbnails = r.thumbnails;
        }
        t.maker_note_bytes = t.maker_note_bytes.max(r.maker_note_bytes);
        self.xmp |= r.xmp;
        self.iptc |= r.iptc;
        self.c2pa |= r.c2pa;
        self.exif_all = r.exif_present;
    }

    pub(crate) fn removed(&self) -> Vec<String> {
        let mut v = Vec::new();
        let t = &self.tally;
        if t.gps_tags > 0 || self.xmp_gps > 0 {
            v.push("GPS location".to_string());
        }
        if t.serials > 0 {
            v.push("serial numbers".to_string());
        }
        if t.owner > 0 {
            v.push("owner and copyright".to_string());
        }
        if t.comments > 0 || self.comments {
            v.push("captions and comments".to_string());
        }
        let thumbs = t.thumbnails + self.thumbs_extra;
        if thumbs > 0 {
            v.push(plural(thumbs, "thumbnail", "thumbnails"));
        }
        if t.maker_note_bytes > 0 {
            v.push("maker notes".to_string());
        }
        if self.xmp {
            v.push("XMP".to_string());
        }
        if self.iptc {
            v.push("IPTC".to_string());
        }
        if self.c2pa {
            v.push("content credentials (C2PA)".to_string());
        }
        for e in &self.extra {
            v.push(e.clone());
        }
        if self.exif_all {
            v.push("camera, date and exposure details".to_string());
        }
        v
    }
}

/// Find the container from the first bytes of the file.
pub fn detect(bytes: &[u8]) -> Container {
    if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 && bytes[2] == 0xFF {
        return Container::Jpeg;
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Container::Png;
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return Container::WebP;
    }
    if bytes.len() >= 8
        && (bytes.starts_with(b"II*\0")
            || bytes.starts_with(b"MM\0*")
            || bytes.starts_with(b"II+\0")
            || bytes.starts_with(b"MM\0+"))
    {
        return Container::Tiff;
    }
    if bmff::is_image_ftyp(bytes) {
        return Container::Bmff;
    }
    Container::Unknown
}

fn unsupported_type() -> MetaError {
    MetaError::Unsupported(
        "This file type is not supported for metadata work. Save it as JPEG, PNG, WebP, TIFF, HEIC or AVIF and try again."
            .to_string(),
    )
}

/// Read everything private and hidden from a file without touching pixels.
pub fn inspect(bytes: &[u8]) -> Result<Report, MetaError> {
    let c = detect(bytes);
    let mut r = Report::new(c);
    match c {
        Container::Jpeg => jpeg::inspect(bytes, &mut r)?,
        Container::Png => png::inspect(bytes, &mut r)?,
        Container::WebP => webp::inspect(bytes, &mut r)?,
        Container::Tiff => tiff::inspect(bytes, &mut r)?,
        Container::Bmff => bmff::inspect(bytes, &mut r)?,
        Container::Unknown => return Err(unsupported_type()),
    }
    r.finish();
    Ok(r)
}

/// Remove metadata at the given level. Pixel data is copied byte for byte.
pub fn strip(bytes: &[u8], level: StripLevel) -> Result<StripOutcome, MetaError> {
    let c = detect(bytes);
    let (out, mut log) = match c {
        Container::Jpeg => jpeg::strip(bytes, level)?,
        Container::Png => png::strip(bytes, level)?,
        Container::WebP => webp::strip(bytes, level)?,
        Container::Tiff => tiff::strip(bytes, level)?,
        Container::Bmff => bmff::strip(bytes, level)?,
        Container::Unknown => return Err(unsupported_type()),
    };
    if level == StripLevel::Everything
        && let Ok(before) = inspect(bytes)
    {
        log.merge_report(&before);
    }
    let mut kept = Vec::new();
    if let Ok(after) = inspect(&out) {
        if after.orientation > 1 {
            kept.push("orientation".to_string());
        }
        if after.icc {
            kept.push("color profile".to_string());
        }
        if level != StripLevel::Everything && (after.camera.is_some() || after.date_taken.is_some())
        {
            kept.push("camera and date details".to_string());
        }
    }
    for k in &log.kept_extra {
        if !kept.contains(k) {
            kept.push(k.clone());
        }
    }
    let removed = log.removed();
    Ok(StripOutcome {
        bytes: out,
        removed,
        kept,
        warnings: log.warnings,
    })
}

/// The raw TIFF-header EXIF block (starts with `II*\0` or `MM\0*`).
///
/// Reads JPEG APP1, PNG eXIf, WebP EXIF and the HEIC/AVIF Exif item. It returns
/// `None` for whole TIFF files, where EXIF is the file itself.
pub fn extract_exif(bytes: &[u8]) -> Option<Vec<u8>> {
    match detect(bytes) {
        Container::Jpeg => jpeg::extract_exif(bytes),
        Container::Png => png::extract_exif(bytes),
        Container::WebP => webp::extract_exif(bytes),
        Container::Bmff => bmff::extract_exif(bytes),
        _ => None,
    }
}

/// The main XMP packet as raw bytes. JPEG extended XMP is not merged in.
pub fn extract_xmp(bytes: &[u8]) -> Option<Vec<u8>> {
    match detect(bytes) {
        Container::Jpeg => jpeg::extract_xmp(bytes),
        Container::Png => png::extract_xmp(bytes),
        Container::WebP => webp::extract_xmp(bytes),
        Container::Tiff => tiff::extract_xmp(bytes),
        Container::Bmff => bmff::extract_xmp(bytes),
        Container::Unknown => None,
    }
}

/// The ICC profile as raw bytes.
///
/// JPEG APP2 chunks are reassembled. WebP ICCP and TIFF tag 34675 are read.
/// PNG stores its profile zlib-compressed and this module has no inflate, so
/// this returns `None` for PNG. Use [`png_icc_compressed`] and inflate it with
/// the `png` crate or another zlib.
pub fn extract_icc(bytes: &[u8]) -> Option<Vec<u8>> {
    match detect(bytes) {
        Container::Jpeg => jpeg::extract_icc(bytes),
        Container::WebP => webp::extract_icc(bytes),
        Container::Tiff => tiff::extract_icc(bytes),
        Container::Bmff => bmff::extract_icc(bytes),
        _ => None,
    }
}

/// The PNG iCCP chunk as (profile name, still zlib-compressed profile bytes).
pub fn png_icc_compressed(bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    png::icc_compressed(bytes)
}

/// Prepare an EXIF block for a re-encoded image.
///
/// Orientation is set to 1. PixelXDimension and PixelYDimension (and IFD0
/// ImageWidth and ImageLength when present) are set to the new size. IFD1 and
/// its thumbnail bytes are removed for real. GPS is removed when `keep_gps` is
/// false. The MakerNote is kept only when `keep_maker_note` is true. It returns
/// `None` when the input is not valid EXIF or nothing useful is left.
pub fn exif_for_reencode(
    exif: &[u8],
    new_w: u32,
    new_h: u32,
    keep_gps: bool,
    keep_maker_note: bool,
) -> Option<Vec<u8>> {
    exif::for_reencode(exif, new_w, new_h, keep_gps, keep_maker_note)
}

/// Wrap a WebP into an extended file that carries these chunks.
///
/// A bare VP8 or VP8L file gets a VP8X chunk with the right flags and canvas
/// size. A file that already has VP8X keeps its chunks, and each given chunk
/// replaces the one of the same kind. It returns the input unchanged when all
/// three are `None`.
pub fn webp_embed(
    webp: &[u8],
    icc: Option<&[u8]>,
    exif: Option<&[u8]>,
    xmp: Option<&[u8]>,
) -> Result<Vec<u8>, MetaError> {
    webp::embed(webp, icc, exif, xmp)
}

/// A ready to splice JPEG APP1 segment: `FFE1`, length, `Exif\0\0`, block.
///
/// It returns an empty vector when the block is too big for one segment
/// (more than 65527 bytes) or is empty.
pub fn jpeg_app1_exif_segment(exif: &[u8]) -> Vec<u8> {
    jpeg::app1_exif_segment(exif)
}

/// The JPEG thumbnail inside an EXIF block (IFD1), when it has one: usually 160 by 120 pixels,
/// and free to read however large the picture is.
pub fn exif_thumbnail(exif: &[u8]) -> Option<&[u8]> {
    exif::thumbnail(exif)
}

/// `exif` with its IFD0 Orientation tag set to `orientation` (1 to 8). `None` when the block has
/// no Orientation tag of the usual kind (use [`jpeg_orientation_only_exif`] then).
pub fn exif_with_orientation(exif: &[u8], orientation: u8) -> Option<Vec<u8>> {
    exif::with_orientation(exif, orientation)
}

/// A minimal valid TIFF block holding only the IFD0 Orientation tag.
///
/// Values outside 1 to 8 are written as 1.
pub fn jpeg_orientation_only_exif(orientation: u8) -> Vec<u8> {
    exif::orientation_only(orientation)
}

// ---------------------------------------------------------------------------
// Shared helpers

pub(crate) fn be16(b: &[u8], o: usize) -> Option<u16> {
    let s = b.get(o..o.checked_add(2)?)?;
    Some(u16::from_be_bytes([s[0], s[1]]))
}

pub(crate) fn be32(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o.checked_add(4)?)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

pub(crate) fn le16(b: &[u8], o: usize) -> Option<u16> {
    let s = b.get(o..o.checked_add(2)?)?;
    Some(u16::from_le_bytes([s[0], s[1]]))
}

pub(crate) fn le32(b: &[u8], o: usize) -> Option<u32> {
    let s = b.get(o..o.checked_add(4)?)?;
    Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

/// First index of `needle` in `hay`.
pub(crate) fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    let first = needle[0];
    let last_start = hay.len() - needle.len();
    let mut i = 0;
    while i <= last_start {
        {
            let p = hay[i..=last_start].iter().position(|&c| c == first)?;
            i += p;
            if &hay[i..i + needle.len()] == needle {
                return Some(i);
            }
            i += 1;
        }
    }
    None
}

/// Concatenate byte ranges of `b`.
pub(crate) fn gather(b: &[u8], ranges: &[std::ops::Range<usize>]) -> Vec<u8> {
    let mut v = Vec::new();
    for r in ranges {
        if let Some(s) = b.get(r.clone()) {
            v.extend_from_slice(s);
        }
    }
    v
}

/// Write `data` back over the ranges that [`gather`] read.
pub(crate) fn scatter(b: &mut [u8], ranges: &[std::ops::Range<usize>], data: &[u8]) {
    let mut at = 0;
    for r in ranges {
        let n = r.end.saturating_sub(r.start);
        if let (Some(dst), Some(src)) = (b.get_mut(r.clone()), data.get(at..at + n)) {
            dst.copy_from_slice(src);
        }
        at += n;
    }
}

const fn make_crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = make_crc_table();

/// CRC-32 as used by PNG, over several parts.
pub(crate) fn crc32(parts: &[&[u8]]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for p in parts {
        for &b in *p {
            c = CRC_TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
        }
    }
    c ^ 0xFFFF_FFFF
}

//! Pieces every operation shares: opening an input, reading its metadata, encoding to any
//! target format, and the lines for the result card.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::color::OutSpace;
use crate::decode::{self, Input, OpenOpts, RowReader, SrcInfo};
use crate::encode::{self, Embed};
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::meta;
use crate::pipeline::{Ctl, ImagePlan, decode_to_image};
use crate::types::{Depth, Image, Layout, Samples};

use super::{Io, NameSpec, Output, human_bytes};

/// What to do with an animated file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Animation {
    /// Stop and say what the choices are (a command line cannot ask).
    Ask,
    /// Use the first frame.
    First,
}

impl Animation {
    /// From the `animation` option.
    pub fn from_opt(s: Option<&str>) -> Self {
        if s == Some("first") {
            Self::First
        } else {
            Self::Ask
        }
    }
}

/// An input that is open for decoding.
pub struct Loaded {
    /// Where it came from.
    pub path: PathBuf,
    /// File name, for card lines.
    pub name: String,
    /// The reader.
    pub reader: Box<dyn RowReader>,
    /// What the reader reports.
    pub info: SrcInfo,
    /// Size of the file in bytes.
    pub file_len: u64,
}

/// Name of a path for result card lines.
pub fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Open an input and check it can be processed.
pub fn load(
    io: &dyn Io,
    path: &Path,
    min_size_hint: Option<(u32, u32)>,
    animation: Animation,
    space: OutSpace,
) -> Result<Loaded> {
    let file = io.open_read(path)?;
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_string);
    let opts = OpenOpts {
        min_size_hint,
        first_frame: animation == Animation::First,
        buffer_budget: None,
        keep_color: space == OutSpace::Keep,
    };
    let reader = decode::open(Input::File(file), ext.as_deref(), &opts)?;
    let info = reader.info().clone();
    if info.frames > 1 && animation == Animation::Ask {
        return Err(ImageError::Unsupported(format!(
            "{} is animated. Choose \"first frame\" (animation=first) to keep one picture, or use a tool for animations. Panoply does not write animated files yet.",
            display_name(path)
        )));
    }
    Ok(Loaded {
        path: path.to_path_buf(),
        name: display_name(path),
        reader,
        info,
        file_len,
    })
}

/// The file bytes, when small enough to hold next to the decode (metadata work needs them).
pub fn source_bytes(io: &dyn Io, path: &Path) -> Option<Vec<u8>> {
    const CAP: u64 = 96 * 1024 * 1024;
    let mut f = io.open_read(path).ok()?;
    let len = f.metadata().ok()?.len();
    if len > CAP {
        return None;
    }
    let mut v = Vec::with_capacity(len as usize);
    f.read_to_end(&mut v).ok()?;
    Some(v)
}

/// Metadata read from the source file.
#[derive(Default)]
pub struct SourceMeta {
    /// Raw EXIF block.
    pub exif: Option<Vec<u8>>,
    /// Raw XMP packet.
    pub xmp: Option<Vec<u8>>,
    /// The inspection report.
    pub report: Option<meta::Report>,
    /// macOS: the source's ImageIO property dictionaries, which is how EXIF, GPS and IPTC reach a
    /// HEIC or AVIF written by the operating system encoder (it takes dictionaries, not a blob).
    #[cfg(target_os = "macos")]
    pub os_props: Option<panoply_platform_macos::Properties>,
}

impl SourceMeta {
    /// Read what the file carries. Returns an empty set when the file is too large or the
    /// container is not one the metadata code knows.
    pub fn read(bytes: Option<&[u8]>) -> SourceMeta {
        Self::read_for(bytes, Format::Jpeg)
    }

    /// Like [`SourceMeta::read`], for a job that writes `target`. A target the OS encoder writes
    /// (HEIC, and AVIF where the system has it) also needs the source's property dictionaries.
    pub fn read_for(bytes: Option<&[u8]>, target: Format) -> SourceMeta {
        let Some(b) = bytes else {
            return SourceMeta::default();
        };
        #[cfg(target_os = "macos")]
        let os_props =
            if matches!(target, Format::Heic | Format::Avif) && b.len() <= 64 * 1024 * 1024 {
                panoply_platform_macos::Decoder::open(b.to_vec())
                    .ok()
                    .map(|d| d.properties())
            } else {
                None
            };
        #[cfg(not(target_os = "macos"))]
        let _ = target;
        SourceMeta {
            exif: meta::extract_exif(b),
            xmp: meta::extract_xmp(b),
            report: meta::inspect(b).ok(),
            #[cfg(target_os = "macos")]
            os_props,
        }
    }

    /// True when the source has a GPS fix or GPS tags.
    pub fn has_gps(&self) -> bool {
        self.report
            .as_ref()
            .is_some_and(|r| r.gps.is_some() || r.gps_tags > 0)
    }
}

/// What was carried into the output, for the result card.
#[derive(Debug, Clone, Default)]
pub struct Carried {
    /// Blobs to embed.
    pub embed: Embed,
    /// "color profile", "EXIF", "GPS location".
    pub kept: Vec<String>,
    /// "depth map", "XMP", "thumbnail".
    pub dropped: Vec<String>,
}

fn target_carries(format: Format) -> (bool, bool) {
    // (EXIF, XMP)
    match format {
        Format::Jpeg | Format::WebP | Format::Png => (true, true),
        Format::Avif | Format::Heic => (true, false),
        _ => (false, false),
    }
}

/// Decide what metadata goes into the output and write the card lines for it.
///
/// `keep` is the user's choice (IPC-9: kept for conversions, stripped for web and portal
/// outputs). The EXIF block is rebuilt for the new pixels: orientation 1, new size, no thumbnail.
#[allow(clippy::too_many_arguments)]
pub fn carry(
    src: &SourceMeta,
    info: &SrcInfo,
    out_w: u32,
    out_h: u32,
    out_icc: Option<&Arc<Vec<u8>>>,
    keep: bool,
    keep_gps: bool,
    target: Format,
) -> Carried {
    let mut c = Carried::default();
    let (can_exif, can_xmp) = target_carries(target);
    if let Some(icc) = out_icc {
        // A wide gamut profile is part of how the picture looks: always embed it.
        if matches!(
            target,
            Format::Jpeg | Format::Png | Format::WebP | Format::Tiff | Format::Heic | Format::Avif
        ) {
            c.embed.icc = Some((**icc).clone());
            c.kept.push("color profile".into());
        } else {
            c.dropped
                .push("color profile (this format cannot carry it)".into());
        }
    }
    let gps = src.has_gps();
    // HEIC and AVIF written by the operating system encoder take property dictionaries, not an
    // EXIF blob, so the blob path below does not apply and the card must not claim it.
    let os_path =
        matches!(target, Format::Heic | Format::Avif) && crate::encode::os_can_encode(target);
    #[cfg(target_os = "macos")]
    if os_path
        && keep
        && let Some(p) = src.os_props.as_ref().filter(|p| !p.is_empty())
    {
        c.embed.os_props = Some(p.clone());
        c.embed.drop_gps = !keep_gps;
        c.kept.push("EXIF".into());
        if gps && keep_gps {
            c.kept.push("GPS location".into());
        } else if gps {
            c.dropped.push("GPS location".into());
        }
    }
    if os_path {
        // Handled above (kept) or nothing to keep; say what was dropped when keeping is off.
        if !keep && (src.exif.is_some() || gps) {
            c.dropped.push("EXIF".into());
            if gps {
                c.dropped.push("GPS location".into());
            }
        }
    } else if let Some(ex) = &src.exif {
        if keep && can_exif {
            if let Some(blob) = meta::exif_for_reencode(ex, out_w, out_h, keep_gps, true) {
                c.embed.exif = Some(blob);
                c.kept.push("EXIF".into());
                if gps && keep_gps {
                    c.kept.push("GPS location".into());
                } else if gps {
                    c.dropped.push("GPS location".into());
                }
            } else {
                c.dropped.push("EXIF (could not be read)".into());
            }
        } else if keep && !can_exif {
            c.dropped
                .push(format!("EXIF ({} cannot carry it)", target.label()));
            if gps {
                c.dropped.push("GPS location".into());
            }
        } else {
            c.dropped.push("EXIF".into());
            if gps {
                c.dropped.push("GPS location".into());
            }
        }
    }
    if let Some(x) = &src.xmp {
        let text_has = |needle: &[u8]| x.windows(needle.len()).any(|w| w == needle);
        // XMP written by Ultra HDR and Google container tools points at a gain map that is not
        // in the output, so it would describe something that is not there.
        let stale = text_has(b"hdrgm:") || text_has(b"GContainer");
        if keep && can_xmp && !stale {
            c.embed.xmp = Some(x.clone());
            c.kept.push("XMP".into());
        } else {
            c.dropped.push("XMP".into());
        }
    }
    if let Some(r) = &src.report {
        if r.thumbnails > 0 {
            c.dropped.push(if r.thumbnails == 1 {
                "embedded thumbnail".into()
            } else {
                format!("{} embedded thumbnails", r.thumbnails)
            });
        }
        for s in &r.secondary_images {
            c.dropped.push(s.clone());
        }
        if let Some(id) = &r.live_photo_id {
            let _ = id;
            c.dropped
                .push("Live Photo link (the paired video is not touched)".into());
        }
    }
    for d in &info.dropped {
        if !c.dropped.contains(d) {
            c.dropped.push(d.clone());
        }
    }
    c
}

/// Join card items: "kept: EXIF, color profile; dropped: XMP".
pub fn kept_dropped_line(c: &Carried) -> Option<String> {
    let mut parts = Vec::new();
    if !c.kept.is_empty() {
        parts.push(format!("kept: {}", c.kept.join(", ")));
    }
    if !c.dropped.is_empty() {
        parts.push(format!("dropped: {}", c.dropped.join(", ")));
    }
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// Settings for one encode.
#[derive(Debug, Clone)]
pub struct EncodeSpec {
    /// Output format.
    pub format: Format,
    /// Lossy quality, 1 to 100. `None` uses the format's default.
    pub quality: Option<f32>,
    /// Lossless WebP.
    pub lossless: bool,
    /// PNG effort.
    pub png_effort: encode::png::Effort,
    /// JPEG progressive scans.
    pub progressive: bool,
    /// JPEG 4:2:0 chroma.
    pub subsample: bool,
    /// Metadata to embed.
    pub embed: Embed,
}

impl EncodeSpec {
    /// Sensible defaults for a format.
    pub fn new(format: Format) -> Self {
        Self {
            format,
            quality: None,
            lossless: false,
            png_effort: encode::png::Effort::Normal,
            progressive: true,
            subsample: true,
            embed: Embed::default(),
        }
    }

    /// The default lossy quality of the format (docs/specs/id-and-image.md 3.1).
    pub fn default_quality(format: Format) -> f32 {
        match format {
            Format::Jpeg => 85.0,
            Format::WebP => 80.0,
            Format::Avif => 60.0,
            Format::Heic => 80.0,
            _ => 90.0,
        }
    }
}

/// The sample depth a format wants.
pub fn depth_for(format: Format, source: Depth) -> Depth {
    match format {
        Format::Png | Format::Tiff => source,
        _ => Depth::U8,
    }
}

/// Encode a whole image to a file's bytes.
pub fn encode_bytes(img: &Image, spec: &EncodeSpec) -> Result<Vec<u8>> {
    let q = spec
        .quality
        .unwrap_or_else(|| EncodeSpec::default_quality(spec.format));
    match spec.format {
        Format::Jpeg => {
            if img.layout.has_alpha() {
                return Err(ImageError::Failed(
                    "internal: JPEG needs the transparency flattened first".into(),
                ));
            }
            let img8 = img.as_u8_image();
            encode::jpeg::encode(
                &img8,
                &encode::jpeg::JpegOpts {
                    quality: q,
                    progressive: spec.progressive,
                    subsample: spec.subsample,
                    optimize: true,
                    mozjpeg_tables: false,
                    trellis: false,
                    embed: spec.embed.clone(),
                },
            )
        }
        Format::Png => encode::png::encode(img, &spec.embed, spec.png_effort),
        Format::WebP => encode::webp::encode(
            img,
            &encode::webp::WebpOpts {
                quality: if spec.lossless { 100.0 } else { q },
                lossless: spec.lossless,
                method: if spec.lossless { 6 } else { 4 },
                embed: spec.embed.clone(),
            },
        ),
        Format::Avif => encode::avif_or_os(img, q, &spec.embed),
        Format::Heic => encode::heic(img, q, &spec.embed),
        Format::Tiff => encode::tiff::encode(img, &spec.embed),
        Format::Bmp => encode::bmp::encode(img),
        Format::Gif => encode::gif::encode(img, 0.8),
        other => Err(ImageError::Unsupported(format!(
            "Panoply cannot write {} files. Choose JPG, PNG, WebP, AVIF, HEIC, TIFF, BMP or GIF.",
            other.label()
        ))),
    }
}

/// Decode bytes we just encoded, keeping the file's own color values (no conversion), so a
/// quality search can compare them with the picture it encoded.
pub fn decode_mem(bytes: Vec<u8>) -> Result<Image> {
    let mut reader = decode::open(
        Input::Mem(std::io::Cursor::new(bytes)),
        None,
        &OpenOpts::default(),
    )?;
    let (cancel, progress) = crate::pipeline::quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let plan = ImagePlan {
        out_space: OutSpace::Keep,
        ..Default::default()
    };
    Ok(decode_to_image(&mut *reader, &plan, &ctl)?.0)
}

/// Rows per band when scoring an encoded picture against its original.
const SCORE_BAND: usize = 128;
/// A band repeats this many rows of the one before, so every SSIM window lies inside one band.
const SCORE_OVERLAP: usize = 10;

/// Receives the decoded rows of an encoded picture and scores them, band by band, against the
/// same rows of the original. Memory stays at a band, however large the picture is.
struct BandScorer<'a> {
    reference: &'a Image,
    layout: Layout,
    width: usize,
    buf: Vec<u8>,
    start: usize,
    luma: f64,
    chroma: f64,
    weight: f64,
}

impl BandScorer<'_> {
    fn have(&self) -> usize {
        self.buf.len() / (self.width * self.layout.channels()).max(1)
    }

    /// Score the rows held, then keep the overlap for the next band.
    fn flush(&mut self, last: bool) {
        let rows = self.have();
        if rows == 0 || (self.weight > 0.0 && rows <= SCORE_OVERLAP) {
            return;
        }
        let ch = self.layout.channels();
        let band = Image {
            width: self.width as u32,
            height: rows as u32,
            layout: self.layout,
            samples: Samples::U8(std::mem::take(&mut self.buf)),
            icc: None,
        };
        let want = self
            .reference
            .crop(0, self.start as u32, self.reference.width, rows as u32);
        let s = crate::ssim::score(&want, &band);
        let w = rows.saturating_sub(SCORE_OVERLAP).max(1) as f64;
        self.luma += s.luma * w;
        self.chroma += s.chroma * w;
        self.weight += w;
        if !last {
            let Samples::U8(v) = band.samples else {
                unreachable!()
            };
            let keep = SCORE_OVERLAP * self.width * ch;
            self.buf = v[v.len() - keep..].to_vec();
            self.start += rows - SCORE_OVERLAP;
        }
    }
}

impl crate::pipeline::RowSink for BandScorer<'_> {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        _icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        if (width, height) != (self.reference.width, self.reference.height) || depth != Depth::U8 {
            return Err(ImageError::Failed(
                "internal: the encoded picture has a different size than the original".into(),
            ));
        }
        self.layout = layout;
        self.width = width as usize;
        Ok(())
    }

    fn push(&mut self, rows: crate::types::Rows<'_>) -> Result<()> {
        let crate::types::Rows::U8(s) = rows else {
            return Err(ImageError::Failed(
                "internal: scoring needs 8 bit rows".into(),
            ));
        };
        self.buf.extend_from_slice(s);
        while self.have() >= SCORE_BAND {
            self.flush(false);
        }
        Ok(())
    }
}

/// Decode `bytes` (a picture we just encoded) in bands and score it against `reference`
/// (8 bit, same size). The result matches scoring the whole picture, to within the rows at band
/// joins, and needs memory for one band instead of two whole pictures.
pub fn score_bytes(reference: &Image, bytes: Vec<u8>) -> Result<crate::ssim::Score> {
    let mut reader = decode::open(
        Input::Mem(std::io::Cursor::new(bytes)),
        None,
        &OpenOpts::default(),
    )?;
    let (cancel, progress) = crate::pipeline::quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let plan = crate::pipeline::StreamPlan {
        out_space: OutSpace::Keep,
        out_depth: Some(Depth::U8),
        ..Default::default()
    };
    let mut sink = BandScorer {
        reference,
        layout: Layout::Rgb,
        width: 0,
        buf: Vec::new(),
        start: 0,
        luma: 0.0,
        chroma: 0.0,
        weight: 0.0,
    };
    crate::pipeline::run_stream(&mut *reader, &plan, &ctl, &mut sink)?;
    sink.flush(true);
    if sink.weight == 0.0 {
        return Err(ImageError::Failed("internal: nothing was scored".into()));
    }
    Ok(crate::ssim::Score {
        luma: sink.luma / sink.weight,
        chroma: sink.chroma / sink.weight,
    })
}

/// Write finished bytes to a new output next to `input`.
pub fn write_output(
    io: &dyn Io,
    input: &Path,
    name: &NameSpec,
    ext: &str,
    bytes: &[u8],
) -> Result<PathBuf> {
    let mut out = Output::create(io, input, name, ext)?;
    out.write_all(bytes)?;
    Ok(out.commit())
}

/// "name: 3.2 MB to 610 KB, 81% smaller".
pub fn size_change_line(name: &str, before: u64, after: u64) -> String {
    if after < before {
        let pct = (100.0 * (before - after) as f64 / before as f64).round();
        format!(
            "{name}: {} to {}, {pct:.0}% smaller",
            human_bytes(before),
            human_bytes(after)
        )
    } else if after > before {
        let pct = (100.0 * (after - before) as f64 / before as f64).round();
        format!(
            "{name}: {} to {}, {pct:.0}% larger",
            human_bytes(before),
            human_bytes(after)
        )
    } else {
        format!(
            "{name}: {} to {}, same size",
            human_bytes(before),
            human_bytes(after)
        )
    }
}

/// Anything that would make the layout wrong for a flat output.
pub fn is_alpha_layout(l: Layout) -> bool {
    l.has_alpha()
}

/// Check the cancel flag.
pub fn check_cancel(io: &dyn Io) -> Result<()> {
    if io.cancel().is_cancelled() {
        Err(ImageError::Cancelled)
    } else {
        Ok(())
    }
}

/// Parse `#rrggbb`, `rrggbb`, `#rgb` or a few color words.
pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let t = s.trim().trim_start_matches('#').to_ascii_lowercase();
    match t.as_str() {
        "white" => return Some([255, 255, 255]),
        "black" => return Some([0, 0, 0]),
        "gray" | "grey" => return Some([128, 128, 128]),
        _ => {}
    }
    let hex = |h: &str| u8::from_str_radix(h, 16).ok();
    match t.len() {
        6 => Some([hex(&t[0..2])?, hex(&t[2..4])?, hex(&t[4..6])?]),
        3 => Some([
            hex(&t[0..1])? * 17,
            hex(&t[1..2])? * 17,
            hex(&t[2..3])? * 17,
        ]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_parse() {
        assert_eq!(parse_color("#ffffff"), Some([255, 255, 255]));
        assert_eq!(parse_color("0a141e"), Some([10, 20, 30]));
        assert_eq!(parse_color("#f00"), Some([255, 0, 0]));
        assert_eq!(parse_color("white"), Some([255, 255, 255]));
        assert_eq!(parse_color("nope"), None);
    }

    #[test]
    fn banded_scoring_matches_scoring_the_whole_picture() {
        // A picture taller than one band, so the band joins are exercised.
        let (w, h) = (96usize, 400usize);
        let mut v = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let t = ((x * 7 + y * 3) % 251) as u8;
                v.extend_from_slice(&[t, t.wrapping_mul(3), (x * 2 + y) as u8]);
            }
        }
        let img = Image {
            width: w as u32,
            height: h as u32,
            layout: Layout::Rgb,
            samples: Samples::U8(v),
            icc: None,
        };
        for q in [30.0, 60.0, 90.0] {
            let bytes = encode_bytes(
                &img,
                &EncodeSpec {
                    quality: Some(q),
                    ..EncodeSpec::new(Format::Jpeg)
                },
            )
            .unwrap();
            let banded = score_bytes(&img, bytes.clone()).unwrap();
            let whole = crate::ssim::score(&img, &decode_mem(bytes).unwrap());
            assert!(
                (banded.luma - whole.luma).abs() < 0.004,
                "q{q}: luma {:.4} against {:.4}",
                banded.luma,
                whole.luma
            );
            assert!(
                (banded.chroma - whole.chroma).abs() < 0.004,
                "q{q}: chroma {:.4} against {:.4}",
                banded.chroma,
                whole.chroma
            );
        }
    }

    #[test]
    fn size_lines_say_which_way() {
        assert!(size_change_line("a.jpg", 3_200_000, 610_000).contains("81% smaller"));
        assert!(size_change_line("a.jpg", 100_000, 120_000).contains("20% larger"));
    }
}

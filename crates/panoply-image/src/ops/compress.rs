//! `image.compress`: make an image smaller without it looking worse, or fit it under a size
//! (docs/specs/id-and-image.md 3.2 and 3.3).
//!
//! Default target: the smallest file whose SSIM against the decoded original is at least 0.97,
//! found by binary search on quality. SSIM here is luma SSIM and color SSIM together, and the
//! worse of the two must pass (see `crate::ssim`). Large pictures are searched on a sample of
//! tiles and the winner is verified on the whole picture. With `format = smallest` every
//! sensible format is tried (the "format shootout") within a time budget per image.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::color::OutSpace;
use crate::encode::{self, Embed};
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::pipeline::{Ctl, ImagePlan, decode_to_image};
use crate::quant;
use crate::ssim::{Reference, Score, estimate_jpeg_quality};
use crate::types::{Image, Layout, Samples};

use super::common::*;
use super::options::Opts;
use super::search::{Sample, largest_fitting, looks_photographic, smallest_passing};
use super::{Io, NameSpec, human_bytes};

/// Pictures with more pixels than this are compressed in strips (`big.rs`) instead of in memory.
///
/// The in-memory path holds the picture (3 bytes a pixel) and lets MozJPEG hold its coefficient
/// buffers for progressive scans and optimized tables (up to 6 more bytes a pixel), so 24
/// megapixels peak near 260 MB and 31 megapixels already pass the 300 MB cap. Above it the strip
/// path takes over: any size, within the cap, at a cost of well under one percent in file size
/// for a photograph (see `big.rs`). The environment variable `PANOPLY_IMAGE_STREAM_PX` moves the
/// limit, to compare the two paths on one picture.
pub const MAX_PIXELS: u64 = 24_000_000;

fn memory_pixel_limit() -> u64 {
    std::env::var("PANOPLY_IMAGE_STREAM_PX")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(MAX_PIXELS)
}
/// Time budget per image for a format shootout.
pub const SHOOTOUT_BUDGET: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy)]
pub(super) enum Mode {
    Smallest(f64),
    UnderSize(u64),
    Quality(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Jpeg,
    WebpLossy,
    WebpLossless,
    PngLossless,
    PngLossy,
    Avif,
    Heic,
    Tiff,
    Gif,
}

impl Kind {
    pub(super) fn format(self) -> Format {
        match self {
            Kind::Jpeg => Format::Jpeg,
            Kind::WebpLossy | Kind::WebpLossless => Format::WebP,
            Kind::PngLossless | Kind::PngLossy => Format::Png,
            Kind::Avif => Format::Avif,
            Kind::Heic => Format::Heic,
            Kind::Tiff => Format::Tiff,
            Kind::Gif => Format::Gif,
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Kind::Jpeg => "JPG",
            Kind::WebpLossy => "WebP",
            Kind::WebpLossless => "WebP lossless",
            Kind::PngLossless => "PNG",
            Kind::PngLossy => "PNG (reduced colors)",
            Kind::Avif => "AVIF",
            Kind::Heic => "HEIC",
            Kind::Tiff => "TIFF",
            Kind::Gif => "GIF",
        }
    }
}

/// One finished candidate.
struct Cand {
    kind: Kind,
    bytes: Vec<u8>,
    q: Option<u32>,
    score: Option<Score>,
}

/// Compress each input.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        check_cancel(io)?;
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Compressing {}", display_name(path)),
        );
        match compress_one(io, path, opts) {
            Ok(Some(p)) => outputs.push(p),
            Ok(None) => {}
            Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
            Err(e) if inputs.len() > 1 => {
                io.note(format!("{}: skipped. {e}", display_name(path)));
                first_err.get_or_insert(e);
            }
            Err(e) => return Err(e),
        }
    }
    if outputs.is_empty()
        && let Some(e) = first_err
    {
        return Err(e);
    }
    Ok(outputs)
}

fn mode_from(opts: &Opts) -> Result<Mode> {
    Ok(match opts.str("target").unwrap_or("smallest") {
        "under_size" => {
            let b = opts.bytes("max_size")?.ok_or_else(|| {
                ImageError::BadOption(
                    "Choose a size to stay under, for example max_size=200kb.".into(),
                )
            })?;
            Mode::UnderSize(b)
        }
        "quality" => Mode::Quality(opts.u32("quality").unwrap_or(80).clamp(1, 100)),
        _ => Mode::Smallest(opts.f64("ssim").unwrap_or(0.97)),
    })
}

pub(super) fn kinds_for(
    fmt: Option<Format>,
    src: Format,
    photographic: bool,
    has_alpha: bool,
    lossy_png: Option<bool>,
    os_avif: bool,
    shootout: bool,
) -> Vec<Kind> {
    let png_lossy = lossy_png.unwrap_or(!photographic);
    let one = |f: Format| -> Vec<Kind> {
        match f {
            Format::Jpeg => vec![Kind::Jpeg],
            Format::Png => {
                let mut v = vec![Kind::PngLossless];
                if png_lossy {
                    v.push(Kind::PngLossy);
                }
                v
            }
            Format::WebP => vec![Kind::WebpLossy, Kind::WebpLossless],
            Format::Avif => vec![Kind::Avif],
            Format::Heic => vec![Kind::Heic],
            Format::Tiff => vec![Kind::Tiff],
            Format::Gif => vec![Kind::Gif],
            _ => vec![],
        }
    };
    if shootout {
        let mut v = Vec::new();
        if !has_alpha {
            v.push(Kind::Jpeg);
        }
        v.push(Kind::WebpLossy);
        if !photographic {
            v.push(Kind::WebpLossless);
        }
        v.push(Kind::PngLossless);
        if png_lossy {
            v.push(Kind::PngLossy);
        }
        if os_avif {
            v.push(Kind::Avif);
        }
        return v;
    }
    let f = fmt.unwrap_or(src);
    let v = one(f);
    if v.is_empty() {
        // Formats Panoply reads but cannot write (JPEG XL, JPEG 2000, RAW, ...): try the usual ones.
        return kinds_for(
            None,
            Format::Png,
            photographic,
            has_alpha,
            lossy_png,
            os_avif,
            true,
        );
    }
    v
}

/// `img` composited on white. Only called for pictures with an alpha channel.
fn flatten_white(img: &Image) -> Image {
    let img8 = img.to_u8();
    let ch = img8.layout.channels();
    let Samples::U8(v) = &img8.samples else {
        unreachable!()
    };
    let gray = img8.layout.is_gray();
    let mut out = Vec::with_capacity(v.len() / ch * (ch - 1));
    for p in v.chunks_exact(ch) {
        let a = p[ch - 1] as u32;
        let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
        if gray {
            out.push(mix(p[0]));
        } else {
            out.extend_from_slice(&[mix(p[0]), mix(p[1]), mix(p[2])]);
        }
    }
    Image {
        width: img8.width,
        height: img8.height,
        layout: img8.layout.with_alpha(false),
        samples: Samples::U8(out),
        icc: img8.icc.clone(),
    }
}

type EncFn<'a> = dyn Fn(&Image, f32, &Embed) -> Result<Vec<u8>> + 'a;

struct Lossy<'a> {
    kind: Kind,
    enc: Box<EncFn<'a>>,
    qlo: u32,
    qhi: u32,
}

fn lossy_candidate(
    io: &dyn Io,
    c: &Lossy<'_>,
    img: &Image,
    mode: Mode,
    embed: &Embed,
) -> Result<Cand> {
    let none = Embed::default();
    match mode {
        Mode::Quality(q) => {
            let q = q.clamp(c.qlo, c.qhi);
            let bytes = (c.enc)(img, q as f32, embed)?;
            let score = score_bytes(img, bytes.clone()).ok();
            Ok(Cand {
                kind: c.kind,
                bytes,
                q: Some(q),
                score,
            })
        }
        Mode::UnderSize(max) => {
            // Quality moves in sixteenth steps, so the result lands close under the size asked
            // for (whole steps can jump several percent).
            const STEPS: u32 = 16;
            let at = |i: u32| i as f32 / STEPS as f32;
            let found = largest_fitting(c.qlo * STEPS, c.qhi * STEPS, |i| {
                check_cancel(io)?;
                Ok::<bool, ImageError>((c.enc)(img, at(i), embed)?.len() as u64 <= max)
            })?;
            let Some(i) = found else {
                let floor = (c.enc)(img, c.qlo as f32, embed)?.len() as u64;
                return Err(ImageError::Failed(format!(
                    "{} cannot get this picture under {}: the smallest it gets at its current size is {}. Make the picture smaller first, or allow a larger size.",
                    c.kind.label(),
                    human_bytes(max),
                    human_bytes(floor)
                )));
            };
            let bytes = (c.enc)(img, at(i), embed)?;
            let score = score_bytes(img, bytes.clone()).ok();
            Ok(Cand {
                kind: c.kind,
                bytes,
                q: Some((at(i)).round() as u32),
                score,
            })
        }
        Mode::Smallest(target) => {
            let sample = Sample::new(img);
            let mut q = smallest_passing(c.qlo, c.qhi, |q| {
                check_cancel(io)?;
                let bytes = (c.enc)(sample.image(), q as f32, &none)?;
                let dec = decode_mem(bytes)?;
                Ok::<bool, ImageError>(sample.score(&dec).worst() >= target)
            })?;
            let mut bytes = (c.enc)(img, q as f32, embed)?;
            let mut score = score_bytes(img, bytes.clone())?;
            // The sample can be a little optimistic: step quality up until the whole picture passes.
            let mut tries = 0;
            while score.worst() < target && q < c.qhi && tries < 8 {
                q = (q + 1 + tries / 3).min(c.qhi);
                bytes = (c.enc)(img, q as f32, embed)?;
                score = score_bytes(img, bytes.clone())?;
                tries += 1;
            }
            Ok(Cand {
                kind: c.kind,
                bytes,
                q: Some(q),
                score: Some(score),
            })
        }
    }
}

fn quant_image(src: &Image, q: &quant::Quantized) -> Image {
    let alpha = q.palette.iter().any(|c| c[3] != 255);
    let layout = if alpha { Layout::Rgba } else { Layout::Rgb };
    let ch = layout.channels();
    let mut out = Vec::with_capacity(q.indices.len() * ch);
    for &i in &q.indices {
        let c = q.palette[i as usize];
        out.extend_from_slice(&c[..ch]);
    }
    Image {
        width: src.width,
        height: src.height,
        layout,
        samples: Samples::U8(out),
        icc: src.icc.clone(),
    }
}

fn palette_candidate(
    io: &dyn Io,
    kind: Kind,
    img: &Image,
    mode: Mode,
    embed: &Embed,
) -> Result<Cand> {
    let img8 = img.to_u8();
    let encode_with = |colors: usize| -> Result<Vec<u8>> {
        match kind {
            Kind::Gif => {
                // The GIF encoder quantizes by itself to 256; for fewer colors reduce first.
                let q = quant::quantize(&img8, colors, 0.8);
                encode::gif::encode(&quant_image(&img8, &q), 0.0)
            }
            _ => {
                let q = quant::quantize(&img8, colors, 0.8);
                encode::png::encode_indexed(
                    img8.width,
                    img8.height,
                    &q.indices,
                    &q.palette,
                    embed,
                    encode::png::Effort::Normal,
                )
            }
        }
    };
    let reference = Reference::new(&img8);
    let score_of = |colors: usize| -> Score {
        let q = quant::quantize(&img8, colors, 0.8);
        reference.score(&quant_image(&img8, &q))
    };
    let colors = match mode {
        Mode::Smallest(t) => smallest_passing(8, 256, |c| {
            check_cancel(io)?;
            Ok::<bool, ImageError>(score_of(c as usize).worst() >= t)
        })? as usize,
        Mode::UnderSize(max) => {
            let c = largest_fitting(8, 256, |c| {
                check_cancel(io)?;
                Ok::<bool, ImageError>(encode_with(c as usize)?.len() as u64 <= max)
            })?;
            c.ok_or_else(|| {
                ImageError::Failed(format!(
                    "Even 8 colors do not get this picture under {}. Make it smaller first.",
                    human_bytes(max)
                ))
            })? as usize
        }
        Mode::Quality(q) => (8 + (q as usize * 248 / 100)).clamp(8, 256),
    };
    let bytes = encode_with(colors)?;
    let score = Some(score_of(colors));
    Ok(Cand {
        kind,
        bytes,
        q: Some(colors as u32),
        score,
    })
}

fn compress_one(io: &dyn Io, path: &std::path::Path, opts: &Opts) -> Result<Option<PathBuf>> {
    let started = Instant::now();
    let mode = mode_from(opts)?;
    let out_space = if opts.str("color_profile") == Some("srgb") {
        OutSpace::Srgb
    } else {
        OutSpace::Keep
    };
    let strip = opts.flag("strip_metadata", true);
    let animation = Animation::from_opt(opts.str("animation"));
    let fmt_opt = opts.str("format").unwrap_or("keep");
    let (shootout, forced) = match fmt_opt {
        "keep" => (false, None),
        "smallest" => (true, None),
        other => (false, Format::from_name(other)),
    };
    let lossy_png = match opts.str("lossy_png").unwrap_or("auto") {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };

    let mut loaded = load(io, path, None, animation, out_space)?;
    let info = loaded.info.clone();
    let name = loaded.name.clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (uw, uh) = crate::orient::oriented_size(o, info.width, info.height);
    let px = uw as u64 * uh as u64;
    if px > memory_pixel_limit() {
        return super::big::run(
            &super::big::Args {
                io,
                path,
                opts,
                mode,
                out_space,
                animation,
                shootout,
                forced,
                strip,
                lossy_png,
            },
            loaded,
        );
    }
    let progress = |f: f32| io.progress(f * 0.2, "Reading");
    let ctl = Ctl {
        cancel: io.cancel(),
        progress: &progress,
    };
    let (img, r) = decode_to_image(
        &mut *loaded.reader,
        &ImagePlan {
            out_space,
            ..Default::default()
        },
        &ctl,
    )?;
    let notes = r.notes.clone();
    let src_bytes = source_bytes(io, path);
    let src_meta = SourceMeta::read_for(
        src_bytes.as_deref(),
        if shootout {
            Format::Avif
        } else {
            forced.unwrap_or(info.format)
        },
    );
    let est_q = if info.format == Format::Jpeg {
        src_bytes.as_deref().and_then(estimate_jpeg_quality)
    } else {
        None
    };
    drop(src_bytes);
    let carried = carry(
        &src_meta,
        &info,
        img.width,
        img.height,
        img.icc.as_ref(),
        !strip,
        true,
        forced.unwrap_or(info.format),
    );
    let photographic = looks_photographic(&img);
    let has_alpha = img.layout.has_alpha() && !img.is_opaque();
    let os_avif = encode::os_can_encode(Format::Avif);
    let kinds = kinds_for(
        forced,
        info.format,
        photographic,
        has_alpha,
        lossy_png,
        os_avif,
        shootout || (forced.is_none() && !can_write(info.format)),
    );

    // One 8 bit copy of the picture is held (and a second, only for 16 bit sources that go to
    // PNG or TIFF, and a flattened one only when JPEG needs the alpha channel gone), so the peak
    // stays near the size of the picture itself.
    let wide = if img.depth() == crate::types::Depth::U16 {
        Some(img.clone())
    } else {
        None
    };
    let img8 = img.to_u8_owned();
    let jpeg_flat = if img8.layout.has_alpha() {
        Some(flatten_white(&img8))
    } else {
        None
    };
    let jpeg_img = jpeg_flat.as_ref().unwrap_or(&img8);
    let lossless_src = wide.as_ref().unwrap_or(&img8);
    let mut cands: Vec<Cand> = Vec::new();
    let mut tried: Vec<String> = Vec::new();
    let mut kinds = kinds;
    let mut fell_back = false;
    // Under a size with a format that cannot trade quality for bytes (a PNG photograph, TIFF, GIF)
    // and no format asked for: when it cannot fit, JPG (WebP when the picture has transparency) is
    // tried next, then WebP. They are told apart from the format the user asked for, which is kept.
    let mut size_fallbacks: Vec<Vec<Kind>> = match mode {
        Mode::UnderSize(_)
            if forced.is_none()
                && !shootout
                && matches!(info.format, Format::Png | Format::Tiff | Format::Gif) =>
        {
            if has_alpha {
                vec![vec![Kind::WebpLossy]]
            } else {
                vec![vec![Kind::Jpeg], vec![Kind::WebpLossy]]
            }
        }
        _ => Vec::new(),
    };
    size_fallbacks.reverse(); // popped from the end, in the order above
    let mut in_size_fallback = false;
    let mut smallest_seen: Option<(Kind, u64)> = None;
    loop {
        for (n, kind) in kinds.iter().enumerate() {
            check_cancel(io)?;
            if n > 0 && started.elapsed() > SHOOTOUT_BUDGET {
                tried.push(format!("{} skipped (time budget)", kind.label()));
                continue;
            }
            io.progress(
                0.2 + 0.8 * n as f32 / kinds.len() as f32,
                &format!("Trying {}", kind.label()),
            );
            let embed = &carried.embed;
            let result: Result<Cand> = match kind {
                Kind::Jpeg => {
                    // Searching above the quality the source was saved at only adds bytes, so the smallest
                    // target stops a little above it. A size or quality the user asked for does not.
                    let hi = match mode {
                        Mode::Smallest(_) => est_q
                            .map(|e| (e as u32 + 8).min(100))
                            .unwrap_or(100)
                            .max(30),
                        _ => 100,
                    };
                    let c = Lossy {
                        kind: *kind,
                        enc: Box::new(|i, q, e| {
                            encode_bytes(
                                i,
                                &EncodeSpec {
                                    quality: Some(q),
                                    embed: e.clone(),
                                    ..EncodeSpec::new(Format::Jpeg)
                                },
                            )
                        }),
                        qlo: match mode {
                            Mode::Smallest(target) => super::search::quality_floor(target, hi),
                            _ => 10,
                        },
                        qhi: hi,
                    };
                    lossy_candidate(io, &c, jpeg_img, mode, embed)
                }
                Kind::WebpLossy => {
                    let c = Lossy {
                        kind: *kind,
                        enc: Box::new(|i, q, e| {
                            encode_bytes(
                                i,
                                &EncodeSpec {
                                    quality: Some(q),
                                    embed: e.clone(),
                                    ..EncodeSpec::new(Format::WebP)
                                },
                            )
                        }),
                        qlo: 5,
                        qhi: 100,
                    };
                    lossy_candidate(io, &c, &img8, mode, embed)
                }
                Kind::Avif | Kind::Heic => {
                    let f = kind.format();
                    if !encode::os_can_encode(f) {
                        Err(ImageError::Unsupported(format!(
                            "{} compression needs the system's {} support, which this computer does not have. Choose JPG or WebP.",
                            f.label(),
                            f.label()
                        )))
                    } else {
                        let c = Lossy {
                            kind: *kind,
                            enc: Box::new(move |i, q, e| {
                                encode_bytes(
                                    i,
                                    &EncodeSpec {
                                        quality: Some(q),
                                        embed: e.clone(),
                                        ..EncodeSpec::new(f)
                                    },
                                )
                            }),
                            qlo: 5,
                            qhi: 95,
                        };
                        lossy_candidate(io, &c, &img8, mode, embed)
                    }
                }
                Kind::WebpLossless => encode_bytes(
                    &img8,
                    &EncodeSpec {
                        lossless: true,
                        embed: embed.clone(),
                        ..EncodeSpec::new(Format::WebP)
                    },
                )
                .map(|bytes| Cand {
                    kind: *kind,
                    bytes,
                    q: None,
                    score: None,
                }),
                Kind::PngLossless => encode_bytes(
                    lossless_src,
                    &EncodeSpec {
                        embed: embed.clone(),
                        png_effort: encode::png::Effort::Normal,
                        ..EncodeSpec::new(Format::Png)
                    },
                )
                .map(|bytes| Cand {
                    kind: *kind,
                    bytes,
                    q: None,
                    score: None,
                }),
                Kind::Tiff => encode_bytes(
                    lossless_src,
                    &EncodeSpec {
                        embed: embed.clone(),
                        ..EncodeSpec::new(Format::Tiff)
                    },
                )
                .map(|bytes| Cand {
                    kind: *kind,
                    bytes,
                    q: None,
                    score: None,
                }),
                Kind::PngLossy | Kind::Gif => {
                    if px > 8_000_000 {
                        Err(ImageError::TooLarge(
                            "Reducing colors is only offered up to 8 megapixels.".into(),
                        ))
                    } else {
                        palette_candidate(io, *kind, &img8, mode, embed)
                    }
                }
            };
            match result {
                Ok(c) => cands.push(c),
                Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
                Err(e) => {
                    if kinds.len() == 1
                        && !in_size_fallback
                        && !matches!(kind, Kind::Heic | Kind::Avif)
                    {
                        return Err(e);
                    }
                    tried.push(format!("{} not possible ({e})", kind.label()));
                }
            }
        }
        // The system's HEIC or AVIF encoder can refuse a picture (too many pixels, no encoder on
        // this computer). The file still gets smaller: fall back to JPG or WebP and say so.
        if cands.is_empty()
            && !fell_back
            && kinds.iter().all(|k| matches!(k, Kind::Heic | Kind::Avif))
        {
            fell_back = true;
            let why = tried.pop().unwrap_or_default();
            io.note(format!(
                "{name}: {why}, so the result is a JPG or WebP instead"
            ));
            kinds = kinds_for(
                None,
                Format::Png,
                photographic,
                has_alpha,
                lossy_png,
                os_avif,
                true,
            );
            kinds.retain(|k| !matches!(k, Kind::Avif | Kind::PngLossless | Kind::PngLossy));
            continue;
        }
        // Under a size, nothing fits, and the kept format cannot get smaller: try the next format.
        if let Mode::UnderSize(max) = mode {
            if let Some(c) = cands.iter().min_by_key(|c| c.bytes.len())
                && smallest_seen.is_none_or(|(_, b)| (c.bytes.len() as u64) < b)
            {
                smallest_seen = Some((c.kind, c.bytes.len() as u64));
            }
            if cands.iter().all(|c| c.bytes.len() as u64 > max)
                && let Some(next) = size_fallbacks.pop()
            {
                io.note(format!(
                    "{name}: {} cannot get under {}, so {} is tried instead",
                    info.format.label(),
                    human_bytes(max),
                    next[0].format().label()
                ));
                cands.clear();
                kinds = next;
                in_size_fallback = true;
                continue;
            }
        }
        break;
    }
    // Under a size: only candidates that fit count.
    if let Mode::UnderSize(max) = mode {
        cands.retain(|c| c.bytes.len() as u64 <= max);
    }
    let Some(best) = cands.iter().min_by_key(|c| c.bytes.len()) else {
        if let Mode::UnderSize(max) = mode {
            let floor = match smallest_seen {
                Some((k, b)) => format!(
                    "the smallest it gets is {} as {}",
                    human_bytes(b),
                    k.label()
                ),
                None => tried.last().cloned().unwrap_or_default(),
            };
            return Err(ImageError::Failed(format!(
                "{name} cannot get under {}: {floor}. Make the picture smaller first, or allow a larger size.",
                human_bytes(max)
            )));
        }
        return Err(ImageError::Failed(format!(
            "{name} could not be compressed with these settings. Try a larger size or another format."
        )));
    };
    if kinds.len() > 1 {
        let mut rows: Vec<String> = cands
            .iter()
            .map(|c| format!("{} {}", c.kind.label(), human_bytes(c.bytes.len() as u64)))
            .collect();
        rows.sort();
        io.note(format!("{name}: tried {}", rows.join(", ")));
        for t in &tried {
            io.note(format!("{name}: {t}"));
        }
    }
    let before = loaded.file_len;
    let after = best.bytes.len() as u64;
    let target_given = !matches!(mode, Mode::Smallest(_));
    // A format the user asked for is written even when it is larger: they wanted that format.
    let asked_other_format = forced.is_some_and(|f| f != info.format);
    if after >= before && !target_given && !asked_other_format {
        io.note(format!(
            "{name}: already well compressed. The best result was {} against the original {}, so nothing was written and the original stays as it is",
            human_bytes(after),
            human_bytes(before)
        ));
        return Ok(None);
    }
    let fmt = best.kind.format();
    let out_path = write_output(
        io,
        path,
        &NameSpec::Label("compressed".into()),
        fmt.ext(),
        &best.bytes,
    )?;
    let mut line = size_change_line(&name, before, after);
    if let Some(s) = best.score {
        line.push_str(&format!(
            ", similarity {:.3} (color {:.3})",
            s.luma, s.chroma
        ));
    }
    if let Some(q) = best.q {
        line.push_str(&match best.kind {
            Kind::PngLossy | Kind::Gif => format!(", {q} colors"),
            _ => format!(", quality {q}"),
        });
    }
    io.note(line);
    io.note(format!(
        "{name}: wrote {}",
        out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if fmt != info.format {
        io.note(format!(
            "{name}: changed format from {} to {}",
            info.format.label(),
            fmt.label()
        ));
    }
    if let Some(l) = kept_dropped_line(&carried) {
        io.note(format!("{name}: {l}"));
    }
    if best.kind == Kind::PngLossy && info.depth == crate::types::Depth::U16 {
        io.note(format!("{name}: 16 bit color was reduced to 8 bit"));
    }
    if o > 1 {
        io.note(format!(
            "{name}: applied the orientation tag ({})",
            super::convert::orientation_words(o)
        ));
    }
    if info.frames > 1 {
        io.note(format!("{name}: used the first frame of an animation"));
    }
    for n in notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(Some(out_path))
}

pub(super) fn can_write(f: Format) -> bool {
    matches!(
        f,
        Format::Jpeg
            | Format::Png
            | Format::WebP
            | Format::Avif
            | Format::Heic
            | Format::Tiff
            | Format::Gif
    )
}

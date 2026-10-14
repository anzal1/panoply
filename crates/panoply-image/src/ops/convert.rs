//! `image.convert`: turn an image into another format (docs/specs/id-and-image.md 3.1).
//!
//! The picture is decoded once through the pipeline (orientation applied, color managed, 16 bit
//! kept for PNG and TIFF, transparency flattened when the target has none) and encoded once.
//! Lossless to lossless paths copy pixel values unchanged. Metadata is kept by default and the
//! result card says what was kept and what was dropped, naming GPS whenever it is kept.

use std::io::BufWriter;
use std::path::PathBuf;

use crate::color::OutSpace;
use crate::encode::{self, Embed};
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::orient;
use crate::pipeline::{Ctl, FlipSink, ImagePlan, StreamPlan, decode_to_image, run_stream};
use crate::types::Depth;

use super::common::*;
use super::options::Opts;
use super::{Io, NameSpec, Output, human_bytes};

/// Pictures above this many pixels stream (JPEG and PNG targets only) instead of being held.
pub const STREAM_PIXELS: u64 = 24_000_000;

/// JPEG conversions of at least this many pixels overlap decoding and encoding.
const OVERLAP_PIXELS: u64 = 2_000_000;

/// A JPEG sink that works out what metadata to embed when the picture's size and color profile
/// are known (at `begin`), then encodes on its own thread.
struct LateJpeg<'a> {
    quality: f32,
    src_meta: &'a SourceMeta,
    info: &'a crate::decode::SrcInfo,
    keep_meta: bool,
    carried: Option<Carried>,
    thread: Option<encode::jpeg::JpegThread>,
}

impl crate::pipeline::RowSink for LateJpeg<'_> {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: crate::types::Layout,
        depth: Depth,
        icc: Option<std::sync::Arc<Vec<u8>>>,
    ) -> Result<()> {
        if depth != Depth::U8 || layout.has_alpha() {
            return Err(ImageError::Failed(
                "internal: JPEG needs 8 bit rows without alpha".into(),
            ));
        }
        let c = carry(
            self.src_meta,
            self.info,
            width,
            height,
            icc.as_ref(),
            self.keep_meta,
            true,
            Format::Jpeg,
        );
        let opts = encode::jpeg::JpegOpts {
            quality: self.quality,
            progressive: false,
            subsample: true,
            optimize: true,
            mozjpeg_tables: false,
            trellis: false,
            embed: c.embed.clone(),
        };
        self.carried = Some(c);
        self.thread = Some(encode::jpeg::JpegThread::start(opts, width, height, layout));
        Ok(())
    }

    fn push(&mut self, rows: crate::types::Rows<'_>) -> Result<()> {
        let crate::types::Rows::U8(s) = rows else {
            return Err(ImageError::Failed("internal: JPEG needs 8 bit rows".into()));
        };
        let t = self.thread.as_mut().expect("begin was called");
        t.push(s)
    }
}

/// Convert each input.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mut outputs = Vec::new();
    let mut first_err: Option<ImageError> = None;
    for (i, path) in inputs.iter().enumerate() {
        check_cancel(io)?;
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Converting {}", display_name(path)),
        );
        match convert_one(io, path, opts, i, inputs.len()) {
            Ok(p) => outputs.push(p),
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

fn convert_one(
    io: &dyn Io,
    path: &std::path::Path,
    opts: &Opts,
    idx: usize,
    total: usize,
) -> Result<PathBuf> {
    let to = Format::from_name(opts.str("to").unwrap_or("jpg")).ok_or_else(|| ImageError::BadOption("The target format is not one Panoply writes. Choose jpg, png, webp, avif, heic, tiff, bmp or gif.".into()))?;
    let quality = opts.f64("quality").map(|q| q as f32);
    let bg = parse_color(opts.str("flatten_color").unwrap_or("#ffffff")).ok_or_else(|| {
        ImageError::BadOption(
            "The background color is not a color. Use something like #ffffff.".into(),
        )
    })?;
    let keep_meta = opts.flag("keep_metadata", true);
    let out_space = if opts.str("color_profile") == Some("srgb") {
        OutSpace::Srgb
    } else {
        OutSpace::Keep
    };
    let animation = Animation::from_opt(opts.str("animation"));

    let mut loaded = load(io, path, None, animation, out_space)?;
    let info = loaded.info.clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (uw, uh) = orient::oriented_size(o, info.width, info.height);
    let pixels = uw as u64 * uh as u64;
    let per = 1.0 / total as f32;
    let base = idx as f32 * per;
    let progress = |f: f32| io.progress(base + f * per, "Converting");
    let ctl = Ctl {
        cancel: io.cancel(),
        progress: &progress,
    };

    // Streaming only for the targets that can write rows as they come. A picture that the
    // orientation tag would turn by 90 or 180 degrees (or mirror top to bottom) cannot be turned
    // a row at a time, so it keeps its tag: the pixels stay as stored and a small EXIF block says
    // how to show them (the card says so).
    let big = pixels > STREAM_PIXELS;
    let stream_ok = matches!(to, Format::Jpeg | Format::Png);
    if big && !stream_ok {
        return Err(ImageError::TooLarge(format!(
            "This picture is {uw} x {uh} pixels ({:.0} megapixels). Panoply can convert a picture that large only to JPG or PNG. Try JPG, or make the picture smaller first.",
            pixels as f64 / 1e6
        )));
    }
    let keep_tag = big && o > 2 && !info.upright;
    let tag_embed = || Embed {
        exif: keep_tag.then(|| crate::meta::jpeg_orientation_only_exif(o)),
        ..Embed::default()
    };

    let mut notes: Vec<String> = Vec::new();
    let src_meta = SourceMeta::read_for(source_bytes(io, path).as_deref(), to);
    let target_depth = depth_for(to, info.depth);
    let alpha_ok = to.has_alpha() && to != Format::Bmp;
    let flatten = if alpha_ok { None } else { Some(bg) };

    let carried;
    let (out_bytes, out_path) = if big {
        // Rows go straight from the decoder to the encoder.
        let mut out = Output::create(io, path, &NameSpec::Plain, to.ext())?;
        let plan = StreamPlan {
            out_space,
            flatten,
            out_depth: Some(target_depth),
            ..Default::default()
        };
        let file = out.file().try_clone().map_err(ImageError::from)?;
        let w = BufWriter::with_capacity(1 << 20, file);
        let result = match to {
            Format::Jpeg => {
                // Huge pictures cannot buffer coefficients for the whole picture, so strips of
                // rows are encoded on several threads (baseline, standard tables, MozJPEG's
                // quantization tables) and joined with restart markers, with no pass over the
                // compressed data. Any size, in the time of the encode divided by the threads.
                let scratch: std::cell::RefCell<Option<Output>> = std::cell::RefCell::new(None);
                let mut spill = || -> Result<std::fs::File> {
                    let mut o =
                        Output::create(io, path, &NameSpec::Label("working".into()), "tmp")?;
                    let f = o.file().try_clone().map_err(ImageError::from)?;
                    *scratch.borrow_mut() = Some(o);
                    Ok(f)
                };
                let jo = encode::jpeg::JpegOpts {
                    quality: quality.unwrap_or(85.0),
                    mozjpeg_tables: true,
                    embed: tag_embed(),
                    ..Default::default()
                };
                let threads = std::thread::available_parallelism()
                    .map_or(2, |n| n.get())
                    .clamp(1, 4);
                let mut sink = encode::jpegstrips::StripSink::with_profile(
                    encode::jpegstrips::Profile::Fast,
                    jo,
                    threads,
                    &mut spill,
                );
                let r = if o == 2 {
                    run_stream(
                        &mut *loaded.reader,
                        &plan,
                        &ctl,
                        &mut FlipSink::new(&mut sink),
                    )
                } else {
                    run_stream(&mut *loaded.reader, &plan, &ctl, &mut sink)
                };
                let mut w = w;
                let done = r.and_then(|r| {
                    let strips = sink.finish()?;
                    encode::jpegopt::join_restart(
                        &strips,
                        r.height as usize,
                        strips.strip_mcus,
                        &mut w,
                    )?;
                    std::io::Write::flush(&mut w).map_err(ImageError::from)?;
                    Ok(r)
                });
                drop(scratch);
                done
            }
            _ => {
                let mut sink = encode::png::stream(w, &tag_embed(), encode::png::Effort::Fast);
                let r = if o == 2 {
                    run_stream(
                        &mut *loaded.reader,
                        &plan,
                        &ctl,
                        &mut FlipSink::new(&mut sink),
                    )
                } else {
                    run_stream(&mut *loaded.reader, &plan, &ctl, &mut sink)
                };
                r.and_then(|r| sink.finish().map(|_| r))
            }
        };
        let r = result?;
        carried = carry(
            &src_meta,
            &info,
            r.width,
            r.height,
            r.icc.as_ref(),
            false,
            false,
            to,
        );
        notes.extend(r.notes);
        notes.push("streamed in rows because the picture is very large".into());
        (
            out.file().metadata().map(|m| m.len()).unwrap_or(0),
            out.commit(),
        )
    } else if to == Format::Jpeg && (o <= 2 || info.upright) && pixels >= OVERLAP_PIXELS {
        // JPEG from a picture that needs no turning: the encoder runs on its own thread while
        // the decoder produces the next rows, so the job takes as long as the slower of the two.
        // The file is the one the in-memory path writes.
        let plan = StreamPlan {
            out_space,
            flatten,
            out_depth: Some(Depth::U8),
            ..Default::default()
        };
        let mut late = LateJpeg {
            quality: quality.unwrap_or_else(|| EncodeSpec::default_quality(Format::Jpeg)),
            src_meta: &src_meta,
            info: &info,
            keep_meta,
            carried: None,
            thread: None,
        };
        let r = if o == 2 {
            run_stream(
                &mut *loaded.reader,
                &plan,
                &ctl,
                &mut FlipSink::new(&mut late),
            )
        } else {
            run_stream(&mut *loaded.reader, &plan, &ctl, &mut late)
        };
        let r = r?;
        notes.extend(r.notes.clone());
        carried = late.carried.take().unwrap_or_default();
        let bytes = late
            .thread
            .take()
            .ok_or_else(|| ImageError::Failed("internal: the JPEG encoder never started".into()))?
            .finish()?;
        (
            bytes.len() as u64,
            write_output(io, path, &NameSpec::Plain, to.ext(), &bytes)?,
        )
    } else {
        let plan = ImagePlan {
            out_space,
            flatten,
            out_depth: Some(target_depth),
            ..Default::default()
        };
        let (mut img, r) = decode_to_image(&mut *loaded.reader, &plan, &ctl)?;
        notes.extend(r.notes.clone());
        if img.layout.has_alpha() && alpha_ok && img.is_opaque() {
            img = img.without_alpha();
        }
        carried = carry(
            &src_meta,
            &info,
            img.width,
            img.height,
            img.icc.as_ref(),
            keep_meta,
            true,
            to,
        );
        let mut spec = EncodeSpec::new(to);
        spec.quality = quality;
        spec.lossless = opts.flag("lossless", false);
        // An instant conversion: baseline JPEG with optimized tables and plain PNG deflate run at
        // the speed of the native tools. Compress is where the full MozJPEG and oxipng effort go.
        spec.progressive = false;
        spec.png_effort = encode::png::Effort::Balanced;
        spec.embed = carried.embed.clone();
        let bytes = encode_bytes(&img, &spec)?;
        (
            bytes.len() as u64,
            write_output(io, path, &NameSpec::Plain, to.ext(), &bytes)?,
        )
    };

    if keep_tag {
        notes.push(format!(
            "kept the orientation tag ({}) instead of turning the picture, which needs the whole picture in memory",
            orientation_words(o)
        ));
    } else if o > 1 {
        notes.push(format!(
            "applied the orientation tag ({})",
            orientation_words(o)
        ));
    }
    if info.frames > 1 {
        notes.push("used the first frame of an animation".into());
    }
    io.note(format!(
        "{}: {}",
        loaded.name,
        size_change_line("", loaded.file_len, out_bytes).trim_start_matches(": ")
    ));
    io.note(format!(
        "{}: wrote {}",
        loaded.name,
        out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if let Some(line) = kept_dropped_line(&carried) {
        io.note(format!("{}: {line}", loaded.name));
    }
    for n in notes {
        io.note(format!("{}: {n}", loaded.name));
    }
    let _ = human_bytes;
    let _ = Depth::U8;
    Ok(out_path)
}

/// "rotated 90 degrees clockwise", for the card.
pub fn orientation_words(o: u8) -> &'static str {
    match o {
        2 => "mirrored left to right",
        3 => "rotated 180 degrees",
        4 => "mirrored top to bottom",
        5 => "transposed",
        6 => "rotated 90 degrees clockwise",
        7 => "transversed",
        8 => "rotated 90 degrees counterclockwise",
        _ => "none",
    }
}

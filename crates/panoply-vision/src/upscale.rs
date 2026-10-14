//! `image.upscale`: make a small or soft picture bigger and sharper (docs/specs/id-and-image.md
//! 3.11).
//!
//! The model is Real-ESRGAN general x4v3 (BSD-3-Clause, 4.5 MB): a fixed 128 x 128 tile in, a
//! 512 x 512 tile out. The picture is cut into 96 pixel cores with 16 pixels of context on every
//! side (mirrored at the picture's edge), each tile is upscaled and only its core kept, and
//! finished rows go straight to the PNG or JPEG writer one strip at a time. Memory therefore
//! scales with the strip, not with the result: a 4x result of a 4 megapixel picture (64 MP) is
//! never held whole. A 2x result is the 4x tile averaged down by exact 2 x 2 boxes, which is the
//! same quality as scaling by 4 and shrinking, and cannot leave seams between strips.
//!
//! Transparency is upscaled separately by bilinear sampling (IPC-8), since a model trained on
//! color pictures would invent edges in it.

use std::fs::File;
use std::path::PathBuf;
use std::time::Instant;

use panoply_image::encode::Embed;
use panoply_image::encode::jpeg::{JpegOpts, JpegSink};
use panoply_image::encode::png::{Effort, stream};
use panoply_image::error::{ImageError, Result};
use panoply_image::format::Format;
use panoply_image::ops::common::{
    Animation, SourceMeta, carry, display_name, kept_dropped_line, source_bytes,
};
use panoply_image::ops::options::Opts;
use panoply_image::ops::{NameSpec, human_bytes, with_commas};
use panoply_image::pipeline::RowSink;
use panoply_image::ssim::estimate_jpeg_quality;
use panoply_image::types::{Depth, Layout, Rows};

use crate::VisionIo;
use crate::models::ESRGAN;
use crate::runtime::{self, In, Loaded, Out, Prefer};
use crate::work::{Source, file_name, load_source};

/// The model's input side.
const TILE_IN: usize = 128;
/// Context on each side of a tile's core, in input pixels.
const MARGIN: usize = 16;
/// The part of a tile that is kept.
const CORE: usize = TILE_IN - 2 * MARGIN;
/// The model's scale.
const MODEL_SCALE: usize = 4;
/// The largest result, in pixels: PNG and JPEG writing plus the strip stay inside the budget.
pub const MAX_OUTPUT_PIXELS: u64 = 120_000_000;
/// The largest input, in pixels. Beyond this a picture is already large and gains little.
const MAX_INPUT_PIXELS: usize = 16_000_000;
/// Above this long side an `auto` scale refuses (the spec: say "no benefit").
const BIG_SIDE: usize = 2000;

/// Run the module on each input.
pub fn run(io: &dyn VisionIo, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let _scope = runtime::JobScope;
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        if io.cancel().is_cancelled() {
            return Err(ImageError::Cancelled);
        }
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Upscaling {}", display_name(path)),
        );
        match one(io, path, opts, i, inputs.len()) {
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

/// The scale to use for a picture of `w x h`: the option, or 4 for a small picture and 2 for a
/// larger one. `Err` says why not.
pub fn pick_scale(choice: Option<&str>, w: usize, h: usize) -> Result<usize> {
    let long = w.max(h);
    let scale = match choice {
        Some("2") => 2,
        Some("4") => 4,
        _ => {
            if long > BIG_SIDE {
                return Err(ImageError::Unsupported(format!(
                    "This picture is already {} x {}. Upscaling it gains little, so nothing was done. Set scale to 2 to do it anyway.",
                    with_commas(w as u64),
                    with_commas(h as u64)
                )));
            }
            if long < 1000 { 4 } else { 2 }
        }
    };
    let out_px = (w * scale) as u64 * (h * scale) as u64;
    if out_px > MAX_OUTPUT_PIXELS {
        return Err(ImageError::TooLarge(format!(
            "The result would be {} x {} ({} megapixels), over the limit of {}. Choose scale 2, or make the picture smaller first.",
            with_commas((w * scale) as u64),
            with_commas((h * scale) as u64),
            out_px / 1_000_000,
            MAX_OUTPUT_PIXELS / 1_000_000
        )));
    }
    Ok(scale)
}

/// Removes the output file if the job does not reach the end.
struct Partial {
    path: PathBuf,
    keep: bool,
}

impl Drop for Partial {
    fn drop(&mut self) {
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

fn one(
    io: &dyn VisionIo,
    path: &std::path::Path,
    opts: &Opts,
    idx: usize,
    total: usize,
) -> Result<PathBuf> {
    let started = Instant::now();
    let animation = Animation::from_opt(opts.str("animation"));
    let src = load_source(io, path, animation, MAX_INPUT_PIXELS)?;
    let name = src.name.clone();
    if src.w * src.h < src.full_w * src.full_h {
        return Err(ImageError::TooLarge(format!(
            "{name} is {} x {}, which is already large; upscaling it would not help and would not fit the memory limit. Make it smaller first, or leave it as it is.",
            with_commas(src.full_w as u64),
            with_commas(src.full_h as u64)
        )));
    }
    let (w, h) = (src.w, src.h);
    let scale = pick_scale(opts.str("scale"), w, h)?;
    let (ow, oh) = (w * scale, h * scale);
    let format = match opts.str("format") {
        Some("png") => Format::Png,
        Some("jpg") => Format::Jpeg,
        _ => match src.info.format {
            Format::Jpeg => Format::Jpeg,
            _ => Format::Png,
        },
    };
    let model = runtime::acquire(io, &ESRGAN, Prefer::Accelerated)?;

    // Metadata for the new size, then the output file.
    let src_bytes = source_bytes(io, path);
    let meta = SourceMeta::read_for(src_bytes.as_deref(), format);
    let carried = carry(
        &meta,
        &src.info,
        ow as u32,
        oh as u32,
        None,
        opts.flag("keep_metadata", true),
        true,
        format,
    );
    let (out_path, file) =
        io.create_output(path, &NameSpec::Label(format!("{scale}x")), format.ext())?;
    let mut partial = Partial {
        path: out_path.clone(),
        keep: false,
    };
    let jpeg_quality = opts
        .f64("quality")
        .map(|q| q as f32)
        .or_else(|| {
            src_bytes
                .as_deref()
                .filter(|_| src.info.format == Format::Jpeg)
                .and_then(estimate_jpeg_quality)
                .map(|q| q.clamp(80.0, 95.0))
        })
        .unwrap_or(92.0);
    let mut sink = Writer::new(file, format, &carried.embed, jpeg_quality);
    let layout = if src.alpha.is_some() && format.has_alpha() {
        Layout::Rgba
    } else {
        Layout::Rgb
    };
    sink.begin(ow as u32, oh as u32, layout, Depth::U8, None)?;

    upscale_into(io, &model, &src, scale, layout, &mut sink, |f| {
        io.progress(
            (idx as f32 + f) / total as f32,
            &format!("Upscaling {name} {scale}x"),
        );
    })?;
    sink.finish()?;
    partial.keep = true;
    let written = std::fs::metadata(&out_path).map_or(0, |m| m.len());

    let secs = started.elapsed().as_secs_f64();
    io.note(format!(
        "{name}: {} x {} to {} x {} in {secs:.1} s ({}), {}",
        with_commas(w as u64),
        with_commas(h as u64),
        with_commas(ow as u64),
        with_commas(oh as u64),
        model.provider,
        human_bytes(written)
    ));
    io.note(format!(
        "{name}: tiled with overlap, no seams, wrote {}",
        file_name(&out_path)
    ));
    if let Some(l) = kept_dropped_line(&carried) {
        io.note(format!("{name}: {l}"));
    }
    if src.alpha.is_some() && layout == Layout::Rgb {
        io.note(format!(
            "{name}: transparency was flattened onto white (JPG cannot hold it); choose png to keep it"
        ));
    }
    for n in src.notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(out_path)
}

/// A streaming PNG or JPEG writer behind one type.
enum Writer {
    Png(Box<panoply_image::encode::png::PngStream<File>>),
    Jpeg(Box<JpegSink<File>>, u64),
}

impl Writer {
    fn new(file: File, format: Format, embed: &Embed, jpeg_quality: f32) -> Writer {
        match format {
            Format::Jpeg => Writer::Jpeg(
                Box::new(JpegSink::new(
                    file,
                    JpegOpts {
                        quality: jpeg_quality,
                        progressive: false,
                        subsample: true,
                        optimize: false,
                        mozjpeg_tables: false,
                        trellis: false,
                        embed: embed.clone(),
                    },
                )),
                0,
            ),
            _ => Writer::Png(Box::new(stream(file, embed, Effort::Balanced))),
        }
    }

    fn finish(self) -> Result<()> {
        match self {
            Writer::Png(p) => p.finish(),
            Writer::Jpeg(j, _) => j.finish().map(|_| ()),
        }
    }
}

impl RowSink for Writer {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        icc: Option<std::sync::Arc<Vec<u8>>>,
    ) -> Result<()> {
        match self {
            Writer::Png(p) => p.begin(width, height, layout, depth, icc),
            Writer::Jpeg(j, _) => j.begin(width, height, layout, depth, icc),
        }
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        match self {
            Writer::Png(p) => p.push(rows),
            Writer::Jpeg(j, n) => {
                *n += rows.len() as u64;
                j.push(rows)
            }
        }
    }
}

/// Reflects an index into `0..n` (mirror without repeating the edge pixel).
#[inline]
fn reflect(i: isize, n: usize) -> usize {
    let n = n as isize;
    if n == 1 {
        return 0;
    }
    let period = 2 * (n - 1);
    let mut m = i.rem_euclid(period);
    if m >= n {
        m = period - m;
    }
    m as usize
}

/// Upscales `src` by `scale` (2 or 4) and pushes the finished rows into `sink`, strip by strip.
pub fn upscale_into(
    io: &dyn VisionIo,
    model: &Loaded,
    src: &Source,
    scale: usize,
    layout: Layout,
    sink: &mut dyn RowSink,
    progress: impl Fn(f32),
) -> Result<()> {
    let (w, h) = (src.w, src.h);
    let strip_w = w * MODEL_SCALE;
    let out_w = w * scale;
    let tiles_x = w.div_ceil(CORE);
    let tiles_y = h.div_ceil(CORE);
    let mut strip = vec![0u8; CORE * MODEL_SCALE * strip_w * 3];
    for ty in 0..tiles_y {
        let cy = ty * CORE;
        let core_h = CORE.min(h - cy);
        for tx in 0..tiles_x {
            if io.cancel().is_cancelled() {
                return Err(ImageError::Cancelled);
            }
            let cx = tx * CORE;
            let core_w = CORE.min(w - cx);
            // The tile's input: 128 x 128 around the core, mirrored at the picture's edge.
            let mut input = vec![0f32; 3 * TILE_IN * TILE_IN];
            let plane = TILE_IN * TILE_IN;
            for y in 0..TILE_IN {
                let sy = reflect(cy as isize - MARGIN as isize + y as isize, h);
                for x in 0..TILE_IN {
                    let sx = reflect(cx as isize - MARGIN as isize + x as isize, w);
                    let p = &src.rgb[(sy * w + sx) * 3..(sy * w + sx) * 3 + 3];
                    input[y * TILE_IN + x] = f32::from(p[0]) / 255.0;
                    input[plane + y * TILE_IN + x] = f32::from(p[1]) / 255.0;
                    input[2 * plane + y * TILE_IN + x] = f32::from(p[2]) / 255.0;
                }
            }
            let out = model.run(
                vec![In::F32(
                    "image",
                    vec![1, 3, TILE_IN as i64, TILE_IN as i64],
                    input,
                )],
                &["upscaled_image"],
            )?;
            let Some(Out::F32(_, up)) = out.into_iter().next() else {
                return Err(ImageError::Failed(
                    "The upscale model returned an unexpected answer. Try again.".into(),
                ));
            };
            let side = TILE_IN * MODEL_SCALE;
            if up.len() != 3 * side * side {
                return Err(ImageError::Failed(
                    "The upscale model returned a tile of the wrong size. Try again.".into(),
                ));
            }
            let up_plane = side * side;
            let off = MARGIN * MODEL_SCALE;
            for y in 0..core_h * MODEL_SCALE {
                let drow = y * strip_w + cx * MODEL_SCALE;
                for x in 0..core_w * MODEL_SCALE {
                    let s = (off + y) * side + off + x;
                    let d = (drow + x) * 3;
                    strip[d] = (up[s].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    strip[d + 1] = (up[up_plane + s].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                    strip[d + 2] = (up[2 * up_plane + s].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
            progress((ty * tiles_x + tx + 1) as f32 / (tiles_x * tiles_y) as f32);
        }
        // Finished rows of this strip, at the requested scale.
        let rows4 = core_h * MODEL_SCALE;
        let rows_out = core_h * scale;
        let mut buf = Vec::with_capacity(rows_out * out_w * layout.channels());
        for oy in 0..rows_out {
            // Rows of the strip this output row averages.
            let (y0, y1) = if scale == 4 {
                (oy, oy + 1)
            } else {
                (oy * 2, oy * 2 + 2)
            };
            for ox in 0..out_w {
                let (x0, x1) = if scale == 4 {
                    (ox, ox + 1)
                } else {
                    (ox * 2, ox * 2 + 2)
                };
                let mut acc = [0u32; 3];
                for yy in y0..y1.min(rows4) {
                    for xx in x0..x1 {
                        let p = &strip[(yy * strip_w + xx) * 3..(yy * strip_w + xx) * 3 + 3];
                        acc[0] += u32::from(p[0]);
                        acc[1] += u32::from(p[1]);
                        acc[2] += u32::from(p[2]);
                    }
                }
                let n = ((y1.min(rows4) - y0) * (x1 - x0)) as u32;
                let px = [
                    ((acc[0] + n / 2) / n) as u8,
                    ((acc[1] + n / 2) / n) as u8,
                    ((acc[2] + n / 2) / n) as u8,
                ];
                if let Some(a) = src.alpha.as_deref() {
                    // Bilinear alpha at the output pixel's center.
                    let fx = (ox as f32 + 0.5) / scale as f32 - 0.5;
                    let fy = ((cy * scale + oy) as f32 + 0.5) / scale as f32 - 0.5;
                    let av = sample_u8(a, w, h, fx, fy);
                    if layout == Layout::Rgba {
                        buf.extend_from_slice(&[px[0], px[1], px[2], av]);
                    } else {
                        let al = u32::from(av);
                        for c in px {
                            buf.push(((u32::from(c) * al + 255 * (255 - al) + 127) / 255) as u8);
                        }
                    }
                } else {
                    buf.extend_from_slice(&px);
                }
            }
        }
        sink.push(Rows::U8(&buf))?;
        progress((ty + 1) as f32 / tiles_y as f32);
    }
    Ok(())
}

fn sample_u8(a: &[u8], w: usize, h: usize, x: f32, y: f32) -> u8 {
    let x = x.clamp(0.0, (w - 1) as f32);
    let y = y.clamp(0.0, (h - 1) as f32);
    let (x0, y0) = (x as usize, y as usize);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let top = f32::from(a[y0 * w + x0]) * (1.0 - fx) + f32::from(a[y0 * w + x1]) * fx;
    let bot = f32::from(a[y1 * w + x0]) * (1.0 - fx) + f32::from(a[y1 * w + x1]) * fx;
    (top * (1.0 - fy) + bot * fy + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflection_mirrors_without_repeating_the_edge() {
        assert_eq!(reflect(-1, 10), 1);
        assert_eq!(reflect(-3, 10), 3);
        assert_eq!(reflect(10, 10), 8);
        assert_eq!(reflect(12, 10), 6);
        assert_eq!(reflect(5, 10), 5);
        assert_eq!(reflect(-100, 1), 0);
        // A reflection further than one picture away is still inside.
        for i in -50..60 {
            assert!(reflect(i, 7) < 7);
        }
    }

    #[test]
    fn scale_rules() {
        assert_eq!(pick_scale(None, 800, 600).unwrap(), 4);
        assert_eq!(pick_scale(None, 1600, 1200).unwrap(), 2);
        assert!(pick_scale(None, 4000, 3000).is_err());
        assert_eq!(pick_scale(Some("2"), 4000, 3000).unwrap(), 2);
        assert_eq!(pick_scale(Some("4"), 1000, 1000).unwrap(), 4);
        // 4x of 6000 x 4000 is 384 MP.
        assert!(pick_scale(Some("4"), 6000, 4000).is_err());
        let e = pick_scale(None, 4000, 3000).unwrap_err().to_string();
        assert!(e.contains("already 4,000 x 3,000"), "{e}");
    }

    #[test]
    fn the_tile_geometry_is_consistent() {
        assert_eq!(CORE + 2 * MARGIN, TILE_IN);
        assert_eq!(TILE_IN * MODEL_SCALE, 512);
    }

    #[test]
    fn alpha_samples_bilinearly() {
        let a = [0u8, 255];
        assert_eq!(sample_u8(&a, 2, 1, 0.0, 0.0), 0);
        assert_eq!(sample_u8(&a, 2, 1, 0.5, 0.0), 128);
        assert_eq!(sample_u8(&a, 2, 1, 5.0, 0.0), 255);
    }
}

//! `image.resize`: make an image a given size (docs/specs/id-and-image.md 3.5).
//!
//! Resampling runs in linear light with premultiplied alpha (Lanczos3 shrinking, Mitchell
//! enlarging), and large JPEGs are first decoded at a reduced size by the decoder itself (the
//! shrink-on-load libvips does), always keeping at least twice the target so the final filter
//! still does the fine work. The picture is decoded once and encoded once.

use std::path::PathBuf;

use crate::color::OutSpace;
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::orient;
use crate::pipeline::{Ctl, ImagePlan, decode_to_image};
use crate::resample::Filter;
use crate::ssim::estimate_jpeg_quality;

use super::common::*;
use super::fit::{Fit, finish, scaled_size};
use super::options::Opts;
use super::{Io, NameSpec};

/// Resize each input.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        check_cancel(io)?;
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Resizing {}", display_name(path)),
        );
        match resize_one(io, path, opts, i, inputs.len()) {
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

/// The box and fit the options ask for, for a picture of `w` by `h`.
fn target_box(opts: &Opts, w: u32, h: u32) -> Result<(u32, u32, Fit)> {
    let fit = match opts.str("fit").unwrap_or("contain") {
        "cover" => Fit::Cover { anchor_y: 0.5 },
        "pad" => Fit::Pad,
        "stretch" => Fit::Stretch,
        _ => Fit::Contain,
    };
    let need = |k: &str| {
        opts.u32(k).ok_or_else(|| {
            ImageError::BadOption(format!("Set {k} to say how big the result should be."))
        })
    };
    match opts.str("mode").unwrap_or("longest") {
        "percent" => {
            let p = opts.f64("percent").ok_or_else(|| {
                ImageError::BadOption(
                    "Set percent to say how big the result should be, for example 50.".into(),
                )
            })?;
            let (bw, bh) = (
                ((w as f64 * p / 100.0).round() as u32).max(1),
                ((h as f64 * p / 100.0).round() as u32).max(1),
            );
            Ok((bw, bh, Fit::Stretch))
        }
        "pixels" => {
            let (bw, bh) = (opts.u32("width"), opts.u32("height"));
            match (bw, bh) {
                (Some(bw), Some(bh)) => Ok((bw, bh, fit)),
                (Some(bw), None) => Ok((
                    bw,
                    ((h as f64 * bw as f64 / w as f64).round() as u32).max(1),
                    Fit::Stretch,
                )),
                (None, Some(bh)) => Ok((
                    ((w as f64 * bh as f64 / h as f64).round() as u32).max(1),
                    bh,
                    Fit::Stretch,
                )),
                (None, None) => Err(ImageError::BadOption(
                    "Set width, height or both to say how big the result should be.".into(),
                )),
            }
        }
        _ => {
            let long = need("longest")?;
            let s = long as f64 / w.max(h) as f64;
            Ok((
                ((w as f64 * s).round() as u32).max(1),
                ((h as f64 * s).round() as u32).max(1),
                Fit::Stretch,
            ))
        }
    }
}

fn resize_one(
    io: &dyn Io,
    path: &std::path::Path,
    opts: &Opts,
    idx: usize,
    total: usize,
) -> Result<PathBuf> {
    let out_space = if opts.str("color_profile") == Some("srgb") {
        OutSpace::Srgb
    } else {
        OutSpace::Keep
    };
    let animation = Animation::from_opt(opts.str("animation"));
    let enlarge = opts.flag("enlarge", false);
    let keep_meta = opts.flag("keep_metadata", true);
    let bg = parse_color(opts.str("background").unwrap_or("#ffffff")).ok_or_else(|| {
        ImageError::BadOption(
            "The background color is not a color. Use something like #ffffff.".into(),
        )
    })?;

    let mut loaded = load(io, path, None, animation, out_space)?;
    let src_info = loaded.info.clone();
    let o = if src_info.upright {
        1
    } else {
        src_info.orientation
    };
    let (uw, uh) = orient::oriented_size(o, src_info.full_width, src_info.full_height);
    let (bw, bh, fit) = target_box(opts, uw, uh)?;
    let (sw, sh) = scaled_size(uw, uh, bw, bh, fit, enlarge);
    // Reopen with a size hint so a big JPEG decodes at a reduced size.
    if src_info.format.decodes_reduced() && (sw < uw || sh < uh) {
        loaded = load(io, path, Some((sw, sh)), animation, out_space)?;
    }
    let info = loaded.info.clone();
    let target_format = Format::from_name(
        opts.str("format")
            .filter(|f| *f != "keep")
            .unwrap_or("keep"),
    )
    .unwrap_or(src_info.format);
    let target_format = if matches!(
        target_format,
        Format::Heic
            | Format::Avif
            | Format::Jpeg
            | Format::Png
            | Format::WebP
            | Format::Tiff
            | Format::Bmp
            | Format::Gif
    ) {
        target_format
    } else {
        Format::Png
    };
    let alpha_ok = target_format.has_alpha() && target_format != Format::Bmp;
    let flatten = if alpha_ok { None } else { Some(bg) };
    let scale = if (sw, sh) != (uw, uh) {
        Some((sw, sh))
    } else {
        None
    };
    let per = 1.0 / total as f32;
    let progress = |f: f32| io.progress((idx as f32 + f) * per, "Resizing");
    let ctl = Ctl {
        cancel: io.cancel(),
        progress: &progress,
    };
    let plan = ImagePlan {
        out_space,
        scale,
        filter: scale.map(|_| Filter::auto(uw, uh, sw, sh)),
        flatten,
        out_depth: Some(depth_for(target_format, info.depth)),
    };
    let (img, r) = decode_to_image(&mut *loaded.reader, &plan, &ctl)?;
    let img = finish(img, bw, bh, fit, bg);
    let src_meta = SourceMeta::read_for(source_bytes(io, path).as_deref(), target_format);
    let carried = carry(
        &src_meta,
        &info,
        img.width,
        img.height,
        img.icc.as_ref(),
        keep_meta,
        true,
        target_format,
    );

    let mut spec = EncodeSpec::new(target_format);
    spec.embed = carried.embed.clone();
    // Same speed profile as Convert: baseline JPEG with optimized tables, plain PNG deflate.
    spec.progressive = false;
    spec.png_effort = crate::encode::png::Effort::Balanced;
    spec.quality = opts.f64("quality").map(|q| q as f32).or_else(|| {
        // Keep a JPEG near the quality it came in at, and never above 90 (the picture is smaller).
        if target_format == Format::Jpeg && src_info.format == Format::Jpeg {
            source_bytes(io, path)
                .and_then(|b| estimate_jpeg_quality(&b))
                .map(|q| q.clamp(60.0, 90.0))
        } else {
            Some(match target_format {
                Format::Jpeg => 90.0,
                Format::WebP => 85.0,
                _ => EncodeSpec::default_quality(target_format),
            })
        }
    });
    let bytes = encode_bytes(&img, &spec)?;
    let label = format!("{}x{}", img.width, img.height);
    let out_path = write_output(
        io,
        path,
        &NameSpec::Label(label),
        target_format.ext(),
        &bytes,
    )?;
    let name = &loaded.name;
    io.note(format!(
        "{name}: {} x {} to {} x {}, {}",
        uw,
        uh,
        img.width,
        img.height,
        size_change_line("", loaded.file_len, bytes.len() as u64).trim_start_matches(": ")
    ));
    if !enlarge && ((bw > uw && bh >= uh) || (bh > uh && bw >= uw)) {
        io.note(format!(
            "{name}: not enlarged (the picture is already smaller than the target)"
        ));
    }
    io.note(format!(
        "{name}: wrote {}",
        out_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    if let Some(l) = kept_dropped_line(&carried) {
        io.note(format!("{name}: {l}"));
    }
    if o > 1 {
        io.note(format!(
            "{name}: applied the orientation tag ({})",
            super::convert::orientation_words(o)
        ));
    }
    if src_info.frames > 1 {
        io.note(format!("{name}: used the first frame of an animation"));
    }
    for n in r.notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(out_path)
}

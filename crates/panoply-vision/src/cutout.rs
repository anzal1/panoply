//! `image.remove_background`: cut the subject out (docs/specs/id-and-image.md 3.8 and 3.10).
//!
//! Engine ladder (ADR 0004): on macOS the operating system's subject finder (Apple Vision) runs
//! first and costs no download and no resident memory; where it is missing, finds nothing, or
//! the user asks for the model, IS-Net general use (int8, 44 MB, MIT) runs through ONNX Runtime.
//! On every platform the same refine and edge cleanup follow: a color guided filter snaps the
//! mask onto the picture's own edges at full resolution, and a foreground estimate removes the
//! old background's color from the soft edge, so the cutout sits on any new background without a
//! halo.

use std::path::PathBuf;
use std::time::Instant;

use panoply_core::CancelToken;
use panoply_image::error::{ImageError, Result};
use panoply_image::format::Format;
use panoply_image::ops::common::{Animation, display_name, parse_color};
use panoply_image::ops::human_bytes;
use panoply_image::ops::options::Opts;

use crate::VisionIo;
use crate::matte::{Refine, foreground_colors, refine_alpha};
use crate::models::ISNET;
use crate::px::{Kernel, reduce_map, resize_u8, sample, to_linear, to_srgb};
use crate::runtime::{self, In, Out, Prefer};
use crate::work::{
    MAX_WORK_PIXELS, Source, WriteSpec, file_name, image_from, load_source, parse_format,
    write_result,
};

/// Which engine finds the subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Engine {
    /// The operating system's engine when there is one that finds a subject, else the model.
    Auto,
    /// The operating system's engine only.
    Os,
    /// The IS-Net model only.
    Model,
}

/// The mask for one picture and what produced it.
pub struct Matte {
    /// One byte a pixel at the working size: 0 clear, 255 solid.
    pub alpha: Vec<u8>,
    /// "Apple Vision" or "IS-Net".
    pub engine: &'static str,
}

/// What the result looks like: the part of the picture that is not the subject.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// The subject on transparency.
    Transparent,
    /// The subject on a solid color.
    Color,
    /// The subject on a blurred copy of the picture.
    Blur,
}

/// Prints a stage timing and the resident size to stderr when `PANOPLY_VISION_TRACE` is set (for
/// measuring where time and memory go).
fn trace(stage: &str, since: Instant) {
    if std::env::var_os("PANOPLY_VISION_TRACE").is_some() {
        let rss = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &std::process::id().to_string()])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .unwrap_or(0);
        eprintln!(
            "trace: {stage} at {:.2} s, resident {} MB",
            since.elapsed().as_secs_f64(),
            rss / 1024
        );
    }
}

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
            &format!("Cutting out {}", display_name(path)),
        );
        match one(io, path, opts) {
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

fn parse_engine(s: Option<&str>) -> Engine {
    match s {
        Some("os") => Engine::Os,
        Some("model") => Engine::Model,
        _ => Engine::Auto,
    }
}

fn one(io: &dyn VisionIo, path: &std::path::Path, opts: &Opts) -> Result<PathBuf> {
    let started = Instant::now();
    let result = match opts.str("result") {
        Some("color") => Look::Color,
        Some("blur") => Look::Blur,
        _ => Look::Transparent,
    };
    let bg = parse_color(opts.str("background").unwrap_or("#ffffff")).ok_or_else(|| {
        ImageError::BadOption(
            "One of the options is not right: the background is not a color. Use something like #ffffff.".into(),
        )
    })?;
    let blur_strength = opts.f64("blur").unwrap_or(30.0).clamp(1.0, 100.0);
    let engine = parse_engine(opts.str("engine"));
    let refine = opts.flag("edge_refine", true);
    let decontaminate = opts.flag("decontaminate", true);
    let animation = Animation::from_opt(opts.str("animation"));

    let mut src = load_source(io, path, animation, MAX_WORK_PIXELS)?;
    trace("decoded", started);
    let name = src.name.clone();
    let format = {
        let f = parse_format(opts.str("format").filter(|f| *f != "auto"), src.info.format);
        match (result, f) {
            (Look::Transparent, Format::Jpeg) if opts.str("format") == Some("jpg") => {
                return Err(ImageError::BadOption(
                    "JPG cannot hold transparency. Choose png or webp for a transparent cutout, or choose a color or blur background.".into(),
                ));
            }
            (Look::Transparent, Format::WebP) => Format::WebP,
            (Look::Transparent, _) => Format::Png,
            (_, f) => f,
        }
    };
    if src.w * src.h < src.full_w * src.full_h {
        io.note(format!(
            "{name}: worked at {} x {} (the picture is {} x {}) to stay inside the memory limit",
            src.w, src.h, src.full_w, src.full_h
        ));
    }

    let mut matte = find_matte(io, &src, engine)?;
    let t_matte = started.elapsed();
    trace("subject found", started);
    let mut alpha = std::mem::take(&mut matte.alpha);
    if refine {
        refine_alpha(
            &src.rgb,
            src.w,
            src.h,
            &mut alpha,
            Refine::for_size(src.w, src.h),
            io.cancel(),
        )?;
    }
    trace("edge refined", started);
    // The picture's own transparency stays: a cutout of a cutout does not bring things back.
    if let Some(own) = src.alpha.as_deref() {
        for (a, &o) in alpha.iter_mut().zip(own) {
            *a = ((u32::from(*a) * u32::from(o) + 127) / 255) as u8;
        }
    }
    let covered = alpha.iter().filter(|&&a| a > 127).count() as f64 / alpha.len().max(1) as f64;
    let fg = if decontaminate {
        foreground_colors(&src.rgb, &alpha, src.w, src.h, io.cancel())?
    } else {
        src.rgb.clone()
    };
    trace("edge colors", started);
    let (w, h) = (src.w, src.h);
    // The original pixels are needed again only for the blur background; every other result
    // gives them back now, so the peak stays at the composed picture plus one working copy.
    let original = if result == Look::Blur {
        std::mem::take(&mut src.rgb)
    } else {
        src.rgb = Vec::new();
        Vec::new()
    };

    let (img, label) = match result {
        Look::Transparent => {
            let mut rgb = fg;
            if !decontaminate {
                for (p, &a) in rgb.chunks_exact_mut(3).zip(&alpha) {
                    if a == 0 {
                        p.fill(0);
                    }
                }
            }
            (image_from(w, h, &rgb, Some(&alpha)), "cutout")
        }
        Look::Color => {
            let out = compose_color(&fg, &alpha, w, h, bg);
            drop(fg);
            (image_from(w, h, &out, None), "new bg")
        }
        Look::Blur => {
            let sigma = (blur_strength / 100.0 * w.max(h) as f64 * 0.02) as f32;
            let out = compose_blur(&original, &fg, &alpha, w, h, sigma, io.cancel())?;
            drop((fg, original));
            (image_from(w, h, &out, None), "new bg")
        }
    };
    drop(alpha);
    trace("composed", started);
    let (out_path, bytes) = write_result(
        io,
        &img,
        &WriteSpec {
            src: &src,
            label,
            format,
            quality: opts.f64("quality"),
            keep_metadata: false,
        },
    )?;
    let secs = started.elapsed().as_secs_f64();
    io.note(format!(
        "{name}: cutout done in {secs:.1} s with {} ({:.1} s to find the subject), {}",
        matte.engine,
        t_matte.as_secs_f64(),
        human_bytes(bytes as u64)
    ));
    if covered < 0.003 {
        io.note(format!(
            "{name}: almost nothing was kept. Try engine=model, or check that the picture has a clear subject"
        ));
    }
    io.note(format!(
        "{name}: edge refined at full resolution{}, wrote {}",
        if decontaminate {
            ", old background color removed from the edge"
        } else {
            ""
        },
        file_name(&out_path)
    ));
    for n in src.notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(out_path)
}

/// Finds the subject with the engine ladder.
pub fn find_matte(io: &dyn VisionIo, src: &Source, engine: Engine) -> Result<Matte> {
    if engine != Engine::Model {
        match os_matte(src) {
            Ok(Some(m)) => return Ok(m),
            Ok(None) | Err(_) if engine == Engine::Auto => {
                io.note(format!(
                    "{}: the system subject finder found nothing usable; used the IS-Net model",
                    src.name
                ));
            }
            Ok(None) => {
                return Err(ImageError::Unsupported(
                    "The system subject finder is not available here. Use engine=model to run the IS-Net model instead.".into(),
                ));
            }
            Err(e) => return Err(e),
        }
    }
    model_matte(io, src)
}

/// The operating system's subject finder, when this platform has one: `Ok(None)` when it is not
/// present or found no subject.
#[allow(unused_variables)]
pub fn os_matte(src: &Source) -> Result<Option<Matte>> {
    #[cfg(target_os = "macos")]
    {
        if !panoply_platform_macos::foreground_mask_available() {
            return Ok(None);
        }
        let flat = flatten_for_model(src);
        let full: &[u8] = flat.as_deref().unwrap_or(&src.rgb);
        // The system analyses at a much lower resolution than a photograph has, so a copy at
        // 2048 px gives the same mask for a fraction of the memory and time; the full
        // resolution edge comes from the refine step afterwards.
        let long = src.w.max(src.h);
        let (aw, ah) = if long > 2048 {
            let s = 2048.0 / long as f64;
            (
                ((src.w as f64 * s).round() as usize).max(1),
                ((src.h as f64 * s).round() as usize).max(1),
            )
        } else {
            (src.w, src.h)
        };
        let small;
        let input: &[u8] = if (aw, ah) == (src.w, src.h) {
            full
        } else {
            small = resize_u8(full, src.w, src.h, 3, aw, ah, Kernel::Lanczos3)?;
            &small
        };
        let mask = match panoply_platform_macos::foreground_instance_mask(
            input, aw as u32, ah as u32, 3,
        ) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };
        let alpha = if (mask.width as usize, mask.height as usize) == (src.w, src.h) {
            mask.alpha
        } else {
            resize_u8(
                &mask.alpha,
                mask.width as usize,
                mask.height as usize,
                1,
                src.w,
                src.h,
                Kernel::CatmullRom,
            )?
        };
        let covered = alpha.iter().filter(|&&a| a > 127).count() as f64 / alpha.len() as f64;
        if !(0.002..=0.985).contains(&covered) {
            return Ok(None);
        }
        Ok(Some(Matte {
            alpha,
            engine: "Apple Vision",
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(None)
    }
}

/// The picture flattened on white when it has its own transparency, else `None` (use as is).
fn flatten_for_model(src: &Source) -> Option<Vec<u8>> {
    let a = src.alpha.as_deref()?;
    let mut out = src.rgb.clone();
    for (p, &al) in out.chunks_exact_mut(3).zip(a) {
        let al = u32::from(al);
        for c in p {
            *c = ((u32::from(*c) * al + 255 * (255 - al) + 127) / 255) as u8;
        }
    }
    Some(out)
}

/// The IS-Net model: the picture at 1024 x 1024, one sigmoid map back, stretched to 0 to 1 and
/// scaled up to the picture's size.
pub fn model_matte(io: &dyn VisionIo, src: &Source) -> Result<Matte> {
    const SIDE: usize = 1024;
    // CoreML was measured on this model (macOS 26, M-series) and is no faster than the CPU
    // (1.5 s against 1.2 s at 1024 px) and holds more memory, so the CPU runs it everywhere.
    let model = runtime::acquire(io, &ISNET, Prefer::Cpu)?;
    io.progress(0.3, "Finding the subject");
    let rgb = flatten_for_model(src);
    let small = resize_u8(
        rgb.as_deref().unwrap_or(&src.rgb),
        src.w,
        src.h,
        3,
        SIDE,
        SIDE,
        Kernel::Bilinear,
    )?;
    let mut tensor = vec![0f32; 3 * SIDE * SIDE];
    for (i, p) in small.chunks_exact(3).enumerate() {
        for k in 0..3 {
            tensor[k * SIDE * SIDE + i] = (f32::from(p[k]) - 128.0) / 256.0;
        }
    }
    drop(small);
    let out = model.run(
        vec![In::F32(
            "input",
            vec![1, 3, SIDE as i64, SIDE as i64],
            tensor,
        )],
        &["output"],
    )?;
    let Some(Out::F32(shape, map)) = out.into_iter().next() else {
        return Err(ImageError::Failed(
            "The background model returned an unexpected answer. Try again.".into(),
        ));
    };
    if map.len() != SIDE * SIDE {
        return Err(ImageError::Failed(format!(
            "The background model returned a {shape:?} map, not {SIDE} x {SIDE}. Try again."
        )));
    }
    let (lo, hi) = map
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = (hi - lo).max(1e-6);
    let small_mask: Vec<u8> = map
        .iter()
        .map(|&v| (((v - lo) / span) * 255.0 + 0.5) as u8)
        .collect();
    let alpha = resize_u8(&small_mask, SIDE, SIDE, 1, src.w, src.h, Kernel::CatmullRom)?;
    Ok(Matte {
        alpha,
        engine: "IS-Net",
    })
}

/// The subject over a solid color, in linear light (IPC-2: alpha composites in linear light, or
/// soft edges get a dark fringe).
pub fn compose_color(fg: &[u8], alpha: &[u8], w: usize, h: usize, bg: [u8; 3]) -> Vec<u8> {
    let bgl = [to_linear(bg[0]), to_linear(bg[1]), to_linear(bg[2])];
    let mut out = Vec::with_capacity(w * h * 3);
    for (p, &a) in fg.chunks_exact(3).zip(alpha) {
        match a {
            255 => out.extend_from_slice(p),
            0 => out.extend_from_slice(&bg),
            _ => {
                let af = f32::from(a) / 255.0;
                for k in 0..3 {
                    out.push(to_srgb(to_linear(p[k]) * af + bgl[k] * (1.0 - af)));
                }
            }
        }
    }
    out
}

/// The subject over a blurred copy of the picture. The blur is made from the background only
/// (normalized convolution), so the subject's colors never bleed into it.
pub fn compose_blur(
    original: &[u8],
    fg: &[u8],
    alpha: &[u8],
    w: usize,
    h: usize,
    sigma: f32,
    cancel: &CancelToken,
) -> Result<Vec<u8>> {
    let f = (w.max(h) / 800).max(1);
    let s_lo = (sigma / f as f32).max(0.5);
    let weight_of = |i: usize| 1.0 - f32::from(alpha[i]) / 255.0;
    // Weight of the background and the background's linear colors times that weight, on a
    // reduced grid. Blurring both the same way and dividing gives the blur of the background
    // alone.
    let (mut wb, lw, lh) = reduce_map(w, h, f, weight_of);
    crate::px::gaussian_blur(&mut wb, lw, lh, s_lo);
    let mut planes: [Vec<f32>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for (k, plane) in planes.iter_mut().enumerate() {
        let (mut lo, _, _) = reduce_map(w, h, f, |i| to_linear(original[i * 3 + k]) * weight_of(i));
        crate::px::gaussian_blur(&mut lo, lw, lh, s_lo);
        *plane = lo;
    }
    // Where no background is near (the subject fills the picture), fall back to a plain blur of
    // the whole picture there.
    let lacking: Vec<usize> = (0..lw * lh).filter(|&i| wb[i] <= 1e-3).collect();
    for (k, plane) in planes.iter_mut().enumerate() {
        for (v, &wt) in plane.iter_mut().zip(&wb) {
            if wt > 1e-3 {
                *v /= wt;
            }
        }
        if !lacking.is_empty() {
            let (mut all, _, _) = reduce_map(w, h, f, |i| to_linear(original[i * 3 + k]));
            crate::px::gaussian_blur(&mut all, lw, lh, s_lo);
            for &i in &lacking {
                plane[i] = all[i];
            }
        }
    }
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        if y % 256 == 0 && cancel.is_cancelled() {
            return Err(ImageError::Cancelled);
        }
        let fy = (y as f32 + 0.5) / f as f32 - 0.5;
        for x in 0..w {
            let i = y * w + x;
            let a = alpha[i];
            if a == 255 {
                out.extend_from_slice(&fg[i * 3..i * 3 + 3]);
                continue;
            }
            let fx = (x as f32 + 0.5) / f as f32 - 0.5;
            let af = f32::from(a) / 255.0;
            for (k, plane) in planes.iter().enumerate() {
                let b = sample(plane, lw, lh, fx, fy);
                out.push(to_srgb(to_linear(fg[i * 3 + k]) * af + b * (1.0 - af)));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_color_composite_is_exact_at_the_ends_and_mixes_in_linear_light() {
        let fg = [255u8, 0, 0, 255, 0, 0, 255, 0, 0];
        let alpha = [255u8, 0, 128];
        let out = compose_color(&fg, &alpha, 3, 1, [0, 0, 255]);
        assert_eq!(&out[0..3], &[255, 0, 0]);
        assert_eq!(&out[3..6], &[0, 0, 255]);
        // Half red over blue in linear light is brighter than the 128 average of sRGB values.
        assert!(out[6] > 170 && out[8] > 170, "{:?}", &out[6..9]);
    }

    #[test]
    fn blur_background_never_takes_subject_color() {
        // Left half is a red subject (solid), right half a green background.
        let (w, h) = (64usize, 32usize);
        let mut rgb = Vec::new();
        let mut alpha = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                if x < 32 {
                    rgb.extend_from_slice(&[230, 20, 20]);
                    alpha[y * w + x] = 255;
                } else {
                    rgb.extend_from_slice(&[20, 200, 20]);
                }
            }
        }
        let cancel = CancelToken::new();
        let out = compose_blur(&rgb, &rgb, &alpha, w, h, 6.0, &cancel).unwrap();
        // A pixel just inside the background: still green, no red bleed.
        let i = (16 * w + 34) * 3;
        assert!(out[i] < 40, "red leaked: {:?}", &out[i..i + 3]);
        assert!(out[i + 1] > 150);
    }

    #[test]
    fn engine_names_parse() {
        assert_eq!(parse_engine(Some("os")), Engine::Os);
        assert_eq!(parse_engine(Some("model")), Engine::Model);
        assert_eq!(parse_engine(None), Engine::Auto);
    }
}

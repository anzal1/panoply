//! `image.resize_to_kb`: make a photo an exact pixel size and land its file size inside a window
//! such as 20 to 50 KB, the way exam and government portals demand
//! (docs/specs/id-and-image.md 2.3 and 2.4).
//!
//! The encoder is searched, not guessed: quality is binary searched on real encodes until the
//! largest size inside the window is found. If the window's minimum is above what the pixels
//! reach, chroma goes to 4:4:4, and only as the last step the file is padded with a JPEG comment
//! segment (and the card says so). 1000 versus 1024 bytes per KB is explicit: a preset says which
//! reading its portal uses, and when nobody knows the target is a size that is inside the window
//! under both readings.

use std::path::{Path, PathBuf};

use crate::color::OutSpace;
use crate::encode::{self, Embed};
use crate::error::{ImageError, Result};
use crate::format::Format;
use crate::orient;
use crate::pipeline::{
    Ctl, ImagePlan, ImageRows, ImageSink, StreamPlan, decode_to_image, quiet, run_stream,
};
use crate::presets::{self, Preset, Window};
use crate::resample::Filter;
use crate::types::{Depth, Image};

use super::common::*;
use super::fit::{Fit, finish, scaled_size};
use super::options::Opts;
use super::search::largest_fitting;
use super::{Io, NameSpec, human_bytes, with_commas};

/// Pictures decoded for a free-size search are first brought to at most this long side.
const FREE_START_LONG: u32 = 4000;

/// What to produce.
#[derive(Debug, Clone)]
pub struct Spec {
    /// Exact pixel size, or `None` to keep the aspect ratio and let the size search shrink.
    pub pixels: Option<(u32, u32)>,
    /// The byte window, or `None` for no size rule.
    pub window: Option<Window>,
    /// Output format (JPEG, WebP or PNG).
    pub format: Format,
    /// Background for padding and for flattening transparency.
    pub background: [u8; 3],
    /// Baseline JPEG only.
    pub baseline: bool,
    /// Auto, crop, pad or stretch.
    pub fit: FitChoice,
    /// Vertical crop anchor, 0.0 keeps the top.
    pub anchor_y: f32,
    /// Allow padding with a comment segment to reach a minimum.
    pub pad_to_min: bool,
    /// The preset this came from, for the card.
    pub preset: Option<&'static Preset>,
    /// KB readings to print (min and max as published, in KB).
    pub published: Option<(f64, f64)>,
}

/// How the picture meets the exact size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitChoice {
    /// Crop, or pad when the aspect ratios differ by more than 15 percent.
    Auto,
    /// Crop to fill.
    Crop,
    /// Pad with the background.
    Pad,
    /// Stretch.
    Stretch,
}

/// A finished result.
#[derive(Debug, Clone)]
pub struct Solution {
    /// The file bytes.
    pub bytes: Vec<u8>,
    /// Final pixel width.
    pub width: u32,
    /// Final pixel height.
    pub height: u32,
    /// Quality used.
    pub quality: u32,
    /// Chroma 4:2:0 (true) or 4:4:4.
    pub subsampled: bool,
    /// Bytes of comment padding added to reach the minimum, 0 when none.
    pub padded_by: usize,
    /// Dimensions were reduced to reach the size (free size mode).
    pub downscaled: bool,
    /// Lines for the card.
    pub notes: Vec<String>,
}

/// Build the spec from the options, with the preset (if any) as the defaults.
pub fn spec_from(opts: &Opts) -> Result<Spec> {
    let preset = match opts.str("preset") {
        Some(id) if !id.is_empty() => Some(presets::find(id)?),
        _ => None,
    };
    let (ow, oh) = (opts.u32("width"), opts.u32("height"));
    let pixels = match (ow, oh) {
        (Some(w), Some(h)) => Some((w, h)),
        (None, None) => preset.and_then(|p| p.pixels()),
        _ => {
            return Err(ImageError::BadOption(
                "Give both width and height, or neither (then the picture keeps its shape).".into(),
            ));
        }
    };
    let basis = opts.str("kb_basis");
    // "both" is asked for by name: inside the window under either reading, even for a preset
    // whose portal states a reading. Left out, a preset's own reading (else both) is used.
    let basis_override = basis.map(|b| if b == "both" { "unknown" } else { b });
    let (min_kb, max_kb) = (opts.f64("min_kb"), opts.f64("max_kb"));
    let (window, published) = if min_kb.is_some() || max_kb.is_some() {
        let (mn, mx) = (min_kb.unwrap_or(0.0), max_kb.unwrap_or(0.0));
        (
            Some(presets::window(
                mn,
                mx,
                "KB",
                basis_override.unwrap_or("unknown"),
            )?),
            Some((mn, mx)),
        )
    } else if let Some(p) = preset.filter(|p| p.has_size_rule()) {
        (
            Some(p.window(basis_override)?),
            Some((p.min_bytes_claimed, p.max_bytes_claimed)),
        )
    } else {
        (None, None)
    };
    let fmt_name = opts
        .str("format")
        .map(str::to_string)
        .or_else(|| preset.map(|p| p.format.clone()))
        .unwrap_or_else(|| "jpg".into());
    let format = match Format::from_name(&fmt_name) {
        Some(f @ (Format::Jpeg | Format::WebP | Format::Png)) => f,
        _ => return Err(ImageError::BadOption("Resize to exact KB writes JPG, WebP or PNG. Choose one of them (PDF sizing is a different module).".into())),
    };
    let bg_text = opts
        .str("background")
        .map(str::to_string)
        .or_else(|| preset.and_then(|p| p.background.clone()))
        .unwrap_or_else(|| "white".into());
    let background = parse_color(&bg_text).ok_or_else(|| {
        ImageError::BadOption(
            "The background color is not a color. Use something like #ffffff.".into(),
        )
    })?;
    // Baseline for presets (portals that reject progressive JPEG), progressive otherwise.
    let baseline = opts
        .bool("baseline")
        .or_else(|| preset.map(|p| p.baseline_only))
        .unwrap_or(false);
    let fit = match opts.str("fit").unwrap_or("auto") {
        "crop" => FitChoice::Crop,
        "pad" => FitChoice::Pad,
        "stretch" => FitChoice::Stretch,
        _ => FitChoice::Auto,
    };
    let anchor_y = if opts.str("anchor") == Some("top") {
        0.15
    } else {
        0.5
    };
    Ok(Spec {
        pixels,
        window,
        format,
        background,
        baseline,
        fit,
        anchor_y,
        pad_to_min: opts.flag("pad_to_min", true),
        preset,
        published,
    })
}

/// Resize each input.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let spec = spec_from(opts)?;
    let animation = Animation::from_opt(opts.str("animation"));
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, path) in inputs.iter().enumerate() {
        check_cancel(io)?;
        io.progress(
            i as f32 / inputs.len() as f32,
            &format!("Sizing {}", display_name(path)),
        );
        match one(io, path, &spec, animation) {
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

fn one(io: &dyn Io, path: &Path, spec: &Spec, animation: Animation) -> Result<PathBuf> {
    let name = display_name(path);
    let (base, load_notes) = prepare(io, path, spec, animation)?;
    let sol = solve(&base, spec, io)?;
    let label = format!(
        "{}x{}, {}KB",
        sol.width,
        sol.height,
        (sol.bytes.len() + 500) / 1000
    );
    let out = write_output(
        io,
        path,
        &NameSpec::Label(label),
        spec.format.ext(),
        &sol.bytes,
    )?;
    for n in load_notes {
        io.note(format!("{name}: {n}"));
    }
    io.note(format!(
        "{name}: wrote {}",
        out.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));
    for n in &sol.notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(out)
}

/// Decode and fit the picture to the exact pixel size (or to a bounded size for free mode).
pub fn prepare(
    io: &dyn Io,
    path: &Path,
    spec: &Spec,
    animation: Animation,
) -> Result<(Image, Vec<String>)> {
    let mut notes = Vec::new();
    let mut loaded = load(io, path, None, animation, OutSpace::Srgb)?;
    let info = loaded.info.clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (uw, uh) = orient::oriented_size(o, info.full_width, info.full_height);
    let flatten = Some(spec.background);
    let (cancel_io, progress) = (io.cancel(), |f: f32| io.progress(f * 0.3, "Reading"));
    let ctl = Ctl {
        cancel: cancel_io,
        progress: &progress,
    };
    let (box_w, box_h, fit, sw, sh) = match spec.pixels {
        Some((bw, bh)) => {
            let fit = match spec.fit {
                FitChoice::Crop => Fit::Cover {
                    anchor_y: spec.anchor_y,
                },
                FitChoice::Pad => Fit::Pad,
                FitChoice::Stretch => Fit::Stretch,
                FitChoice::Auto => {
                    let (a, b) = (uw as f64 / uh as f64, bw as f64 / bh as f64);
                    if (a - b).abs() / b > 0.15 {
                        notes.push("the picture's shape differs from the target by more than 15 percent, so it was padded instead of cropped".into());
                        Fit::Pad
                    } else {
                        Fit::Cover {
                            anchor_y: spec.anchor_y,
                        }
                    }
                }
            };
            let (sw, sh) = scaled_size(uw, uh, bw, bh, fit, true);
            (bw, bh, fit, sw, sh)
        }
        None => {
            let long = uw.max(uh);
            let s = if long > FREE_START_LONG {
                FREE_START_LONG as f64 / long as f64
            } else {
                1.0
            };
            let (sw, sh) = (
                ((uw as f64 * s).round() as u32).max(1),
                ((uh as f64 * s).round() as u32).max(1),
            );
            (sw, sh, Fit::Contain, sw, sh)
        }
    };
    if info.format.decodes_reduced() && (sw < uw || sh < uh) {
        loaded = load(io, path, Some((sw, sh)), animation, OutSpace::Srgb)?;
    }
    let scale = if (sw, sh) != (uw, uh) {
        Some((sw, sh))
    } else {
        None
    };
    let plan = ImagePlan {
        out_space: OutSpace::Srgb,
        scale,
        filter: scale.map(|_| Filter::auto(uw, uh, sw, sh)),
        flatten,
        out_depth: Some(Depth::U8),
    };
    let (img, r) = decode_to_image(&mut *loaded.reader, &plan, &ctl)?;
    notes.extend(r.notes);
    if o > 1 {
        notes.push(format!(
            "applied the orientation tag ({})",
            super::convert::orientation_words(o)
        ));
    }
    if info.frames > 1 {
        notes.push("used the first frame of an animation".into());
    }
    Ok((finish(img, box_w, box_h, fit, spec.background), notes))
}

/// Resample an in-memory image to a new size.
pub fn resample_image(img: &Image, w: u32, h: u32) -> Result<Image> {
    if (w, h) == (img.width, img.height) {
        return Ok(img.clone());
    }
    let mut reader = ImageRows::new(img);
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let plan = StreamPlan {
        scale: Some((w, h)),
        filter: Some(Filter::auto(img.width, img.height, w, h)),
        ..Default::default()
    };
    let mut sink = ImageSink::new();
    run_stream(&mut reader, &plan, &ctl, &mut sink)?;
    sink.into_image()
}

fn enc(img: &Image, spec: &Spec, q: u32, subsample: bool, progressive: bool) -> Result<Vec<u8>> {
    let mut s = EncodeSpec::new(spec.format);
    s.quality = Some(q as f32);
    s.subsample = subsample;
    s.progressive = progressive;
    s.embed = Embed::default();
    // Exam presets want no color profile and no EXIF: nothing is embedded.
    encode_bytes(img, &s)
}

/// Find the encoding of `base` that lands inside the spec's window.
pub fn solve(base: &Image, spec: &Spec, io: &dyn Io) -> Result<Solution> {
    let mut notes = Vec::new();
    let window = spec.window;
    let progressive_opts: &[bool] = if spec.baseline {
        &[false]
    } else {
        &[true, false]
    };
    let hi = window.and_then(|w| w.hi);
    let lo = window.map(|w| w.lo).unwrap_or(0);

    // No size rule: one sensible encode.
    let Some(win) = window else {
        let bytes = enc(base, spec, 90, true, !spec.baseline)?;
        notes.push(format!(
            "{} x {} px, {} bytes",
            base.width,
            base.height,
            with_commas(bytes.len() as u64)
        ));
        return Ok(Solution {
            bytes,
            width: base.width,
            height: base.height,
            quality: 90,
            subsampled: true,
            padded_by: 0,
            downscaled: false,
            notes,
        });
    };

    // PNG has no quality knob: lossless, then fewer colors.
    if spec.format == Format::Png {
        return solve_png(base, spec, win, notes);
    }

    let try_size = |img: &Image, sub: bool, prog: bool| -> Result<Option<(u32, Vec<u8>)>> {
        let q = largest_fitting(1, 100, |q| {
            check_cancel(io)?;
            Ok::<bool, ImageError>(hi.is_none_or(|h| {
                enc(img, spec, q, sub, prog)
                    .map(|b| b.len() as u64 <= h)
                    .unwrap_or(false)
            }))
        })?;
        match q {
            Some(q) => Ok(Some((q, enc(img, spec, q, sub, prog)?))),
            None => Ok(None),
        }
    };

    // Candidates at the exact size: the best quality that fits, for each way of encoding. Plain
    // 4:2:0 first, 4:4:4 only when that finds nothing inside the window.
    let mut best: Option<(u32, bool, Vec<u8>)> = None;
    let mut below_min: Option<(u32, bool, Vec<u8>)> = None;
    for &sub in &[true, false] {
        let mut in_window: Vec<(u32, bool, Vec<u8>)> = Vec::new();
        for &prog in progressive_opts {
            if let Some((q, bytes)) = try_size(base, sub, prog)? {
                if bytes.len() as u64 >= lo {
                    in_window.push((q, sub, bytes));
                } else if below_min
                    .as_ref()
                    .is_none_or(|(_, _, b)| bytes.len() > b.len())
                {
                    below_min = Some((q, sub, bytes));
                }
            }
        }
        best = in_window
            .into_iter()
            .max_by_key(|(q, _, b)| (*q, std::cmp::Reverse(b.len())));
        if best.is_some() {
            break;
        }
    }
    // With no pixel size to hold, a size that only fits at a poor quality is a bad answer: the
    // picture is made smaller until a decent quality fits, which is what a person would do.
    let poor = spec.pixels.is_none() && best.as_ref().is_some_and(|(q, _, _)| *q < FLOOR_Q);
    if let (Some((q, sub, bytes)), false) = (best, poor) {
        return Ok(done(base, spec, win, q, sub, bytes, 0, false, notes));
    }
    if let Some((q, sub, mut bytes)) = below_min {
        // Even the best quality that fits is below the minimum: pad as the last step.
        if !spec.pad_to_min {
            return Err(ImageError::Failed(format!(
                "This picture is too small or too simple to reach {} at {} x {} pixels (the largest it gets is {}). Allow padding (pad_to_min), use a larger pixel size, or lower the minimum.",
                human_bytes(lo),
                base.width,
                base.height,
                human_bytes(bytes.len() as u64)
            )));
        }
        let margin = 64u64.min(hi.map(|h| h.saturating_sub(lo) / 2).unwrap_or(64));
        let want = (lo + margin) as usize;
        let before = bytes.len();
        bytes = if spec.format == Format::Jpeg {
            encode::jpeg::pad_with_comments(&bytes, want)
        } else {
            return Err(ImageError::Failed(format!(
                "{} cannot be padded to a minimum size. Use JPG for this portal.",
                spec.format.label()
            )));
        };
        let pad = bytes.len() - before;
        notes.push(format!("the picture only reaches {} on its own, so {} bytes of comment padding were added to meet the {} minimum", human_bytes(before as u64), with_commas(pad as u64), human_bytes(lo)));
        return Ok(done(base, spec, win, q, sub, bytes, pad, false, notes));
    }

    // Nothing at this pixel size fits under the maximum.
    let h = hi.unwrap_or(0);
    let floor = enc(base, spec, 1, true, false)?.len() as u64;
    if spec.pixels.is_some() {
        return Err(ImageError::Failed(format!(
            "At {} x {} pixels the smallest this picture gets is {}, above the {} limit. This portal's rules ask for both, so the picture cannot meet them: try a plainer photo with less detail, or check the portal's current rule.",
            base.width,
            base.height,
            human_bytes(floor),
            human_bytes(h)
        )));
    }
    shrink_to_fit(base, spec, win, io, notes, &try_size)
}

/// Finds the best quality that fits for an image, a chroma setting and a progressive setting.
type SizeFn<'a> = dyn Fn(&Image, bool, bool) -> Result<Option<(u32, Vec<u8>)>> + 'a;

/// Lowest JPEG quality accepted when the picture is free to shrink.
const FLOOR_Q: u32 = 75;

/// Free size mode: find the largest scale at which quality `FLOOR_Q` fits under the maximum
/// (bisection on the scale), then use the best quality that fits at that size.
fn shrink_to_fit(
    base: &Image,
    spec: &Spec,
    win: Window,
    io: &dyn Io,
    mut notes: Vec<String>,
    try_size: &SizeFn<'_>,
) -> Result<Solution> {
    let h = win.hi.unwrap_or(0);
    let at = |scale: f64| -> Result<Image> {
        let (w, hh) = (
            ((base.width as f64 * scale).round() as u32).max(8),
            ((base.height as f64 * scale).round() as u32).max(8),
        );
        resample_image(base, w, hh)
    };
    let fits = |img: &Image| -> Result<bool> {
        Ok(enc(img, spec, FLOOR_Q, true, !spec.baseline)?.len() as u64 <= h)
    };
    let (mut lo, mut hi) = (0.02f64, 1.0f64);
    if !fits(&at(lo)?)? {
        return Err(ImageError::Failed(format!(
            "This picture could not be brought under {}. Choose a larger limit.",
            human_bytes(h)
        )));
    }
    for _ in 0..9 {
        check_cancel(io)?;
        let mid = (lo + hi) / 2.0;
        if fits(&at(mid)?)? {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let small = at(lo)?;
    let Some((q, mut bytes)) = try_size(&small, true, !spec.baseline)? else {
        return Err(ImageError::Failed(format!(
            "This picture could not be brought under {}. Choose a larger limit.",
            human_bytes(h)
        )));
    };
    let mut pad = 0;
    if (bytes.len() as u64) < win.lo && spec.format == Format::Jpeg && spec.pad_to_min {
        let before = bytes.len();
        bytes = encode::jpeg::pad_with_comments(&bytes, (win.lo + 64) as usize);
        pad = bytes.len() - before;
        notes.push(format!(
            "{} bytes of comment padding were added to meet the {} minimum",
            with_commas(pad as u64),
            human_bytes(win.lo)
        ));
    }
    if (small.width, small.height) != (base.width, base.height) {
        notes.push(format!(
            "the picture was reduced from {} x {} to {} x {} pixels so a good quality fits {}",
            base.width,
            base.height,
            small.width,
            small.height,
            human_bytes(h)
        ));
    }
    Ok(done(&small, spec, win, q, true, bytes, pad, true, notes))
}

#[allow(clippy::too_many_arguments)]
fn done(
    img: &Image,
    spec: &Spec,
    win: Window,
    q: u32,
    sub: bool,
    bytes: Vec<u8>,
    pad: usize,
    downscaled: bool,
    mut notes: Vec<String>,
) -> Solution {
    let n = bytes.len() as u64;
    notes.insert(0, card_line(img.width, img.height, n, spec, win));
    if let Some(p) = spec.preset {
        let verified = match p.status.as_str() {
            "verified" => "verified",
            "conflict" => "sources disagree, the stricter reading was used",
            _ => "not recently verified",
        };
        notes.push(format!("preset {} ({verified}), last checked {}, source {}. This meets the published rule; it does not promise the portal accepts the file", p.id, if p.last_verified.is_empty() { "never" } else { &p.last_verified }, p.source_url));
    }
    Solution {
        bytes,
        width: img.width,
        height: img.height,
        quality: q,
        subsampled: sub,
        padded_by: pad,
        downscaled,
        notes,
    }
}

/// "200 x 230 px, 47,980 bytes (47.9 KB or 46.9 KB), inside 20 to 50 KB under both readings."
pub fn card_line(w: u32, h: u32, bytes: u64, spec: &Spec, win: Window) -> String {
    let kb1000 = bytes as f64 / 1000.0;
    let kb1024 = bytes as f64 / 1024.0;
    let mut s = format!(
        "{w} x {h} px, {} bytes ({kb1000:.1} KB or {kb1024:.1} KB)",
        with_commas(bytes)
    );
    if let Some((mn, mx)) = spec.published {
        let range = if mn > 0.0 && mx > 0.0 {
            format!("{} to {} KB", trim(mn), trim(mx))
        } else if mx > 0.0 {
            format!("{} KB", trim(mx))
        } else {
            format!("{} KB", trim(mn))
        };
        let in1000 = bytes >= (mn * 1000.0) as u64 && (mx <= 0.0 || bytes <= (mx * 1000.0) as u64);
        let in1024 = bytes >= (mn * 1024.0) as u64 && (mx <= 0.0 || bytes <= (mx * 1024.0) as u64);
        let how = match (in1000, in1024) {
            (true, true) => "under both readings".to_string(),
            (true, false) => "under the 1000 bytes per KB reading only".to_string(),
            (false, true) => "under the 1024 bytes per KB reading only".to_string(),
            (false, false) => "OUTSIDE the window".to_string(),
        };
        let word = if mn > 0.0 { "inside" } else { "under" };
        s.push_str(&format!(", {word} {range} {how}"));
    }
    let _ = win;
    s.push('.');
    s
}

fn trim(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as u64)
    } else {
        format!("{v}")
    }
}

fn solve_png(base: &Image, spec: &Spec, win: Window, notes: Vec<String>) -> Result<Solution> {
    let hi = win.hi.unwrap_or(u64::MAX);
    let lossless = encode_bytes(
        base,
        &EncodeSpec {
            png_effort: encode::png::Effort::Normal,
            ..EncodeSpec::new(Format::Png)
        },
    )?;
    if lossless.len() as u64 <= hi {
        if lossless.len() as u64 >= win.lo {
            return Ok(done(base, spec, win, 0, false, lossless, 0, false, notes));
        }
        return Err(ImageError::Failed(format!(
            "A PNG of this picture is {}, below the {} minimum. Use JPG, which can be padded to a minimum size.",
            human_bytes(lossless.len() as u64),
            human_bytes(win.lo)
        )));
    }
    let n = largest_fitting(8, 256, |c| {
        let q = crate::quant::quantize(base, c as usize, 0.8);
        Ok::<bool, ImageError>(
            encode::png::encode_indexed(
                base.width,
                base.height,
                &q.indices,
                &q.palette,
                &Embed::default(),
                encode::png::Effort::Fast,
            )?
            .len() as u64
                <= hi,
        )
    })?;
    let Some(c) = n else {
        return Err(ImageError::Failed(format!(
            "Even 8 colors do not bring this PNG under {}. Use JPG, or a smaller pixel size.",
            human_bytes(hi)
        )));
    };
    let q = crate::quant::quantize(base, c as usize, 0.8);
    let bytes = encode::png::encode_indexed(
        base.width,
        base.height,
        &q.indices,
        &q.palette,
        &Embed::default(),
        encode::png::Effort::Normal,
    )?;
    let mut notes = notes;
    notes.push(format!("the PNG was reduced to {c} colors to fit"));
    Ok(done(base, spec, win, c, false, bytes, 0, false, notes))
}

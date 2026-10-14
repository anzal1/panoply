//! `image.erase`: paint over something and make it disappear (docs/specs/id-and-image.md 3.14).
//!
//! The fill is MI-GAN (512 px Places2, MIT, 28 MB), run on a square crop around each cluster of
//! masked pixels, never on the whole picture, so memory scales with the crop and a 12 megapixel
//! photo costs the same as a small one. Only pixels inside the (expanded) mask are replaced; every
//! other pixel is copied, and the job checks that with a digest of the pixels outside the mask
//! before and after.
//!
//! The mask comes from a second picture (white means erase; a picture with transparency uses its
//! alpha), from the `mask` option, or from rectangles in the `rect` option.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use panoply_core::CancelToken;
use panoply_image::color::OutSpace;
use panoply_image::error::{ImageError, Result};
use panoply_image::ops::common::{Animation, display_name, load};
use panoply_image::ops::human_bytes;
use panoply_image::ops::options::Opts;
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image};
use panoply_image::resample::Filter;
use panoply_image::types::{Depth, Layout, Samples};
use sha2::{Digest, Sha256};

use crate::VisionIo;
use crate::models::MIGAN;
use crate::px::{Kernel, Rect, dilate, parse_rects, resize_u8};
use crate::runtime::{self, In, Loaded, Out, Prefer};
use crate::tile::copy_out;
use crate::work::{
    MAX_WORK_PIXELS, WriteSpec, file_name, image_from, load_source, parse_format, write_result,
};

/// The model's working side, in pixels.
const MODEL_SIDE: usize = 512;
/// Context kept around the masked area on each side, in pixels (the pipeline uses 128).
const PAD: usize = 128;
/// The largest crop side fed to the model; bigger areas are shrunk to this first.
const MAX_CROP: usize = 1536;
/// Two clusters closer than this many pixels are filled together.
const MERGE_GAP: usize = 96;

/// Run the module on each input.
pub fn run(io: &dyn VisionIo, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let _scope = runtime::JobScope;
    let rect_text = opts.str("rect");
    let mask_path = opts.str("mask").map(PathBuf::from);
    let invert = opts.flag("invert", false);
    // What each input is: an image, with a shared mask from rect or the mask option, or an image
    // followed by its mask picture.
    let pairs: Vec<(PathBuf, Option<PathBuf>)> = if rect_text.is_some() {
        inputs.iter().map(|p| (p.clone(), None)).collect()
    } else if let Some(m) = &mask_path {
        inputs
            .iter()
            .map(|p| (p.clone(), Some(m.clone())))
            .collect()
    } else if inputs.len() == 2 {
        vec![(inputs[0].clone(), Some(inputs[1].clone()))]
    } else {
        return Err(ImageError::BadOption(
            "Say what to erase: give a mask picture as the second file (white is erased), or set rect to a rectangle such as 100,200,300,80.".into(),
        ));
    };
    let mut outputs = Vec::new();
    let mut first_err = None;
    for (i, (image, mask)) in pairs.iter().enumerate() {
        if io.cancel().is_cancelled() {
            return Err(ImageError::Cancelled);
        }
        io.progress(
            i as f32 / pairs.len() as f32,
            &format!("Erasing in {}", display_name(image)),
        );
        match one(io, image, mask.as_deref(), rect_text, invert, opts) {
            Ok(p) => outputs.push(p),
            Err(ImageError::Cancelled) => return Err(ImageError::Cancelled),
            Err(e) if pairs.len() > 1 => {
                io.note(format!("{}: skipped. {e}", display_name(image)));
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

fn one(
    io: &dyn VisionIo,
    image: &std::path::Path,
    mask_file: Option<&std::path::Path>,
    rect_text: Option<&str>,
    invert: bool,
    opts: &Opts,
) -> Result<PathBuf> {
    let started = Instant::now();
    let animation = Animation::from_opt(opts.str("animation"));
    let expand = opts.u32("expand").unwrap_or(8) as usize;
    let mut src = load_source(io, image, animation, MAX_WORK_PIXELS)?;
    let name = src.name.clone();
    let (w, h) = (src.w, src.h);
    let mut mask = match (rect_text, mask_file) {
        (Some(t), _) => {
            let mut m = vec![0u8; w * h];
            for r in parse_rects(t, w, h)? {
                for y in r.y..r.y + r.h {
                    m[y * w + r.x..y * w + r.x + r.w].fill(1);
                }
            }
            m
        }
        (None, Some(p)) => read_mask(io, p, w, h, invert)?,
        (None, None) => unreachable!("run() checked that a mask or rectangle is given"),
    };
    if rect_text.is_some() && invert {
        for v in &mut mask {
            *v ^= 1;
        }
    }
    let marked = mask.iter().filter(|&&v| v != 0).count();
    if marked == 0 {
        return Err(ImageError::BadOption(
            "The mask is empty, so there is nothing to erase. White areas of the mask picture are erased; check the picture, or use invert=true if yours is the other way round.".into(),
        ));
    }
    let inpainter = Inpainter::new(io)?;
    let report = inpainter.fill(io, &mut src.rgb, w, h, &mask, expand)?;
    let alpha = src.alpha.take().map(|mut a| {
        // The new content must be visible even where the old object was transparent.
        for (v, &c) in a.iter_mut().zip(&report.changed) {
            if c != 0 {
                *v = 255;
            }
        }
        a
    });
    let img = image_from(w, h, &src.rgb, alpha.as_deref());
    let format = parse_format(opts.str("format").filter(|f| *f != "keep"), src.info.format);
    let (out_path, bytes) = write_result(
        io,
        &img,
        &WriteSpec {
            src: &src,
            label: "erased",
            format,
            quality: opts.f64("quality"),
            keep_metadata: opts.flag("keep_metadata", true),
        },
    )?;
    let changed_px = report.changed.iter().filter(|&&v| v != 0).count();
    io.note(format!(
        "{name}: erased {} area{} ({:.1}% of the picture) in {:.1} s, {}",
        report.clusters,
        if report.clusters == 1 { "" } else { "s" },
        100.0 * changed_px as f64 / (w * h) as f64,
        started.elapsed().as_secs_f64(),
        human_bytes(bytes as u64)
    ));
    io.note(format!(
        "{name}: the rest of the picture is untouched (digest of the pixels outside the mask is the same before and after), wrote {}",
        file_name(&out_path)
    ));
    if report.shrunk {
        io.note(format!(
            "{name}: a large area was filled at reduced detail; paint smaller areas for finer results"
        ));
    }
    if w * h < src.full_w * src.full_h {
        io.note(format!(
            "{name}: worked at {w} x {h} (the picture is {} x {}) to stay inside the memory limit",
            src.full_w, src.full_h
        ));
    }
    for n in src.notes {
        io.note(format!("{name}: {n}"));
    }
    Ok(out_path)
}

/// Reads a mask picture at the size of the image: 1 where it says erase. A picture with real
/// transparency uses its alpha; any other uses its brightness (white erases).
pub fn read_mask(
    io: &dyn VisionIo,
    path: &std::path::Path,
    w: usize,
    h: usize,
    invert: bool,
) -> Result<Vec<u8>> {
    let mut loaded = load(io, path, None, Animation::First, OutSpace::Srgb)?;
    let info = loaded.info.clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (uw, uh) = panoply_image::orient::oriented_size(o, info.full_width, info.full_height);
    let scale = ((uw as usize, uh as usize) != (w, h)).then_some((w as u32, h as u32));
    let progress = |_: f32| {};
    let ctl = Ctl {
        cancel: io.cancel(),
        progress: &progress,
    };
    let plan = ImagePlan {
        out_space: OutSpace::Srgb,
        scale,
        filter: scale.map(|_| Filter::auto(uw, uh, w as u32, h as u32)),
        flatten: None,
        out_depth: Some(Depth::U8),
    };
    let (img, _) = decode_to_image(&mut *loaded.reader, &plan, &ctl)?;
    let Samples::U8(s) = &img.samples else {
        return Err(ImageError::Failed(
            "internal: the mask was not 8 bit".into(),
        ));
    };
    let ch = img.layout.channels();
    let use_alpha = img.layout.has_alpha() && s.chunks_exact(ch).any(|p| p[ch - 1] != 255);
    let mut out = Vec::with_capacity(w * h);
    for p in s.chunks_exact(ch) {
        let v = if use_alpha {
            p[ch - 1]
        } else {
            match img.layout {
                Layout::Gray | Layout::GrayAlpha => p[0],
                _ => {
                    ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000)
                        as u8
                }
            }
        };
        out.push(u8::from((v > 127) != invert));
    }
    Ok(out)
}

/// What a fill did.
pub struct FillReport {
    /// How many separate areas were filled.
    pub clusters: usize,
    /// One byte a pixel: 1 where pixels were allowed to change (the expanded mask).
    pub changed: Vec<u8>,
    /// True when an area was too large for full detail and was shrunk for the model.
    pub shrunk: bool,
}

/// The fill model, loaded for a job.
pub struct Inpainter {
    model: Arc<Loaded>,
}

impl Inpainter {
    /// Loads the model (downloading it once if needed).
    pub fn new(io: &dyn VisionIo) -> Result<Self> {
        Ok(Self {
            model: runtime::acquire(io, &MIGAN, Prefer::Cpu)?,
        })
    }

    /// Fills the masked pixels of `rgb` (`w x h`, 3 bytes a pixel). `mask` is 1 where to erase;
    /// `expand` grows it first. Pixels outside the grown mask are not touched.
    pub fn fill(
        &self,
        io: &dyn VisionIo,
        rgb: &mut [u8],
        w: usize,
        h: usize,
        mask: &[u8],
        expand: usize,
    ) -> Result<FillReport> {
        let boxes = clusters(mask, w, h);
        // Where pixels may change: the expanded mask of every cluster, found before anything is
        // filled so the "nothing else changed" digest can be taken first.
        let mut changed = vec![0u8; w * h];
        let mut plans = Vec::new();
        for b in &boxes {
            let crop = crop_for(*b, expand, w, h);
            let win = copy_out(mask, w, 1, crop);
            let grown = dilate(&win, crop.w, crop.h, expand);
            for y in 0..crop.h {
                for x in 0..crop.w {
                    if grown[y * crop.w + x] != 0 {
                        changed[(crop.y + y) * w + crop.x + x] = 1;
                    }
                }
            }
            plans.push((crop, grown));
        }
        let before = digest_outside(rgb, &changed);
        let mut shrunk = false;
        for (i, (crop, grown)) in plans.iter().enumerate() {
            if io.cancel().is_cancelled() {
                return Err(ImageError::Cancelled);
            }
            io.progress(
                0.2 + 0.7 * i as f32 / plans.len() as f32,
                "Filling in the erased area",
            );
            shrunk |= self.fill_one(rgb, w, *crop, grown)?;
        }
        if digest_outside(rgb, &changed) != before {
            return Err(ImageError::Failed(
                "internal: the fill changed pixels outside the mask, so the result was discarded."
                    .into(),
            ));
        }
        Ok(FillReport {
            clusters: boxes.len(),
            changed,
            shrunk,
        })
    }

    /// Runs the model on one crop and writes the filled pixels inside `grown` back. Returns true
    /// when the crop was shrunk first.
    fn fill_one(&self, rgb: &mut [u8], w: usize, crop: Rect, grown: &[u8]) -> Result<bool> {
        let (cw, ch) = (crop.w, crop.h);
        let pix = copy_out(rgb, w, 3, crop);
        // Shrink an area bigger than the model can take in one piece.
        let f = (cw.max(ch) as f64 / MAX_CROP as f64).max(1.0);
        let (mw, mh) = (
            ((cw as f64 / f).round() as usize).max(1),
            ((ch as f64 / f).round() as usize).max(1),
        );
        let small_pix;
        let small_mask: Vec<u8>;
        let (pix_in, mask_in): (&[u8], &[u8]) = if f > 1.0 {
            small_pix = resize_u8(&pix, cw, ch, 3, mw, mh, Kernel::Lanczos3)?;
            let m255: Vec<u8> = grown.iter().map(|&v| v * 255).collect();
            let sm = resize_u8(&m255, cw, ch, 1, mw, mh, Kernel::Bilinear)?;
            small_mask = sm.iter().map(|&v| u8::from(v > 40)).collect();
            (&small_pix, &small_mask)
        } else {
            (&pix, grown)
        };
        // Planar uint8 image and a mask where 255 means keep.
        let n = mw * mh;
        let mut planar = vec![0u8; 3 * n];
        for (i, p) in pix_in.chunks_exact(3).enumerate() {
            planar[i] = p[0];
            planar[n + i] = p[1];
            planar[2 * n + i] = p[2];
        }
        let keep: Vec<u8> = mask_in
            .iter()
            .map(|&v| if v != 0 { 0 } else { 255 })
            .collect();
        let out = self.model.run(
            vec![
                In::U8("image", vec![1, 3, mh as i64, mw as i64], planar),
                In::U8("mask", vec![1, 1, mh as i64, mw as i64], keep),
            ],
            &["result"],
        )?;
        let Some(Out::U8(shape, res)) = out.into_iter().next() else {
            return Err(ImageError::Failed(
                "The fill model returned an unexpected answer. Try again.".into(),
            ));
        };
        if res.len() != 3 * n {
            return Err(ImageError::Failed(format!(
                "The fill model returned a {shape:?} picture, not {mw} x {mh}. Try again."
            )));
        }
        // Back to interleaved at the crop's size.
        let mut inter = vec![0u8; n * 3];
        for i in 0..n {
            inter[i * 3] = res[i];
            inter[i * 3 + 1] = res[n + i];
            inter[i * 3 + 2] = res[2 * n + i];
        }
        let filled = if f > 1.0 {
            resize_u8(&inter, mw, mh, 3, cw, ch, Kernel::CatmullRom)?
        } else {
            inter
        };
        for y in 0..ch {
            for x in 0..cw {
                if grown[y * cw + x] != 0 {
                    let d = ((crop.y + y) * w + crop.x + x) * 3;
                    let s = (y * cw + x) * 3;
                    rgb[d..d + 3].copy_from_slice(&filled[s..s + 3]);
                }
            }
        }
        Ok(f > 1.0)
    }
}

/// SHA-256 of the pixels where `changed` is 0.
fn digest_outside(rgb: &[u8], changed: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    let mut run_start = 0usize;
    for (i, &c) in changed.iter().enumerate() {
        if c != 0 {
            if i > run_start {
                h.update(&rgb[run_start * 3..i * 3]);
            }
            run_start = i + 1;
        }
    }
    if changed.len() > run_start {
        h.update(&rgb[run_start * 3..changed.len() * 3]);
    }
    h.finalize().into()
}

/// The square crop to hand the model for a masked box: the box plus context, at least the
/// model's own working size, kept square and inside the picture.
fn crop_for(b: Rect, expand: usize, w: usize, h: usize) -> Rect {
    let ext = b.w.max(b.h) + 2 * expand;
    let side = (ext + 2 * PAD).max(MODEL_SIDE).min(w).min(h);
    let cx = b.x + b.w / 2;
    let cy = b.y + b.h / 2;
    let x0 = cx.saturating_sub(side / 2).min(w - side);
    let y0 = cy.saturating_sub(side / 2).min(h - side);
    let mut r = Rect {
        x: x0,
        y: y0,
        w: side,
        h: side,
    };
    // The box itself plus the expansion must be inside (it is, unless the picture is tiny).
    let grown = b.grown(expand, w, h);
    if grown.x < r.x
        || grown.y < r.y
        || grown.x + grown.w > r.x + r.w
        || grown.y + grown.h > r.y + r.h
    {
        r = r.union(&grown);
    }
    r
}

/// Bounding boxes of the clusters of marked pixels. Marked pixels closer together than
/// [`MERGE_GAP`] belong to one cluster, so one fill sees them all.
pub fn clusters(mask: &[u8], w: usize, h: usize) -> Vec<Rect> {
    const CELL: usize = 16;
    let (gw, gh) = (w.div_ceil(CELL), h.div_ceil(CELL));
    let mut marked = vec![false; gw * gh];
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] != 0 {
                marked[(y / CELL) * gw + x / CELL] = true;
            }
        }
    }
    let gap = MERGE_GAP.div_ceil(CELL);
    let mut parent: Vec<usize> = (0..gw * gh).collect();
    fn find(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for cy in 0..gh {
        for cx in 0..gw {
            if !marked[cy * gw + cx] {
                continue;
            }
            for ny in cy.saturating_sub(gap)..=(cy + gap).min(gh - 1) {
                for nx in cx.saturating_sub(gap)..=(cx + gap).min(gw - 1) {
                    if marked[ny * gw + nx] {
                        let (a, b) = (
                            find(&mut parent, cy * gw + cx),
                            find(&mut parent, ny * gw + nx),
                        );
                        if a != b {
                            parent[a] = b;
                        }
                    }
                }
            }
        }
    }
    let mut boxes: std::collections::BTreeMap<usize, (usize, usize, usize, usize)> =
        std::collections::BTreeMap::new();
    for y in 0..h {
        for x in 0..w {
            if mask[y * w + x] != 0 {
                let id = find(&mut parent, (y / CELL) * gw + x / CELL);
                let e = boxes.entry(id).or_insert((x, y, x, y));
                e.0 = e.0.min(x);
                e.1 = e.1.min(y);
                e.2 = e.2.max(x);
                e.3 = e.3.max(y);
            }
        }
    }
    boxes
        .into_values()
        .map(|(x0, y0, x1, y1)| Rect {
            x: x0,
            y: y0,
            w: x1 - x0 + 1,
            h: y1 - y0 + 1,
        })
        .collect()
}

/// Cancel helper for callers that hold only a token.
pub fn check(cancel: &CancelToken) -> Result<()> {
    crate::work::check(cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn far_apart_marks_are_separate_clusters_and_near_ones_merge() {
        let (w, h) = (800usize, 400usize);
        let mut m = vec![0u8; w * h];
        for y in 100..110 {
            for x in 100..110 {
                m[y * w + x] = 1;
            }
        }
        // 40 px away: merges. 500 px away: separate.
        for y in 100..110 {
            for x in 150..160 {
                m[y * w + x] = 1;
            }
        }
        for y in 300..310 {
            for x in 700..710 {
                m[y * w + x] = 1;
            }
        }
        let c = clusters(&m, w, h);
        assert_eq!(c.len(), 2);
        let a = c.iter().find(|r| r.x < 200).unwrap();
        assert_eq!((a.x, a.w), (100, 60));
    }

    #[test]
    fn crops_are_square_contain_the_mask_and_stay_inside() {
        let b = Rect {
            x: 10,
            y: 10,
            w: 30,
            h: 20,
        };
        let c = crop_for(b, 8, 2000, 1500);
        assert_eq!(c.w, c.h);
        assert!(c.x == 0 && c.y == 0, "{c:?}");
        assert!(c.w >= MODEL_SIDE);
        let b = Rect {
            x: 1900,
            y: 1400,
            w: 50,
            h: 50,
        };
        let c = crop_for(b, 8, 2000, 1500);
        assert!(c.x + c.w <= 2000 && c.y + c.h <= 1500, "{c:?}");
        assert!(c.x <= b.x && c.y <= b.y && c.x + c.w >= b.x + b.w && c.y + c.h >= b.y + b.h);
        // A small picture gives a crop the size of the picture's short side.
        let c = crop_for(
            Rect {
                x: 5,
                y: 5,
                w: 10,
                h: 10,
            },
            4,
            300,
            200,
        );
        assert_eq!((c.w, c.h), (200, 200));
    }

    #[test]
    fn digest_ignores_changed_pixels_and_notices_other_changes() {
        let mut rgb = vec![7u8; 10 * 3];
        let mut changed = vec![0u8; 10];
        changed[3] = 1;
        changed[4] = 1;
        let a = digest_outside(&rgb, &changed);
        rgb[3 * 3] = 200;
        assert_eq!(digest_outside(&rgb, &changed), a);
        rgb[0] = 1;
        assert_ne!(digest_outside(&rgb, &changed), a);
    }
}

//! Where the faces are, for the tools that must never change one (Remove watermark, UP-38).
//!
//! The engine is YuNet through ONNX Runtime, the passport photo tool's fallback face engine, on every
//! platform. It is used here instead of Apple Vision on purpose: the ONNX Runtime is already loaded
//! for the fill model, so YuNet costs about 40 MB of the job's memory where any Apple Vision request
//! costs about 145 MB that stay resident beside the fill model (measured on the smoke batch, UP-38).
//! The model (0.2 MB) and the runtime are fetched once after a yes, like the fill model, by
//! [`ensure`]; [`find`] itself never fetches. The result is face boxes in picture pixels.

use crate::px::Rect;

/// True when the face model and the runtime are already on this computer.
pub fn ready() -> bool {
    panoply_id::runtime::face_engine_ready()
}

/// Makes sure the face model and the runtime are on this computer, fetching what is missing after
/// the person's yes (the same consent sheet as the fill model).
pub fn ensure() -> panoply_image::error::Result<()> {
    if !ready() {
        panoply_id::runtime::ensure_onnxruntime()?;
        panoply_id::runtime::ensure_model()?;
    }
    Ok(())
}

/// [`ensure`], then [`find`]: the call a job makes.
pub fn find_ensuring(rgb: &[u8], w: usize, h: usize) -> panoply_image::error::Result<Vec<Rect>> {
    ensure()?;
    Ok(find(rgb, w, h))
}

/// The faces in an 8 bit RGB picture, as boxes clamped to the picture. Never fetches and never
/// fails: an engine that is not on this computer or errors gives no boxes.
pub fn find(rgb: &[u8], w: usize, h: usize) -> Vec<Rect> {
    // Faces are found on a reduced copy (a face is large next to 1,600 pixels), so a 30 megapixel
    // picture costs no more memory than a small one, and the boxes are scaled back.
    let f = w.max(h).div_ceil(SEARCH_LONG).max(1);
    if f == 1 {
        return engine(rgb, w, h)
            .into_iter()
            .filter_map(|(x, y, bw, bh)| clamp(x, y, bw, bh, w, h))
            .collect();
    }
    let (sw, sh) = (w.div_ceil(f), h.div_ceil(f));
    let mut small = vec![0u8; sw * sh * 3];
    for dy in 0..sh {
        for dx in 0..sw {
            let (x1, y1) = (((dx + 1) * f).min(w), ((dy + 1) * f).min(h));
            let mut sum = [0u32; 3];
            for y in dy * f..y1 {
                for x in dx * f..x1 {
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(rgb[(y * w + x) * 3 + c]);
                    }
                }
            }
            let n = ((x1 - dx * f) * (y1 - dy * f)) as u32;
            for (c, s) in sum.iter().enumerate() {
                small[(dy * sw + dx) * 3 + c] = (s / n) as u8;
            }
        }
    }
    let k = f as f64;
    engine(&small, sw, sh)
        .into_iter()
        .filter_map(|(x, y, bw, bh)| clamp(x * k, y * k, bw * k, bh * k, w, h))
        .collect()
}

/// The longest side faces are looked for at.
const SEARCH_LONG: usize = 1600;

/// A float box as a rectangle inside a `w` by `h` picture; `None` when nothing is left.
pub fn clamp(x: f64, y: f64, bw: f64, bh: f64, w: usize, h: usize) -> Option<Rect> {
    let x0 = x.max(0.0).min(w as f64);
    let y0 = y.max(0.0).min(h as f64);
    let x1 = (x + bw).max(0.0).min(w as f64);
    let y1 = (y + bh).max(0.0).min(h as f64);
    if x1 - x0 < 2.0 || y1 - y0 < 2.0 {
        return None;
    }
    Some(Rect {
        x: x0 as usize,
        y: y0 as usize,
        w: (x1 - x0) as usize,
        h: (y1 - y0) as usize,
    })
}

fn engine(rgb: &[u8], w: usize, h: usize) -> Vec<(f64, f64, f64, f64)> {
    if !panoply_id::runtime::face_engine_ready() {
        return Vec::new();
    }
    let img = crate::work::image_from(w, h, rgb, None);
    panoply_id::yunet::detect_once(&img)
        .map(|v| v.into_iter().map(|f| (f.x, f.y, f.w, f.h)).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_is_clamped_to_the_picture_and_a_sliver_is_dropped() {
        let r = clamp(-10.0, 5.0, 60.0, 400.0, 100, 50).unwrap();
        assert_eq!((r.x, r.y, r.w, r.h), (0, 5, 50, 45));
        assert!(clamp(120.0, 5.0, 30.0, 30.0, 100, 50).is_none());
        assert!(clamp(10.0, 10.0, 1.0, 30.0, 100, 50).is_none());
    }

    #[test]
    fn a_picture_with_no_face_gives_no_boxes() {
        let rgb = vec![128u8; 64 * 64 * 3];
        assert!(find(&rgb, 64, 64).is_empty());
    }
}

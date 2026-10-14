//! Fitting a picture into a box: contain, cover (crop), pad or stretch. The resample runs in the
//! pipeline at the scaled size; the crop or pad then happens here on the small result.

use crate::types::{Image, Layout, Samples};

/// How a picture meets a box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fit {
    /// Fit inside the box, keeping the aspect ratio. The result can be smaller than the box.
    Contain,
    /// Fill the box, keeping the aspect ratio, and crop the overflow. `anchor_y` is where the
    /// crop sits vertically (0.0 keeps the top, 0.5 the middle).
    Cover {
        /// Vertical anchor, 0.0 to 1.0.
        anchor_y: f32,
    },
    /// Fit inside the box and fill the rest with the background color.
    Pad,
    /// Distort to the exact box.
    Stretch,
}

/// Size the picture is resampled to before the crop or pad, given a box.
/// `enlarge` false never makes the picture bigger than it is.
pub fn scaled_size(w: u32, h: u32, bw: u32, bh: u32, fit: Fit, enlarge: bool) -> (u32, u32) {
    let (wf, hf, bwf, bhf) = (w as f64, h as f64, bw as f64, bh as f64);
    let r = |v: f64| (v.round() as u32).max(1);
    match fit {
        Fit::Stretch => {
            if enlarge {
                (bw, bh)
            } else {
                (bw.min(w), bh.min(h))
            }
        }
        Fit::Contain | Fit::Pad => {
            let mut s = (bwf / wf).min(bhf / hf);
            if !enlarge {
                s = s.min(1.0);
            }
            (r(wf * s).min(bw.max(1)), r(hf * s).min(bh.max(1)))
        }
        Fit::Cover { .. } => {
            let mut s = (bwf / wf).max(bhf / hf);
            if !enlarge {
                s = s.min(1.0);
            }
            (
                (wf * s).ceil().max(1.0) as u32,
                (hf * s).ceil().max(1.0) as u32,
            )
        }
    }
}

/// Crop or pad a resampled picture to the box. `bg` fills pad areas.
pub fn finish(img: Image, bw: u32, bh: u32, fit: Fit, bg: [u8; 3]) -> Image {
    match fit {
        Fit::Contain | Fit::Stretch => img,
        Fit::Cover { anchor_y } => {
            if img.width <= bw && img.height <= bh {
                return img;
            }
            let (cw, ch) = (img.width.min(bw), img.height.min(bh));
            let x = (img.width - cw) / 2;
            let y = ((img.height - ch) as f32 * anchor_y.clamp(0.0, 1.0)).round() as u32;
            img.crop(x, y, cw, ch)
        }
        Fit::Pad => {
            if img.width == bw && img.height == bh {
                return img;
            }
            let neutral = bg[0] == bg[1] && bg[1] == bg[2];
            let base = if img.layout.is_gray() && !neutral {
                img.to_rgb_layout()
            } else {
                img
            };
            let ch = base.layout.channels();
            let mut out = Image::new(bw, bh, base.layout, base.depth());
            out.icc = base.icc.clone();
            let fill = |layout: Layout| -> Vec<u16> {
                let gray =
                    ((bg[0] as u32 * 299 + bg[1] as u32 * 587 + bg[2] as u32 * 114) / 1000) as u16;
                match layout {
                    Layout::Gray => vec![gray],
                    Layout::GrayAlpha => vec![gray, 255],
                    Layout::Rgb => vec![bg[0] as u16, bg[1] as u16, bg[2] as u16],
                    Layout::Rgba => vec![bg[0] as u16, bg[1] as u16, bg[2] as u16, 255],
                }
            };
            let f = fill(base.layout);
            let (ox, oy) = ((bw - base.width) / 2, (bh - base.height) / 2);
            match (&mut out.samples, &base.samples) {
                (Samples::U8(d), Samples::U8(s)) => {
                    for px in d.chunks_exact_mut(ch) {
                        for (k, v) in px.iter_mut().enumerate() {
                            *v = f[k] as u8;
                        }
                    }
                    for r in 0..base.height as usize {
                        let doff = ((oy as usize + r) * bw as usize + ox as usize) * ch;
                        d[doff..doff + base.width as usize * ch].copy_from_slice(
                            &s[r * base.width as usize * ch..(r + 1) * base.width as usize * ch],
                        );
                    }
                }
                (Samples::U16(d), Samples::U16(s)) => {
                    for px in d.chunks_exact_mut(ch) {
                        for (k, v) in px.iter_mut().enumerate() {
                            *v = f[k] * 257;
                        }
                    }
                    for r in 0..base.height as usize {
                        let doff = ((oy as usize + r) * bw as usize + ox as usize) * ch;
                        d[doff..doff + base.width as usize * ch].copy_from_slice(
                            &s[r * base.width as usize * ch..(r + 1) * base.width as usize * ch],
                        );
                    }
                }
                _ => unreachable!("depth mismatch"),
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Depth;

    #[test]
    fn sizes_for_each_fit() {
        assert_eq!(
            scaled_size(4000, 3000, 1000, 1000, Fit::Contain, true),
            (1000, 750)
        );
        assert_eq!(
            scaled_size(4000, 3000, 1000, 1000, Fit::Pad, true),
            (1000, 750)
        );
        assert_eq!(
            scaled_size(4000, 3000, 1000, 1000, Fit::Cover { anchor_y: 0.5 }, true),
            (1334, 1000)
        );
        assert_eq!(
            scaled_size(4000, 3000, 1000, 1000, Fit::Stretch, true),
            (1000, 1000)
        );
        // Do not enlarge.
        assert_eq!(
            scaled_size(400, 300, 1000, 1000, Fit::Contain, false),
            (400, 300)
        );
        assert_eq!(
            scaled_size(400, 300, 1000, 1000, Fit::Stretch, false),
            (400, 300)
        );
    }

    #[test]
    fn cover_crops_to_the_box_and_pad_fills_it() {
        let mut img = Image::new(8, 4, Layout::Rgb, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            v.iter_mut().for_each(|s| *s = 10);
        }
        let c = finish(
            img.clone(),
            4,
            4,
            Fit::Cover { anchor_y: 0.5 },
            [255, 255, 255],
        );
        assert_eq!((c.width, c.height), (4, 4));
        let p = finish(img, 8, 8, Fit::Pad, [255, 255, 255]);
        assert_eq!((p.width, p.height), (8, 8));
        let v = p.as_u8().unwrap();
        assert_eq!(&v[0..3], &[255, 255, 255]);
        assert_eq!(&v[(2 * 8 + 3) * 3..(2 * 8 + 3) * 3 + 3], &[10, 10, 10]);
    }
}

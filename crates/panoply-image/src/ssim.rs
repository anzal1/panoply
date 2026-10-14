//! SSIM, written for Panoply (the usual Rust and C crates for it are GPL or AGPL).
//!
//! This is the mean structural similarity index of Wang, Bovik, Sheikh and Simoncelli (2004):
//! an 11 by 11 Gaussian window with sigma 1.5, constants K1 = 0.01 and K2 = 0.03 on a 0 to 255
//! range, computed on luma (BT.601 weights of the sRGB coded values, the same luma that `PIL`
//! and scikit-image use for gray conversion) and averaged over every window that fits inside
//! the picture ("valid" convolution, no padding). A score of 1.0 means identical.
//!
//! Work is done in bands of rows with separable filters, so memory is a few megabytes however
//! large the picture is. Pictures smaller than the window fall back to one global score.

use crate::types::{Image, Layout, Samples};

const WIN: usize = 11;
const SIGMA: f32 = 1.5;
const C1: f32 = (0.01 * 255.0) * (0.01 * 255.0);
const C2: f32 = (0.03 * 255.0) * (0.03 * 255.0);
/// Output rows per band.
const BAND: usize = 32;

fn gaussian() -> [f32; WIN] {
    let mut g = [0f32; WIN];
    let mut sum = 0.0;
    for (i, v) in g.iter_mut().enumerate() {
        let x = i as f32 - (WIN / 2) as f32;
        *v = (-(x * x) / (2.0 * SIGMA * SIGMA)).exp();
        sum += *v;
    }
    g.iter_mut().for_each(|v| *v /= sum);
    g
}

/// BT.601 luma of an image as 8 bit samples. Alpha is ignored (flatten first if it matters).
pub fn luma(img: &Image) -> Vec<u8> {
    let ch = img.layout.channels();
    let px = img.width as usize * img.height as usize;
    let mut out = Vec::with_capacity(px);
    let to8 = |v: u16| ((v as u32 * 255 + 32767) / 65535) as u8;
    match (&img.samples, img.layout) {
        (Samples::U8(v), Layout::Gray | Layout::GrayAlpha) => {
            out.extend(v.chunks_exact(ch).map(|p| p[0]))
        }
        (Samples::U16(v), Layout::Gray | Layout::GrayAlpha) => {
            out.extend(v.chunks_exact(ch).map(|p| to8(p[0])))
        }
        (Samples::U8(v), _) => out.extend(v.chunks_exact(ch).map(|p| {
            ((p[0] as u32 * 299 + p[1] as u32 * 587 + p[2] as u32 * 114 + 500) / 1000) as u8
        })),
        (Samples::U16(v), _) => out.extend(v.chunks_exact(ch).map(|p| {
            let (r, g, b) = (to8(p[0]) as u32, to8(p[1]) as u32, to8(p[2]) as u32);
            ((r * 299 + g * 587 + b * 114 + 500) / 1000) as u8
        })),
    }
    out
}

/// Mean SSIM of two luma planes of the same size.
pub fn ssim_luma(a: &[u8], b: &[u8], w: usize, h: usize) -> f64 {
    assert_eq!(a.len(), w * h);
    assert_eq!(b.len(), w * h);
    if w < WIN || h < WIN {
        return ssim_global(a, b);
    }
    let g = gaussian();
    let ow = w - (WIN - 1);
    let oh = h - (WIN - 1);
    // Five filtered planes for a band: mean a, mean b, mean a*a, mean b*b, mean a*b.
    let band_in = BAND + WIN - 1;
    let mut hp: [Vec<f32>; 5] = std::array::from_fn(|_| vec![0f32; band_in * ow]);
    let mut fa = vec![0f32; w];
    let mut fb = vec![0f32; w];
    let mut faa = vec![0f32; w];
    let mut fbb = vec![0f32; w];
    let mut fab = vec![0f32; w];
    let mut acc: [Vec<f32>; 5] = std::array::from_fn(|_| vec![0f32; ow]);
    let mut total = 0f64;
    let mut y0 = 0;
    while y0 < oh {
        let rows = BAND.min(oh - y0);
        let in_rows = rows + WIN - 1;
        // Horizontal pass over the input rows of this band.
        for r in 0..in_rows {
            let y = y0 + r;
            let (ra, rb) = (&a[y * w..(y + 1) * w], &b[y * w..(y + 1) * w]);
            for x in 0..w {
                let (va, vb) = (ra[x] as f32, rb[x] as f32);
                fa[x] = va;
                fb[x] = vb;
                faa[x] = va * va;
                fbb[x] = vb * vb;
                fab[x] = va * vb;
            }
            for (p, src) in [&fa, &fb, &faa, &fbb, &fab].into_iter().enumerate() {
                let dst = &mut hp[p][r * ow..(r + 1) * ow];
                dst.iter_mut().for_each(|d| *d = 0.0);
                for (k, &gk) in g.iter().enumerate() {
                    let s = &src[k..k + ow];
                    for (d, &v) in dst.iter_mut().zip(s) {
                        *d += gk * v;
                    }
                }
            }
        }
        // Vertical pass and the SSIM formula.
        for r in 0..rows {
            for p in 0..5 {
                acc[p].iter_mut().for_each(|d| *d = 0.0);
                for (k, &gk) in g.iter().enumerate() {
                    let s = &hp[p][(r + k) * ow..(r + k + 1) * ow];
                    for (d, &v) in acc[p].iter_mut().zip(s) {
                        *d += gk * v;
                    }
                }
            }
            let mut row_sum = 0f32;
            #[allow(clippy::needless_range_loop)] // five parallel planes indexed by column
            for x in 0..ow {
                let (ma, mb) = (acc[0][x], acc[1][x]);
                let va = acc[2][x] - ma * ma;
                let vb = acc[3][x] - mb * mb;
                let cov = acc[4][x] - ma * mb;
                let num = (2.0 * ma * mb + C1) * (2.0 * cov + C2);
                let den = (ma * ma + mb * mb + C1) * (va + vb + C2);
                row_sum += num / den;
            }
            total += row_sum as f64;
        }
        y0 += rows;
    }
    total / (ow as f64 * oh as f64)
}

/// One score from whole-picture statistics, for pictures smaller than the window.
fn ssim_global(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().max(1) as f64;
    let ma = a.iter().map(|&v| v as f64).sum::<f64>() / n;
    let mb = b.iter().map(|&v| v as f64).sum::<f64>() / n;
    let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (dx, dy) = (x as f64 - ma, y as f64 - mb);
        va += dx * dx;
        vb += dy * dy;
        cov += dx * dy;
    }
    va /= n;
    vb /= n;
    cov /= n;
    let (c1, c2) = (C1 as f64, C2 as f64);
    ((2.0 * ma * mb + c1) * (2.0 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2))
}

/// SSIM of luma and of chroma. Luma alone cannot see color bleeding, which is what a heavily
/// compressed JPEG shows first, so a score that stands for "looks the same" needs both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Score {
    /// SSIM of the luma plane.
    pub luma: f64,
    /// Mean SSIM of the Cb and Cr planes (1.0 for gray pictures).
    pub chroma: f64,
}

impl Score {
    /// The number a quality target is held to: the worse of the two.
    pub fn worst(&self) -> f64 {
        self.luma.min(self.chroma)
    }
}

/// Y, Cb and Cr planes (BT.601 full range) of an image. Chroma is box averaged 2 by 2, which
/// is how most encoders store it and keeps the score fast. Transparent pixels are mixed with
/// mid gray first, so edge halos show up on both light and dark backgrounds.
struct Planes {
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
    w: usize,
    h: usize,
    cw: usize,
    ch: usize,
    gray: bool,
}

fn planes(img: &Image) -> Planes {
    let (w, h) = (img.width as usize, img.height as usize);
    let gray = img.layout.is_gray();
    let lay = img.layout;
    let chn = lay.channels();
    let to8 = |v: u16| ((v as u32 * 255 + 32767) / 65535) as u8;
    let img8;
    let samples: &[u8] = match &img.samples {
        Samples::U8(v) => v,
        Samples::U16(v) => {
            img8 = v.iter().map(|&s| to8(s)).collect::<Vec<u8>>();
            &img8
        }
    };
    let px = |i: usize| -> [i32; 3] {
        let p = &samples[i * chn..(i + 1) * chn];
        let (r, g, b, a) = match lay {
            Layout::Gray => (p[0] as i32, p[0] as i32, p[0] as i32, 255),
            Layout::GrayAlpha => (p[0] as i32, p[0] as i32, p[0] as i32, p[1] as i32),
            Layout::Rgb => (p[0] as i32, p[1] as i32, p[2] as i32, 255),
            Layout::Rgba => (p[0] as i32, p[1] as i32, p[2] as i32, p[3] as i32),
        };
        if a == 255 {
            [r, g, b]
        } else {
            let mix = |c: i32| (c * a + 128 * (255 - a) + 127) / 255;
            [mix(r), mix(g), mix(b)]
        }
    };
    let y_of =
        |c: [i32; 3]| ((c[0] * 299 + c[1] * 587 + c[2] * 114 + 500) / 1000).clamp(0, 255) as u8;
    let mut y = Vec::with_capacity(w * h);
    for i in 0..w * h {
        y.push(y_of(px(i)));
    }
    let (cw, ch) = (w / 2, h / 2);
    let (mut cb, mut cr) = (Vec::new(), Vec::new());
    if !gray && cw >= 1 && ch >= 1 {
        cb.reserve(cw * ch);
        cr.reserve(cw * ch);
        for j in 0..ch {
            for i in 0..cw {
                let (mut sb, mut sr) = (0i32, 0i32);
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let c = px((2 * j + dy) * w + 2 * i + dx);
                    sb += -169 * c[0] - 331 * c[1] + 500 * c[2];
                    sr += 500 * c[0] - 419 * c[1] - 81 * c[2];
                }
                cb.push((128 + (sb + 2000) / 4000).clamp(0, 255) as u8);
                cr.push((128 + (sr + 2000) / 4000).clamp(0, 255) as u8);
            }
        }
    }
    Planes {
        y,
        cb,
        cr,
        w,
        h,
        cw,
        ch,
        gray,
    }
}

/// A reference picture with its planes computed once, for scoring many candidates against it.
pub struct Reference {
    planes: Planes,
    width: u32,
    height: u32,
}

impl Reference {
    /// Prepare `img` as the reference.
    pub fn new(img: &Image) -> Reference {
        Reference {
            planes: planes(img),
            width: img.width,
            height: img.height,
        }
    }

    /// Score `other` (same size) against the reference.
    pub fn score(&self, other: &Image) -> Score {
        assert_eq!(
            (self.width, self.height),
            (other.width, other.height),
            "SSIM needs pictures of the same size"
        );
        let pb = planes(other);
        let pa = &self.planes;
        let luma = ssim_luma(&pa.y, &pb.y, pa.w, pa.h);
        let chroma = if pa.gray || pb.gray || pa.cb.is_empty() {
            1.0
        } else {
            (ssim_luma(&pa.cb, &pb.cb, pa.cw, pa.ch) + ssim_luma(&pa.cr, &pb.cr, pa.cw, pa.ch))
                / 2.0
        };
        Score { luma, chroma }
    }
}

/// Luma and chroma SSIM of two images of the same size.
pub fn score(a: &Image, b: &Image) -> Score {
    Reference::new(a).score(b)
}

/// Estimate the quality (1 to 100) a JPEG was saved at from its luma quantization table, the
/// way ImageMagick's `identify` does: compare with the standard table scaled to every quality.
/// Returns `None` when the file has no usable table.
pub fn estimate_jpeg_quality(jpeg: &[u8]) -> Option<f32> {
    const STD_LUMA: [u32; 64] = [
        16, 11, 10, 16, 24, 40, 51, 61, 12, 12, 14, 19, 26, 58, 60, 55, 14, 13, 16, 24, 40, 57, 69,
        56, 14, 17, 22, 29, 51, 87, 80, 62, 18, 22, 37, 56, 68, 109, 103, 77, 24, 35, 55, 64, 81,
        104, 113, 92, 49, 64, 78, 87, 103, 121, 120, 101, 72, 92, 95, 98, 112, 100, 103, 99,
    ];
    let mut i = 2;
    while i + 4 <= jpeg.len() && jpeg[i] == 0xFF {
        let m = jpeg[i + 1];
        if m == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        if m == 0xDB {
            let seg = jpeg.get(i + 4..i + 2 + len)?;
            let mut p = 0;
            while p < seg.len() {
                let (pq, tq) = (seg[p] >> 4, seg[p] & 15);
                p += 1;
                let size = if pq == 0 { 64 } else { 128 };
                let data = seg.get(p..p + size)?;
                p += size;
                if tq == 0 {
                    // Table entries are stored in zigzag order; the standard table above is in
                    // natural order. A sum over all entries does not care about the order.
                    let sum: u32 = if pq == 0 {
                        data.iter().map(|&v| v as u32).sum()
                    } else {
                        data.as_chunks::<2>()
                            .0
                            .iter()
                            .map(|c| u16::from_be_bytes([c[0], c[1]]) as u32)
                            .sum()
                    };
                    let std_sum: u32 = STD_LUMA.iter().sum();
                    // scale = 100 * sum / std_sum, then invert the IJG scaling formula.
                    let scale = 100.0 * sum as f32 / std_sum as f32;
                    let q = if scale <= 100.0 {
                        (200.0 - scale) / 2.0
                    } else {
                        5000.0 / scale
                    };
                    return Some(q.clamp(1.0, 100.0));
                }
            }
        }
        i += 2 + len;
    }
    None
}

/// Mean SSIM of two images of the same size, on luma.
pub fn ssim(a: &Image, b: &Image) -> f64 {
    assert_eq!(
        (a.width, a.height),
        (b.width, b.height),
        "SSIM needs pictures of the same size"
    );
    ssim_luma(&luma(a), &luma(b), a.width as usize, a.height as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Depth;

    /// A slow, plain f64 version of the same definition, as the oracle.
    fn naive(a: &[u8], b: &[u8], w: usize, h: usize) -> f64 {
        let g64: Vec<f64> = {
            let raw: Vec<f64> = (0..WIN)
                .map(|i| (-((i as f64 - 5.0).powi(2)) / (2.0 * 1.5f64.powi(2))).exp())
                .collect();
            let s: f64 = raw.iter().sum();
            raw.iter().map(|v| v / s).collect()
        };
        let (c1, c2) = (0.01f64 * 255.0, 0.03f64 * 255.0);
        let (c1, c2) = (c1 * c1, c2 * c2);
        let mut sum = 0.0;
        let mut n = 0.0;
        for y in 0..=(h - WIN) {
            for x in 0..=(w - WIN) {
                let (mut ma, mut mb, mut maa, mut mbb, mut mab) = (0.0, 0.0, 0.0, 0.0, 0.0);
                for j in 0..WIN {
                    for i in 0..WIN {
                        let wgt = g64[i] * g64[j];
                        let (va, vb) =
                            (a[(y + j) * w + x + i] as f64, b[(y + j) * w + x + i] as f64);
                        ma += wgt * va;
                        mb += wgt * vb;
                        maa += wgt * va * va;
                        mbb += wgt * vb * vb;
                        mab += wgt * va * vb;
                    }
                }
                let (sa, sb, sab) = (maa - ma * ma, mbb - mb * mb, mab - ma * mb);
                sum += ((2.0 * ma * mb + c1) * (2.0 * sab + c2))
                    / ((ma * ma + mb * mb + c1) * (sa + sb + c2));
                n += 1.0;
            }
        }
        sum / n
    }

    fn pseudo(w: usize, h: usize, seed: u32) -> Vec<u8> {
        let mut s = seed;
        (0..w * h)
            .map(|i| {
                s = s.wrapping_mul(1664525).wrapping_add(1013904223);
                let (x, y) = (i % w, i / w);
                ((x * 3 + y * 2) as u32 % 200 + (s >> 28)) as u8
            })
            .collect()
    }

    #[test]
    fn identical_pictures_score_one() {
        let a = pseudo(64, 48, 1);
        assert!((ssim_luma(&a, &a, 64, 48) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn matches_the_f64_definition() {
        for (w, h) in [(40usize, 33usize), (70, 50), (23, 90)] {
            let a = pseudo(w, h, 7);
            let b = pseudo(w, h, 99);
            let fast = ssim_luma(&a, &b, w, h);
            let slow = naive(&a, &b, w, h);
            assert!(
                (fast - slow).abs() < 2e-4,
                "{w}x{h}: fast {fast} slow {slow}"
            );
        }
    }

    #[test]
    fn more_distortion_scores_lower() {
        let a = pseudo(80, 80, 3);
        let noisy = |amount: u32| -> Vec<u8> {
            a.iter()
                .enumerate()
                .map(|(i, &v)| {
                    v.saturating_add(
                        ((i as u32).wrapping_mul(2654435761) >> 28) as u8 % (amount as u8 + 1),
                    )
                })
                .collect()
        };
        let s_low = ssim_luma(&a, &noisy(4), 80, 80);
        let s_high = ssim_luma(&a, &noisy(40), 80, 80);
        assert!(s_low > s_high);
        assert!(s_high < 1.0);
    }

    #[test]
    fn tiny_pictures_use_the_global_score() {
        let a = vec![10u8, 20, 30, 40];
        assert!((ssim_luma(&a, &a, 2, 2) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn luma_weights_are_bt601() {
        let mut img = Image::new(3, 1, Layout::Rgb, Depth::U8);
        if let Samples::U8(v) = &mut img.samples {
            v.copy_from_slice(&[255, 0, 0, 0, 255, 0, 0, 0, 255]);
        }
        assert_eq!(luma(&img), vec![76, 150, 29]);
    }

    #[test]
    fn color_bleeding_shows_up_in_chroma_not_luma() {
        // Same luma everywhere, but a red and green stripe pattern is smeared into gray-ish.
        let mut a = Image::new(32, 32, Layout::Rgb, Depth::U8);
        let mut b = Image::new(32, 32, Layout::Rgb, Depth::U8);
        if let (Samples::U8(va), Samples::U8(vb)) = (&mut a.samples, &mut b.samples) {
            for (i, p) in va.chunks_exact_mut(3).enumerate() {
                let x = i % 32;
                p.copy_from_slice(if x % 4 < 2 {
                    &[220, 80, 80]
                } else {
                    &[80, 160, 80]
                });
            }
            for (i, p) in vb.chunks_exact_mut(3).enumerate() {
                let _ = i;
                p.copy_from_slice(&[132, 117, 80]);
            }
        }
        let s = score(&a, &b);
        assert!(s.chroma < s.luma + 0.2);
        assert!(s.worst() <= s.luma);
        let same = score(&a, &a);
        assert!((same.luma - 1.0).abs() < 1e-6 && (same.chroma - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jpeg_quality_estimate_is_near_the_truth() {
        // Build a JPEG-like header with a quality 75 table: standard table scaled by 50 percent.
        let std: [u8; 64] = [
            16, 11, 12, 14, 12, 10, 16, 14, 13, 14, 18, 17, 16, 19, 24, 40, 26, 24, 22, 22, 24, 49,
            35, 37, 29, 40, 58, 51, 61, 60, 57, 51, 56, 55, 64, 72, 92, 78, 64, 68, 87, 69, 55, 56,
            80, 109, 81, 87, 95, 98, 103, 104, 103, 62, 77, 113, 121, 112, 100, 120, 92, 101, 103,
            99,
        ];
        let q75: Vec<u8> = std
            .iter()
            .map(|&v| ((v as u32 * 50 + 50) / 100).max(1) as u8)
            .collect();
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xDB, 0, 67, 0];
        jpeg.extend_from_slice(&q75);
        jpeg.extend_from_slice(&[0xFF, 0xDA]);
        let q = estimate_jpeg_quality(&jpeg).unwrap();
        assert!((q - 75.0).abs() < 3.0, "estimated {q}");
    }

    #[test]
    fn bands_do_not_change_the_score() {
        // Taller than one band, so the vertical overlap logic is exercised.
        let (w, h) = (50usize, 100usize);
        let a = pseudo(w, h, 11);
        let b = pseudo(w, h, 12);
        let s = ssim_luma(&a, &b, w, h);
        assert!((s - naive(&a, &b, w, h)).abs() < 2e-4);
    }
}

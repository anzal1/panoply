//! Color handling (IPC-2 and IPC-5).
//!
//! - Transfer curves: look-up tables for sRGB to linear light and back, used by the resampler.
//! - ICC profiles: [`classify_rgb_profile`] decides by probing whether a profile is plain sRGB,
//!   a wide gamut profile with the sRGB curve (Display P3), or something else.
//! - [`ColorConv`] turns the raw rows a decoder produced (gray, RGB, CMYK, with or without a
//!   profile) into working rows: sRGB, or the source's own wide gamut when the caller keeps it.
//! - [`BlueNoise`] is the dither mask used whenever 16 bit samples go down to 8 bit.

use std::sync::{Arc, OnceLock};

use moxcms::{ColorProfile, DataColorSpace, Layout as CmsLayout, TransformOptions};

use crate::error::{ImageError, Result};
use crate::types::{Depth, Layout, Rows};

// ---------------------------------------------------------------- transfer curves

fn srgb_to_linear(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(v: f64) -> f64 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// The look-up tables. 8 bit sRGB goes to 16 bit linear. 16 bit linear goes back to 8 or 16 bit
/// sRGB. 16 bit sRGB goes to 16 bit linear.
pub struct Luts {
    /// sRGB 8 bit to linear 16 bit.
    pub s8_to_lin16: [u16; 256],
    /// Linear 16 bit to sRGB 8 bit.
    pub lin16_to_s8: Vec<u8>,
    /// Linear 16 bit to sRGB 16 bit.
    pub lin16_to_s16: Vec<u16>,
    /// sRGB 16 bit to linear 16 bit.
    pub s16_to_lin16: Vec<u16>,
}

/// The shared tables, built on first use (about 1 ms).
pub fn luts() -> &'static Luts {
    static LUTS: OnceLock<Luts> = OnceLock::new();
    LUTS.get_or_init(|| {
        let mut s8 = [0u16; 256];
        for (i, o) in s8.iter_mut().enumerate() {
            *o = (srgb_to_linear(i as f64 / 255.0) * 65535.0).round() as u16;
        }
        let mut l8 = vec![0u8; 65536];
        let mut l16 = vec![0u16; 65536];
        let mut s16 = vec![0u16; 65536];
        for i in 0..65536usize {
            let lin = i as f64 / 65535.0;
            let enc = linear_to_srgb(lin);
            l8[i] = (enc * 255.0).round() as u8;
            l16[i] = (enc * 65535.0).round() as u16;
            s16[i] = (srgb_to_linear(i as f64 / 65535.0) * 65535.0).round() as u16;
        }
        Luts {
            s8_to_lin16: s8,
            lin16_to_s8: l8,
            lin16_to_s16: l16,
            s16_to_lin16: s16,
        }
    })
}

// ---------------------------------------------------------------- ICC

/// What a profile means for the pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IccKind {
    /// Behaves like sRGB: no conversion needed.
    Srgb,
    /// Wide gamut primaries with the sRGB curve (Display P3): can be kept as is.
    WideGamut,
    /// Anything else (Adobe RGB, ProPhoto, camera profiles): convert to sRGB.
    Other,
}

fn opts() -> TransformOptions {
    TransformOptions::default()
}

fn parse_profile(icc: &[u8]) -> Result<ColorProfile> {
    ColorProfile::new_from_slice(icc)
        .map_err(|e| ImageError::Corrupt(format!("The embedded color profile could not be read ({e:?}). Try exporting the image again without it.")))
}

/// Decide what an RGB profile is by sending probe colors through it into sRGB.
///
/// Gray ramp unchanged and primaries unchanged means plain sRGB. Gray ramp unchanged but the
/// primaries moved means a wide gamut profile with the sRGB curve. Anything else is "other".
pub fn classify_rgb_profile(icc: &[u8]) -> Result<IccKind> {
    let src = parse_profile(icc)?;
    if src.color_space != DataColorSpace::Rgb {
        return Ok(IccKind::Other);
    }
    let dst = ColorProfile::new_srgb();
    let t = match src.create_transform_8bit(CmsLayout::Rgb, &dst, CmsLayout::Rgb, opts()) {
        Ok(t) => t,
        Err(_) => return Ok(IccKind::Other),
    };
    let grays: [u8; 18] = [
        0, 0, 0, 40, 40, 40, 90, 90, 90, 128, 128, 128, 190, 190, 190, 255, 255, 255,
    ];
    // Moderately saturated probes: fully saturated ones clip in sRGB and hide the difference.
    let colors: [u8; 18] = [
        200, 60, 60, 60, 200, 60, 60, 60, 200, 200, 200, 60, 60, 200, 200, 200, 60, 200,
    ];
    let mut out_g = [0u8; 18];
    let mut out_c = [0u8; 18];
    t.transform(&grays, &mut out_g).map_err(|_| {
        ImageError::Unsupported(
            "This color profile cannot be used. Try exporting the image as sRGB.".into(),
        )
    })?;
    t.transform(&colors, &mut out_c).map_err(|_| {
        ImageError::Unsupported(
            "This color profile cannot be used. Try exporting the image as sRGB.".into(),
        )
    })?;
    let dev = |a: &[u8], b: &[u8]| {
        a.iter()
            .zip(b)
            .map(|(&x, &y)| (x as i32 - y as i32).abs())
            .max()
            .unwrap_or(0)
    };
    let gray_dev = dev(&grays, &out_g);
    let color_dev = dev(&colors, &out_c);
    Ok(if gray_dev <= 2 && color_dev <= 2 {
        IccKind::Srgb
    } else if gray_dev <= 3 {
        IccKind::WideGamut
    } else {
        IccKind::Other
    })
}

/// A standard Display P3 profile, for tagging pixels that came out of the OS decoder in P3.
pub fn display_p3_icc() -> Arc<Vec<u8>> {
    static P3: OnceLock<Arc<Vec<u8>>> = OnceLock::new();
    P3.get_or_init(|| Arc::new(ColorProfile::new_display_p3().encode().unwrap_or_default()))
        .clone()
}

// ---------------------------------------------------------------- converter

/// What the decoder tells us about the color of the raw rows.
#[derive(Debug, Clone, Default)]
pub struct SrcColor {
    /// The embedded ICC profile, if any.
    pub icc: Option<Vec<u8>>,
    /// PNG gAMA value (for example 0.45455), when there is no profile.
    pub png_gamma: Option<f32>,
    /// The samples are CMYK (four inks, no alpha).
    pub cmyk: bool,
    /// CMYK samples are stored inverted (Adobe Photoshop JPEG and PSD style, 255 = no ink).
    pub cmyk_inverted: bool,
}

/// What the caller wants out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutSpace {
    /// Convert everything to sRGB (web and portal targets).
    Srgb,
    /// Keep a wide gamut source as it is, and say which profile to embed (archival, print).
    Keep,
}

enum Plan {
    Identity,
    Cms8(Arc<moxcms::Transform8BitExecutor>),
    Cms16(Arc<moxcms::Transform16BitExecutor>),
    Gamma8(Box<[u8; 256]>),
    Gamma16(Vec<u16>),
    NaiveCmyk,
}

/// Turns raw decoded rows into working rows.
pub struct ColorConv {
    plan: Plan,
    raw: Layout,
    raw_cmyk: bool,
    inverted: bool,
    /// Layout of the rows that come out.
    pub out_layout: Layout,
    /// The profile the output pixels are in, when it is not sRGB (embed it on encode).
    pub out_icc: Option<Arc<Vec<u8>>>,
    /// A line for the result card when the pixels were converted.
    pub note: Option<String>,
}

impl ColorConv {
    /// Plan the conversion for rows of layout `raw` and depth `depth`.
    pub fn new(raw: Layout, depth: Depth, color: &SrcColor, out: OutSpace) -> Result<ColorConv> {
        let mut out_icc = None;
        let mut note = None;
        let out_layout = if color.cmyk { Layout::Rgb } else { raw };
        let cms_in = |l: Layout| match (color.cmyk, l) {
            (true, _) => CmsLayout::Rgba,
            (false, Layout::Gray) => CmsLayout::Gray,
            (false, Layout::GrayAlpha) => CmsLayout::GrayAlpha,
            (false, Layout::Rgb) => CmsLayout::Rgb,
            (false, Layout::Rgba) => CmsLayout::Rgba,
        };
        let srgb = ColorProfile::new_srgb();
        let plan = if color.cmyk {
            match color.icc.as_deref().map(parse_profile) {
                Some(Ok(p)) if p.color_space == DataColorSpace::Cmyk => {
                    note = Some("converted CMYK to sRGB with the embedded profile".to_string());
                    cms_plan(&p, cms_in(raw), &srgb, CmsLayout::Rgb, depth)?
                }
                _ => {
                    note = Some("converted CMYK to sRGB (no profile in the file)".to_string());
                    Plan::NaiveCmyk
                }
            }
        } else if let Some(icc) = &color.icc {
            let prof = parse_profile(icc)?;
            match (raw.is_gray(), prof.color_space) {
                (true, DataColorSpace::Gray) => {
                    // A gray profile (dot gain, gamma 1.8 and so on): bring it to sRGB gray.
                    let t_in = cms_in(raw);
                    let t_out = if raw.has_alpha() {
                        CmsLayout::Rgba
                    } else {
                        CmsLayout::Rgb
                    };
                    note = Some("converted gray to sRGB".to_string());
                    cms_plan(&prof, t_in, &srgb, t_out, depth)?
                }
                (false, DataColorSpace::Rgb) => match classify_rgb_profile(icc)? {
                    IccKind::Srgb => Plan::Identity,
                    IccKind::WideGamut if out == OutSpace::Keep => {
                        out_icc = Some(Arc::new(icc.clone()));
                        Plan::Identity
                    }
                    _ => {
                        note = Some("converted colors to sRGB".to_string());
                        cms_plan(&prof, cms_in(raw), &srgb, cms_in(raw), depth)?
                    }
                },
                _ => Plan::Identity,
            }
        } else if let Some(g) = color
            .png_gamma
            .filter(|g| (*g - 0.45455).abs() > 0.012 && *g > 0.0)
        {
            // No profile, but the file says how it was encoded: decode with that gamma, encode as sRGB.
            note = Some("applied the PNG gamma".to_string());
            match depth {
                Depth::U8 => {
                    let mut t = Box::new([0u8; 256]);
                    for (i, o) in t.iter_mut().enumerate() {
                        let lin = (i as f64 / 255.0).powf(1.0 / g as f64);
                        *o = (linear_to_srgb(lin.clamp(0.0, 1.0)) * 255.0).round() as u8;
                    }
                    Plan::Gamma8(t)
                }
                Depth::U16 => {
                    let t = (0..65536usize)
                        .map(|i| {
                            let lin = (i as f64 / 65535.0).powf(1.0 / g as f64);
                            (linear_to_srgb(lin.clamp(0.0, 1.0)) * 65535.0).round() as u16
                        })
                        .collect();
                    Plan::Gamma16(t)
                }
            }
        } else {
            Plan::Identity
        };
        Ok(ColorConv {
            plan,
            raw,
            raw_cmyk: color.cmyk,
            inverted: color.cmyk && color.cmyk_inverted,
            out_layout,
            out_icc,
            note,
        })
    }

    /// True when rows come out exactly as they went in.
    pub fn is_identity(&self) -> bool {
        matches!(self.plan, Plan::Identity)
    }

    /// Convert 8 bit rows. `dst` is resized to fit.
    pub fn convert_u8(&self, src: &[u8], dst: &mut Vec<u8>) -> Result<()> {
        let ch_in = if self.raw_cmyk {
            4
        } else {
            self.raw.channels()
        };
        let ch_out = self.out_layout.channels();
        let px = src.len() / ch_in;
        dst.clear();
        dst.resize(px * ch_out, 0);
        match &self.plan {
            Plan::Identity => dst.copy_from_slice(src),
            Plan::Gamma8(t) => {
                let alpha_at = if self.raw.has_alpha() {
                    Some(ch_in - 1)
                } else {
                    None
                };
                for (s, d) in src.chunks_exact(ch_in).zip(dst.chunks_exact_mut(ch_out)) {
                    for (i, (&sv, dv)) in s.iter().zip(d.iter_mut()).enumerate() {
                        *dv = if Some(i) == alpha_at {
                            sv
                        } else {
                            t[sv as usize]
                        };
                    }
                }
            }
            Plan::NaiveCmyk => {
                for (s, d) in src
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst.as_chunks_mut::<3>().0.iter_mut())
                {
                    let (c, m, y, k) = if self.inverted {
                        (
                            255 - s[0] as u32,
                            255 - s[1] as u32,
                            255 - s[2] as u32,
                            255 - s[3] as u32,
                        )
                    } else {
                        (s[0] as u32, s[1] as u32, s[2] as u32, s[3] as u32)
                    };
                    d[0] = ((255 - c) * (255 - k) / 255) as u8;
                    d[1] = ((255 - m) * (255 - k) / 255) as u8;
                    d[2] = ((255 - y) * (255 - k) / 255) as u8;
                }
            }
            Plan::Cms8(t) => {
                if self.inverted {
                    let inv: Vec<u8> = src.iter().map(|v| 255 - v).collect();
                    run8(t, &inv, dst, self.raw, self.raw_cmyk, self.out_layout)?;
                } else {
                    run8(t, src, dst, self.raw, self.raw_cmyk, self.out_layout)?;
                }
            }
            Plan::Cms16(_) | Plan::Gamma16(_) => {
                return Err(ImageError::Failed(
                    "internal: 8 bit rows sent to a 16 bit color plan".into(),
                ));
            }
        }
        Ok(())
    }

    /// Convert 16 bit rows. `dst` is resized to fit.
    pub fn convert_u16(&self, src: &[u16], dst: &mut Vec<u16>) -> Result<()> {
        let ch_in = if self.raw_cmyk {
            4
        } else {
            self.raw.channels()
        };
        let ch_out = self.out_layout.channels();
        let px = src.len() / ch_in;
        dst.clear();
        dst.resize(px * ch_out, 0);
        match &self.plan {
            Plan::Identity => dst.copy_from_slice(src),
            Plan::Gamma16(t) => {
                let alpha_at = if self.raw.has_alpha() {
                    Some(ch_in - 1)
                } else {
                    None
                };
                for (s, d) in src.chunks_exact(ch_in).zip(dst.chunks_exact_mut(ch_out)) {
                    for (i, (&sv, dv)) in s.iter().zip(d.iter_mut()).enumerate() {
                        *dv = if Some(i) == alpha_at {
                            sv
                        } else {
                            t[sv as usize]
                        };
                    }
                }
            }
            Plan::NaiveCmyk => {
                for (s, d) in src
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst.as_chunks_mut::<3>().0.iter_mut())
                {
                    let (c, m, y, k) = if self.inverted {
                        (
                            65535 - s[0] as u64,
                            65535 - s[1] as u64,
                            65535 - s[2] as u64,
                            65535 - s[3] as u64,
                        )
                    } else {
                        (s[0] as u64, s[1] as u64, s[2] as u64, s[3] as u64)
                    };
                    d[0] = ((65535 - c) * (65535 - k) / 65535) as u16;
                    d[1] = ((65535 - m) * (65535 - k) / 65535) as u16;
                    d[2] = ((65535 - y) * (65535 - k) / 65535) as u16;
                }
            }
            Plan::Cms16(t) => {
                let r = if self.inverted {
                    let inv: Vec<u16> = src.iter().map(|v| 65535 - v).collect();
                    t.transform(&inv, dst)
                } else {
                    t.transform(src, dst)
                };
                r.map_err(|e| ImageError::Failed(format!("Color conversion failed ({e:?}).")))?;
            }
            Plan::Cms8(_) | Plan::Gamma8(_) => {
                return Err(ImageError::Failed(
                    "internal: 16 bit rows sent to an 8 bit color plan".into(),
                ));
            }
        }
        Ok(())
    }

    /// Convert borrowed rows of either depth into `dst`, which must match the depth.
    pub fn convert(&self, src: Rows<'_>, dst: &mut crate::pipeline::RowBuf) -> Result<()> {
        match (src, dst) {
            (Rows::U8(s), crate::pipeline::RowBuf::U8(d)) => self.convert_u8(s, d),
            (Rows::U16(s), crate::pipeline::RowBuf::U16(d)) => self.convert_u16(s, d),
            _ => Err(ImageError::Failed(
                "internal: mismatched sample depth".into(),
            )),
        }
    }
}

fn run8(
    t: &Arc<moxcms::Transform8BitExecutor>,
    src: &[u8],
    dst: &mut [u8],
    raw: Layout,
    cmyk: bool,
    out: Layout,
) -> Result<()> {
    let err = |e: moxcms::CmsError| ImageError::Failed(format!("Color conversion failed ({e:?})."));
    if !cmyk && raw.is_gray() && out == raw {
        // Gray goes through an RGB(A) result, then back to gray.
        let ch = raw.channels();
        let rgb_ch = if raw.has_alpha() { 4 } else { 3 };
        let mut tmp = vec![0u8; src.len() / ch * rgb_ch];
        t.transform(src, &mut tmp).map_err(err)?;
        for (p, d) in tmp.chunks_exact(rgb_ch).zip(dst.chunks_exact_mut(ch)) {
            d[0] = p[0];
            if raw.has_alpha() {
                d[1] = p[3];
            }
        }
        return Ok(());
    }
    t.transform(src, dst).map_err(err)
}

fn cms_plan(
    src: &ColorProfile,
    in_l: CmsLayout,
    dst: &ColorProfile,
    out_l: CmsLayout,
    depth: Depth,
) -> Result<Plan> {
    let err = |e: moxcms::CmsError| {
        ImageError::Unsupported(format!(
            "This color profile cannot be used ({e:?}). Try exporting the image as sRGB."
        ))
    };
    Ok(match depth {
        Depth::U8 => Plan::Cms8(
            src.create_transform_8bit(in_l, dst, out_l, opts())
                .map_err(err)?,
        ),
        Depth::U16 => Plan::Cms16(
            src.create_transform_16bit(in_l, dst, out_l, opts())
                .map_err(err)?,
        ),
    })
}

// ---------------------------------------------------------------- blue noise

/// A 64 by 64 blue noise threshold mask made by the void and cluster method (Ulichney 1993).
/// Values are spread evenly over 0..4096, and neighbors differ, so dither error has no
/// visible pattern.
pub struct BlueNoise {
    mask: Vec<u16>,
}

impl BlueNoise {
    const N: usize = 64;

    /// The shared mask, built on first use.
    pub fn shared() -> &'static BlueNoise {
        static BN: OnceLock<BlueNoise> = OnceLock::new();
        BN.get_or_init(BlueNoise::build)
    }

    /// Threshold in `[0, 1)` for pixel `(x, y)`.
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        let i = (y % Self::N) * Self::N + (x % Self::N);
        (self.mask[i] as f32 + 0.5) / 4096.0
    }

    fn build() -> BlueNoise {
        const N: usize = BlueNoise::N;
        const TOTAL: usize = N * N;
        // Gaussian energy kernel on the torus, sigma 1.5.
        let sigma2 = 2.0 * 1.5f32 * 1.5f32;
        let mut kernel = vec![0f32; TOTAL];
        for dy in 0..N {
            for dx in 0..N {
                let x = dx.min(N - dx) as f32;
                let y = dy.min(N - dy) as f32;
                kernel[dy * N + dx] = (-(x * x + y * y) / sigma2).exp();
            }
        }
        let mut energy = vec![0f32; TOTAL];
        let mut pattern = vec![false; TOTAL];
        let mut state = 0x9E3779B97F4A7C15u64;
        let mut rnd = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        let splat = |energy: &mut [f32], idx: usize, sign: f32| {
            let (px, py) = (idx % N, idx / N);
            for y in 0..N {
                let ky = (y + N - py) % N;
                for x in 0..N {
                    let kx = (x + N - px) % N;
                    energy[y * N + x] += sign * kernel[ky * N + kx];
                }
            }
        };
        // Initial pattern: about 10 percent ones at random places.
        let ones = TOTAL / 10;
        let mut placed = 0;
        while placed < ones {
            let i = (rnd() % TOTAL as u64) as usize;
            if !pattern[i] {
                pattern[i] = true;
                splat(&mut energy, i, 1.0);
                placed += 1;
            }
        }
        let tightest = |p: &[bool], e: &[f32]| {
            (0..TOTAL)
                .filter(|&i| p[i])
                .max_by(|&a, &b| e[a].total_cmp(&e[b]))
                .unwrap()
        };
        let largest_void = |p: &[bool], e: &[f32]| {
            (0..TOTAL)
                .filter(|&i| !p[i])
                .min_by(|&a, &b| e[a].total_cmp(&e[b]))
                .unwrap()
        };
        // Relax: move the tightest one into the largest void until it settles.
        for _ in 0..TOTAL {
            let c = tightest(&pattern, &energy);
            pattern[c] = false;
            splat(&mut energy, c, -1.0);
            let v = largest_void(&pattern, &energy);
            if v == c {
                pattern[c] = true;
                splat(&mut energy, c, 1.0);
                break;
            }
            pattern[v] = true;
            splat(&mut energy, v, 1.0);
        }
        let mut rank = vec![0u16; TOTAL];
        // Phase 1: remove ones from the initial pattern, tightest first, rank descending.
        let mut p1 = pattern.clone();
        let mut e1 = energy.clone();
        let mut r = ones;
        while r > 0 {
            let c = tightest(&p1, &e1);
            p1[c] = false;
            splat(&mut e1, c, -1.0);
            r -= 1;
            rank[c] = r as u16;
        }
        // Phase 2: add ones into the largest voids until half of the cells are set.
        let mut p2 = pattern.clone();
        let mut e2 = energy.clone();
        let mut r = ones;
        while r < TOTAL / 2 {
            let v = largest_void(&p2, &e2);
            rank[v] = r as u16;
            p2[v] = true;
            splat(&mut e2, v, 1.0);
            r += 1;
        }
        // Phase 3: the remaining cells, by the tightest cluster of zeros.
        let mut e3: Vec<f32> = e2.iter().map(|v| -v).collect();
        // For this phase use the energy of zeros: recompute from the inverse pattern.
        e3.iter_mut().for_each(|v| *v = 0.0);
        for (i, &set) in p2.iter().enumerate() {
            if !set {
                splat(&mut e3, i, 1.0);
            }
        }
        let mut p3: Vec<bool> = p2.iter().map(|&b| !b).collect();
        let mut r = TOTAL / 2;
        while r < TOTAL {
            let c = (0..TOTAL)
                .filter(|&i| p3[i])
                .max_by(|&a, &b| e3[a].total_cmp(&e3[b]))
                .unwrap();
            rank[c] = r as u16;
            p3[c] = false;
            splat(&mut e3, c, -1.0);
            r += 1;
        }
        // Scale ranks (0..4095) into the mask.
        BlueNoise { mask: rank }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn luts_round_trip_8_bit() {
        let l = luts();
        for v in 0..=255u8 {
            let lin = l.s8_to_lin16[v as usize];
            assert_eq!(l.lin16_to_s8[lin as usize], v, "value {v}");
        }
        assert_eq!(l.s8_to_lin16[0], 0);
        assert_eq!(l.s8_to_lin16[255], 65535);
    }

    #[test]
    fn luts_16_bit_endpoints() {
        let l = luts();
        assert_eq!(l.lin16_to_s16[0], 0);
        assert_eq!(l.lin16_to_s16[65535], 65535);
        assert_eq!(l.s16_to_lin16[65535], 65535);
    }

    #[test]
    fn blue_noise_is_a_permutation_with_even_spread() {
        let bn = BlueNoise::shared();
        let mut seen = vec![false; 4096];
        for y in 0..64 {
            for x in 0..64 {
                let v = (bn.at(x, y) * 4096.0) as usize;
                assert!(!seen[v.min(4095)], "duplicate rank");
                seen[v.min(4095)] = true;
            }
        }
        // Any 8 by 8 window should average near the middle for blue noise.
        let mut worst = 0.0f32;
        for wy in (0..64).step_by(8) {
            for wx in (0..64).step_by(8) {
                let mut s = 0.0;
                for y in 0..8 {
                    for x in 0..8 {
                        s += bn.at(wx + x, wy + y);
                    }
                }
                worst = worst.max((s / 64.0 - 0.5).abs());
            }
        }
        assert!(worst < 0.12, "windows are not even: {worst}");
    }

    #[test]
    fn srgb_profile_is_srgb() {
        let icc = ColorProfile::new_srgb().encode().unwrap();
        assert_eq!(classify_rgb_profile(&icc).unwrap(), IccKind::Srgb);
    }

    #[test]
    fn display_p3_is_wide_gamut_and_adobe_is_other() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        assert_eq!(classify_rgb_profile(&p3).unwrap(), IccKind::WideGamut);
        let adobe = ColorProfile::new_adobe_rgb().encode().unwrap();
        assert_eq!(classify_rgb_profile(&adobe).unwrap(), IccKind::Other);
    }

    #[test]
    fn p3_converts_to_srgb_and_keeps() {
        let p3 = ColorProfile::new_display_p3().encode().unwrap();
        let color = SrcColor {
            icc: Some(p3.clone()),
            ..Default::default()
        };
        let conv = ColorConv::new(Layout::Rgb, Depth::U8, &color, OutSpace::Srgb).unwrap();
        let mut out = Vec::new();
        conv.convert_u8(&[0, 255, 0, 128, 128, 128], &mut out)
            .unwrap();
        // Saturated P3 green is outside sRGB, so it clips to pure green. Gray stays gray.
        assert_eq!(&out[0..3], &[0, 255, 0]);
        assert!(
            out[3].abs_diff(128) <= 1 && out[4].abs_diff(128) <= 1 && out[5].abs_diff(128) <= 1
        );
        let keep = ColorConv::new(Layout::Rgb, Depth::U8, &color, OutSpace::Keep).unwrap();
        assert!(keep.is_identity());
        assert!(keep.out_icc.is_some());
    }

    #[test]
    fn naive_cmyk_handles_adobe_inversion() {
        let color = SrcColor {
            cmyk: true,
            cmyk_inverted: true,
            ..Default::default()
        };
        let conv = ColorConv::new(Layout::Rgb, Depth::U8, &color, OutSpace::Srgb).unwrap();
        let mut out = Vec::new();
        // Inverted storage: 255 everywhere means no ink, so white.
        conv.convert_u8(&[255, 255, 255, 255], &mut out).unwrap();
        assert_eq!(out, vec![255, 255, 255]);
        // Full black ink (K stored as 0).
        conv.convert_u8(&[255, 255, 255, 0], &mut out).unwrap();
        assert_eq!(out, vec![0, 0, 0]);
    }
}

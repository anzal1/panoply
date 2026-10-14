//! HDR to SDR tone-mapping 3D lookup tables.
//!
//! Panoply tone-maps video by running ffmpeg with the `lut3d` filter and a
//! `.cube` file generated here, so one table performs the whole conversion in
//! a single pass and works with any LGPL ffmpeg build (no zscale or libplacebo
//! needed). The intended ffmpeg chain is:
//!
//! 1. decoded YUV (BT.2020 matrix, PQ or HLG transfer),
//! 2. `format=gbrp16le`, giving non-linear PQ or HLG encoded R'G'B' with
//!    BT.2020 primaries,
//! 3. `lut3d=file=x.cube:interp=tetrahedral`, which is the table built here,
//! 4. BT.709 primaries with the BT.709 OETF (non-linear, 0..1), converted to
//!    limited-range BT.709 YUV by swscale.
//!
//! So the table maps encoded BT.2020 PQ/HLG triples in `[0,1]^3` to encoded
//! BT.709 SDR triples in `[0,1]^3`.
//!
//! # Pipeline (what [`map_pixel`] computes)
//!
//! 1. **Decode** to display light in nits. PQ uses the ST 2084 EOTF per channel.
//!    HLG uses the BT.2100 inverse OETF per channel, then the BT.2100 OOTF with
//!    the luminance based form `Fd = Lw * Ys^(gamma - 1) * Es`, with `Lw` of
//!    1000 nits and a system gamma of 1.2 (the value BT.2100 gives for a
//!    1000 nit display).
//! 2. **Tone map** the luminance `Y` (BT.2020 weights 0.2627, 0.6780, 0.0593,
//!    computed from the primaries) with the selected [`Curve`] and scale all
//!    three channels by `mapped Y / Y`, which preserves hue and the ratios
//!    between channels. Exposure follows BT.2408: reference diffuse white is
//!    203 nits and lands near SDR white. The source peak maps to exactly 1.0.
//! 3. **Gamut**: convert linear BT.2020 to linear BT.709 (matrix below). With
//!    saturation protection on, each channel's distance from the pixel's own
//!    luminance is soft-clipped against the room the BT.709 cube leaves on that
//!    side (a rational knee that starts at 60% of the way to the edge and
//!    approaches the edge without reaching past it), so every colour ends up in
//!    gamut by desaturating toward its luminance. With protection off, values
//!    are hard clipped per channel. A hard "smallest blend that fits" is
//!    ill-conditioned for bright colours (the allowed chroma vanishes as the
//!    luminance nears 1) and interpolates badly in a coarse lattice, which is
//!    why the soft knee is used.
//! 4. **Encode** with the BT.709 OETF.
//!
//! The BT.2020 to BT.709 linear matrix (ITU-R BT.2087, rounded to 6 places;
//! the code derives it from the primaries and the D65 white point so that every
//! row sums to 1 and neutrals stay exactly neutral):
//!
//! ```text
//!  1.660491  -0.587641  -0.072850
//! -0.124550   1.132900  -0.008349
//! -0.018151  -0.100579   1.118730
//! ```
//!
//! # Curves
//!
//! * [`Curve::Bt2390`]: the Hermite spline EETF of BT.2390 section 5.4, run in
//!   the PQ domain, normalised to the source peak, with `KS = 1.5 * maxLum - 0.5`
//!   and the black-level terms set to zero. The target peak is
//!   [`SDR_TARGET_NITS`] (180 nits), the level that renders as SDR white.
//!   Anything below the knee passes through unchanged.
//! * [`Curve::Hable`]: the Uncharted 2 filmic curve in units of diffuse white,
//!   normalised so the source peak maps to 1. The exposure bias is solved
//!   (bisection, once per table) so that 203 nits lands at a fixed SDR level
//!   instead of the usual hand tuned constant.
//! * [`Curve::Mobius`]: linear up to a transition point at 0.6 of diffuse
//!   white, then a Mobius (rational) shoulder with matched slope that reaches
//!   exactly 1 at the source peak.
//!
//! # Banding and lattice resolution
//!
//! PQ code values are very dense in the shadows, and a uniform lattice cannot
//! follow that, so the table is linearly (tetrahedrally) interpolated between
//! lattice points that are coarse in dark regions. The error stays small in
//! absolute terms because dark PQ values map to very dark SDR values, and the
//! tests measure it against the exact per-pixel function. Use a larger size if
//! shadow banding matters more than file size and load time.
//!
//! # Limitations
//!
//! * Static tone mapping only (no per-frame or per-scene analysis).
//! * HLG always assumes a 1000 nit reference display for the OOTF;
//!   `source_peak_nits` only sets the tone-mapping peak.
//! * The source peak is clamped to 300..=10000 nits.
//! * The gamut step is a soft knee, not the minimal blend: in-gamut colours more
//!   than 60% of the way to the BT.709 edge are slightly desaturated (a pure
//!   BT.709 primary loses about 20% of its distance from the luminance).
//! * With a 4000 nit or higher source, 203 nit white lands below the usual
//!   SDR white level (the shoulder has to make room for far more range).

use std::fmt::Write as _;

/// The transfer function of the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transfer {
    /// SMPTE ST 2084 perceptual quantizer (HDR10, Dolby Vision profile 8.1).
    Pq,
    /// ITU-R BT.2100 hybrid log-gamma.
    Hlg,
}

/// The tone-mapping curve for highlights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Curve {
    /// BT.2390 Hermite spline EETF applied in the PQ domain.
    Bt2390,
    /// The Uncharted 2 filmic curve, normalised so the source peak maps to 1.
    Hable,
    /// A linear segment followed by a Mobius shoulder.
    Mobius,
}

/// Table generation parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Source transfer function.
    pub transfer: Transfer,
    /// Highlight roll-off curve.
    pub curve: Curve,
    /// Peak luminance of the source in nits (PQ: from MaxCLL or mastering display
    /// metadata, default 1000; HLG: nominal 1000). Clamped to 300..=10000.
    pub source_peak_nits: f64,
    /// Protect saturated colours from clipping by desaturating toward the
    /// luminance as they near the gamut edge. When false, out of range values
    /// are hard clipped per channel.
    pub saturation_protection: bool,
    /// Lattice size per axis (for example 33, 49 or 65). Clamped to 2..=256.
    pub size: usize,
}

impl Params {
    /// Sensible defaults: [`Curve::Bt2390`], 1000 nit source peak, saturation
    /// protection on. The lattice size is 65 for both transfers: PQ is dense in
    /// the shadows and the highlight roll-off has sharp features, and 65 keeps
    /// the interpolation error at HLG well under one 8 bit code value too (a
    /// 49 point HLG table is borderline). Build time is a few tens of
    /// milliseconds; the `.cube` text is about 7 MB.
    pub fn new(transfer: Transfer) -> Params {
        Params {
            transfer,
            curve: Curve::Bt2390,
            source_peak_nits: 1000.0,
            saturation_protection: true,
            size: 65,
        }
    }
}

/// A 3D lookup table: encoded BT.2020 RGB in, encoded BT.709 RGB out.
#[derive(Debug, Clone, PartialEq)]
pub struct Lut {
    /// Lattice points per axis.
    pub size: usize,
    /// `size^3` entries, red index varying fastest, then green, then blue
    /// (the `.cube` ordering).
    pub data: Vec<[f32; 3]>,
}

/// Reference diffuse white of BT.2408 in nits.
pub const REFERENCE_WHITE_NITS: f64 = 203.0;

/// The luminance, in nits, that the BT.2390 stage treats as the SDR peak.
///
/// It sits between the 100 nit SDR mastering reference and the 203 nit HDR
/// diffuse white. The EETF rolls the top of the range off, so with this value
/// a 203 nit neutral lands at about 0.81 linear (0.90 encoded) for a 1000 nit
/// source, which keeps skin and midtones close to the SDR grade.
pub const SDR_TARGET_NITS: f64 = 180.0;

// ST 2084 constants.
const M1: f64 = 2610.0 / 16384.0;
const M2: f64 = 2523.0 / 4096.0 * 128.0;
const C1: f64 = 3424.0 / 4096.0;
const C2: f64 = 2413.0 / 4096.0 * 32.0;
const C3: f64 = 2392.0 / 4096.0 * 32.0;
const PQ_PEAK_NITS: f64 = 10000.0;

// BT.2100 HLG constants.
const HLG_A: f64 = 0.178_832_77;
const HLG_B: f64 = 1.0 - 4.0 * HLG_A;
const HLG_NOMINAL_PEAK_NITS: f64 = 1000.0;
const HLG_SYSTEM_GAMMA: f64 = 1.2;
const HLG_LUMA: [f64; 3] = [0.2627, 0.6780, 0.0593];

// Shaping constants.
const MOBIUS_TRANSITION: f64 = 0.6;
const HABLE_DIFFUSE_TARGET: f64 = 0.81;
const MIN_PEAK_NITS: f64 = 300.0;
const MAX_LATTICE: usize = 256;
/// Fraction of the gamut distance where saturation protection starts to act.
const GAMUT_KNEE: f64 = 0.6;

/// PQ encoded signal (0..1) to absolute nits (ST 2084 EOTF).
pub fn pq_to_nits(e: f64) -> f64 {
    let e = e.clamp(0.0, 1.0);
    let p = e.powf(1.0 / M2);
    let num = (p - C1).max(0.0);
    let den = C2 - C3 * p;
    PQ_PEAK_NITS * (num / den).powf(1.0 / M1)
}

/// Absolute nits to a PQ encoded signal (0..1), the inverse of [`pq_to_nits`].
///
/// Zero nits maps to exactly 0 (the raw formula gives 7e-7 there) and
/// 10000 nits maps to exactly 1.
pub fn nits_to_pq(nits: f64) -> f64 {
    if nits <= 0.0 {
        return 0.0;
    }
    let y = (nits / PQ_PEAK_NITS).min(1.0);
    let yp = y.powf(M1);
    (((C1 + C2 * yp) / (1.0 + C3 * yp)).powf(M2)).min(1.0)
}

/// BT.2100 HLG inverse OETF: encoded signal to normalised scene light.
fn hlg_inverse_oetf(e: f64) -> f64 {
    let e = e.clamp(0.0, 1.0);
    if e <= 0.5 {
        e * e / 3.0
    } else {
        let c = 0.5 - HLG_A * (4.0 * HLG_A).ln();
        (((e - c) / HLG_A).exp() + HLG_B) / 12.0
    }
}

/// BT.709 OETF.
fn bt709_oetf(l: f64) -> f64 {
    if l < 0.018 {
        4.5 * l
    } else {
        1.099 * l.powf(0.45) - 0.099
    }
}

type Mat3 = [[f64; 3]; 3];

fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn mat_vec(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

fn mat_inv(m: &Mat3) -> Mat3 {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = m[1][2] * m[2][0] - m[1][0] * m[2][2];
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    let inv_det = 1.0 / det;
    [
        [
            c00 * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            c01 * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            c02 * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ]
}

/// RGB to XYZ matrix from xy chromaticities of the primaries and white point.
fn rgb_to_xyz(prim: [[f64; 2]; 3], white: [f64; 2]) -> Mat3 {
    let xyz = |c: [f64; 2]| [c[0] / c[1], 1.0, (1.0 - c[0] - c[1]) / c[1]];
    let cols = [xyz(prim[0]), xyz(prim[1]), xyz(prim[2])];
    let mut p = [[0.0; 3]; 3];
    for (j, col) in cols.iter().enumerate() {
        for (i, v) in col.iter().enumerate() {
            p[i][j] = *v;
        }
    }
    let s = mat_vec(&mat_inv(&p), xyz(white));
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, cell) in row.iter_mut().enumerate() {
            *cell = p[i][j] * s[j];
        }
    }
    out
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Soft-clip a gamut distance above [`GAMUT_KNEE`]: slope 1 at the knee, then a
/// rational roll-off that approaches 1.0 (the gamut edge) and never exceeds it.
fn compress_distance(d: f64) -> f64 {
    let room = 1.0 - GAMUT_KNEE;
    let over = d - GAMUT_KNEE;
    GAMUT_KNEE + over * room / (over + room)
}

/// The Uncharted 2 filmic function (before normalisation).
fn hable_raw(x: f64) -> f64 {
    const A: f64 = 0.15;
    const B: f64 = 0.50;
    const C: f64 = 0.10;
    const D: f64 = 0.20;
    const E: f64 = 0.02;
    const F: f64 = 0.30;
    ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F
}

/// Solve the Hable exposure bias so diffuse white (x = 1) lands on
/// [`HABLE_DIFFUSE_TARGET`] when the source peak (x = `peak_rel`) maps to 1.
fn hable_exposure(peak_rel: f64) -> f64 {
    let level = |k: f64| hable_raw(k) / hable_raw(k * peak_rel);
    let (mut lo, mut hi) = (0.01_f64.ln(), 1000.0_f64.ln());
    for _ in 0..60 {
        let mid = 0.5 * (lo + hi);
        if level(mid.exp()) < HABLE_DIFFUSE_TARGET {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    (0.5 * (lo + hi)).exp()
}

/// The luminance shaper of the selected curve, with its derived constants.
#[derive(Debug, Clone, Copy)]
enum Shaper {
    Bt2390 {
        pq_peak: f64,
        max_lum: f64,
        knee: f64,
    },
    Hable {
        exposure: f64,
        norm: f64,
    },
    Mobius {
        beta: f64,
    },
}

/// A prepared per-pixel mapper. Building it once and calling [`Mapper::map`]
/// many times is much cheaper than calling [`map_pixel`] in a loop.
#[derive(Debug, Clone, Copy)]
pub struct Mapper {
    transfer: Transfer,
    protect: bool,
    shaper: Shaper,
    to_709: Mat3,
    luma_2020: [f64; 3],
    luma_709: [f64; 3],
}

impl Mapper {
    /// Prepare a mapper for the given parameters.
    pub fn new(p: &Params) -> Mapper {
        let peak = p.source_peak_nits.clamp(MIN_PEAK_NITS, PQ_PEAK_NITS);
        let m2020 = rgb_to_xyz(
            [[0.708, 0.292], [0.170, 0.797], [0.131, 0.046]],
            [0.3127, 0.3290],
        );
        let m709 = rgb_to_xyz(
            [[0.640, 0.330], [0.300, 0.600], [0.150, 0.060]],
            [0.3127, 0.3290],
        );
        let to_709 = mat_mul(&mat_inv(&m709), &m2020);
        let peak_rel = peak / REFERENCE_WHITE_NITS;
        let shaper = match p.curve {
            Curve::Bt2390 => {
                let pq_peak = nits_to_pq(peak);
                let max_lum = (nits_to_pq(SDR_TARGET_NITS) / pq_peak).min(1.0);
                Shaper::Bt2390 {
                    pq_peak,
                    max_lum,
                    knee: 1.5 * max_lum - 0.5,
                }
            }
            Curve::Hable => {
                let exposure = hable_exposure(peak_rel);
                Shaper::Hable {
                    exposure,
                    norm: hable_raw(exposure * peak_rel),
                }
            }
            Curve::Mobius => {
                let t = MOBIUS_TRANSITION;
                Shaper::Mobius {
                    beta: (1.0 - t) * (peak_rel - t) / (peak_rel - 1.0),
                }
            }
        };
        Mapper {
            transfer: p.transfer,
            protect: p.saturation_protection,
            shaper,
            to_709,
            luma_2020: m2020[1],
            luma_709: m709[1],
        }
    }

    /// Decode one encoded channel: PQ gives nits, HLG gives scene light.
    fn decode_axis(&self, v: f64) -> f64 {
        match self.transfer {
            Transfer::Pq => pq_to_nits(v),
            Transfer::Hlg => hlg_inverse_oetf(v),
        }
    }

    /// Decoded channels to display light in nits (applies the HLG OOTF).
    fn display_light(&self, c: [f64; 3]) -> [f64; 3] {
        match self.transfer {
            Transfer::Pq => c,
            Transfer::Hlg => {
                let ys = dot(HLG_LUMA, c);
                let k = HLG_NOMINAL_PEAK_NITS * ys.powf(HLG_SYSTEM_GAMMA - 1.0);
                [k * c[0], k * c[1], k * c[2]]
            }
        }
    }

    /// Tone map a luminance in nits to relative SDR luminance in `[0,1]`.
    fn shape(&self, y: f64) -> f64 {
        let out = match self.shaper {
            Shaper::Bt2390 {
                pq_peak,
                max_lum,
                knee,
            } => {
                let e1 = (nits_to_pq(y) / pq_peak).min(1.0);
                if e1 < knee {
                    y / SDR_TARGET_NITS
                } else {
                    let t = (e1 - knee) / (1.0 - knee);
                    let t2 = t * t;
                    let t3 = t2 * t;
                    let e2 = (2.0 * t3 - 3.0 * t2 + 1.0) * knee
                        + (t3 - 2.0 * t2 + t) * (1.0 - knee)
                        + (-2.0 * t3 + 3.0 * t2) * max_lum;
                    pq_to_nits(e2 * pq_peak) / SDR_TARGET_NITS
                }
            }
            Shaper::Hable { exposure, norm } => {
                hable_raw(exposure * y / REFERENCE_WHITE_NITS) / norm
            }
            Shaper::Mobius { beta } => {
                let x = y / REFERENCE_WHITE_NITS;
                if x <= MOBIUS_TRANSITION {
                    x
                } else {
                    let u = x - MOBIUS_TRANSITION;
                    MOBIUS_TRANSITION + beta * u / (u + beta)
                }
            }
        };
        out.clamp(0.0, 1.0)
    }

    /// Display light in nits to linear BT.709 relative light, before gamut handling.
    fn tone_map_linear(&self, nits: [f64; 3]) -> [f64; 3] {
        let y = dot(self.luma_2020, nits);
        if y <= 0.0 {
            return [0.0; 3];
        }
        let s = self.shape(y) / y;
        mat_vec(&self.to_709, [nits[0] * s, nits[1] * s, nits[2] * s])
    }

    /// Gamut handling and the BT.709 OETF.
    fn finish(&self, mut lin: [f64; 3]) -> [f64; 3] {
        if self.protect {
            let y = dot(self.luma_709, lin).clamp(0.0, 1.0);
            // Distance of each channel from the luminance axis, as a fraction of
            // the room the gamut leaves on that side (1.0 is the gamut edge),
            // soft-clipped so nothing leaves the gamut. Each channel depends
            // only on its own distance, which keeps the mapping smooth.
            let room_up = (1.0 - y).max(1e-9);
            let room_down = y.max(1e-9);
            for c in &mut lin {
                if *c > y {
                    let d = (*c - y) / room_up;
                    if d > GAMUT_KNEE {
                        *c = y + compress_distance(d) * room_up;
                    }
                } else {
                    let d = (y - *c) / room_down;
                    if d > GAMUT_KNEE {
                        *c = y - compress_distance(d) * room_down;
                    }
                }
            }
        }
        [
            bt709_oetf(lin[0].clamp(0.0, 1.0)),
            bt709_oetf(lin[1].clamp(0.0, 1.0)),
            bt709_oetf(lin[2].clamp(0.0, 1.0)),
        ]
    }

    /// Linear BT.709 relative light before any gamut handling (may leave `[0,1]`).
    pub fn map_linear(&self, rgb: [f64; 3]) -> [f64; 3] {
        let d = [
            self.decode_axis(rgb[0]),
            self.decode_axis(rgb[1]),
            self.decode_axis(rgb[2]),
        ];
        self.tone_map_linear(self.display_light(d))
    }

    /// Encoded BT.2020 RGB in, encoded BT.709 RGB out (exactly what the table samples).
    pub fn map(&self, rgb: [f64; 3]) -> [f64; 3] {
        self.finish(self.map_linear(rgb))
    }
}

/// The exact per-pixel function the table samples (no interpolation): encoded
/// BT.2020 RGB in, encoded BT.709 RGB out.
pub fn map_pixel(p: &Params, rgb: [f64; 3]) -> [f64; 3] {
    Mapper::new(p).map(rgb)
}

/// Linear-light BT.709 value (relative to SDR white) before gamut handling.
/// Channels may fall below 0 or above 1; useful to see what protection fixes.
pub fn map_pixel_linear(p: &Params, rgb: [f64; 3]) -> [f64; 3] {
    Mapper::new(p).map_linear(rgb)
}

/// Build the lookup table. Deterministic and single threaded.
pub fn build_lut(p: &Params) -> Lut {
    let n = p.size.clamp(2, MAX_LATTICE);
    let mapper = Mapper::new(p);
    // The lattice is uniform, so each axis only has n distinct decoded values.
    let axis: Vec<f64> = (0..n)
        .map(|i| mapper.decode_axis(i as f64 / (n - 1) as f64))
        .collect();
    let mut data = Vec::with_capacity(n * n * n);
    for &b in &axis {
        for &g in &axis {
            for &r in &axis {
                let light = mapper.display_light([r, g, b]);
                let out = mapper.finish(mapper.tone_map_linear(light));
                data.push([out[0] as f32, out[1] as f32, out[2] as f32]);
            }
        }
    }
    Lut { size: n, data }
}

impl Lut {
    /// The `.cube` file text (red index varies fastest). Quotes and line breaks
    /// in `title` are replaced by spaces.
    pub fn to_cube(&self, title: &str) -> String {
        let title: String = title
            .chars()
            .map(|c| if c == '"' || c.is_control() { ' ' } else { c })
            .collect();
        let mut s = String::with_capacity(self.data.len() * 27 + 128);
        let _ = writeln!(s, "TITLE \"{title}\"");
        let _ = writeln!(s, "LUT_3D_SIZE {}", self.size);
        s.push_str("DOMAIN_MIN 0 0 0\nDOMAIN_MAX 1 1 1\n");
        for v in &self.data {
            let _ = writeln!(s, "{:.6} {:.6} {:.6}", v[0], v[1], v[2]);
        }
        s
    }

    /// Look up one triple with tetrahedral interpolation (the method of
    /// ffmpeg's `interp=tetrahedral`). Inputs are clamped to `[0,1]`.
    pub fn lookup(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size;
        let top = (n - 1) as f32;
        let mut base = [0usize; 3];
        let mut frac = [0.0f32; 3];
        for (i, &v) in rgb.iter().enumerate() {
            let pos = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) } * top;
            let cell = (pos.floor() as usize).min(n - 2);
            base[i] = cell;
            frac[i] = pos - cell as f32;
        }
        let at = |dr: usize, dg: usize, db: usize| -> [f32; 3] {
            self.data[(base[0] + dr) + n * ((base[1] + dg) + n * (base[2] + db))]
        };
        let [fr, fg, fb] = frac;
        // Walk from corner 000 to corner 111 along the axes in order of
        // decreasing fractional part; the three steps are the edges of the
        // tetrahedron containing the point.
        let (s1, s2, s3, w1, w2, w3) = if fr >= fg {
            if fg >= fb {
                ((1, 0, 0), (1, 1, 0), (1, 1, 1), fr, fg, fb)
            } else if fr >= fb {
                ((1, 0, 0), (1, 0, 1), (1, 1, 1), fr, fb, fg)
            } else {
                ((0, 0, 1), (1, 0, 1), (1, 1, 1), fb, fr, fg)
            }
        } else if fb >= fg {
            ((0, 0, 1), (0, 1, 1), (1, 1, 1), fb, fg, fr)
        } else if fb >= fr {
            ((0, 1, 0), (0, 1, 1), (1, 1, 1), fg, fb, fr)
        } else {
            ((0, 1, 0), (1, 1, 0), (1, 1, 1), fg, fr, fb)
        };
        let c0 = at(0, 0, 0);
        let c1 = at(s1.0, s1.1, s1.2);
        let c2 = at(s2.0, s2.1, s2.2);
        let c3 = at(s3.0, s3.1, s3.2);
        let mut out = [0.0f32; 3];
        for (i, o) in out.iter_mut().enumerate() {
            *o = c0[i] + w1 * (c1[i] - c0[i]) + w2 * (c2[i] - c1[i]) + w3 * (c3[i] - c2[i]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const CURVES: [Curve; 3] = [Curve::Bt2390, Curve::Hable, Curve::Mobius];
    const TRANSFERS: [Transfer; 2] = [Transfer::Pq, Transfer::Hlg];

    fn params(transfer: Transfer, curve: Curve) -> Params {
        Params {
            curve,
            ..Params::new(transfer)
        }
    }

    /// Encoded neutral level that carries `nits` of display light.
    fn neutral_for_nits(transfer: Transfer, nits: f64) -> f64 {
        match transfer {
            Transfer::Pq => nits_to_pq(nits),
            // Neutral HLG: nits = 1000 * e^1.2 with e the scene light, so invert.
            Transfer::Hlg => {
                let scene = (nits / HLG_NOMINAL_PEAK_NITS).powf(1.0 / HLG_SYSTEM_GAMMA);
                let a = HLG_A;
                let c = 0.5 - a * (4.0 * a).ln();
                if scene <= 1.0 / 12.0 {
                    (3.0 * scene).sqrt()
                } else {
                    a * (12.0 * scene - HLG_B).ln() + c
                }
            }
        }
    }

    struct Rng(u64);
    impl Rng {
        fn next_f64(&mut self) -> f64 {
            // splitmix64
            self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^= z >> 31;
            (z >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    #[test]
    fn pq_round_trip_and_anchors() {
        assert_eq!(nits_to_pq(0.0), 0.0);
        assert_eq!(pq_to_nits(0.0), 0.0);
        assert!((nits_to_pq(10000.0) - 1.0).abs() < 1e-12);
        assert!((pq_to_nits(1.0) - 10000.0).abs() / 10000.0 < 1e-9);
        let e203 = nits_to_pq(203.0);
        assert!((e203 - 0.5807).abs() < 5e-4, "pq(203) = {e203}");
        let mut n = 0.01;
        while n <= 10000.0 {
            let back = pq_to_nits(nits_to_pq(n));
            assert!(((back - n) / n).abs() < 1e-6, "{n} -> {back}");
            n *= 1.37;
        }
    }

    #[test]
    fn conversion_matrix_rows_sum_to_one() {
        let m = Mapper::new(&Params::new(Transfer::Pq));
        let expect = [
            [1.660491, -0.587641, -0.072850],
            [-0.124550, 1.132900, -0.008349],
            [-0.018151, -0.100579, 1.118730],
        ];
        for (row, exp) in m.to_709.iter().zip(expect.iter()) {
            assert!((row.iter().sum::<f64>() - 1.0).abs() < 1e-12);
            for (a, b) in row.iter().zip(exp.iter()) {
                assert!((a - b).abs() < 2e-5, "{a} vs {b}");
            }
        }
        assert!((m.luma_2020[0] - 0.2627).abs() < 1e-4);
        assert!((m.luma_2020[1] - 0.6780).abs() < 1e-4);
        assert!((m.luma_2020[2] - 0.0593).abs() < 1e-4);
    }

    #[test]
    fn neutral_axis_behaviour() {
        for transfer in TRANSFERS {
            for curve in CURVES {
                let p = params(transfer, curve);
                let m = Mapper::new(&p);
                let mut prev = -1.0;
                for i in 0..=4096 {
                    let v = i as f64 / 4096.0;
                    let o = m.map([v, v, v]);
                    assert!((o[0] - o[1]).abs() < 1e-6 && (o[1] - o[2]).abs() < 1e-6);
                    assert!(
                        o[0] >= prev - 1e-12,
                        "{transfer:?} {curve:?} not monotonic at {v}"
                    );
                    assert!((0.0..=1.0).contains(&o[0]));
                    prev = o[0];
                }
                let black = m.map([0.0; 3]);
                assert!(black.iter().all(|c| c.abs() < 1e-9), "black {black:?}");
                let peak = neutral_for_nits(transfer, p.source_peak_nits);
                let top = m.map([peak; 3]);
                assert!(
                    top.iter().all(|c| (c - 1.0).abs() < 1e-6),
                    "{transfer:?} {curve:?} peak -> {top:?}"
                );
                let white = neutral_for_nits(transfer, REFERENCE_WHITE_NITS);
                let g = m.map([white; 3])[0];
                println!("{transfer:?} {curve:?}: 203 nit gray -> {g:.4}");
                assert!(
                    (0.88..=0.97).contains(&g),
                    "{transfer:?} {curve:?}: 203 nit gray -> {g}"
                );
            }
        }
    }

    #[test]
    fn strictly_increasing_below_peak() {
        for transfer in TRANSFERS {
            for curve in CURVES {
                let p = params(transfer, curve);
                let m = Mapper::new(&p);
                let peak = neutral_for_nits(transfer, p.source_peak_nits);
                let mut prev = m.map([0.0; 3])[0];
                for i in 1..=500 {
                    let v = peak * 0.98 * i as f64 / 500.0 + 0.001;
                    let o = m.map([v, v, v])[0];
                    assert!(o > prev, "{transfer:?} {curve:?} flat at {v}");
                    prev = o;
                }
            }
        }
    }

    #[test]
    fn other_peaks_hit_their_peak() {
        for curve in CURVES {
            for peak in [400.0, 600.0, 1500.0, 4000.0, 10000.0] {
                let p = Params {
                    source_peak_nits: peak,
                    ..params(Transfer::Pq, curve)
                };
                let top = map_pixel(&p, [nits_to_pq(peak); 3]);
                assert!(
                    top.iter().all(|c| (c - 1.0).abs() < 1e-6),
                    "{curve:?} {peak}"
                );
                let g = map_pixel(&p, [nits_to_pq(203.0); 3])[0];
                println!("{curve:?} peak {peak}: 203 nit gray -> {g:.4}");
            }
        }
        // For peaks up to about 1200 nits diffuse white stays in the SDR white band.
        for curve in CURVES {
            for peak in [400.0, 600.0, 1000.0, 1200.0] {
                let p = Params {
                    source_peak_nits: peak,
                    ..params(Transfer::Pq, curve)
                };
                let g = map_pixel(&p, [nits_to_pq(203.0); 3])[0];
                assert!((0.88..=0.97).contains(&g), "{curve:?} {peak}: {g}");
            }
        }
    }

    #[test]
    fn saturation_protection_works() {
        let on = Params::new(Transfer::Pq);
        let off = Params {
            saturation_protection: false,
            ..on
        };
        let level = nits_to_pq(100.0);
        let bright = nits_to_pq(300.0);
        let samples = [
            [0.0, level, 0.0],
            [level, 0.0, 0.0],
            [bright, 0.0, 0.0],
            [0.0, bright, 0.0],
            [0.0, 0.0, bright],
        ];
        for rgb in samples {
            let lin = map_pixel_linear(&on, rgb);
            assert!(
                lin.iter().any(|&c| !(0.0..=1.0).contains(&c)),
                "{rgb:?} stays in gamut before clipping: {lin:?}"
            );
            let protected = map_pixel(&on, rgb);
            let clipped = map_pixel(&off, rgb);
            assert!(protected.iter().all(|c| (0.0..=1.0).contains(c)));
            assert!(clipped.iter().all(|c| (0.0..=1.0).contains(c)));
            let diff: f64 = protected
                .iter()
                .zip(clipped.iter())
                .map(|(a, b)| (a - b).abs())
                .sum();
            assert!(diff > 1e-3, "protection changed nothing for {rgb:?}");
            // Protection pulls the clipped channel in from the edge by moving the
            // others toward luminance, so it never has a fully clipped channel
            // unless the blend ended exactly on the boundary.
            let clipped_channels = clipped.iter().filter(|&&c| c == 0.0 || c == 1.0).count();
            assert!(clipped_channels >= 1);
        }
    }

    #[test]
    fn hue_ordering_is_preserved() {
        for transfer in TRANSFERS {
            for curve in CURVES {
                for protect in [true, false] {
                    let p = Params {
                        saturation_protection: protect,
                        ..params(transfer, curve)
                    };
                    let rgb = match transfer {
                        Transfer::Pq => [nits_to_pq(150.0), nits_to_pq(70.0), nits_to_pq(20.0)],
                        Transfer::Hlg => [0.7, 0.5, 0.3],
                    };
                    let o = map_pixel(&p, rgb);
                    assert!(o[0] > o[1] && o[1] > o[2], "{transfer:?} {curve:?} {o:?}");
                }
            }
        }
    }

    #[test]
    fn lut_layout_and_cube_text() {
        let p = Params {
            size: 17,
            ..Params::new(Transfer::Pq)
        };
        let lut = build_lut(&p);
        assert_eq!(lut.size, 17);
        assert_eq!(lut.data.len(), 17 * 17 * 17);
        let n = 17;
        let m = Mapper::new(&p);
        let at = |r: usize, g: usize, b: usize| lut.data[r + n * (g + n * b)];
        let exact = |r: usize, g: usize, b: usize| {
            let o = m.map([r as f64 / 16.0, g as f64 / 16.0, b as f64 / 16.0]);
            [o[0] as f32, o[1] as f32, o[2] as f32]
        };
        // Red varies fastest: index 1 is the lattice point (1,0,0), index n is (0,1,0).
        assert_eq!(lut.data[1], exact(1, 0, 0));
        assert_eq!(lut.data[n], exact(0, 1, 0));
        assert_eq!(lut.data[n * n], exact(0, 0, 1));
        assert_eq!(at(9, 4, 12), exact(9, 4, 12));
        assert_eq!(at(16, 16, 16), exact(16, 16, 16));
        assert_eq!(lut.data[0], [0.0; 3]);

        let cube = lut.to_cube("Panoply \"test\"\n");
        let mut lines = cube.lines();
        assert_eq!(lines.next().unwrap(), "TITLE \"Panoply  test  \"");
        assert_eq!(lines.next().unwrap(), "LUT_3D_SIZE 17");
        assert_eq!(lines.next().unwrap(), "DOMAIN_MIN 0 0 0");
        assert_eq!(lines.next().unwrap(), "DOMAIN_MAX 1 1 1");
        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), n * n * n);
        for idx in [0, 1, n, n * n, 1234, n * n * n - 1] {
            let vals: Vec<f32> = rows[idx]
                .split(' ')
                .map(|t| {
                    assert_eq!(t.split('.').nth(1).map(str::len), Some(6), "{t}");
                    t.parse().unwrap()
                })
                .collect();
            assert_eq!(vals.len(), 3);
            for (a, b) in vals.iter().zip(lut.data[idx].iter()) {
                assert!((a - b).abs() < 1e-6);
            }
        }
    }

    #[test]
    fn lookup_is_exact_on_lattice_and_monotone_on_neutrals() {
        let lut = build_lut(&Params {
            size: 33,
            ..Params::new(Transfer::Hlg)
        });
        let n = lut.size;
        for (r, g, b) in [(0, 0, 0), (32, 32, 32), (5, 20, 9), (31, 0, 17)] {
            let want = lut.data[r + n * (g + n * b)];
            let got = lut.lookup([r as f32 / 32.0, g as f32 / 32.0, b as f32 / 32.0]);
            for (a, c) in got.iter().zip(want.iter()) {
                assert!((a - c).abs() < 1e-6);
            }
        }
        let mut prev = -1.0f32;
        for i in 0..=1000 {
            let v = i as f32 / 1000.0;
            let o = lut.lookup([v, v, v])[0];
            assert!(o >= prev - 1e-6);
            prev = o;
        }
    }

    /// Interpolation error statistics in 8 bit output code values.
    struct ErrStats {
        p99: f64,
        max: f64,
    }

    /// Error of the default table against `map_pixel` over 2000 deterministic
    /// random triples with every channel uniform in `[0, scale]` (PQ signal).
    fn interpolation_error(transfer: Transfer, scale: f64) -> ErrStats {
        let p = Params::new(transfer);
        let lut = build_lut(&p);
        let m = Mapper::new(&p);
        let mut rng = Rng(0x5EED_0000 + transfer as u64);
        let mut errs: Vec<f64> = (0..2000)
            .map(|_| {
                let rgb = [
                    rng.next_f64() * scale,
                    rng.next_f64() * scale,
                    rng.next_f64() * scale,
                ];
                let exact = m.map(rgb);
                let got = lut.lookup([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]);
                (0..3)
                    .map(|i| (got[i] as f64 - exact[i]).abs() * 255.0)
                    .fold(0.0, f64::max)
            })
            .collect();
        errs.sort_by(|a, b| a.partial_cmp(b).unwrap());
        ErrStats {
            p99: errs[(errs.len() as f64 * 0.99).ceil() as usize - 1],
            max: errs[errs.len() - 1],
        }
    }

    #[test]
    fn interpolation_error_pq() {
        // Content cannot exceed its own peak (MaxCLL), so the main statistic
        // samples channels up to the PQ level of the 1000 nit source peak.
        let peak_code = nits_to_pq(1000.0);
        let valid = interpolation_error(Transfer::Pq, peak_code);
        let dark = interpolation_error(Transfer::Pq, 0.25);
        let full = interpolation_error(Transfer::Pq, 1.0);
        println!(
            "PQ 65: channels in [0, peak]: p99 {:.3} max {:.3} code values; \
             dark box [0, 0.25]: p99 {:.3} max {:.3}; full cube incl. above peak: p99 {:.3} max {:.3}",
            valid.p99, valid.max, dark.p99, dark.max, full.p99, full.max
        );
        assert!(valid.p99 < 1.5, "p99 {}", valid.p99);
        assert!(dark.p99 < 1.5, "dark p99 {}", dark.p99);
        assert!(full.p99 < 3.0, "full cube p99 {}", full.p99);
    }

    #[test]
    fn interpolation_error_hlg() {
        let valid = interpolation_error(Transfer::Hlg, 1.0);
        let dark = interpolation_error(Transfer::Hlg, 0.25);
        println!(
            "HLG {}: full cube: p99 {:.3} max {:.3} code values; dark box [0, 0.25]: p99 {:.3} max {:.3}",
            Params::new(Transfer::Hlg).size,
            valid.p99,
            valid.max,
            dark.p99,
            dark.max
        );
        assert!(valid.p99 < 1.0, "p99 {}", valid.p99);
        assert!(dark.p99 < 1.0, "dark p99 {}", dark.p99);
    }

    #[test]
    fn build_time_for_65() {
        let p = Params::new(Transfer::Pq);
        assert_eq!(p.size, 65);
        let start = Instant::now();
        let lut = build_lut(&p);
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        println!("build_lut 65^3 PQ Bt2390: {ms:.1} ms");
        assert_eq!(lut.data.len(), 65 * 65 * 65);
        assert!(ms < 1500.0, "build took {ms} ms");
        for curve in [Curve::Hable, Curve::Mobius] {
            let start = Instant::now();
            let _ = build_lut(&Params { curve, ..p });
            println!(
                "build_lut 65^3 PQ {curve:?}: {:.1} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        let start = Instant::now();
        let _ = build_lut(&Params {
            size: 65,
            ..Params::new(Transfer::Hlg)
        });
        println!(
            "build_lut 65^3 HLG Bt2390: {:.1} ms",
            start.elapsed().as_secs_f64() * 1000.0
        );
    }

    #[test]
    fn deterministic() {
        let p = Params {
            size: 9,
            ..Params::new(Transfer::Hlg)
        };
        assert_eq!(build_lut(&p), build_lut(&p));
    }

    #[test]
    fn hlg_reference_white_matches_pq_white() {
        for curve in CURVES {
            let pq = map_pixel(&params(Transfer::Pq, curve), [nits_to_pq(203.0); 3])[0];
            let hlg = map_pixel(&params(Transfer::Hlg, curve), [0.75; 3])[0];
            println!("{curve:?}: PQ 203 nits -> {pq:.4}, HLG 75% -> {hlg:.4}");
            assert!((pq - hlg).abs() < 0.06, "{curve:?}: {pq} vs {hlg}");
            assert!((0.88..=0.97).contains(&hlg));
        }
    }
}

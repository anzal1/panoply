//! EBU R128 / ITU-R BS.1770-4 loudness metering.
//!
//! This module measures, in one streaming pass and with constant memory:
//!
//! * integrated loudness (BS.1770-4 with the -70 LUFS absolute gate and the
//!   -10 LU relative gate),
//! * loudness range (EBU Tech 3342: 3 s short-term blocks, -70 LUFS absolute
//!   gate, -20 LU relative gate, 10th to 95th percentile),
//! * maximum momentary (400 ms) and short-term (3 s) loudness (EBU Tech 3341),
//! * true peak (BS.1770-4 Annex 2) and sample peak, and a clipped sample count.
//!
//! The code is original and written from the published standards. Usage:
//!
//! ```ignore
//! let roles = roles_for("stereo", 2);
//! let mut meter = Meter::new(48_000, &roles, false);
//! meter.push(&interleaved_samples);
//! let result = meter.finish();
//! println!("{:.1} LUFS", result.integrated_lufs);
//! ```
//!
//! # Method
//!
//! Each measured channel is K-weighted by two cascaded biquads (a high shelf
//! and the RLB high-pass). The standard publishes coefficients for 48 kHz
//! only, so they are derived for any sample rate from the analog prototype
//! parameters with the bilinear transform; at 48 kHz they reproduce the
//! published values. The weighted mean square of every channel is multiplied
//! by its channel gain and summed.
//!
//! Audio is cut into steps of 100 ms. The sum of the last four steps is a
//! 400 ms gating block (75% overlap) and the sum of the last thirty steps is
//! a 3 s short-term block. Because both are plain sums of step energies, a
//! single accumulator per step is enough.
//!
//! # Deviations from the standards
//!
//! * The 100 ms step is rounded to a whole number of samples. For sample rates
//!   where that is not exact (for example 11025 Hz, 1102.5 samples, rounded to
//!   1103) blocks are a fraction of a percent longer than nominal.
//! * A trailing partial step (under 100 ms) and a trailing partial frame are
//!   not measured. [`Loudness::seconds`] still counts every complete frame.
//! * Block loudness values are kept in histograms with 0.01 LU resolution
//!   between -70 and +10 LUFS (louder blocks share the top bin, with their
//!   exact energy). Relative gate thresholds and the loudness range
//!   percentiles are therefore quantised to 0.01 LU.
//! * The true peak interpolator is an original 12 taps per phase Kaiser
//!   windowed sinc, not the coefficient table printed in Annex 2. It has
//!   unity gain at every phase. Interpolated points are only evaluated where
//!   the filter window lies fully inside the signal, so nothing is invented
//!   at the very start and end (about 6 samples at each end are covered by
//!   the sample peak only).
//! * Channels that BS.1770-4 gives no gain to (LFE, height channels) are not
//!   measured for loudness; all channels count towards peaks.
//! * Sample rates outside 8 kHz to 384 kHz are clamped into that range.

/// Absolute gate in LUFS.
const ABS_GATE_LUFS: f64 = -70.0;
/// Offset in the BS.1770 loudness formula.
const LUFS_OFFSET: f64 = -0.691;
/// Histogram resolution in LU.
const BIN_WIDTH: f64 = 0.01;
/// Histogram size: -70 to +10 LUFS at 0.01 LU.
const BIN_COUNT: usize = 8000;
/// Steps per 400 ms momentary block.
const BLOCK_STEPS: usize = 4;
/// Steps per 3 s short-term block.
const SHORT_STEPS: usize = 30;
/// Interpolator taps per output phase.
const TP_TAPS: usize = 12;
/// Maximum oversampling factor.
const TP_MAX_PHASES: usize = 4;
/// Samples at or above this magnitude count as clipped.
const CLIP_LEVEL: f32 = 0.9999;
/// Gain applied to the mean square of the surround channels (+1.5 dB).
const SURROUND_GAIN: f64 = 1.41;

/// Which speaker a channel feeds. Gains: Left, Right, Center 1.0; SurroundLeft, SurroundRight 1.41 (+1.5 dB); Lfe and Unused are not measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Front left, gain 1.0.
    Left,
    /// Front right, gain 1.0.
    Right,
    /// Front center (also a mono channel), gain 1.0.
    Center,
    /// Left surround, gain 1.41.
    SurroundLeft,
    /// Right surround, gain 1.41.
    SurroundRight,
    /// Low frequency effects, not measured for loudness.
    Lfe,
    /// Any other channel, not measured for loudness.
    Unused,
}

impl Role {
    /// Mean square weight of this role (0.0 when not measured).
    fn gain(self) -> f64 {
        match self {
            Role::Left | Role::Right | Role::Center => 1.0,
            Role::SurroundLeft | Role::SurroundRight => SURROUND_GAIN,
            Role::Lfe | Role::Unused => 0.0,
        }
    }
}

/// Channel roles for an ffmpeg channel layout name ("mono", "stereo", "2.1", "3.0", "quad", "5.0", "5.1", "5.1(side)", "6.1", "7.1", ...) or, when the name is unknown, a sensible guess from the channel count (1 mono->Center, 2 stereo, 6 as 5.1 in FL FR FC LFE BL BR order, 8 as 7.1 FL FR FC LFE BL BR SL SR with BL BR SL SR all surround).
///
/// Back center channels are treated as surround, wide and front-of-center
/// channels as plain front channels, height channels as [`Role::Unused`].
/// The returned vector is in the channel order ffmpeg uses for that layout.
/// When a known layout does not have `channels` channels the guess from the
/// channel count is used instead.
pub fn roles_for(layout: &str, channels: usize) -> Vec<Role> {
    use Role::{
        Center as C, Left as L, Lfe as E, Right as R, SurroundLeft as SL, SurroundRight as SR,
        Unused as U,
    };
    let name = layout.trim().to_ascii_lowercase();
    let known: Option<Vec<Role>> = match name.as_str() {
        "mono" => Some(vec![C]),
        "stereo" | "downmix" => Some(vec![L, R]),
        "2.1" => Some(vec![L, R, E]),
        "3.0" => Some(vec![L, R, C]),
        "3.0(back)" => Some(vec![L, R, SL]),
        "4.0" => Some(vec![L, R, C, SL]),
        "quad" | "quad(side)" | "2.2" => Some(vec![L, R, SL, SR]),
        "3.1" => Some(vec![L, R, C, E]),
        "5.0" | "5.0(side)" => Some(vec![L, R, C, SL, SR]),
        "4.1" => Some(vec![L, R, C, E, SL]),
        "5.1" | "5.1(side)" => Some(vec![L, R, C, E, SL, SR]),
        "6.0" => Some(vec![L, R, C, SL, SL, SR]),
        "6.0(front)" => Some(vec![L, R, L, R, SL, SR]),
        "hexagonal" => Some(vec![L, R, C, SL, SR, SL]),
        "6.1" => Some(vec![L, R, C, E, SL, SL, SR]),
        "6.1(back)" => Some(vec![L, R, C, E, SL, SR, SL]),
        "6.1(front)" => Some(vec![L, R, E, L, R, SL, SR]),
        "7.0" => Some(vec![L, R, C, SL, SR, SL, SR]),
        "7.0(front)" => Some(vec![L, R, C, L, R, SL, SR]),
        "7.1" => Some(vec![L, R, C, E, SL, SR, SL, SR]),
        "7.1(wide)" => Some(vec![L, R, C, E, SL, SR, L, R]),
        "7.1(wide-side)" => Some(vec![L, R, C, E, L, R, SL, SR]),
        "octagonal" => Some(vec![L, R, C, SL, SR, SL, SL, SR]),
        "cube" => Some(vec![L, R, SL, SR, U, U, U, U]),
        "5.1.2" => Some(vec![L, R, C, E, SL, SR, U, U]),
        "5.1.4" => Some(vec![L, R, C, E, SL, SR, U, U, U, U]),
        "7.1.2" => Some(vec![L, R, C, E, SL, SR, SL, SR, U, U]),
        "7.1.4" => Some(vec![L, R, C, E, SL, SR, SL, SR, U, U, U, U]),
        _ => None,
    };
    if let Some(roles) = known.filter(|r| r.len() == channels) {
        return roles;
    }
    guess_roles(channels)
}

/// Guess roles from a bare channel count.
fn guess_roles(channels: usize) -> Vec<Role> {
    use Role::{
        Center as C, Left as L, Lfe as E, Right as R, SurroundLeft as SL, SurroundRight as SR,
        Unused as U,
    };
    let base: &[Role] = match channels {
        0 => &[],
        1 => &[C],
        2 => &[L, R],
        3 => &[L, R, C],
        4 => &[L, R, SL, SR],
        5 => &[L, R, C, SL, SR],
        6 => &[L, R, C, E, SL, SR],
        7 => &[L, R, C, E, SL, SL, SR],
        _ => &[L, R, C, E, SL, SR, SL, SR],
    };
    let mut roles = base.to_vec();
    roles.resize(channels, U);
    roles
}

/// Second order section, direct form coefficients with `a0` normalised to 1.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
}

/// First K-weighting stage: the high shelf "pre-filter" (about +4 dB above
/// 1.7 kHz), derived for sample rate `fs`.
fn shelf_coefficients(fs: f64) -> Biquad {
    let f0 = 1681.974450955533;
    let gain_db = 3.999843853973347;
    let q = 0.7071752369554196;
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let vh = 10f64.powf(gain_db / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    Biquad {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// Second K-weighting stage: the RLB high-pass at about 38 Hz. The
/// numerator is the unnormalised 1, -2, 1 of the standard.
fn rlb_coefficients(fs: f64) -> Biquad {
    let f0 = 38.13547087602444;
    let q = 0.5003270373238773;
    let k = (std::f64::consts::PI * f0 / fs).tan();
    let a0 = 1.0 + k / q + k * k;
    Biquad {
        b0: 1.0,
        b1: -2.0,
        b2: 1.0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// Zeroth order modified Bessel function of the first kind.
fn bessel_i0(x: f64) -> f64 {
    let q = x * x / 4.0;
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..80 {
        term *= q / (k * k) as f64;
        sum += term;
        if term < 1e-18 * sum {
            break;
        }
    }
    sum
}

/// Polyphase interpolator table for `phases` (2 or 4): `table[k][p]` weights
/// the sample `k` frames old for output phase `p`. Lanes beyond `phases` are 0.
fn design_interpolator(phases: usize) -> [[f32; TP_MAX_PHASES]; TP_TAPS] {
    const BETA: f64 = 7.0;
    let total = TP_TAPS * phases;
    let center = (total / 2) as f64;
    let half = total as f64 / 2.0;
    let norm = bessel_i0(BETA);
    let mut phase_taps = [[0f64; TP_TAPS]; TP_MAX_PHASES];
    for (p, taps) in phase_taps.iter_mut().enumerate().take(phases) {
        for (k, tap) in taps.iter_mut().enumerate() {
            let j = (p + k * phases) as f64;
            let t = (j - center) / phases as f64;
            let sinc = if t == 0.0 {
                1.0
            } else {
                (std::f64::consts::PI * t).sin() / (std::f64::consts::PI * t)
            };
            let r = (j - center) / half;
            let window = bessel_i0(BETA * (1.0 - r * r).max(0.0).sqrt()) / norm;
            *tap = sinc * window;
        }
        let sum: f64 = taps.iter().sum();
        for tap in taps.iter_mut() {
            *tap /= sum;
        }
    }
    let mut table = [[0f32; TP_MAX_PHASES]; TP_TAPS];
    for (k, row) in table.iter_mut().enumerate() {
        for (lane, taps) in row.iter_mut().zip(phase_taps.iter()).take(phases) {
            *lane = taps[k] as f32;
        }
    }
    table
}

/// Per-channel running state.
#[derive(Debug, Clone, Copy)]
struct Channel {
    /// Mean square weight (0 when the channel is not measured).
    weight: f64,
    /// Transposed direct form II state: two per stage.
    z: [f64; 4],
    /// Last samples for the true peak interpolator, newest first.
    hist: [f32; TP_TAPS],
}

/// Fixed-resolution histogram of block energies.
#[derive(Debug, Clone)]
struct Histogram {
    counts: Vec<u64>,
    energy: Vec<f64>,
}

fn energy_to_lufs(energy: f64) -> f64 {
    if energy > 0.0 {
        LUFS_OFFSET + 10.0 * energy.log10()
    } else {
        f64::NEG_INFINITY
    }
}

fn amplitude_to_db(amplitude: f64) -> f64 {
    if amplitude > 0.0 {
        20.0 * amplitude.log10()
    } else {
        f64::NEG_INFINITY
    }
}

impl Histogram {
    fn new() -> Self {
        Histogram {
            counts: vec![0; BIN_COUNT],
            energy: vec![0.0; BIN_COUNT],
        }
    }

    /// Record a block, dropping it when it is under the absolute gate.
    fn add(&mut self, energy: f64) {
        let lufs = energy_to_lufs(energy);
        if lufs < ABS_GATE_LUFS || !lufs.is_finite() {
            return;
        }
        let idx = (((lufs - ABS_GATE_LUFS) / BIN_WIDTH) as usize).min(BIN_COUNT - 1);
        self.counts[idx] += 1;
        self.energy[idx] += energy;
    }

    /// Mean energy of all blocks in bins `start..`, `None` when there are none.
    fn mean_from(&self, start: usize) -> Option<f64> {
        let start = start.min(BIN_COUNT);
        let count: u64 = self.counts[start..].iter().sum();
        if count == 0 {
            return None;
        }
        let sum: f64 = self.energy[start..].iter().sum();
        Some(sum / count as f64)
    }

    /// First bin that passes a relative gate `offset_lu` (negative) below the
    /// loudness of the mean of everything over the absolute gate.
    fn relative_start(&self, offset_lu: f64) -> Option<usize> {
        let threshold = energy_to_lufs(self.mean_from(0)?) + offset_lu;
        let pos = (threshold - ABS_GATE_LUFS) / BIN_WIDTH;
        Some(if pos <= 0.0 { 0 } else { pos.ceil() as usize })
    }

    /// Loudness at the given fraction (0..=1) of the blocks in bins `start..`.
    fn percentile(&self, start: usize, fraction: f64) -> Option<f64> {
        let start = start.min(BIN_COUNT);
        let total: u64 = self.counts[start..].iter().sum();
        if total == 0 {
            return None;
        }
        let rank = (fraction * (total - 1) as f64 + 0.5).floor() as u64;
        let mut seen = 0u64;
        for (i, &c) in self.counts.iter().enumerate().skip(start) {
            seen += c;
            if seen > rank {
                return Some(ABS_GATE_LUFS + (i as f64 + 0.5) * BIN_WIDTH);
            }
        }
        None
    }
}

/// Streaming loudness meter. Create with [`Meter::new`], feed audio with
/// [`Meter::push`] and read the results with [`Meter::finish`].
#[derive(Debug, Clone)]
pub struct Meter {
    shelf: Biquad,
    rlb: Biquad,
    channels: Vec<Channel>,
    /// Samples per 100 ms step.
    step_len: usize,
    step_pos: usize,
    step_acc: f64,
    /// Energy sums of the last thirty steps.
    ring: [f64; SHORT_STEPS],
    steps_done: u64,
    gated: Histogram,
    short_term: Histogram,
    max_momentary: f64,
    max_short: f64,
    peak: f32,
    true_peak: f32,
    clipped: u64,
    frames: u64,
    sample_rate: u32,
    tp_phases: usize,
    tp_table: [[f32; TP_MAX_PHASES]; TP_TAPS],
    /// Samples of a frame split across `push` calls.
    pending: Vec<f32>,
}

impl Meter {
    /// sample_rate in Hz (8000..=384000). `roles.len()` is the channel count. When `mono_as_dual` is true a single measured channel is counted twice (+3.01 LU, "dual mono"); default behaviour used by Panoply is false.
    pub fn new(sample_rate: u32, roles: &[Role], mono_as_dual: bool) -> Meter {
        let sample_rate = sample_rate.clamp(8_000, 384_000);
        let fs = f64::from(sample_rate);
        let measured = roles.iter().filter(|r| r.gain() > 0.0).count();
        let dual = if mono_as_dual && measured == 1 {
            2.0
        } else {
            1.0
        };
        let channels = roles
            .iter()
            .map(|r| Channel {
                weight: r.gain() * dual,
                z: [0.0; 4],
                hist: [0.0; TP_TAPS],
            })
            .collect::<Vec<_>>();
        let tp_phases = if sample_rate < 96_000 {
            4
        } else if sample_rate < 192_000 {
            2
        } else {
            1
        };
        let tp_table = if tp_phases > 1 {
            design_interpolator(tp_phases)
        } else {
            [[0.0; TP_MAX_PHASES]; TP_TAPS]
        };
        let step_len = ((fs * 0.1).round() as usize).max(1);
        let pending = Vec::with_capacity(channels.len());
        Meter {
            shelf: shelf_coefficients(fs),
            rlb: rlb_coefficients(fs),
            channels,
            step_len,
            step_pos: 0,
            step_acc: 0.0,
            ring: [0.0; SHORT_STEPS],
            steps_done: 0,
            gated: Histogram::new(),
            short_term: Histogram::new(),
            max_momentary: 0.0,
            max_short: 0.0,
            peak: 0.0,
            true_peak: 0.0,
            clipped: 0,
            frames: 0,
            sample_rate,
            tp_phases,
            tp_table,
            pending,
        }
    }

    /// Feed interleaved f32 samples (full scale = 1.0), any chunk length. A frame split across calls is
    /// completed by the next call; a trailing partial frame at [`Meter::finish`] is ignored.
    pub fn push(&mut self, interleaved: &[f32]) {
        let n = self.channels.len();
        if n == 0 {
            return;
        }
        let mut rest = interleaved;
        if !self.pending.is_empty() {
            let take = (n - self.pending.len()).min(rest.len());
            self.pending.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
            if self.pending.len() < n {
                return;
            }
            let frame = std::mem::take(&mut self.pending);
            self.process_frame(&frame);
            self.pending = frame;
            self.pending.clear();
        }
        let mut chunks = rest.chunks_exact(n);
        for frame in &mut chunks {
            self.process_frame(frame);
        }
        self.pending.extend_from_slice(chunks.remainder());
    }

    /// Process one complete frame (one sample per channel).
    fn process_frame(&mut self, frame: &[f32]) {
        let (sh, rl) = (self.shelf, self.rlb);
        let oversample = self.tp_phases > 1;
        let mut energy = 0.0f64;
        let mut peak = 0.0f32;
        let mut tp = 0.0f32;
        let mut clips = 0u64;
        // Interpolate only once the history holds real samples, so the start of
        // the signal is not treated as a step from silence.
        let primed = self.frames + 1 >= TP_TAPS as u64;
        for (ch, &raw) in self.channels.iter_mut().zip(frame) {
            let x = if raw.is_finite() { raw } else { 0.0 };
            let ax = x.abs();
            peak = peak.max(ax);
            clips += u64::from(ax >= CLIP_LEVEL);
            if ch.weight > 0.0 {
                let xd = f64::from(x);
                let y1 = sh.b0 * xd + ch.z[0];
                ch.z[0] = sh.b1 * xd - sh.a1 * y1 + ch.z[1];
                ch.z[1] = sh.b2 * xd - sh.a2 * y1;
                let y2 = rl.b0 * y1 + ch.z[2];
                ch.z[2] = rl.b1 * y1 - rl.a1 * y2 + ch.z[3];
                ch.z[3] = rl.b2 * y1 - rl.a2 * y2;
                energy += ch.weight * y2 * y2;
            }
            if oversample {
                ch.hist.copy_within(0..TP_TAPS - 1, 1);
                ch.hist[0] = x;
            }
            if oversample && primed {
                let mut out = [0f32; TP_MAX_PHASES];
                for (taps, &h) in self.tp_table.iter().zip(ch.hist.iter()) {
                    for (o, &c) in out.iter_mut().zip(taps.iter()) {
                        *o += c * h;
                    }
                }
                for o in out {
                    tp = tp.max(o.abs());
                }
            }
        }
        self.peak = self.peak.max(peak);
        self.true_peak = self.true_peak.max(tp);
        self.clipped += clips;
        self.frames += 1;
        self.step_acc += energy;
        self.step_pos += 1;
        if self.step_pos == self.step_len {
            self.complete_step();
        }
    }

    /// Energy per sample of the last `steps` steps.
    fn window_energy(&self, steps: usize) -> f64 {
        let mut sum = 0.0;
        for i in 0..steps {
            let slot = ((self.steps_done - 1 - i as u64) % SHORT_STEPS as u64) as usize;
            sum += self.ring[slot];
        }
        sum / (steps * self.step_len) as f64
    }

    /// A 100 ms step is complete: emit the momentary and short-term blocks.
    fn complete_step(&mut self) {
        let slot = (self.steps_done % SHORT_STEPS as u64) as usize;
        self.ring[slot] = self.step_acc;
        self.steps_done += 1;
        self.step_acc = 0.0;
        self.step_pos = 0;
        if self.steps_done >= BLOCK_STEPS as u64 {
            let e = self.window_energy(BLOCK_STEPS);
            self.gated.add(e);
            self.max_momentary = self.max_momentary.max(e);
        }
        if self.steps_done >= SHORT_STEPS as u64 {
            let e = self.window_energy(SHORT_STEPS);
            self.short_term.add(e);
            self.max_short = self.max_short.max(e);
        }
    }

    /// Finish and return the results.
    pub fn finish(self) -> Loudness {
        let integrated_lufs = self
            .gated
            .relative_start(-10.0)
            .and_then(|start| self.gated.mean_from(start))
            .map_or(f64::NEG_INFINITY, energy_to_lufs);
        let range_lu = self
            .short_term
            .relative_start(-20.0)
            .and_then(|start| {
                let low = self.short_term.percentile(start, 0.10)?;
                let high = self.short_term.percentile(start, 0.95)?;
                Some((high - low).max(0.0))
            })
            .unwrap_or(0.0);
        let short_term_max_lufs = if self.steps_done >= SHORT_STEPS as u64 {
            energy_to_lufs(self.max_short)
        } else {
            f64::NEG_INFINITY
        };
        let momentary_max_lufs = if self.steps_done >= BLOCK_STEPS as u64 {
            energy_to_lufs(self.max_momentary)
        } else {
            f64::NEG_INFINITY
        };
        let sample_peak = f64::from(self.peak);
        let true_peak = f64::from(self.true_peak.max(self.peak));
        Loudness {
            integrated_lufs,
            range_lu,
            short_term_max_lufs,
            momentary_max_lufs,
            true_peak_dbtp: amplitude_to_db(true_peak),
            sample_peak_dbfs: amplitude_to_db(sample_peak),
            clipped_samples: self.clipped,
            seconds: self.frames as f64 / f64::from(self.sample_rate),
        }
    }
}

/// Results of a loudness measurement.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Loudness {
    /// Integrated loudness in LUFS with the absolute (-70 LUFS) and relative (-10 LU) gates, BS.1770-4. f64::NEG_INFINITY when nothing passes the gate.
    pub integrated_lufs: f64,
    /// Loudness range in LU per EBU Tech 3342 (3 s short-term blocks, absolute gate -70, relative gate -20 LU, 10th to 95th percentile). 0.0 when not enough material.
    pub range_lu: f64,
    /// Highest 3 s short-term loudness (LUFS), NEG_INFINITY if shorter than 3 s of material.
    pub short_term_max_lufs: f64,
    /// Highest 400 ms momentary loudness (LUFS).
    pub momentary_max_lufs: f64,
    /// True peak in dBTP: maximum over measured and unmeasured (all) channels of the oversampled peak (BS.1770-4 Annex 2: 4x oversampling for sample rates under 96 kHz, 2x from 96 to under 192 kHz, none from 192 kHz). NEG_INFINITY for digital silence.
    pub true_peak_dbtp: f64,
    /// Sample peak in dBFS over all channels.
    pub sample_peak_dbfs: f64,
    /// Count of samples (counted per channel) whose absolute value is at least 0.9999 (clipping).
    pub clipped_samples: u64,
    /// Seconds of audio measured.
    pub seconds: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    /// Squared magnitude of the K-weighting response at `f` Hz.
    fn k_gain_sq(fs: f64, f: f64) -> f64 {
        fn mag_sq(b: &Biquad, w: f64) -> f64 {
            let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
            let nr = b.b0 + b.b1 * c1 + b.b2 * c2;
            let ni = -(b.b1 * s1 + b.b2 * s2);
            let dr = 1.0 + b.a1 * c1 + b.a2 * c2;
            let di = -(b.a1 * s1 + b.a2 * s2);
            (nr * nr + ni * ni) / (dr * dr + di * di)
        }
        let w = 2.0 * PI * f / fs;
        mag_sq(&shelf_coefficients(fs), w) * mag_sq(&rlb_coefficients(fs), w)
    }

    /// Interleaved sine, `n` frames starting at frame `start`, same on `ch` channels.
    fn tone(fs: u32, ch: usize, start: usize, n: usize, amp: f64, freq: f64) -> Vec<f32> {
        let mut v = Vec::with_capacity(n * ch);
        for i in start..start + n {
            let s = (amp * (2.0 * PI * freq * i as f64 / f64::from(fs)).sin()) as f32;
            for _ in 0..ch {
                v.push(s);
            }
        }
        v
    }

    /// Stereo amplitude for a 997 Hz tone to read `lufs`.
    fn amp_for(fs: u32, lufs: f64) -> f64 {
        let e = 10f64.powf((lufs - LUFS_OFFSET) / 10.0);
        (e / k_gain_sq(f64::from(fs), 997.0)).sqrt()
    }

    /// Stereo 997 Hz segments `(seconds, lufs)` with continuous phase.
    fn segments(fs: u32, parts: &[(f64, f64)]) -> Vec<f32> {
        let mut out = Vec::new();
        let mut at = 0usize;
        for &(secs, lufs) in parts {
            let n = (secs * f64::from(fs)) as usize;
            let amp = if lufs.is_finite() {
                amp_for(fs, lufs)
            } else {
                0.0
            };
            out.extend(tone(fs, 2, at, n, amp, 997.0));
            at += n;
        }
        out
    }

    fn measure(fs: u32, roles: &[Role], dual: bool, data: &[f32]) -> Loudness {
        let mut m = Meter::new(fs, roles, dual);
        m.push(data);
        m.finish()
    }

    fn stereo() -> Vec<Role> {
        roles_for("stereo", 2)
    }

    fn close(a: f64, b: f64) -> bool {
        a == b || (a - b).abs() < 1e-9
    }

    #[test]
    fn coefficients_match_the_standard_at_48k() {
        let s = shelf_coefficients(48_000.0);
        let r = rlb_coefficients(48_000.0);
        let want_s = [
            1.53512485958697,
            -2.69169618940638,
            1.19839281085285,
            -1.69065929318241,
            0.73248077421585,
        ];
        let got_s = [s.b0, s.b1, s.b2, s.a1, s.a2];
        for (g, w) in got_s.iter().zip(want_s) {
            assert!((g - w).abs() < 1e-9, "shelf {g} vs {w}");
        }
        let want_r = [1.0, -2.0, 1.0, -1.99004745483398, 0.99007225036621];
        let got_r = [r.b0, r.b1, r.b2, r.a1, r.a2];
        for (g, w) in got_r.iter().zip(want_r) {
            assert!((g - w).abs() < 1e-9, "rlb {g} vs {w}");
        }
    }

    #[test]
    fn stereo_sine_reads_its_level() {
        let fs = 48_000;
        for level in [-23.0f64, -33.0] {
            let amp = 10f64.powf(level / 20.0);
            let data = tone(fs, 2, 0, 20 * fs as usize, amp, 997.0);
            let r = measure(fs, &stereo(), false, &data);
            assert!(
                (r.integrated_lufs - level).abs() < 0.1,
                "{level}: {}",
                r.integrated_lufs
            );
            assert!(
                (r.momentary_max_lufs - level).abs() < 0.1,
                "momentary {}",
                r.momentary_max_lufs
            );
            assert!(
                (r.short_term_max_lufs - level).abs() < 0.1,
                "short {}",
                r.short_term_max_lufs
            );
            assert!((r.seconds - 20.0).abs() < 1e-9);
        }
    }

    #[test]
    fn mono_single_channel_and_dual_mono() {
        let fs = 48_000;
        let amp = 10f64.powf(-23.0 / 20.0);
        let data = tone(fs, 1, 0, 20 * fs as usize, amp, 997.0);
        let single = measure(fs, &[Role::Center], false, &data);
        assert!(
            (single.integrated_lufs + 26.0103).abs() < 0.1,
            "{}",
            single.integrated_lufs
        );
        let dual = measure(fs, &[Role::Center], true, &data);
        assert!(
            (dual.integrated_lufs + 23.0).abs() < 0.1,
            "{}",
            dual.integrated_lufs
        );
        assert!((dual.integrated_lufs - single.integrated_lufs - 3.0103).abs() < 1e-6);
    }

    #[test]
    fn gating_removes_quiet_and_sub_threshold_material() {
        let fs = 48_000;
        let r = measure(
            fs,
            &stereo(),
            false,
            &segments(fs, &[(20.0, -20.0), (20.0, -50.0)]),
        );
        assert!(
            (r.integrated_lufs + 20.0).abs() < 0.1,
            "relative gate: {}",
            r.integrated_lufs
        );
        let r = measure(
            fs,
            &stereo(),
            false,
            &segments(fs, &[(20.0, -20.0), (20.0, -80.0)]),
        );
        assert!(
            (r.integrated_lufs + 20.0).abs() < 0.1,
            "absolute gate: {}",
            r.integrated_lufs
        );
        let r = measure(fs, &stereo(), false, &segments(fs, &[(20.0, -80.0)]));
        assert_eq!(r.integrated_lufs, f64::NEG_INFINITY);
    }

    #[test]
    fn loudness_range() {
        let fs = 48_000;
        let r = measure(
            fs,
            &stereo(),
            false,
            &segments(fs, &[(20.0, -20.0), (20.0, -30.0)]),
        );
        assert!((r.range_lu - 10.0).abs() < 0.5, "lra {}", r.range_lu);
        let r = measure(fs, &stereo(), false, &segments(fs, &[(30.0, -23.0)]));
        assert!(r.range_lu.abs() < 0.1, "constant lra {}", r.range_lu);
        let r = measure(fs, &stereo(), false, &segments(fs, &[(2.0, -23.0)]));
        assert_eq!(r.range_lu, 0.0);
        assert_eq!(r.short_term_max_lufs, f64::NEG_INFINITY);
    }

    #[test]
    fn true_peak_sees_intersample_overs() {
        let fs = 48_000u32;
        let n = 2 * fs as usize;
        let mut data = Vec::new();
        for i in 0..n {
            let s = (PI * i as f64 / 2.0 + PI / 4.0).sin() as f32;
            data.push(s);
            data.push(s);
        }
        let r = measure(fs, &stereo(), false, &data);
        assert!(
            (r.sample_peak_dbfs + 3.0103).abs() < 0.01,
            "sample {}",
            r.sample_peak_dbfs
        );
        assert!(r.true_peak_dbtp.abs() < 0.15, "true {}", r.true_peak_dbtp);
        let silent = measure(fs, &stereo(), false, &vec![0.0; 2 * fs as usize]);
        assert_eq!(silent.true_peak_dbtp, f64::NEG_INFINITY);
        assert_eq!(silent.sample_peak_dbfs, f64::NEG_INFINITY);
        assert_eq!(silent.integrated_lufs, f64::NEG_INFINITY);
        assert_eq!(silent.clipped_samples, 0);
    }

    #[test]
    fn true_peak_accuracy_over_frequency_and_phase() {
        let mut worst_hi = f64::NEG_INFINITY;
        for (fs, floor) in [(44_100u32, -0.1f64), (96_000, -0.55)] {
            let mut worst_lo = 0.0f64;
            for frac in [0.05, 0.1, 0.17, 0.23, 0.25, 0.31, 0.37, 0.42] {
                for phase in [0.0, 0.7, 1.9, 3.1, 4.4] {
                    let n = fs as usize;
                    let mut data = Vec::new();
                    for i in 0..n {
                        data.push((2.0 * PI * frac * i as f64 + phase).sin() as f32);
                    }
                    let r = measure(fs, &[Role::Center], false, &data);
                    worst_lo = worst_lo.min(r.true_peak_dbtp);
                    worst_hi = worst_hi.max(r.true_peak_dbtp);
                }
            }
            println!("true peak of unit sines at {fs} Hz: worst low {worst_lo:.4} dBTP");
            assert!(worst_lo > floor, "{fs}: {worst_lo}");
        }
        assert!(worst_hi < 0.02, "overshoot {worst_hi}");
    }

    #[test]
    fn surround_gains_and_lfe() {
        let fs = 48_000;
        let amp = 0.1;
        let roles = roles_for("5.1", 6);
        let mut data = Vec::new();
        for i in 0..(20 * fs as usize) {
            let s = (amp * (2.0 * PI * 1000.0 * i as f64 / f64::from(fs)).sin()) as f32;
            data.extend([s; 6]);
        }
        let r = measure(fs, &roles, false, &data);
        let ms = amp * amp / 2.0 * k_gain_sq(f64::from(fs), 1000.0);
        let want = LUFS_OFFSET + 10.0 * (ms * (3.0 + 2.0 * 1.41)).log10();
        assert!(
            (r.integrated_lufs - want).abs() < 0.05,
            "{} vs {want}",
            r.integrated_lufs
        );
        // The same signal on the five measured channels only (LFE silent) reads the same.
        let mut data5 = data.clone();
        for frame in data5.chunks_exact_mut(roles.len()) {
            frame[3] = 0.0;
        }
        let r5 = measure(fs, &roles, false, &data5);
        assert!((r5.integrated_lufs - r.integrated_lufs).abs() < 1e-9);
        // Five channel layout without LFE.
        let roles5 = roles_for("5.0", 5);
        let mut d5 = Vec::new();
        for frame in data.chunks_exact(roles.len()) {
            d5.extend([frame[0], frame[1], frame[2], frame[4], frame[5]]);
        }
        let r50 = measure(fs, &roles5, false, &d5);
        assert!((r50.integrated_lufs - want).abs() < 0.05);
    }

    #[test]
    fn streaming_is_chunk_independent() {
        let fs = 48_000;
        let mut data = segments(fs, &[(5.0, -18.0), (4.0, -40.0), (3.5, -26.0)]);
        // Make it less tidy: some clipping and a drifting second channel.
        for (i, frame) in data.chunks_exact_mut(stereo().len()).enumerate() {
            frame[1] *= 0.5 + 0.5 * ((i as f32) * 1e-5).sin().abs();
            if i % 9973 == 0 {
                frame[0] = 1.0;
            }
        }
        let whole = measure(fs, &stereo(), false, &data);
        for chunk in [1usize, 7, 480, 100_000] {
            let mut m = Meter::new(fs, &stereo(), false);
            for piece in data.chunks(chunk) {
                m.push(piece);
            }
            let r = m.finish();
            assert!(
                close(r.integrated_lufs, whole.integrated_lufs),
                "chunk {chunk}"
            );
            assert!(close(r.range_lu, whole.range_lu), "chunk {chunk}");
            assert!(
                close(r.short_term_max_lufs, whole.short_term_max_lufs),
                "chunk {chunk}"
            );
            assert!(
                close(r.momentary_max_lufs, whole.momentary_max_lufs),
                "chunk {chunk}"
            );
            assert!(
                close(r.true_peak_dbtp, whole.true_peak_dbtp),
                "chunk {chunk}"
            );
            assert!(
                close(r.sample_peak_dbfs, whole.sample_peak_dbfs),
                "chunk {chunk}"
            );
            assert_eq!(r.clipped_samples, whole.clipped_samples);
            assert!(close(r.seconds, whole.seconds));
        }
    }

    #[test]
    fn sample_rate_independence() {
        for fs in [44_100u32, 48_000, 96_000, 22_050, 192_000, 11_025] {
            let amp = 10f64.powf(-23.0 / 20.0);
            let data = tone(fs, 2, 0, 10 * fs as usize, amp, 997.0);
            let r = measure(fs, &stereo(), false, &data);
            assert!(
                (r.integrated_lufs + 23.0).abs() < 0.15,
                "{fs}: {}",
                r.integrated_lufs
            );
        }
    }

    #[test]
    fn clipped_samples_are_counted() {
        let fs = 48_000u32;
        let mut data = Vec::new();
        for i in 0..fs as usize {
            let s = if (i / 240) % 2 == 0 { 1.0f32 } else { -1.0 };
            data.push(s);
            data.push(s * 0.5);
        }
        let r = measure(fs, &stereo(), false, &data);
        assert_eq!(r.clipped_samples, u64::from(fs));
        assert!(r.sample_peak_dbfs.abs() < 1e-6);
        assert!(r.true_peak_dbtp >= -1e-6);
        let quiet = measure(fs, &stereo(), false, &tone(fs, 2, 0, 1000, 0.5, 440.0));
        assert_eq!(quiet.clipped_samples, 0);
    }

    #[test]
    fn layouts_map_to_roles() {
        assert_eq!(roles_for("mono", 1), vec![Role::Center]);
        assert_eq!(roles_for("stereo", 2), vec![Role::Left, Role::Right]);
        assert_eq!(roles_for("2.1", 3)[2], Role::Lfe);
        let r51 = roles_for("5.1", 6);
        assert_eq!(r51[3], Role::Lfe);
        assert_eq!(r51[4], Role::SurroundLeft);
        assert_eq!(roles_for("5.1(side)", 6), r51);
        assert_eq!(roles_for("7.1", 8)[6], Role::SurroundLeft);
        assert_eq!(roles_for("weird", 6), r51);
        assert_eq!(roles_for("weird", 1), vec![Role::Center]);
        assert_eq!(roles_for("stereo", 6), r51);
        assert_eq!(roles_for("weird", 10).len(), 10);
        assert!(roles_for("", 0).is_empty());
    }

    #[test]
    fn speed() {
        let fs = 48_000u32;
        // One second of music-like audio: a few tones plus noise.
        let mut seed = 0x1234_5678u32;
        let mut second = Vec::new();
        for i in 0..fs as usize {
            let t = i as f64 / f64::from(fs);
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let noise = (f64::from(seed >> 8) / f64::from(1u32 << 24) - 0.5) * 0.1;
            let l =
                0.2 * (2.0 * PI * 110.0 * t).sin() + 0.1 * (2.0 * PI * 1760.0 * t).sin() + noise;
            let r =
                0.2 * (2.0 * PI * 165.0 * t).sin() + 0.1 * (2.0 * PI * 3520.0 * t).sin() - noise;
            second.push(l as f32);
            second.push(r as f32);
        }
        let mut m = Meter::new(fs, &stereo(), false);
        let start = std::time::Instant::now();
        for _ in 0..600 {
            m.push(&second);
        }
        let r = m.finish();
        let elapsed = start.elapsed().as_secs_f64();
        let factor = r.seconds / elapsed;
        println!(
            "speed: {:.0}x real time ({:.0} s of stereo 48 kHz in {:.3} s)",
            factor, r.seconds, elapsed
        );
        assert!(factor > 100.0, "too slow: {factor}x");
    }
}

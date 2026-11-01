//! Panoply media in the browser: the desktop EBU R128 / BS.1770 loudness meter (panoply-media loudness.rs, copied unchanged), the gain
//! plan of the normalize module, and a lookahead peak limiter envelope that replaces the desktop's ffmpeg alimiter.
#![allow(dead_code)]
mod loudness;

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct LoudMeter { meter: Option<loudness::Meter>, block: usize, peaks: Vec<f32>, cur: f32, in_block: usize, channels: usize }

#[wasm_bindgen]
impl LoudMeter {
    /// `layout` is "mono", "stereo", "5.1" and so on; unknown layouts fall back to the channel count.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: u32, channels: usize, layout: &str) -> LoudMeter {
        let roles = loudness::roles_for(layout, channels);
        LoudMeter { meter: Some(loudness::Meter::new(sample_rate, &roles, false)), block: ((sample_rate as f64 * 0.001).round() as usize).max(1), peaks: vec![], cur: 0.0, in_block: 0, channels }
    }
    /// Interleaved f32 samples.
    pub fn push(&mut self, interleaved: &[f32]) {
        if let Some(m) = self.meter.as_mut() { m.push(interleaved); }
        for frame in interleaved.chunks_exact(self.channels) {
            let p = frame.iter().fold(0f32, |a, &v| a.max(v.abs()));
            self.cur = self.cur.max(p); self.in_block += 1;
            if self.in_block == self.block { self.peaks.push(self.cur); self.cur = 0.0; self.in_block = 0; }
        }
    }
    /// JSON: integrated_lufs, range_lu, true_peak_dbtp, sample_peak_dbfs, seconds and more.
    pub fn finish(&mut self) -> String {
        if self.in_block > 0 { self.peaks.push(self.cur); self.in_block = 0; }
        let l = self.meter.take().map(|m| m.finish());
        match l { Some(l) => serde_json::json!({"integrated_lufs": l.integrated_lufs, "range_lu": l.range_lu, "short_term_max_lufs": l.short_term_max_lufs, "momentary_max_lufs": l.momentary_max_lufs, "true_peak_dbtp": l.true_peak_dbtp, "sample_peak_dbfs": l.sample_peak_dbfs, "clipped": l.clipped_samples, "seconds": l.seconds}).to_string(), None => "{}".into() }
    }
    /// Per millisecond gains for `gain_db`, limited so that no sample goes over `ceiling_db` (0 means no limiter): a 5 ms lookahead
    /// minimum over the needed reduction, then a 50 ms release. Applied block by block, linearly between blocks.
    pub fn envelope(&self, gain_db: f64, ceiling_db: f64, limit: bool) -> Vec<f32> {
        let g = 10f64.powf(gain_db / 20.0);
        let ceil = 10f64.powf(ceiling_db / 20.0);
        let n = self.peaks.len();
        let need: Vec<f64> = self.peaks.iter().map(|&p| if limit && f64::from(p) * g > ceil { ceil / (f64::from(p) * g) } else { 1.0 }).collect();
        let look = 8usize;
        let mut env: Vec<f64> = (0..n).map(|i| need[i.saturating_sub(look)..=(i + look).min(n.saturating_sub(1))].iter().fold(1.0f64, |m, &v| m.min(v))).collect();
        // Release: the reduction recovers over about 50 ms, but never past what this block allows.
        let rel = (-1.0f64 / 50.0).exp();
        for i in 1..n { let recovered = 1.0 - (1.0 - env[i - 1]) * rel; env[i] = env[i].min(recovered.max(0.0).min(1.0).max(env[i])); let r = 1.0 - (1.0 - env[i - 1]) * rel; if r < env[i] { env[i] = r; } env[i] = env[i].min(need[i]); }
        // Attack: ease into a reduction over the lookahead, so the gain never steps.
        for i in (0..n.saturating_sub(1)).rev() { env[i] = env[i].min(env[i + 1] + 0.1); env[i] = env[i].min(1.0); }
        env.iter().map(|&e| (e * g) as f32).collect()
    }
    pub fn block_samples(&self) -> usize { self.block }
}

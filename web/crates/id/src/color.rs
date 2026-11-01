//! The sRGB and linear-light tables of panoply-image color.rs (the only part the resampler uses).
use std::sync::OnceLock;

fn srgb_to_linear(v: f64) -> f64 { if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) } }
fn linear_to_srgb(v: f64) -> f64 { if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 } }

pub struct Luts {
    pub s8_to_lin16: [u16; 256],
    pub lin16_to_s8: Vec<u8>,
    pub lin16_to_s16: Vec<u16>,
    pub s16_to_lin16: Vec<u16>,
}

pub fn luts() -> &'static Luts {
    static LUTS: OnceLock<Luts> = OnceLock::new();
    LUTS.get_or_init(|| {
        let mut s8 = [0u16; 256];
        for (i, o) in s8.iter_mut().enumerate() { *o = (srgb_to_linear(i as f64 / 255.0) * 65535.0).round() as u16; }
        let mut l8 = vec![0u8; 65536]; let mut l16 = vec![0u16; 65536]; let mut s16 = vec![0u16; 65536];
        for i in 0..65536usize {
            let lin = i as f64 / 65535.0; let enc = linear_to_srgb(lin);
            l8[i] = (enc * 255.0).round() as u8; l16[i] = (enc * 65535.0).round() as u16; s16[i] = (srgb_to_linear(i as f64 / 65535.0) * 65535.0).round() as u16;
        }
        Luts { s8_to_lin16: s8, lin16_to_s8: l8, lin16_to_s16: l16, s16_to_lin16: s16 }
    })
}

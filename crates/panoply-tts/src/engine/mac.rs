//! AVSpeechSynthesizer on macOS, through `panoply-platform-macos`.

use panoply_platform_macos::speech;

use super::{Pcm, Settings};
use crate::Voice;
use crate::error::{Result, TtsError};

pub fn available() -> bool {
    speech::available()
}

pub fn voices() -> Vec<Voice> {
    speech::voices()
        .into_iter()
        .map(|v| Voice {
            id: v.id,
            name: v.name,
            language: v.language,
            quality: match v.quality {
                3 => "premium",
                2 => "enhanced",
                _ => "standard",
            },
        })
        .collect()
}

/// Apple's rate runs 0.0 to 1.0 with 0.5 as normal speed, and is not linear in words per
/// minute. This keeps 1.0 at 0.5 and puts 2.0 close to twice as fast (measured by
/// `crates/panoply-cli/tests/speak.rs`).
pub fn rate_for(speed: f32) -> f32 {
    let s = speed.clamp(0.5, 3.0);
    if s <= 1.0 {
        0.5 - (1.0 - s) * 0.3
    } else {
        (0.5 + (s - 1.0) * 0.13).min(1.0)
    }
}

pub fn render(text: &str, s: &Settings<'_>, sink: &mut dyn FnMut(Pcm) -> bool) -> Result<()> {
    let req = speech::Request {
        voice: s.voice,
        rate: rate_for(s.speed),
    };
    speech::synthesize(text, &req, &mut |p| {
        sink(Pcm {
            sample_rate: p.sample_rate,
            channels: p.channels,
            samples: p.samples,
        })
    })
    .map_err(|e| TtsError::Engine(e.message().to_string()))
}

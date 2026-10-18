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

/// What to hand AVSpeechSynthesisVoice: an identifier or a language tag go through as they are,
/// and a plain name such as `Samantha` (any case) is looked up in the installed voices and
/// replaced by that voice's identifier. A name that matches nothing goes through too, so the
/// engine's own "no voice called" message is the one the person sees.
pub fn resolve_voice(key: &str, voices: &[speech::Voice]) -> String {
    if voices.iter().any(|v| v.id == key) {
        return key.to_string();
    }
    voices
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case(key.trim()))
        .map_or_else(|| key.to_string(), |v| v.id.clone())
}

pub fn render(text: &str, s: &Settings<'_>, sink: &mut dyn FnMut(Pcm) -> bool) -> Result<()> {
    let voice = s.voice.map(|key| resolve_voice(key, &speech::voices()));
    let req = speech::Request {
        voice: voice.as_deref(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(id: &str, name: &str) -> speech::Voice {
        speech::Voice {
            id: id.into(),
            name: name.into(),
            language: "en-US".into(),
            quality: 1,
        }
    }

    #[test]
    fn a_name_in_any_case_becomes_the_identifier_and_other_keys_pass_through() {
        let list = [
            voice("com.apple.voice.compact.en-US.Samantha", "Samantha"),
            voice("com.apple.voice.compact.en-IN.Rishi", "Rishi"),
        ];
        assert_eq!(
            resolve_voice("samantha", &list),
            "com.apple.voice.compact.en-US.Samantha"
        );
        assert_eq!(
            resolve_voice("com.apple.voice.compact.en-IN.Rishi", &list),
            "com.apple.voice.compact.en-IN.Rishi"
        );
        assert_eq!(resolve_voice("hi-IN", &list), "hi-IN");
        assert_eq!(resolve_voice("nobody", &list), "nobody");
    }
}

//! `Windows.Media.SpeechSynthesis` on Windows, through the `windows` crate: the text goes to a
//! stream in memory, which holds a WAV, and that is handed on as audio blocks.
//!
//! Written against the documented API and compile-checked for `x86_64-pc-windows-msvc`; it has
//! not been run (there is no Windows machine in the build environment).

use windows::Media::SpeechSynthesis::{SpeechSynthesizer, VoiceInformation};
use windows::Storage::Streams::DataReader;
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::HSTRING;

use super::{Pcm, Settings};
use crate::Voice;
use crate::error::{Result, TtsError};
use crate::wav;

fn err(what: &str, e: windows::core::Error) -> TtsError {
    TtsError::Engine(format!("Windows speech failed while {what} ({e})."))
}

#[allow(unsafe_code)]
fn init() {
    // Keep the multithreaded apartment alive until the process ends, so the activation
    // factories the windows crate caches stay valid for every thread (see the OCR engine).
    static MTA: std::sync::Once = std::sync::Once::new();
    MTA.call_once(|| {
        // SAFETY: CoIncrementMTAUsage has no preconditions. The cookie is deliberately never
        // released: the apartment lives as long as the process.
        unsafe {
            let _ = CoIncrementMTAUsage();
        }
    });
    // SAFETY: RoInitialize has no preconditions; a repeated or conflicting call returns an
    // error code that is ignored on purpose.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

fn voice_of(v: &VoiceInformation) -> Option<Voice> {
    Some(Voice {
        id: v.Id().ok()?.to_string(),
        name: v.DisplayName().ok()?.to_string(),
        language: v.Language().ok()?.to_string(),
        quality: "standard",
    })
}

pub fn available() -> bool {
    init();
    SpeechSynthesizer::AllVoices().is_ok_and(|v| v.Size().unwrap_or(0) > 0)
}

pub fn voices() -> Vec<Voice> {
    init();
    SpeechSynthesizer::AllVoices()
        .map(|all| all.into_iter().filter_map(|v| voice_of(&v)).collect())
        .unwrap_or_default()
}

fn pick(key: &str) -> Result<VoiceInformation> {
    let all = SpeechSynthesizer::AllVoices().map_err(|e| err("listing voices", e))?;
    let found = all.into_iter().find_map(|v| {
        let info = voice_of(&v)?;
        (info.id == key
            || info.name.eq_ignore_ascii_case(key)
            || info.language.eq_ignore_ascii_case(key))
        .then_some(v)
    });
    found.ok_or_else(|| {
        TtsError::Engine(format!(
            "There is no voice called \"{key}\" on this PC. Run the voice list to see the installed ones."
        ))
    })
}

pub fn render(text: &str, s: &Settings<'_>, sink: &mut dyn FnMut(Pcm) -> bool) -> Result<()> {
    init();
    let synth = SpeechSynthesizer::new().map_err(|e| err("starting the engine", e))?;
    if let Some(key) = s.voice {
        let voice = pick(key)?;
        synth
            .SetVoice(&voice)
            .map_err(|e| err("choosing the voice", e))?;
    }
    let options = synth.Options().map_err(|e| err("reading the options", e))?;
    options
        .SetSpeakingRate(f64::from(s.speed.clamp(0.5, 3.0)))
        .map_err(|e| err("setting the speed", e))?;
    let stream = synth
        .SynthesizeTextToStreamAsync(&HSTRING::from(text))
        .and_then(|op| op.join())
        .map_err(|e| err("speaking", e))?;
    let size = stream.Size().map_err(|e| err("reading the audio", e))? as u32;
    let input = stream
        .GetInputStreamAt(0)
        .map_err(|e| err("reading the audio", e))?;
    let reader = DataReader::CreateDataReader(&input).map_err(|e| err("reading the audio", e))?;
    reader
        .LoadAsync(size)
        .and_then(|op| op.join())
        .map_err(|e| err("reading the audio", e))?;
    let mut bytes = vec![0u8; size as usize];
    reader
        .ReadBytes(&mut bytes)
        .map_err(|e| err("reading the audio", e))?;
    let (sample_rate, channels, samples) = wav::parse(&bytes)?;
    sink(Pcm {
        sample_rate,
        channels,
        samples,
    });
    Ok(())
}

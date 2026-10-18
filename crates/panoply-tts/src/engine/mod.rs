//! One speech engine per platform. Each turns a chunk of text into blocks of 16 bit audio.

use crate::Voice;
use crate::error::Result;

/// A block of audio.
pub struct Pcm {
    /// Samples per second.
    pub sample_rate: u32,
    /// 1 or 2.
    pub channels: u16,
    /// 16 bit samples, interleaved when stereo.
    pub samples: Vec<i16>,
}

/// What a chunk is spoken with.
pub struct Settings<'a> {
    /// A voice id, a language tag, or `None` for the system default.
    pub voice: Option<&'a str>,
    /// 1.0 is normal speed.
    pub speed: f32,
}

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
use mac as imp;

#[cfg(windows)]
mod win;
#[cfg(windows)]
use win as imp;

#[cfg(not(any(target_os = "macos", windows)))]
mod none;
#[cfg(not(any(target_os = "macos", windows)))]
use none as imp;

/// True when this system has a speech engine.
pub fn available() -> bool {
    imp::available()
}

/// The installed voices.
pub fn voices() -> Vec<Voice> {
    imp::voices()
}

/// Speak one chunk. `sink` gets each block of audio in order and returns `false` to stop.
pub fn render(text: &str, s: &Settings<'_>, sink: &mut dyn FnMut(Pcm) -> bool) -> Result<()> {
    imp::render(text, s, sink)
}

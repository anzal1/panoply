//! Systems with no speech engine (Linux). No permissively licensed engine ships with the OS,
//! and the open ones (Piper, Kokoro) need espeak-ng, which is GPL-3.0.

use super::{Pcm, Settings};
use crate::Voice;
use crate::error::{Result, TtsError};

pub fn available() -> bool {
    false
}

pub fn voices() -> Vec<Voice> {
    Vec::new()
}

pub fn render(_: &str, _: &Settings<'_>, _: &mut dyn FnMut(Pcm) -> bool) -> Result<()> {
    Err(TtsError::Unsupported(
        "Reading aloud is not available on this system.".into(),
    ))
}

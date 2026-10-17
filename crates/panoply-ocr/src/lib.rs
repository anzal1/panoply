//! OCR for Panoply: an engine ladder, word boxes with confidence, and the searchable PDF text
//! layer.
//!
//! # The ladder
//!
//! "OS first, own model second" (ADR 0004). For a set of languages the ladder tries, in order:
//!
//! 1. the operating system's engine, when it covers every requested language: Apple Vision
//!    (`VNRecognizeTextRequest`, accurate mode) on macOS, `Windows.Media.Ocr` on Windows;
//! 2. PP-OCRv5 mobile on ONNX Runtime ([`ppocr`]), on every platform, for Latin and
//!    Devanagari script;
//! 3. Tesseract, when a `tesseract` program is installed, for the rest (Bengali).
//!
//! An engine that fails (missing model with no network, a language pack not installed) hands
//! over to the next one, and the error of the last is what the user reads when all fail.
//!
//! Measured on macOS 26.6, Apple Vision reads neither Hindi nor Bengali, so those two are the
//! cases the fallbacks exist for. See `docs/research/ocr-results.md` for error rates.

// Pixel loops walk runtime sized buffers in fixed groups of 3 and 4 samples; the plain
// `chunks_exact` form reads the same (panoply-image allows it for the same reason).
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod cer;
pub mod clipboard;
pub mod engine;
pub mod error;
pub mod input;
pub mod lang;
pub mod layout;
pub mod models;
pub mod ppocr;
pub mod probe;
pub mod redact;
pub mod searchable;
pub mod textlayer;
pub mod types;

use panoply_core::CancelToken;

pub use error::{OcrError, Result};
pub use lang::{Lang, Script};
pub use models::Fetch;
pub use types::{EngineKind, Line, OcrImage, OcrPage, Rect, Word};

/// The engine ladder.
#[derive(Debug, Clone, Default)]
pub struct Ocr {
    /// The PP-OCR engine and its settings.
    pub ppocr: ppocr::PpOcr,
    /// Use this engine only, for measuring and tests.
    pub only: Option<EngineKind>,
}

impl Ocr {
    /// The default ladder. `PANOPLY_OCR_ENGINE` (`vision`, `windows`, `ppocr` or `tesseract`)
    /// narrows it to one engine, for measuring an engine on its own or for choosing the model
    /// path on a machine that also has an OS engine.
    pub fn new() -> Self {
        let only = std::env::var("PANOPLY_OCR_ENGINE").ok().and_then(|v| {
            match v.to_ascii_lowercase().as_str() {
                "vision" => Some(EngineKind::AppleVision),
                "windows" => Some(EngineKind::WindowsOcr),
                "ppocr" => Some(EngineKind::PpOcr),
                "tesseract" => Some(EngineKind::Tesseract),
                _ => None,
            }
        });
        Ocr {
            only,
            ..Ocr::default()
        }
    }

    /// A ladder with one engine only.
    pub fn only(kind: EngineKind) -> Self {
        Ocr {
            only: Some(kind),
            ..Ocr::default()
        }
    }

    /// The engines that could read these languages on this machine, in the order they are
    /// tried. Whether each one will actually succeed is only known when it runs (a model may
    /// still need downloading).
    pub fn candidates(&self, langs: &[Lang]) -> Vec<EngineKind> {
        let mut v = Vec::new();
        if engine::vision::covers(langs) {
            v.push(EngineKind::AppleVision);
        }
        if engine::windows_ocr::covers(langs) {
            v.push(EngineKind::WindowsOcr);
        }
        if ppocr::PpOcr::covers(langs) {
            v.push(EngineKind::PpOcr);
        }
        if engine::tesseract::covers(langs) {
            v.push(EngineKind::Tesseract);
        }
        if let Some(only) = self.only {
            v.retain(|k| *k == only);
        }
        v
    }

    /// Read one page image with the first engine that can.
    pub fn recognize(
        &self,
        img: &OcrImage,
        langs: &[Lang],
        fetch: &mut Fetch<'_>,
        cancel: &CancelToken,
    ) -> Result<OcrPage> {
        let order = self.candidates(langs);
        if order.is_empty() {
            return Err(OcrError::Unsupported(no_engine_message(langs, self.only)));
        }
        let mut first_err: Option<OcrError> = None;
        for kind in order {
            if cancel.is_cancelled() {
                return Err(OcrError::Cancelled);
            }
            let r = match kind {
                EngineKind::AppleVision => engine::vision::recognize(img, langs),
                EngineKind::WindowsOcr => engine::windows_ocr::recognize(img, langs),
                EngineKind::PpOcr => self.ppocr.recognize(img, langs, fetch, cancel),
                EngineKind::Tesseract => self.tesseract(img, langs, fetch, cancel),
            };
            match r {
                Ok(page) => return Ok(self.second_look(img, langs, page, fetch, cancel)),
                Err(OcrError::Cancelled) => return Err(OcrError::Cancelled),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        Err(first_err.expect("at least one engine was tried"))
    }
}

impl Ocr {
    /// Tesseract. Its own page layout analysis is read first; when the PP-OCR detector can run,
    /// the lines it finds are also read one by one, and whichever reading found more words
    /// wins (ties go to the page reading). Layout analysis can drop whole lines of a small
    /// picture and line reading can fail on a line it would read inside a page, so neither is
    /// right everywhere; the detector's models are small and are only needed for this check.
    fn tesseract(
        &self,
        img: &OcrImage,
        langs: &[Lang],
        fetch: &mut Fetch<'_>,
        cancel: &CancelToken,
    ) -> Result<OcrPage> {
        let page = engine::tesseract::recognize(img, langs, fetch);
        if cancel.is_cancelled() {
            return Err(OcrError::Cancelled);
        }
        let by_lines = self
            .ppocr
            .detect(img, fetch, cancel)
            .ok()
            .filter(|r| !r.is_empty())
            .and_then(|rects| engine::tesseract::recognize_lines(img, &rects, langs, fetch).ok());
        match (page, by_lines) {
            (Ok(a), Some(b)) if b.word_count() > a.word_count() => Ok(b),
            (Ok(a), _) => Ok(a),
            (Err(_), Some(b)) => Ok(b),
            (Err(e), None) => Err(e),
        }
    }

    /// With no language chosen, the OS engine may find nothing because the page is in a script
    /// it does not read (Vision reads no Hindi or Bengali). When the first reading found next
    /// to nothing (or only unsure text) on a picture that looks like lines of text, the fallback readers get a try, in
    /// order: Devanagari, then Bengali (when Tesseract is there). A reading wins when its mean
    /// confidence is at least one half and it holds clearly more sure text than the first. Nothing is tried when a
    /// language was chosen, when one engine was forced, or when the fallbacks cannot run
    /// (models not downloaded and no network).
    fn second_look(
        &self,
        img: &OcrImage,
        langs: &[Lang],
        first: OcrPage,
        fetch: &mut Fetch<'_>,
        cancel: &CancelToken,
    ) -> OcrPage {
        if !langs.is_empty()
            || self.only.is_some()
            || (first.word_count() > 2 && first.mean_confidence() >= 0.6)
            || !probe::looks_like_text(img, 3)
        {
            return first;
        }
        let mut best = first;
        for code in ["hi", "bn"] {
            // Good enough: stop before downloading more.
            if best.word_count() > 2 && best.mean_confidence() >= 0.6 {
                break;
            }
            let Ok(l) = Lang::parse(code) else { continue };
            let chosen = [l];
            let tried = if ppocr::PpOcr::covers(&chosen) {
                self.ppocr.recognize(img, &chosen, fetch, cancel)
            } else if engine::tesseract::covers(&chosen) {
                engine::tesseract::recognize(img, &chosen, fetch)
            } else {
                continue;
            };
            if let Ok(p) = tried
                && p.mean_confidence() >= 0.5
                && weight(&p) > 1.2 * weight(&best)
            {
                best = p;
            }
        }
        best
    }
}

/// How much sure text a reading holds: letters weighted by confidence.
fn weight(p: &OcrPage) -> f32 {
    p.words()
        .map(|w| w.confidence * w.text.chars().count() as f32)
        .sum()
}

fn no_engine_message(langs: &[Lang], only: Option<EngineKind>) -> String {
    if let Some(k) = only {
        return format!(
            "{} cannot read {} on this machine.",
            k.name(),
            if langs.is_empty() {
                "text".to_string()
            } else {
                lang::phrase(langs)
            }
        );
    }
    let bn = langs.iter().any(|l| l.script() == Script::Bengali);
    if bn {
        "Reading Bengali needs Tesseract, which is not installed on this machine. Install it and try again."
            .to_string()
    } else {
        format!(
            "Panoply cannot read {} on this machine yet.",
            lang::phrase(langs)
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hindi_never_goes_to_apple_vision() {
        let hi = Lang::parse_list("hi").unwrap();
        let c = Ocr::new().candidates(&hi);
        assert!(!c.contains(&EngineKind::AppleVision), "{c:?}");
        assert!(c.contains(&EngineKind::PpOcr), "{c:?}");
    }

    #[test]
    fn an_only_ladder_filters_candidates() {
        let en = Lang::parse_list("en").unwrap();
        let c = Ocr::only(EngineKind::PpOcr).candidates(&en);
        assert_eq!(c, vec![EngineKind::PpOcr]);
        let none = Ocr::only(EngineKind::WindowsOcr).candidates(&en);
        // Not on this platform unless it is Windows.
        assert_eq!(none.is_empty(), !cfg!(windows));
    }

    #[test]
    fn nothing_covers_an_unreadable_mix() {
        // Hindi with Bengali: no single engine reads both unless Tesseract is installed.
        let l = Lang::parse_list("hi,bn").unwrap();
        let c = Ocr::new().candidates(&l);
        assert!(!c.contains(&EngineKind::AppleVision) && !c.contains(&EngineKind::PpOcr));
    }
}

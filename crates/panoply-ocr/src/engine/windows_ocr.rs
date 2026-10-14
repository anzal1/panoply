//! `Windows.Media.Ocr` on Windows, through the `windows` crate. On other systems this reports
//! itself unavailable.
//!
//! The engine reads one language at a time (the languages with an OCR pack installed on the
//! machine), returns lines of words with boxes, and does not report a confidence. This was
//! written against the documented API and compile-checked for `x86_64-pc-windows-msvc`; it has
//! not been run (there is no Windows machine in the build environment).

use crate::error::{OcrError, Result};
use crate::lang::Lang;
use crate::types::{OcrImage, OcrPage};

/// True when Windows OCR can read every one of these languages (no languages means the user's
/// profile languages).
pub fn covers(langs: &[Lang]) -> bool {
    imp::covers(langs)
}

/// Read one image.
pub fn recognize(img: &OcrImage, langs: &[Lang]) -> Result<OcrPage> {
    imp::recognize(img, langs)
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    pub fn covers(_: &[Lang]) -> bool {
        false
    }

    pub fn recognize(_: &OcrImage, _: &[Lang]) -> Result<OcrPage> {
        Err(OcrError::Unsupported(
            "Windows OCR is only on Windows.".into(),
        ))
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use crate::types::{EngineKind, Line, Rect, Word};
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::DataWriter;
    use windows::Win32::System::Com::CoIncrementMTAUsage;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::HSTRING;

    fn win_err(what: &str, e: windows::core::Error) -> OcrError {
        OcrError::Engine(format!("Windows OCR failed while {what} ({e})."))
    }

    #[allow(unsafe_code)]
    fn init() {
        // The windows crate caches the activation factories it creates in statics. Those point
        // into the multithreaded apartment, which Windows tears down when the last thread that
        // initialized it exits; a later thread would then call through a dead pointer and crash
        // the process. Asking for the apartment to stay alive until the process ends keeps the
        // cached factories valid for every thread, whenever it starts.
        static MTA: std::sync::Once = std::sync::Once::new();
        MTA.call_once(|| {
            // SAFETY: CoIncrementMTAUsage has no preconditions. The cookie is deliberately never
            // released: the apartment lives as long as the process.
            unsafe {
                let _ = CoIncrementMTAUsage();
            }
        });
        // Already initialized (on this thread, with either apartment model) is fine.
        // SAFETY: RoInitialize has no preconditions; a repeated or conflicting call returns an
        // error code that is ignored on purpose.
        unsafe {
            let _ = RoInitialize(RO_INIT_MULTITHREADED);
        }
    }

    fn engine_for(langs: &[Lang]) -> Result<OcrEngine> {
        init();
        if langs.is_empty() {
            return OcrEngine::TryCreateFromUserProfileLanguages()
                .map_err(|e| win_err("starting", e));
        }
        for l in langs {
            let lang = Language::CreateLanguage(&HSTRING::from(l.windows()))
                .map_err(|e| win_err("starting", e))?;
            if OcrEngine::IsLanguageSupported(&lang).unwrap_or(false)
                && let Ok(engine) = OcrEngine::TryCreateFromLanguage(&lang)
            {
                return Ok(engine);
            }
        }
        Err(OcrError::Unsupported(format!(
            "Windows OCR has no language pack installed for {}.",
            crate::lang::phrase(langs)
        )))
    }

    pub fn covers(langs: &[Lang]) -> bool {
        init();
        if langs.is_empty() {
            return OcrEngine::TryCreateFromUserProfileLanguages().is_ok();
        }
        langs.iter().all(|l| {
            Language::CreateLanguage(&HSTRING::from(l.windows()))
                .ok()
                .and_then(|lang| OcrEngine::IsLanguageSupported(&lang).ok())
                .unwrap_or(false)
        })
    }

    pub fn recognize(img: &OcrImage, langs: &[Lang]) -> Result<OcrPage> {
        let engine = engine_for(langs)?;
        let max = OcrEngine::MaxImageDimension().unwrap_or(10_000);
        let (w, h) = (img.width, img.height);
        if w > max || h > max {
            return Err(OcrError::BadInput(format!(
                "This picture is larger than Windows OCR reads ({max} pixels on a side). Shrink it first."
            )));
        }
        // BGRA, opaque.
        let mut bgra = Vec::with_capacity(w as usize * h as usize * 4);
        if img.channels == 3 {
            for p in img.data.chunks_exact(3) {
                bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        } else {
            for &g in &img.data {
                bgra.extend_from_slice(&[g, g, g, 255]);
            }
        }
        let writer = DataWriter::new().map_err(|e| win_err("preparing the picture", e))?;
        writer
            .WriteBytes(&bgra)
            .map_err(|e| win_err("preparing the picture", e))?;
        let buffer = writer
            .DetachBuffer()
            .map_err(|e| win_err("preparing the picture", e))?;
        let bitmap = SoftwareBitmap::CreateWithAlpha(
            BitmapPixelFormat::Bgra8,
            w as i32,
            h as i32,
            BitmapAlphaMode::Ignore,
        )
        .map_err(|e| win_err("preparing the picture", e))?;
        bitmap
            .CopyFromBuffer(&buffer)
            .map_err(|e| win_err("preparing the picture", e))?;
        let result = engine
            .RecognizeAsync(&bitmap)
            .map_err(|e| win_err("reading", e))?
            .join()
            .map_err(|e| win_err("reading", e))?;
        let mut lines = Vec::new();
        for line in result.Lines().map_err(|e| win_err("reading", e))? {
            let mut words = Vec::new();
            for word in line.Words().map_err(|e| win_err("reading", e))? {
                let r = word.BoundingRect().map_err(|e| win_err("reading", e))?;
                let text = word.Text().map_err(|e| win_err("reading", e))?.to_string();
                if text.trim().is_empty() {
                    continue;
                }
                words.push(Word {
                    text,
                    bbox: Rect {
                        x: r.X,
                        y: r.Y,
                        w: r.Width,
                        h: r.Height,
                    },
                    // Windows OCR does not report a confidence.
                    confidence: 1.0,
                });
            }
            if let Some(l) = Line::from_words(words) {
                lines.push(l);
            }
        }
        Ok(OcrPage {
            width: w,
            height: h,
            lines: crate::layout::reading_order(lines),
            engine: EngineKind::WindowsOcr,
            languages: langs.to_vec(),
        })
    }
}

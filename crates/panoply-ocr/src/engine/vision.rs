//! Apple Vision on macOS, through `panoply-platform-macos`. On other systems this reports
//! itself unavailable.

use crate::error::{OcrError, Result};
use crate::lang::Lang;
use crate::types::{EngineKind, Line, OcrImage, OcrPage, Rect, Word};

/// True when Apple Vision can run here.
pub fn available() -> bool {
    #[cfg(target_os = "macos")]
    {
        panoply_platform_macos::ocr::available()
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// True when Vision reads every one of these languages (no languages means automatic, which
/// it always can do).
pub fn covers(langs: &[Lang]) -> bool {
    available() && langs.iter().all(|l| l.vision().is_some() && supported(l))
}

#[cfg(target_os = "macos")]
fn supported(l: &Lang) -> bool {
    use std::sync::OnceLock;
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    let list = LIST.get_or_init(|| panoply_platform_macos::ocr::supported_languages(true));
    l.vision().is_some_and(|v| list.iter().any(|s| s == v))
}

#[cfg(not(target_os = "macos"))]
fn supported(_: &Lang) -> bool {
    false
}

/// The languages Vision reads on this Mac, as Panoply language codes.
pub fn languages() -> Vec<Lang> {
    Lang::all()
        .filter(|l| available() && supported(l))
        .collect()
}

/// Read one image with Vision in accurate mode.
#[cfg(target_os = "macos")]
pub fn recognize(img: &OcrImage, langs: &[Lang]) -> Result<OcrPage> {
    use panoply_platform_macos::ocr;
    let tags: Vec<&str> = langs.iter().filter_map(Lang::vision).collect();
    let req = ocr::Request {
        languages: &tags,
        auto_detect: true,
        accurate: true,
        language_correction: true,
    };
    let obs = ocr::recognize(
        img.width as usize,
        img.height as usize,
        usize::from(img.channels),
        &img.data,
        &req,
    )
    .map_err(|e| OcrError::Engine(e.message().to_string()))?;
    let (w, h) = (img.width as f32, img.height as f32);
    let mut lines = Vec::new();
    for o in obs {
        let words: Vec<Word> = o
            .words
            .iter()
            .map(|b| Word {
                text: b.text.clone(),
                // Vision's origin is the bottom left, and its boxes are fractions of the image.
                bbox: Rect {
                    x: b.x * w,
                    y: (1.0 - b.y - b.h) * h,
                    w: b.w * w,
                    h: b.h * h,
                },
                confidence: o.confidence,
            })
            .collect();
        if let Some(l) = Line::from_words(words) {
            lines.push(l);
        }
    }
    Ok(OcrPage {
        width: img.width,
        height: img.height,
        lines: crate::layout::reading_order(lines),
        engine: EngineKind::AppleVision,
        languages: langs.to_vec(),
    })
}

/// Read one image with Vision.
#[cfg(not(target_os = "macos"))]
pub fn recognize(_img: &OcrImage, _langs: &[Lang]) -> Result<OcrPage> {
    Err(OcrError::Unsupported(
        "Apple Vision is only on macOS.".into(),
    ))
}

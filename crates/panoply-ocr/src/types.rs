//! What every engine returns: pages of lines of words, each with a box and a confidence.

use std::fmt;

/// An axis aligned box in pixels of the image the engine was given. The origin is the top left
/// corner and y grows downwards.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// The right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// The bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// The smallest box holding both.
    pub fn union(&self, o: &Rect) -> Rect {
        let x = self.x.min(o.x);
        let y = self.y.min(o.y);
        Rect {
            x,
            y,
            w: self.right().max(o.right()) - x,
            h: self.bottom().max(o.bottom()) - y,
        }
    }

    /// Height of the overlap of the two boxes' vertical ranges, 0 when they do not overlap.
    pub fn v_overlap(&self, o: &Rect) -> f32 {
        (self.bottom().min(o.bottom()) - self.y.max(o.y)).max(0.0)
    }
}

/// One recognized word.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    /// The text, without surrounding spaces.
    pub text: String,
    /// Where it is on the page image.
    pub bbox: Rect,
    /// How sure the engine is, 0.0 to 1.0. Apple Vision reports one value per line, so all
    /// the words of a line share it.
    pub confidence: f32,
}

/// One line of text, left to right in the order it was read.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// The words of the line. Never empty.
    pub words: Vec<Word>,
    /// The box around the line.
    pub bbox: Rect,
}

impl Line {
    /// A line from its words. The box is the union of the word boxes.
    pub fn from_words(words: Vec<Word>) -> Option<Line> {
        let mut it = words.iter();
        let first = it.next()?.bbox;
        let bbox = it.fold(first, |b, w| b.union(&w.bbox));
        Some(Line { words, bbox })
    }

    /// The line as text: words separated by one space.
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Mean word confidence.
    pub fn confidence(&self) -> f32 {
        if self.words.is_empty() {
            return 0.0;
        }
        self.words.iter().map(|w| w.confidence).sum::<f32>() / self.words.len() as f32
    }
}

/// Which engine produced a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineKind {
    /// Apple Vision (`VNRecognizeTextRequest`), macOS.
    AppleVision,
    /// `Windows.Media.Ocr`, Windows.
    WindowsOcr,
    /// PP-OCRv5 mobile detection and recognition on ONNX Runtime, any platform.
    PpOcr,
    /// Tesseract, through a `tesseract` program on the machine.
    Tesseract,
}

impl EngineKind {
    /// A short name for reports.
    pub fn name(self) -> &'static str {
        match self {
            EngineKind::AppleVision => "Apple Vision",
            EngineKind::WindowsOcr => "Windows OCR",
            EngineKind::PpOcr => "PP-OCRv5",
            EngineKind::Tesseract => "Tesseract",
        }
    }
}

impl fmt::Display for EngineKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// The result of reading one page image.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrPage {
    /// Width of the image that was read, in pixels.
    pub width: u32,
    /// Height of the image that was read, in pixels.
    pub height: u32,
    /// Lines in reading order.
    pub lines: Vec<Line>,
    /// The engine that read it.
    pub engine: EngineKind,
    /// The languages the engine was asked for (empty: automatic).
    pub languages: Vec<crate::lang::Lang>,
}

/// Words below this confidence are underlined in the preview and counted as low confidence.
pub const LOW_CONFIDENCE: f32 = 0.6;

impl OcrPage {
    /// Every word in reading order.
    pub fn words(&self) -> impl Iterator<Item = &Word> {
        self.lines.iter().flat_map(|l| l.words.iter())
    }

    /// Number of words.
    pub fn word_count(&self) -> usize {
        self.lines.iter().map(|l| l.words.len()).sum()
    }

    /// Mean confidence over words, weighted by letters so that a stray low confidence mark
    /// does not outweigh a paragraph. 0.0 for an empty page.
    pub fn mean_confidence(&self) -> f32 {
        let (mut sum, mut n) = (0.0f32, 0usize);
        for w in self.words() {
            let len = w.text.chars().count().max(1);
            sum += w.confidence * len as f32;
            n += len;
        }
        if n == 0 { 0.0 } else { sum / n as f32 }
    }

    /// Words under [`LOW_CONFIDENCE`].
    pub fn low_confidence_words(&self) -> usize {
        self.words()
            .filter(|w| w.confidence < LOW_CONFIDENCE)
            .count()
    }

    /// Plain text: one line per recognized line, in reading order.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(Line::text)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// A picture handed to an engine: 8 bit pixels, row major, with no padding.
#[derive(Debug, Clone)]
pub struct OcrImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// 1 for gray, 3 for RGB.
    pub channels: u8,
    /// `width * height * channels` bytes.
    pub data: Vec<u8>,
}

impl OcrImage {
    /// A gray image. `None` when the length does not match.
    pub fn gray(width: u32, height: u32, data: Vec<u8>) -> Option<OcrImage> {
        (data.len() == width as usize * height as usize && width > 0 && height > 0).then_some(
            OcrImage {
                width,
                height,
                channels: 1,
                data,
            },
        )
    }

    /// An RGB image. `None` when the length does not match.
    pub fn rgb(width: u32, height: u32, data: Vec<u8>) -> Option<OcrImage> {
        (data.len() == width as usize * height as usize * 3 && width > 0 && height > 0).then_some(
            OcrImage {
                width,
                height,
                channels: 3,
                data,
            },
        )
    }

    /// RGBA pixels (straight alpha) composited onto white. Screenshots and PNG scans often
    /// carry an alpha channel, and dark text on a transparent background must not turn black on
    /// black.
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<OcrImage> {
        if rgba.len() != width as usize * height as usize * 4 || width == 0 || height == 0 {
            return None;
        }
        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
        for px in rgba.chunks_exact(4) {
            let a = u32::from(px[3]);
            for &c in &px[..3] {
                let c = u32::from(c);
                rgb.push(((c * a + 255 * (255 - a) + 127) / 255) as u8);
            }
        }
        OcrImage::rgb(width, height, rgb)
    }

    /// The pixels as one gray channel (Rec. 601 luma for color).
    pub fn to_gray(&self) -> Vec<u8> {
        if self.channels == 1 {
            return self.data.clone();
        }
        self.data
            .chunks_exact(3)
            .map(|p| {
                ((u32::from(p[0]) * 299 + u32::from(p[1]) * 587 + u32::from(p[2]) * 114) / 1000)
                    as u8
            })
            .collect()
    }

    /// The pixels as RGB, three equal channels for a gray image.
    pub fn to_rgb(&self) -> Vec<u8> {
        if self.channels == 3 {
            return self.data.clone();
        }
        self.data.iter().flat_map(|&g| [g, g, g]).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(t: &str, x: f32, c: f32) -> Word {
        Word {
            text: t.into(),
            bbox: Rect {
                x,
                y: 10.0,
                w: 20.0,
                h: 12.0,
            },
            confidence: c,
        }
    }

    #[test]
    fn a_line_joins_words_and_takes_the_union_box() {
        let l = Line::from_words(vec![word("hello", 5.0, 0.9), word("world", 40.0, 0.5)]).unwrap();
        assert_eq!(l.text(), "hello world");
        assert_eq!(l.bbox.x, 5.0);
        assert_eq!(l.bbox.right(), 60.0);
        assert!((l.confidence() - 0.7).abs() < 1e-6);
        assert!(Line::from_words(vec![]).is_none());
    }

    #[test]
    fn page_statistics_count_low_confidence_words() {
        let l = Line::from_words(vec![word("a", 0.0, 0.2), word("long", 30.0, 1.0)]).unwrap();
        let p = OcrPage {
            width: 100,
            height: 50,
            lines: vec![l],
            engine: EngineKind::PpOcr,
            languages: vec![],
        };
        assert_eq!(p.word_count(), 2);
        assert_eq!(p.low_confidence_words(), 1);
        // "a" is one letter at 0.2, "long" four at 1.0: letter weighted mean 0.84.
        assert!((p.mean_confidence() - 0.84).abs() < 1e-6);
        assert_eq!(p.text(), "a long");
    }

    #[test]
    fn transparent_pixels_become_white() {
        let img = OcrImage::from_rgba(2, 1, &[0, 0, 0, 0, 10, 20, 30, 255]).unwrap();
        assert_eq!(img.data, vec![255, 255, 255, 10, 20, 30]);
    }

    #[test]
    fn sizes_are_checked() {
        assert!(OcrImage::gray(2, 2, vec![0; 3]).is_none());
        assert!(OcrImage::rgb(2, 2, vec![0; 12]).is_some());
        assert!(OcrImage::gray(0, 2, vec![]).is_none());
    }
}

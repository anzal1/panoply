//! File formats the pipeline knows, found from the first bytes (never from the name).

use std::path::Path;

/// A raster format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    /// JPEG.
    Jpeg,
    /// PNG (including APNG).
    Png,
    /// WebP (including animated).
    WebP,
    /// GIF.
    Gif,
    /// TIFF (also the container of DNG and many RAW files).
    Tiff,
    /// Windows bitmap.
    Bmp,
    /// Windows icon.
    Ico,
    /// HEIC and HEIF.
    Heic,
    /// AVIF.
    Avif,
    /// JPEG XL.
    Jxl,
    /// JPEG 2000.
    Jp2,
    /// Camera RAW (DNG, CR2, CR3, NEF, ARW, RAF, ORF, RW2).
    Raw,
    /// OpenEXR.
    Exr,
}

impl Format {
    /// True when the decoder can hand back a smaller picture for less work and memory than the
    /// full one, given a size hint at open time: JPEG through its DCT scaling, and the formats
    /// the operating system decodes (HEIC, AVIF, RAW and the rest) through a reduced decode.
    pub fn decodes_reduced(self) -> bool {
        matches!(
            self,
            Self::Jpeg | Self::Heic | Self::Avif | Self::Jxl | Self::Jp2 | Self::Raw | Self::Exr
        )
    }

    /// Short lowercase name, the extension used for output files.
    pub fn ext(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Png => "png",
            Self::WebP => "webp",
            Self::Gif => "gif",
            Self::Tiff => "tif",
            Self::Bmp => "bmp",
            Self::Ico => "ico",
            Self::Heic => "heic",
            Self::Avif => "avif",
            Self::Jxl => "jxl",
            Self::Jp2 => "jp2",
            Self::Raw => "dng",
            Self::Exr => "exr",
        }
    }

    /// Name shown to people.
    pub fn label(self) -> &'static str {
        match self {
            Self::Jpeg => "JPEG",
            Self::Png => "PNG",
            Self::WebP => "WebP",
            Self::Gif => "GIF",
            Self::Tiff => "TIFF",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
            Self::Heic => "HEIC",
            Self::Avif => "AVIF",
            Self::Jxl => "JPEG XL",
            Self::Jp2 => "JPEG 2000",
            Self::Raw => "RAW",
            Self::Exr => "OpenEXR",
        }
    }

    /// Parse a user supplied name such as `jpg`, `JPEG` or `.heic`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name
            .trim()
            .trim_start_matches('.')
            .to_ascii_lowercase()
            .as_str()
        {
            "jpg" | "jpeg" | "jpe" | "jfif" => Some(Self::Jpeg),
            "png" => Some(Self::Png),
            "webp" => Some(Self::WebP),
            "gif" => Some(Self::Gif),
            "tif" | "tiff" => Some(Self::Tiff),
            "bmp" => Some(Self::Bmp),
            "ico" => Some(Self::Ico),
            "heic" | "heif" | "hif" => Some(Self::Heic),
            "avif" => Some(Self::Avif),
            "jxl" => Some(Self::Jxl),
            "jp2" | "j2k" => Some(Self::Jp2),
            "dng" | "cr2" | "cr3" | "nef" | "arw" | "raf" | "orf" | "rw2" => Some(Self::Raw),
            "exr" => Some(Self::Exr),
            _ => None,
        }
    }

    /// True when the format can carry transparency.
    pub fn has_alpha(self) -> bool {
        matches!(
            self,
            Self::Png
                | Self::WebP
                | Self::Gif
                | Self::Tiff
                | Self::Heic
                | Self::Avif
                | Self::Jxl
                | Self::Ico
        )
    }
}

/// Identify a file from its first bytes. `ext_hint` only separates RAW files from plain TIFF.
pub fn detect(head: &[u8], ext_hint: Option<&str>) -> Option<Format> {
    let raw_ext = ext_hint.and_then(Format::from_name) == Some(Format::Raw);
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(Format::Jpeg);
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Some(Format::Png);
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some(Format::Gif);
    }
    if head.len() >= 12 && &head[0..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return Some(Format::WebP);
    }
    if head.starts_with(b"II*\0") || head.starts_with(b"MM\0*") {
        return Some(if raw_ext { Format::Raw } else { Format::Tiff });
    }
    if head.starts_with(b"IIRO")
        || head.starts_with(b"FUJIFILMCCD-RAW")
        || head.starts_with(b"IIU\0")
    {
        return Some(Format::Raw);
    }
    if head.starts_with(b"BM") && head.len() > 14 {
        return Some(Format::Bmp);
    }
    if head.starts_with(&[0, 0, 1, 0]) && head.len() > 6 {
        return Some(Format::Ico);
    }
    if head.starts_with(&[0xFF, 0x0A]) || head.starts_with(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' '])
    {
        return Some(Format::Jxl);
    }
    if head.starts_with(&[0, 0, 0, 0x0C, b'j', b'P', b' ', b' '])
        || head.starts_with(&[0xFF, 0x4F, 0xFF, 0x51])
    {
        return Some(Format::Jp2);
    }
    if head.starts_with(&[0x76, 0x2F, 0x31, 0x01]) {
        return Some(Format::Exr);
    }
    if head.len() >= 12 && &head[4..8] == b"ftyp" {
        let brand = &head[8..12];
        return match brand {
            b"avif" | b"avis" => Some(Format::Avif),
            b"heic" | b"heix" | b"hevc" | b"hevx" | b"heim" | b"heis" | b"hevm" | b"hevs"
            | b"mif1" | b"msf1" | b"heif" => {
                // mif1 and msf1 are generic HEIF: the compatible brands tell AVIF from HEIC.
                let boxlen = u32::from_be_bytes([head[0], head[1], head[2], head[3]]) as usize;
                let end = boxlen.min(head.len());
                let brands = &head[8..end];
                if brands.windows(4).any(|w| w == b"avif" || w == b"avis")
                    && !brands.windows(4).any(|w| w == b"heic")
                {
                    Some(Format::Avif)
                } else {
                    Some(Format::Heic)
                }
            }
            b"crx " => Some(Format::Raw),
            _ => None,
        };
    }
    None
}

/// Read the first bytes of a file and identify it.
pub fn detect_file(path: &Path) -> Option<Format> {
    use std::io::Read;
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    detect(&head[..n], path.extension().and_then(|e| e.to_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_bytes_win_over_names() {
        assert_eq!(
            detect(&[0xFF, 0xD8, 0xFF, 0xE0], Some("png")),
            Some(Format::Jpeg)
        );
        assert_eq!(detect(b"RIFF\0\0\0\0WEBPVP8 ", None), Some(Format::WebP));
        assert_eq!(
            detect(b"\0\0\0\x18ftypavif\0\0\0\0avifmif1", None),
            Some(Format::Avif)
        );
        assert_eq!(
            detect(b"\0\0\0\x18ftypheic\0\0\0\0mif1heic", None),
            Some(Format::Heic)
        );
        assert_eq!(detect(b"II*\0\x08\0\0\0", Some("dng")), Some(Format::Raw));
        assert_eq!(detect(b"II*\0\x08\0\0\0", Some("tif")), Some(Format::Tiff));
        assert_eq!(detect(b"hello world", None), None);
    }

    #[test]
    fn names_parse() {
        assert_eq!(Format::from_name(".JPEG"), Some(Format::Jpeg));
        assert_eq!(Format::from_name("hif"), Some(Format::Heic));
        assert_eq!(Format::from_name("xyz"), None);
    }
}

/// The sentence for a file that is not a picture Panoply reads. When the bytes say what it really
/// is (a PDF named .jpg, a video, an SVG), say that, so the next step is clear.
pub fn not_an_image_message(head: &[u8]) -> String {
    use panoply_core::sniff::FamilyHint;
    let text_start = String::from_utf8_lossy(&head[..head.len().min(64)]).to_ascii_lowercase();
    if text_start.contains("<svg") || (text_start.contains("<?xml") && head.len() < 64) {
        return "This is an SVG drawing, which Panoply cannot open yet. Export it as a PNG or JPG first.".into();
    }
    let s = panoply_core::sniff_bytes(head, None);
    if s.from_bytes {
        if s.mime == "application/pdf" {
            return "This file is a PDF, not a picture. Use PDF to images to turn its pages into pictures.".into();
        }
        let kind = match s.family_hint {
            FamilyHint::Video => Some("a video"),
            FamilyHint::Audio => Some("a sound file"),
            FamilyHint::Document => Some("a document"),
            FamilyHint::Archive => Some("an archive"),
            _ => None,
        };
        if let Some(kind) = kind {
            return format!("This file is {kind}, not a picture. Pick a tool for {kind} instead.");
        }
    }
    "This file is not an image Panoply can read. Check that it is a JPEG, PNG, WebP, GIF, TIFF, BMP, HEIC or AVIF file.".into()
}

#[cfg(test)]
mod not_an_image_tests {
    use super::not_an_image_message;

    #[test]
    fn a_pdf_named_like_a_picture_is_called_a_pdf() {
        assert!(not_an_image_message(b"%PDF-1.7\n%\xE2\xE3").contains("PDF"));
        assert!(
            not_an_image_message(b"<svg xmlns=\"http://www.w3.org/2000/svg\">").contains("SVG")
        );
        assert!(
            not_an_image_message(&[0x13, 0x37, 0x42, 0x99, 1, 2, 3])
                .contains("not an image Panoply can read")
        );
    }
}

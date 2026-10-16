//! File identification from magic bytes, with the extension as a fallback.
//!
//! The bytes win. A file named `photo.jpg` that is really a PNG sniffs as PNG.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// Enough bytes for every signature `infer` checks.
const HEAD_BYTES: usize = 8192;

/// Coarse family used to route a file to the right modules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FamilyHint {
    /// Raster and vector pictures.
    Image,
    /// Audio.
    Audio,
    /// Video.
    Video,
    /// Documents, PDFs, office files, ebooks.
    Document,
    /// Archives and compressed containers.
    Archive,
    /// Fonts.
    Font,
    /// Text and data formats.
    Text,
    /// Not recognised.
    Unknown,
}

impl FamilyHint {
    /// Lowercase name, stable for the CLI and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Document => "document",
            Self::Archive => "archive",
            Self::Font => "font",
            Self::Text => "text",
            Self::Unknown => "unknown",
        }
    }
}

/// What we learned about a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sniffed {
    /// MIME type, `application/octet-stream` when unknown.
    pub mime: String,
    /// Canonical extension without the dot, empty when unknown.
    pub ext: String,
    /// Coarse family.
    pub family_hint: FamilyHint,
    /// True when the type came from the bytes, false when from the file name.
    pub from_bytes: bool,
}

/// Sniff a file on disk. Reads at most 8 KiB. Unreadable files fall back to the name.
pub fn sniff(path: &Path) -> Sniffed {
    let mut head = Vec::with_capacity(HEAD_BYTES);
    if let Ok(f) = File::open(path) {
        let _ = f.take(HEAD_BYTES as u64).read_to_end(&mut head);
    }
    let by_ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    sniff_bytes(&head, by_ext.as_deref())
}

/// Sniff from a buffer, with an optional extension for the fallback.
pub fn sniff_bytes(head: &[u8], ext_hint: Option<&str>) -> Sniffed {
    if let Some(kind) = infer::get(head) {
        let mime = kind.mime_type();
        // ZIP-based containers share a signature. Trust the name to split docx from zip.
        let refined = refine_container(mime, ext_hint);
        let (mime, ext) = refined.unwrap_or((mime, kind.extension()));
        return Sniffed {
            mime: mime.to_string(),
            ext: ext.to_string(),
            family_hint: family_for_mime(mime),
            from_bytes: true,
        };
    }
    if let Some(ext) = ext_hint {
        let ext = ext.trim_start_matches('.').to_ascii_lowercase();
        if let Some(mime) = mime_for_ext(&ext) {
            return Sniffed {
                mime: mime.to_string(),
                family_hint: family_for_mime(mime),
                ext,
                from_bytes: false,
            };
        }
    }
    Sniffed {
        mime: "application/octet-stream".into(),
        ext: String::new(),
        family_hint: if looks_like_text(head) {
            FamilyHint::Text
        } else {
            FamilyHint::Unknown
        },
        from_bytes: false,
    }
}

/// A plain ZIP whose name says it is an office or ebook file is that file.
fn refine_container(mime: &str, ext: Option<&str>) -> Option<(&'static str, &'static str)> {
    if mime != "application/zip" {
        return None;
    }
    match ext? {
        "docx" => Some((
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
            "docx",
        )),
        "xlsx" => Some((
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
            "xlsx",
        )),
        "pptx" => Some((
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
            "pptx",
        )),
        "epub" => Some(("application/epub+zip", "epub")),
        _ => None,
    }
}

fn looks_like_text(head: &[u8]) -> bool {
    !head.is_empty() && std::str::from_utf8(head).is_ok() && !head.contains(&0)
}

/// Map a MIME type to a coarse family.
pub fn family_for_mime(mime: &str) -> FamilyHint {
    match mime {
        m if m.starts_with("image/") => FamilyHint::Image,
        m if m.starts_with("audio/") => FamilyHint::Audio,
        m if m.starts_with("video/") => FamilyHint::Video,
        m if m.starts_with("font/") || m.starts_with("application/font") => FamilyHint::Font,
        m if m.starts_with("text/") => FamilyHint::Text,
        "application/json" | "application/xml" | "application/toml" | "application/yaml" => {
            FamilyHint::Text
        }
        "application/pdf" | "application/rtf" | "application/epub+zip" => FamilyHint::Document,
        m if m.contains("officedocument") || m.contains("msword") || m.contains("opendocument") => {
            FamilyHint::Document
        }
        "application/zip"
        | "application/gzip"
        | "application/x-tar"
        | "application/x-7z-compressed"
        | "application/vnd.rar"
        | "application/x-bzip2"
        | "application/x-xz"
        | "application/zstd" => FamilyHint::Archive,
        _ => FamilyHint::Unknown,
    }
}

/// Extension to MIME for the common types the bytes cannot tell us (text formats mostly).
pub fn mime_for_ext(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "txt" | "log" => "text/plain",
        // Contact cards and Windows internet shortcuts are plain text with no magic bytes.
        "vcf" | "vcard" => "text/vcard",
        "url" => "text/plain",
        "md" | "markdown" => "text/markdown",
        "csv" => "text/csv",
        "tsv" => "text/tab-separated-values",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "srt" => "application/x-subrip",
        "vtt" => "text/vtt",
        "json" => "application/json",
        "xml" => "application/xml",
        "toml" => "application/toml",
        "yaml" | "yml" => "application/yaml",
        "svg" => "image/svg+xml",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "tif" | "tiff" => "image/tiff",
        "bmp" => "image/bmp",
        "mp3" => "audio/mpeg",
        "wav" => "audio/x-wav",
        "flac" => "audio/x-flac",
        "m4a" => "audio/m4a",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "pdf" => "application/pdf",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "epub" => "application/epub+zip",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D,
    ];

    #[test]
    fn bytes_beat_extension() {
        let s = sniff_bytes(PNG, Some("jpg"));
        assert_eq!(s.mime, "image/png");
        assert_eq!(s.ext, "png");
        assert_eq!(s.family_hint, FamilyHint::Image);
        assert!(s.from_bytes);
    }

    #[test]
    fn pdf_from_magic() {
        let s = sniff_bytes(b"%PDF-1.7\n%....", None);
        assert_eq!(s.mime, "application/pdf");
        assert_eq!(s.family_hint, FamilyHint::Document);
    }

    #[test]
    fn extension_fallback_for_text() {
        let s = sniff_bytes(b"1\n00:00:01,000 --> 00:00:02,000\nhi\n", Some("srt"));
        assert_eq!(s.ext, "srt");
        assert!(!s.from_bytes);
    }

    #[test]
    fn text_formats_without_magic_bytes_are_text_by_extension() {
        for (ext, head, mime) in [
            (
                "vcf",
                &b"BEGIN:VCARD\nVERSION:3.0\nFN:Asha Rao\nEND:VCARD\n"[..],
                "text/vcard",
            ),
            (
                "url",
                &b"[InternetShortcut]\nURL=https://example.com\n"[..],
                "text/plain",
            ),
            ("md", &b"# Title\n"[..], "text/markdown"),
            ("yaml", &b"a: 1\n"[..], "application/yaml"),
            ("yml", &b"a: 1\n"[..], "application/yaml"),
            ("toml", &b"a = 1\n"[..], "application/toml"),
        ] {
            let s = sniff_bytes(head, Some(ext));
            assert_eq!(s.mime, mime, ".{ext}");
            assert_eq!(s.ext, ext, ".{ext}");
            assert_eq!(s.family_hint, FamilyHint::Text, ".{ext}");
        }
    }

    #[test]
    fn unknown_binary_is_unknown() {
        let s = sniff_bytes(&[0, 1, 2, 3, 4, 5, 6, 7], None);
        assert_eq!(s.mime, "application/octet-stream");
        assert_eq!(s.family_hint, FamilyHint::Unknown);
    }

    #[test]
    fn plain_text_without_extension_is_text() {
        let s = sniff_bytes(b"hello world\n", None);
        assert_eq!(s.family_hint, FamilyHint::Text);
    }

    #[test]
    fn zip_named_docx_is_docx() {
        let s = sniff_bytes(&[b'P', b'K', 3, 4, 0, 0, 0, 0, 0, 0, 0, 0], Some("docx"));
        assert_eq!(s.ext, "docx");
        assert_eq!(s.family_hint, FamilyHint::Document);
    }

    #[test]
    fn plain_zip_stays_zip() {
        let s = sniff_bytes(&[b'P', b'K', 3, 4, 0, 0, 0, 0, 0, 0, 0, 0], Some("zip"));
        assert_eq!(s.ext, "zip");
        assert_eq!(s.family_hint, FamilyHint::Archive);
    }

    #[test]
    fn sniff_reads_a_real_file() {
        let path = std::env::temp_dir().join(format!("panoply-sniff-{}.jpg", std::process::id()));
        std::fs::write(&path, PNG).unwrap();
        let s = sniff(&path);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(s.mime, "image/png");
    }

    #[test]
    fn missing_file_falls_back_to_name() {
        let s = sniff(Path::new("/definitely/not/here.mp3"));
        assert_eq!(s.mime, "audio/mpeg");
        assert_eq!(s.family_hint, FamilyHint::Audio);
    }
}

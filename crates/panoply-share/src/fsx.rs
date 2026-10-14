//! File system rules for received files: names are sanitized, paths stay inside the chosen
//! folder, symlinks are never followed, and nothing is ever overwritten.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const MAX_COMPONENT_BYTES: usize = 200;

/// Make one path segment safe on every platform Panoply runs on.
///
/// Separators, control characters and the characters Windows forbids become `_`; trailing dots
/// and spaces go; reserved Windows device names get a prefix; the length is capped while
/// keeping the extension. The result is never empty and never `.` or `..`.
pub fn sanitize_component(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    s = s.trim_matches(|c| c == ' ' || c == '.').to_string();
    if s.is_empty() {
        return "file".into();
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        s.insert(0, '_');
    }
    if s.len() > MAX_COMPONENT_BYTES {
        let (stem, ext) = match s.rfind('.') {
            Some(i) if s.len() - i <= 16 && i > 0 => (&s[..i], &s[i..]),
            _ => (s.as_str(), ""),
        };
        let keep = MAX_COMPONENT_BYTES - ext.len();
        let mut cut = keep.min(stem.len());
        while !stem.is_char_boundary(cut) {
            cut -= 1;
        }
        s = format!("{}{}", &stem[..cut], ext);
    }
    s
}

/// Split a sender supplied file name into sanitized segments. Folder trees arrive as
/// `a/b/c.txt`. A `..` segment rejects the whole name; empty and `.` segments are dropped.
pub fn split_relative(file_name: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for seg in file_name.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => return Err(Error::Input("a file name contains '..'".into())),
            s => out.push(sanitize_component(s)),
        }
    }
    if out.is_empty() {
        return Err(Error::Input("a file has no name".into()));
    }
    Ok(out)
}

/// Create (if needed) each directory in `parts` below `root`, refusing symlinks and files in
/// the way. Returns the final directory.
pub fn ensure_dirs(root: &Path, parts: &[String]) -> io::Result<PathBuf> {
    fs::create_dir_all(root)?;
    let mut cur = root.to_path_buf();
    for p in parts {
        cur.push(p);
        match fs::symlink_metadata(&cur) {
            Ok(md) if md.file_type().is_dir() => {}
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "a file or link is in the way of a folder",
                ));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => fs::create_dir(&cur)?,
            Err(e) => return Err(e),
        }
    }
    Ok(cur)
}

/// An in-progress download: a hidden temporary file next to the final location.
#[derive(Debug)]
pub struct PartFile {
    /// Path of the temporary file.
    pub path: PathBuf,
    /// Directory the finished file will land in.
    pub dir: PathBuf,
    /// Wanted final name (already sanitized).
    pub name: String,
}

/// Open a fresh temporary file in `dir` for a file that will be called `name`.
pub fn create_part(dir: &Path, name: &str, token: &str) -> io::Result<(File, PartFile)> {
    let part_name = format!(
        ".{}.{}.panoply-part",
        truncate(name, 100),
        &token[..token.len().min(8)]
    );
    let path = dir.join(part_name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    Ok((
        file,
        PartFile {
            path,
            dir: dir.to_path_buf(),
            name: name.to_string(),
        },
    ))
}

fn truncate(s: &str, max: usize) -> &str {
    let mut cut = s.len().min(max);
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    &s[..cut]
}

/// `name`, then `stem (1).ext`, `stem (2).ext`, and so on.
pub fn numbered(name: &str, n: u32) -> String {
    if n == 0 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
        _ => format!("{name} ({n})"),
    }
}

impl PartFile {
    /// Move the finished file to its final name without ever replacing an existing file.
    ///
    /// Uses a hard link (which fails when the target exists) and falls back to an existence
    /// check plus rename on file systems without hard links.
    pub fn finalize(self) -> io::Result<PathBuf> {
        for n in 0..10_000u32 {
            let candidate = self.dir.join(numbered(&self.name, n));
            match fs::hard_link(&self.path, &candidate) {
                Ok(()) => {
                    let _ = fs::remove_file(&self.path);
                    return Ok(candidate);
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(_) => {
                    if fs::symlink_metadata(&candidate).is_ok() {
                        continue;
                    }
                    fs::rename(&self.path, &candidate)?;
                    return Ok(candidate);
                }
            }
        }
        let _ = fs::remove_file(&self.path);
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not find a free file name",
        ))
    }

    /// Delete the temporary file (cancelled or failed transfer).
    pub fn discard(&self) {
        let _ = fs::remove_file(&self.path);
    }
}

/// Free bytes on the volume that holds `dir` (the nearest existing ancestor).
pub fn free_space(dir: &Path) -> Option<u64> {
    let mut p = dir;
    loop {
        if p.exists() {
            return fs4::available_space(p).ok();
        }
        p = p.parent()?;
    }
}

/// A guess at the MIME type from the extension. Receivers treat it as a hint only.
pub fn mime_for(path: &Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "heic" => "image/heic",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        "txt" | "md" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "7z" => "application/x-7z-compressed",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "wav" => "audio/wav",
        "flac" => "audio/flac",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "avi" => "video/x-msvideo",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "apk" => "application/vnd.android.package-archive",
        _ => "application/octet-stream",
    }
}

/// Extensions that run code when opened. Used to warn about files from untrusted senders.
pub fn looks_executable(name: &str) -> bool {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(
        ext.as_str(),
        "exe"
            | "msi"
            | "bat"
            | "cmd"
            | "com"
            | "scr"
            | "ps1"
            | "vbs"
            | "js"
            | "jar"
            | "app"
            | "command"
            | "sh"
            | "pkg"
            | "dmg"
            | "apk"
            | "appimage"
            | "lnk"
            | "reg"
            | "dll"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_hostile_names() {
        assert_eq!(sanitize_component("a/b"), "a_b");
        assert_eq!(sanitize_component("..."), "file");
        assert_eq!(sanitize_component("con.txt"), "_con.txt");
        assert_eq!(sanitize_component("what?.png"), "what_.png");
        assert_eq!(sanitize_component("tab\tname"), "tab_name");
        let long = format!("{}.jpg", "é".repeat(300));
        let s = sanitize_component(&long);
        assert!(s.len() <= 200 && s.ends_with(".jpg"));
    }

    #[test]
    fn splits_trees_and_rejects_traversal() {
        assert_eq!(split_relative("a/b/c.txt").unwrap(), ["a", "b", "c.txt"]);
        assert_eq!(split_relative("/abs/x").unwrap(), ["abs", "x"]);
        assert_eq!(split_relative("a\\b").unwrap(), ["a", "b"]);
        assert!(split_relative("../x").is_err());
        assert!(split_relative("a/../../x").is_err());
        assert!(split_relative("/").is_err());
    }

    #[test]
    fn numbering() {
        assert_eq!(numbered("a.txt", 0), "a.txt");
        assert_eq!(numbered("a.txt", 2), "a (2).txt");
        assert_eq!(numbered("noext", 1), "noext (1)");
        assert_eq!(numbered(".hidden", 1), ".hidden (1)");
    }
}

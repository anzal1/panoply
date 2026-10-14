//! Copying text to the system clipboard through the platform's own command, so the crate needs
//! no clipboard library.

use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{OcrError, Result};

/// Put `text` on the clipboard: `pbcopy` on macOS, `clip` on Windows, `wl-copy` or `xclip` on
/// Linux.
pub fn copy(text: &str) -> Result<()> {
    let attempts: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip", &[])]
    } else {
        &[("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])]
    };
    for (prog, args) in attempts {
        let Ok(mut child) = Command::new(prog)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        // `clip` reads the console code page unless given UTF-16 with a byte order mark.
        let ok = if cfg!(windows) {
            let mut bytes = vec![0xFF, 0xFE];
            bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
            child
                .stdin
                .take()
                .is_some_and(|mut s| s.write_all(&bytes).is_ok())
        } else {
            child
                .stdin
                .take()
                .is_some_and(|mut s| s.write_all(text.as_bytes()).is_ok())
        };
        let status = child.wait();
        if ok && status.map(|s| s.success()).unwrap_or(false) {
            return Ok(());
        }
    }
    Err(OcrError::Unsupported(
        "Panoply could not reach the clipboard on this system. The text was not copied.".into(),
    ))
}

//! Downloading a pinned file once, with a checksum, and reading members out of a zip.
//!
//! The download uses the system `curl` (it ships with Windows 10 and later, macOS and mainstream
//! Linux), over https only, into a temporary name. The file is checked against its pinned SHA-256
//! before it is used and renamed into place, so another process never sees half a file and a
//! file that fails its check is never kept.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use panoply_image::error::{ImageError, Result};
use sha2::{Digest, Sha256};

use crate::VisionIo;

/// A plain "could not get it" error with a next step.
pub(crate) fn engine_err(msg: impl Into<String>) -> ImageError {
    ImageError::Unsupported(msg.into())
}

/// SHA-256 of a file as lower case hex.
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// SHA-256 of bytes as lower case hex.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// "42 MB" style size for labels shown before a download (decimal, like file managers).
pub fn mb(bytes: u64) -> String {
    if bytes >= 10_000_000 {
        format!("{:.0} MB", bytes as f64 / 1e6)
    } else {
        format!("{:.1} MB", bytes as f64 / 1e6)
    }
}

/// The host name of an https URL, for the "downloaded from" line on the result card.
pub fn host_of(url: &str) -> &str {
    url.trim_start_matches("https://")
        .split('/')
        .next()
        .unwrap_or(url)
}

/// Download `url` to `dest` with curl. Watches the file grow so progress and cancel work.
/// `size` is the pinned size in bytes, used for the progress fraction only.
pub fn download(url: &str, dest: &Path, size: u64, io: &dyn VisionIo, label: &str) -> Result<()> {
    let mut child = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--max-time",
            "1800",
            "--output",
        ])
        .arg(dest)
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| {
            engine_err(format!(
                "Panoply needs to download a file for this and could not start curl ({e}). Install curl and try again."
            ))
        })?;
    loop {
        if io.cancel().is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_file(dest);
            return Err(ImageError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return Ok(()),
            Ok(Some(_)) => {
                let _ = std::fs::remove_file(dest);
                return Err(engine_err(format!(
                    "Panoply could not download {label} from {}. Check your connection and try again.",
                    host_of(url)
                )));
            }
            Ok(None) => {}
            Err(e) => {
                return Err(engine_err(format!(
                    "The download stopped unexpectedly ({e}). Try again."
                )));
            }
        }
        if let Ok(m) = std::fs::metadata(dest) {
            io.progress(
                (m.len() as f64 / size.max(1) as f64).min(0.99) as f32,
                label,
            );
        }
        std::thread::sleep(Duration::from_millis(150));
    }
}

/// Read one member out of a zip file, stored or deflated. Zip64 and encrypted archives are not
/// supported (the pinned archives are neither).
pub fn unzip_member(zip: &Path, member: &str) -> Result<Vec<u8>> {
    let bytes = std::fs::read(zip)
        .map_err(|e| engine_err(format!("Could not read the download ({e}).")))?;
    zip_member(&bytes, member)
}

fn u16_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as usize)
}

fn u32_at(b: &[u8], o: usize) -> Option<usize> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as usize)
}

/// Read one member out of zip bytes.
pub fn zip_member(b: &[u8], member: &str) -> Result<Vec<u8>> {
    let bad = || engine_err("The downloaded archive is not a readable zip file.");
    // The end of central directory record is in the last 64 KB + 22 bytes.
    let tail = b.len().saturating_sub(65_557);
    let eocd = (tail..b.len().saturating_sub(21))
        .rev()
        .find(|&i| b[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(bad)?;
    let entries = u16_at(b, eocd + 10).ok_or_else(bad)?;
    let mut off = u32_at(b, eocd + 16).ok_or_else(bad)?;
    for _ in 0..entries {
        if b.get(off..off + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            return Err(bad());
        }
        let method = u16_at(b, off + 10).ok_or_else(bad)?;
        let csize = u32_at(b, off + 20).ok_or_else(bad)?;
        let usize_ = u32_at(b, off + 24).ok_or_else(bad)?;
        let nlen = u16_at(b, off + 28).ok_or_else(bad)?;
        let elen = u16_at(b, off + 30).ok_or_else(bad)?;
        let clen = u16_at(b, off + 32).ok_or_else(bad)?;
        let local = u32_at(b, off + 42).ok_or_else(bad)?;
        let name = b.get(off + 46..off + 46 + nlen).ok_or_else(bad)?;
        if name == member.as_bytes() {
            if b.get(local..local + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
                return Err(bad());
            }
            let lname = u16_at(b, local + 26).ok_or_else(bad)?;
            let lextra = u16_at(b, local + 28).ok_or_else(bad)?;
            let start = local + 30 + lname + lextra;
            let raw = b.get(start..start + csize).ok_or_else(bad)?;
            let out = match method {
                0 => raw.to_vec(),
                8 => miniz_oxide::inflate::decompress_to_vec(raw)
                    .map_err(|_| engine_err("The downloaded archive is damaged."))?,
                _ => {
                    return Err(engine_err(
                        "The downloaded archive uses a compression this build cannot read.",
                    ));
                }
            };
            if out.len() != usize_ {
                return Err(engine_err("The downloaded archive is damaged."));
            }
            return Ok(out);
        }
        off += 46 + nlen + elen + clen;
    }
    Err(engine_err(format!(
        "The downloaded archive does not contain {member}."
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny zip with one stored member and one deflated member, built by hand.
    fn tiny_zip() -> Vec<u8> {
        fn entry(
            out: &mut Vec<u8>,
            cd: &mut Vec<u8>,
            name: &str,
            method: u16,
            raw: &[u8],
            plain: usize,
        ) {
            let off = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04, 20, 0, 0, 0]);
            out.extend_from_slice(&method.to_le_bytes());
            out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
            out.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            out.extend_from_slice(&(plain as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&[0, 0]);
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(raw);
            cd.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02, 20, 0, 20, 0, 0, 0]);
            cd.extend_from_slice(&method.to_le_bytes());
            cd.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0]);
            cd.extend_from_slice(&(raw.len() as u32).to_le_bytes());
            cd.extend_from_slice(&(plain as u32).to_le_bytes());
            cd.extend_from_slice(&(name.len() as u16).to_le_bytes());
            cd.extend_from_slice(&[0; 12]);
            cd.extend_from_slice(&off.to_le_bytes());
            cd.extend_from_slice(name.as_bytes());
        }
        let plain = b"hello hello hello hello hello hello".to_vec();
        let deflated = miniz_oxide::deflate::compress_to_vec(&plain, 6);
        let mut out = Vec::new();
        let mut cd = Vec::new();
        entry(&mut out, &mut cd, "a/stored.txt", 0, b"stored!", 7);
        entry(
            &mut out,
            &mut cd,
            "a/deflated.txt",
            8,
            &deflated,
            plain.len(),
        );
        let cd_off = out.len() as u32;
        out.extend_from_slice(&cd);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06, 0, 0, 0, 0, 2, 0, 2, 0]);
        out.extend_from_slice(&(cd.len() as u32).to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&[0, 0]);
        out
    }

    #[test]
    fn reads_stored_and_deflated_members() {
        let z = tiny_zip();
        assert_eq!(zip_member(&z, "a/stored.txt").unwrap(), b"stored!");
        assert_eq!(
            zip_member(&z, "a/deflated.txt").unwrap(),
            b"hello hello hello hello hello hello"
        );
        assert!(zip_member(&z, "a/missing.txt").is_err());
        assert!(zip_member(b"not a zip at all", "x").is_err());
    }

    #[test]
    fn digests_and_labels() {
        assert_eq!(
            sha256_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(mb(44_229_662), "44 MB");
        assert_eq!(mb(4_540_895), "4.5 MB");
        assert_eq!(host_of("https://huggingface.co/a/b"), "huggingface.co");
    }
}

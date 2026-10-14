//! Fetching the pinned helper build once, with a checksum.
//!
//! The download uses the system `curl` and `tar` (both ship with Windows 10 and later, macOS and
//! mainstream Linux), over https only. The archive is checked against the SHA-256 in
//! [`super::pins`] before anything is unpacked, and unpacked into a temporary folder that is
//! renamed into place, so another process never sees half an engine.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::pins::{self, Packing, Pin};
use super::{engines_dir, exe};
use crate::error::{MediaError, Result};
use crate::io::Io;

fn engine_err(msg: impl Into<String>) -> MediaError {
    MediaError::Engine(msg.into())
}

/// The ffmpeg and ffprobe inside an installed engine folder.
fn programs(dir: &Path) -> (PathBuf, PathBuf) {
    (
        dir.join("bin").join(exe("ffmpeg")),
        dir.join("bin").join(exe("ffprobe")),
    )
}

/// Make sure the pinned build is on disk. `Ok(None)` means there is no pin for this platform.
pub(super) fn ensure_pinned(io: &dyn Io) -> Result<Option<(PathBuf, PathBuf, Option<PathBuf>)>> {
    let Some(pin) = pins::for_this_platform() else {
        return Ok(None);
    };
    let dir = engines_dir().join(pin.tag);
    let (ffmpeg, ffprobe) = programs(&dir);
    if ffmpeg.is_file() && ffprobe.is_file() {
        return Ok(Some((ffmpeg, ffprobe, None)));
    }
    if !io.network_allowed() {
        return Err(engine_err(format!(
            "Panoply needs to download its video engine ({:.0} MB, once) and downloads are not allowed for this job. Allow it, or set PANOPLY_FFMPEG to an ffmpeg program.",
            pin.size as f64 / 1e6
        )));
    }
    install(pin, &dir, io)?;
    Ok(Some((ffmpeg, ffprobe, None)))
}

fn install(pin: &Pin, dir: &Path, io: &dyn Io) -> Result<()> {
    let parent = dir
        .parent()
        .ok_or_else(|| engine_err("The engines folder has no parent folder."))?;
    std::fs::create_dir_all(parent)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", parent.display())))?;
    let tmp = parent.join(format!(".part-{}-{}", pin.tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp)
        .map_err(|e| engine_err(format!("Could not create {}: {e}", tmp.display())))?;
    let result = (|| {
        let archive = tmp.join("engine.archive");
        // Say so before touching the network (FLOWS.md: network access is labelled first).
        let label = format!(
            "Downloading the video engine ({:.0} MB, once)",
            pin.size as f64 / 1e6
        );
        io.progress(0.0, &label);
        download(pin.url, &archive, pin.size, io, &label)?;
        install_archive(&archive, pin.packing, pin.sha256, &tmp, dir)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    io.note(format!(
        "Downloaded the video engine ({:.0} MB) from github.com. This happens once.",
        pin.size as f64 / 1e6
    ));
    Ok(())
}

/// Download with `curl`, watching the file grow so progress and cancel work.
fn download(url: &str, dest: &Path, size: u64, io: &dyn Io, label: &str) -> Result<()> {
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
                "Panoply needs its video engine for this and could not start curl to download it ({e}). Install curl, or set PANOPLY_FFMPEG to an ffmpeg program."
            ))
        })?;
    loop {
        if io.cancel().is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(MediaError::Cancelled);
        }
        match child.try_wait() {
            Ok(Some(st)) if st.success() => return Ok(()),
            Ok(Some(_)) => {
                return Err(engine_err(
                    "Panoply needs its video engine for this and could not download it. Check your connection and try again, or set PANOPLY_FFMPEG to an ffmpeg program.",
                ));
            }
            Ok(None) => {}
            Err(e) => {
                return Err(engine_err(format!(
                    "The download stopped unexpectedly ({e})."
                )));
            }
        }
        if let Ok(m) = std::fs::metadata(dest) {
            io.progress(
                (m.len() as f64 / size.max(1) as f64).min(0.99) as f32,
                label,
            );
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

pub(super) fn sha256_file(path: &Path) -> std::io::Result<String> {
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
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Check, unpack and install an archive. Split from the download so it can be tested on a local
/// file.
pub(super) fn install_archive(
    archive: &Path,
    packing: Packing,
    sha256: &str,
    tmp: &Path,
    dest: &Path,
) -> Result<()> {
    let got = sha256_file(archive)
        .map_err(|e| engine_err(format!("Could not read the download: {e}")))?;
    if got != sha256 {
        return Err(engine_err(format!(
            "The downloaded video engine did not match its pinned checksum (expected {sha256}, got {got}). It was not used."
        )));
    }
    let x = tmp.join("x");
    std::fs::create_dir_all(&x)
        .map_err(|e| engine_err(format!("Could not create a folder: {e}")))?;
    let flag = match packing {
        Packing::Zip => "-xf",
        Packing::TarXz => "-xJf",
    };
    let status = Command::new("tar")
        .arg(flag)
        .arg(archive)
        .arg("-C")
        .arg(&x)
        .stdin(Stdio::null())
        .status()
        .map_err(|e| {
            engine_err(format!(
                "Could not start tar to unpack the video engine ({e})."
            ))
        })?;
    if !status.success() {
        return Err(engine_err("Could not unpack the video engine."));
    }
    // The archive holds one top folder: <name>/bin/ffmpeg.
    let top = std::fs::read_dir(&x)
        .map_err(|e| engine_err(format!("Could not read the unpacked engine: {e}")))?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join("bin").join(exe("ffmpeg")).is_file())
        .ok_or_else(|| engine_err("The engine archive did not contain ffmpeg."))?;
    if !top.join("bin").join(exe("ffprobe")).is_file() {
        return Err(engine_err("The engine archive did not contain ffprobe."));
    }
    if dest.exists() {
        let _ = std::fs::remove_dir_all(dest);
    }
    // Rename is atomic on one volume, so another process never sees half an engine.
    std::fs::rename(&top, dest)
        .map_err(|e| engine_err(format!("Could not install the video engine: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn scratch(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("panoply-media-fetch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn every_pin_is_https_with_a_full_checksum() {
        for p in pins::FFMPEG {
            assert!(p.url.starts_with("https://"), "{}", p.tag);
            assert_eq!(p.sha256.len(), 64, "{}", p.tag);
            assert!(p.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
            assert!(p.size > 10_000_000);
            assert!(
                p.tag.contains("lgpl"),
                "only LGPL builds are pinned: {}",
                p.tag
            );
            assert!(!p.url.contains("-gpl"), "{}", p.url);
        }
        for (os, arch) in [
            ("windows", "x86_64"),
            ("windows", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
        ] {
            assert!(
                pins::FFMPEG.iter().any(|p| p.os == os && p.arch == arch),
                "{os} {arch}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_checked_archive_installs_and_a_wrong_checksum_does_not() {
        let d = scratch("install");
        let pack = d.join("pack/ffmpeg-test/bin");
        std::fs::create_dir_all(&pack).unwrap();
        std::fs::write(pack.join("ffmpeg"), "#!/bin/sh\n").unwrap();
        std::fs::write(pack.join("ffprobe"), "#!/bin/sh\n").unwrap();
        let archive = d.join("e.tar.xz");
        let ok = Command::new("tar")
            .arg("-cJf")
            .arg(&archive)
            .arg("-C")
            .arg(d.join("pack"))
            .arg("ffmpeg-test")
            .status();
        if ok.map(|s| !s.success()).unwrap_or(true) {
            // No xz here: nothing to test on this machine.
            std::fs::remove_dir_all(&d).unwrap();
            return;
        }
        let sum = sha256_file(&archive).unwrap();
        let tmp = d.join("tmp");
        std::fs::create_dir_all(&tmp).unwrap();
        let wrong = "0".repeat(64);
        let err =
            install_archive(&archive, Packing::TarXz, &wrong, &tmp, &d.join("dest")).unwrap_err();
        assert!(err.to_string().contains("did not match"), "{err}");
        assert!(!d.join("dest").exists());
        install_archive(&archive, Packing::TarXz, &sum, &tmp, &d.join("dest")).unwrap();
        assert!(d.join("dest/bin/ffmpeg").is_file());
        std::fs::remove_dir_all(&d).unwrap();
    }
}

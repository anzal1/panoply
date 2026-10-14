//! The pinned helper builds.
//!
//! Windows and Linux: BtbN's FFmpeg-Builds `lgpl` variant (LGPLv3, built with
//! `--disable-gpl --disable-nonfree`), the month-end tag `autobuild-2026-09-30-13-08`. BtbN keeps
//! the last build of each month for two years (its README, "Release Retention Policy"), so the
//! URL outlives the daily builds, which are pruned after 14 days. The archive digests were read
//! from the release's own `checksums.sha256` and match the `digest` field of the GitHub release
//! API. Because that retention is a README promise and not a contract, the four archives should
//! also be mirrored into a Panoply release (see `crates/panoply-media/engine/README.md`).
//!
//! macOS: there is no credible prebuilt LGPL ffmpeg for macOS (evermeet, osxexperts,
//! martin-riedl and Homebrew are all `--enable-gpl`). The build recipe and workflow are in
//! `crates/panoply-media/engine/` and publish a Panoply release; until its two archives are
//! pinned here the macOS engine is an ffmpeg and ffprobe found on `PATH` (or `PANOPLY_FFMPEG`).

/// How the archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Packing {
    /// A zip file (Windows). Unpacked with the system `tar`, which reads zip on Windows 10 and later.
    Zip,
    /// A tar file compressed with xz (Linux).
    TarXz,
}

/// One prebuilt helper archive.
pub struct Pin {
    /// `macos`, `linux` or `windows` (`std::env::consts::OS`).
    pub os: &'static str,
    /// `x86_64` or `aarch64` (`std::env::consts::ARCH`).
    pub arch: &'static str,
    /// Cache folder name.
    pub tag: &'static str,
    /// Download address (https only).
    pub url: &'static str,
    /// SHA-256 of the archive.
    pub sha256: &'static str,
    /// Download size in bytes, for the label shown before the download.
    pub size: u64,
    /// How the archive is packed.
    pub packing: Packing,
}

/// Archives for every platform that has one.
pub const FFMPEG: &[Pin] = &[
    Pin {
        os: "windows",
        arch: "x86_64",
        tag: "ffmpeg-n9.0.2-win64-lgpl",
        url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n9.0.2-17-g2a571b6068-win64-lgpl-9.0.zip",
        sha256: "6b264b9e6019103f601d98c292bd332fd87acf1c5e941ddff4fb71760fe63432",
        size: 171_535_354,
        packing: Packing::Zip,
    },
    Pin {
        os: "windows",
        arch: "aarch64",
        tag: "ffmpeg-n9.0.2-winarm64-lgpl",
        url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n9.0.2-17-g2a571b6068-winarm64-lgpl-9.0.zip",
        sha256: "d189d06c804b01e60e9677901f410f85900682f6a59f476b933a7fa48176b0c6",
        size: 117_605_161,
        packing: Packing::Zip,
    },
    Pin {
        os: "linux",
        arch: "x86_64",
        tag: "ffmpeg-n9.0.2-linux64-lgpl",
        url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n9.0.2-17-g2a571b6068-linux64-lgpl-9.0.tar.xz",
        sha256: "2d41cbea0ca1a15029b638330740f78d6f5aeb062355bf425433fc938705350a",
        size: 137_863_452,
        packing: Packing::TarXz,
    },
    Pin {
        os: "linux",
        arch: "aarch64",
        tag: "ffmpeg-n9.0.2-linuxarm64-lgpl",
        url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/autobuild-2026-09-30-13-08/ffmpeg-n9.0.2-17-g2a571b6068-linuxarm64-lgpl-9.0.tar.xz",
        sha256: "b8cc09200780d37801179c803f597c22440b0b06c556addd2e6a07b27475ce37",
        size: 116_918_620,
        packing: Packing::TarXz,
    },
];

/// The pin for this platform, if there is one.
pub fn for_this_platform() -> Option<&'static Pin> {
    FFMPEG
        .iter()
        .find(|p| p.os == std::env::consts::OS && p.arch == std::env::consts::ARCH)
}

//! The encoder ladder (spec 0.1, "HWL"): the OS hardware encoder first, then a permissive
//! software encoder, and the result card says which one ran.
//!
//! macOS: VideoToolbox. Windows: Media Foundation (forced to hardware), then NVENC, QSV and AMF
//! when present. Linux: VA-API, then NVENC, QSV and AMF. These are ffmpeg's wrappers over the OS
//! and vendor APIs, allowed in an LGPL build. Software: SVT-AV1 (BSD) for AV1, libvpx (BSD) for
//! VP9. x264 and x265 are GPL and never requested (ADR 0002). OpenH264 (BSD) is the H.264
//! software fallback only in an ffmpeg the user supplies: BtbN's pinned build contains a
//! self-built OpenH264, which does not carry Cisco's patent grant (that applies only to Cisco's
//! own binary, downloaded by the end user), so Panoply does not use it.

use crate::engine::{Engine, Source};
use crate::error::{MediaError, Result};

/// A video codec Panoply can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VCodec {
    /// H.264 / AVC.
    H264,
    /// H.265 / HEVC. Hardware only (x265 is GPL).
    Hevc,
    /// AV1.
    Av1,
    /// VP9.
    Vp9,
}

impl VCodec {
    /// Plain name.
    pub fn plain(self) -> &'static str {
        match self {
            VCodec::H264 => "H.264",
            VCodec::Hevc => "HEVC",
            VCodec::Av1 => "AV1",
            VCodec::Vp9 => "VP9",
        }
    }

    /// ffmpeg's decoder-side name.
    pub fn name(self) -> &'static str {
        match self {
            VCodec::H264 => "h264",
            VCodec::Hevc => "hevc",
            VCodec::Av1 => "av1",
            VCodec::Vp9 => "vp9",
        }
    }

    /// The codec a probe name stands for, when Panoply can write it.
    pub fn from_name(n: &str) -> Option<VCodec> {
        match n {
            "h264" => Some(VCodec::H264),
            "hevc" | "h265" => Some(VCodec::Hevc),
            "av1" => Some(VCodec::Av1),
            "vp9" => Some(VCodec::Vp9),
            _ => None,
        }
    }
}

/// Which acceleration an encoder uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accel {
    /// Apple VideoToolbox.
    VideoToolbox,
    /// Windows Media Foundation.
    MediaFoundation,
    /// NVIDIA NVENC.
    Nvenc,
    /// Intel Quick Sync.
    Qsv,
    /// AMD AMF.
    Amf,
    /// VA-API.
    Vaapi,
    /// Software.
    Software,
}

impl Accel {
    /// True for anything that runs on a GPU or media block.
    pub fn is_hardware(self) -> bool {
        !matches!(self, Accel::Software)
    }

    /// Plain name.
    pub fn plain(self) -> &'static str {
        match self {
            Accel::VideoToolbox => "VideoToolbox",
            Accel::MediaFoundation => "Media Foundation",
            Accel::Nvenc => "NVENC",
            Accel::Qsv => "Quick Sync",
            Accel::Amf => "AMD AMF",
            Accel::Vaapi => "VA-API",
            Accel::Software => "software",
        }
    }
}

/// How the encoder is told to spend bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Rate {
    /// Average bits per second.
    Bitrate(u64),
    /// A quality level from 1 (smallest) to 100 (best).
    Quality(u8),
}

/// A video encoder that can be used on this machine.
#[derive(Debug, Clone)]
pub struct VEnc {
    /// ffmpeg's encoder name.
    pub name: &'static str,
    /// The codec it writes.
    pub codec: VCodec,
    /// The acceleration it uses.
    pub accel: Accel,
    /// Arguments that go before the input (a device, for VA-API).
    pub pre: Vec<String>,
    /// A filter appended at the end of the video chain (upload to the GPU, for VA-API).
    pub post_filter: Option<String>,
}

impl VEnc {
    /// "H.264 on the VideoToolbox hardware encoder".
    pub fn label(&self) -> String {
        if self.accel.is_hardware() {
            format!(
                "{} on the {} hardware encoder",
                self.codec.plain(),
                self.accel.plain()
            )
        } else {
            format!("{} in software ({})", self.codec.plain(), self.name)
        }
    }

    /// The pixel format the encoder wants for this depth.
    pub fn pix_fmt(&self, ten_bit: bool) -> &'static str {
        match (self.accel, ten_bit) {
            (Accel::Vaapi, false) => "nv12",
            (Accel::Vaapi, true) => "p010le",
            (Accel::Nvenc | Accel::VideoToolbox | Accel::Amf | Accel::Qsv, true) => "p010le",
            (Accel::Nvenc | Accel::Qsv | Accel::Amf | Accel::MediaFoundation, false) => "nv12",
            (_, true) => "yuv420p10le",
            (_, false) => "yuv420p",
        }
    }

    /// The output-side encoder arguments.
    pub fn args(&self, rate: Rate, ten_bit: bool, fps: f64) -> Vec<String> {
        let mut a: Vec<String> = vec!["-c:v".into(), self.name.into()];
        let push = |a: &mut Vec<String>, k: &str, v: &str| {
            a.push(k.into());
            a.push(v.into());
        };
        let bps = match rate {
            Rate::Bitrate(b) => Some(b),
            Rate::Quality(_) => None,
        };
        let q = match rate {
            Rate::Quality(q) => Some(q.clamp(1, 100)),
            Rate::Bitrate(_) => None,
        };
        // The distance between forced keyframes: two seconds, a normal live and web default.
        let gop = ((fps.max(1.0) * 2.0).round() as u32).max(2).to_string();
        match (self.accel, self.codec) {
            (Accel::VideoToolbox, c) => {
                match (bps, q) {
                    (Some(b), _) => push(&mut a, "-b:v", &b.to_string()),
                    (_, Some(q)) => push(&mut a, "-q:v", &q.to_string()),
                    _ => {}
                }
                if c == VCodec::H264 {
                    push(&mut a, "-profile:v", "high");
                } else if ten_bit {
                    push(&mut a, "-profile:v", "main10");
                } else {
                    push(&mut a, "-profile:v", "main");
                }
                push(&mut a, "-allow_sw", "0");
                push(&mut a, "-g", &gop);
            }
            (Accel::MediaFoundation, _) => {
                push(&mut a, "-hw_encoding", "1");
                match (bps, q) {
                    (Some(b), _) => {
                        push(&mut a, "-rate_control", "u_vbr");
                        push(&mut a, "-b:v", &b.to_string());
                    }
                    (_, Some(q)) => {
                        push(&mut a, "-rate_control", "quality");
                        push(&mut a, "-quality", &q.to_string());
                    }
                    _ => {}
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Nvenc, c) => {
                push(&mut a, "-preset", "p5");
                push(&mut a, "-tune", "hq");
                match (bps, q) {
                    (Some(b), _) => {
                        push(&mut a, "-rc", "vbr");
                        push(&mut a, "-b:v", &b.to_string());
                        push(&mut a, "-maxrate", &(b * 3 / 2).to_string());
                    }
                    (_, Some(q)) => {
                        push(&mut a, "-rc", "vbr");
                        push(&mut a, "-cq", &crf(c, q).to_string());
                        push(&mut a, "-b:v", "0");
                    }
                    _ => {}
                }
                if ten_bit && c == VCodec::Hevc {
                    push(&mut a, "-profile:v", "main10");
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Qsv, c) => {
                match (bps, q) {
                    (Some(b), _) => push(&mut a, "-b:v", &b.to_string()),
                    (_, Some(q)) => push(&mut a, "-global_quality", &crf(c, q).to_string()),
                    _ => {}
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Amf, c) => {
                push(&mut a, "-quality", "balanced");
                match (bps, q) {
                    (Some(b), _) => {
                        push(&mut a, "-rc", "vbr_peak");
                        push(&mut a, "-b:v", &b.to_string());
                    }
                    (_, Some(q)) => {
                        push(&mut a, "-rc", "cqp");
                        let qp = crf(c, q).to_string();
                        push(&mut a, "-qp_i", &qp);
                        push(&mut a, "-qp_p", &qp);
                    }
                    _ => {}
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Vaapi, c) => {
                match (bps, q) {
                    (Some(b), _) => {
                        push(&mut a, "-b:v", &b.to_string());
                        push(&mut a, "-maxrate", &(b * 3 / 2).to_string());
                    }
                    (_, Some(q)) => {
                        push(&mut a, "-rc_mode", "CQP");
                        push(&mut a, "-qp", &crf(c, q).to_string());
                    }
                    _ => {}
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Software, VCodec::Av1) => {
                push(&mut a, "-preset", "8");
                match (bps, q) {
                    (Some(b), _) => push(&mut a, "-b:v", &b.to_string()),
                    (_, Some(q)) => push(&mut a, "-crf", &crf(VCodec::Av1, q).to_string()),
                    _ => {}
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Software, VCodec::Vp9) => {
                push(&mut a, "-deadline", "good");
                push(&mut a, "-cpu-used", "4");
                push(&mut a, "-row-mt", "1");
                match (bps, q) {
                    (Some(b), _) => push(&mut a, "-b:v", &b.to_string()),
                    (_, Some(q)) => {
                        push(&mut a, "-crf", &crf(VCodec::Vp9, q).to_string());
                        push(&mut a, "-b:v", "0");
                    }
                    _ => {}
                }
                push(&mut a, "-g", &gop);
            }
            (Accel::Software, _) => {
                // OpenH264 and friends: bitrate only.
                let b = bps.unwrap_or(4_000_000);
                push(&mut a, "-b:v", &b.to_string());
                push(&mut a, "-g", &gop);
            }
        }
        push(&mut a, "-pix_fmt", self.pix_fmt(ten_bit));
        if self.codec == VCodec::Hevc {
            push(&mut a, "-tag:v", "hvc1");
        }
        a
    }
}

/// A constant-quality number on the codec's own scale from a 1 to 100 quality level.
fn crf(codec: VCodec, q: u8) -> u32 {
    let base = 15.0 + (100.0 - f64::from(q)) * 0.3;
    let shift = match codec {
        VCodec::H264 => 0.0,
        VCodec::Hevc => 4.0,
        VCodec::Vp9 => 6.0,
        VCodec::Av1 => 8.0,
    };
    (base + shift).round().max(1.0) as u32
}

fn vaapi_device() -> Option<String> {
    (128..140)
        .map(|n| format!("/dev/dri/renderD{n}"))
        .find(|p| std::path::Path::new(p).exists())
}

fn candidates(codec: VCodec) -> Vec<(&'static str, Accel)> {
    let os = std::env::consts::OS;
    let n = |c: VCodec, suffix: &'static str| -> &'static str {
        match (c, suffix) {
            (VCodec::H264, "videotoolbox") => "h264_videotoolbox",
            (VCodec::Hevc, "videotoolbox") => "hevc_videotoolbox",
            (VCodec::H264, "mf") => "h264_mf",
            (VCodec::Hevc, "mf") => "hevc_mf",
            (VCodec::Av1, "mf") => "av1_mf",
            (VCodec::H264, "nvenc") => "h264_nvenc",
            (VCodec::Hevc, "nvenc") => "hevc_nvenc",
            (VCodec::Av1, "nvenc") => "av1_nvenc",
            (VCodec::H264, "qsv") => "h264_qsv",
            (VCodec::Hevc, "qsv") => "hevc_qsv",
            (VCodec::Av1, "qsv") => "av1_qsv",
            (VCodec::H264, "amf") => "h264_amf",
            (VCodec::Hevc, "amf") => "hevc_amf",
            (VCodec::Av1, "amf") => "av1_amf",
            (VCodec::H264, "vaapi") => "h264_vaapi",
            (VCodec::Hevc, "vaapi") => "hevc_vaapi",
            (VCodec::Av1, "vaapi") => "av1_vaapi",
            (VCodec::Vp9, "vaapi") => "vp9_vaapi",
            _ => "",
        }
    };
    let mut v: Vec<(&'static str, Accel)> = Vec::new();
    match os {
        "macos" => {
            if matches!(codec, VCodec::H264 | VCodec::Hevc) {
                v.push((n(codec, "videotoolbox"), Accel::VideoToolbox));
            }
        }
        "windows" => {
            for (s, a) in [
                ("mf", Accel::MediaFoundation),
                ("nvenc", Accel::Nvenc),
                ("qsv", Accel::Qsv),
                ("amf", Accel::Amf),
            ] {
                let name = n(codec, s);
                if !name.is_empty() {
                    v.push((name, a));
                }
            }
        }
        _ => {
            for (s, a) in [
                ("vaapi", Accel::Vaapi),
                ("nvenc", Accel::Nvenc),
                ("qsv", Accel::Qsv),
                ("amf", Accel::Amf),
            ] {
                let name = n(codec, s);
                if !name.is_empty() {
                    v.push((name, a));
                }
            }
        }
    }
    match codec {
        VCodec::Av1 => v.push(("libsvtav1", Accel::Software)),
        VCodec::Vp9 => v.push(("libvpx-vp9", Accel::Software)),
        VCodec::H264 => v.push(("libopenh264", Accel::Software)),
        VCodec::Hevc => {}
    }
    v
}

fn build(name: &'static str, codec: VCodec, accel: Accel) -> Option<VEnc> {
    match accel {
        Accel::Vaapi => {
            let dev = vaapi_device()?;
            Some(VEnc {
                name,
                codec,
                accel,
                pre: vec!["-vaapi_device".into(), dev],
                post_filter: Some("hwupload".into()),
            })
        }
        _ => Some(VEnc {
            name,
            codec,
            accel,
            pre: Vec::new(),
            post_filter: None,
        }),
    }
}

/// Pick the first encoder for `codec` that exists and works on this machine. With
/// `hardware_only` software encoders are skipped. `PANOPLY_ENCODER` (an encoder name) forces
/// one, for tests and for people with an unusual setup.
pub fn pick(engine: &Engine, codec: VCodec, hardware_only: bool) -> Option<VEnc> {
    let forced = std::env::var("PANOPLY_ENCODER").ok();
    for (name, accel) in candidates(codec) {
        if let Some(f) = &forced
            && f != name
        {
            continue;
        }
        if !accel.is_hardware() && hardware_only {
            continue;
        }
        if name == "libopenh264" && engine.source == Source::Pinned {
            continue;
        }
        if !engine.has_encoder(name) {
            continue;
        }
        let Some(enc) = build(name, codec, accel) else {
            continue;
        };
        // A hardware encoder can be listed and still have no device: run it once on two frames.
        if accel.is_hardware() {
            let pre: Vec<&str> = enc.pre.iter().map(String::as_str).collect();
            if !engine.encoder_works(name, &pre, &probe_extra(&enc)) {
                continue;
            }
        }
        return Some(enc);
    }
    None
}

fn probe_extra(enc: &VEnc) -> Vec<&'static str> {
    // Only the pixel format the encoder wants; device arguments go before the input and the
    // probe builds its own tiny input, so hardware encoders that need an upload are probed
    // with the same filter.
    match enc.accel {
        Accel::Vaapi => vec!["-vf", "format=nv12,hwupload"],
        Accel::VideoToolbox | Accel::Nvenc | Accel::Amf | Accel::Qsv | Accel::MediaFoundation => {
            vec!["-pix_fmt", "nv12"]
        }
        Accel::Software => vec![],
    }
}

/// The message for a codec nobody here can encode.
pub fn none_for(codec: VCodec, hardware_only: bool) -> MediaError {
    match codec {
        VCodec::Hevc => MediaError::Unsupported(
            "This computer has no hardware HEVC encoder, and Panoply does not ship a software one. Choose H.264 or AV1 instead."
                .into(),
        ),
        VCodec::H264 => MediaError::Unsupported(
            "This computer has no H.264 encoder Panoply can use (no hardware encoder was found). Choose AV1 or VP9 (WebM) instead, or use a computer with a graphics card that encodes video."
                .into(),
        ),
        c => MediaError::Unsupported(format!(
            "No {} encoder is available{}.",
            c.plain(),
            if hardware_only { " in hardware" } else { "" }
        )),
    }
}

/// An audio codec Panoply can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ACodec {
    /// AAC.
    Aac,
    /// MP3.
    Mp3,
    /// Opus.
    Opus,
    /// Vorbis.
    Vorbis,
    /// FLAC.
    Flac,
    /// Uncompressed PCM (16 bit unless told otherwise).
    Pcm,
    /// Apple Lossless.
    Alac,
}

impl ACodec {
    /// ffmpeg's decoder-side name.
    pub fn name(self) -> &'static str {
        match self {
            ACodec::Aac => "aac",
            ACodec::Mp3 => "mp3",
            ACodec::Opus => "opus",
            ACodec::Vorbis => "vorbis",
            ACodec::Flac => "flac",
            ACodec::Pcm => "pcm_s16le",
            ACodec::Alac => "alac",
        }
    }

    /// Plain name.
    pub fn plain(self) -> &'static str {
        match self {
            ACodec::Aac => "AAC",
            ACodec::Mp3 => "MP3",
            ACodec::Opus => "Opus",
            ACodec::Vorbis => "Vorbis",
            ACodec::Flac => "FLAC",
            ACodec::Pcm => "WAV",
            ACodec::Alac => "Apple Lossless",
        }
    }

    /// True for lossy formats.
    pub fn is_lossy(self) -> bool {
        matches!(
            self,
            ACodec::Aac | ACodec::Mp3 | ACodec::Opus | ACodec::Vorbis
        )
    }

    /// The codec a probe name stands for.
    pub fn from_name(n: &str) -> Option<ACodec> {
        match n {
            "aac" => Some(ACodec::Aac),
            "mp3" => Some(ACodec::Mp3),
            "opus" => Some(ACodec::Opus),
            "vorbis" => Some(ACodec::Vorbis),
            "flac" => Some(ACodec::Flac),
            "alac" => Some(ACodec::Alac),
            n if n.starts_with("pcm_") => Some(ACodec::Pcm),
            _ => None,
        }
    }
}

/// True for a probe codec name that is lossy audio.
pub fn is_lossy_audio(name: &str) -> bool {
    matches!(
        name,
        "aac"
            | "mp3"
            | "mp2"
            | "opus"
            | "vorbis"
            | "ac3"
            | "eac3"
            | "dts"
            | "wmav2"
            | "wmav1"
            | "amr_nb"
            | "amr_wb"
            | "cook"
            | "atrac3"
    )
}

/// The encoder arguments for one audio stream (`-c:a` and the quality), with a bit rate in bits
/// per second chosen by the caller (`None` picks the codec's default for the channel count).
pub fn audio_args(
    engine: &Engine,
    codec: ACodec,
    channels: u32,
    bitrate: Option<u64>,
) -> Result<Vec<String>> {
    let need = |name: &str| -> Result<()> {
        if engine.has_encoder(name) {
            Ok(())
        } else {
            Err(MediaError::Unsupported(format!(
                "The video engine on this computer has no {name} encoder. Update the engine, or choose another format."
            )))
        }
    };
    let br = |default_stereo: u64| -> String {
        let b = bitrate.unwrap_or(if channels <= 1 {
            default_stereo * 2 / 3
        } else {
            default_stereo * u64::from(channels.min(6)).max(2) / 2
        });
        b.to_string()
    };
    let mut a: Vec<String> = Vec::new();
    let mut add = |k: &str, v: &str| {
        a.push(k.to_string());
        a.push(v.to_string());
    };
    match codec {
        ACodec::Aac => {
            // AudioToolbox is better than the native encoder where it exists (macOS).
            if engine.has_encoder("aac_at") {
                add("-c:a", "aac_at");
            } else {
                need("aac")?;
                add("-c:a", "aac");
            }
            add("-b:a", &br(160_000));
        }
        ACodec::Mp3 => {
            need("libmp3lame")?;
            add("-c:a", "libmp3lame");
            match bitrate {
                Some(b) => add("-b:a", &b.to_string()),
                None => add("-q:a", "2"),
            }
        }
        ACodec::Opus => {
            need("libopus")?;
            add("-c:a", "libopus");
            add("-b:a", &br(128_000));
            add("-vbr", "on");
        }
        ACodec::Vorbis => {
            need("libvorbis")?;
            add("-c:a", "libvorbis");
            match bitrate {
                Some(b) => add("-b:a", &b.to_string()),
                None => add("-q:a", "5"),
            }
        }
        ACodec::Flac => {
            need("flac")?;
            add("-c:a", "flac");
            add("-compression_level", "5");
        }
        ACodec::Alac => {
            need("alac")?;
            add("-c:a", "alac");
        }
        ACodec::Pcm => {
            add("-c:a", "pcm_s16le");
        }
    }
    Ok(a)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vt() -> VEnc {
        VEnc {
            name: "h264_videotoolbox",
            codec: VCodec::H264,
            accel: Accel::VideoToolbox,
            pre: vec![],
            post_filter: None,
        }
    }

    #[test]
    fn videotoolbox_gets_a_bitrate_and_never_falls_back_to_software() {
        let a = vt().args(Rate::Bitrate(4_000_000), false, 30.0);
        let joined = a.join(" ");
        assert!(joined.contains("-b:v 4000000"), "{joined}");
        assert!(joined.contains("-allow_sw 0"), "{joined}");
        assert!(joined.contains("-g 60"), "{joined}");
        assert!(joined.contains("-pix_fmt yuv420p"), "{joined}");
    }

    #[test]
    fn hevc_is_tagged_for_apple_and_ten_bit_uses_main10() {
        let e = VEnc {
            name: "hevc_videotoolbox",
            codec: VCodec::Hevc,
            accel: Accel::VideoToolbox,
            pre: vec![],
            post_filter: None,
        };
        let joined = e.args(Rate::Bitrate(8_000_000), true, 24.0).join(" ");
        assert!(joined.contains("-tag:v hvc1"), "{joined}");
        assert!(joined.contains("-profile:v main10"), "{joined}");
        assert!(joined.contains("-pix_fmt p010le"), "{joined}");
    }

    #[test]
    fn software_vp9_uses_constant_quality_with_zero_bitrate() {
        let e = VEnc {
            name: "libvpx-vp9",
            codec: VCodec::Vp9,
            accel: Accel::Software,
            pre: vec![],
            post_filter: None,
        };
        let joined = e.args(Rate::Quality(70), false, 30.0).join(" ");
        assert!(joined.contains("-b:v 0"), "{joined}");
        assert!(joined.contains("-crf"), "{joined}");
    }

    #[test]
    fn candidates_never_name_a_gpl_encoder() {
        for c in [VCodec::H264, VCodec::Hevc, VCodec::Av1, VCodec::Vp9] {
            for (name, _) in candidates(c) {
                assert!(!name.contains("x264") && !name.contains("x265"), "{name}");
                assert!(name != "libx264" && name != "libx265");
            }
        }
        // HEVC has no software candidate at all.
        assert!(
            candidates(VCodec::Hevc)
                .iter()
                .all(|(_, a)| a.is_hardware())
        );
    }

    #[test]
    fn quality_maps_higher_to_lower_crf() {
        assert!(crf(VCodec::H264, 90) < crf(VCodec::H264, 50));
        assert!(crf(VCodec::Av1, 70) > crf(VCodec::H264, 70));
    }
}

//! The probe profile (spec section 0.1, "P"): what a file is, read without decoding.
//!
//! One ffprobe call gives the container, every stream and the chapters. Own code turns that
//! JSON into [`Profile`], the one type every module plans from.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::engine::Engine;
use crate::error::{MediaError, Result, unreadable};
use crate::io::{Io, tool_path};
use crate::proc;

/// What kind of stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Pictures.
    Video,
    /// Sound.
    Audio,
    /// Text or bitmap subtitles.
    Subtitle,
    /// Timecode, metadata or other data.
    Data,
    /// A font, a cover picture or another attached file.
    Attachment,
}

/// The dynamic range of a video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hdr {
    /// Standard dynamic range.
    Sdr,
    /// HDR10 or HDR10+: PQ transfer, BT.2020 primaries.
    Hdr10,
    /// Hybrid log-gamma.
    Hlg,
    /// Dolby Vision, with its profile and the base layer compatibility id.
    DolbyVision {
        /// 5, 7, 8, 9 and so on.
        profile: u8,
        /// 0 none, 1 HDR10, 2 SDR, 4 HLG.
        compat: u8,
    },
}

impl Hdr {
    /// True for anything that is not plain SDR.
    pub fn is_hdr(self) -> bool {
        !matches!(self, Hdr::Sdr)
    }

    /// Plain words for the report.
    pub fn plain(self) -> String {
        match self {
            Hdr::Sdr => "standard range (SDR)".into(),
            Hdr::Hdr10 => "HDR10".into(),
            Hdr::Hlg => "HLG".into(),
            Hdr::DolbyVision { profile, compat } => match (profile, compat) {
                (8, 4) => "Dolby Vision profile 8.4 (HLG base layer)".into(),
                (8, 1) => "Dolby Vision profile 8.1 (HDR10 base layer)".into(),
                (p, _) => format!("Dolby Vision profile {p}"),
            },
        }
    }
}

/// One stream of a file.
#[derive(Debug, Clone)]
pub struct Stream {
    /// Index in the file.
    pub index: u32,
    /// Video, audio, subtitle, data or attachment.
    pub kind: Kind,
    /// ffmpeg's codec name: `h264`, `hevc`, `aac`, `subrip`.
    pub codec: String,
    /// The codec profile, such as `High` or `Main 10`.
    pub profile: String,
    /// The codec tag as four characters, such as `avc1` or `hvc1`.
    pub tag: String,
    /// Width in pixels (video).
    pub width: u32,
    /// Height in pixels (video).
    pub height: u32,
    /// Pixel format (video).
    pub pix_fmt: String,
    /// Bits per sample component (video 8, 10, 12) or per audio sample.
    pub bit_depth: u32,
    /// Average frame rate.
    pub fps: f64,
    /// Nominal (base) frame rate.
    pub r_fps: f64,
    /// Primaries tag.
    pub color_primaries: String,
    /// Transfer tag.
    pub color_transfer: String,
    /// Matrix tag.
    pub color_space: String,
    /// `tv` or `pc`.
    pub color_range: String,
    /// `progressive`, `tt`, `bb`, `tb`, `bt` or empty.
    pub field_order: String,
    /// Sample aspect ratio such as `1:1`.
    pub sar: String,
    /// Clockwise degrees the picture must be turned for display (0, 90, 180, 270).
    pub rotation: i32,
    /// The dynamic range.
    pub hdr: Hdr,
    /// True when mastering display or content light metadata is present.
    pub hdr_metadata: bool,
    /// Peak luminance of the mastering display in nits, when stated.
    pub max_luminance: Option<f64>,
    /// Maximum content light level in nits, when stated.
    pub max_cll: Option<f64>,
    /// Audio channels.
    pub channels: u32,
    /// Audio channel layout name.
    pub channel_layout: String,
    /// Audio sample rate in hertz.
    pub sample_rate: u32,
    /// Bit rate when the file says.
    pub bit_rate: Option<u64>,
    /// Language tag.
    pub language: String,
    /// Title tag.
    pub title: String,
    /// The default flag.
    pub default: bool,
    /// The forced flag.
    pub forced: bool,
    /// A commentary, descriptive or similar track.
    pub commentary: bool,
    /// Cover art stored as a video stream.
    pub attached_pic: bool,
    /// Duration in seconds when the stream says.
    pub duration: Option<f64>,
    /// Frame or packet count when the container stores it.
    pub frames: Option<u64>,
    /// Time base numerator and denominator.
    pub time_base: (i64, i64),
    /// Subtitle text kind: true for text, false for bitmap.
    pub text_subtitle: bool,
}

impl Stream {
    fn blank(index: u32, kind: Kind) -> Self {
        Stream {
            index,
            kind,
            codec: String::new(),
            profile: String::new(),
            tag: String::new(),
            width: 0,
            height: 0,
            pix_fmt: String::new(),
            bit_depth: 0,
            fps: 0.0,
            r_fps: 0.0,
            color_primaries: String::new(),
            color_transfer: String::new(),
            color_space: String::new(),
            color_range: String::new(),
            field_order: String::new(),
            sar: String::new(),
            rotation: 0,
            hdr: Hdr::Sdr,
            hdr_metadata: false,
            max_luminance: None,
            max_cll: None,
            channels: 0,
            channel_layout: String::new(),
            sample_rate: 0,
            bit_rate: None,
            language: String::new(),
            title: String::new(),
            default: false,
            forced: false,
            commentary: false,
            attached_pic: false,
            duration: None,
            frames: None,
            time_base: (1, 1000),
            text_subtitle: false,
        }
    }
}

/// The container family, as far as planning needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Container {
    /// MP4 (ISO base media, not QuickTime).
    Mp4,
    /// QuickTime MOV.
    Mov,
    /// Matroska.
    Mkv,
    /// WebM.
    Webm,
    /// AVI.
    Avi,
    /// MPEG transport stream.
    Ts,
    /// Ogg.
    Ogg,
    /// MP3.
    Mp3,
    /// WAV.
    Wav,
    /// FLAC.
    Flac,
    /// ADTS AAC.
    Aac,
    /// Anything else, by ffmpeg's name.
    Other,
}

/// What a file is.
#[derive(Debug, Clone)]
pub struct Profile {
    /// The path probed.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// ffmpeg's container name.
    pub format_name: String,
    /// The container family.
    pub container: Container,
    /// Duration in seconds.
    pub duration: Option<f64>,
    /// Overall bit rate.
    pub bit_rate: Option<u64>,
    /// Every stream.
    pub streams: Vec<Stream>,
    /// Number of chapters.
    pub chapters: usize,
    /// Container-level tags (title, creation time, location, make, model).
    pub tags: Vec<(String, String)>,
}

impl Profile {
    /// The streams of one kind, skipping cover art for video.
    pub fn of(&self, kind: Kind) -> impl Iterator<Item = &Stream> {
        self.streams
            .iter()
            .filter(move |s| s.kind == kind && !(kind == Kind::Video && s.attached_pic))
    }

    /// The first real video stream.
    pub fn video(&self) -> Option<&Stream> {
        self.of(Kind::Video).next()
    }

    /// The first audio stream (the default one when several are marked).
    pub fn audio(&self) -> Option<&Stream> {
        self.of(Kind::Audio)
            .find(|s| s.default)
            .or_else(|| self.of(Kind::Audio).next())
    }

    /// True when the file has pictures that are not just cover art.
    pub fn has_video(&self) -> bool {
        self.video().is_some()
    }

    /// True when the file has sound.
    pub fn has_audio(&self) -> bool {
        self.of(Kind::Audio).next().is_some()
    }

    /// Duration, or an error that says what to do when the file does not state one.
    pub fn need_duration(&self) -> Result<f64> {
        self.duration
            .filter(|d| *d > 0.0 && d.is_finite())
            .ok_or_else(|| {
                MediaError::Corrupt(
                    "This file does not say how long it is, so Panoply cannot plan the job. Try Repair video first."
                        .into(),
                )
            })
    }

    /// The value of a container tag, case-insensitive.
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

fn s(v: &Value, k: &str) -> String {
    v.get(k)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn n(v: &Value, k: &str) -> Option<f64> {
    match v.get(k)? {
        Value::String(t) => t.parse().ok(),
        Value::Number(x) => x.as_f64(),
        _ => None,
    }
}

fn frac(text: &str) -> f64 {
    let Some((a, b)) = text.split_once('/') else {
        return text.parse().unwrap_or(0.0);
    };
    let (a, b): (f64, f64) = (a.parse().unwrap_or(0.0), b.parse().unwrap_or(0.0));
    if b == 0.0 { 0.0 } else { a / b }
}

fn flag(v: &Value, k: &str) -> bool {
    v.get("disposition")
        .and_then(|d| d.get(k))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        != 0
}

fn tags_of(v: &Value) -> Vec<(String, String)> {
    v.get("tags")
        .and_then(Value::as_object)
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|t| (k.clone(), t.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn stream_from(v: &Value) -> Stream {
    let kind = match v.get("codec_type").and_then(Value::as_str) {
        Some("video") => Kind::Video,
        Some("audio") => Kind::Audio,
        Some("subtitle") => Kind::Subtitle,
        Some("attachment") => Kind::Attachment,
        _ => Kind::Data,
    };
    let mut st = Stream::blank(n(v, "index").unwrap_or(0.0) as u32, kind);
    st.codec = s(v, "codec_name");
    st.profile = s(v, "profile");
    st.tag = s(v, "codec_tag_string");
    st.width = n(v, "width").unwrap_or(0.0) as u32;
    st.height = n(v, "height").unwrap_or(0.0) as u32;
    st.pix_fmt = s(v, "pix_fmt");
    st.bit_depth = n(v, "bits_per_raw_sample")
        .map(|b| b as u32)
        .filter(|b| *b > 0)
        .or_else(|| {
            if st.pix_fmt.contains("p10")
                || st.pix_fmt.contains("10le")
                || st.pix_fmt.contains("10be")
            {
                Some(10)
            } else if st.pix_fmt.contains("p12") || st.pix_fmt.contains("12le") {
                Some(12)
            } else if kind == Kind::Video && !st.pix_fmt.is_empty() {
                Some(8)
            } else {
                None
            }
        })
        .or_else(|| n(v, "bits_per_sample").map(|b| b as u32))
        .unwrap_or(0);
    st.fps = frac(&s(v, "avg_frame_rate"));
    st.r_fps = frac(&s(v, "r_frame_rate"));
    st.color_primaries = s(v, "color_primaries");
    st.color_transfer = s(v, "color_transfer");
    st.color_space = s(v, "color_space");
    st.color_range = s(v, "color_range");
    st.field_order = s(v, "field_order");
    st.sar = s(v, "sample_aspect_ratio");
    st.channels = n(v, "channels").unwrap_or(0.0) as u32;
    st.channel_layout = s(v, "channel_layout");
    st.sample_rate = n(v, "sample_rate").unwrap_or(0.0) as u32;
    st.bit_rate = n(v, "bit_rate").map(|b| b as u64).filter(|b| *b > 0);
    st.duration = n(v, "duration").filter(|d| *d > 0.0);
    st.frames = n(v, "nb_frames").map(|f| f as u64).filter(|f| *f > 0);
    let tb = s(v, "time_base");
    if let Some((a, b)) = tb.split_once('/') {
        st.time_base = (a.parse().unwrap_or(1), b.parse().unwrap_or(1000));
    }
    st.default = flag(v, "default");
    st.forced = flag(v, "forced");
    st.attached_pic = flag(v, "attached_pic");
    st.commentary = flag(v, "comment") || flag(v, "descriptions") || flag(v, "visual_impaired");
    st.text_subtitle = matches!(
        st.codec.as_str(),
        "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "webvtt" | "text" | "ttml" | "microdvd"
    );
    let tags = tags_of(v);
    for (k, val) in &tags {
        match k.to_ascii_lowercase().as_str() {
            "language" => st.language = val.clone(),
            "title" => st.title = val.clone(),
            "rotate" => {
                if let Ok(r) = val.parse::<i32>() {
                    st.rotation = r.rem_euclid(360);
                }
            }
            _ => {}
        }
    }
    if let Some(list) = v.get("side_data_list").and_then(Value::as_array) {
        for sd in list {
            match sd
                .get("side_data_type")
                .and_then(Value::as_str)
                .unwrap_or("")
            {
                "Display Matrix" => {
                    if let Some(r) = n(sd, "rotation") {
                        // ffprobe reports counter-clockwise degrees; the display needs clockwise.
                        st.rotation = ((-r.round() as i32) % 360 + 360) % 360;
                    }
                }
                "Mastering display metadata" => {
                    st.hdr_metadata = true;
                    st.max_luminance = sd
                        .get("max_luminance")
                        .and_then(Value::as_str)
                        .map(frac)
                        .filter(|n| *n > 0.0);
                }
                "Content light level metadata" => {
                    st.hdr_metadata = true;
                    st.max_cll = n(sd, "max_content").filter(|n| *n > 0.0);
                }
                "DOVI configuration record" => {
                    let profile = n(sd, "dv_profile").unwrap_or(0.0) as u8;
                    let compat = n(sd, "dv_bl_signal_compatibility_id").unwrap_or(0.0) as u8;
                    st.hdr = Hdr::DolbyVision { profile, compat };
                }
                _ => {}
            }
        }
    }
    if !matches!(st.hdr, Hdr::DolbyVision { .. }) {
        st.hdr = match st.color_transfer.as_str() {
            "smpte2084" => Hdr::Hdr10,
            "arib-std-b67" => Hdr::Hlg,
            _ => Hdr::Sdr,
        };
    }
    st
}

fn container_of(format_name: &str, brand: &str, ext: &str) -> Container {
    let f = format_name;
    if f.contains("mp4") || f.contains("mov") {
        return if brand.trim() == "qt" || ext == "mov" {
            Container::Mov
        } else {
            Container::Mp4
        };
    }
    if f.contains("matroska") || f.contains("webm") {
        return if ext == "webm" {
            Container::Webm
        } else {
            Container::Mkv
        };
    }
    match f {
        "avi" => Container::Avi,
        "mpegts" => Container::Ts,
        "ogg" => Container::Ogg,
        "mp3" => Container::Mp3,
        "wav" => Container::Wav,
        "flac" => Container::Flac,
        "aac" => Container::Aac,
        _ => Container::Other,
    }
}

/// Parse ffprobe's JSON into a profile. Split from [`probe`] so it can be tested on text.
pub fn parse_profile(path: &Path, json: &[u8], size: u64) -> Result<Profile> {
    let v: Value = serde_json::from_slice(json)
        .map_err(|e| unreadable(format!("the probe answer was not readable: {e}")))?;
    let fmt = v.get("format").cloned().unwrap_or(Value::Null);
    let format_name = s(&fmt, "format_name");
    if format_name.is_empty() {
        return Err(unreadable("no container was recognised"));
    }
    let tags = tags_of(&fmt);
    let brand = tags
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("major_brand"))
        .map(|(_, v)| v.as_str())
        .unwrap_or("");
    let ext = crate::units::ext_of(path);
    let streams: Vec<Stream> = v
        .get("streams")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(stream_from).collect())
        .unwrap_or_default();
    let mut duration = n(&fmt, "duration").filter(|d| *d > 0.0 && d.is_finite());
    if duration.is_none() {
        duration = streams
            .iter()
            .filter_map(|s| s.duration)
            .fold(None, |m: Option<f64>, d| Some(m.map_or(d, |x| x.max(d))));
    }
    Ok(Profile {
        path: path.to_path_buf(),
        size: n(&fmt, "size").map(|x| x as u64).unwrap_or(size),
        container: container_of(&format_name, brand, &ext),
        format_name,
        duration,
        bit_rate: n(&fmt, "bit_rate").map(|b| b as u64).filter(|b| *b > 0),
        streams,
        chapters: v
            .get("chapters")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0),
        tags,
    })
}

/// Probe a file: header and index only, no decoding. The path is checked against the host's
/// grant first.
pub fn probe(engine: &Engine, io: &dyn Io, path: &Path) -> Result<Profile> {
    let real = io.read_path(path)?;
    probe_trusted(engine, io, &real)
}

/// Probe a file Panoply itself wrote (a result that is being checked): no grant check, because
/// the grant covers what the user chose, not what a job creates.
pub fn probe_trusted(engine: &Engine, io: &dyn Io, real: &Path) -> Result<Profile> {
    let size = std::fs::metadata(real)?.len();
    let args: Vec<OsString> = [
        "-v",
        "error",
        "-hide_banner",
        "-of",
        "json",
        "-show_format",
        "-show_streams",
        "-show_chapters",
        "-show_entries",
        "stream_side_data:format_tags:stream_tags",
    ]
    .iter()
    .map(OsString::from)
    .chain([OsString::from(tool_path(real))])
    .collect();
    // `-show_entries` above is only a hint to include side data; the sections asked for with
    // -show_format and -show_streams stay complete.
    let out = proc::capture(engine, Some(io), &engine.ffprobe, &args)?;
    if !out.ok || out.stdout.is_empty() {
        return Err(proc::explain(&out.tail, &crate::units::name_of(real)));
    }
    parse_profile(real, &out.stdout, size)
}

/// Frame spacing statistics from the first packets of the video stream.
#[derive(Debug, Clone, Copy)]
pub struct Spacing {
    /// Smallest gap between presentation times, seconds.
    pub min: f64,
    /// Largest gap.
    pub max: f64,
    /// Packets looked at.
    pub packets: usize,
}

impl Spacing {
    /// True when gaps differ by more than rounding.
    pub fn is_variable(&self) -> bool {
        self.packets > 8 && self.max > self.min * 1.5 + 0.0005
    }
}

/// Sample the first 200 packets of the first video stream and measure the gaps between their
/// presentation times (VFR detection, spec 0.1).
pub fn video_spacing(engine: &Engine, io: &dyn Io, path: &Path) -> Result<Option<Spacing>> {
    let real = io.read_path(path)?;
    let args: Vec<OsString> = [
        "-v",
        "error",
        "-select_streams",
        "v:0",
        "-show_entries",
        "packet=pts_time",
        "-read_intervals",
        "%+#200",
        "-of",
        "csv=p=0",
    ]
    .iter()
    .map(OsString::from)
    .chain([OsString::from(tool_path(&real))])
    .collect();
    let out = proc::capture(engine, Some(io), &engine.ffprobe, &args)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut pts: Vec<f64> = text
        .lines()
        .filter_map(|l| l.split(',').next()?.trim().parse::<f64>().ok())
        .collect();
    if pts.len() < 3 {
        return Ok(None);
    }
    pts.sort_by(f64::total_cmp);
    let gaps: Vec<f64> = pts
        .windows(2)
        .map(|w| w[1] - w[0])
        .filter(|g| *g > 0.0)
        .collect();
    if gaps.is_empty() {
        return Ok(None);
    }
    Ok(Some(Spacing {
        min: gaps.iter().cloned().fold(f64::INFINITY, f64::min),
        max: gaps.iter().cloned().fold(0.0, f64::max),
        packets: pts.len(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "streams": [
        {"index":0,"codec_name":"hevc","profile":"Main 10","codec_type":"video","codec_tag_string":"hvc1",
         "width":3840,"height":2160,"pix_fmt":"yuv420p10le","r_frame_rate":"60/1","avg_frame_rate":"3000/101",
         "color_primaries":"bt2020","color_transfer":"arib-std-b67","color_space":"bt2020nc","time_base":"1/600",
         "disposition":{"default":1},
         "tags":{"language":"und"},
         "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]},
        {"index":1,"codec_name":"aac","codec_type":"audio","sample_rate":"48000","channels":2,"channel_layout":"stereo",
         "bit_rate":"128000","disposition":{"default":1},"tags":{"language":"eng","title":"Stereo"}},
        {"index":2,"codec_name":"mov_text","codec_type":"subtitle","disposition":{"forced":1}}
      ],
      "format": {"format_name":"mov,mp4,m4a,3gp,3g2,mj2","duration":"12.5","size":"1000","bit_rate":"640",
                 "tags":{"major_brand":"qt  ","com.apple.quicktime.model":"iPhone 15"}},
      "chapters": [{"id":1}]
    }"#;

    #[test]
    fn a_phone_file_parses_into_a_profile() {
        let p = parse_profile(Path::new("/x/a.mov"), SAMPLE.as_bytes(), 1000).unwrap();
        assert_eq!(p.container, Container::Mov);
        assert_eq!(p.duration, Some(12.5));
        assert_eq!(p.chapters, 1);
        let v = p.video().unwrap();
        assert_eq!(v.hdr, Hdr::Hlg);
        assert_eq!(v.bit_depth, 10);
        assert_eq!(
            v.rotation, 90,
            "ffprobe says -90, the display needs 90 clockwise"
        );
        assert!((v.fps - 29.7).abs() < 0.1);
        let a = p.audio().unwrap();
        assert_eq!(
            (a.channels, a.sample_rate, a.language.as_str()),
            (2, 48000, "eng")
        );
        let sub = &p.streams[2];
        assert!(sub.text_subtitle && sub.forced);
        assert_eq!(p.tag("com.apple.quicktime.model"), Some("iPhone 15"));
    }

    #[test]
    fn dolby_vision_side_data_wins_over_transfer() {
        let json = r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc","color_transfer":"arib-std-b67",
          "side_data_list":[{"side_data_type":"DOVI configuration record","dv_profile":8,"dv_bl_signal_compatibility_id":4}]}],
          "format":{"format_name":"mov,mp4,m4a,3gp,3g2,mj2"}}"#;
        let p = parse_profile(Path::new("a.mp4"), json.as_bytes(), 1).unwrap();
        assert_eq!(
            p.video().unwrap().hdr,
            Hdr::DolbyVision {
                profile: 8,
                compat: 4
            }
        );
        assert!(p.video().unwrap().hdr.plain().contains("HLG"));
    }

    #[test]
    fn no_container_is_unreadable_not_a_panic() {
        assert!(parse_profile(Path::new("a"), b"{}", 1).is_err());
        assert!(parse_profile(Path::new("a"), b"not json", 1).is_err());
    }

    #[test]
    fn spacing_detects_variable_rates() {
        let even = Spacing {
            min: 0.0333,
            max: 0.0334,
            packets: 100,
        };
        let uneven = Spacing {
            min: 0.0167,
            max: 0.133,
            packets: 100,
        };
        assert!(!even.is_variable());
        assert!(uneven.is_variable());
    }
}

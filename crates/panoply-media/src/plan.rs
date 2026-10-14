//! Per-stream routing for container changes (spec 4.3, "COPY"): which streams can go into the
//! target as they are, which need only a transcode of that one stream, and which cannot be
//! carried at all. Copy is always tried first, and audio or subtitles are converted before the
//! video is touched, because a re-encoded picture is the expensive and lossy part.

use crate::encoders::{ACodec, VCodec};
use crate::probe::{Kind, Profile, Stream};

/// A video container Panoply can write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// MP4.
    Mp4,
    /// QuickTime MOV.
    Mov,
    /// Matroska.
    Mkv,
    /// WebM.
    Webm,
    /// AVI.
    Avi,
}

impl Target {
    /// Parse the option value.
    pub fn from_name(n: &str) -> Option<Target> {
        match n {
            "mp4" => Some(Target::Mp4),
            "mov" => Some(Target::Mov),
            "mkv" => Some(Target::Mkv),
            "webm" => Some(Target::Webm),
            "avi" => Some(Target::Avi),
            _ => None,
        }
    }

    /// File extension.
    pub fn ext(self) -> &'static str {
        match self {
            Target::Mp4 => "mp4",
            Target::Mov => "mov",
            Target::Mkv => "mkv",
            Target::Webm => "webm",
            Target::Avi => "avi",
        }
    }

    /// ffmpeg's muxer name.
    pub fn format(self) -> &'static str {
        match self {
            Target::Mp4 => "mp4",
            Target::Mov => "mov",
            Target::Mkv => "matroska",
            Target::Webm => "webm",
            Target::Avi => "avi",
        }
    }

    /// True when a finished file should have its index at the front (spec MOOV).
    pub fn faststart(self) -> bool {
        matches!(self, Target::Mp4 | Target::Mov)
    }

    /// Can the container hold this video codec as it is?
    pub fn holds_video(self, codec: &str) -> bool {
        match self {
            Target::Mp4 => matches!(
                codec,
                "h264" | "hevc" | "av1" | "vp9" | "mpeg4" | "mpeg2video" | "h263"
            ),
            Target::Mov => matches!(
                codec,
                "h264"
                    | "hevc"
                    | "prores"
                    | "mpeg4"
                    | "mjpeg"
                    | "dnxhd"
                    | "av1"
                    | "mpeg2video"
                    | "dvvideo"
            ),
            Target::Mkv => true,
            Target::Webm => matches!(codec, "vp8" | "vp9" | "av1"),
            Target::Avi => matches!(
                codec,
                "mpeg4" | "h264" | "mjpeg" | "msmpeg4v3" | "mpeg2video" | "dvvideo" | "huffyuv"
            ),
        }
    }

    /// Can the container hold this audio codec as it is?
    pub fn holds_audio(self, codec: &str) -> bool {
        match self {
            Target::Mp4 => matches!(
                codec,
                "aac" | "mp3" | "ac3" | "eac3" | "opus" | "flac" | "alac" | "mp2"
            ),
            Target::Mov => {
                codec.starts_with("pcm_")
                    || matches!(codec, "aac" | "alac" | "mp3" | "ac3" | "eac3")
            }
            Target::Mkv => true,
            Target::Webm => matches!(codec, "opus" | "vorbis"),
            Target::Avi => {
                codec.starts_with("pcm_") || matches!(codec, "mp3" | "ac3" | "mp2" | "aac")
            }
        }
    }

    /// What a text subtitle becomes in this container, if it needs to change.
    fn text_subtitle(self, codec: &str) -> Option<SubRoute> {
        match self {
            Target::Mp4 | Target::Mov => match codec {
                "mov_text" => Some(SubRoute::Copy),
                "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "text" => {
                    Some(SubRoute::Convert("mov_text"))
                }
                _ => None,
            },
            Target::Mkv => match codec {
                "mov_text" => Some(SubRoute::Convert("srt")),
                "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "text" => Some(SubRoute::Copy),
                _ => None,
            },
            Target::Webm => match codec {
                "webvtt" => Some(SubRoute::Copy),
                "subrip" | "srt" | "ass" | "ssa" | "mov_text" | "text" => {
                    Some(SubRoute::Convert("webvtt"))
                }
                _ => None,
            },
            Target::Avi => None,
        }
    }

    /// Bitmap subtitles the container can carry as they are.
    fn holds_bitmap_subtitle(self, codec: &str) -> bool {
        matches!(self, Target::Mkv)
            && matches!(codec, "hdmv_pgs_subtitle" | "dvd_subtitle" | "dvb_subtitle")
    }
}

/// What happens to a video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VRoute {
    /// Copy the stream.
    Copy,
    /// Encode it as this codec.
    Encode(VCodec),
    /// Encode it as MPEG-4 part 2 (ffmpeg's own encoder), for AVI when nothing better exists.
    EncodeMpeg4,
}

/// What happens to an audio stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ARoute {
    /// Copy the stream.
    Copy,
    /// Encode it as this codec.
    Encode(ACodec),
}

/// What happens to a subtitle stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubRoute {
    /// Copy the stream.
    Copy,
    /// Convert the text to this subtitle codec.
    Convert(&'static str),
    /// Leave it out, with the reason shown on the result card.
    Drop(String),
}

/// The decisions for one file.
#[derive(Debug, Clone, Default)]
pub struct Plan {
    /// `(stream index, route)` for each video stream kept.
    pub video: Vec<(u32, VRoute)>,
    /// Audio streams kept.
    pub audio: Vec<(u32, ARoute)>,
    /// Subtitle streams.
    pub subs: Vec<(u32, SubRoute)>,
    /// Cover pictures kept as attached pictures (index).
    pub covers: Vec<u32>,
    /// Lines for the result card.
    pub notes: Vec<String>,
}

impl Plan {
    /// True when nothing is re-encoded.
    pub fn is_copy_only(&self) -> bool {
        self.video.iter().all(|(_, r)| *r == VRoute::Copy)
            && self.audio.iter().all(|(_, r)| *r == ARoute::Copy)
            && self
                .subs
                .iter()
                .all(|(_, r)| !matches!(r, SubRoute::Convert(_)))
    }

    /// True when a video stream is re-encoded.
    pub fn encodes_video(&self) -> bool {
        self.video.iter().any(|(_, r)| *r != VRoute::Copy)
    }
}

fn audio_fallback(target: Target) -> ACodec {
    match target {
        Target::Webm => ACodec::Opus,
        Target::Avi => ACodec::Mp3,
        _ => ACodec::Aac,
    }
}

/// Route every stream of `p` into `target`. `force` asks for a particular video codec (the HEVC
/// and AV1 targets of the Convert module): a stream already in that codec is copied.
pub fn plan_convert(p: &Profile, target: Target, force: Option<VCodec>) -> Plan {
    let mut plan = Plan::default();
    for s in &p.streams {
        match s.kind {
            Kind::Video if s.attached_pic => {
                if matches!(target, Target::Mp4 | Target::Mov | Target::Mkv)
                    && matches!(s.codec.as_str(), "mjpeg" | "png")
                {
                    plan.covers.push(s.index);
                } else {
                    plan.notes
                        .push("Cover art was left out: this format cannot carry it.".into());
                }
            }
            Kind::Video => {
                let route = route_video(s, target, force);
                if route != VRoute::Copy
                    && s.hdr.is_hdr()
                    && matches!(route, VRoute::Encode(VCodec::H264))
                {
                    plan.notes.push(
                        "The video is HDR and H.264 here is 8 bit, so colours may look flat. Use HDR to SDR, or choose HEVC."
                            .into(),
                    );
                }
                plan.video.push((s.index, route));
            }
            Kind::Audio => {
                let route = if target.holds_audio(&s.codec) {
                    ARoute::Copy
                } else {
                    ARoute::Encode(audio_fallback(target))
                };
                plan.audio.push((s.index, route));
            }
            Kind::Subtitle => {
                let route = route_subtitle(s, target);
                plan.subs.push((s.index, route));
            }
            Kind::Attachment => {
                if target != Target::Mkv {
                    plan.notes.push(format!(
                        "An attached file ({}) was left out: only MKV can carry attachments.",
                        s.title
                    ));
                }
            }
            Kind::Data => {
                plan.notes.push(
                    "A data track (timecode or camera telemetry) was not carried over.".into(),
                );
            }
        }
    }
    for (i, r) in &plan.subs {
        if let SubRoute::Drop(why) = r {
            plan.notes.push(why.clone());
            let _ = i;
        }
    }
    if target == Target::Avi && p.chapters > 0 {
        plan.notes
            .push("AVI cannot keep chapters, so they were left out.".into());
    }
    plan
}

fn route_video(s: &Stream, target: Target, force: Option<VCodec>) -> VRoute {
    if let Some(f) = force {
        if s.codec == f.name() && target.holds_video(&s.codec) {
            return VRoute::Copy;
        }
        return VRoute::Encode(f);
    }
    if target.holds_video(&s.codec) {
        return VRoute::Copy;
    }
    match target {
        Target::Webm => VRoute::Encode(VCodec::Vp9),
        Target::Avi => VRoute::Encode(VCodec::H264),
        _ => VRoute::Encode(VCodec::H264),
    }
}

fn route_subtitle(s: &Stream, target: Target) -> SubRoute {
    if s.text_subtitle {
        return target.text_subtitle(&s.codec).unwrap_or_else(|| {
            SubRoute::Drop(format!(
                "A {} subtitle track was left out: {} cannot carry it.",
                s.language_or("unnamed"),
                target.ext().to_uppercase()
            ))
        });
    }
    if target.holds_bitmap_subtitle(&s.codec) {
        return SubRoute::Copy;
    }
    SubRoute::Drop(format!(
        "A picture-based subtitle track ({}, {}) cannot go in {}, so it was left out. Use MKV to keep it.",
        s.language_or("unnamed"),
        s.codec,
        target.ext().to_uppercase()
    ))
}

impl Stream {
    /// The language tag, or this fallback.
    pub fn language_or<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.language.is_empty() || self.language == "und" {
            fallback
        } else {
            &self.language
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::parse_profile;
    use std::path::Path;

    fn profile(streams: &str, fmt: &str) -> Profile {
        let json = format!(
            r#"{{"streams":[{streams}],"format":{{"format_name":"{fmt}","duration":"10"}}}}"#
        );
        parse_profile(Path::new("a.mkv"), json.as_bytes(), 1).unwrap()
    }

    const H264: &str =
        r#"{"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080}"#;
    const AAC: &str = r#"{"index":1,"codec_type":"audio","codec_name":"aac","channels":2}"#;
    const OPUS: &str = r#"{"index":2,"codec_type":"audio","codec_name":"opus","channels":2}"#;
    const SRT: &str =
        r#"{"index":3,"codec_type":"subtitle","codec_name":"subrip","tags":{"language":"eng"}}"#;
    const PGS: &str = r#"{"index":4,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle","tags":{"language":"fra"}}"#;
    const VP9: &str = r#"{"index":0,"codec_type":"video","codec_name":"vp9"}"#;
    const THEORA: &str = r#"{"index":0,"codec_type":"video","codec_name":"theora"}"#;

    #[test]
    fn mkv_to_mp4_copies_h264_and_aac_and_converts_text_subtitles() {
        let p = profile(&[H264, AAC, SRT, PGS].join(","), "matroska,webm");
        let plan = plan_convert(&p, Target::Mp4, None);
        assert_eq!(plan.video, vec![(0, VRoute::Copy)]);
        assert_eq!(plan.audio, vec![(1, ARoute::Copy)]);
        assert_eq!(plan.subs[0], (3, SubRoute::Convert("mov_text")));
        assert!(
            matches!(plan.subs[1].1, SubRoute::Drop(_)),
            "PGS cannot go in MP4"
        );
        assert!(
            plan.notes.iter().any(|n| n.contains("picture-based")),
            "{:?}",
            plan.notes
        );
        assert!(!plan.encodes_video());
    }

    #[test]
    fn h264_to_webm_must_encode_video_and_audio_but_only_what_does_not_fit() {
        let p = profile(&[H264, OPUS].join(","), "mov,mp4,m4a,3gp,3g2,mj2");
        let plan = plan_convert(&p, Target::Webm, None);
        assert_eq!(plan.video, vec![(0, VRoute::Encode(VCodec::Vp9))]);
        assert_eq!(
            plan.audio,
            vec![(2, ARoute::Copy)],
            "opus already fits WebM"
        );
        let p = profile(&[VP9, AAC].join(","), "matroska,webm");
        let plan = plan_convert(&p, Target::Webm, None);
        assert_eq!(plan.video, vec![(0, VRoute::Copy)]);
        assert_eq!(
            plan.audio,
            vec![(1, ARoute::Encode(ACodec::Opus))],
            "only the audio changes"
        );
    }

    #[test]
    fn an_unusual_codec_goes_to_h264_for_mp4_and_stays_for_mkv() {
        let p = profile(THEORA, "ogg");
        assert_eq!(
            plan_convert(&p, Target::Mp4, None).video,
            vec![(0, VRoute::Encode(VCodec::H264))]
        );
        assert_eq!(
            plan_convert(&p, Target::Mkv, None).video,
            vec![(0, VRoute::Copy)]
        );
    }

    #[test]
    fn the_hevc_target_copies_hevc_and_encodes_the_rest() {
        let hevc = r#"{"index":0,"codec_type":"video","codec_name":"hevc"}"#;
        let p = profile(hevc, "matroska,webm");
        assert_eq!(
            plan_convert(&p, Target::Mp4, Some(VCodec::Hevc)).video,
            vec![(0, VRoute::Copy)]
        );
        let p = profile(H264, "matroska,webm");
        assert_eq!(
            plan_convert(&p, Target::Mp4, Some(VCodec::Hevc)).video,
            vec![(0, VRoute::Encode(VCodec::Hevc))]
        );
    }

    #[test]
    fn mp4_subtitles_become_srt_in_mkv_and_avi_drops_them() {
        let mov_text = r#"{"index":3,"codec_type":"subtitle","codec_name":"mov_text"}"#;
        let p = profile(&[H264, mov_text].join(","), "mov,mp4,m4a,3gp,3g2,mj2");
        assert_eq!(
            plan_convert(&p, Target::Mkv, None).subs[0].1,
            SubRoute::Convert("srt")
        );
        assert!(matches!(
            plan_convert(&p, Target::Avi, None).subs[0].1,
            SubRoute::Drop(_)
        ));
    }
}

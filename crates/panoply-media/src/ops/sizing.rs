//! The arithmetic of "make it fit": how many bits the picture may spend, and what to give up
//! when that is too few (spec 0.1, SIZE, and 4.2).
//!
//! `video_bps = (target_bytes * 0.98 * 8 / duration_s) - audio_bps`. The 0.98 reserves
//! container overhead, audio stays as it is when it is already small, and duration comes from
//! the probe, never from the file size. When the picture gets fewer bits than the codec's floor
//! (measured on the corpus, stored in `floors.toml`, see `docs/research/media-results.md`), the
//! resolution drops first, then the frame rate, and the plan says so. When even that is not
//! enough the job is refused with the nearest size that works, instead of producing mush.

use crate::encoders::VCodec;
use crate::error::{MediaError, Result};
use crate::units::{duration_text, size_text};

/// Share of the target kept for the container (index, interleave padding).
pub const OVERHEAD: f64 = 0.98;

/// Rungs of the resolution ladder, as the length of the shorter side in pixels.
pub const LADDER: &[u32] = &[2160, 1440, 1080, 720, 540, 480, 360, 240];

/// Bits per pixel per frame below which a codec's picture falls apart, and above which it stops
/// getting visibly better.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floor {
    /// Below this the picture looks like mush.
    pub floor: f64,
    /// A clean picture; "smallest that still looks good" aims here.
    pub knee: f64,
}

/// A sound track as the planner sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInfo {
    /// Stream index.
    pub index: u32,
    /// Codec name.
    pub codec: String,
    /// Channels.
    pub channels: u32,
    /// Bits per second, as stored.
    pub bps: u64,
    /// Marked as commentary or descriptive.
    pub commentary: bool,
}

/// What is known about the source.
#[derive(Debug, Clone)]
pub struct VideoInfo {
    /// Stored width.
    pub w: u32,
    /// Stored height.
    pub h: u32,
    /// Frames per second.
    pub fps: f64,
    /// Seconds.
    pub duration: f64,
    /// Sound tracks.
    pub audio: Vec<AudioInfo>,
}

/// What is wanted.
#[derive(Debug, Clone)]
pub struct Request {
    /// The most bytes the file may have.
    pub target_bytes: u64,
    /// The codec.
    pub codec: VCodec,
    /// The floors for this codec and encoder kind.
    pub floor: Floor,
    /// The tallest the shorter side may be (a preset's cap).
    pub max_short_side: Option<u32>,
    /// A frame rate the output must not exceed (0 for none).
    pub max_fps: f64,
}

/// What happens to one sound track.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioDecision {
    /// Stream index.
    pub index: u32,
    /// True to copy it as it is.
    pub copy: bool,
    /// Bits per second of the result.
    pub bps: u64,
}

/// The plan.
#[derive(Debug, Clone)]
pub struct Plan {
    /// Output width (stored pixels).
    pub w: u32,
    /// Output height (stored pixels).
    pub h: u32,
    /// Output frame rate, or `None` to keep the source's.
    pub fps: Option<f64>,
    /// Bits per second for the picture.
    pub video_bps: u64,
    /// The sound tracks that are kept.
    pub audio: Vec<AudioDecision>,
    /// Things to tell the user.
    pub notes: Vec<String>,
}

/// Bits per second for a sound track the planner keeps or re-encodes: stereo AAC or Opus at or
/// under 128 kb/s is copied, anything else becomes AAC at 96 kb/s stereo or 64 kb/s mono.
pub fn audio_decision(a: &AudioInfo) -> AudioDecision {
    let small = matches!(a.codec.as_str(), "aac" | "opus")
        && a.channels <= 2
        && a.bps > 0
        && a.bps <= 128_000;
    if small {
        AudioDecision {
            index: a.index,
            copy: true,
            bps: a.bps,
        }
    } else {
        AudioDecision {
            index: a.index,
            copy: false,
            bps: if a.channels <= 1 { 64_000 } else { 96_000 },
        }
    }
}

fn scaled(w: u32, h: u32, short: u32) -> (u32, u32) {
    let cur = w.min(h);
    if short >= cur {
        return (w, h);
    }
    let f = f64::from(short) / f64::from(cur);
    let even = |x: f64| ((x / 2.0).round() as u32 * 2).max(2);
    if w <= h {
        (short, even(f64::from(h) * f))
    } else {
        (even(f64::from(w) * f), short)
    }
}

fn needed(floor: f64, w: u32, h: u32, fps: f64) -> u64 {
    (floor * f64::from(w) * f64::from(h) * fps) as u64
}

/// A size rounded up to a figure a person would type: 1, 2, 5, 10, 25, 50, 100 MB and so on.
pub fn nice_size(bytes: u64) -> u64 {
    let mb = bytes as f64 / 1e6;
    let steps = [
        0.5, 1.0, 2.0, 3.0, 5.0, 8.0, 10.0, 15.0, 20.0, 25.0, 30.0, 50.0, 64.0, 100.0, 200.0,
        500.0, 1000.0,
    ];
    steps
        .iter()
        .find(|s| **s >= mb)
        .map(|s| (*s * 1e6) as u64)
        .unwrap_or(((mb / 100.0).ceil() * 100.0 * 1e6) as u64)
}

/// Plan the picture and sound for a size target.
pub fn plan_size(v: &VideoInfo, r: &Request) -> Result<Plan> {
    if v.duration <= 0.0 {
        return Err(MediaError::Corrupt("The video does not say how long it is, so a size target cannot be planned. Try Repair video first.".into()));
    }
    let mut notes = Vec::new();
    let mut audio: Vec<AudioDecision> = Vec::new();
    // Commentary tracks go first when a size target needs the bytes (spec AUD).
    let (main, commentary): (Vec<&AudioInfo>, Vec<&AudioInfo>) =
        v.audio.iter().partition(|a| !a.commentary);
    for a in &main {
        audio.push(audio_decision(a));
    }
    let budget = r.target_bytes as f64 * OVERHEAD * 8.0 / v.duration;
    let mut audio_bps: u64 = audio.iter().map(|a| a.bps).sum();
    if audio_bps as f64 > budget * 0.5 && audio.len() > 1 {
        // More than half the bits on sound: keep only the first track.
        let first = audio[0].clone();
        notes.push(format!(
            "{} extra sound track(s) were left out to leave room for the picture.",
            audio.len() - 1
        ));
        audio = vec![first];
        audio_bps = audio[0].bps;
    }
    if !commentary.is_empty() {
        notes.push(format!(
            "{} commentary track(s) were left out to fit the size.",
            commentary.len()
        ));
    }
    // Sound that takes more than 40% of the bits is squeezed: re-encoded at a speech-grade rate.
    if audio_bps as f64 > budget * 0.4 {
        for d in &mut audio {
            let ch = v
                .audio
                .iter()
                .find(|a| a.index == d.index)
                .map(|a| a.channels)
                .unwrap_or(2);
            d.copy = false;
            d.bps = if ch <= 1 { 32_000 } else { 48_000 };
        }
        audio_bps = audio.iter().map(|a| a.bps).sum();
        notes
            .push("The sound was re-encoded at a lower rate to leave room for the picture.".into());
    }
    let video_budget = budget - audio_bps as f64;
    let (mut w, mut h) = (v.w, v.h);
    let mut fps = v.fps.max(1.0);
    if let Some(cap) = r.max_short_side
        && w.min(h) > cap
    {
        (w, h) = scaled(w, h, cap);
        notes.push(format!(
            "Scaled down to {w} x {h}, the largest this service takes."
        ));
    }
    if r.max_fps > 0.0 && fps > r.max_fps + 0.5 {
        fps = r.max_fps;
    }
    let mut changed_fps = fps < v.fps - 0.01;
    if video_budget <= 0.0 {
        return Err(too_small(v, r, audio_bps));
    }
    // Resolution first.
    let start_short = w.min(h);
    let mut rungs: Vec<u32> = vec![start_short];
    rungs.extend(LADDER.iter().copied().filter(|x| *x < start_short));
    let mut fit = None;
    for short in &rungs {
        let (rw, rh) = scaled(w, h, *short);
        if (needed(r.floor.floor, rw, rh, fps) as f64) <= video_budget {
            fit = Some((rw, rh, fps));
            break;
        }
    }
    // Then frame rate.
    if fit.is_none() {
        let last = *rungs.last().unwrap_or(&start_short);
        let (rw, rh) = scaled(w, h, last);
        for f in [24.0, 15.0] {
            if f < fps && (needed(r.floor.floor, rw, rh, f) as f64) <= video_budget {
                fit = Some((rw, rh, f));
                changed_fps = true;
                break;
            }
        }
    }
    let Some((ow, oh, ofps)) = fit else {
        return Err(too_small(v, r, audio_bps));
    };
    if (ow, oh) != (v.w, v.h)
        && r.max_short_side
            .is_none_or(|c| ow.min(oh) < c.min(v.w.min(v.h)))
    {
        notes.push(format!(
            "At {} the picture would look like mush ({:.2} Mb/s for {} x {}), so it was scaled down to {ow} x {oh}.",
            size_text(r.target_bytes),
            video_budget / 1e6,
            v.w,
            v.h
        ));
    }
    if changed_fps && ofps < v.fps - 0.5 {
        notes.push(format!(
            "The frame rate was lowered to {ofps:.0} fps to keep the picture clean at this size."
        ));
    }
    Ok(Plan {
        w: ow,
        h: oh,
        fps: ((ofps - v.fps).abs() > 0.01).then_some(ofps),
        video_bps: video_budget.max(1.0) as u64,
        audio,
        notes,
    })
}

fn too_small(v: &VideoInfo, r: &Request, audio_bps: u64) -> MediaError {
    // The smallest picture the ladder allows, at 15 fps, plus the sound.
    let short = *LADDER.last().expect("the ladder has rungs");
    let (w, h) = scaled(v.w, v.h, short.min(v.w.min(v.h)));
    let fps = v.fps.clamp(1.0, 15.0);
    let min_bps = needed(r.floor.floor, w, h, fps) + audio_bps;
    let min_bytes = (min_bps as f64 * v.duration / 8.0 / OVERHEAD) as u64;
    let have = r.target_bytes as f64 * OVERHEAD * 8.0 / v.duration - audio_bps as f64;
    MediaError::Failed(format!(
        "{} at {} leaves {:.2} Mb/s for the picture, which would look like mush even at {} x {}. Raise the size to {}, or cut the video shorter ({} is what {} takes).",
        duration_text(v.duration),
        size_text(r.target_bytes),
        have.max(0.0) / 1e6,
        w,
        h,
        size_text(nice_size(min_bytes)),
        duration_text(v.duration * r.target_bytes as f64 / min_bytes.max(1) as f64),
        size_text(r.target_bytes)
    ))
}

/// The next bit rate to try after an attempt that came out at `actual` bytes when `target` was
/// the limit: scaled by `target / actual * 0.97`, as the spec says, capped so one step never
/// more than doubles or halves it.
pub fn retry_bps(prev_bps: u64, target: u64, actual: u64) -> u64 {
    let f = (target as f64 / actual.max(1) as f64 * 0.97).clamp(0.5, 2.0);
    (prev_bps as f64 * f).max(1.0) as u64
}

/// Bits per second for "smallest that still looks good" and for quality levels: the knee of the
/// codec's curve scaled by the quality (1 to 100, 50 sits at the knee), never above `ceiling`.
pub fn quality_bps(
    w: u32,
    h: u32,
    fps: f64,
    floor: Floor,
    quality: u8,
    ceiling: Option<u64>,
) -> u64 {
    let q = f64::from(quality.clamp(1, 100)) / 100.0;
    // From the floor at 1 to twice the knee at 100, passing through the knee at 50.
    let bpp = if q <= 0.5 {
        floor.floor + (floor.knee - floor.floor) * (q / 0.5)
    } else {
        floor.knee + (floor.knee * 1.0) * ((q - 0.5) / 0.5)
    };
    let bps = needed(bpp, w, h, fps.max(1.0));
    ceiling.map(|c| bps.min(c)).unwrap_or(bps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(w: u32, h: u32, fps: f64, duration: f64) -> VideoInfo {
        VideoInfo {
            w,
            h,
            fps,
            duration,
            audio: vec![AudioInfo {
                index: 1,
                codec: "aac".into(),
                channels: 2,
                bps: 128_000,
                commentary: false,
            }],
        }
    }

    fn req(bytes: u64) -> Request {
        Request {
            target_bytes: bytes,
            codec: VCodec::H264,
            floor: Floor {
                floor: 0.03,
                knee: 0.07,
            },
            max_short_side: None,
            max_fps: 0.0,
        }
    }

    #[test]
    fn the_size_formula_reserves_overhead_and_audio() {
        // 25 MB for 100 s: 0.98 * 25e6 * 8 / 100 = 1.96 Mb/s, minus 128 kb/s of copied audio.
        let p = plan_size(&video(1280, 720, 30.0, 100.0), &req(25_000_000)).unwrap();
        assert_eq!(
            p.video_bps,
            (25_000_000f64 * 0.98 * 8.0 / 100.0 - 128_000.0) as u64
        );
        assert_eq!(
            (p.w, p.h),
            (1280, 720),
            "nothing is scaled when the bits suffice"
        );
        assert!(p.audio[0].copy && p.audio[0].bps == 128_000);
    }

    #[test]
    fn heavy_audio_is_re_encoded_small() {
        let mut v = video(1280, 720, 30.0, 100.0);
        v.audio[0] = AudioInfo {
            index: 1,
            codec: "ac3".into(),
            channels: 6,
            bps: 448_000,
            commentary: false,
        };
        let p = plan_size(&v, &req(25_000_000)).unwrap();
        assert!(!p.audio[0].copy);
        assert_eq!(p.audio[0].bps, 96_000);
    }

    #[test]
    fn too_few_bits_lower_the_resolution_before_anything_else() {
        // 1080p30 for 60 s in 8 MB leaves about 0.9 Mb/s: the floor at 1080p is 1.9 Mb/s.
        let p = plan_size(&video(1920, 1080, 30.0, 60.0), &req(8_000_000)).unwrap();
        assert!(p.h < 1080 && p.w < 1920, "{}x{}", p.w, p.h);
        assert!(
            p.fps.is_none(),
            "the frame rate is kept while a lower resolution is enough"
        );
        assert!(
            p.notes.iter().any(|n| n.contains("scaled down")),
            "{:?}",
            p.notes
        );
        assert_eq!(p.w % 2, 0);
        assert_eq!(p.h % 2, 0);
        // The picture now has at least the floor's bits.
        assert!(p.video_bps as f64 >= 0.03 * f64::from(p.w) * f64::from(p.h) * 30.0);
    }

    #[test]
    fn portrait_video_scales_by_its_shorter_side() {
        let p = plan_size(&video(1080, 1920, 30.0, 60.0), &req(8_000_000)).unwrap();
        assert!(p.w < 1080 && p.h < 1920);
        assert!(p.w < p.h, "still portrait: {}x{}", p.w, p.h);
    }

    #[test]
    fn an_impossible_target_is_refused_with_the_nearest_workable_size() {
        let e = plan_size(&video(1920, 1080, 30.0, 30.0), &req(250_000))
            .unwrap_err()
            .to_string();
        assert!(e.contains("look like mush"), "{e}");
        assert!(e.contains("Raise the size to"), "{e}");
    }

    #[test]
    fn a_preset_cap_on_height_is_applied_and_said() {
        let mut r = req(100_000_000);
        r.max_short_side = Some(720);
        let p = plan_size(&video(1920, 1080, 30.0, 60.0), &r).unwrap();
        assert_eq!(p.h, 720);
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains("largest this service takes")),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn commentary_tracks_are_dropped_for_a_size_target() {
        let mut v = video(1280, 720, 30.0, 100.0);
        v.audio.push(AudioInfo {
            index: 2,
            codec: "aac".into(),
            channels: 2,
            bps: 96_000,
            commentary: true,
        });
        let p = plan_size(&v, &req(25_000_000)).unwrap();
        assert_eq!(p.audio.len(), 1);
        assert!(p.notes.iter().any(|n| n.contains("commentary")));
    }

    #[test]
    fn retries_scale_by_the_miss_with_a_margin() {
        // 10% over: scale by 1/1.1 * 0.97.
        let next = retry_bps(2_000_000, 10_000_000, 11_000_000);
        assert!((next as f64 - 2_000_000.0 * 10.0 / 11.0 * 0.97).abs() < 2.0);
        // Never more than a factor of two in one step.
        assert_eq!(retry_bps(1_000_000, 10_000_000, 1_000_000), 2_000_000);
    }

    #[test]
    fn quality_runs_from_the_floor_through_the_knee() {
        let f = Floor {
            floor: 0.03,
            knee: 0.07,
        };
        let lo = quality_bps(1920, 1080, 30.0, f, 1, None);
        let mid = quality_bps(1920, 1080, 30.0, f, 50, None);
        let hi = quality_bps(1920, 1080, 30.0, f, 100, None);
        assert!(lo < mid && mid < hi);
        assert_eq!(mid, (0.07 * 1920.0 * 1080.0 * 30.0) as u64);
        assert_eq!(
            quality_bps(1920, 1080, 30.0, f, 100, Some(5_000_000)),
            5_000_000
        );
    }

    #[test]
    fn nice_sizes_are_figures_a_person_types() {
        assert_eq!(nice_size(1_300_000), 2_000_000);
        assert_eq!(nice_size(9_000_000), 10_000_000);
        assert_eq!(nice_size(24_000_000), 25_000_000);
    }
}

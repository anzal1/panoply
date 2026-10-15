//! How good does the result look? VMAF (libvmaf, BSD-licensed) measured on the actual output
//! against the source, on a short sample from the middle so the cost is bounded. When the
//! helper has no libvmaf, the SSIM filter (always there) answers instead, and the card says
//! which it was.

use std::ffi::OsString;
use std::path::Path;

use super::hdr_to_sdr::filter_path;
use crate::engine::Engine;
use crate::error::Result;
use crate::io::{Io, tool_path};
use crate::proc;

/// A quality measurement.
#[derive(Debug, Clone, Copy)]
pub struct Quality {
    /// The score.
    pub score: f64,
    /// True for VMAF (0 to 100), false for SSIM (0 to 1).
    pub vmaf: bool,
    /// Seconds of video that were compared.
    pub seconds: f64,
}

impl Quality {
    /// "VMAF 93.4" or "SSIM 0.981".
    pub fn text(&self) -> String {
        if self.vmaf {
            format!("VMAF {:.1}", self.score)
        } else {
            format!("SSIM {:.3}", self.score)
        }
    }
}

/// Compare `out` with `src` on up to `window` seconds from the middle. `width` and `height` are
/// the source's stored size, so a scaled result is brought back up before comparing. Returns
/// `None` when the measurement cannot be made (the card then says nothing about quality).
#[allow(clippy::too_many_arguments)]
pub fn measure(
    engine: &Engine,
    io: &dyn Io,
    src: &Path,
    out: &Path,
    duration: f64,
    width: u32,
    height: u32,
    window: f64,
    log: &Path,
) -> Result<Option<Quality>> {
    let seconds = window.min(duration).max(0.5);
    let start = ((duration - seconds) / 2.0).max(0.0);
    let use_vmaf = engine.has_filter("libvmaf");
    // Each thread holds frames of float planes at the comparison size; eight of them on a 4K
    // picture cost 3 GB. Two keep the check under a second of extra time on a clip this short.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get().min(2))
        .unwrap_or(2);
    let sub = 3; // every third frame: enough for a stable mean, a third of the cost
    // Both pictures are compared at no more than 1080p, the size VMAF's model is made for. At the
    // source's own 4K the measurement held 3.5 GB resident for a 5 second clip.
    let (cw, ch) = compare_size(width, height);
    let filter = if use_vmaf {
        format!(
            "[0:v]scale={cw}:{ch}:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[d];[1:v]scale={cw}:{ch}:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[r];[d][r]libvmaf=n_threads={threads}:n_subsample={sub}:log_fmt=json:log_path={}",
            filter_path(log)
        )
    } else {
        format!(
            "[0:v]scale={cw}:{ch}:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[d];[1:v]scale={cw}:{ch}:flags=bicubic,format=yuv420p,setpts=PTS-STARTPTS[r];[d][r]ssim=stats_file=-"
        )
    };
    let args: Vec<OsString> = [
        "-ss".to_string(),
        format!("{start:.3}"),
        "-t".into(),
        format!("{seconds:.3}"),
        "-i".into(),
        tool_path(out),
        "-ss".into(),
        format!("{start:.3}"),
        "-t".into(),
        format!("{seconds:.3}"),
        "-i".into(),
        tool_path(src),
        "-lavfi".into(),
        filter,
        "-an".into(),
        "-f".into(),
        "null".into(),
        "-".into(),
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    let mut cmd_args: Vec<OsString> = ["-nostdin", "-hide_banner"]
        .iter()
        .map(OsString::from)
        .collect();
    cmd_args.extend(args);
    let done = proc::capture_ffmpeg(engine, io, &cmd_args)?;
    if !done.ok {
        return Ok(None);
    }
    let text = if use_vmaf {
        std::fs::read_to_string(log).unwrap_or_default()
    } else {
        String::from_utf8_lossy(&done.stdout).into_owned() + "\n" + &done.tail
    };
    Ok(if use_vmaf {
        parse_vmaf(&text, seconds)
    } else {
        parse_ssim(&text, seconds)
    })
}

/// The size both pictures are compared at: the source's own size up to 1080p worth of pixels, then
/// scaled down keeping the shape, to even numbers.
pub fn compare_size(width: u32, height: u32) -> (u32, u32) {
    const LIMIT: u64 = 1920 * 1080;
    let px = u64::from(width) * u64::from(height);
    if px <= LIMIT || width == 0 || height == 0 {
        return (width, height);
    }
    let f = (LIMIT as f64 / px as f64).sqrt();
    let even = |x: f64| ((x / 2.0).round() as u32 * 2).max(2);
    (even(f64::from(width) * f), even(f64::from(height) * f))
}

/// The pooled mean from libvmaf's JSON log. The per-frame list also has a "vmaf" key, so the
/// search starts at the pooled section.
pub fn parse_vmaf(text: &str, seconds: f64) -> Option<Quality> {
    // "pooled_metrics": { ..., "vmaf": { "min": .., "max": .., "mean": 93.1, ...
    let pooled = &text[text.find("\"pooled_metrics\"")?..];
    let rest = &pooled[pooled.find("\"vmaf\"")?..];
    let tail = &rest[rest.find("\"mean\"")? + 6..];
    let num: String = tail
        .trim_start_matches([' ', ':'])
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    let v: f64 = num.parse().ok()?;
    Some(Quality {
        score: v,
        vmaf: true,
        seconds,
    })
}

/// The `All:` value of the last SSIM summary line.
pub fn parse_ssim(text: &str, seconds: f64) -> Option<Quality> {
    let l = text
        .lines()
        .rev()
        .find(|l| l.contains("All:") && l.contains("SSIM"))?;
    let i = l.find("All:")?;
    let v: f64 = l[i + 4..].split_whitespace().next()?.parse().ok()?;
    Some(Quality {
        score: v,
        vmaf: false,
        seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn libvmaf_json_is_read() {
        let j = r#"{"version":"3.0.0","frames":[{"metrics":{"vmaf":99.0,"integer_motion2":1.0}}],"pooled_metrics":{"integer_motion2":{"min":0.0,"max":9.0,"mean":1.5},"vmaf":{"min":88.1,"max":97.0,"mean":93.25,"harmonic_mean":93.0}}}"#;
        let q = parse_vmaf(j, 20.0).unwrap();
        assert!((q.score - 93.25).abs() < 1e-9 && q.vmaf);
        assert_eq!(q.text(), "VMAF 93.2");
    }

    #[test]
    fn the_ssim_summary_is_read() {
        let l = "[Parsed_ssim_5 @ 0x1] SSIM Y:0.98 (17.1) U:0.99 (19.0) V:0.99 (19.2) All:0.981234 (17.2)";
        let q = parse_ssim(l, 5.0).unwrap();
        assert!((q.score - 0.981234).abs() < 1e-9 && !q.vmaf);
    }

    #[test]
    fn the_comparison_never_runs_above_1080p() {
        assert_eq!(compare_size(1280, 720), (1280, 720));
        assert_eq!(compare_size(1920, 1080), (1920, 1080));
        assert_eq!(compare_size(1080, 1920), (1080, 1920));
        assert_eq!(compare_size(3840, 2160), (1920, 1080));
        assert_eq!(compare_size(7680, 4320), (1920, 1080));
        let (w, h) = compare_size(4000, 1556);
        assert!(w % 2 == 0 && h % 2 == 0 && u64::from(w) * u64::from(h) <= 1920 * 1080 + 4000);
    }

    #[test]
    fn nothing_to_read_is_none() {
        assert!(parse_vmaf("no metrics here", 1.0).is_none());
        assert!(parse_ssim("", 1.0).is_none());
    }
}

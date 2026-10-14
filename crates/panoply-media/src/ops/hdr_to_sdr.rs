//! `media.hdr_to_sdr`: fix the washed-out look of an HDR video on screens that are not HDR
//! (spec 4.28).
//!
//! One pass, one table. Panoply builds a 3D lookup table ([`crate::hdr`]) that holds the whole
//! conversion (PQ or HLG decode, tone mapping in linear light, BT.2020 to BT.709 gamut mapping,
//! the BT.709 transfer curve) and the helper applies it with ffmpeg's `lut3d` filter. That works
//! with any LGPL ffmpeg build, with no libplacebo, zscale or Vulkan, and it never pumps because
//! the mapping is the same for every frame. The price: the mapping is static (peak brightness
//! comes from the file's metadata, not from scanning each scene), and Dolby Vision profile 5,
//! whose picture is only meaningful with its per-frame metadata, is refused with a plain
//! message instead of guessed.

use std::path::{Path, PathBuf};

use crate::encoders::{self, Rate, VCodec};
use crate::error::{MediaError, Result};
use crate::hdr::{Curve, Params, Transfer, build_lut};
use crate::io::{Io, Naming};
use crate::opts::Opts;
use crate::plan::{ARoute, SubRoute, Target, plan_convert};
use crate::probe::{Hdr, Profile, Stream};
use crate::units::{ext_of, name_of};

use super::convert::video_bitrate;
use super::{Args, Job, change_text, with_name};

/// Run `media.hdr_to_sdr`.
pub fn run(io: &dyn Io, inputs: &[PathBuf], opts: &Opts) -> Result<Vec<PathBuf>> {
    let curve = match opts.text("curve", "bt2390") {
        "hable" => Curve::Hable,
        "mobius" => Curve::Mobius,
        _ => Curve::Bt2390,
    };
    let peak = opts.f64("peak_nits").filter(|p| *p > 0.0);
    let protect = opts.flag("saturation_protection", true);
    let codec = if opts.text("codec", "h264") == "hevc" {
        VCodec::Hevc
    } else {
        VCodec::H264
    };
    let mut job = Job::new(io)?;
    let n = inputs.len();
    for (i, input) in inputs.iter().enumerate() {
        job.slice(i, n);
        one(&mut job, input, curve, peak, protect, codec)
            .map_err(|e| with_name(n > 1, input, e))?;
    }
    Ok(job.finish())
}

/// The transfer to decode with, or `None` when the stream is not HDR (or is Dolby Vision that
/// carries an SDR base layer).
pub fn source_transfer(v: &Stream) -> Result<Option<Transfer>> {
    Ok(match v.hdr {
        Hdr::Sdr => None,
        Hdr::Hdr10 => Some(Transfer::Pq),
        Hdr::Hlg => Some(Transfer::Hlg),
        Hdr::DolbyVision { profile: 5, .. } => {
            return Err(MediaError::Unsupported(
                "This is Dolby Vision profile 5. Its picture only looks right with the Dolby Vision data applied for every frame, and Panoply's converter cannot do that on this computer, so it would turn out purple and green. Open it in an app that supports Dolby Vision, or ask for a standard HDR copy."
                    .into(),
            ));
        }
        Hdr::DolbyVision { compat: 2, .. } => None,
        Hdr::DolbyVision { .. } => Some(if v.color_transfer == "arib-std-b67" {
            Transfer::Hlg
        } else {
            Transfer::Pq
        }),
    })
}

/// Build the lookup table for a stream and the filter chain that applies it: yuv in, planar RGB,
/// the table, then back to `pix` in BT.709 with matching tags. Returns the chain, the table's
/// temporary file (inside `dir`, removed with the job) and the source peak that was used.
#[allow(clippy::too_many_arguments)]
pub fn sdr_chain(
    job: &mut Job,
    dir: &Path,
    v: &Stream,
    transfer: Transfer,
    curve: Curve,
    peak: Option<f64>,
    protect: bool,
    pix: &str,
    post: Option<&str>,
) -> Result<(String, PathBuf, f64)> {
    if !job.engine.has_filter("lut3d") {
        return Err(MediaError::Engine(
            "The video engine on this computer has no lut3d filter, which HDR to SDR needs. Update the engine.".into(),
        ));
    }
    let source_peak = peak
        .or(v.max_cll)
        .or(v.max_luminance)
        .unwrap_or(1000.0)
        .clamp(300.0, 10_000.0);
    let mut params = Params::new(transfer);
    params.curve = curve;
    params.source_peak_nits = source_peak;
    params.saturation_protection = protect;
    let lut = build_lut(&params);
    let cube = job.stage.scratch(dir, "cube");
    std::fs::write(&cube, lut.to_cube("Panoply HDR to SDR"))?;
    let mut chain = format!(
        "scale=in_color_matrix=bt2020:in_range=tv:out_range=pc:flags=bicubic+accurate_rnd+full_chroma_int,format=gbrp16le,lut3d=file={}:interp=tetrahedral,scale=out_color_matrix=bt709:out_range=tv:flags=bicubic+accurate_rnd+full_chroma_int+error_diffusion,format={pix},setparams=colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv",
        filter_path(&cube)
    );
    if let Some(f) = post {
        chain.push(',');
        chain.push_str(f);
    }
    Ok((chain, cube, source_peak))
}

/// `file='path'` with the characters a filter graph treats specially escaped.
pub fn filter_path(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.replace(':', "\\:").replace('\'', "\\'");
    format!("'{s}'")
}

fn one(
    job: &mut Job,
    input: &Path,
    curve: Curve,
    peak: Option<f64>,
    protect: bool,
    codec: VCodec,
) -> Result<()> {
    let input = job.input(input)?;
    let p = job.probe(&input)?;
    let v = p
        .video()
        .ok_or_else(|| {
            MediaError::Unsupported(
                "This file has no picture, so there is nothing to convert.".into(),
            )
        })?
        .clone();
    let Some(transfer) = source_transfer(&v)? else {
        job.note(format!(
            "{} is already standard range, so nothing was changed.",
            name_of(&input)
        ));
        return Ok(());
    };
    let enc = encoders::pick(&job.engine, codec, false)
        .ok_or_else(|| encoders::none_for(codec, false))?;
    let target = if ext_of(&input) == "mov" {
        Target::Mov
    } else {
        Target::Mp4
    };
    let plan = plan_convert(&p, target, None);
    let (dir, part) = job.part(&input, target.ext())?;
    let ten = codec == VCodec::Hevc;
    let pix = enc.pix_fmt(ten);
    let (chain, cube, source_peak) = sdr_chain(
        job,
        &dir,
        &v,
        transfer,
        curve,
        peak,
        protect,
        pix,
        enc.post_filter.as_deref(),
    )?;
    let bps = video_bitrate(
        codec,
        enc.accel.is_hardware(),
        v.width,
        v.height,
        v.fps,
        v.bit_rate.or(p.bit_rate),
    );
    let mut a = Args::new();
    a.a("-y").a("-noautorotate");
    a.all(&enc.pre);
    a.input(&input);
    a.kv("-map_metadata", "0");
    a.kv("-map_chapters", "0");
    let main = v.index;
    a.kv("-map", format!("0:{main}"));
    a.kv("-vf", chain);
    a.all(enc.args(Rate::Bitrate(bps), ten, v.fps));
    a.kv("-colorspace", "bt709");
    a.kv("-color_primaries", "bt709");
    a.kv("-color_trc", "bt709");
    a.kv("-color_range", "tv");
    for (oa, (idx, route)) in plan.audio.iter().enumerate() {
        a.kv("-map", format!("0:{idx}"));
        match route {
            ARoute::Copy => {
                a.kv(&format!("-c:a:{oa}"), "copy");
            }
            ARoute::Encode(_) => {
                let s = p
                    .streams
                    .iter()
                    .find(|s| s.index == *idx)
                    .expect("planned stream exists");
                let ap = super::audio::plan_audio(
                    &job.engine,
                    s,
                    super::audio::AudioFormat::M4a,
                    &super::audio::AudioWish {
                        force_encode: true,
                        ..Default::default()
                    },
                )?;
                a.all(super::per_stream(&ap.args, 'a', oa));
            }
        }
    }
    let mut os = 0usize;
    for (idx, route) in &plan.subs {
        match route {
            SubRoute::Copy => {
                a.kv("-map", format!("0:{idx}"));
                a.kv(&format!("-c:s:{os}"), "copy");
                os += 1;
            }
            SubRoute::Convert(c) => {
                a.kv("-map", format!("0:{idx}"));
                a.kv(&format!("-c:s:{os}"), *c);
                os += 1;
            }
            SubRoute::Drop(_) => {}
        }
    }
    a.kv("-movflags", "+faststart");
    a.kv("-f", target.format());
    a.a(crate::io::tool_path(&part));
    job.ffmpeg(a, p.duration, "Converting HDR to SDR", (0.0, 1.0), &input)?;
    job.stage.discard(&cube);
    let out = job.commit(
        &part,
        &dir,
        &input,
        &Naming::Labelled("SDR".into()),
        target.ext(),
    )?;
    let after = std::fs::metadata(&out)?.len();
    job.verify(&out, p.duration)?;
    job.note(format!(
        "{}: {} converted to SDR (BT.709) with the {} curve for a {:.0} nit source, written as {} ({}).",
        name_of(&out),
        v.hdr.plain(),
        match curve {
            Curve::Bt2390 => "BT.2390",
            Curve::Hable => "Hable",
            Curve::Mobius => "Mobius",
        },
        source_peak,
        enc.label(),
        change_text(p.size, after)
    ));
    for n in plan
        .notes
        .iter()
        .filter(|n| !n.contains("The video is HDR"))
    {
        job.note(n.clone());
    }
    describe_dropped(job, &p);
    Ok(())
}

fn describe_dropped(job: &Job, p: &Profile) {
    let extra = p.of(crate::probe::Kind::Video).count().saturating_sub(1);
    if extra > 0 {
        job.note(format!(
            "{extra} extra video stream(s) were left out; only the main picture is converted."
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(hdr: Hdr, transfer: &str) -> Stream {
        let mut s = crate::probe::parse_profile(
            std::path::Path::new("a.mp4"),
            br#"{"streams":[{"index":0,"codec_type":"video","codec_name":"hevc"}],"format":{"format_name":"mp4"}}"#,
            1,
        )
        .unwrap()
        .streams
        .remove(0);
        s.hdr = hdr;
        s.color_transfer = transfer.into();
        s
    }

    #[test]
    fn the_transfer_follows_the_flavour() {
        assert_eq!(
            source_transfer(&stream(Hdr::Hdr10, "smpte2084")).unwrap(),
            Some(Transfer::Pq)
        );
        assert_eq!(
            source_transfer(&stream(Hdr::Hlg, "arib-std-b67")).unwrap(),
            Some(Transfer::Hlg)
        );
        assert_eq!(source_transfer(&stream(Hdr::Sdr, "bt709")).unwrap(), None);
        // iPhone Dolby Vision 8.4 has an HLG base layer.
        assert_eq!(
            source_transfer(&stream(
                Hdr::DolbyVision {
                    profile: 8,
                    compat: 4
                },
                "arib-std-b67"
            ))
            .unwrap(),
            Some(Transfer::Hlg)
        );
        assert_eq!(
            source_transfer(&stream(
                Hdr::DolbyVision {
                    profile: 8,
                    compat: 1
                },
                "smpte2084"
            ))
            .unwrap(),
            Some(Transfer::Pq)
        );
        assert_eq!(
            source_transfer(&stream(
                Hdr::DolbyVision {
                    profile: 8,
                    compat: 2
                },
                "bt709"
            ))
            .unwrap(),
            None
        );
    }

    #[test]
    fn dolby_vision_profile_5_is_refused_in_plain_words() {
        let e = source_transfer(&stream(
            Hdr::DolbyVision {
                profile: 5,
                compat: 0,
            },
            "smpte2084",
        ))
        .unwrap_err();
        assert!(e.to_string().contains("purple and green"), "{e}");
    }

    #[test]
    fn filter_paths_are_escaped_for_windows_drives() {
        let s = filter_path(std::path::Path::new(r"C:\Users\a b\x.cube"));
        assert_eq!(s, "'C\\:/Users/a b/x.cube'");
    }
}

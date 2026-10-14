//! OS decoders (ADR 0004: the OS first). On macOS this is ImageIO, through the
//! `panoply-platform-macos` crate: HEIC, HEIF, AVIF, JPEG XL, JPEG 2000, OpenEXR and camera RAW,
//! decoded in bands so a 200 megapixel HEIC never needs its full bitmap. Elsewhere nothing is
//! wired yet and these formats are refused with a plain message (see [`unavailable`]).

use super::{Input, OpenOpts, RowReader};
use crate::error::{ImageError, Result};
use crate::format::Format;

#[cfg(not(target_os = "macos"))]
pub(crate) fn open(_file: Input, format: Format, _opts: &OpenOpts) -> Result<Box<dyn RowReader>> {
    Err(unavailable(format))
}

/// The plain error for a format this system cannot decode (no OS decoder is wired here).
/// HEIC and HEIF get their own wording: the planned route is the Windows HEIF extension
/// through WIC where it is installed, otherwise a separate LGPL helper process, so the
/// message names the helper and what to do meanwhile.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(crate) fn unavailable(format: Format) -> ImageError {
    match format {
        Format::Heic => ImageError::Unsupported(
            "HEIC needs the HEIC helper on this system, and it is not installed. Export the photo as JPEG from the app that made it (on an iPhone, Settings, Camera, Formats, Most Compatible), or install the HEIC helper when it is available.".into(),
        ),
        other => ImageError::Unsupported(format!(
            "{} files need the operating system's decoder, and this system does not offer one yet. Convert the file to JPEG or PNG with another app first.",
            other.label()
        )),
    }
}

#[cfg(target_os = "macos")]
pub(crate) use mac::open;

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Arc;

    use panoply_platform_macos as pm;

    use super::*;
    use crate::color::{IccKind, classify_rgb_profile};
    use crate::decode::SrcInfo;
    use crate::pipeline::RowBuf;
    use crate::types::{Depth, Layout};

    struct OsReader {
        info: SrcInfo,
        dec: pm::Decoder,
        space: pm::Space,
        next: u32,
        wide: bool,
        alpha: bool,
        scratch8: Vec<u8>,
        scratch16: Vec<u16>,
    }

    fn map_err(e: pm::Error) -> ImageError {
        let m = e.message().to_string();
        if m.contains("too large") || m.contains("too many pixels") {
            ImageError::TooLarge(format!(
                "{m} Make the picture smaller with another tool first, or export a smaller version."
            ))
        } else if m.contains("damaged")
            || m.contains("cut short")
            || m.contains("no readable image")
        {
            ImageError::Corrupt(format!(
                "{m} Try opening it in another app, or export it again from the original."
            ))
        } else {
            ImageError::Unsupported(format!("{m} Convert the file with another app first."))
        }
    }

    pub(crate) fn open(
        input: Input,
        format: Format,
        opts: &OpenOpts,
    ) -> Result<Box<dyn RowReader>> {
        let data = input.into_vec()?;
        // A caller that shrinks the picture says how small a result it can use; the OS then
        // decodes at about twice that size (like the JPEG decoder's shrink on load) instead of
        // developing the whole picture, which is what a 33 MP 10 bit HEIF or a RAW costs.
        let reduce = opts.min_size_hint.map(|(hw, hh)| pm::Reduce {
            min_short: hw.min(hh).saturating_mul(2),
            min_long: hw.max(hh).saturating_mul(2),
        });
        let dec = pm::Decoder::open_reduced(data, reduce).map_err(map_err)?;
        let pi = dec.info().clone();
        let (w, h) = dec.oriented_size();
        let (full_w, full_h) = dec.full_oriented_size();
        let wide = pi.bits_per_component > 8;
        let alpha = pi.has_alpha;
        let layout = if alpha { Layout::Rgba } else { Layout::Rgb };

        // Choose the color space the OS draws into: sRGB, or the file's own wide gamut profile
        // when the caller keeps color and the profile has the sRGB curve (Display P3).
        let mut space = pm::Space::Srgb;
        let mut managed_icc = None;
        let mut label = pi.colorspace_name.clone();
        if opts.keep_color
            && let Some(icc) = &pi.icc
            && classify_rgb_profile(icc)
                .map(|k| k == IccKind::WideGamut)
                .unwrap_or(false)
        {
            space = pm::Space::Source;
            managed_icc = Some(Arc::new(icc.clone()));
        }
        if label.is_none() && pi.icc.is_some() {
            label = Some("embedded profile".into());
        }
        let mut info = SrcInfo::plain(
            format,
            w,
            h,
            layout,
            if wide { Depth::U16 } else { Depth::U8 },
        );
        info.full_width = full_w;
        info.full_height = full_h;
        info.upright = true;
        info.orientation = 1;
        info.color_managed = true;
        info.managed_icc = managed_icc;
        info.premultiplied = alpha;
        info.frames = pi.frames;
        if pi.frames > 1 && !pi.is_animated {
            // A multi-image file (burst, depth plus photo): the primary picture is the job.
            info.frames = 1;
        }
        info.has_alpha = alpha;
        info.color_label = label;
        if pi.has_depth {
            info.dropped.push("depth map".into());
        }
        if pi.has_gain_map {
            info.dropped
                .push("HDR gain map (the standard dynamic range picture was used)".into());
        }
        if pi.has_portrait_matte {
            info.dropped.push("portrait matte".into());
        }
        Ok(Box::new(OsReader {
            info,
            dec,
            space,
            next: 0,
            wide,
            alpha,
            scratch8: Vec::new(),
            scratch16: Vec::new(),
        }))
    }

    impl RowReader for OsReader {
        fn info(&self) -> &SrcInfo {
            &self.info
        }

        fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
            let left = self.info.height - self.next;
            let n = (max_rows as u32).min(left).min(256);
            if n == 0 {
                return Ok(0);
            }
            let w = self.info.width as usize;
            match out {
                RowBuf::U8(d) => {
                    self.scratch8.resize(n as usize * w * 4, 0);
                    self.dec
                        .read_rows_u8(0, self.next, n, self.space, &mut self.scratch8)
                        .map_err(map_err)?;
                    d.clear();
                    if self.alpha {
                        d.extend_from_slice(&self.scratch8);
                    } else {
                        d.extend(
                            self.scratch8
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .flat_map(|p| [p[0], p[1], p[2]]),
                        );
                    }
                }
                RowBuf::U16(d) => {
                    self.scratch16.resize(n as usize * w * 4, 0);
                    self.dec
                        .read_rows_u16(0, self.next, n, self.space, &mut self.scratch16)
                        .map_err(map_err)?;
                    d.clear();
                    if self.alpha {
                        d.extend_from_slice(&self.scratch16);
                    } else {
                        d.extend(
                            self.scratch16
                                .as_chunks::<4>()
                                .0
                                .iter()
                                .flat_map(|p| [p[0], p[1], p[2]]),
                        );
                    }
                }
            }
            let _ = self.wide;
            self.next += n;
            Ok(n as usize)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heic_without_an_os_decoder_names_the_helper_and_the_way_out() {
        let m = unavailable(Format::Heic).to_string();
        assert!(
            m.contains("HEIC needs the HEIC helper on this system"),
            "{m}"
        );
        assert!(m.contains("Most Compatible"), "{m}");
        let m = unavailable(Format::Raw).to_string();
        assert!(m.contains("RAW") && m.contains("JPEG or PNG"), "{m}");
    }
}

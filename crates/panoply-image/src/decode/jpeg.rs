//! JPEG through MozJPEG's decoder (libjpeg-turbo with NEON or SSE2): streams scanlines, can
//! decode at 1/8 to 16/8 size, reads CMYK and YCCK, and hands back the ICC profile and the
//! EXIF orientation.
//!
//! MozJPEG reports a damaged file by unwinding, so every call that can fail runs inside
//! `catch_unwind` and turns the panic into a plain error. The workspace builds with
//! `panic = "unwind"` for exactly this reason.

use std::io::{BufReader, Read, Seek, SeekFrom};
use std::panic::{AssertUnwindSafe, catch_unwind};

use mozjpeg::{ColorSpace, Decompress, Marker};

use super::{Input, OpenOpts, RowReader, SrcInfo};
use crate::error::{ImageError, Result, corrupt};
use crate::format::Format;
use crate::orient::orientation_from_exif;
use crate::pipeline::RowBuf;
use crate::types::{Depth, Layout};

type Started = mozjpeg::decompress::DecompressStarted<BufReader<Input>>;

pub(crate) struct JpegReader {
    info: SrcInfo,
    dec: Option<Started>,
    rows_done: usize,
    channels: usize,
}

fn guard<T>(what: &str, f: impl FnOnce() -> std::io::Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(corrupt(format!("{what}: {e}"))),
        Err(p) => {
            let msg = p
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "decoder error".into());
            Err(corrupt(format!(
                "{what}: {}",
                msg.trim_start_matches("libjpeg fatal error: ")
            )))
        }
    }
}

/// A JPEG that stops before its end marker is cut short: libjpeg would quietly fill the rest
/// with gray, so check the tail ourselves.
fn is_truncated(file: &mut Input) -> std::io::Result<bool> {
    let len = file.len()?;
    let tail = len.min(4096);
    file.seek(SeekFrom::Start(len - tail))?;
    let mut buf = vec![0u8; tail as usize];
    file.read_exact(&mut buf)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(!buf.windows(2).any(|w| w == [0xFF, 0xD9]))
}

impl JpegReader {
    pub(crate) fn open(mut file: Input, opts: &OpenOpts) -> Result<Self> {
        if is_truncated(&mut file)? {
            return Err(ImageError::Corrupt(
                "This JPEG is incomplete: the file ends before the picture does. Download or export it again from the original.".into(),
            ));
        }
        let markers = [Marker::APP(1), Marker::APP(2), Marker::APP(14)];
        let dec = guard("reading the JPEG header", || {
            Decompress::with_markers(&markers).from_reader(BufReader::with_capacity(1 << 16, file))
        })?;
        let (w, h) = dec.size();
        if w == 0 || h == 0 {
            return Err(corrupt("zero size"));
        }
        let mut exif: Option<Vec<u8>> = None;
        let mut icc_parts: Vec<(u8, Vec<u8>)> = Vec::new();
        let mut adobe = false;
        for m in dec.markers() {
            match m.marker {
                Marker::APP(1) if m.data.starts_with(b"Exif\0\0") && exif.is_none() => {
                    exif = Some(m.data[6..].to_vec());
                }
                Marker::APP(2) if m.data.starts_with(b"ICC_PROFILE\0") && m.data.len() > 14 => {
                    icc_parts.push((m.data[12], m.data[14..].to_vec()));
                }
                Marker::APP(14) if m.data.starts_with(b"Adobe") => adobe = true,
                _ => {}
            }
        }
        icc_parts.sort_by_key(|p| p.0);
        let icc = (!icc_parts.is_empty())
            .then(|| icc_parts.into_iter().flat_map(|p| p.1).collect::<Vec<u8>>());
        let orientation = exif.as_deref().map(orientation_from_exif).unwrap_or(1);
        let cs = dec.color_space();
        let (layout, cmyk) = match cs {
            ColorSpace::JCS_GRAYSCALE => (Layout::Gray, false),
            ColorSpace::JCS_CMYK | ColorSpace::JCS_YCCK => (Layout::Rgba, true),
            ColorSpace::JCS_UNKNOWN => return Err(corrupt("unknown color space")),
            _ => (Layout::Rgb, false),
        };
        let channels = if cmyk { 4 } else { layout.channels() };
        let mut dec = dec;
        // Decoder side shrink: scale n/8 with the smallest n that keeps the picture at least
        // twice the requested size, so the final Lanczos pass still does the fine work.
        let mut scale_n = 8u8;
        if let Some((hw, hh)) = opts.min_size_hint {
            // Orientation is not known to the caller, so compare sorted sides.
            let (hs, hl) = (hw.min(hh) as usize * 2, hw.max(hh) as usize * 2);
            for n in 1..8u8 {
                let sw = (w * n as usize).div_ceil(8);
                let sh = (h * n as usize).div_ceil(8);
                if sw.min(sh) >= hs && sw.max(sh) >= hl {
                    scale_n = n;
                    break;
                }
            }
            if scale_n != 8 {
                guard("setting the decode size", || {
                    dec.scale(scale_n);
                    Ok(())
                })?;
            }
        }
        let started = guard("starting the JPEG decode", || {
            if cmyk {
                dec.to_colorspace(ColorSpace::JCS_CMYK)
            } else if layout == Layout::Gray {
                dec.grayscale()
            } else {
                dec.rgb()
            }
        })
        .map_err(|e| {
            if e.to_string().contains("arithmetic coding") {
                ImageError::Unsupported(
                    "This JPEG uses arithmetic coding, a rare variant that Panoply cannot read yet. Save it again as a standard JPEG or as PNG with another app first.".into(),
                )
            } else {
                e
            }
        })?;
        let (ow, oh) = (started.width() as u32, started.height() as u32);
        let mut info = SrcInfo::plain(Format::Jpeg, ow, oh, layout, Depth::U8);
        info.full_width = w as u32;
        info.full_height = h as u32;
        info.orientation = orientation;
        info.color.icc = icc;
        info.color.cmyk = cmyk;
        info.color.cmyk_inverted = cmyk && adobe;
        info.has_alpha = false;
        if cmyk {
            info.color_label = Some("CMYK".into());
        }
        Ok(Self {
            info,
            dec: Some(started),
            rows_done: 0,
            channels,
        })
    }
}

impl RowReader for JpegReader {
    fn info(&self) -> &SrcInfo {
        &self.info
    }

    fn read_rows(&mut self, max_rows: usize, out: &mut RowBuf) -> Result<usize> {
        let RowBuf::U8(buf) = out else {
            return Err(ImageError::Failed("internal: JPEG rows are 8 bit".into()));
        };
        let left = self.info.height as usize - self.rows_done;
        let n = max_rows.min(left);
        if n == 0 {
            return Ok(0);
        }
        let row_len = self.info.width as usize * self.channels;
        buf.clear();
        buf.resize(n * row_len, 0);
        let dec = self.dec.as_mut().expect("reader is open");
        guard("decoding the JPEG", || {
            dec.read_scanlines_into(&mut buf[..])?;
            Ok(())
        })?;
        self.rows_done += n;
        if self.rows_done == self.info.height as usize
            && let Some(d) = self.dec.take()
        {
            let _ = catch_unwind(AssertUnwindSafe(|| d.finish()));
        }
        Ok(n)
    }
}

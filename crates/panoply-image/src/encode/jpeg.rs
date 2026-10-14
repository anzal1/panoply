//! JPEG output through MozJPEG: trellis quantization, optimized Huffman tables and progressive
//! scans for the smallest files, or plain baseline with fixed tables when rows must stream.
//!
//! Like the decoder, every MozJPEG call runs under `catch_unwind`.

use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use mozjpeg::{ColorSpace, Compress, Marker};

use super::Embed;
use crate::error::{ImageError, Result};
use crate::pipeline::RowSink;
use crate::types::{Depth, Image, Layout, Rows};

/// JPEG settings.
#[derive(Debug, Clone)]
pub struct JpegOpts {
    /// 1 to 100.
    pub quality: f32,
    /// Progressive scans (smaller files, the MozJPEG default). Off for baseline-only portals.
    pub progressive: bool,
    /// Chroma 4:2:0 (smaller). Off gives 4:4:4 (sharper color edges, for text and UI).
    pub subsample: bool,
    /// Two-pass Huffman optimization. It needs the whole picture in memory, so streaming
    /// callers turn it off.
    pub optimize: bool,
    /// With `progressive` and `optimize` both off: use MozJPEG's quantization tables (the ones
    /// behind its small files) instead of the libjpeg ones. Streams like libjpeg does.
    pub mozjpeg_tables: bool,
    /// With `progressive` off: keep MozJPEG's trellis quantization (the part of its saving that
    /// does not need a coefficient buffer) instead of the plain libjpeg quantizer. With
    /// `optimize` off too the encode still streams, using the standard Huffman tables; the
    /// tables can then be optimized afterwards without a coefficient buffer by
    /// [`super::jpegopt::optimize_huffman`].
    pub trellis: bool,
    /// Metadata to embed.
    pub embed: Embed,
}

impl Default for JpegOpts {
    fn default() -> Self {
        Self {
            quality: 85.0,
            progressive: true,
            subsample: true,
            optimize: true,
            mozjpeg_tables: false,
            trellis: false,
            embed: Embed::default(),
        }
    }
}

/// Largest side a JPEG can have.
pub const MAX_SIDE: u32 = 65535;

fn guard<T>(f: impl FnOnce() -> std::io::Result<T>) -> Result<T> {
    let fail = |m: String| {
        ImageError::Failed(format!(
            "The JPEG could not be written ({m}). Try again, or save as PNG or WebP."
        ))
    };
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(fail(e.to_string())),
        Err(p) => Err(fail(
            p.downcast_ref::<String>()
                .cloned()
                .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "encoder error".into()),
        )),
    }
}

/// A [`RowSink`] that writes a JPEG to `W` as rows arrive.
pub struct JpegSink<W: Write> {
    opts: JpegOpts,
    out: Option<W>,
    started: Option<mozjpeg::compress::CompressStarted<W>>,
    channels: usize,
}

impl<W: Write> JpegSink<W> {
    /// A sink that will write to `out` with these settings.
    pub fn new(out: W, opts: JpegOpts) -> Self {
        Self {
            opts,
            out: Some(out),
            started: None,
            channels: 3,
        }
    }

    /// Finish the file and give back the writer.
    pub fn finish(mut self) -> Result<W> {
        let started = self.started.take().ok_or_else(|| {
            ImageError::Failed("internal: JPEG finished before it started".into())
        })?;
        guard(|| started.finish())
    }
}

impl<W: Write> RowSink for JpegSink<W> {
    fn begin(
        &mut self,
        width: u32,
        height: u32,
        layout: Layout,
        depth: Depth,
        _icc: Option<Arc<Vec<u8>>>,
    ) -> Result<()> {
        if depth != Depth::U8 || layout.has_alpha() {
            return Err(ImageError::Failed(
                "internal: JPEG needs 8 bit rows without alpha".into(),
            ));
        }
        if width > MAX_SIDE || height > MAX_SIDE {
            return Err(ImageError::TooLarge(format!(
                "JPEG files cannot be larger than {MAX_SIDE} pixels on a side, and this picture is {width} x {height}. Use WebP (up to 16383) or PNG, or make the picture smaller."
            )));
        }
        let (cs, ch) = if layout == Layout::Gray {
            (ColorSpace::JCS_GRAYSCALE, 1)
        } else {
            (ColorSpace::JCS_RGB, 3)
        };
        self.channels = ch;
        let opts = self.opts.clone();
        let out = self.out.take().expect("begin is called once");
        let started = guard(move || {
            let mut c = Compress::new(cs);
            if !opts.progressive && opts.trellis {
                // MozJPEG's default profile (trellis, deringing) with the progressive script
                // switched off: baseline scans, and Huffman tables optimized only on request.
                c.set_optimize_scans(false);
                c.set_optimize_coding(opts.optimize);
            } else if !opts.progressive && !opts.optimize {
                // libjpeg-turbo behaviour: baseline, standard tables, streams with no extra memory.
                c.set_fastest_defaults();
            } else if !opts.progressive {
                c.set_fastest_defaults();
                c.set_optimize_coding(true);
            }
            c.set_size(width as usize, height as usize);
            if !opts.progressive && !opts.optimize && !opts.trellis && opts.mozjpeg_tables {
                let q = opts.quality.clamp(1.0, 100.0);
                let t = mozjpeg::qtable::NRobidoux.scaled(q, q);
                c.set_luma_qtable(&t);
                c.set_chroma_qtable(&t);
            } else {
                // libjpeg takes whole quality steps, which can change the size by several percent
                // near the top. A fractional quality is honoured by scaling MozJPEG's own luma
                // table by hand, so a size target can land close under its limit.
                let q = opts.quality.clamp(1.0, 100.0);
                c.set_quality(q.floor());
                if q.fract() > 0.01
                    && q < 100.0
                    && (opts.progressive || opts.optimize || opts.trellis)
                {
                    c.set_luma_qtable(&mozjpeg::qtable::NRobidoux.scaled(q, q));
                }
            }
            if ch == 3 {
                if opts.subsample {
                    c.set_chroma_sampling_pixel_sizes((2, 2), (2, 2));
                } else {
                    c.set_chroma_sampling_pixel_sizes((1, 1), (1, 1));
                }
            }
            let mut s = c.start_compress(out)?;
            if let Some(icc) = &opts.embed.icc {
                s.write_icc_profile(icc);
            }
            if let Some(exif) = &opts.embed.exif
                && exif.len() + 8 <= 65535
            {
                let mut seg = b"Exif\0\0".to_vec();
                seg.extend_from_slice(exif);
                s.write_marker(Marker::APP(1), &seg);
            }
            if let Some(xmp) = &opts.embed.xmp {
                const NS: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
                if xmp.len() + NS.len() + 2 <= 65535 {
                    let mut seg = NS.to_vec();
                    seg.extend_from_slice(xmp);
                    s.write_marker(Marker::APP(1), &seg);
                }
            }
            Ok(s)
        })?;
        self.started = Some(started);
        Ok(())
    }

    fn push(&mut self, rows: Rows<'_>) -> Result<()> {
        let Rows::U8(s) = rows else {
            return Err(ImageError::Failed("internal: JPEG needs 8 bit rows".into()));
        };
        let started = self.started.as_mut().expect("begin was called");
        guard(|| started.write_scanlines(s))
    }
}

/// A JPEG encoder on its own thread: rows are handed over as they arrive from the decoder and
/// encoded while the decoder works on the next ones, so a conversion takes as long as the slower
/// of the two instead of the sum. The file is the same one [`JpegSink`] writes.
pub struct JpegThread {
    tx: Option<std::sync::mpsc::SyncSender<Vec<u8>>>,
    handle: Option<std::thread::JoinHandle<Result<Vec<u8>>>>,
}

impl JpegThread {
    /// Start encoding a `width` by `height` picture with these settings.
    pub fn start(opts: JpegOpts, width: u32, height: u32, layout: Layout) -> JpegThread {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(4);
        let handle = std::thread::spawn(move || -> Result<Vec<u8>> {
            let mut sink = JpegSink::new(
                Vec::with_capacity(width as usize * height as usize / 8 + 1024),
                opts,
            );
            sink.begin(width, height, layout, Depth::U8, None)?;
            for rows in rx {
                sink.push(Rows::U8(&rows))?;
            }
            sink.finish()
        });
        JpegThread {
            tx: Some(tx),
            handle: Some(handle),
        }
    }

    /// Hand over the next rows (top to bottom, 8 bit, no alpha).
    pub fn push(&mut self, rows: &[u8]) -> Result<()> {
        let sent = self
            .tx
            .as_ref()
            .is_some_and(|tx| tx.send(rows.to_vec()).is_ok());
        if sent {
            Ok(())
        } else {
            // The encoder stopped: its own error says why.
            self.tx = None;
            match self.handle.take().map(|h| h.join()) {
                Some(Ok(Err(e))) => Err(e),
                _ => Err(ImageError::Failed(
                    "internal: the JPEG encoder stopped".into(),
                )),
            }
        }
    }

    /// Wait for the encoder and take the file.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.tx = None;
        match self.handle.take().map(|h| h.join()) {
            Some(Ok(r)) => r,
            _ => Err(ImageError::Failed(
                "internal: the JPEG encoder stopped".into(),
            )),
        }
    }
}

/// Encode a whole 8 bit image without alpha.
pub fn encode(img: &Image, opts: &JpegOpts) -> Result<Vec<u8>> {
    let mut sink = JpegSink::new(Vec::with_capacity(img.byte_size() / 8 + 1024), opts.clone());
    sink.begin(img.width, img.height, img.layout, img.depth(), None)?;
    // Feed in modest slices so the encoder's input copy stays small.
    let step = (1 << 20) / img.row_len().max(1) + 1;
    let mut y = 0;
    while y < img.height {
        let n = (step as u32).min(img.height - y);
        sink.push(img.rows(y, n))?;
        y += n;
    }
    sink.finish()
}

/// Insert COM segments before the first scan so the file reaches `target` bytes (portals that
/// enforce a minimum size). Returns the input unchanged when it is already big enough.
pub fn pad_with_comments(jpeg: &[u8], target: usize) -> Vec<u8> {
    if jpeg.len() >= target {
        return jpeg.to_vec();
    }
    // Find the first SOS marker by walking the segments.
    let mut i = 2;
    let mut sos = None;
    while i + 4 <= jpeg.len() {
        if jpeg[i] != 0xFF {
            break;
        }
        let m = jpeg[i + 1];
        if m == 0xDA {
            sos = Some(i);
            break;
        }
        if m == 0xD8 || (0xD0..=0xD7).contains(&m) || m == 0x01 || m == 0xFF {
            i += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        let len = u16::from_be_bytes([jpeg[i + 2], jpeg[i + 3]]) as usize;
        i += 2 + len;
    }
    let Some(at) = sos else { return jpeg.to_vec() };
    let mut need = target - jpeg.len();
    let mut out = Vec::with_capacity(target);
    out.extend_from_slice(&jpeg[..at]);
    while need > 0 {
        // A segment costs 4 bytes of header plus its data.
        let data = need.saturating_sub(4).clamp(1, 65533);
        out.extend_from_slice(&[0xFF, 0xFE]);
        out.extend_from_slice(&((data + 2) as u16).to_be_bytes());
        out.extend(std::iter::repeat_n(b' ', data));
        need = need.saturating_sub(data + 4);
    }
    out.extend_from_slice(&jpeg[at..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32, layout: Layout) -> Image {
        let mut img = Image::new(w, h, layout, Depth::U8);
        let ch = layout.channels();
        if let crate::types::Samples::U8(v) = &mut img.samples {
            for y in 0..h as usize {
                for x in 0..w as usize {
                    for c in 0..ch {
                        v[(y * w as usize + x) * ch + c] =
                            ((x * 255 / w as usize) as u8).wrapping_add((y * 3 + c * 40) as u8);
                    }
                }
            }
        }
        img
    }

    fn marker_present(data: &[u8], m: u8) -> bool {
        data.windows(2).any(|w| w == [0xFF, m])
    }

    #[test]
    fn progressive_and_baseline_are_what_they_say() {
        let img = gradient(96, 64, Layout::Rgb);
        let prog = encode(
            &img,
            &JpegOpts {
                progressive: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(marker_present(&prog, 0xC2), "progressive SOF2 expected");
        let base = encode(
            &img,
            &JpegOpts {
                progressive: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            marker_present(&base, 0xC0) && !marker_present(&base, 0xC2),
            "baseline SOF0 expected"
        );
    }

    #[test]
    fn lower_quality_is_smaller() {
        let img = gradient(128, 96, Layout::Rgb);
        let hi = encode(
            &img,
            &JpegOpts {
                quality: 95.0,
                ..Default::default()
            },
        )
        .unwrap();
        let lo = encode(
            &img,
            &JpegOpts {
                quality: 40.0,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(lo.len() < hi.len());
    }

    #[test]
    fn gray_encodes() {
        let img = gradient(40, 40, Layout::Gray);
        let j = encode(&img, &JpegOpts::default()).unwrap();
        assert!(j.starts_with(&[0xFF, 0xD8]));
    }

    #[test]
    fn embeds_icc_and_exif() {
        let img = gradient(32, 32, Layout::Rgb);
        let mut o = JpegOpts::default();
        o.embed.icc = Some(vec![7u8; 500]);
        o.embed.exif = Some(b"II*\0\x08\0\0\0\0\0\0\0\0\0".to_vec());
        let j = encode(&img, &o).unwrap();
        assert!(j.windows(12).any(|w| w == b"ICC_PROFILE\0"));
        assert!(j.windows(6).any(|w| w == b"Exif\0\0"));
    }

    #[test]
    fn padding_reaches_the_target_exactly() {
        let img = gradient(32, 32, Layout::Rgb);
        let j = encode(&img, &JpegOpts::default()).unwrap();
        let target = j.len() + 10_000;
        let padded = pad_with_comments(&j, target);
        assert_eq!(padded.len(), target);
        let j2 = encode(&img, &JpegOpts::default()).unwrap();
        assert_eq!(pad_with_comments(&j2, 10).len(), j2.len());
    }

    #[test]
    fn too_big_is_refused_plainly() {
        let mut s = JpegSink::new(Vec::new(), JpegOpts::default());
        let e = s
            .begin(70000, 10, Layout::Rgb, Depth::U8, None)
            .unwrap_err();
        assert!(e.to_string().contains("65535"));
    }
}

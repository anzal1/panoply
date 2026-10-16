//! First previews: a small picture of an image file without decoding the whole picture where
//! the format allows it (docs/FLOWS.md budgets: the first preview in under 100 ms).
//!
//! What each format offers, cheapest first:
//!
//! | format | route | cost |
//! |---|---|---|
//! | JPEG | the EXIF thumbnail (`fast`), else the decoder's own 1/2, 1/4 or 1/8 DCT scaling | milliseconds, then proportional to the compressed size |
//! | HEIC, HEIF | the file's `thmb` thumbnail (`fast`), else a reduced decode by the system | about 75 ms for a 200 MP file, then the decode at the reduced size |
//! | camera RAW | the preview the file carries (every RAW has one), never a develop | about 100 ms |
//! | PNG, WebP, GIF, BMP, TIFF, AVIF, JPEG XL, JPEG 2000, EXR | the whole picture is read, rows streaming into a resampler | the decode time of the format |
//!
//! A preview with `fast` set may be smaller than asked for ([`Preview::refine`] says so) and is
//! meant to be shown at once and replaced when a sharper one arrives.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

use crate::color::OutSpace;
use crate::decode::{self, Input, OpenOpts};
use crate::error::{ImageError, Result};
use crate::format::{self, Format};
use crate::pipeline::{Ctl, ImagePlan, decode_to_image, quiet};
use crate::resample::Filter;
use crate::types::{Image, Layout, Samples};

/// Where a preview came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// The thumbnail the file carries (EXIF, HEIF `thmb`, a RAW's preview).
    Embedded,
    /// The picture decoded at a reduced size (DCT scaling, or the system's reduced decode).
    Reduced,
    /// The whole picture was read and shrunk.
    Full,
}

/// A small upright picture.
#[derive(Debug, Clone)]
pub struct Preview {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes of straight (not premultiplied) RGBA, in sRGB.
    pub rgba: Vec<u8>,
    /// How it was made.
    pub route: Route,
    /// True when a sharper preview is available for more work: this one is smaller than asked
    /// for (an embedded thumbnail) while the picture is larger.
    pub refine: bool,
}

/// A preview of `file` at most `max_side` pixels on the long side. `fast` accepts the file's own
/// thumbnail at any size, for a first look that is replaced later; without it the preview is as
/// large as asked for (up to the picture's own size).
pub fn preview(
    mut file: File,
    ext_hint: Option<&str>,
    max_side: u32,
    fast: bool,
) -> Result<Preview> {
    let max_side = max_side.clamp(16, 8192);
    let mut head = [0u8; 64];
    let n = file.read(&mut head)?;
    file.seek(SeekFrom::Start(0))?;
    let fmt = format::detect(&head[..n], ext_hint)
        .ok_or_else(|| ImageError::Unsupported(crate::format::not_an_image_message(&head[..n])))?;
    match fmt {
        Format::Jpeg => jpeg(file, max_side, fast),
        Format::Heic | Format::Avif | Format::Jxl | Format::Jp2 | Format::Raw | Format::Exr => {
            os(file, fmt, max_side, fast)
        }
        _ => decoded(file, ext_hint, max_side, Route::Full, None),
    }
}

fn jpeg(mut file: File, max_side: u32, fast: bool) -> Result<Preview> {
    if fast {
        // The EXIF block is in the first segments; 256 KB covers any real one.
        let mut head = vec![0u8; 256 * 1024];
        let n = file.read(&mut head)?;
        head.truncate(n);
        file.seek(SeekFrom::Start(0))?;
        if let Some(exif) = crate::meta::extract_exif(&head)
            && let Some(thumb) = crate::meta::exif_thumbnail(&exif)
        {
            let orientation = crate::orient::orientation_from_exif(&exif);
            if let Ok(p) =
                decode_bytes(thumb.to_vec(), max_side, Route::Embedded, Some(orientation))
            {
                return Ok(p);
            }
        }
    }
    decoded(file, Some("jpg"), max_side, Route::Reduced, None)
}

fn decode_bytes(
    bytes: Vec<u8>,
    max_side: u32,
    route: Route,
    force_orientation: Option<u8>,
) -> Result<Preview> {
    let mut reader = decode::open(
        Input::Mem(std::io::Cursor::new(bytes)),
        Some("jpg"),
        &OpenOpts::default(),
    )?;
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let (img, _) = decode_to_image(&mut *reader, &ImagePlan::default(), &ctl)?;
    // The thumbnail has no orientation tag of its own; the picture's applies to it.
    let img = match force_orientation {
        Some(o) if o > 1 => crate::orient::apply(img, o),
        _ => img,
    };
    Ok(finish(img, max_side, route, true))
}

/// Decode through the pipeline at the size wanted, streaming.
fn decoded(
    file: File,
    ext_hint: Option<&str>,
    max_side: u32,
    route: Route,
    _unused: Option<()>,
) -> Result<Preview> {
    // For JPEG the decoder shrinks by DCT scaling to the smallest size that is still twice
    // `min_size_hint`, so ask for half of what is wanted.
    let half = (max_side / 2).max(1);
    let opts = OpenOpts {
        min_size_hint: Some((half, half)),
        first_frame: true,
        buffer_budget: None,
        keep_color: false,
    };
    let mut reader = decode::open(Input::File(file), ext_hint, &opts)?;
    let info = reader.info().clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (fw, fh) = crate::orient::oriented_size(o, info.full_width, info.full_height);
    let (pw, ph) = fit(fw, fh, max_side);
    let scale =
        ((pw, ph) != (info.width, info.height) && pw.max(ph) < fw.max(fh)).then_some((pw, ph));
    let (cancel, progress) = quiet();
    let ctl = Ctl {
        cancel: &cancel,
        progress: &progress,
    };
    let plan = ImagePlan {
        out_space: OutSpace::Srgb,
        scale,
        filter: scale.map(|_| Filter::auto(fw, fh, pw, ph)),
        flatten: None,
        out_depth: Some(crate::types::Depth::U8),
    };
    let (img, _) = decode_to_image(&mut *reader, &plan, &ctl)?;
    Ok(finish(img, max_side, route, false))
}

/// The size of a `w` by `h` picture fitted inside `max_side` on its long side, never enlarged.
fn fit(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_side {
        return (w, h);
    }
    let s = max_side as f64 / long as f64;
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

/// RGBA8 from any 8 bit image, shrunk to `max_side` when it is larger.
fn finish(img: Image, max_side: u32, route: Route, thumbnail: bool) -> Preview {
    let img = if img.width.max(img.height) > max_side {
        let (w, h) = fit(img.width, img.height, max_side);
        let mut rd = crate::pipeline::ImageRows::new(&img);
        let (cancel, progress) = quiet();
        let ctl = Ctl {
            cancel: &cancel,
            progress: &progress,
        };
        let plan = crate::pipeline::StreamPlan {
            out_space: OutSpace::Keep,
            scale: Some((w, h)),
            filter: Some(Filter::auto(img.width, img.height, w, h)),
            flatten: None,
            out_depth: Some(crate::types::Depth::U8),
        };
        let mut sink = crate::pipeline::ImageSink::new();
        match crate::pipeline::run_stream(&mut rd, &plan, &ctl, &mut sink)
            .and_then(|_| sink.into_image())
        {
            Ok(small) => small,
            Err(_) => img,
        }
    } else {
        img
    };
    let (width, height) = (img.width, img.height);
    let img8 = img.to_u8();
    let ch = img8.layout.channels();
    let Samples::U8(v) = &img8.samples else {
        unreachable!("to_u8 gives 8 bit samples")
    };
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for p in v.chunks_exact(ch) {
        match img8.layout {
            Layout::Gray => rgba.extend_from_slice(&[p[0], p[0], p[0], 255]),
            Layout::GrayAlpha => rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
            Layout::Rgb => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
            Layout::Rgba => rgba.extend_from_slice(p),
        }
    }
    Preview {
        width,
        height,
        rgba,
        route,
        refine: thumbnail,
    }
}

#[cfg(target_os = "macos")]
fn os(mut file: File, fmt: Format, max_side: u32, fast: bool) -> Result<Preview> {
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    let dec = panoply_platform_macos::Decoder::open(data).map_err(|e| {
        ImageError::Unsupported(format!(
            "{} Convert the file with another app first.",
            e.message()
        ))
    })?;
    let t = dec.thumbnail(max_side, fast).map_err(|e| {
        ImageError::Failed(format!("{} Try opening the picture instead.", e.message()))
    })?;
    let (fw, fh) = dec.full_oriented_size();
    let smaller = t.width.max(t.height) < max_side.min(fw.max(fh));
    let _ = fmt;
    // The system hands back premultiplied alpha.
    let mut rgba = t.rgba;
    for p in rgba.chunks_exact_mut(4) {
        let a = u32::from(p[3]);
        if a != 0 && a != 255 {
            for c in &mut p[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    Ok(Preview {
        width: t.width,
        height: t.height,
        rgba,
        route: if t.embedded {
            Route::Embedded
        } else {
            Route::Reduced
        },
        refine: t.embedded && smaller,
    })
}

#[cfg(not(target_os = "macos"))]
fn os(_file: File, fmt: Format, _max_side: u32, _fast: bool) -> Result<Preview> {
    Err(decode::os::unavailable(fmt))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_never_enlarges_and_keeps_the_shape() {
        assert_eq!(fit(4000, 3000, 1000), (1000, 750));
        assert_eq!(fit(3000, 4000, 1000), (750, 1000));
        assert_eq!(fit(500, 300, 1000), (500, 300));
        assert_eq!(fit(100000, 1, 1000), (1000, 1));
    }
}

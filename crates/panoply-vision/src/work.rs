//! Reading an input into working pixels and writing a result, through the `panoply-image`
//! pipeline so orientation, color and metadata behave as in every other image module (IPC-1,
//! IPC-2, IPC-9).

use std::path::{Path, PathBuf};

use panoply_core::CancelToken;
use panoply_image::color::OutSpace;
use panoply_image::decode::SrcInfo;
use panoply_image::encode::png::Effort;
use panoply_image::error::{ImageError, Result};
use panoply_image::format::Format;
use panoply_image::ops::common::{
    Animation, EncodeSpec, SourceMeta, carry, display_name, encode_bytes, kept_dropped_line, load,
    source_bytes, write_output,
};
use panoply_image::ops::{Io, NameSpec};
use panoply_image::orient;
use panoply_image::pipeline::{Ctl, ImagePlan, decode_to_image};
use panoply_image::resample::Filter;
use panoply_image::ssim::estimate_jpeg_quality;
use panoply_image::types::{Depth, Image, Layout, Samples};

/// The most pixels a job holds at full size. Over this a picture is decoded reduced and the card
/// says so: 24 MP is 72 MB of RGB, which leaves room for the working planes inside the 300 MB
/// image budget.
pub const MAX_WORK_PIXELS: usize = 24_000_000;

/// An input decoded to upright 8 bit sRGB.
pub struct Source {
    /// Where it came from.
    pub path: PathBuf,
    /// File name, for card lines.
    pub name: String,
    /// What the reader reported for the file.
    pub info: SrcInfo,
    /// Size of the file in bytes.
    pub file_len: u64,
    /// Working width.
    pub w: usize,
    /// Working height.
    pub h: usize,
    /// Width of the picture in the file (upright), before any reduction.
    pub full_w: usize,
    /// Height of the picture in the file (upright), before any reduction.
    pub full_h: usize,
    /// RGB, 3 bytes a pixel.
    pub rgb: Vec<u8>,
    /// The picture's own transparency, when it has some.
    pub alpha: Option<Vec<u8>>,
    /// Lines from the decode for the result card ("converted CMYK to sRGB").
    pub notes: Vec<String>,
}

/// Decode `path` to upright 8 bit sRGB, at most `max_pixels` large.
pub fn load_source(
    io: &dyn Io,
    path: &Path,
    animation: Animation,
    max_pixels: usize,
) -> Result<Source> {
    let mut loaded = load(io, path, None, animation, OutSpace::Srgb)?;
    let info = loaded.info.clone();
    let o = if info.upright { 1 } else { info.orientation };
    let (uw, uh) = orient::oriented_size(o, info.full_width, info.full_height);
    let scale = if (uw as usize) * (uh as usize) > max_pixels {
        let s = (max_pixels as f64 / (uw as f64 * uh as f64)).sqrt();
        Some((
            ((uw as f64 * s) as u32).max(1),
            ((uh as f64 * s) as u32).max(1),
        ))
    } else {
        None
    };
    if let Some(sz) = scale
        && info.format.decodes_reduced()
    {
        loaded = load(io, path, Some(sz), animation, OutSpace::Srgb)?;
    }
    let progress = |_: f32| {};
    let ctl = Ctl {
        cancel: io.cancel(),
        progress: &progress,
    };
    let plan = ImagePlan {
        out_space: OutSpace::Srgb,
        scale,
        filter: scale.map(|(sw, sh)| Filter::auto(uw, uh, sw, sh)),
        flatten: None,
        out_depth: Some(Depth::U8),
    };
    let (img, out) = decode_to_image(&mut *loaded.reader, &plan, &ctl)?;
    let (w, h) = (img.width as usize, img.height as usize);
    let Samples::U8(samples) = &img.samples else {
        return Err(ImageError::Failed(
            "internal: the picture was not decoded to 8 bit".into(),
        ));
    };
    let (rgb, alpha) = split_rgb_alpha(samples, img.layout, w * h);
    Ok(Source {
        path: path.to_path_buf(),
        name: display_name(path),
        info,
        file_len: loaded.file_len,
        w,
        h,
        full_w: uw as usize,
        full_h: uh as usize,
        rgb,
        alpha,
        notes: out.notes,
    })
}

/// Splits decoded samples into RGB and, when the picture is not fully opaque, an alpha plane.
fn split_rgb_alpha(samples: &[u8], layout: Layout, n: usize) -> (Vec<u8>, Option<Vec<u8>>) {
    let ch = layout.channels();
    let mut rgb = Vec::with_capacity(n * 3);
    let mut alpha = layout.has_alpha().then(|| Vec::with_capacity(n));
    for p in samples.chunks_exact(ch) {
        match layout {
            Layout::Gray => rgb.extend_from_slice(&[p[0], p[0], p[0]]),
            Layout::GrayAlpha => rgb.extend_from_slice(&[p[0], p[0], p[0]]),
            Layout::Rgb | Layout::Rgba => rgb.extend_from_slice(&p[..3]),
        }
        if let Some(a) = alpha.as_mut() {
            a.push(p[ch - 1]);
        }
    }
    let alpha = alpha.filter(|a| a.iter().any(|&v| v != 255));
    (rgb, alpha)
}

/// An 8 bit [`Image`] from RGB and an optional alpha plane.
pub fn image_from(w: usize, h: usize, rgb: &[u8], alpha: Option<&[u8]>) -> Image {
    let (layout, samples) = match alpha {
        Some(a) => {
            let mut v = Vec::with_capacity(w * h * 4);
            for (p, &al) in rgb.chunks_exact(3).zip(a) {
                v.extend_from_slice(&[p[0], p[1], p[2], al]);
            }
            (Layout::Rgba, v)
        }
        None => (Layout::Rgb, rgb.to_vec()),
    };
    Image {
        width: w as u32,
        height: h as u32,
        layout,
        samples: Samples::U8(samples),
        icc: None,
    }
}

/// The format a result is written in when the user did not choose one: the source's own when
/// Panoply can write it as is (JPEG, PNG, WebP), PNG otherwise (lossless, and it keeps alpha).
pub fn same_format(src: Format) -> Format {
    match src {
        Format::Jpeg | Format::Png | Format::WebP => src,
        _ => Format::Png,
    }
}

/// Parse a format option (`png`, `jpg`, `webp`, `keep`).
pub fn parse_format(choice: Option<&str>, src: Format) -> Format {
    match choice {
        Some("png") => Format::Png,
        Some("jpg") | Some("jpeg") => Format::Jpeg,
        Some("webp") => Format::WebP,
        _ => same_format(src),
    }
}

/// How a result is encoded and labelled.
pub struct WriteSpec<'a> {
    /// The input the result belongs to.
    pub src: &'a Source,
    /// The label in the file name, "name (label).ext".
    pub label: &'a str,
    /// The format to write.
    pub format: Format,
    /// Lossy quality, 1 to 100; `None` keeps a JPEG near the quality it came in at.
    pub quality: Option<f64>,
    /// Carry EXIF, XMP and the color profile into the result.
    pub keep_metadata: bool,
}

/// Encode `img` and write it as a new file next to the input. Returns the path and the size.
pub fn write_result(io: &dyn Io, img: &Image, spec: &WriteSpec<'_>) -> Result<(PathBuf, usize)> {
    let src = spec.src;
    let target = spec.format;
    let src_bytes = source_bytes(io, &src.path);
    let meta = SourceMeta::read_for(src_bytes.as_deref(), target);
    let carried = carry(
        &meta,
        &src.info,
        img.width,
        img.height,
        img.icc.as_ref(),
        spec.keep_metadata,
        true,
        target,
    );
    let mut enc = EncodeSpec::new(target);
    enc.embed = carried.embed.clone();
    enc.progressive = false;
    enc.png_effort = Effort::Balanced;
    enc.quality = spec.quality.map(|q| q as f32).or_else(|| {
        if target == Format::Jpeg && src.info.format == Format::Jpeg {
            src_bytes
                .as_deref()
                .and_then(estimate_jpeg_quality)
                .map(|q| q.clamp(60.0, 95.0))
        } else {
            Some(match target {
                Format::Jpeg => 92.0,
                Format::WebP => 90.0,
                _ => EncodeSpec::default_quality(target),
            })
        }
    });
    let flat;
    let to_encode = if img.layout.has_alpha() && !target.has_alpha() {
        flat = flatten_white(img);
        &flat
    } else {
        img
    };
    let bytes = encode_bytes(to_encode, &enc)?;
    let path = write_output(
        io,
        &src.path,
        &NameSpec::Label(spec.label.to_string()),
        target.ext(),
        &bytes,
    )?;
    if let Some(l) = kept_dropped_line(&carried) {
        io.note(format!("{}: {l}", src.name));
    }
    Ok((path, bytes.len()))
}

/// Composite a picture with alpha onto white and drop the channel.
fn flatten_white(img: &Image) -> Image {
    let Samples::U8(s) = &img.samples else {
        return img.clone();
    };
    let ch = img.layout.channels();
    let mut out = Vec::with_capacity(s.len() / ch * 3);
    for p in s.chunks_exact(ch) {
        let a = u32::from(p[ch - 1]);
        for &c in &p[..3.min(ch - 1).max(1)] {
            out.push(((u32::from(c) * a + 255 * (255 - a) + 127) / 255) as u8);
        }
        if ch == 2 {
            let g = *out.last().unwrap_or(&0);
            out.push(g);
            out.push(g);
        }
    }
    Image {
        width: img.width,
        height: img.height,
        layout: Layout::Rgb,
        samples: Samples::U8(out),
        icc: img.icc.clone(),
    }
}

/// The file name of a path for card lines.
pub fn file_name(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Cancel check shared by the modules.
pub fn check(cancel: &CancelToken) -> Result<()> {
    if cancel.is_cancelled() {
        Err(ImageError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alpha_is_split_only_when_it_matters() {
        let samples = [10u8, 20, 30, 255, 40, 50, 60, 255];
        let (rgb, a) = split_rgb_alpha(&samples, Layout::Rgba, 2);
        assert_eq!(rgb, [10, 20, 30, 40, 50, 60]);
        assert!(a.is_none());
        let samples = [10u8, 20, 30, 0, 40, 50, 60, 255];
        let (_, a) = split_rgb_alpha(&samples, Layout::Rgba, 2);
        assert_eq!(a.unwrap(), [0, 255]);
        let (rgb, _) = split_rgb_alpha(&[7, 255, 9, 255], Layout::GrayAlpha, 2);
        assert_eq!(rgb, [7, 7, 7, 9, 9, 9]);
    }

    #[test]
    fn same_format_keeps_what_it_can_write() {
        assert_eq!(same_format(Format::Jpeg), Format::Jpeg);
        assert_eq!(same_format(Format::WebP), Format::WebP);
        assert_eq!(same_format(Format::Heic), Format::Png);
        assert_eq!(parse_format(Some("png"), Format::Jpeg), Format::Png);
        assert_eq!(parse_format(Some("keep"), Format::Jpeg), Format::Jpeg);
    }

    #[test]
    fn flattening_blends_with_white() {
        let img = image_from(2, 1, &[0, 0, 0, 0, 0, 0], Some(&[0, 255]));
        let f = flatten_white(&img);
        let Samples::U8(v) = f.samples else { panic!() };
        assert_eq!(v, [255, 255, 255, 0, 0, 0]);
    }
}

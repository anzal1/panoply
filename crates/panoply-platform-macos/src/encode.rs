use objc2_core_foundation::{CFData, CFMutableData, CFNumber, CFRetained, CFString, CFType, Type};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorSpace, CGDataProvider, CGImageAlphaInfo, CGImageByteOrderInfo,
    kCGColorSpaceDisplayP3, kCGColorSpaceSRGB,
};
use objc2_image_io::{kCGImageDestinationLossyCompressionQuality, kCGImagePropertyOrientation};

use crate::cf::{self, sys};
use crate::decode::{MAX_DIMENSION, Space};
use crate::error::Error;
use crate::props::Properties;
use crate::sys as ffi;

/// The largest image `encode` accepts. The caller already holds the pixels (4 or 8 bytes each)
/// and the encoder needs roughly one more copy plus its own state, so 25 megapixels of 8 bit
/// RGBA is about 100 MB in and 100 MB working, inside the 300 MB budget.
pub const MAX_ENCODE_PIXELS: u64 = 25_000_000;

/// An output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// HEIC (HEVC in HEIF), `public.heic`.
    Heic,
    /// AVIF, `public.avif`.
    Avif,
}

impl Format {
    fn uti(self) -> &'static str {
        match self {
            Format::Heic => "public.heic",
            Format::Avif => "public.avif",
        }
    }

    fn name(self) -> &'static str {
        match self {
            Format::Heic => "HEIC",
            Format::Avif => "AVIF",
        }
    }
}

/// Can this system encode `format`? Asked of ImageIO at run time, never assumed.
pub fn can_encode(format: Format) -> bool {
    ffi::destination_type_identifiers()
        .iter()
        .any(|t| t == format.uti())
}

/// Pixel samples, RGBA, straight (not premultiplied) alpha, tightly packed.
#[derive(Debug, Clone, Copy)]
pub enum Pixels<'a> {
    /// 8 bits per component.
    U8(&'a [u8]),
    /// 16 bits per component, native endian.
    U16(&'a [u16]),
}

/// What to encode.
#[derive(Debug, Clone, Copy)]
pub struct EncodeInput<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// The samples.
    pub pixels: Pixels<'a>,
    /// Color space the samples are in. `Source` is treated as sRGB unless `icc` is given.
    pub space: Space,
}

/// How to encode it.
#[derive(Debug, Clone, Copy)]
pub struct EncodeOptions<'a> {
    /// Lossy quality, 0.0 to 1.0.
    pub quality: f32,
    /// Tag the output with this ICC profile instead of `space`.
    pub icc: Option<&'a [u8]>,
    /// Copy EXIF, TIFF, IPTC and GPS from a source file, orientation forced to 1.
    pub keep_properties: Option<&'a Properties>,
    /// Leave out the GPS section even when `keep_properties` has one.
    pub drop_gps: bool,
    /// Ignore alpha and encode RGB only.
    pub opaque: bool,
}

impl Default for EncodeOptions<'_> {
    fn default() -> Self {
        EncodeOptions {
            quality: 0.8,
            icc: None,
            keep_properties: None,
            drop_gps: false,
            opaque: false,
        }
    }
}

/// Encodes an image through ImageIO.
pub fn encode(input: &EncodeInput, format: Format, opts: &EncodeOptions) -> Result<Vec<u8>, Error> {
    if !can_encode(format) {
        return Err(Error::new(format!(
            "This Mac cannot write {} files.",
            format.name()
        )));
    }
    let (w, h) = (input.width as usize, input.height as usize);
    if w == 0 || h == 0 {
        return Err(Error::new("The image has no pixels."));
    }
    if input.width > MAX_DIMENSION || input.height > MAX_DIMENSION {
        return Err(Error::new("This image is too large to encode."));
    }
    if u64::from(input.width) * u64::from(input.height) > MAX_ENCODE_PIXELS {
        return Err(Error::new("This image has too many pixels to encode."));
    }
    let samples = w * h * 4;
    let (bytes, wide): (&[u8], bool) = match input.pixels {
        Pixels::U8(p) => {
            if p.len() != samples {
                return Err(Error::new(
                    "The pixel buffer does not match the image size.",
                ));
            }
            (p, false)
        }
        Pixels::U16(p) => {
            if p.len() != samples {
                return Err(Error::new(
                    "The pixel buffer does not match the image size.",
                ));
            }
            (ffi::u16_as_bytes(p), true)
        }
    };
    if !(0.0..=1.0).contains(&opts.quality) {
        return Err(Error::new("Quality must be between 0 and 1."));
    }

    let space = match opts.icc {
        Some(icc) => ffi::colorspace_from_icc(&CFData::from_bytes(icc))
            .ok_or_else(|| Error::new("The ICC profile is not valid."))?,
        None => {
            let name = match input.space {
                Space::DisplayP3 => sys!(kCGColorSpaceDisplayP3),
                Space::Srgb | Space::Source => sys!(kCGColorSpaceSRGB),
            };
            CGColorSpace::with_name(Some(name))
                .ok_or_else(|| Error::new("The system could not make a color space."))?
        }
    };

    let alpha = if opts.opaque {
        CGImageAlphaInfo::NoneSkipLast
    } else {
        CGImageAlphaInfo::Last
    };
    let bitmap_info = CGBitmapInfo(
        alpha.0
            | if wide {
                CGImageByteOrderInfo::Order16Little.0
            } else {
                0
            },
    );
    let provider = CGDataProvider::with_cf_data(Some(&CFData::from_bytes(bytes)))
        .ok_or_else(|| Error::new("The system could not wrap the pixels."))?;
    let bytes_per_pixel = if wide { 8 } else { 4 };
    let image = ffi::image_from_pixels(
        w,
        h,
        if wide { 16 } else { 8 },
        bytes_per_pixel * 8,
        w * bytes_per_pixel,
        &space,
        bitmap_info,
        &provider,
    )
    .ok_or_else(|| Error::new("The system could not build an image from these pixels."))?;

    let out = CFMutableData::new(None, 0).ok_or_else(|| Error::new("Out of memory."))?;
    let dest = ffi::dest_with_data(&out, &CFString::from_str(format.uti()))
        .ok_or_else(|| Error::new(format!("This Mac cannot write {} files.", format.name())))?;

    let quality = CFNumber::new_f64(f64::from(opts.quality));
    let one = CFNumber::new_i32(1);
    let mut keys: Vec<CFRetained<CFString>> = Vec::new();
    let mut values: Vec<CFRetained<CFType>> = Vec::new();
    keys.push(sys!(kCGImageDestinationLossyCompressionQuality).retain());
    values.push(as_cf_type(quality));
    if let Some(props) = opts.keep_properties {
        for (k, v) in props.for_output(opts.drop_gps) {
            keys.push(k);
            values.push(v);
        }
        keys.push(sys!(kCGImagePropertyOrientation).retain());
        values.push(as_cf_type(one));
    }
    let kr: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
    let vr: Vec<&CFType> = values.iter().map(|v| &**v).collect();
    let dict = cf::make_dict_slices(&kr, &vr);

    ffi::dest_add_image(&dest, &image, Some(cf::untyped(&dict)));
    if !ffi::dest_finalize(&dest) {
        return Err(Error::new(format!(
            "The system failed to write the {} file.",
            format.name()
        )));
    }
    drop(dest);
    let bytes = out.to_vec();
    if bytes.is_empty() {
        return Err(Error::new(format!(
            "The system wrote an empty {} file.",
            format.name()
        )));
    }
    Ok(bytes)
}

fn as_cf_type<T: objc2_core_foundation::Type + 'static>(
    value: CFRetained<T>,
) -> CFRetained<CFType> {
    // SAFETY: every CF object is a CFType, so upcasting the static type is sound.
    unsafe { CFRetained::cast_unchecked(value) }
}

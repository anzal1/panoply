//! Camera RAW through CoreImage's `CIRAWFilter`, rendered in thin bands.
//!
//! ImageIO develops a RAW file as one whole picture: measured on the corpus it peaks at 2.0 GB
//! for a 12 MP iPhone DNG, 1.3 GB for a 11 MP Pixel DNG and 0.32 GB for a 4.5 MP Canon sRAW,
//! about 120 bytes a pixel of scratch (float planes and GPU surfaces) that no option of
//! `CGImageSource` lowers. `CIRAWFilter` is the engine ImageIO uses underneath, with the same
//! defaults, and unlike ImageIO it renders only the rectangle it is asked for. With the
//! intermediate cache off and bands of 32 rows the same three files peak at 0.35 GB, 0.29 GB
//! and 0.23 GB, and the pixels match ImageIO's to within one level of 255 (Canon sRAW, Pixel
//! DNG: largest difference 1).
//!
//! CoreImage is opened with `dlopen` at the first RAW file, so no other job pays for loading it
//! (it adds about 140 MB of Metal state to the process). Any failure here (the framework or the
//! class is missing, the file is not one `CIRAWFilter` takes, the size differs from what ImageIO
//! reported) returns `None` and the caller falls back to the ImageIO path, which is slower and
//! larger but always worked.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::sync::OnceLock;

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::{CFBoolean, CFData, CFString, CFType};
use objc2_core_graphics::CGColorSpace;

use crate::cf;
use crate::error::Error;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// `RTLD_LAZY` on macOS.
const RTLD_LAZY: c_int = 0x1;

/// Rows rendered per call. The peak of a render grows with its height (iPhone DNG: 340 MB at 4
/// rows, 353 MB at 32, 453 MB at 128, 730 MB at 512), while narrow bands repeat the
/// demosaic margin: 32 rows is the knee.
const BAND_ROWS: usize = 32;

/// True for the uniform type identifiers ImageIO reports for camera RAW (`public.camera-raw-image`,
/// `com.adobe.raw-image` for DNG, `com.canon.cr2-raw-image`, `com.nikon.raw-image` and so on).
pub(crate) fn is_raw_uti(uti: &str) -> bool {
    uti == "public.camera-raw-image" || uti.contains("raw-image")
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Point {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Size {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect {
    origin: Point,
    size: Size,
}

// SAFETY: `Point`, `Size` and `Rect` have the layout of CGPoint, CGSize and CGRect (two, two
// and four `f64` fields, `repr(C)`), and the encodings below are the ones the runtime reports
// for those types.
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
}
// SAFETY: see `Point`.
unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
}
// SAFETY: see `Point`.
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

/// A `CGColorSpaceRef` passed as a message argument.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct ColorSpaceRef(*const c_void);

// SAFETY: a `CGColorSpaceRef` is a pointer to the opaque `CGColorSpace` struct, which is what
// this encoding names.
unsafe impl Encode for ColorSpaceRef {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("CGColorSpace", &[]));
}
// SAFETY: see the `Encode` impl.
unsafe impl RefEncode for ColorSpaceRef {
    const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
}

/// CoreImage, opened once.
struct Lib {
    handle: *mut c_void,
}

// SAFETY: the handle is an opaque token that `dlsym` reads and nothing mutates.
unsafe impl Send for Lib {}
// SAFETY: see `Send`.
unsafe impl Sync for Lib {}

fn lib() -> Option<&'static Lib> {
    static LIB: OnceLock<Option<Lib>> = OnceLock::new();
    LIB.get_or_init(|| {
        // SAFETY: the path is a NUL terminated literal, and loading a system framework has no
        // preconditions.
        let handle = unsafe {
            dlopen(
                c"/System/Library/Frameworks/CoreImage.framework/CoreImage".as_ptr(),
                RTLD_LAZY,
            )
        };
        (!handle.is_null()).then_some(Lib { handle })
    })
    .as_ref()
}

impl Lib {
    /// The address of an exported global.
    fn global(&self, name: &CStr) -> Option<*const c_void> {
        // SAFETY: `handle` came from a successful `dlopen` and `name` is NUL terminated.
        let p = unsafe { dlsym(self.handle, name.as_ptr()) };
        (!p.is_null()).then_some(p.cast_const())
    }

    /// An exported `CIFormat` constant (`kCIFormatRGBA8`).
    fn format(&self, name: &CStr) -> Option<c_int> {
        // SAFETY: CoreImage exports these as `const int` variables, so the address holds an int.
        self.global(name).map(|p| unsafe { *p.cast::<c_int>() })
    }

    /// An exported `NSString *` constant (`kCIContextCacheIntermediates`).
    fn string(&self, name: &CStr) -> Option<&'static CFString> {
        let p = self.global(name)?;
        // SAFETY: CoreImage exports these as `NSString *const` variables, so the address holds a
        // pointer to a string that lives for the life of the process (and NSString is CFString
        // under toll free bridging).
        unsafe { (*p.cast::<*const CFString>()).as_ref() }
    }
}

/// A developed RAW picture that renders bands on demand.
pub(crate) struct RawImage {
    image: Retained<AnyObject>,
    ctx: Retained<AnyObject>,
    width: u32,
    height: u32,
    min_x: f64,
    min_y: f64,
    format8: c_int,
    format16: c_int,
}

impl RawImage {
    /// Opens `data` as a RAW with the given EXIF orientation applied and `scale` (1.0 or less)
    /// of the sensor size. `None` when CoreImage cannot take the file.
    pub(crate) fn open(data: &CFData, uti: &str, orientation: u8, scale: f32) -> Option<RawImage> {
        let lib = lib()?;
        let filter_class = AnyClass::get(c"CIRAWFilter")?;
        let context_class = AnyClass::get(c"CIContext")?;
        let format8 = lib.format(c"kCIFormatRGBA8")?;
        let format16 = lib.format(c"kCIFormatRGBA16")?;
        autoreleasepool(|_| {
            // SAFETY: a CFData is an NSData under toll free bridging, and a CFString an NSString.
            let data_obj: &AnyObject = unsafe { &*(data as *const CFData).cast::<AnyObject>() };
            let hint = CFString::from_str(uti);
            // SAFETY: as above.
            let hint_obj: &AnyObject = unsafe { &*(&*hint as *const CFString).cast::<AnyObject>() };
            // SAFETY: `filterWithImageData:identifierHint:` takes an NSData and an NSString and
            // returns a nullable filter.
            let filter: Option<Retained<AnyObject>> = unsafe {
                msg_send![filter_class, filterWithImageData: data_obj, identifierHint: Some(hint_obj)]
            };
            let filter = filter?;
            let o = u32::from(orientation.clamp(1, 8));
            // SAFETY: both setters take one scalar, `CGImagePropertyOrientation` (uint32) and
            // `float`.
            unsafe {
                let _: () = msg_send![&*filter, setOrientation: o];
                if scale < 0.999 {
                    let _: () = msg_send![&*filter, setScaleFactor: scale.max(0.02)];
                }
            }
            // SAFETY: `outputImage` takes no arguments and returns a nullable CIImage.
            let image: Option<Retained<AnyObject>> = unsafe { msg_send![&*filter, outputImage] };
            let image = image?;
            // SAFETY: `extent` takes no arguments and returns a CGRect.
            let extent: Rect = unsafe { msg_send![&*image, extent] };
            let (w, h) = (extent.size.width.round(), extent.size.height.round());
            if !(1.0..=1.0e6).contains(&w) || !(1.0..=1.0e6).contains(&h) {
                return None;
            }
            // Without an intermediate cache a render keeps only what its own tiles need.
            let off: &CFType = CFBoolean::new(false).as_ref();
            let key = lib.string(c"kCIContextCacheIntermediates")?;
            let options = cf::make_dict(&[(key, off)]);
            // SAFETY: a CFDictionary is an NSDictionary under toll free bridging.
            let options_obj: &AnyObject =
                unsafe { &*(&*options as *const cf::PropDict).cast::<AnyObject>() };
            // SAFETY: `contextWithOptions:` takes a nullable NSDictionary and returns a context.
            let ctx: Option<Retained<AnyObject>> =
                unsafe { msg_send![context_class, contextWithOptions: Some(options_obj)] };
            Some(RawImage {
                image,
                ctx: ctx?,
                width: w as u32,
                height: h as u32,
                min_x: extent.origin.x,
                min_y: extent.origin.y,
                format8,
                format16,
            })
        })
    }

    /// Size of the developed picture, upright, in pixels.
    pub(crate) fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Renders upright rows `[y0, y0 + rows)` into `out`, which holds exactly those rows as
    /// RGBA, 8 bit or 16 bit native endian. Alpha is opaque.
    pub(crate) fn render(
        &self,
        y0: usize,
        rows: usize,
        space: &CGColorSpace,
        wide: bool,
        out: &mut [u8],
    ) -> Result<(), Error> {
        let (w, h) = (self.width as usize, self.height as usize);
        let bpp = if wide { 8 } else { 4 };
        let row_bytes = w * bpp;
        if y0 + rows > h || out.len() != rows * row_bytes {
            return Err(Error::new("Those rows are outside the image."));
        }
        let format = if wide { self.format16 } else { self.format8 };
        let cs = ColorSpaceRef((space as *const CGColorSpace).cast());
        let mut done = 0;
        while done < rows {
            let n = BAND_ROWS.min(rows - done);
            let slice = &mut out[done * row_bytes..(done + n) * row_bytes];
            // CoreImage's y axis points up and the bitmap's first row is the top one.
            let rect = Rect {
                origin: Point {
                    x: self.min_x,
                    y: self.min_y + (h - (y0 + done) - n) as f64,
                },
                size: Size {
                    width: w as f64,
                    height: n as f64,
                },
            };
            autoreleasepool(|_| {
                // SAFETY: `slice` is `n * row_bytes` writable bytes, exactly what the render
                // writes for this rect and format, and it is borrowed for the whole call. The
                // color space is a live CGColorSpace.
                unsafe {
                    let _: () = msg_send![
                        &*self.ctx,
                        render: &*self.image,
                        toBitmap: slice.as_mut_ptr().cast::<c_void>(),
                        rowBytes: row_bytes as isize,
                        bounds: rect,
                        format: format,
                        colorSpace: cs
                    ];
                }
            });
            done += n;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_type_names_are_recognised() {
        for u in [
            "com.adobe.raw-image",
            "com.canon.cr2-raw-image",
            "com.canon.cr3-raw-image",
            "com.nikon.raw-image",
            "com.sony.arw-raw-image",
            "public.camera-raw-image",
        ] {
            assert!(is_raw_uti(u), "{u}");
        }
        for u in ["public.jpeg", "public.heic", "public.png", "public.avif"] {
            assert!(!is_raw_uti(u), "{u}");
        }
    }
}

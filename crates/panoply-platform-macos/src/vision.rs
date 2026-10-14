//! Apple Vision's foreground instance mask (`VNGenerateForegroundInstanceMaskRequest`, macOS 14
//! and later), the OS engine for background removal.
//!
//! Vision is opened with `dlopen` on the first call, so no other job pays for loading it, and
//! the engine lives in the system: no download, and no model memory of ours stays resident. A
//! request returns one soft mask for all the salient objects (a person, a pet, a product), at
//! the size of the input picture.
//!
//! Every failure (the framework or the class is missing on an older macOS, the request throws,
//! Vision finds no subject) is returned as an [`Error`] with a plain sentence, so the caller can
//! fall back to its own model.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::ptr::NonNull;
use std::sync::OnceLock;

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::{CFData, CFRetained, CFString, CFType};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorSpace, CGDataProvider, CGImageAlphaInfo, kCGColorSpaceSRGB,
};

use crate::cf::sys;
use crate::error::Error;
use crate::sys as ffi;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
}

/// `RTLD_LAZY` on macOS.
const RTLD_LAZY: c_int = 0x1;

/// `kCVPixelFormatType_OneComponent8` ('L008').
const FORMAT_L8: u32 = 0x4C30_3038;
/// `kCVPixelFormatType_OneComponent16Half` ('L00h').
const FORMAT_L16_HALF: u32 = 0x4C30_3068;
/// `kCVPixelFormatType_OneComponent32Float` ('L00f').
const FORMAT_L32_FLOAT: u32 = 0x4C30_3066;
/// `kCVPixelBufferLock_ReadOnly`.
const LOCK_READ_ONLY: u64 = 1;

/// The most pixels one request is given. Vision keeps its own copies of the picture, so a
/// request on 50 megapixels would blow the memory budget for nothing: it analyses at a much
/// lower resolution anyway.
pub const MAX_VISION_PIXELS: u64 = 40_000_000;

/// A `CGImageRef` passed as a message argument.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct ImageRef(*const c_void);

// SAFETY: a `CGImageRef` is a pointer to the opaque `CGImage` struct, which is what this
// encoding names.
unsafe impl Encode for ImageRef {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}
// SAFETY: see the `Encode` impl.
unsafe impl RefEncode for ImageRef {
    const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
}

/// A `CVPixelBufferRef` returned from a message.
#[repr(transparent)]
#[derive(Clone, Copy)]
struct BufferRef(*mut c_void);

// SAFETY: a `CVPixelBufferRef` is a pointer to the opaque `__CVBuffer` struct, which is what
// this encoding names.
unsafe impl Encode for BufferRef {
    const ENCODING: Encoding = Encoding::Pointer(&Encoding::Struct("__CVBuffer", &[]));
}
// SAFETY: see the `Encode` impl.
unsafe impl RefEncode for BufferRef {
    const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
}

/// A soft foreground mask the size of the input picture.
#[derive(Debug, Clone)]
pub struct ForegroundMask {
    /// Width in pixels (the input picture's width).
    pub width: u32,
    /// Height in pixels (the input picture's height).
    pub height: u32,
    /// One byte per pixel, row major: 0 is background, 255 is subject, values between are the
    /// soft edge.
    pub alpha: Vec<u8>,
    /// How many separate objects Vision found (a group of three people is three).
    pub instances: usize,
}

type GetSize = unsafe extern "C" fn(*const c_void) -> usize;
type GetFormat = unsafe extern "C" fn(*const c_void) -> u32;
type LockFn = unsafe extern "C" fn(*const c_void, u64) -> i32;
type BaseFn = unsafe extern "C" fn(*const c_void) -> *const u8;

/// The CoreVideo calls that read a pixel buffer, resolved once.
struct VideoFns {
    width: GetSize,
    height: GetSize,
    bytes_per_row: GetSize,
    format: GetFormat,
    lock: LockFn,
    unlock: LockFn,
    base: BaseFn,
}

fn symbol(handle: *mut c_void, name: &CStr) -> Option<*mut c_void> {
    // SAFETY: `handle` came from a successful `dlopen` and `name` is NUL terminated.
    let p = unsafe { dlsym(handle, name.as_ptr()) };
    (!p.is_null()).then_some(p)
}

fn video() -> Option<&'static VideoFns> {
    static FNS: OnceLock<Option<VideoFns>> = OnceLock::new();
    FNS.get_or_init(|| {
        // SAFETY: the path is a NUL terminated literal, and loading a system framework has no
        // preconditions.
        let h = unsafe {
            dlopen(
                c"/System/Library/Frameworks/CoreVideo.framework/CoreVideo".as_ptr(),
                RTLD_LAZY,
            )
        };
        if h.is_null() {
            return None;
        }
        // SAFETY: each address is the exported C function of that name, whose signature is the
        // one declared in the type aliases above (CVPixelBuffer.h): size_t getters take a
        // CVPixelBufferRef, lock and unlock take the ref and a flags word and return a
        // CVReturn, and the base address getter returns a pointer.
        unsafe {
            Some(VideoFns {
                width: std::mem::transmute::<*mut c_void, GetSize>(symbol(
                    h,
                    c"CVPixelBufferGetWidth",
                )?),
                height: std::mem::transmute::<*mut c_void, GetSize>(symbol(
                    h,
                    c"CVPixelBufferGetHeight",
                )?),
                bytes_per_row: std::mem::transmute::<*mut c_void, GetSize>(symbol(
                    h,
                    c"CVPixelBufferGetBytesPerRow",
                )?),
                format: std::mem::transmute::<*mut c_void, GetFormat>(symbol(
                    h,
                    c"CVPixelBufferGetPixelFormatType",
                )?),
                lock: std::mem::transmute::<*mut c_void, LockFn>(symbol(
                    h,
                    c"CVPixelBufferLockBaseAddress",
                )?),
                unlock: std::mem::transmute::<*mut c_void, LockFn>(symbol(
                    h,
                    c"CVPixelBufferUnlockBaseAddress",
                )?),
                base: std::mem::transmute::<*mut c_void, BaseFn>(symbol(
                    h,
                    c"CVPixelBufferGetBaseAddress",
                )?),
            })
        }
    })
    .as_ref()
}

/// Vision, opened once. The handle only keeps the framework loaded.
fn vision_loaded() -> bool {
    static LOADED: OnceLock<bool> = OnceLock::new();
    *LOADED.get_or_init(|| {
        // SAFETY: the path is a NUL terminated literal, and loading a system framework has no
        // preconditions.
        let h = unsafe {
            dlopen(
                c"/System/Library/Frameworks/Vision.framework/Vision".as_ptr(),
                RTLD_LAZY,
            )
        };
        !h.is_null()
    })
}

/// True when this Mac has the foreground instance mask request (macOS 14 and later). The check
/// is made at run time against the loaded framework, so an older system answers false.
pub fn foreground_mask_available() -> bool {
    vision_loaded()
        && AnyClass::get(c"VNGenerateForegroundInstanceMaskRequest").is_some()
        && AnyClass::get(c"VNImageRequestHandler").is_some()
        && video().is_some()
}

/// The description of an `NSError`, or a fallback sentence.
fn error_text(err: *mut AnyObject, fallback: &str) -> String {
    if err.is_null() {
        return fallback.to_string();
    }
    // SAFETY: a non-null `NSError **` out value is a valid NSError the callee autoreleased.
    let desc: Option<Retained<AnyObject>> = unsafe { msg_send![&*err, localizedDescription] };
    match desc {
        Some(d) => {
            // SAFETY: an NSString is a CFString under toll free bridging.
            let s: &CFString = unsafe { &*(&*d as *const AnyObject).cast::<CFString>() };
            s.to_string()
        }
        None => fallback.to_string(),
    }
}

/// Builds a `CGImage` over a copy of RGBA pixels (alpha ignored).
fn cg_image(
    rgba: &[u8],
    width: usize,
    height: usize,
) -> Result<CFRetained<objc2_core_graphics::CGImage>, Error> {
    let space = CGColorSpace::with_name(Some(sys!(kCGColorSpaceSRGB)))
        .ok_or_else(|| Error::new("The system could not make a color space."))?;
    let provider = CGDataProvider::with_cf_data(Some(&CFData::from_bytes(rgba)))
        .ok_or_else(|| Error::new("The system could not wrap the pixels."))?;
    ffi::image_from_pixels(
        width,
        height,
        8,
        32,
        width * 4,
        &space,
        CGBitmapInfo(CGImageAlphaInfo::NoneSkipLast.0),
        &provider,
    )
    .ok_or_else(|| Error::new("The system could not build an image from these pixels."))
}

/// Runs the foreground instance mask request on an 8 bit sRGB picture and returns the soft mask
/// of every foreground object together.
///
/// `pixels` holds `width * height` pixels, 3 bytes each (RGB) or 4 (RGBA, the fourth byte is
/// ignored). Vision decides what the subject is; when it finds none this returns an error with
/// a plain sentence.
pub fn foreground_instance_mask(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: usize,
) -> Result<ForegroundMask, Error> {
    find_foreground(pixels, width, height, channels)?
        .ok_or_else(|| Error::new("The system found no subject in this picture."))
}

/// The one implementation of the foreground request. `Ok(None)` means Vision ran and found no
/// subject, which callers treat differently from a failure.
pub(crate) fn find_foreground(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: usize,
) -> Result<Option<ForegroundMask>, Error> {
    let (w, h) = (width as usize, height as usize);
    if !(channels == 3 || channels == 4) || w == 0 || h == 0 || pixels.len() != w * h * channels {
        return Err(Error::new("The picture does not match its size."));
    }
    if (w as u64) * (h as u64) > MAX_VISION_PIXELS {
        return Err(Error::new(
            "This picture is too large for the system's subject finder.",
        ));
    }
    if !foreground_mask_available() {
        return Err(Error::new(
            "This Mac does not have the system subject finder (it needs macOS 14 or later).",
        ));
    }
    let vfns = video().ok_or_else(|| Error::new("The system video library is missing."))?;
    // Vision wants 4 bytes a pixel; the extra copy is freed before the request returns.
    let rgba: Vec<u8>;
    let rgba_ref: &[u8] = if channels == 4 {
        pixels
    } else {
        let mut v = Vec::with_capacity(w * h * 4);
        for p in pixels.chunks_exact(3) {
            v.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        rgba = v;
        &rgba
    };
    let image = cg_image(rgba_ref, w, h)?;

    autoreleasepool(|_| {
        let (Some(req_class), Some(handler_class), Some(array_class), Some(dict_class)) = (
            AnyClass::get(c"VNGenerateForegroundInstanceMaskRequest"),
            AnyClass::get(c"VNImageRequestHandler"),
            AnyClass::get(c"NSArray"),
            AnyClass::get(c"NSDictionary"),
        ) else {
            return Err(Error::new("The system subject finder is not available."));
        };
        // SAFETY: `new` on a concrete Vision request class returns a retained instance.
        let request: Option<Retained<AnyObject>> = unsafe { msg_send![req_class, new] };
        let request =
            request.ok_or_else(|| Error::new("The system subject finder did not start."))?;
        // SAFETY: `dictionary` returns an empty NSDictionary.
        let options: Retained<AnyObject> = unsafe { msg_send![dict_class, dictionary] };
        // SAFETY: `alloc` returns an uninitialised instance that `initWithCGImage:options:`
        // consumes. The image is a live CGImage and `options` an NSDictionary.
        let handler: Option<Retained<AnyObject>> = unsafe {
            let alloc: Allocated<AnyObject> = msg_send![handler_class, alloc];
            msg_send![
                alloc,
                initWithCGImage: ImageRef((&*image as *const objc2_core_graphics::CGImage).cast()),
                options: &*options
            ]
        };
        let handler =
            handler.ok_or_else(|| Error::new("The system subject finder did not start."))?;
        // SAFETY: `arrayWithObject:` takes one object and returns an NSArray of it.
        let requests: Retained<AnyObject> =
            unsafe { msg_send![array_class, arrayWithObject: &*request] };

        let mut err: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: `performRequests:error:` takes an NSArray of requests and an `NSError **`
        // that it fills on failure; `err` is a valid out pointer for the call.
        let ok: bool =
            unsafe { msg_send![&*handler, performRequests: &*requests, error: &mut err] };
        if !ok {
            return Err(Error::new(format!(
                "The system subject finder failed: {}",
                error_text(err, "no reason given")
            )));
        }
        // SAFETY: `results` takes no arguments and returns a nullable NSArray.
        let results: Option<Retained<AnyObject>> = unsafe { msg_send![&*request, results] };
        let Some(results) = results else {
            return Ok(None);
        };
        // SAFETY: `count` takes no arguments and returns NSUInteger.
        let n: usize = unsafe { msg_send![&*results, count] };
        if n == 0 {
            return Ok(None);
        }
        // SAFETY: index 0 is in range (count is at least 1), `objectAtIndex:` returns the
        // observation.
        let obs: Retained<AnyObject> = unsafe { msg_send![&*results, objectAtIndex: 0usize] };
        // SAFETY: `allInstances` takes no arguments and returns an NSIndexSet.
        let all: Retained<AnyObject> = unsafe { msg_send![&*obs, allInstances] };
        // SAFETY: `count` on an NSIndexSet returns NSUInteger.
        let instances: usize = unsafe { msg_send![&*all, count] };
        if instances == 0 {
            return Ok(None);
        }
        let mut err2: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: the instance set and the handler are live objects of the types the method
        // takes, and `err2` is a valid `NSError **`. The returned CVPixelBufferRef is owned by
        // us (CF_RETURNS_RETAINED), released below.
        let buffer: BufferRef = unsafe {
            msg_send![
                &*obs,
                generateScaledMaskForImageForInstances: &*all,
                fromRequestHandler: &*handler,
                error: &mut err2
            ]
        };
        let buffer = NonNull::new(buffer.0).ok_or_else(|| {
            Error::new(format!(
                "The system could not draw the subject mask: {}",
                error_text(err2, "no reason given")
            ))
        })?;
        // SAFETY: the pointer is a +1 retained CF object, so owning it as a `CFRetained`
        // releases it exactly once when dropped.
        let _owned: CFRetained<CFType> = unsafe { CFRetained::from_raw(buffer.cast::<CFType>()) };
        read_mask(vfns, buffer.as_ptr(), instances).map(Some)
    })
}

/// Copies a one component CVPixelBuffer into a byte mask.
fn read_mask(
    vfns: &VideoFns,
    buffer: *const c_void,
    instances: usize,
) -> Result<ForegroundMask, Error> {
    // SAFETY: `buffer` is a live CVPixelBuffer for these calls (kept alive by the caller).
    let (bw, bh, stride, format) = unsafe {
        (
            (vfns.width)(buffer),
            (vfns.height)(buffer),
            (vfns.bytes_per_row)(buffer),
            (vfns.format)(buffer),
        )
    };
    let bytes_per_px = match format {
        FORMAT_L8 => 1,
        FORMAT_L16_HALF => 2,
        FORMAT_L32_FLOAT => 4,
        other => {
            return Err(Error::new(format!(
                "The system returned a mask in an unexpected format ({other:#x})."
            )));
        }
    };
    if bw == 0 || bh == 0 || stride < bw * bytes_per_px {
        return Err(Error::new("The system returned an empty mask."));
    }
    // SAFETY: lock for reading before touching the memory, unlock after, as CoreVideo requires.
    if unsafe { (vfns.lock)(buffer, LOCK_READ_ONLY) } != 0 {
        return Err(Error::new("The system would not let us read the mask."));
    }
    // SAFETY: while locked the base address points at `stride * bh` readable bytes.
    let base = unsafe { (vfns.base)(buffer) };
    let mut alpha = vec![0u8; bw * bh];
    if base.is_null() {
        // SAFETY: balances the lock above.
        unsafe { (vfns.unlock)(buffer, LOCK_READ_ONLY) };
        return Err(Error::new("The system returned an empty mask."));
    }
    for y in 0..bh {
        // SAFETY: row `y` starts `y * stride` bytes into a locked buffer of `stride * bh`
        // bytes and holds at least `bw * bytes_per_px` bytes (checked above).
        let row = unsafe { std::slice::from_raw_parts(base.add(y * stride), bw * bytes_per_px) };
        let out = &mut alpha[y * bw..(y + 1) * bw];
        match format {
            FORMAT_L8 => out.copy_from_slice(row),
            FORMAT_L16_HALF => {
                for (o, c) in out.iter_mut().zip(row.chunks_exact(2)) {
                    let v = half_to_f32(u16::from_le_bytes([c[0], c[1]]));
                    *o = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
            _ => {
                for (o, c) in out.iter_mut().zip(row.chunks_exact(4)) {
                    let v = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                    *o = (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                }
            }
        }
    }
    // SAFETY: balances the lock above.
    unsafe { (vfns.unlock)(buffer, LOCK_READ_ONLY) };
    Ok(ForegroundMask {
        width: bw as u32,
        height: bh as u32,
        alpha,
        instances,
    })
}

/// IEEE half precision to single.
fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0f32 } else { 1.0 };
    let exp = i32::from((h >> 10) & 0x1f);
    let frac = f32::from(h & 0x03ff);
    match exp {
        0 => sign * frac * 2f32.powi(-24),
        31 => {
            if frac == 0.0 {
                sign * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => sign * (1.0 + frac / 1024.0) * 2f32.powi(exp - 15),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_floats_decode() {
        assert_eq!(half_to_f32(0x3c00), 1.0);
        assert_eq!(half_to_f32(0x0000), 0.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0xbc00), -1.0);
    }

    #[test]
    fn bad_sizes_are_refused() {
        assert!(foreground_instance_mask(&[0; 5], 2, 2, 3).is_err());
        assert!(foreground_instance_mask(&[0; 12], 2, 2, 5).is_err());
    }

    #[test]
    fn a_flat_picture_has_no_subject_or_a_plain_error() {
        // A uniform gray square has no salient object; the call must fail with a sentence
        // rather than crash, whatever the Mac's Vision version does.
        let px = vec![128u8; 64 * 64 * 3];
        if let Err(e) = foreground_instance_mask(&px, 64, 64, 3) {
            assert!(!e.message().is_empty());
        }
    }
}

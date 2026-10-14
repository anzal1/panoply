//! Apple Vision (faces): face rectangles with landmarks, and the foreground subject mask.
//!
//! The engine behind the passport photo module on macOS (ADR 0004: the OS engine first). Vision
//! is opened with `dlopen` at the first call, so jobs that never need a face never load it, and
//! nothing stays resident for us: the framework belongs to the system.
//!
//! What is used:
//!
//! * `VNDetectFaceLandmarksRequest`: a box, a pose (roll, yaw, pitch when the OS gives them) and
//!   region point sets (jaw contour, eyes, pupils, brows, nose, lips) for each face. The points
//!   come back normalised to the face box with the origin at the bottom left; this module turns
//!   them into pixels with the origin at the top left, like every other image coordinate here.
//! * `VNGenerateForegroundInstanceMaskRequest` (macOS 14): a soft mask of everything the system
//!   calls a foreground subject, scaled to the picture. For a head and shoulders photo that is the
//!   person (and what they hold).
//!
//! Both run on the Neural Engine or GPU where there is one. Both take pixels the caller holds:
//! the caller passes tightly packed 8 bit RGB.

use std::ffi::{CStr, c_char, c_int, c_void};

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject};
use objc2_core_foundation::{CFData, CFRetained, CFString};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo, kCGColorSpaceSRGB,
};

use crate::cf::{self, sys};
use crate::error::Error;
use crate::sys as ffi;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
}

/// `RTLD_LAZY` on macOS.
const RTLD_LAZY: c_int = 0x1;

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

// SAFETY: `Point`, `Size` and `Rect` have the layout of CGPoint, CGSize and CGRect (`repr(C)`
// over `f64` fields) and the encodings are the ones the runtime reports for those types.
unsafe impl Encode for Point {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
}
// SAFETY: a pointer to a `Point` is encoded as a pointer to its encoding.
unsafe impl RefEncode for Point {
    const ENCODING_REF: Encoding = Encoding::Pointer(&<Self as Encode>::ENCODING);
}
// SAFETY: see `Point`.
unsafe impl Encode for Size {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
}
// SAFETY: see `Point`.
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Point::ENCODING, Size::ENCODING]);
}

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

/// A point set in picture pixels, origin top left.
pub type Points = Vec<(f64, f64)>;

/// One face found by Vision.
#[derive(Debug, Clone, Default)]
pub struct Face {
    /// Left edge of the face box, in pixels.
    pub x: f64,
    /// Top edge of the face box, in pixels.
    pub y: f64,
    /// Box width.
    pub width: f64,
    /// Box height.
    pub height: f64,
    /// How sure Vision is, 0 to 1.
    pub confidence: f32,
    /// Roll in degrees as Vision reports it (sign is Vision's own), when the OS gives it.
    pub roll_deg: Option<f32>,
    /// Yaw in degrees (turning left or right), when the OS gives it.
    pub yaw_deg: Option<f32>,
    /// Pitch in degrees (nodding), when the OS gives it.
    pub pitch_deg: Option<f32>,
    /// The jaw line from ear to ear through the chin.
    pub contour: Points,
    /// The eye on the left side of the picture: outline.
    pub eye_left: Points,
    /// The eye on the right side of the picture: outline.
    pub eye_right: Points,
    /// Pupil on the left side of the picture.
    pub pupil_left: Points,
    /// Pupil on the right side of the picture.
    pub pupil_right: Points,
    /// Left brow (left side of the picture).
    pub brow_left: Points,
    /// Right brow (right side of the picture).
    pub brow_right: Points,
    /// Nose outline.
    pub nose: Points,
    /// Outer lip outline.
    pub lips_outer: Points,
    /// Inner lip outline (the mouth opening).
    pub lips_inner: Points,
}

fn lib_loaded() -> bool {
    use std::sync::OnceLock;
    static LOADED: OnceLock<bool> = OnceLock::new();
    *LOADED.get_or_init(|| {
        // SAFETY: the paths are NUL terminated literals, and loading a system framework has no
        // preconditions. Vision pulls Foundation in with it.
        let v = unsafe {
            dlopen(
                c"/System/Library/Frameworks/Vision.framework/Vision".as_ptr(),
                RTLD_LAZY,
            )
        };
        !v.is_null()
    })
}

/// True when Vision can be used here: the framework opens and has the face landmarks request.
pub fn available() -> bool {
    lib_loaded() && AnyClass::get(c"VNDetectFaceLandmarksRequest").is_some()
}

/// True when the foreground subject mask request exists (macOS 14 and later).
pub fn subject_mask_available() -> bool {
    crate::vision::foreground_mask_available()
}

fn class(name: &CStr) -> Result<&'static AnyClass, Error> {
    if !lib_loaded() {
        return Err(Error::new("The Vision framework could not be opened."));
    }
    AnyClass::get(name).ok_or_else(|| Error::new("This Mac's Vision does not have that request."))
}

/// A CGImage over a copy of the RGB pixels (as opaque RGBA).
fn image_of(rgb: &[u8], width: u32, height: u32) -> Result<CFRetained<CGImage>, Error> {
    let (w, h) = (width as usize, height as usize);
    if w == 0 || h == 0 || rgb.len() != w * h * 3 {
        return Err(Error::new(
            "The pixel buffer does not match the image size.",
        ));
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    for p in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
    }
    let space = CGColorSpace::with_name(Some(sys!(kCGColorSpaceSRGB)))
        .ok_or_else(|| Error::new("The system could not make a color space."))?;
    let provider = CGDataProvider::with_cf_data(Some(&CFData::from_bytes(&rgba)))
        .ok_or_else(|| Error::new("The system could not wrap the pixels."))?;
    ffi::image_from_pixels(
        w,
        h,
        8,
        32,
        w * 4,
        &space,
        CGBitmapInfo(CGImageAlphaInfo::NoneSkipLast.0),
        &provider,
    )
    .ok_or_else(|| Error::new("The system could not build an image from these pixels."))
}

fn error_text(err: &AnyObject) -> String {
    autoreleasepool(|_| {
        // SAFETY: `localizedDescription` takes no arguments and returns an NSString.
        let s: Option<Retained<AnyObject>> = unsafe { msg_send![err, localizedDescription] };
        match s {
            Some(s) => {
                // SAFETY: an NSString is a CFString under toll free bridging.
                let cf: &CFString = unsafe { &*(&*s as *const AnyObject).cast::<CFString>() };
                cf.to_string()
            }
            None => "Vision reported an error.".to_string(),
        }
    })
}

/// A request handler over the image, plus the image it holds (kept alive with it).
struct Handler {
    object: Retained<AnyObject>,
    _image: CFRetained<CGImage>,
}

fn handler(rgb: &[u8], width: u32, height: u32) -> Result<Handler, Error> {
    let image = image_of(rgb, width, height)?;
    let cls = class(c"VNImageRequestHandler")?;
    let opts = cf::make_dict(&[]);
    // SAFETY: a CFDictionary is an NSDictionary under toll free bridging.
    let opts_obj: &AnyObject = unsafe { &*(&*opts as *const cf::PropDict).cast::<AnyObject>() };
    let image_ref = ImageRef((&*image as *const CGImage).cast());
    // SAFETY: `alloc` and `initWithCGImage:options:` take a live CGImage and an NSDictionary.
    // The handler retains the image, and `Handler` keeps its own reference as well.
    let object: Option<Retained<AnyObject>> = unsafe {
        let alloc: Allocated<AnyObject> = msg_send![cls, alloc];
        msg_send![alloc, initWithCGImage: image_ref, options: opts_obj]
    };
    Ok(Handler {
        object: object.ok_or_else(|| Error::new("Vision could not take this picture."))?,
        _image: image,
    })
}

/// Run one request on a handler.
fn perform(handler: &Handler, request: &AnyObject) -> Result<(), Error> {
    let nsarray = class(c"NSArray")?;
    // SAFETY: `arrayWithObject:` takes one object and returns an autoreleased array.
    let array: Retained<AnyObject> = unsafe { msg_send![nsarray, arrayWithObject: request] };
    let mut err: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: `performRequests:error:` takes an NSArray of requests and an NSError out
    // parameter (autoreleased, so it is only read inside the caller's autorelease pool).
    let ok: bool =
        unsafe { msg_send![&*handler.object, performRequests: &*array, error: &mut err] };
    if ok {
        return Ok(());
    }
    // SAFETY: on failure `err` is null or a live NSError for the current pool.
    let text = unsafe { err.as_ref() }.map_or_else(|| "no reason given".to_string(), error_text);
    Err(Error::new(format!(
        "Vision could not read this picture: {text}"
    )))
}

fn count_of(array: &AnyObject) -> usize {
    // SAFETY: `count` takes no arguments and returns an NSUInteger.
    unsafe { msg_send![array, count] }
}

fn object_at(array: &AnyObject, i: usize) -> Option<Retained<AnyObject>> {
    // SAFETY: `objectAtIndex:` takes an in-range index (callers use `count_of`).
    unsafe { msg_send![array, objectAtIndex: i] }
}

fn rect_of(obs: &AnyObject) -> Rect {
    // SAFETY: `boundingBox` takes no arguments and returns a CGRect.
    unsafe { msg_send![obs, boundingBox] }
}

fn number_f32(obj: Option<Retained<AnyObject>>) -> Option<f32> {
    let obj = obj?;
    // SAFETY: the property is an NSNumber, which answers `floatValue`.
    Some(unsafe { msg_send![&*obj, floatValue] })
}

/// A landmark region as pixel points, origin top left. `rect` is the face box in normalised
/// picture coordinates (origin bottom left), which is the space the region points are in.
fn region(landmarks: &AnyObject, name: &CStr, rect: Rect, w: f64, h: f64) -> Points {
    let sel_obj: Option<Retained<AnyObject>> = {
        let sel = objc2::runtime::Sel::register(name);
        // SAFETY: every landmark region property takes no arguments and returns a nullable
        // VNFaceLandmarkRegion2D object.
        unsafe { msg_send![landmarks, performSelector: sel] }
    };
    let Some(reg) = sel_obj else {
        return Vec::new();
    };
    // SAFETY: `pointCount` takes no arguments and returns an NSUInteger.
    let n: usize = unsafe { msg_send![&*reg, pointCount] };
    // SAFETY: `normalizedPoints` returns a pointer to `pointCount` CGPoints that lives as long
    // as the region object, which `reg` keeps alive while the points are copied below.
    let ptr: *const Point = unsafe { msg_send![&*reg, normalizedPoints] };
    if n == 0 || ptr.is_null() {
        return Vec::new();
    }
    // SAFETY: see above: `n` points are valid at `ptr`.
    let pts = unsafe { std::slice::from_raw_parts(ptr, n) };
    pts.iter()
        .map(|p| {
            let nx = rect.origin.x + p.x * rect.size.width;
            let ny = rect.origin.y + p.y * rect.size.height;
            (nx * w, (1.0 - ny) * h)
        })
        .collect()
}

/// Find the faces in an RGB picture, with landmarks. Faces come sorted from left to right.
pub fn detect_faces(rgb: &[u8], width: u32, height: u32) -> Result<Vec<Face>, Error> {
    autoreleasepool(|_| {
        let h = handler(rgb, width, height)?;
        let cls = class(c"VNDetectFaceLandmarksRequest")?;
        // SAFETY: `new` returns a retained, initialised request.
        let request: Retained<AnyObject> = unsafe { msg_send![cls, new] };
        perform(&h, &request)?;
        // SAFETY: `results` takes no arguments and returns a nullable NSArray.
        let results: Option<Retained<AnyObject>> = unsafe { msg_send![&*request, results] };
        let Some(results) = results else {
            return Ok(Vec::new());
        };
        let (w, hh) = (f64::from(width), f64::from(height));
        let mut faces = Vec::new();
        for i in 0..count_of(&results) {
            let Some(obs) = object_at(&results, i) else {
                continue;
            };
            let r = rect_of(&obs);
            let mut f = Face {
                x: r.origin.x * w,
                y: (1.0 - r.origin.y - r.size.height) * hh,
                width: r.size.width * w,
                height: r.size.height * hh,
                // SAFETY: `confidence` takes no arguments and returns a float.
                confidence: unsafe { msg_send![&*obs, confidence] },
                ..Face::default()
            };
            // SAFETY: roll, yaw and pitch are nullable NSNumbers (radians).
            unsafe {
                let roll: Option<Retained<AnyObject>> = msg_send![&*obs, roll];
                let yaw: Option<Retained<AnyObject>> = msg_send![&*obs, yaw];
                let pitch: Option<Retained<AnyObject>> = msg_send![&*obs, pitch];
                f.roll_deg = number_f32(roll).map(f32::to_degrees);
                f.yaw_deg = number_f32(yaw).map(f32::to_degrees);
                f.pitch_deg = number_f32(pitch).map(f32::to_degrees);
            }
            // SAFETY: `landmarks` takes no arguments and returns a nullable VNFaceLandmarks2D.
            let lm: Option<Retained<AnyObject>> = unsafe { msg_send![&*obs, landmarks] };
            if let Some(lm) = lm {
                f.contour = region(&lm, c"faceContour", r, w, hh);
                f.eye_left = region(&lm, c"leftEye", r, w, hh);
                f.eye_right = region(&lm, c"rightEye", r, w, hh);
                f.pupil_left = region(&lm, c"leftPupil", r, w, hh);
                f.pupil_right = region(&lm, c"rightPupil", r, w, hh);
                f.brow_left = region(&lm, c"leftEyebrow", r, w, hh);
                f.brow_right = region(&lm, c"rightEyebrow", r, w, hh);
                f.nose = region(&lm, c"nose", r, w, hh);
                f.lips_outer = region(&lm, c"outerLips", r, w, hh);
                f.lips_inner = region(&lm, c"innerLips", r, w, hh);
                // "Left" in Vision is the subject's left, which is on the right of the picture
                // for a camera that faces them. Name by picture side instead.
                let mean_x =
                    |p: &Points| p.iter().map(|q| q.0).sum::<f64>() / p.len().max(1) as f64;
                if !f.eye_left.is_empty()
                    && !f.eye_right.is_empty()
                    && mean_x(&f.eye_left) > mean_x(&f.eye_right)
                {
                    std::mem::swap(&mut f.eye_left, &mut f.eye_right);
                    std::mem::swap(&mut f.pupil_left, &mut f.pupil_right);
                    std::mem::swap(&mut f.brow_left, &mut f.brow_right);
                }
            }
            faces.push(f);
        }
        faces.sort_by(|a, b| a.x.total_cmp(&b.x));
        Ok(faces)
    })
}

/// A soft mask of the foreground subjects, one byte per pixel (255 is subject), the size of the
/// picture. `None` when Vision found no subject. Needs macOS 14 or later.
///
/// This is the same request as [`crate::foreground_instance_mask`]; there is one implementation
/// of that FFI, in `vision.rs`, and this only adapts its result to the face code's shape.
pub fn subject_mask(rgb: &[u8], width: u32, height: u32) -> Result<Option<Vec<u8>>, Error> {
    match crate::vision::find_foreground(rgb, width, height, 3)? {
        None => Ok(None),
        Some(mask) if (mask.width, mask.height) == (width, height) => Ok(Some(mask.alpha)),
        Some(_) => Err(Error::new("The subject mask had an unexpected format.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_with_no_face_gives_no_faces() {
        if !available() {
            return;
        }
        let (w, h) = (96u32, 96u32);
        let rgb = vec![200u8; (w * h * 3) as usize];
        assert!(detect_faces(&rgb, w, h).unwrap().is_empty());
    }

    #[test]
    fn a_wrong_buffer_is_refused() {
        assert!(detect_faces(&[0; 10], 4, 4).is_err());
    }
}

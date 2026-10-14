//! Text recognition through Apple Vision (`VNRecognizeTextRequest`), behind a safe API.
//!
//! Vision is opened with `dlopen` at the first call, so no other job in the process pays for
//! loading it, and its classes are found by name at run time: nothing here links the framework.
//! Any failure to find the framework or a class is reported as a plain [`Error`], and the OCR
//! ladder in `panoply-ocr` moves on to the next engine.
//!
//! Coordinates in the results are Vision's own: normalized to the image, origin at the bottom
//! left. The caller flips them.
//!
//! What Vision gives and does not give, measured on macOS 26.6: word boxes through
//! `boundingBoxForRange:error:`, one confidence per recognized line (every word of a line
//! carries its line's confidence), and no Hindi or Bengali (the supported language list in
//! accurate mode has Latin script languages, Chinese, Japanese, Korean, Russian, Ukrainian,
//! Thai, Vietnamese, Arabic and Turkish). [`supported_languages`] reports the list at run time.

use std::ffi::{c_char, c_int, c_void};
use std::sync::OnceLock;

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::msg_send;
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2_core_foundation::{CFArray, CFData, CFRetained, CFString};
use objc2_core_graphics::{CGBitmapInfo, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo};

use crate::error::Error;
use crate::sys;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
}

/// `RTLD_LAZY` on macOS.
const RTLD_LAZY: c_int = 0x1;

/// Largest side of an image handed to Vision, in pixels. Larger pages are the caller's job to
/// reduce: Vision itself scales, but a 20,000 pixel side costs memory for nothing.
pub const MAX_SIDE: usize = 16_384;

/// One recognized word with its box.
#[derive(Debug, Clone, PartialEq)]
pub struct WordBox {
    /// The word.
    pub text: String,
    /// Left edge, 0.0 to 1.0 of the image width.
    pub x: f32,
    /// Bottom edge, 0.0 to 1.0 of the image height (origin at the bottom).
    pub y: f32,
    /// Width, 0.0 to 1.0 of the image width.
    pub w: f32,
    /// Height, 0.0 to 1.0 of the image height.
    pub h: f32,
}

/// One recognized line of text.
#[derive(Debug, Clone, PartialEq)]
pub struct Observation {
    /// The whole line.
    pub text: String,
    /// Vision's confidence for the line, 0.0 to 1.0.
    pub confidence: f32,
    /// The line's words with boxes. Never empty: a line Vision gives no word boxes for comes
    /// back as one word covering the line.
    pub words: Vec<WordBox>,
}

/// What to ask Vision for.
#[derive(Debug, Clone)]
pub struct Request<'a> {
    /// Language codes in Vision's notation (`en-US`, `fr-FR`, `zh-Hans`). Empty means automatic.
    pub languages: &'a [&'a str],
    /// Let Vision detect the language (macOS 13 and later). Ignored when `languages` is set.
    pub auto_detect: bool,
    /// `true` for the accurate level, `false` for fast.
    pub accurate: bool,
    /// Correct words against a dictionary. Helps prose, hurts codes and numbers.
    pub language_correction: bool,
}

impl Default for Request<'_> {
    fn default() -> Self {
        Request {
            languages: &[],
            auto_detect: true,
            accurate: true,
            language_correction: true,
        }
    }
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
// and four `f64` fields, `repr(C)`), and these are the encodings the runtime reports for them.
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

/// `NSRange`: a location and a length in UTF-16 code units.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
struct Range {
    location: usize,
    length: usize,
}

// SAFETY: `Range` has the layout of NSRange (two `NSUInteger`, `repr(C)`), and the encoding is
// the one the runtime reports for it.
unsafe impl Encode for Range {
    const ENCODING: Encoding = Encoding::Struct("_NSRange", &[usize::ENCODING, usize::ENCODING]);
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

/// Opens Vision once. `false` when the framework is not there (it always is on macOS 10.13 and
/// later, so this is for a stripped or very old system).
fn open_vision() -> bool {
    static OPEN: OnceLock<bool> = OnceLock::new();
    *OPEN.get_or_init(|| {
        // SAFETY: the path is a NUL terminated literal, and loading a system framework has no
        // preconditions.
        let handle = unsafe {
            dlopen(
                c"/System/Library/Frameworks/Vision.framework/Vision".as_ptr(),
                RTLD_LAZY,
            )
        };
        !handle.is_null()
    })
}

/// True when Vision can be opened and has the text recognition class.
pub fn available() -> bool {
    open_vision() && AnyClass::get(c"VNRecognizeTextRequest").is_some()
}

fn class(name: &'static std::ffi::CStr) -> Result<&'static AnyClass, Error> {
    if !open_vision() {
        return Err(Error::new("Apple Vision is not available on this Mac."));
    }
    AnyClass::get(name).ok_or_else(|| Error::new("Apple Vision is missing a part this needs."))
}

/// The languages Vision recognizes in the accurate (or fast) level, in its own notation
/// (`en-US`, `fr-FR`, `ja-JP`, ...). Empty when Vision is unavailable.
pub fn supported_languages(accurate: bool) -> Vec<String> {
    let Ok(cls) = class(c"VNRecognizeTextRequest") else {
        return Vec::new();
    };
    autoreleasepool(|_| {
        // SAFETY: `new` takes no arguments and returns a retained request.
        let req: Retained<AnyObject> = unsafe { msg_send![cls, new] };
        let level: isize = if accurate { 0 } else { 1 };
        // SAFETY: `setRecognitionLevel:` takes one NSInteger.
        unsafe {
            let _: () = msg_send![&*req, setRecognitionLevel: level];
        }
        let mut err: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: `supportedRecognitionLanguagesAndReturnError:` takes an NSError out pointer
        // and returns a nullable NSArray of NSString.
        let arr: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*req, supportedRecognitionLanguagesAndReturnError: &mut err] };
        arr.map(|a| strings_of(&a)).unwrap_or_default()
    })
}

fn strings_of(array: &AnyObject) -> Vec<String> {
    // SAFETY: `count` takes no arguments and returns an NSUInteger.
    let n: usize = unsafe { msg_send![array, count] };
    (0..n)
        .filter_map(|i| {
            // SAFETY: `objectAtIndex:` with an in range index returns an object (here NSString).
            let o: Option<Retained<AnyObject>> = unsafe { msg_send![array, objectAtIndex: i] };
            o.map(|o| ns_string(&o))
        })
        .collect()
}

/// An NSString as a Rust string.
fn ns_string(s: &AnyObject) -> String {
    // SAFETY: NSString and CFString are toll free bridged, and the caller passes an NSString.
    let cf: &CFString = unsafe { &*(s as *const AnyObject).cast::<CFString>() };
    cf.to_string()
}

/// Builds the CGImage Vision reads: 8 bit gray for one channel, RGB (alpha skipped) for three.
fn make_image(
    width: usize,
    height: usize,
    channels: usize,
    pixels: &[u8],
) -> Result<CFRetained<CGImage>, Error> {
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(Error::new(
            "This picture is too large or too small to read.",
        ));
    }
    if !(channels == 1 || channels == 3) || pixels.len() != width * height * channels {
        return Err(Error::new("The picture's pixels do not match its size."));
    }
    let (data, bpp, space, info) = if channels == 1 {
        (
            CFData::from_bytes(pixels),
            8,
            CGColorSpace::new_device_gray(),
            CGBitmapInfo(CGImageAlphaInfo::None.0),
        )
    } else {
        // CoreGraphics has no 24 bit pixel: widen to four bytes with the last one skipped.
        let mut rgbx = Vec::with_capacity(width * height * 4);
        for px in pixels.chunks_exact(3) {
            rgbx.extend_from_slice(&[px[0], px[1], px[2], 255]);
        }
        (
            CFData::from_bytes(&rgbx),
            32,
            CGColorSpace::new_device_rgb(),
            CGBitmapInfo(CGImageAlphaInfo::NoneSkipLast.0),
        )
    };
    let space = space.ok_or_else(|| Error::new("The color space for this picture is missing."))?;
    let provider = CGDataProvider::with_cf_data(Some(&data))
        .ok_or_else(|| Error::new("The picture could not be handed to Apple Vision."))?;
    sys::image_from_pixels(
        width,
        height,
        8,
        bpp,
        width * bpp / 8,
        &space,
        info,
        &provider,
    )
    .ok_or_else(|| Error::new("The picture could not be handed to Apple Vision."))
}

/// Recognizes the text in an 8 bit picture of `channels` 1 (gray) or 3 (RGB), row major.
pub fn recognize(
    width: usize,
    height: usize,
    channels: usize,
    pixels: &[u8],
    request: &Request<'_>,
) -> Result<Vec<Observation>, Error> {
    let request_class = class(c"VNRecognizeTextRequest")?;
    let handler_class = class(c"VNImageRequestHandler")?;
    let dict_class = class(c"NSDictionary")?;
    let array_class = class(c"NSArray")?;
    let image = make_image(width, height, channels, pixels)?;
    autoreleasepool(|_| {
        // SAFETY: `new` takes no arguments and returns a retained request.
        let req: Retained<AnyObject> = unsafe { msg_send![request_class, new] };
        let level: isize = if request.accurate { 0 } else { 1 };
        // SAFETY: each setter takes one scalar (NSInteger or BOOL) as declared by Vision.
        unsafe {
            let _: () = msg_send![&*req, setRecognitionLevel: level];
            let _: () =
                msg_send![&*req, setUsesLanguageCorrection: Bool::new(request.language_correction)];
        }
        if request.languages.is_empty() {
            let responds: Bool = {
                // SAFETY: `respondsToSelector:` takes a selector and returns BOOL.
                unsafe {
                    msg_send![&*req, respondsToSelector: objc2::sel!(setAutomaticallyDetectsLanguage:)]
                }
            };
            if responds.as_bool() {
                // SAFETY: `setAutomaticallyDetectsLanguage:` takes one BOOL (macOS 13 and later,
                // checked just above).
                unsafe {
                    let _: () = msg_send![&*req, setAutomaticallyDetectsLanguage: Bool::new(request.auto_detect)];
                }
            }
        } else {
            let names: Vec<CFRetained<CFString>> = request
                .languages
                .iter()
                .map(|l| CFString::from_str(l))
                .collect();
            let arr = CFArray::from_retained_objects(&names);
            // SAFETY: a CFArray of CFStrings is an NSArray of NSStrings under toll free
            // bridging, and `setRecognitionLanguages:` takes an NSArray of NSString.
            unsafe {
                let arr_obj: &AnyObject = &*(&*arr as *const CFArray<CFString>).cast::<AnyObject>();
                let _: () = msg_send![&*req, setRecognitionLanguages: arr_obj];
            }
        }
        // SAFETY: `dictionary` takes no arguments and returns an empty NSDictionary.
        let options: Retained<AnyObject> = unsafe { msg_send![dict_class, dictionary] };
        let cg = ImageRef((&*image as *const CGImage).cast());
        // SAFETY: `initWithCGImage:options:` takes a live CGImageRef and an NSDictionary and
        // returns the initialized handler. The image outlives the handler (both end in this
        // block).
        let handler: Retained<AnyObject> = unsafe {
            msg_send![msg_send![handler_class, alloc], initWithCGImage: cg, options: &*options]
        };
        // SAFETY: `arrayWithObject:` takes one object and returns an NSArray holding it.
        let requests: Retained<AnyObject> =
            unsafe { msg_send![array_class, arrayWithObject: &*req] };
        let mut err: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: `performRequests:error:` takes an NSArray of requests and an NSError out
        // pointer, and returns BOOL.
        let ok: Bool =
            unsafe { msg_send![&*handler, performRequests: &*requests, error: &mut err] };
        if !ok.as_bool() {
            return Err(Error::new(
                "Apple Vision could not read text from this picture.",
            ));
        }
        // SAFETY: `results` takes no arguments and returns a nullable NSArray.
        let results: Option<Retained<AnyObject>> = unsafe { msg_send![&*req, results] };
        let Some(results) = results else {
            return Ok(Vec::new());
        };
        // SAFETY: `count` takes no arguments and returns an NSUInteger.
        let n: usize = unsafe { msg_send![&*results, count] };
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            // SAFETY: `objectAtIndex:` with an in range index returns the observation.
            let obs: Option<Retained<AnyObject>> =
                unsafe { msg_send![&*results, objectAtIndex: i] };
            if let Some(obs) = obs
                && let Some(o) = observation(&obs)
            {
                out.push(o);
            }
        }
        Ok(out)
    })
}

/// One `VNRecognizedTextObservation` as an [`Observation`]. `None` when it has no text.
fn observation(obs: &AnyObject) -> Option<Observation> {
    // SAFETY: `topCandidates:` takes an NSUInteger and returns an NSArray of recognized text.
    let cands: Retained<AnyObject> = unsafe { msg_send![obs, topCandidates: 1usize] };
    // SAFETY: `count` takes no arguments and returns an NSUInteger.
    let n: usize = unsafe { msg_send![&*cands, count] };
    if n == 0 {
        return None;
    }
    // SAFETY: `objectAtIndex:` with index 0 of a non empty array returns the candidate.
    let cand: Retained<AnyObject> = unsafe { msg_send![&*cands, objectAtIndex: 0usize] };
    // SAFETY: `string` takes no arguments and returns an NSString.
    let s: Retained<AnyObject> = unsafe { msg_send![&*cand, string] };
    let text = ns_string(&s);
    if text.trim().is_empty() {
        return None;
    }
    // SAFETY: `confidence` takes no arguments and returns a float.
    let confidence: f32 = unsafe { msg_send![&*cand, confidence] };
    // SAFETY: `boundingBox` takes no arguments and returns a CGRect.
    let whole: Rect = unsafe { msg_send![obs, boundingBox] };
    let line_box = WordBox {
        text: text.clone(),
        x: whole.origin.x as f32,
        y: whole.origin.y as f32,
        w: whole.size.width as f32,
        h: whole.size.height as f32,
    };
    let mut words = Vec::new();
    let mut utf16 = 0usize;
    let mut start: Option<(usize, usize)> = None; // (utf16 start, byte start)
    let bytes_end = text.len();
    let mut flush = |from: (usize, usize), to_utf16: usize, to_byte: usize| {
        let word = &text[from.1..to_byte];
        let range = Range {
            location: from.0,
            length: to_utf16 - from.0,
        };
        let mut err: *mut AnyObject = std::ptr::null_mut();
        // SAFETY: `boundingBoxForRange:error:` takes an NSRange inside the string (it is built
        // from the string's own UTF-16 offsets) and an NSError out pointer, and returns a
        // nullable rectangle observation.
        let r: Option<Retained<AnyObject>> =
            unsafe { msg_send![&*cand, boundingBoxForRange: range, error: &mut err] };
        let rect = r.map(|r| {
            // SAFETY: `boundingBox` takes no arguments and returns a CGRect.
            let b: Rect = unsafe { msg_send![&*r, boundingBox] };
            b
        });
        if let Some(b) = rect {
            words.push(WordBox {
                text: word.to_string(),
                x: b.origin.x as f32,
                y: b.origin.y as f32,
                w: b.size.width as f32,
                h: b.size.height as f32,
            });
        }
    };
    for (byte, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if let Some(from) = start.take() {
                flush(from, utf16, byte);
            }
        } else if start.is_none() {
            start = Some((utf16, byte));
        }
        utf16 += ch.len_utf16();
    }
    if let Some(from) = start.take() {
        flush(from, utf16, bytes_end);
    }
    if words.is_empty() {
        words.push(line_box);
    }
    Some(Observation {
        text,
        confidence,
        words,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_lists_its_languages() {
        let langs = supported_languages(true);
        assert!(langs.iter().any(|l| l.starts_with("en")), "{langs:?}");
    }

    #[test]
    fn blank_picture_has_no_text() {
        let px = vec![255u8; 64 * 64];
        let out = recognize(64, 64, 1, &px, &Request::default()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn mismatched_pixels_are_refused() {
        let px = vec![0u8; 10];
        assert!(recognize(64, 64, 1, &px, &Request::default()).is_err());
        assert!(recognize(0, 64, 1, &px, &Request::default()).is_err());
    }
}

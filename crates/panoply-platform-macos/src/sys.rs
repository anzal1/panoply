//! Thin safe wrappers over the ImageIO and CoreGraphics calls this crate needs. All `unsafe`
//! in the crate is in this file, each block with its reason. The generated bindings mark
//! nearly every call `unsafe` because they cannot prove arguments are valid CF objects. Here
//! every argument is a typed reference to a live object, which is what the calls require.

use std::ffi::c_void;
use std::ptr::NonNull;

use objc2_core_foundation::{
    CFArray, CFData, CFDictionary, CFMutableData, CFRetained, CFString, CGAffineTransform, CGRect,
};
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGContext, CGDataProvider, CGImage,
    CGInterpolationQuality,
};
use objc2_image_io::{CGImageDestination, CGImageSource, CGImageSourceStatus};

use crate::cf::{self, PropDict};

pub(crate) fn source_with_data(
    data: &CFData,
    options: Option<&CFDictionary>,
) -> Option<CFRetained<CGImageSource>> {
    // SAFETY: `data` and `options` are live CF objects of the right type. ImageIO retains the
    // data for the life of the source, so the caller may drop its own reference.
    unsafe { CGImageSource::with_data(data, options) }
}

pub(crate) fn source_count(src: &CGImageSource) -> usize {
    // SAFETY: `src` is a live CGImageSource.
    unsafe { src.count() }
}

pub(crate) fn source_primary_index(src: &CGImageSource) -> usize {
    // SAFETY: `src` is a live CGImageSource.
    unsafe { src.primary_image_index() }
}

pub(crate) fn source_status_at(src: &CGImageSource, index: usize) -> CGImageSourceStatus {
    // SAFETY: `src` is a live CGImageSource. An out of range index returns an error status.
    unsafe { src.status_at_index(index) }
}

pub(crate) fn source_uti(src: &CGImageSource) -> Option<String> {
    // SAFETY: `src` is a live CGImageSource. The returned string is retained by the wrapper.
    unsafe { src.r#type() }.map(|s| s.to_string())
}

pub(crate) fn source_props_at(
    src: &CGImageSource,
    index: usize,
    options: Option<&CFDictionary>,
) -> Option<CFRetained<PropDict>> {
    // SAFETY: `src` is a live CGImageSource and `options` a live dictionary or none.
    unsafe { src.properties_at_index(index, options) }.map(cf::typed)
}

pub(crate) fn source_image_at(
    src: &CGImageSource,
    index: usize,
    options: Option<&CFDictionary>,
) -> Option<CFRetained<CGImage>> {
    // SAFETY: `src` is a live CGImageSource and `options` a live dictionary or none. ImageIO
    // returns null for an invalid index or undecodable header, which maps to `None`.
    unsafe { src.image_at_index(index, options) }
}

pub(crate) fn source_thumbnail_at(
    src: &CGImageSource,
    index: usize,
    options: Option<&CFDictionary>,
) -> Option<CFRetained<CGImage>> {
    // SAFETY: `src` is a live CGImageSource and `options` a live dictionary or none. ImageIO
    // returns null when it cannot make a thumbnail, which maps to `None`.
    unsafe { src.thumbnail_at_index(index, options) }
}

pub(crate) fn source_aux_exists(src: &CGImageSource, index: usize, kind: &CFString) -> bool {
    // SAFETY: `src` is a live CGImageSource and `kind` a live CFString constant.
    unsafe { src.auxiliary_data_info_at_index(index, kind) }.is_some()
}

pub(crate) fn source_type_identifiers() -> Vec<String> {
    // SAFETY: takes no arguments and returns a retained CFArray of CFString identifiers.
    let array = unsafe { CGImageSource::type_identifiers() };
    strings_of(array)
}

pub(crate) fn destination_type_identifiers() -> Vec<String> {
    // SAFETY: takes no arguments and returns a retained CFArray of CFString identifiers.
    let array = unsafe { CGImageDestination::type_identifiers() };
    strings_of(array)
}

fn strings_of(array: CFRetained<CFArray>) -> Vec<String> {
    // SAFETY: both ImageIO calls above document an array of UTI strings. The cast only changes
    // the static element type.
    let typed: CFRetained<CFArray<CFString>> = unsafe { CFRetained::cast_unchecked(array) };
    typed.iter().map(|s| s.to_string()).collect()
}

pub(crate) fn dest_with_data(
    data: &CFMutableData,
    uti: &CFString,
) -> Option<CFRetained<CGImageDestination>> {
    // SAFETY: `data` is a live mutable CFData that outlives the destination (the caller keeps
    // both until after `dest_finalize`). Count 1 means one image will be added.
    unsafe { CGImageDestination::with_data(data, uti, 1, None) }
}

pub(crate) fn dest_add_image(
    dest: &CGImageDestination,
    image: &CGImage,
    props: Option<&CFDictionary>,
) {
    // SAFETY: `dest` and `image` are live and `props` is a live dictionary or none.
    unsafe { dest.add_image(image, props) }
}

pub(crate) fn dest_finalize(dest: &CGImageDestination) -> bool {
    // SAFETY: `dest` is a live destination. Finalize is called once.
    unsafe { dest.finalize() }
}

pub(crate) fn colorspace_from_icc(icc: &CFData) -> Option<CFRetained<CGColorSpace>> {
    let as_type: &objc2_core_foundation::CFType = icc.as_ref();
    // SAFETY: `as_type` is a live CFData. CGColorSpaceCreateWithICCData returns null when the
    // bytes are not a usable profile, which maps to `None`.
    unsafe { CGColorSpace::with_icc_data(Some(as_type)) }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn image_from_pixels(
    width: usize,
    height: usize,
    bits_per_component: usize,
    bits_per_pixel: usize,
    bytes_per_row: usize,
    space: &CGColorSpace,
    bitmap_info: CGBitmapInfo,
    provider: &CGDataProvider,
) -> Option<CFRetained<CGImage>> {
    // SAFETY: the provider holds an immutable CFData copy of the pixels with at least
    // `bytes_per_row * height` bytes (checked by the caller), a null decode array means the
    // default decode, and the other arguments describe that layout.
    unsafe {
        CGImage::new(
            width,
            height,
            bits_per_component,
            bits_per_pixel,
            bytes_per_row,
            Some(space),
            bitmap_info,
            Some(provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}

unsafe extern "C-unwind" {
    // Not bound by objc2-core-graphics 0.3.
    fn CGBitmapContextCreate(
        data: *mut c_void,
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: Option<&CGColorSpace>,
        bitmap_info: u32,
    ) -> Option<NonNull<CGContext>>;
}

/// A bitmap context drawing into caller owned memory.
///
/// The context must not be used after the buffer it was made over is gone, so the type has a
/// lifetime tied to a mutable borrow of the buffer.
pub(crate) struct BitmapContext<'buf> {
    ctx: CFRetained<CGContext>,
    _buffer: std::marker::PhantomData<&'buf mut [u8]>,
}

impl<'buf> BitmapContext<'buf> {
    /// Creates a context over `buffer`. The buffer must hold `bytes_per_row * height` bytes.
    pub(crate) fn new(
        buffer: &'buf mut [u8],
        width: usize,
        height: usize,
        bits_per_component: usize,
        bytes_per_row: usize,
        space: &CGColorSpace,
        bitmap_info: u32,
    ) -> Option<Self> {
        let needed = bytes_per_row.checked_mul(height)?;
        if buffer.len() < needed || bytes_per_row == 0 || height == 0 {
            return None;
        }
        // SAFETY: the buffer is valid for `bytes_per_row * height` writable bytes (checked
        // above) and is exclusively borrowed for `'buf`, which bounds the context's life.
        // CoreGraphics rejects unsupported pixel formats by returning null.
        let raw = unsafe {
            CGBitmapContextCreate(
                buffer.as_mut_ptr().cast(),
                width,
                height,
                bits_per_component,
                bytes_per_row,
                Some(space),
                bitmap_info,
            )
        }?;
        // SAFETY: CGBitmapContextCreate follows the Create rule, so we own one retain count.
        let ctx = unsafe { CFRetained::from_raw(raw) };
        Some(BitmapContext {
            ctx,
            _buffer: std::marker::PhantomData,
        })
    }

    /// Draws `image` into the context. `upright` maps stored image space to upright
    /// space and `shift` then moves the wanted band into the context.
    pub(crate) fn draw_transformed(
        &self,
        image: &CGImage,
        shift: CGAffineTransform,
        upright: CGAffineTransform,
        dest: CGRect,
    ) {
        let ctx = Some(&*self.ctx);
        CGContext::set_interpolation_quality(ctx, CGInterpolationQuality::None);
        CGContext::set_should_antialias(ctx, false);
        CGContext::set_allows_antialiasing(ctx, false);
        CGContext::set_rendering_intent(ctx, CGColorRenderingIntent::RenderingIntentDefault);
        // The context CTM starts as identity. Concatenating `shift` and then `upright` makes
        // a point go through `upright` first and `shift` second.
        CGContext::concat_ctm(ctx, shift);
        CGContext::concat_ctm(ctx, upright);
        let rect = dest;
        CGContext::draw_image(ctx, rect, Some(image));
    }
}

/// Views a `u16` buffer as bytes so one context code path serves both sample sizes.
pub(crate) fn u16_as_bytes_mut(samples: &mut [u16]) -> &mut [u8] {
    let len = std::mem::size_of_val(samples);
    // SAFETY: a `u16` slice is `len` initialized bytes, `u8` has alignment 1 and no invalid bit
    // patterns, and the result borrows the input mutably so nothing else can alias it.
    unsafe { std::slice::from_raw_parts_mut(samples.as_mut_ptr().cast::<u8>(), len) }
}

/// Views a `u16` slice as bytes (read only).
pub(crate) fn u16_as_bytes(samples: &[u16]) -> &[u8] {
    let len = std::mem::size_of_val(samples);
    // SAFETY: same reasoning as `u16_as_bytes_mut`, for a shared borrow.
    unsafe { std::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), len) }
}

/// Cuts a rectangle (stored pixel coordinates, origin top left) out of a lazy image. ImageIO
/// keeps the result lazy, so only the tiles under the rectangle are decoded when it is drawn.
pub(crate) fn crop_image(image: &CGImage, rect: CGRect) -> Option<CFRetained<CGImage>> {
    CGImage::with_image_in_rect(Some(image), rect)
}

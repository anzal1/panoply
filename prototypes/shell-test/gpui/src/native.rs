//! Thin AppKit helpers. GPUI owns the NSPanel; everything here only adjusts it
//! (shape, level, alpha, layer transform, positioning) through plain Objective-C messages.
#![allow(clippy::missing_safety_doc)]

use gpui::Window;
use objc2::encode::{Encode, Encoding};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{SystemTime, UNIX_EPOCH};

pub type Obj = *mut AnyObject;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Sz {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct Rect {
    pub o: Pt,
    pub s: Sz,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Affine {
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    tx: f64,
    ty: f64,
}

unsafe impl Encode for Pt {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]);
}
unsafe impl Encode for Sz {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]);
}
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[Pt::ENCODING, Sz::ENCODING]);
}
unsafe impl Encode for Affine {
    const ENCODING: Encoding = Encoding::Struct(
        "CGAffineTransform",
        &[Encoding::Double, Encoding::Double, Encoding::Double, Encoding::Double, Encoding::Double, Encoding::Double],
    );
}

pub fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0
}

/// The NSWindow that hosts this GPUI window.
pub fn ns_window(window: &Window) -> Option<Obj> {
    let h = HasWindowHandle::window_handle(window).ok()?;
    match h.as_raw() {
        RawWindowHandle::AppKit(a) => {
            let view = a.ns_view.as_ptr() as Obj;
            let w: Obj = unsafe { msg_send![view, window] };
            (!w.is_null()).then_some(w)
        }
        _ => None,
    }
}

const BORDERLESS_NONACTIVATING: usize = 1 << 7; // NSWindowStyleMaskNonactivatingPanel
const COLLECTION: usize = (1 << 1) | (1 << 6) | (1 << 8); // moveToActiveSpace | ignoresCycle | fullScreenAuxiliary

/// One-time panel setup: borderless, rounded layer mask, floating level, system shadow.
pub fn setup_panel(w: Obj, radius: f64) {
    unsafe {
        let _: () = msg_send![w, setStyleMask: BORDERLESS_NONACTIVATING];
        let _: () = msg_send![w, setHasShadow: Bool::YES];
        let _: () = msg_send![w, setLevel: 3isize]; // NSFloatingWindowLevel
        let _: () = msg_send![w, setCollectionBehavior: COLLECTION];
        let _: () = msg_send![w, setAnimationBehavior: 2isize]; // none
        let _: () = msg_send![w, setHidesOnDeactivate: Bool::NO];
        let _: () = msg_send![w, setMovable: Bool::NO];
        let cv: Obj = msg_send![w, contentView];
        let _: () = msg_send![cv, setWantsLayer: Bool::YES];
        let layer: Obj = msg_send![cv, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setCornerRadius: radius];
            let _: () = msg_send![layer, setMasksToBounds: Bool::YES];
        }
        let _: () = msg_send![w, invalidateShadow];
    }
}

pub fn set_alpha(w: Obj, a: f64) {
    unsafe {
        let _: () = msg_send![w, setAlphaValue: a];
    }
}

pub fn set_ignores_mouse(w: Obj, v: bool) {
    unsafe {
        let _: () = msg_send![w, setIgnoresMouseEvents: Bool::new(v)];
    }
}

pub fn order_front_key(w: Obj) {
    unsafe {
        let _: () = msg_send![w, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
    }
}

pub fn order_front_regardless(w: Obj) {
    unsafe {
        let _: () = msg_send![w, orderFrontRegardless];
    }
}

pub fn order_out(w: Obj) {
    unsafe {
        let _: () = msg_send![w, orderOut: std::ptr::null_mut::<AnyObject>()];
    }
}

pub fn is_key(w: Obj) -> bool {
    unsafe { let b: Bool = msg_send![w, isKeyWindow]; b.as_bool() }
}
pub fn is_visible(w: Obj) -> bool {
    unsafe { let b: Bool = msg_send![w, isVisible]; b.as_bool() }
}
pub fn frame(w: Obj) -> Rect {
    unsafe { msg_send![w, frame] }
}
pub fn alpha(w: Obj) -> f64 {
    unsafe { msg_send![w, alphaValue] }
}
pub fn level(w: Obj) -> isize {
    unsafe { msg_send![w, level] }
}
pub fn style_mask(w: Obj) -> usize {
    unsafe { msg_send![w, styleMask] }
}

pub fn invalidate_shadow(w: Obj) {
    unsafe {
        let _: () = msg_send![w, invalidateShadow];
    }
}

/// Scale about the bar centre plus a vertical offset (points, positive = up), applied to the content layer.
pub fn set_motion(w: Obj, scale: f64, dy: f64) {
    unsafe {
        let cv: Obj = msg_send![w, contentView];
        let layer: Obj = msg_send![cv, layer];
        if layer.is_null() {
            return;
        }
        let bounds: Rect = msg_send![layer, bounds];
        let anchor: Pt = msg_send![layer, anchorPoint];
        // The layer transform is applied about its anchor point; compensate so the scale pivots on the centre.
        let (cx, cy) = (bounds.s.w / 2.0, bounds.s.h / 2.0);
        let (ax, ay) = (anchor.x * bounds.s.w, anchor.y * bounds.s.h);
        let t = Affine { a: scale, b: 0.0, c: 0.0, d: scale, tx: (cx - ax) * (1.0 - scale), ty: (cy - ay) * (1.0 - scale) + dy };
        let txn = class!(CATransaction);
        let _: () = msg_send![txn, begin];
        let _: () = msg_send![txn, setDisableActions: Bool::YES];
        let _: () = msg_send![layer, setAffineTransform: t];
        let _: () = msg_send![txn, commit];
    }
}

/// Place the panel horizontally centred with its top edge at `top_frac` of the screen height,
/// on the screen under the mouse.
pub fn position(w: Obj, width: f64, height: f64, top_frac: f64) {
    unsafe {
        let mouse: Pt = msg_send![class!(NSEvent), mouseLocation];
        let screens: Obj = msg_send![class!(NSScreen), screens];
        let n: usize = msg_send![screens, count];
        let mut chosen: Option<Rect> = None;
        for i in 0..n {
            let s: Obj = msg_send![screens, objectAtIndex: i];
            let f: Rect = msg_send![s, frame];
            if mouse.x >= f.o.x && mouse.x < f.o.x + f.s.w && mouse.y >= f.o.y && mouse.y < f.o.y + f.s.h {
                chosen = Some(f);
                break;
            }
        }
        let f = chosen.unwrap_or_else(|| {
            let m: Obj = msg_send![class!(NSScreen), mainScreen];
            msg_send![m, frame]
        });
        let origin = Pt {
            x: (f.o.x + f.s.w / 2.0 - width / 2.0).round(),
            y: (f.o.y + f.s.h - f.s.h * top_frac - height).round(),
        };
        let _: () = msg_send![w, setFrameOrigin: origin];
    }
}

pub fn reduce_motion() -> bool {
    unsafe {
        let ws: Obj = msg_send![class!(NSWorkspace), sharedWorkspace];
        let b: Bool = msg_send![ws, accessibilityDisplayShouldReduceMotion];
        b.as_bool()
    }
}

/// System accent colour as sRGB 0..1, falling back to system blue.
pub fn accent_rgb() -> (f32, f32, f32) {
    unsafe {
        let c: Obj = msg_send![class!(NSColor), controlAccentColor];
        let space: Obj = msg_send![class!(NSColorSpace), sRGBColorSpace];
        let conv: Obj = msg_send![c, colorUsingColorSpace: space];
        if conv.is_null() {
            return (0.0, 0.478, 1.0);
        }
        let r: f64 = msg_send![conv, redComponent];
        let g: f64 = msg_send![conv, greenComponent];
        let b: f64 = msg_send![conv, blueComponent];
        (r as f32, g as f32, b as f32)
    }
}

pub fn blur_view_count(w: Obj) -> usize {
    unsafe {
        let cv: Obj = msg_send![w, contentView];
        let subs: Obj = msg_send![cv, subviews];
        let n: usize = msg_send![subs, count];
        let mut c = 0;
        for i in 0..n {
            let s: Obj = msg_send![subs, objectAtIndex: i];
            let is_fx: Bool = msg_send![s, isKindOfClass: class!(NSVisualEffectView)];
            if is_fx.as_bool() {
                c += 1;
            }
        }
        c
    }
}

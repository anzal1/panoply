//! AppKit helpers. GPUI owns the NSPanel; everything here only adjusts it (shape, level, alpha,
//! layer transform, position) through plain Objective-C messages, as in the shell kill test.

use std::ffi::CString;
use std::path::Path;

use gpui::Window;
use objc2::encode::{Encode, Encoding};
use objc2::runtime::{AnyObject, Bool};
use objc2::{class, msg_send};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

pub(crate) type Obj = *mut AnyObject;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Pt {
    pub x: f64,
    pub y: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sz {
    pub w: f64,
    pub h: f64,
}
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Rect {
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
        &[
            Encoding::Double,
            Encoding::Double,
            Encoding::Double,
            Encoding::Double,
            Encoding::Double,
            Encoding::Double,
        ],
    );
}

/// A handle to the NSWindow behind a GPUI window.
#[derive(Clone, Copy)]
pub(crate) struct Panel(Obj);

const NONACTIVATING_PANEL: usize = 1 << 7;
// moveToActiveSpace | ignoresCycle | fullScreenAuxiliary
const COLLECTION: usize = (1 << 1) | (1 << 6) | (1 << 8);

impl Panel {
    /// The NSWindow that hosts this GPUI window.
    pub(crate) fn of(window: &Window) -> Option<Panel> {
        let h = HasWindowHandle::window_handle(window).ok()?;
        match h.as_raw() {
            RawWindowHandle::AppKit(a) => {
                let view = a.ns_view.as_ptr() as Obj;
                let w: Obj = unsafe { msg_send![view, window] };
                (!w.is_null()).then_some(Panel(w))
            }
            _ => None,
        }
    }

    /// One-time setup: borderless non-activating panel, rounded layer mask, floating, shadow.
    pub(crate) fn setup_bar(self, radius: f64) {
        let w = self.0;
        unsafe {
            let _: () = msg_send![w, setStyleMask: NONACTIVATING_PANEL];
            let _: () = msg_send![w, setHasShadow: Bool::YES];
            let _: () = msg_send![w, setLevel: 3isize];
            let _: () = msg_send![w, setCollectionBehavior: COLLECTION];
            let _: () = msg_send![w, setAnimationBehavior: 2isize];
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

    pub(crate) fn set_alpha(self, a: f64) {
        unsafe {
            let _: () = msg_send![self.0, setAlphaValue: a];
        }
    }

    pub(crate) fn set_ignores_mouse(self, v: bool) {
        unsafe {
            let _: () = msg_send![self.0, setIgnoresMouseEvents: Bool::new(v)];
        }
    }

    pub(crate) fn show_key(self) {
        unsafe {
            let _: () = msg_send![self.0, makeKeyAndOrderFront: std::ptr::null_mut::<AnyObject>()];
        }
    }

    pub(crate) fn order_front_regardless(self) {
        unsafe {
            let _: () = msg_send![self.0, orderFrontRegardless];
        }
    }

    pub(crate) fn order_out(self) {
        unsafe {
            let _: () = msg_send![self.0, orderOut: std::ptr::null_mut::<AnyObject>()];
        }
    }

    pub(crate) fn is_key(self) -> bool {
        unsafe {
            let b: Bool = msg_send![self.0, isKeyWindow];
            b.as_bool()
        }
    }

    pub(crate) fn is_visible(self) -> bool {
        unsafe {
            let b: Bool = msg_send![self.0, isVisible];
            b.as_bool()
        }
    }

    pub(crate) fn frame(self) -> Rect {
        unsafe { msg_send![self.0, frame] }
    }

    pub(crate) fn alpha(self) -> f64 {
        unsafe { msg_send![self.0, alphaValue] }
    }

    pub(crate) fn invalidate_shadow(self) {
        unsafe {
            let _: () = msg_send![self.0, invalidateShadow];
        }
    }

    /// Move the frame origin (AppKit coordinates, bottom-left based). Never orders the window front.
    pub(crate) fn set_origin(self, x: f64, y: f64) {
        unsafe {
            let _: () = msg_send![self.0, setFrameOrigin: Pt { x, y }];
        }
    }

    /// Scale about the centre plus a vertical offset (points, positive is up) on the content layer.
    pub(crate) fn set_motion(self, scale: f64, dy: f64) {
        unsafe {
            let cv: Obj = msg_send![self.0, contentView];
            let layer: Obj = msg_send![cv, layer];
            if layer.is_null() {
                return;
            }
            let bounds: Rect = msg_send![layer, bounds];
            let anchor: Pt = msg_send![layer, anchorPoint];
            let (cx, cy) = (bounds.s.w / 2.0, bounds.s.h / 2.0);
            let (ax, ay) = (anchor.x * bounds.s.w, anchor.y * bounds.s.h);
            let t = Affine {
                a: scale,
                b: 0.0,
                c: 0.0,
                d: scale,
                tx: (cx - ax) * (1.0 - scale),
                ty: (cy - ay) * (1.0 - scale) + dy,
            };
            let txn = class!(CATransaction);
            let _: () = msg_send![txn, begin];
            let _: () = msg_send![txn, setDisableActions: Bool::YES];
            let _: () = msg_send![layer, setAffineTransform: t];
            let _: () = msg_send![txn, commit];
        }
    }

    /// Centre horizontally with the top edge at `top_frac` of the screen height, on the screen
    /// under the mouse.
    pub(crate) fn position(self, width: f64, height: f64, top_frac: f64) {
        unsafe {
            let mouse: Pt = msg_send![class!(NSEvent), mouseLocation];
            let screens: Obj = msg_send![class!(NSScreen), screens];
            let n: usize = msg_send![screens, count];
            let mut chosen: Option<Rect> = None;
            for i in 0..n {
                let s: Obj = msg_send![screens, objectAtIndex: i];
                let f: Rect = msg_send![s, frame];
                if mouse.x >= f.o.x
                    && mouse.x < f.o.x + f.s.w
                    && mouse.y >= f.o.y
                    && mouse.y < f.o.y + f.s.h
                {
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
            let _: () = msg_send![self.0, setFrameOrigin: origin];
        }
    }

    /// The GPUI-owned NSView (the one that implements NSTextInputClient and accessibility).
    pub(crate) fn content_view(self) -> Obj {
        unsafe { msg_send![self.0, contentView] }
    }

    /// Wake the accessibility adapter the way an assistive technology does: ask the content view
    /// for its accessibility children. GPUI then builds its tree on the next frame.
    pub(crate) fn wake_a11y(self) {
        unsafe {
            let cv = self.content_view();
            let _: Obj = msg_send![cv, accessibilityChildren];
        }
    }

    pub(crate) fn blur_view_count(self) -> usize {
        unsafe {
            let cv: Obj = msg_send![self.0, contentView];
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
}

/// System Reduce Motion setting.
pub(crate) fn reduce_motion() -> bool {
    if std::env::var("PANOPLY_REDUCE_MOTION").is_ok_and(|v| v == "1") {
        return true;
    }
    unsafe {
        let ws: Obj = msg_send![class!(NSWorkspace), sharedWorkspace];
        let b: Bool = msg_send![ws, accessibilityDisplayShouldReduceMotion];
        b.as_bool()
    }
}

/// System Increase Contrast setting.
pub(crate) fn increase_contrast() -> bool {
    if std::env::var("PANOPLY_HIGH_CONTRAST").is_ok_and(|v| v == "1") {
        return true;
    }
    unsafe {
        let ws: Obj = msg_send![class!(NSWorkspace), sharedWorkspace];
        let b: Bool = msg_send![ws, accessibilityDisplayShouldIncreaseContrast];
        b.as_bool()
    }
}

/// System accent colour as sRGB 0..1, falling back to system blue.
pub(crate) fn accent_rgb() -> (f32, f32, f32) {
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

/// Move a file to the Trash (so Undo is itself undoable from Finder).
pub(crate) fn os_trash(path: &Path) -> std::io::Result<()> {
    let s = path
        .to_str()
        .and_then(|s| CString::new(s).ok())
        .ok_or_else(|| std::io::Error::other("path is not valid text"))?;
    unsafe {
        let ns: Obj = msg_send![class!(NSString), stringWithUTF8String: s.as_ptr()];
        let url: Obj = msg_send![class!(NSURL), fileURLWithPath: ns];
        let fm: Obj = msg_send![class!(NSFileManager), defaultManager];
        let mut err: Obj = std::ptr::null_mut();
        let ok: Bool = msg_send![
            fm,
            trashItemAtURL: url,
            resultingItemURL: std::ptr::null_mut::<Obj>(),
            error: &mut err as *mut Obj
        ];
        if ok.as_bool() {
            Ok(())
        } else {
            Err(std::io::Error::other("the Trash would not take the file"))
        }
    }
}

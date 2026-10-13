//! Native macOS glue: real translucency, window behaviour, instant show/hide.
//! Everything here must run on the main thread.

use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2::{MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSRunningApplication, NSWorkspace, NSAutoresizingMaskOptions, NSEvent, NSGlassEffectView, NSScreen, NSView, NSWindow,
    NSWindowCollectionBehavior, NSWindowOrderingMode,
};
use objc2_foundation::{NSObjectProtocol, NSPoint, NSRect};
use std::cell::RefCell;
use std::ffi::c_void;

pub const WIN_W: f64 = 720.0;
pub const RADIUS: f64 = 20.0;

thread_local! {
    /// The app that was frontmost before we took the keyboard (main thread only).
    static PREV: RefCell<Option<Retained<NSRunningApplication>>> = const { RefCell::new(None) };
}

unsafe fn win<'a>(ptr: *mut c_void) -> &'a NSWindow {
    &*(ptr as *const NSWindow)
}

/// Window behaviour: present on the active Space, floats, allowed over full screen apps.
pub fn configure_window(ptr: *mut c_void) {
    let w = unsafe { win(ptr) };
    // The window stays ordered in (a hair above transparent) while "hidden" so WebKit keeps the
    // page live and the first frame after a show needs no occlusion round trip. Ordering it
    // front again pulls it onto the active Space. (CanJoinAllSpaces was tried first, but
    // WindowServer reports such a window as occluded forever and WebKit never presents.)
    w.setCollectionBehavior(
        NSWindowCollectionBehavior::MoveToActiveSpace
            | NSWindowCollectionBehavior::FullScreenAuxiliary
            | NSWindowCollectionBehavior::IgnoresCycle,
    );
}

/// Tries macOS 26 Liquid Glass (NSGlassEffectView) behind the transparent webview.
/// Returns false when the class is missing so the caller can fall back to vibrancy.
pub fn install_glass(ptr: *mut c_void) -> bool {
    let Some(mtm) = MainThreadMarker::new() else { return false };
    if AnyClass::get(c"NSGlassEffectView").is_none() {
        return false;
    }
    let w = unsafe { win(ptr) };
    let Some(content) = w.contentView() else { return false };
    let glass: Retained<NSGlassEffectView> =
        NSGlassEffectView::initWithFrame(NSGlassEffectView::alloc(mtm), content.bounds());
    glass.setCornerRadius(RADIUS);
    glass.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    content.addSubview_positioned_relativeTo(&glass, NSWindowOrderingMode::Below, None);
    w.invalidateShadow();
    true
}

/// Centered horizontally, top edge at 22% of the height of the screen under the pointer.
pub fn place(ptr: *mut c_void) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let w = unsafe { win(ptr) };
    let m = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let mut frame: Option<NSRect> = None;
    for s in screens.iter() {
        let f = s.frame();
        if m.x >= f.origin.x
            && m.x < f.origin.x + f.size.width
            && m.y >= f.origin.y
            && m.y < f.origin.y + f.size.height
        {
            frame = Some(f);
            break;
        }
    }
    let f = match frame.or_else(|| screens.iter().next().map(|s| s.frame())) {
        Some(f) => f,
        None => return,
    };
    let x = f.origin.x + ((f.size.width - WIN_W) / 2.0).round();
    let top = f.origin.y + f.size.height - (f.size.height * 0.22).round();
    w.setFrameTopLeftPoint(NSPoint::new(x, top));
}

/// The material view (glass, or the vibrancy fallback) sitting behind the webview.
fn material<'a>(w: &'a NSWindow) -> Option<Retained<NSView>> {
    let content = w.contentView()?;
    for v in content.subviews().iter() {
        let n = v.class().name().to_str().unwrap_or("").to_string();
        if n.contains("NSGlassEffectView") || n.contains("NSVisualEffectView") {
            return Some(v);
        }
    }
    None
}

/// Fade the material with a native Core Animation transaction (same clock as the compositor).
/// `c` is the cubic-bezier control points of the timing function.
///
/// Why the material and not the window: lowering the window alpha below about 0.5 while the
/// app is active makes WindowServer report the window occluded. WebKit then stops presenting
/// frames, and the next show pays ~240 ms before the page can paint. With the window pinned at
/// alpha 1 and only the material faded, the page stays live and a show is one frame of work.
fn fade_material(w: &NSWindow, to: f64, secs: f64, c: [f32; 4]) {
    use objc2::runtime::{AnyObject, MessageReceiver};
    use objc2::{class, msg_send};
    let Some(m) = material(w) else { return };
    unsafe {
        let _: () = msg_send![class!(NSAnimationContext), beginGrouping];
        let ctx: Retained<AnyObject> = msg_send![class!(NSAnimationContext), currentContext];
        let _: () = msg_send![&*ctx, setDuration: secs];
        let cls = class!(CAMediaTimingFunction);
        let tf: *mut AnyObject =
            cls.send_message(objc2::sel!(functionWithControlPoints::::), (c[0], c[1], c[2], c[3]));
        if !tf.is_null() {
            let _: () = msg_send![&*ctx, setTimingFunction: tf];
        }
        let anim: Retained<NSView> = msg_send![&*m, animator];
        anim.setAlphaValue(to);
        let _: () = msg_send![class!(NSAnimationContext), endGrouping];
    }
}

/// Bring the (already ordered-in, empty) window up and give it the keyboard.
#[allow(deprecated)]
pub fn show_and_focus(ptr: *mut c_void) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let w = unsafe { win(ptr) };
    w.setIgnoresMouseEvents(false);
    if let Some(m) = material(w) {
        m.setAlphaValue(0.0);
        m.setHidden(false);
    }
    fade_material(w, 1.0, 0.18, [0.2, 0.9, 0.25, 1.0]);
    w.invalidateShadow();
    let app = NSApplication::sharedApplication(mtm);
    if !app.isActive() {
        if let Some(front) = NSWorkspace::sharedWorkspace().frontmostApplication() {
            if front.processIdentifier() != std::process::id() as i32 {
                PREV.with(|p| *p.borrow_mut() = Some(front));
            }
        }
    }
    app.activateIgnoringOtherApps(true);
    w.makeKeyAndOrderFront(None);
    focus_webview(w);
}

/// Start the native fade out (120 ms ease-in) and stop taking mouse events.
pub fn begin_hide(ptr: *mut c_void) {
    let w = unsafe { win(ptr) };
    w.setIgnoresMouseEvents(true);
    fade_material(w, 0.0, 0.12, [0.42, 0.0, 1.0, 1.0]);
}

/// Order the window in with nothing visible in it: transparent webview, material hidden,
/// click-through. WindowServer counts it as on screen, so WebKit keeps presenting frames.
pub fn park(ptr: *mut c_void) {
    let w = unsafe { win(ptr) };
    w.setAlphaValue(1.0);
    w.setIgnoresMouseEvents(true);
    if let Some(m) = material(w) {
        m.setAlphaValue(0.0);
        m.setHidden(true);
    }
    w.invalidateShadow();
    w.orderFront(None);
}

/// Finish a hide: hide the material and, if we are still the active app, hand activation back
/// to whatever was frontmost before so typing continues where the user left off. (`hide:`
/// would make the next activation pay a ~200 ms unhide; `deactivate` does not.)
pub fn finish_hide(ptr: *mut c_void) {
    let w = unsafe { win(ptr) };
    w.setIgnoresMouseEvents(true);
    if let Some(m) = material(w) {
        m.setAlphaValue(0.0);
        m.setHidden(true);
    }
    w.invalidateShadow();
    if let Some(mtm) = MainThreadMarker::new() {
        let app = NSApplication::sharedApplication(mtm);
        if app.isActive() {
            // Give the keyboard back explicitly; plain `deactivate` is not reliable for an
            // accessory app whose window is still ordered in.
            match PREV.with(|p| p.borrow_mut().take()) {
                Some(prev) => {
                    prev.activateWithOptions(NSApplicationActivationOptions::empty());
                }
                None => app.deactivate(),
            }
        }
    }
}

fn is_web_view(v: &NSView) -> bool {
    AnyClass::get(c"WKWebView").map(|c| v.isKindOfClass(c)).unwrap_or(false)
}

/// Make the WKWebView the first responder so keystrokes reach the page the moment we show.
fn focus_webview(w: &NSWindow) {
    if let Some(content) = w.contentView() {
        for v in content.subviews().iter() {
            if is_web_view(&v) {
                w.makeFirstResponder(Some(&v));
                break;
            }
        }
    }
}

/// Recompute the system shadow from what is currently painted (called while the glass fades
/// in and once it has settled, so the shadow follows the glass instead of lagging it).
pub fn invalidate_shadow(ptr: *mut c_void) {
    unsafe { win(ptr) }.invalidateShadow();
}

pub fn is_key(ptr: *mut c_void) -> bool {
    unsafe { win(ptr) }.isKeyWindow()
}

pub fn frame_json(ptr: *mut c_void) -> String {
    let f = unsafe { win(ptr) }.frame();
    format!(
        "{{\"x\":{},\"y\":{},\"w\":{},\"h\":{}}}",
        f.origin.x, f.origin.y, f.size.width, f.size.height
    )
}


pub fn probe(ptr: *mut c_void) -> String {
    let w = unsafe { win(ptr) };
    let (active, hidden) = MainThreadMarker::new()
        .map(|m| {
            let a = NSApplication::sharedApplication(m);
            (a.isActive(), a.isHidden())
        })
        .unwrap_or((false, false));
    let mat_op = material(w)
        .and_then(|m| unsafe {
            use objc2::runtime::AnyObject;
            let layer: Option<Retained<AnyObject>> = objc2::msg_send![&*m, layer];
            let pl: Option<Retained<AnyObject>> = objc2::msg_send![&*layer?, presentationLayer];
            let o: f32 = objc2::msg_send![&*pl?, opacity];
            Some(o)
        })
        .unwrap_or(-1.0);
    format!(
        "{{\"matOpacity\":{:.3},\"appActive\":{},\"appHidden\":{},\"level\":{},\"onScreen\":{},\"isVisible\":{},\"occluded\":{},\"alpha\":{}}}",
        mat_op,
        active,
        hidden,
        w.level(),
        w.screen().is_some(),
        w.isVisible(),
        !w.occlusionState().contains(objc2_app_kit::NSWindowOcclusionState::Visible),
        w.alphaValue()
    )
}

/// WebKit throttles page rendering updates (requestAnimationFrame, CSS animation ticks) to
/// about 60 fps by default, even on ProMotion. Turn that preference off so the page can run
/// at the display rate (120 Hz). Needs the WKWebView to exist; call from setup.
pub fn unlock_display_rate(ptr: *mut c_void) -> bool {
    use objc2::runtime::{AnyObject, Bool};
    use objc2_foundation::{NSArray, NSString};
    let w = unsafe { win(ptr) };
    let Some(cls) = AnyClass::get(c"WKPreferences") else { return false };
    let Some(content) = w.contentView() else { return false };
    let mut done = false;
    for v in content.subviews().iter() {
        if !is_web_view(&v) { continue; }
        let obj: &AnyObject = &v;
        // Do not let WindowServer occlusion decide whether the page is visible: only whether
        // the window is on screen. Occlusion updates lag ~240 ms behind an order-in, and an
        // all-transparent parked window is reported occluded, which would freeze rAF.
        unsafe { let _: () = objc2::msg_send![obj, _setWindowOcclusionDetectionEnabled: Bool::NO]; }
        let cfg: Option<Retained<AnyObject>> = unsafe { objc2::msg_send![obj, configuration] };
        let Some(cfg) = cfg else { continue };
        let prefs: Option<Retained<AnyObject>> = unsafe { objc2::msg_send![&*cfg, preferences] };
        let Some(prefs) = prefs else { continue };
        let arr: Option<Retained<NSArray<AnyObject>>> = unsafe { objc2::msg_send![cls, _experimentalFeatures] };
        let Some(arr) = arr else { continue };
        for f in arr.iter() {
            let key: Option<Retained<NSString>> = unsafe { objc2::msg_send![&*f, key] };
            if key.map(|k| k.to_string() == "PreferPageRenderingUpdatesNear60FPSEnabled").unwrap_or(false) {
                unsafe { let _: () = objc2::msg_send![&*prefs, _setEnabled: Bool::NO, forExperimentalFeature: &*f]; }
                done = true;
            }
        }
    }
    done
}

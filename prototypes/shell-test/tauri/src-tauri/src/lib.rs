//! Arsenal command bar, Tauri 2 build.
//!
//! Rust owns: the pre-created hidden window, native translucency, the global hotkey,
//! the control server on 127.0.0.1:47812 and the show_ms bookkeeping. The webview owns
//! every pixel and reports presented-frame timestamps back through `invoke`.

mod mac;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, DragDropEvent, Manager, State, WebviewWindow, WindowEvent};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

const PORT: u16 = 47812;

/// Set ARSENAL_DEBUG=1 to trace focus and show/hide decisions on stderr.
macro_rules! dlog {
    ($($a:tt)*) => {
        if *DEBUG.get_or_init(|| std::env::var_os("ARSENAL_DEBUG").is_some()) {
            eprintln!("[{:.1}] {}", now_ms() % 100000.0, format!($($a)*));
        }
    };
}
static DEBUG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

#[derive(Default)]
struct Shared {
    /// Logical visibility: true from the moment a show is requested until a hide is requested.
    visible: AtomicBool,
    /// Set while /selftest runs so focus changes cannot hide the bar mid-measurement.
    selftest: AtomicBool,
    /// Waiting for the first presented frame of the current show (t0, wall ms).
    pending_t0: Mutex<Option<f64>>,
    show_ms: Mutex<Vec<f64>>,
    selftest_tx: Mutex<Option<Sender<String>>>,
    /// NSWindow pointer as usize (only dereferenced on the main thread).
    ns_window: Mutex<usize>,
    material: Mutex<&'static str>,
}

fn now_ms() -> f64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    d.as_secs() as f64 * 1000.0 + d.subsec_nanos() as f64 / 1_000_000.0
}

fn results_dir() -> PathBuf {
    if let Ok(p) = std::env::var("ARSENAL_RESULTS_DIR") {
        return PathBuf::from(p);
    }
    // Resolved at build time: <shell-test>/results (this crate lives in <shell-test>/tauri/src-tauri).
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../results")
}

fn nsw(shared: &Shared) -> *mut std::ffi::c_void {
    *shared.ns_window.lock().unwrap() as *mut std::ffi::c_void
}

fn js(window: &WebviewWindow, code: &str) {
    let _ = window.eval(code);
}

/// The single show/hide entry point, shared by the hotkey and GET /toggle.
/// `t0` is the wall clock taken the instant the trigger arrived.
fn toggle(app: &AppHandle, t0: f64) {
    let app2 = app.clone();
    let _ = app.run_on_main_thread(move || {
        let shared = app2.state::<Shared>();
        let Some(window) = app2.get_webview_window("main") else { return };
        if shared.visible.load(Ordering::SeqCst) {
            dlog!("toggle -> hide");
            hide(&shared, &window);
        } else {
            show(&app2, &shared, &window, Some(t0));
        }
    });
}

/// Start a hide: native fade and the page's own hide animation run side by side.
fn hide(shared: &Shared, window: &WebviewWindow) {
    dlog!("hide pre={}", mac::probe(nsw(shared)));
    shared.visible.store(false, Ordering::SeqCst);
    mac::begin_hide(nsw(shared));
    js(window, "window.__arsenal.hide()");
}

fn show(app: &AppHandle, shared: &Shared, window: &WebviewWindow, t0: Option<f64>) {
    dlog!("show t0={:?} pre={}", t0, mac::probe(nsw(shared)));
    shared.visible.store(true, Ordering::SeqCst);
    *shared.pending_t0.lock().unwrap() = t0;
    let ptr = nsw(shared);
    // Kick the webview first: it prepares state while the main thread orders the window in.
    js(window, "window.__arsenal.show()");
    // The shadow is derived from painted pixels: refresh it as the glass fades in and once settled.
    let app = app.clone();
    thread::spawn(move || {
        for ms in [70u64, 130] {
            thread::sleep(Duration::from_millis(ms));
            let a2 = app.clone();
            let _ = app.run_on_main_thread(move || {
                let sh = a2.state::<Shared>();
                if sh.visible.load(Ordering::SeqCst) {
                    mac::invalidate_shadow(nsw(&sh));
                }
            });
        }
    });
    let a = now_ms();
    mac::place(ptr);
    let b = now_ms();
    mac::show_and_focus(ptr);
    dlog!("show: since t0 {:.2?} ms, place {:.2} ms, show_and_focus {:.2} ms", t0.map(|t| a - t), b - a, now_ms() - b);
}

// ---- commands invoked by the webview ------------------------------------------------

/// First presented frame after a show: the webview's t1 (timeOrigin + now at the inner rAF).
#[tauri::command]
fn shown(shared: State<'_, Shared>, t1: f64) {
    dlog!("shown: t1 - now = {:.2} ms", t1 - now_ms());
    if let Some(t0) = shared.pending_t0.lock().unwrap().take() {
        shared.show_ms.lock().unwrap().push(((t1 - t0) * 1000.0).round() / 1000.0);
    }
}

/// The webview started hiding itself (Escape, or any in-page trigger).
#[tauri::command]
fn begin_hide(app: AppHandle, shared: State<'_, Shared>) {
    shared.visible.store(false, Ordering::SeqCst);
    let ptr = *shared.ns_window.lock().unwrap();
    let _ = app.run_on_main_thread(move || mac::begin_hide(ptr as *mut _));
}

/// Hide animation finished: take the window off screen unless a show raced in.
#[tauri::command]
fn finish_hide(app: AppHandle, shared: State<'_, Shared>) {
    dlog!("finish_hide visible={} pre={}", shared.visible.load(Ordering::SeqCst), mac::probe(*shared.ns_window.lock().unwrap() as *mut _));
    if shared.visible.load(Ordering::SeqCst) {
        return;
    }
    let ptr = *shared.ns_window.lock().unwrap();
    let _ = app.run_on_main_thread(move || mac::finish_hide(ptr as *mut _));
}

/// First presented frame after launch (the transparent warm-up). Logs the wall clock; the
/// window stays ordered in at alpha 0, ready for an instant show.
#[tauri::command]
fn launched(t: f64) {
    dlog!("launched t={t}");
    let dir = results_dir();
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join("tauri-launch.txt")) {
        let _ = writeln!(f, "{:.3}", t);
    }
}

#[tauri::command]
fn selftest_done(shared: State<'_, Shared>, json: String) {
    if let Some(tx) = shared.selftest_tx.lock().unwrap().take() {
        let _ = tx.send(json);
    }
}

// ---- control server -----------------------------------------------------------------

fn serve(app: AppHandle) {
    let server = match tiny_http::Server::http(("127.0.0.1", PORT)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("control server failed to bind {PORT}: {e}");
            return;
        }
    };
    for req in server.incoming_requests() {
        // Stamp the arrival before anything else.
        let t0 = now_ms();
        let app = app.clone();
        thread::spawn(move || handle(app, req, t0));
    }
}

fn respond(req: tiny_http::Request, body: String) {
    let h = tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
    let _ = req.respond(tiny_http::Response::from_string(body).with_header(h));
}

fn handle(app: AppHandle, req: tiny_http::Request, t0: f64) {
    let path = req.url().split('?').next().unwrap_or("").to_string();
    let shared = app.state::<Shared>();
    match path.as_str() {
        "/toggle" => {
            let will_show = !shared.visible.load(Ordering::SeqCst);
            toggle(&app, t0);
            respond(req, format!("{{\"ok\":true,\"visible\":{}}}", will_show));
        }
        "/metrics" => {
            let v = shared.show_ms.lock().unwrap();
            let items: Vec<String> = v.iter().map(|x| format!("{}", x)).collect();
            respond(req, format!("{{\"show_ms\":[{}]}}", items.join(",")));
        }
        "/state" => {
            let ptr = *shared.ns_window.lock().unwrap();
            let (tx, rx) = channel();
            let _ = app.run_on_main_thread(move || {
                let p = ptr as *mut _;
                let _ = tx.send((mac::is_key(p), format!("{},\"probe\":{}", mac::frame_json(p).trim_end_matches('}'), mac::probe(p)) + "}"));
            });
            let (key, frame) = rx.recv_timeout(Duration::from_secs(2)).unwrap_or((false, "null".into()));
            respond(
                req,
                format!(
                    "{{\"visible\":{},\"key\":{},\"frame\":{},\"material\":\"{}\"}}",
                    shared.visible.load(Ordering::SeqCst),
                    key,
                    frame,
                    *shared.material.lock().unwrap()
                ),
            );
        }
        "/selftest" => {
            let (tx, rx) = channel::<String>();
            *shared.selftest_tx.lock().unwrap() = Some(tx);
            shared.selftest.store(true, Ordering::SeqCst);
            let app2 = app.clone();
            let _ = app.run_on_main_thread(move || {
                let shared = app2.state::<Shared>();
                let Some(window) = app2.get_webview_window("main") else { return };
                if !shared.visible.load(Ordering::SeqCst) {
                    show(&app2, &shared, &window, None);
                }
                js(&window, "window.__arsenal.selftest()");
            });
            let out = rx
                .recv_timeout(Duration::from_secs(120))
                .unwrap_or_else(|_| "{\"error\":\"selftest timed out\"}".to_string());
            shared.selftest.store(false, Ordering::SeqCst);
            respond(req, out);
        }
        _ => {
            let _ = req.respond(tiny_http::Response::from_string("not found").with_status_code(404));
        }
    }
}

// ---- app ------------------------------------------------------------------------------

pub fn run() {
    let hotkey = Shortcut::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Digit2);

    tauri::Builder::default()
        .manage(Shared::default())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(move |app, sc, ev| {
                    if sc == &hotkey && ev.state() == ShortcutState::Pressed {
                        toggle(app, now_ms());
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![shown, begin_hide, finish_hide, launched, selftest_done])
        .setup(move |app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let window = app.get_webview_window("main").expect("main window");
            let shared = app.state::<Shared>();
            let ptr = window.ns_window()?;
            *shared.ns_window.lock().unwrap() = ptr as usize;

            mac::configure_window(ptr);
            let rate = mac::unlock_display_rate(ptr);
            dlog!("unlock_display_rate: {rate}");
            let want = std::env::var("ARSENAL_MATERIAL").unwrap_or_default();
            let mut material = "none";
            if want != "vibrancy" && mac::install_glass(ptr) {
                material = "NSGlassEffectView";
            } else {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
                if apply_vibrancy(
                    &window,
                    NSVisualEffectMaterial::HudWindow,
                    Some(NSVisualEffectState::Active),
                    Some(mac::RADIUS),
                )
                .is_ok()
                {
                    material = "NSVisualEffectView.hudWindow";
                }
            }
            *shared.material.lock().unwrap() = material;

            // Invisible warm-up: order the window in at alpha 0 so the webview renders one real
            // frame (this is the "first presented frame" for the launch log) and the compositor
            // layers exist before the first hotkey press. `launched` orders it back out.
            let h2 = app.handle().clone();
            thread::spawn(move || {
                // Let the run loop finish launching first: a window ordered in during
                // `applicationDidFinishLaunching` stays occluded and never presents a frame.
                thread::sleep(Duration::from_millis(60));
                let h3 = h2.clone();
                let _ = h2.run_on_main_thread(move || {
                    let shared = h3.state::<Shared>();
                    if shared.visible.load(Ordering::SeqCst) {
                        return;
                    }
                    // Parked: ordered in, material hidden, page transparent, click-through.
                    // Nothing is drawn, but the webview presents a real frame (the launch
                    // timestamp) and stays live, so showing later never waits on WindowServer.
                    let ptr = nsw(&shared);
                    mac::place(ptr);
                    mac::park(ptr);
                });
            });

            // Hide when the bar loses focus.
            let handle = app.handle().clone();
            window.on_window_event(move |ev| match ev {
                WindowEvent::Focused(f) if *f => dlog!("focus gained"),
                WindowEvent::Focused(false) => {
                    dlog!("focus lost, visible={}", handle.state::<Shared>().visible.load(Ordering::SeqCst));
                    let shared = handle.state::<Shared>();
                    if shared.visible.load(Ordering::SeqCst) && !shared.selftest.load(Ordering::SeqCst) {
                        if let Some(w) = handle.get_webview_window("main") {
                            hide(&shared, &w);
                        }
                    }
                }
                WindowEvent::DragDrop(d) => {
                    if let Some(w) = handle.get_webview_window("main") {
                        let (kind, paths) = match d {
                            DragDropEvent::Enter { paths, .. } => ("enter", Some(paths)),
                            DragDropEvent::Over { .. } => ("over", None),
                            DragDropEvent::Drop { paths, .. } => ("drop", Some(paths)),
                            DragDropEvent::Leave => ("leave", None),
                            _ => return,
                        };
                        if kind == "over" {
                            return;
                        }
                        let list: Vec<String> =
                            paths.map(|p| p.iter().map(|x| x.to_string_lossy().into_owned()).collect()).unwrap_or_default();
                        let arg = serde_json::to_string(&list).unwrap_or_else(|_| "[]".into());
                        js(&w, &format!("window.__arsenal.drag('{kind}',{arg})"));
                    }
                }
                _ => {}
            });

            if let Err(e) = app.global_shortcut().register(hotkey) {
                // Another app owns the chord: keep running, the control port still works.
                eprintln!("could not register Control+Option+2: {e}");
            }

            let h = app.handle().clone();
            thread::spawn(move || serve(h));
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Arsenal");
}

mod fuzzy;
mod icons_gen;
mod model;
mod native;
mod view;

use futures::channel::{mpsc, oneshot};
use futures::{FutureExt, StreamExt};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use gpui::*;
use model::*;
use native::now_ms;
use std::borrow::Cow;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use view::{Bar, Phase};

const PORT: u16 = 47814;

struct Icons;
impl AssetSource for Icons {
    fn load(&self, path: &str) -> anyhow::Result<Option<Cow<'static, [u8]>>> {
        let name = path.strip_prefix("icons/").and_then(|p| p.strip_suffix(".svg"));
        Ok(name.and_then(|n| icons_gen::ICONS.iter().find(|(k, _)| *k == n)).map(|(_, s)| Cow::Borrowed(s.as_bytes())))
    }
    fn list(&self, _path: &str) -> anyhow::Result<Vec<SharedString>> {
        Ok(icons_gen::ICONS.iter().map(|(k, _)| SharedString::from(format!("icons/{k}.svg"))).collect())
    }
}

enum Msg {
    Toggle(f64),
    SelfTest(std::sync::mpsc::Sender<String>),
    State(std::sync::mpsc::Sender<String>),
    Snapshot { query: String, sel: usize, file: Option<String>, path: String, reply: std::sync::mpsc::Sender<String> },
}

fn spec_folder() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        let mut d = exe.canonicalize().unwrap_or(exe);
        for _ in 0..8 {
            if !d.pop() {
                break;
            }
            if d.join("SPEC.md").exists() {
                return d;
            }
        }
    }
    PathBuf::from("/Users/anzalhussainabidi/personal/panoply/prototypes/shell-test")
}

pub fn record_launch(t: f64) {
    let dir = spec_folder().join("results");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("gpui-kit-launch.txt")) {
        let _ = writeln!(f, "{t:.3}");
    }
}

fn start_server(tx: mpsc::UnboundedSender<Msg>, metrics: Arc<Mutex<Vec<f64>>>) {
    std::thread::spawn(move || {
        let server = match tiny_http::Server::http(("127.0.0.1", PORT)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("control server failed: {e}");
                return;
            }
        };
        let json = || tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap();
        for req in server.incoming_requests() {
            // t0: first instant the request is visible to user space.
            let t0 = now_ms();
            let path = req.url().split('?').next().unwrap_or("").to_string();
            match path.as_str() {
                "/toggle" => {
                    let _ = tx.unbounded_send(Msg::Toggle(t0));
                    let _ = req.respond(tiny_http::Response::from_string("{\"ok\":true}").with_header(json()));
                }
                "/metrics" => {
                    let a = metrics.lock().unwrap().clone();
                    let body = format!("{{\"show_ms\":[{}]}}", a.iter().map(|v| format!("{v:.3}")).collect::<Vec<_>>().join(","));
                    let _ = req.respond(tiny_http::Response::from_string(body).with_header(json()));
                }
                "/snapshot" => {
                    // dev only: /snapshot?q=compress&sel=3&file=report.pdf&path=/tmp/x.png
                    let url = req.url().to_string();
                    let qs = url.split_once('?').map(|x| x.1).unwrap_or("");
                    let get = |k: &str| qs.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}="))).map(|v| v.replace("%20", " ").replace('+', " "));
                    let msg_q = get("q").unwrap_or_default();
                    let sel = get("sel").and_then(|v| v.parse().ok()).unwrap_or(0);
                    let file = get("file");
                    let out = get("path").unwrap_or_else(|| "/tmp/arsenal-gpui.png".into());
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let (rtx, rrx) = std::sync::mpsc::channel();
                        let _ = tx.unbounded_send(Msg::Snapshot { query: msg_q, sel, file, path: out, reply: rtx });
                        let body = rrx.recv_timeout(Duration::from_secs(20)).unwrap_or_else(|_| "{\"error\":\"timeout\"}".into());
                        let _ = req.respond(tiny_http::Response::from_string(body));
                    });
                }
                "/selftest" | "/state" => {
                    let tx = tx.clone();
                    let is_state = path == "/state";
                    std::thread::spawn(move || {
                        let (rtx, rrx) = std::sync::mpsc::channel();
                        let _ = tx.unbounded_send(if is_state { Msg::State(rtx) } else { Msg::SelfTest(rtx) });
                        let body = rrx.recv_timeout(Duration::from_secs(90)).unwrap_or_else(|_| "{\"error\":\"timeout\"}".into());
                        let _ = req.respond(tiny_http::Response::from_string(body).with_header(json()));
                    });
                }
                _ => {
                    let _ = req.respond(tiny_http::Response::from_string("").with_status_code(404));
                }
            }
        }
    });
}

fn selftest_steps() -> Vec<String> {
    let mut steps = vec![];
    for phrase in ["compress pdf", "heic to jpg", "trim video"] {
        let mut cur = String::new();
        for ch in phrase.chars() {
            cur.push(ch);
            steps.push(cur.clone());
        }
        while !cur.is_empty() {
            cur.pop();
            steps.push(cur.clone());
        }
    }
    steps
}

async fn run_selftest(handle: WindowHandle<Bar>, cx: &mut AsyncApp) -> String {
    let ok = handle.update(cx, |bar, window, cx| {
        bar.suppress_autohide = true;
        if matches!(bar.phase, Phase::Hidden | Phase::Hiding) {
            bar.show(None, window, cx);
        }
    });
    if ok.is_err() {
        return "{\"error\":\"window\"}".into();
    }
    // wait until the show animation has finished
    for _ in 0..200 {
        cx.background_executor().timer(Duration::from_millis(8)).await;
        if handle.update(cx, |bar, _, _| bar.phase == Phase::Shown).unwrap_or(false) {
            break;
        }
    }
    cx.background_executor().timer(Duration::from_millis(200)).await;
    let _ = handle.update(cx, |bar, window, cx| bar.set_query("", window, cx));
    cx.background_executor().timer(Duration::from_millis(100)).await;

    let mut seed = (now_ms() as u64) | 1;
    let mut rnd = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        (seed % 1000) as f64 / 1000.0
    };
    let mut samples: Vec<f64> = vec![];
    for q in selftest_steps() {
        let gap = 12.0 + rnd() * 18.0;
        cx.background_executor().timer(Duration::from_micros((gap * 1000.0) as u64)).await;
        let (tx, rx) = oneshot::channel::<f64>();
        let start = now_ms();
        let _ = handle.update(cx, |bar, window, cx| {
            bar.pending_mark = Some(tx);
            bar.set_query(&q, window, cx);
        });
        let mut rx = rx.fuse();
        let mut timeout = cx.background_executor().timer(Duration::from_secs(2)).fuse();
        futures::select_biased! {
            end = rx => { if let Ok(end) = end { samples.push(end - start); } }
            _ = timeout => {}
        }
    }
    let _ = handle.update(cx, |bar, _, _| bar.suppress_autohide = false);
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    if samples.is_empty() {
        return "{\"error\":\"no samples\"}".into();
    }
    let n = samples.len();
    let pct = |p: f64| samples[(((p / 100.0 * n as f64).ceil() as usize).max(1)) - 1];
    format!("{{\"n\":{},\"p50\":{:.3},\"p95\":{:.3},\"max\":{:.3}}}", n, pct(50.0), pct(95.0), samples[n - 1])
}

fn main() {
    let model = Rc::new(Model::load());
    let metrics: Arc<Mutex<Vec<f64>>> = Arc::new(Mutex::new(vec![]));
    let app = gpui_platform::application().with_assets(Icons);

    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);

        let opts = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds {
                origin: point(px(200.), px(200.)),
                size: size(px(WIDTH), px(HEIGHT)),
            })),
            titlebar: None,
            focus: false,
            show: false,
            kind: WindowKind::PopUp,
            is_movable: false,
            is_resizable: false,
            is_minimizable: false,
            inactive_frame_interval: None,
            window_background: WindowBackgroundAppearance::Blurred,
            ..Default::default()
        };
        let m2 = metrics.clone();
        let handle = cx
            .open_window(opts, move |window, cx| cx.new(|cx| Bar::new(window, cx, model, m2)))
            .expect("open window");

        let (tx, mut rx) = mpsc::unbounded::<Msg>();
        start_server(tx.clone(), metrics.clone());

        // Global hotkey: Control+Option+4 (Carbon RegisterEventHotKey, no permissions needed).
        match GlobalHotKeyManager::new() {
            Ok(mgr) => {
                let hk = HotKey::new(Some(Modifiers::CONTROL | Modifiers::ALT), Code::Digit4);
                if let Err(e) = mgr.register(hk) {
                    eprintln!("hotkey register failed: {e}");
                }
                let htx = tx.clone();
                GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                    if e.state == HotKeyState::Pressed {
                        let _ = htx.unbounded_send(Msg::Toggle(now_ms()));
                    }
                }));
                std::mem::forget(mgr);
            }
            Err(e) => eprintln!("hotkey manager failed: {e}"),
        }

        cx.spawn(async move |cx| {
            while let Some(msg) = rx.next().await {
                match msg {
                    Msg::Toggle(t0) => {
                        let _ = handle.update(cx, |bar, window, cx| bar.toggle(t0, window, cx));
                    }
                    Msg::State(reply) => {
                        let s = handle.update(cx, |bar, _, _| bar.state_json()).unwrap_or_default();
                        let _ = reply.send(s);
                    }
                    Msg::Snapshot { query, sel, file, path, reply } => {
                        let (file, drag) = match file.as_deref() {
                            Some("drag") => (None, Some("pdf".to_string())),
                            Some(f) => (Some(DroppedFile::from_path(std::path::Path::new(f))), None),
                            None => (None, None),
                        };
                        let _ = handle.update(cx, |bar, window, cx| bar.debug_state(&query, sel, file, drag, window, cx));
                        cx.background_executor().timer(Duration::from_millis(250)).await;
                        #[cfg(feature = "snapshot")]
                        let r = handle.update(cx, |_, window, _| window.render_to_image());
                        #[cfg(not(feature = "snapshot"))]
                        let r: Result<Result<image::RgbaImage>> = Ok(Err(anyhow::anyhow!("build with --features snapshot")));
                        let s = match r {
                            Ok(Ok(img)) => match img.save(&path) {
                                Ok(_) => format!("{{\"ok\":true,\"w\":{},\"h\":{}}}", img.width(), img.height()),
                                Err(e) => format!("{{\"error\":\"{e}\"}}"),
                            },
                            Ok(Err(e)) => format!("{{\"error\":\"{e}\"}}"),
                            Err(e) => format!("{{\"error\":\"{e}\"}}"),
                        };
                        let _ = reply.send(s);
                    }
                    Msg::SelfTest(reply) => {
                        let s = run_selftest(handle, cx).await;
                        let _ = reply.send(s);
                    }
                }
            }
        })
        .detach();
    });
}

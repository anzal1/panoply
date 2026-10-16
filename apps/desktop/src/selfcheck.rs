//! Dev only (feature `snapshot`): the headless run. It opens the real windows at x = -20000, never
//! ordered front and never key, drives them with the same calls a person's input makes, saves PNGs
//! of what GPUI rendered, and measures the budgets. Nothing is ever shown on screen.
//!
//! Frames are produced by calling `Window::draw` (layout and paint) and `render_to_image`; nothing
//! is presented. The timings are therefore the CPU side of a frame, which is what the 4 ms rule
//! on the UI thread is about.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::*;
use serde_json::{Value, json};

use crate::catalog::Catalog;
use crate::history::{Entry, History};
use crate::platform::{self, Panel};
use crate::profile::FileProfile;
use crate::ui::app::{AppView, Route};
use crate::ui::bar::{Bar, BarMode};
use crate::ui::icons::Icons;
use crate::ui::live::{app_window_options_at, bar_window_options};
use crate::ui::{FindStatus, Hub, Recents};

mod find;
pub(crate) use find::run_find;

/// The file index parts of a hub for runs that do not look at files: switched off, so nothing
/// ever scans and the bar and Find page show their "unavailable" state.
fn find_off(cx: &mut App) -> (crate::finder::Finder, Entity<FindStatus>) {
    let f = crate::finder::Finder::off();
    let st = f.state();
    (f, cx.new(|_| FindStatus { state: st }))
}

struct Drv<'a> {
    cx: &'a mut AsyncApp,
    dark: bool,
    update_ms: Vec<f64>,
    draw_ms: Vec<f64>,
    /// What the next frames are of, so a slow one can be named.
    tag: String,
    slow: Vec<(String, f64)>,
}

/// The render has no backdrop blur, so review PNGs are composited over a flat desktop colour
/// (the wireframes' blue grey, or a dark grey) to read like the real thing.
fn flatten(img: &image::RgbaImage, dark: bool) -> image::RgbaImage {
    let bg = if dark {
        image::Rgba([52, 56, 66, 255])
    } else {
        image::Rgba([201, 208, 219, 255])
    };
    let mut out = image::RgbaImage::from_pixel(img.width(), img.height(), bg);
    // render_to_image returns premultiplied colour; undo that so the overlay blends straight alpha.
    let mut straight = img.clone();
    for p in straight.pixels_mut() {
        let a = u32::from(p[3]);
        if a > 0 && a < 255 {
            for c in 0..3 {
                p[c] = ((u32::from(p[c]) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    image::imageops::overlay(&mut out, &straight, 0, 0);
    out
}

fn percentile(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    s[(((p / 100.0) * s.len() as f64).ceil() as usize).clamp(1, s.len()) - 1]
}

impl Drv<'_> {
    fn win<V: 'static, R>(
        &mut self,
        h: WindowHandle<V>,
        f: impl FnOnce(&mut Window, &mut App) -> R,
    ) -> Option<R> {
        let any: AnyWindowHandle = h.into();
        self.cx
            .update_window(any, |_, window, cx| f(window, cx))
            .ok()
    }

    /// A state change on the UI thread, timed.
    fn change<V: 'static + Render, R>(
        &mut self,
        h: WindowHandle<V>,
        f: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Option<R> {
        let t = Instant::now();
        let r = h.update(self.cx, f).ok();
        self.update_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        r
    }

    /// One frame: layout and paint, nothing presented. Timed.
    fn draw<V: 'static>(&mut self, h: WindowHandle<V>) -> f64 {
        let t = Instant::now();
        let _ = self.win(h, |window, cx| window.draw(cx).clear(cx));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        self.draw_ms.push(ms);
        if ms > 4.0 {
            self.slow.push((self.tag.clone(), ms));
        }
        ms
    }

    async fn settle<V: 'static>(&mut self, h: WindowHandle<V>) {
        for _ in 0..3 {
            self.cx
                .background_executor()
                .timer(Duration::from_millis(25))
                .await;
            self.draw(h);
        }
    }

    fn png<V: 'static>(&mut self, h: WindowHandle<V>, path: &Path) -> String {
        self.draw(h);
        let dark = self.dark;
        match self.win(h, |window, _| window.render_to_image()) {
            Some(Ok(img)) => match flatten(&img, dark).save(path) {
                Ok(()) => format!("{} ({}x{})", path.display(), img.width(), img.height()),
                Err(e) => format!("save failed: {e}"),
            },
            Some(Err(e)) => format!("render failed: {e}"),
            None => "no window".to_string(),
        }
    }

    fn a11y<V: 'static>(&mut self, h: WindowHandle<V>) -> Value {
        let _ = self.win(h, |window, _| Panel::of(window).map(|p| p.wake_a11y()));
        self.draw(h);
        self.draw(h);
        let tree = self
            .win(h, |window, _| window.debug_a11y_tree_json())
            .flatten();
        tree.and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or(Value::Null)
    }
}

/// Roles that a person or a screen reader acts on, and so must have a label.
const ACTIONABLE: &[&str] = &[
    "Button",
    "RadioButton",
    "Switch",
    "TextInput",
    "SearchInput",
    "ListBoxOption",
    "DisclosureTriangle",
    "Link",
];

/// Check a GPUI accessibility tree: every actionable node has a role and a non-empty label.
fn check_a11y(state: &str, tree: &Value) -> Value {
    let mut actionable = 0;
    let mut offenders = Vec::new();
    let mut roles: std::collections::BTreeMap<String, usize> = Default::default();
    if let Some(nodes) = tree["nodes"].as_object() {
        for (id, n) in nodes {
            let a = &n["aria"];
            let role = a["role"].as_str().unwrap_or("").to_string();
            if role.is_empty() {
                continue;
            }
            *roles.entry(role.clone()).or_default() += 1;
            if ACTIONABLE.contains(&role.as_str()) {
                actionable += 1;
                let label = a["label"].as_str().unwrap_or("");
                if label.trim().is_empty() {
                    offenders.push(json!({"id": id, "role": role}));
                }
            }
        }
    }
    json!({"state": state, "actionable": actionable, "unlabelled": offenders, "roles": roles, "pass": offenders.is_empty() && actionable > 0})
}

fn sample_pdf(path: &Path, bytes: usize) {
    let mut v = b"%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R]/Count 1>>endobj\n3 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 200 200]>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF\n".to_vec();
    v.resize(bytes, b' ');
    let _ = std::fs::write(path, v);
}

/// A smooth picture (compresses well) so "under 50 KB" is reachable.
fn sample_png(path: &Path, w: u32, h: u32) {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 / w as f32;
            let fy = y as f32 / h as f32;
            let r = (128.0 + 120.0 * (fx * 6.0).sin() * (fy * 3.0).cos()) as u8;
            let g = (255.0 * fx) as u8;
            let b = (255.0 * (1.0 - fy)) as u8;
            data.extend_from_slice(&[r, g, b]);
        }
    }
    if let Ok(file) = std::fs::File::create(path) {
        let mut enc = png::Encoder::new(file, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut wr) = enc.write_header() {
            let _ = wr.write_image_data(&data);
        }
    }
}

fn profile_with(path: &Path, bytes: u64) -> FileProfile {
    let mut p = FileProfile::read(path);
    p.bytes = bytes;
    p
}

fn seeded_recents(now: u64, home: &Path) -> Vec<Entry> {
    let e = |ago: u64, id: &str, name: &str, input: &str, head: &str| Entry {
        at: now - ago,
        tool_id: id.into(),
        tool_name: name.into(),
        inputs: vec![home.join(input)],
        options: Value::Null,
        outputs: vec![],
        headline: head.into(),
    };
    vec![
        e(
            120,
            "image.resize_to_kb",
            "Resize to exact KB",
            "signature.jpg",
            "312 KB to 47 KB, 85% smaller",
        ),
        e(
            1500,
            "pdf.compress",
            "Compress PDF",
            "Lease.pdf",
            "4.2 MB to 880 KB, 79% smaller",
        ),
        e(
            7200,
            "pdf.merge",
            "Merge PDF",
            "Invoices",
            "3 files, 2.1 MB",
        ),
        e(
            90_000,
            "image.convert",
            "Convert image",
            "IMG_2041.heic",
            "3.1 MB to 1.4 MB, 55% smaller",
        ),
    ]
}

/// A file of the kind a tool takes, for the open-time measurement and the contact sheet. The
/// bytes are just enough for the sniffer to say what it is.
fn sample_for(manifest: &panoply_manifest::Manifest, dir: &Path) -> Option<PathBuf> {
    let a = manifest.accepts.first()?;
    if a.starts_with("image") {
        sample_png(&dir.join("budget.png"), 64, 48);
        return Some(dir.join("budget.png"));
    }
    let (name, bytes): (&str, Vec<u8>) = if a.contains("pdf") {
        ("budget.pdf", b"%PDF-1.4\n%%EOF\n".to_vec())
    } else if a.starts_with("video") {
        // An MP4 `ftyp` box.
        let mut v = vec![0, 0, 0, 24];
        v.extend_from_slice(b"ftypisom\0\0\x02\0isomiso2mp41");
        ("budget.mp4", v)
    } else if a.starts_with("audio") {
        let mut v = b"ID3\x03\0\0\0\0\0\0".to_vec();
        v.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        v.resize(64, 0);
        ("budget.mp3", v)
    } else {
        ("budget.txt", b"hello\n".to_vec())
    };
    let p = dir.join(name);
    std::fs::write(&p, bytes).ok()?;
    Some(p)
}

/// Entry point for `panoply-desktop --selfcheck`.
pub(crate) fn run(out: PathBuf) {
    let _ = std::fs::create_dir_all(&out);
    let work = std::env::temp_dir().join(format!("panoply-selfcheck-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let started = Instant::now();
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        platform::set_trash_dir(work.join("trash"));
        let t_catalog = Instant::now();
        // The community example modules are loaded too, as they are in a debug build or a bundle.
        let mut dirs = crate::paths::module_search_dirs();
        dirs.push(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm"));
        let catalog = Arc::new(Catalog::with_dirs(&dirs));
        let catalog_ms = t_catalog.elapsed().as_secs_f64() * 1000.0;
        let now = crate::units::now_secs();
        let recents = cx.new(|_| Recents { entries: seeded_recents(now, Path::new("/Users/me/Documents")), loaded: true });
        let (finder, find_status) = find_off(cx);
        let hub = Hub { catalog, history: History::at(work.join("history.jsonl")), recents, finder, find_status };
        cx.set_global(hub.clone());
        let bar_hub = hub.clone();
        let bar = cx
            .open_window(bar_window_options(BarMode::Offscreen), move |window, cx| {
                cx.new(|cx| Bar::new(window, cx, bar_hub, BarMode::Offscreen))
            })
            .expect("open the offscreen bar");
        let app_hub = hub.clone();
        let app_win = cx
            .open_window(app_window_options_at(true, None), move |window, cx| cx.new(|cx| AppView::new(window, cx, app_hub)))
            .expect("open the offscreen app window");
        let out2 = out.clone();
        let work2 = work.clone();
        cx.spawn(async move |cx| {
            let mut d = Drv { cx, dark: false, update_ms: vec![], draw_ms: vec![], tag: String::new(), slow: vec![] };
            let mut log: Vec<String> = Vec::new();
            d.tag = "first frames".into();
            let first = d.draw(bar);
            let first_app = d.draw(app_win);
            d.settle(bar).await;
            d.settle(app_win).await;
            d.update_ms.clear();
            d.draw_ms.clear();
            let png = |name: &str| out2.join(name);

            // ---- the bar ----
            log.push(d.png(bar, &png("01-bar-at-rest.png")));
            let pdf = work2.join("Lease.pdf");
            sample_pdf(&pdf, 4_200_000);
            d.tag = "bar with a dropped PDF".into();
            d.change(bar, |b, _, cx| b.set_files(vec![profile_with(&pdf, 4_200_000)], cx));
            d.settle(bar).await;
            log.push(d.png(bar, &png("02-bar-pdf-dropped.png")));

            d.tag = "bar plain words".into();
            let sig = work2.join("signature.png");
            sample_png(&sig, 1200, 480);
            let sig_bytes = std::fs::metadata(&sig).map(|m| m.len()).unwrap_or(0);
            d.change(bar, |b, window, cx| {
                b.set_files(vec![FileProfile::read(&sig)], cx);
                b.set_query("make this under 50 kb", window, cx);
            });
            d.settle(bar).await;
            log.push(d.png(bar, &png("03-bar-plain-words.png")));

            d.tag = "bar convert rows".into();
            d.change(bar, |b, window, cx| {
                b.set_files(Vec::new(), cx);
                b.set_query("heic to jpg", window, cx);
            });
            d.settle(bar).await;
            log.push(d.png(bar, &png("04-bar-convert.png")));
            d.change(bar, |b, window, cx| b.set_query("docx to jpg", window, cx));
            d.settle(bar).await;
            log.push(d.png(bar, &png("05-bar-convert-route.png")));

            // ---- the panel in the bar: options, estimate, run, result ----
            d.change(bar, |b, window, cx| {
                b.set_query("", window, cx);
                b.set_files(vec![FileProfile::read(&sig)], cx);
                b.set_query("under 50 kb", window, cx);
            });
            d.settle(bar).await;
            d.tag = "bar tool panel options".into();
            let t0 = Instant::now();
            d.change(bar, |b, window, cx| b.activate(window, cx));
            let open_draw = d.draw(bar);
            let open_total = t0.elapsed().as_secs_f64() * 1000.0;
            d.settle(bar).await;
            log.push(d.png(bar, &png("06-panel-options.png")));

            // Run it for real and watch the worker.
            d.tag = "bar panel running and result".into();
            let ran_at = Instant::now();
            let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
            if let Some(p) = panel {
                d.win(bar, |window, cx| p.update(cx, |p, cx| p.run(window, cx)));
            }
            let mut saw_running = false;
            let mut run_frames = 0;
            for _ in 0..400 {
                d.cx.background_executor().timer(Duration::from_millis(25)).await;
                d.draw(bar);
                run_frames += 1;
                let (done, failed) = d
                    .change(bar, |b, _, cx| {
                        b.panel.as_ref().map(|p| (p.read(cx).is_done(), p.read(cx).failure().is_some())).unwrap_or((false, false))
                    })
                    .unwrap_or((false, false));
                if !done && !failed && !saw_running && ran_at.elapsed() > Duration::from_millis(350) {
                    saw_running = true;
                    log.push(d.png(bar, &png("07-panel-running.png")));
                }
                if done || failed {
                    break;
                }
            }
            let run_ms = ran_at.elapsed().as_secs_f64() * 1000.0;
            d.settle(bar).await;
            log.push(d.png(bar, &png("08-panel-result.png")));
            let headline = d.change(bar, |b, _, cx| b.panel.as_ref().and_then(|p| p.read(cx).result_headline())).flatten();
            let outputs = d.change(bar, |b, _, cx| b.panel.as_ref().map(|p| p.read(cx).outputs())).flatten().unwrap_or_default();
            // Undo: the new file goes to the Trash (a temp-dir file, so the real Trash gets one file at most).
            let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
            if let Some(p) = panel {
                d.win(bar, |_, cx| p.update(cx, |p, cx| p.undo(cx)));
            }
            for _ in 0..40 {
                d.cx.background_executor().timer(Duration::from_millis(25)).await;
                d.draw(bar);
            }
            let undone = outputs.iter().all(|o| !o.exists());
            log.push(d.png(bar, &png("09-panel-undone.png")));

            // A failure in plain words: a PDF that is not a PDF.
            d.tag = "bar panel failure".into();
            let junk = work2.join("Scan.pdf");
            let mut junk_bytes = b"%PDF-1.7\nthis is not a pdf at all\n".to_vec();
            junk_bytes.resize(600_000, b' ');
            let _ = std::fs::write(&junk, junk_bytes);
            d.change(bar, |b, window, cx| {
                b.close_panel(window, cx);
                b.set_files(vec![FileProfile::read(&junk)], cx);
            });
            d.settle(bar).await;
            d.change(bar, |b, window, cx| b.activate(window, cx));
            d.settle(bar).await;
            let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
            if let Some(p) = panel {
                d.win(bar, |window, cx| p.update(cx, |p, cx| p.run(window, cx)));
            }
            for _ in 0..200 {
                d.cx.background_executor().timer(Duration::from_millis(25)).await;
                d.draw(bar);
                if d.change(bar, |b, _, cx| b.panel.as_ref().is_some_and(|p| p.read(cx).failure().is_some())).unwrap_or(false) {
                    break;
                }
            }
            d.settle(bar).await;
            log.push(d.png(bar, &png("10-panel-failed.png")));
            let failure = d
                .change(bar, |b, _, cx| b.panel.as_ref().and_then(|p| p.read(cx).failure().cloned()))
                .flatten()
                .map(|f| json!({"message": f.message, "next_step": f.next_step}));

            // ---- a tool that needs a download asks first, in the panel, before anything starts ----
            // The question is put to the panel the way a job does it (the real job path is covered
            // by tests/consent.rs). A throwaway data folder, and no yes from the environment.
            d.tag = "bar consent sheet".into();
            panoply_core::assets::set_data_dir_override(Some(out2.join("consent-data")));
            panoply_core::assets::ignore_environment_yes(true);
            d.change(bar, |b, window, cx| {
                b.close_panel(window, cx);
                b.set_files(vec![FileProfile::read(&sig)], cx);
                b.set_query("under 50 kb", window, cx);
            });
            d.settle(bar).await;
            d.change(bar, |b, window, cx| b.activate(window, cx));
            d.settle(bar).await;
            let wanted = panoply_host::assets::find("isnet").map(|e| e.info);
            let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
            let mut answers = None;
            if let (Some(p), Some(info)) = (panel.clone(), wanted) {
                answers = d.win(bar, |window, cx| p.update(cx, |p, cx| p.preview_consent(info, window, cx)));
            }
            d.settle(bar).await;
            let consent_item = d.change(bar, |b, _, cx| b.panel.as_ref().and_then(|p| p.read(cx).consent_item())).flatten();
            log.push(d.png(bar, &png("10b-panel-consent.png")));
            let c_consent = check_a11y("bar consent sheet", &d.a11y(bar));
            let wrote_nothing_while_asking = !panoply_core::assets::consent_file().exists();
            // Not now: the worker is told no, and nothing is recorded.
            if let Some(p) = panel {
                d.win(bar, |_, cx| p.update(cx, |p, cx| p.answer_consent(false, cx)));
            }
            let told_no = answers.as_ref().is_some_and(|rx| rx.try_recv() == Ok(false));
            let nothing_recorded = !panoply_core::assets::consent_file().exists();
            panoply_core::assets::set_data_dir_override(None);
            panoply_core::assets::ignore_environment_yes(false);
            d.change(bar, |b, window, cx| b.close_panel(window, cx));
            d.settle(bar).await;

            // ---- an instant transform (pattern A) runs the moment Return is pressed ----
            d.tag = "bar instant transform".into();
            d.change(bar, |b, window, cx| {
                b.close_panel(window, cx);
                b.set_files(vec![FileProfile::read(&sig)], cx);
                b.set_query("jpg", window, cx);
            });
            d.settle(bar).await;
            let instant_t = Instant::now();
            d.change(bar, |b, window, cx| b.activate(window, cx));
            let mut instant_headline = None;
            for _ in 0..200 {
                d.cx.background_executor().timer(Duration::from_millis(25)).await;
                d.draw(bar);
                instant_headline = d.change(bar, |b, _, cx| b.panel.as_ref().and_then(|p| p.read(cx).result_headline())).flatten();
                if instant_headline.is_some() {
                    break;
                }
            }
            let instant_ms = instant_t.elapsed().as_secs_f64() * 1000.0;
            d.settle(bar).await;
            log.push(d.png(bar, &png("11-bar-instant-transform-result.png")));
            // "Then...": Command+Return on the result card sends the new file to the bar as the next input.
            let ks = Keystroke::parse("cmd-enter").expect("keystroke");
            d.win(bar, move |window, cx| window.dispatch_keystroke(ks, cx));
            d.settle(bar).await;
            let chained = d
                .change(bar, |b, _, _| (b.panel.is_none(), b.files.iter().map(|f| f.name.clone()).collect::<Vec<_>>()))
                .unwrap_or_default();

            // ---- the grapheme patch on GPUI Kit's input: Backspace removes a whole Indic conjunct cluster ----
            d.tag = "bar text input".into();
            d.change(bar, |b, window, cx| {
                b.close_panel(window, cx);
                b.set_files(Vec::new(), cx);
                b.set_query("", window, cx);
                b.set_query("\u{0915}\u{094D}\u{0937}\u{0947}\u{0924}\u{094D}\u{0930}", window, cx);
            });
            d.draw(bar);
            let ks = Keystroke::parse("backspace").expect("keystroke");
            d.win(bar, move |window, cx| window.dispatch_keystroke(ks, cx));
            d.settle(bar).await;
            let after_backspace = d.change(bar, |b, _, _| b.query.clone()).unwrap_or_default();
            // Stock GPUI Kit leaves "\u{0915}\u{094D}\u{0937}\u{0947}\u{0924}\u{094D}" (one code point gone); the patch removes the whole cluster.
            let grapheme_ok = after_backspace == "\u{0915}\u{094D}\u{0937}\u{0947}";

            // ---- accessibility of the bar in three states ----
            d.change(bar, |b, window, cx| {
                b.close_panel(window, cx);
                b.set_files(Vec::new(), cx);
                b.set_query("", window, cx);
            });
            d.settle(bar).await;
            let a_bar = d.a11y(bar);
            let _ = std::fs::write(out2.join("a11y-bar.json"), serde_json::to_string_pretty(&a_bar).unwrap_or_default());
            let c_bar = check_a11y("bar at rest", &a_bar);
            d.change(bar, |b, window, cx| {
                b.set_files(vec![FileProfile::read(&sig)], cx);
                b.set_query("under 50 kb", window, cx);
                b.activate(window, cx);
            });
            d.settle(bar).await;
            let a_panel = d.a11y(bar);
            let c_panel = check_a11y("bar with a tool panel", &a_panel);
            let _ = std::fs::write(out2.join("a11y-panel.json"), serde_json::to_string_pretty(&a_panel).unwrap_or_default());

            d.tag = "app window".into();
            // ---- the app window ----
            d.change(bar, |b, window, cx| b.close_panel(window, cx));
            d.tag = "app window home".into();
            d.settle(app_win).await;
            log.push(d.png(app_win, &png("12-app-home.png")));
            let a_app = d.a11y(app_win);
            let c_app = check_a11y("app window home", &a_app);
            d.tag = "app window category".into();
            d.change(app_win, |a, _, cx| a.go(Route::Category(crate::catalog::Category::Pdf), cx));
            d.settle(app_win).await;
            log.push(d.png(app_win, &png("13-app-category-pdf.png")));
            d.tag = "app window modules".into();
            d.change(app_win, |a, _, cx| a.go(Route::Modules, cx));
            d.settle(app_win).await;
            log.push(d.png(app_win, &png("14-app-modules.png")));
            d.tag = "app window history".into();
            d.change(app_win, |a, _, cx| a.go(Route::History, cx));
            d.settle(app_win).await;
            let c_hist = check_a11y("app window history", &d.a11y(app_win));
            let rz = hub.catalog.index_of("image.resize_to_kb").unwrap_or(0);
            let sigp = vec![FileProfile::read(&sig)];
            let t_app = Instant::now();
            d.tag = "app window tool panel".into();
            d.change(app_win, |a, window, cx| {
                a.open_tool(rz, sigp, vec![("max_kb".to_string(), json!(50))], false, window, cx)
            });
            d.draw(app_win);
            let app_open_ms = t_app.elapsed().as_secs_f64() * 1000.0;
            d.settle(app_win).await;
            log.push(d.png(app_win, &png("15-app-tool-panel.png")));
            let c_app_panel = check_a11y("app window tool panel", &d.a11y(app_win));
            d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
            d.settle(app_win).await;

            // ---- keyboard: Tab through the home page and record what takes focus ----
            d.change(app_win, |a, window, cx| {
                a.go(Route::Home, cx);
                a.reset_focus(window, cx);
            });
            d.settle(app_win).await;
            let mut tab_sequence: Vec<String> = Vec::new();
            for _ in 0..16 {
                let ks = Keystroke::parse("tab").expect("keystroke");
                d.win(app_win, move |window, cx| window.dispatch_keystroke(ks, cx));
                let tree = d.a11y(app_win);
                if let Some(label) = tree["gpui_focus"]
                    .as_str()
                    .and_then(|id| tree["nodes"][id]["aria"]["label"].as_str())
                {
                    tab_sequence.push(label.to_string());
                }
            }
            let mut distinct = tab_sequence.clone();
            distinct.sort();
            distinct.dedup();
            d.change(app_win, |a, window, cx| a.reset_focus(window, cx));
            d.settle(app_win).await;

            // ---- dark ----
            d.dark = true;
            crate::ui::theme::force_appearance(Some(true));
            d.settle(bar).await;
            d.settle(app_win).await;
            log.push(d.png(bar, &png("16-bar-at-rest-dark.png")));
            log.push(d.png(app_win, &png("17-app-home-dark.png")));
            d.dark = false;
            crate::ui::theme::force_appearance(Some(false));

            d.tag = "panel open sweep".into();
            // ---- budgets: panel open time for every tool that takes a file ----
            let mut opens = Vec::new();
            for (i, tool) in hub.catalog.tools().iter().enumerate() {
                let files = sample_for(&tool.manifest, &work2).map(|p| vec![FileProfile::read(&p)]).unwrap_or_default();
                let t = Instant::now();
                d.change(bar, |b, window, cx| b.open_tool(i, files, Vec::new(), false, window, cx));
                let draw_ms = d.draw(bar);
                let total = t.elapsed().as_secs_f64() * 1000.0;
                opens.push(json!({"tool": tool.id, "options": tool.manifest.options.len(), "open_and_first_frame_ms": total, "frame_ms": draw_ms}));
                d.change(bar, |b, window, cx| b.close_panel(window, cx));
                d.settle(bar).await;
            }
            let open_max = opens.iter().filter_map(|o| o["open_and_first_frame_ms"].as_f64()).fold(0.0, f64::max);

            // ---- budgets: typing in the bar ----
            let mut keystroke = Vec::new();
            let mut q = String::new();
            for ch in "compress pdf".chars() {
                q.push(ch);
                let qq = q.clone();
                let t = Instant::now();
                d.change(bar, |b, window, cx| b.set_query(&qq, window, cx));
                d.draw(bar);
                keystroke.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            d.change(bar, |b, window, cx| b.set_query("", window, cx));
            d.settle(bar).await;

            // ---- budgets: idle memory and CPU, with both windows alive and every state visited ----
            let _ = d.change(bar, |b, window, cx| {
                b.close_panel(window, cx);
                b.set_files(Vec::new(), cx);
            });
            d.settle(bar).await;
            d.cx.background_executor().timer(Duration::from_secs(3)).await;
            let rss_before = platform::proc::rss_mb();
            let cpu0 = platform::proc::cpu_seconds();
            let w0 = Instant::now();
            d.cx.background_executor().timer(Duration::from_secs(8)).await;
            let cpu1 = platform::proc::cpu_seconds();
            let idle_s = w0.elapsed().as_secs_f64();
            let idle_cpu_pct = (cpu1 - cpu0) / idle_s * 100.0;
            let rss = platform::proc::rss_mb();

            let slow: Vec<Value> = d.slow.iter().map(|(t, ms)| json!({"state": t, "ms": ms})).collect();
            let ui_update_max = d.update_ms.iter().cloned().fold(0.0, f64::max);
            let ui_draw_max = d.draw_ms.iter().cloned().fold(0.0, f64::max);
            let report = json!({
                "build": if cfg!(debug_assertions) { "debug" } else { "release" },
                "note": "Windows were never shown. A frame here is Window::draw (layout and paint on the UI thread); nothing is presented, so GPU time is not included.",
                "startup": {
                    "catalog_load_ms": catalog_ms,
                    "process_to_scenarios_ms": started.elapsed().as_secs_f64() * 1000.0,
                    "first_frame_bar_ms": first,
                    "first_frame_app_ms": first_app,
                    "tools": hub.catalog.tools().len(),
                    "community_modules": hub.catalog.tools().iter().filter(|t| matches!(t.source, crate::catalog::Source::Community(_))).count(),
                },
                "flow": {
                    "bar_panel_open_then_first_frame_ms": open_total,
                    "bar_panel_first_frame_ms": open_draw,
                    "app_panel_open_then_first_frame_ms": app_open_ms,
                    "run_wall_ms": run_ms,
                    "frames_drawn_while_running": run_frames,
                    "saw_running_state": saw_running,
                    "result_headline": headline,
                    "input_bytes": sig_bytes,
                    "outputs": outputs.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "undo_trashed_outputs": undone,
                    "then_chain": {"panel_closed": chained.0, "files_on_bar": chained.1},
                    "grapheme_patch_backspace_removes_whole_cluster": grapheme_ok,
                    "text_after_backspace": after_backspace,
                    "instant_transform_png_to_jpg": {"headline": instant_headline, "return_to_result_ms": instant_ms},
                    "failure": failure,
                    "consent": {
                        "asked_before_starting": consent_item.as_ref().map(|i| json!({"id": i.id, "name": i.name, "size": i.size_label(), "source": i.source, "license": i.license})),
                        "nothing_written_while_asking": wrote_nothing_while_asking,
                        "not_now_tells_the_job_no": told_no,
                        "not_now_recorded_nothing": nothing_recorded,
                    },
                },
                "budgets": {
                    "panel_open_ms_every_tool": opens,
                    "panel_open_max_ms": open_max,
                    "panel_open_limit_ms": 50,
                    "keystroke_to_frame_ms": {"p50": percentile(&keystroke, 50.0), "p95": percentile(&keystroke, 95.0), "max": keystroke.iter().cloned().fold(0.0, f64::max)},
                    "ui_state_change_ms": {"p50": percentile(&d.update_ms, 50.0), "p95": percentile(&d.update_ms, 95.0), "max": ui_update_max, "samples": d.update_ms.len()},
                    "ui_frame_ms": {"p50": percentile(&d.draw_ms, 50.0), "p95": percentile(&d.draw_ms, 95.0), "max": ui_draw_max, "samples": d.draw_ms.len()},
                    "ui_task_limit_ms": 4,
                    "frames_over_4ms": slow,
                    "idle_rss_mb_before_sampling": rss_before,
                    "idle_rss_mb": rss,
                    "idle_rss_limit_mb": 100,
                    "idle_cpu_percent": idle_cpu_pct,
                    "idle_sample_seconds": idle_s,
                },
                "accessibility": [c_bar, c_panel, c_app, c_hist, c_app_panel, c_consent],
                "keyboard": {
                    "tab_sequence_on_home": tab_sequence,
                    "distinct_controls_reached": distinct.len(),
                    "pass": distinct.len() >= 12,
                },
                "screens": log,
            });
            let text = serde_json::to_string_pretty(&report).unwrap_or_default();
            let _ = std::fs::write(out2.join("selfcheck-report.json"), &text);
            println!("{text}");
            let _ = std::fs::remove_dir_all(&work2);
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

/// `--selfcheck-idle [--hold N]`: a fresh process with both windows created and drawn once, nothing
/// else. Prints its pid so `footprint` can be pointed at it, holds for N seconds, then reports
/// resident memory and the CPU used while idle. This is the number the idle budget is about.
pub(crate) fn run_idle(hold_secs: u64) {
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        let t = Instant::now();
        let catalog = Arc::new(Catalog::load());
        let recents = cx.new(|_| Recents {
            entries: Vec::new(),
            loaded: true,
        });
        let (finder, find_status) = find_off(cx);
        let hub = Hub {
            catalog,
            history: History::at(std::env::temp_dir().join("panoply-idle-history.jsonl")),
            recents,
            finder,
            find_status,
        };
        cx.set_global(hub.clone());
        let bar_hub = hub.clone();
        let bar = cx
            .open_window(bar_window_options(BarMode::Offscreen), move |window, cx| {
                cx.new(|cx| Bar::new(window, cx, bar_hub, BarMode::Offscreen))
            })
            .expect("open the offscreen bar");
        let app_hub = hub.clone();
        let app_win = cx
            .open_window(app_window_options_at(true, None), move |window, cx| {
                cx.new(|cx| AppView::new(window, cx, app_hub))
            })
            .expect("open the offscreen app window");
        cx.spawn(async move |cx| {
            let mut d = Drv {
                cx,
                dark: false,
                update_ms: vec![],
                draw_ms: vec![],
                tag: String::new(),
                slow: vec![],
            };
            d.draw(bar);
            d.draw(app_win);
            d.settle(bar).await;
            d.settle(app_win).await;
            let ready_ms = t.elapsed().as_secs_f64() * 1000.0;
            println!("pid {}", std::process::id());
            let hold = hold_secs.max(10);
            d.cx.background_executor()
                .timer(Duration::from_secs(hold - 8))
                .await;
            let cpu0 = platform::proc::cpu_seconds();
            let w0 = Instant::now();
            d.cx.background_executor()
                .timer(Duration::from_secs(8))
                .await;
            let pct = (platform::proc::cpu_seconds() - cpu0) / w0.elapsed().as_secs_f64() * 100.0;
            let report = json!({
                "what": "fresh process, bar and app window created and drawn once, nothing else",
                "ready_ms_from_app_run": ready_ms,
                "rss_mb": platform::proc::rss_mb(),
                "idle_cpu_percent": pct,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&report).unwrap_or_default()
            );
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

/// One review shot: the steady-state frame cost (median of five draws), then the PNG.
fn shot<V: 'static>(
    d: &mut Drv,
    h: WindowHandle<V>,
    name: &str,
    path: &Path,
    log: &mut Vec<String>,
    times: &mut Vec<(String, f64)>,
) {
    let mut v: Vec<f64> = (0..5).map(|_| d.draw(h)).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    times.push((
        format!("{name} ({})", if d.dark { "dark" } else { "light" }),
        v[2],
    ));
    log.push(d.png(h, path));
}

/// A small synthetic home folder for the disk map renders: about 40 MB of zero-filled files with the
/// shapes a real one has (a few big videos, many small files, a build folder, downloads).
/// Launch film renders: a believable home folder of about 108 GB, as (path, bytes) pairs. Nothing is
/// written to disk: `Scan::synthetic` builds the tree straight from these.
fn film_home_listing() -> Vec<(String, u64)> {
    const MB: u64 = 1_000_000;
    const GB: u64 = 1_000_000_000;
    let mut v: Vec<(String, u64)> = Vec::new();
    let mut add = |p: &str, n: u64| v.push((p.to_string(), n));
    // Movies: several colours when drilled into (video, photos, audio).
    add("Movies/trip-2025.mp4", 18 * GB + 400 * MB);
    add("Movies/wedding-cut-v3.mov", 9 * GB + 600 * MB);
    add("Movies/birthday.mov", 3 * GB + 100 * MB);
    add("Movies/clip-a.mp4", 1 * GB + 500 * MB);
    for (i, mb) in [1200u64, 900, 700].iter().enumerate() {
        add(
            &format!("Movies/Rough cuts/rough-{:02}.mp4", i + 1),
            mb * MB,
        );
    }
    for i in 0..10 {
        add(&format!("Movies/Stills/DSC_{:04}.jpg", 4410 + i), 85 * MB);
    }
    for i in 0..5 {
        add(&format!("Movies/Edit music/bed-{:02}.wav", i + 1), 120 * MB);
    }
    // Pictures
    for i in 0..80 {
        add(&format!("Pictures/RAW/DSC_{:04}.dng", 1000 + i), 48 * MB);
    }
    for i in 0..400 {
        add(&format!("Pictures/Photos/IMG_{:04}.jpg", 1 + i), 4 * MB);
    }
    // Music
    for i in 0..240 {
        add(&format!("Music/Albums/track-{:03}.mp3", i + 1), 8 * MB);
    }
    for i in 0..60 {
        add(&format!("Music/Lossless/track-{:03}.flac", i + 1), 38 * MB);
    }
    // Documents
    add("Documents/Lease.pdf", 4 * MB);
    add("Documents/Taxes/2024-return.pdf", 7 * MB);
    add("Documents/Taxes/2023-return.pdf", 5 * MB);
    for i in 0..40 {
        add(&format!("Documents/Scans/scan-{:03}.pdf", i + 1), 25 * MB);
    }
    add("Documents/budget.xlsx", 2 * MB);
    // Developer
    for i in 0..40 {
        add(
            &format!("Developer/app/target/debug/deps/libdep{:02}.rlib", i),
            280 * MB,
        );
    }
    for i in 0..20 {
        add(
            &format!("Developer/app/target/debug/incremental/unit-{:02}.o", i),
            450 * MB,
        );
    }
    for i in 0..300 {
        add(
            &format!("Developer/web/node_modules/pkg{}/index{:03}.js", i % 12, i),
            11 * MB,
        );
    }
    // Downloads
    add("Downloads/installer.dmg", 6 * GB + 400 * MB);
    add("Downloads/ubuntu-24.04.iso", 5 * GB + 800 * MB);
    add(
        "Downloads/Screen Recording 2026-10-12.mov",
        4 * GB + 400 * MB,
    );
    add("Downloads/archive.zip", 2 * GB + 200 * MB);
    add("Downloads/payload.bin", 2 * GB + 100 * MB);
    add("Downloads/export.sqlite", 1 * GB);
    // Library (its own row is locked; a cache folder inside it can be picked)
    for i in 0..30 {
        add(
            &format!(
                "Library/Caches/com.google.Chrome/Default/Cache/data_{:02}",
                i
            ),
            100 * MB,
        );
    }
    for i in 0..12 {
        add(
            &format!(
                "Library/Caches/com.apple.Safari/Cache/fsCachedData/{:02}",
                i
            ),
            100 * MB,
        );
    }
    v
}

fn synthetic_disk_tree(root: &Path) -> std::io::Result<()> {
    let put = |rel: &str, n: usize| -> std::io::Result<()> {
        let p = root.join(rel);
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(p, vec![0u8; n])
    };
    put("Movies/trip-2025.mp4", 6_000_000)?;
    put("Movies/birthday.mov", 3_000_000)?;
    put("Movies/clip-a.mp4", 1_500_000)?;
    for i in 0..40 {
        put(&format!("Pictures/Photos/IMG_{i:04}.jpg"), 80_000)?;
    }
    for i in 0..4 {
        put(&format!("Pictures/RAW/DSC_{i:04}.dng"), 1_000_000)?;
    }
    for i in 0..8 {
        put(&format!("Music/Album/track-{i:02}.mp3"), 400_000)?;
    }
    put("Documents/Lease.pdf", 2_000_000)?;
    put("Documents/Taxes/2024-return.pdf", 600_000)?;
    put("Documents/Taxes/2023-return.pdf", 500_000)?;
    put("Documents/budget.xlsx", 400_000)?;
    for i in 0..30 {
        put(&format!("Documents/Notes/note-{i:02}.txt"), 2_000)?;
    }
    for i in 0..30 {
        put(
            &format!("Developer/app/target/debug/deps/dep-{i:02}.o"),
            100_000,
        )?;
    }
    put("Developer/app/target/debug/libapp.rlib", 2_000_000)?;
    for i in 0..60 {
        put(&format!("Developer/app/src/mod{i:02}.rs"), 3_000)?;
    }
    for i in 0..100 {
        put(
            &format!("Developer/web/node_modules/pkg{}/index{i:03}.js", i % 12),
            15_000,
        )?;
    }
    put("Downloads/installer.dmg", 3_000_000)?;
    put("Downloads/archive.zip", 2_000_000)?;
    put("Downloads/export.sqlite", 1_000_000)?;
    put("Downloads/payload.bin", 2_000_000)?;
    put("Library/Caches/com.example.browser/cache.db", 1_500_000)?;
    put("Library/Caches/com.example.browser/thumbs.dat", 700_000)?;
    Ok(())
}

/// `--selfcheck-review --out DIR`: every screen and state of the bar and the app window in light and
/// dark, one PNG each (`<state>-light.png`, `<state>-dark.png`, rendered at the display's 2x), plus
/// one contact sheet of the options panel of every tool. Nothing is shown on screen. The files in
/// `docs/design/review/` come from here.
pub(crate) fn run_review(out: PathBuf) {
    let _ = std::fs::create_dir_all(&out);
    let work = std::env::temp_dir().join(format!("panoply-review-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        platform::set_trash_dir(work.join("trash"));
        let mut dirs = crate::paths::module_search_dirs();
        dirs.push(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../modules/examples-wasm"),
        );
        let catalog = Arc::new(Catalog::with_dirs(&dirs));
        let now = crate::units::now_secs();
        let recents = cx.new(|_| Recents {
            entries: seeded_recents(now, Path::new("/Users/me/Documents")),
            loaded: true,
        });
        let (finder, find_status) = find_off(cx);
        let hub = Hub {
            catalog,
            history: History::at(work.join("history.jsonl")),
            recents: recents.clone(),
            finder,
            find_status,
        };
        cx.set_global(hub.clone());
        let bar_hub = hub.clone();
        let bar = cx
            .open_window(bar_window_options(BarMode::Offscreen), move |window, cx| {
                cx.new(|cx| Bar::new(window, cx, bar_hub, BarMode::Offscreen))
            })
            .expect("open the offscreen bar");
        let app_hub = hub.clone();
        let app_win = cx
            .open_window(app_window_options_at(true, None), move |window, cx| {
                cx.new(|cx| AppView::new(window, cx, app_hub))
            })
            .expect("open the offscreen app window");
        let out2 = out.clone();
        let work2 = work.clone();
        cx.spawn(async move |cx| {
            let mut d = Drv {
                cx,
                dark: false,
                update_ms: vec![],
                draw_ms: vec![],
                tag: String::new(),
                slow: vec![],
            };
            d.settle(bar).await;
            d.settle(app_win).await;
            let pdf = work2.join("Lease.pdf");
            sample_pdf(&pdf, 4_200_000);
            let sig = work2.join("signature.png");
            sample_png(&sig, 1200, 480);
            let junk = work2.join("Scan.pdf");
            let mut junk_bytes = b"%PDF-1.7\nthis is not a pdf at all\n".to_vec();
            junk_bytes.resize(600_000, b' ');
            let _ = std::fs::write(&junk, junk_bytes);
            let only = std::env::args().skip_while(|a| a != "--appearance").nth(1);
            let mut log: Vec<String> = Vec::new();
            let mut times: Vec<(String, f64)> = Vec::new();
            for dark in [false, true] {
                if only.as_deref().is_some_and(|o| (o == "dark") != dark) {
                    continue;
                }
                let tag = if dark { "dark" } else { "light" };
                crate::ui::theme::force_appearance(Some(dark));
                d.dark = dark;
                let seeded =
                    seeded_recents(crate::units::now_secs(), Path::new("/Users/me/Documents"));
                let seeded2 = seeded.clone();
                {
                    let rc = recents.clone();
                    d.win(bar, move |_, cx| {
                        rc.update(cx, |r, cx| {
                            r.entries = seeded2;
                            cx.notify();
                        })
                    });
                }
                let file = |name: &str| out2.join(format!("{name}-{tag}.png"));
                // Launch film renders (docs/launch): no Full Disk Access preview banner, and a move that goes through.
                let film = std::env::args().any(|a| a == "--film");

                // ---- the disk map (a real scan of a small synthetic folder) ----
                {
                    if film {
                        // Launch film: a believable ~108 GB home folder built in memory (no disk read), shown under /Users/maya.
                        let listing = film_home_listing();
                        let refs: Vec<(&str, u64)> =
                            listing.iter().map(|(p, n)| (p.as_str(), *n)).collect();
                        let scan = panoply_disk::Scan::synthetic("/Users/maya", &refs);
                        let root = PathBuf::from("/Users/maya");
                        let root2 = root.clone();
                        d.change(app_win, move |a, _, cx| {
                            let dv = a.disk_view();
                            dv.update(cx, |v, cx| {
                                v.load(scan, root2.clone(), cx);
                                v.preview_home(root2.clone(), cx);
                            });
                            a.go(Route::Disk, cx);
                        });
                        d.settle(app_win).await;
                        shot(
                            &mut d,
                            app_win,
                            "film-map",
                            &file("film-map"),
                            &mut log,
                            &mut times,
                        );
                        d.change(app_win, |a, _, cx| {
                            let dv = a.disk_view();
                            dv.update(cx, |v, cx| v.open_named("Movies", cx));
                        });
                        d.settle(app_win).await;
                        shot(
                            &mut d,
                            app_win,
                            "film-drilled",
                            &file("film-drilled"),
                            &mut log,
                            &mut times,
                        );
                        d.change(app_win, |a, _, cx| {
                            let dv = a.disk_view();
                            dv.update(cx, |v, cx| v.pick_path(&["trip-2025.mp4"], cx));
                        });
                        d.settle(app_win).await;
                        shot(
                            &mut d,
                            app_win,
                            "film-drilled-picked",
                            &file("film-drilled-picked"),
                            &mut log,
                            &mut times,
                        );
                        d.change(app_win, |a, _, cx| {
                            let dv = a.disk_view();
                            dv.update(cx, |v, cx| {
                                v.open_root(cx);
                                v.pick_path(&["Downloads", "installer.dmg"], cx);
                                v.pick_path(&["Library", "Caches", "com.google.Chrome"], cx);
                                v.show_review(true, cx);
                            });
                        });
                        d.settle(app_win).await;
                        shot(
                            &mut d,
                            app_win,
                            "film-review",
                            &file("film-review"),
                            &mut log,
                            &mut times,
                        );
                        d.change(app_win, |a, _, cx| {
                            let dv = a.disk_view();
                            dv.update(cx, |v, cx| v.preview_move_clean(cx));
                        });
                        d.settle(app_win).await;
                        shot(
                            &mut d,
                            app_win,
                            "film-moved",
                            &file("film-moved"),
                            &mut log,
                            &mut times,
                        );
                        continue;
                    }

                    let home = work2.join("Home");
                    let _ = std::fs::remove_dir_all(&home);
                    synthetic_disk_tree(&home).expect("make the synthetic tree");
                    let scan = panoply_disk::scan(&home, &panoply_disk::ScanOptions::default())
                        .expect("scan the synthetic tree");
                    let root = home.clone();
                    let home_for_picks = home.clone();
                    d.change(app_win, move |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| {
                            v.load(scan, root, cx);
                            v.preview_home(home_for_picks.clone(), cx);
                        });
                        a.go(Route::Disk, cx);
                    });
                    d.settle(app_win).await;
                    shot(
                        &mut d,
                        app_win,
                        "disk-map",
                        &file("disk-map"),
                        &mut log,
                        &mut times,
                    );
                    // The review panel with three picks and the Full Disk Access note. Nothing is
                    // moved: the render only draws the panel, and the tree is a temp folder.
                    d.change(app_win, move |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| {
                            v.pick_named(&["Movies", "Downloads"], cx);
                            // Library itself is protected, so its box is locked; a cache folder inside
                            // it can be picked, with a note.
                            v.pick_path(&["Library", "Caches", "com.example.browser"], cx);
                            v.preview_fda_banner(!film, cx);
                            v.show_review(true, cx);
                        });
                    });
                    d.settle(app_win).await;
                    shot(
                        &mut d,
                        app_win,
                        "disk-map-review",
                        &file("disk-map-review"),
                        &mut log,
                        &mut times,
                    );
                    // The move's result with Undo (a fake mover: nothing leaves the temp folder or
                    // reaches the real Trash), one item refused so the failure line shows, then
                    // the Undo result.
                    d.change(app_win, move |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| {
                            if film {
                                v.preview_move_clean(cx)
                            } else {
                                v.preview_move(false, cx)
                            }
                        });
                    });
                    d.settle(app_win).await;
                    shot(
                        &mut d,
                        app_win,
                        "disk-map-moved",
                        &file("disk-map-moved"),
                        &mut log,
                        &mut times,
                    );
                    d.change(app_win, |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| v.preview_move(true, cx));
                    });
                    d.settle(app_win).await;
                    shot(
                        &mut d,
                        app_win,
                        "disk-map-undone",
                        &file("disk-map-undone"),
                        &mut log,
                        &mut times,
                    );
                    d.change(app_win, |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| v.show_review(false, cx));
                    });
                    d.change(app_win, move |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| {
                            v.open_named("Developer", cx);
                            v.preview_fda_banner(!film, cx);
                        });
                    });
                    d.settle(app_win).await;
                    shot(
                        &mut d,
                        app_win,
                        "disk-map-drilled",
                        &file("disk-map-drilled"),
                        &mut log,
                        &mut times,
                    );
                    // The pages around the map: before a scan, mid-scan, a failed start.
                    for (kind, name) in [
                        ("idle", "disk-start"),
                        ("scanning", "disk-scanning"),
                        ("failed", "disk-failed"),
                    ] {
                        d.change(app_win, move |a, _, cx| {
                            let dv = a.disk_view();
                            dv.update(cx, |v, cx| {
                                v.preview_fda_banner(false, cx);
                                v.preview_phase(kind, cx);
                            });
                        });
                        d.settle(app_win).await;
                        shot(&mut d, app_win, name, &file(name), &mut log, &mut times);
                    }
                    d.change(app_win, |a, _, cx| {
                        let dv = a.disk_view();
                        dv.update(cx, |v, cx| v.preview_done(cx));
                        a.go(Route::Home, cx);
                    });
                    if std::env::args().any(|a| a == "--only-disk") {
                        continue;
                    }
                }

                // ---- the bar ----
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(Vec::new(), cx);
                    b.set_query("", window, cx);
                });
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-rest",
                    &file("bar-rest"),
                    &mut log,
                    &mut times,
                );
                d.change(bar, |b, _, cx| {
                    b.set_files(vec![profile_with(&pdf, 4_200_000)], cx)
                });
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-pdf",
                    &file("bar-pdf"),
                    &mut log,
                    &mut times,
                );
                d.change(bar, |b, window, cx| {
                    b.set_files(vec![FileProfile::read(&sig)], cx);
                    b.set_query("make this under 50 kb", window, cx);
                });
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-words",
                    &file("bar-words"),
                    &mut log,
                    &mut times,
                );
                d.change(bar, |b, window, cx| {
                    b.set_files(Vec::new(), cx);
                    b.set_query("heic to jpg", window, cx);
                });
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-convert",
                    &file("bar-convert"),
                    &mut log,
                    &mut times,
                );
                d.change(bar, |b, window, cx| b.set_query("zzzqx", window, cx));
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-empty",
                    &file("bar-empty"),
                    &mut log,
                    &mut times,
                );
                d.change(bar, |b, window, cx| {
                    b.set_query("", window, cx);
                    b.set_files(vec![FileProfile::read(&sig)], cx);
                    b.set_query("under 50 kb", window, cx);
                });
                d.settle(bar).await;
                d.change(bar, |b, window, cx| b.activate(window, cx));
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-options",
                    &file("bar-options"),
                    &mut log,
                    &mut times,
                );
                for _ in 0..2 {
                    let ks = Keystroke::parse("tab").expect("keystroke");
                    d.win(bar, move |window, cx| window.dispatch_keystroke(ks, cx));
                    d.draw(bar);
                }
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-options-focus",
                    &file("bar-options-focus"),
                    &mut log,
                    &mut times,
                );
                // The Format pop-up menu, open.
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(bar, |_, cx| {
                        p.update(cx, |p, cx| p.preview_menu("format", cx))
                    });
                }
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-options-menu",
                    &file("bar-options-menu"),
                    &mut log,
                    &mut times,
                );
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(bar, |_, cx| p.update(cx, |p, _| p.menu = None));
                }
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(bar, |window, cx| p.update(cx, |p, cx| p.run(window, cx)));
                }
                let ran_at = Instant::now();
                let mut saw_running = false;
                for _ in 0..400 {
                    d.cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                    d.draw(bar);
                    let done = d
                        .change(bar, |b, _, cx| {
                            b.panel
                                .as_ref()
                                .map(|p| p.read(cx).is_done() || p.read(cx).failure().is_some())
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    if !done && !saw_running && ran_at.elapsed() > Duration::from_millis(350) {
                        saw_running = true;
                        shot(
                            &mut d,
                            bar,
                            "bar-running",
                            &file("bar-running"),
                            &mut log,
                            &mut times,
                        );
                    }
                    if done {
                        break;
                    }
                }
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-result",
                    &file("bar-result"),
                    &mut log,
                    &mut times,
                );
                // Failure.
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(vec![FileProfile::read(&junk)], cx);
                });
                d.settle(bar).await;
                d.change(bar, |b, window, cx| b.activate(window, cx));
                d.settle(bar).await;
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(bar, |window, cx| p.update(cx, |p, cx| p.run(window, cx)));
                }
                for _ in 0..200 {
                    d.cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                    d.draw(bar);
                    if d.change(bar, |b, _, cx| {
                        b.panel
                            .as_ref()
                            .is_some_and(|p| p.read(cx).failure().is_some())
                    })
                    .unwrap_or(false)
                    {
                        break;
                    }
                }
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-failed",
                    &file("bar-failed"),
                    &mut log,
                    &mut times,
                );
                // Consent sheet.
                panoply_core::assets::set_data_dir_override(Some(out2.join("consent-data")));
                panoply_core::assets::ignore_environment_yes(true);
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(vec![FileProfile::read(&sig)], cx);
                    b.set_query("under 50 kb", window, cx);
                });
                d.settle(bar).await;
                d.change(bar, |b, window, cx| b.activate(window, cx));
                d.settle(bar).await;
                let wanted = panoply_host::assets::find("isnet").map(|e| e.info);
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let (Some(p), Some(info)) = (panel.clone(), wanted) {
                    let _ = d.win(bar, |window, cx| {
                        p.update(cx, |p, cx| p.preview_consent(info, window, cx))
                    });
                }
                d.settle(bar).await;
                shot(
                    &mut d,
                    bar,
                    "bar-consent",
                    &file("bar-consent"),
                    &mut log,
                    &mut times,
                );
                if let Some(p) = panel {
                    d.win(bar, |_, cx| {
                        p.update(cx, |p, cx| p.answer_consent(false, cx))
                    });
                }
                panoply_core::assets::set_data_dir_override(None);
                panoply_core::assets::ignore_environment_yes(false);
                let _ = std::fs::remove_dir_all(out2.join("consent-data"));
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(Vec::new(), cx);
                    b.set_query("", window, cx);
                });
                d.settle(bar).await;

                // ---- the app window ----
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-home",
                    &file("app-home"),
                    &mut log,
                    &mut times,
                );
                // Keyboard focus: Tab three times, then the ring shows on the third control.
                for _ in 0..3 {
                    let ks = Keystroke::parse("tab").expect("keystroke");
                    d.win(app_win, move |window, cx| window.dispatch_keystroke(ks, cx));
                    d.draw(app_win);
                }
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-focus",
                    &file("app-focus"),
                    &mut log,
                    &mut times,
                );
                {
                    let rc = recents.clone();
                    d.win(bar, move |_, cx| {
                        rc.update(cx, |r, cx| {
                            r.entries.clear();
                            cx.notify();
                        })
                    });
                }
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-home-empty",
                    &file("app-home-empty"),
                    &mut log,
                    &mut times,
                );
                {
                    let rc = recents.clone();
                    let s = seeded.clone();
                    d.win(bar, move |_, cx| {
                        rc.update(cx, |r, cx| {
                            r.entries = s;
                            cx.notify();
                        })
                    });
                }
                d.change(app_win, |a, _, cx| a.set_query("compress".into(), cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-results",
                    &file("app-results"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.set_query("zzzqx".into(), cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-results-empty",
                    &file("app-results-empty"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| {
                    a.set_query(String::new(), cx);
                    a.go(Route::Home, cx);
                });
                d.change(app_win, |a, _, cx| {
                    a.go(Route::Category(crate::catalog::Category::Pdf), cx)
                });
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-category",
                    &file("app-category"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.go(Route::Modules, cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-modules",
                    &file("app-modules"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.scroll_modules_to_model_tools(cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-modules-models",
                    &file("app-modules-models"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.go(Route::History, cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-history",
                    &file("app-history"),
                    &mut log,
                    &mut times,
                );
                {
                    let rc = recents.clone();
                    d.win(bar, move |_, cx| {
                        rc.update(cx, |r, cx| {
                            r.entries.clear();
                            cx.notify();
                        })
                    });
                }
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-history-empty",
                    &file("app-history-empty"),
                    &mut log,
                    &mut times,
                );
                {
                    let rc = recents.clone();
                    let s = seeded.clone();
                    d.win(bar, move |_, cx| {
                        rc.update(cx, |r, cx| {
                            r.entries = s;
                            cx.notify();
                        })
                    });
                }
                d.change(app_win, |a, _, cx| a.go(Route::Find, cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-find",
                    &file("app-find"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.go(Route::Nearby, cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-nearby",
                    &file("app-nearby"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.go(Route::Settings, cx));
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-settings",
                    &file("app-settings"),
                    &mut log,
                    &mut times,
                );
                let rz = hub.catalog.index_of("image.resize_to_kb").unwrap_or(0);
                let sigp = vec![FileProfile::read(&sig)];
                d.change(app_win, |a, window, cx| {
                    a.open_tool(
                        rz,
                        sigp,
                        vec![("max_kb".to_string(), json!(50))],
                        false,
                        window,
                        cx,
                    )
                });
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-tool-options",
                    &file("app-tool-options"),
                    &mut log,
                    &mut times,
                );
                let pp = hub.catalog.index_of("id.passport_photo").unwrap_or(0);
                let sigp = vec![FileProfile::read(&sig)];
                d.change(app_win, |a, window, cx| {
                    a.open_tool(pp, sigp, Vec::new(), false, window, cx)
                });
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-tool-many-options",
                    &file("app-tool-many-options"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, window, cx| {
                    a.open_tool(rz, Vec::new(), Vec::new(), false, window, cx)
                });
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-tool-no-file",
                    &file("app-tool-no-file"),
                    &mut log,
                    &mut times,
                );
                let sigp = vec![FileProfile::read(&sig)];
                d.change(app_win, |a, window, cx| {
                    a.open_tool(
                        rz,
                        sigp,
                        vec![("max_kb".to_string(), json!(50))],
                        false,
                        window,
                        cx,
                    )
                });
                d.settle(app_win).await;
                let panel = d.change(app_win, |a, _, _| a.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(app_win, |window, cx| {
                        p.update(cx, |p, cx| p.run(window, cx))
                    });
                }
                for _ in 0..400 {
                    d.cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                    d.draw(app_win);
                    if d.change(app_win, |a, _, cx| {
                        a.panel.as_ref().is_some_and(|p| p.read(cx).is_done())
                    })
                    .unwrap_or(false)
                    {
                        break;
                    }
                }
                d.settle(app_win).await;
                shot(
                    &mut d,
                    app_win,
                    "app-tool-result",
                    &file("app-tool-result"),
                    &mut log,
                    &mut times,
                );
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                d.settle(app_win).await;

                // ---- the same screens with Increase Contrast on ----
                crate::ui::theme::force_contrast(Some(true));
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(Vec::new(), cx);
                    b.set_query("", window, cx);
                });
                d.settle(bar).await;
                d.settle(app_win).await;
                shot(
                    &mut d,
                    bar,
                    "bar-rest-contrast",
                    &file("bar-rest-contrast"),
                    &mut log,
                    &mut times,
                );
                shot(
                    &mut d,
                    app_win,
                    "app-home-contrast",
                    &file("app-home-contrast"),
                    &mut log,
                    &mut times,
                );
                crate::ui::theme::force_contrast(Some(false));

                // ---- every tool's options panel, in the app window, as one contact sheet ----
                let tools: Vec<(usize, String)> = hub
                    .catalog
                    .tools()
                    .iter()
                    .enumerate()
                    .map(|(i, t)| (i, t.id.clone()))
                    .collect();
                let mut shots: Vec<image::RgbaImage> = Vec::new();
                for (i, _id) in &tools {
                    let i = *i;
                    let files = hub
                        .catalog
                        .tool(i)
                        .manifest
                        .accepts
                        .first()
                        .map(|_| ())
                        .and_then(|_| sample_for(&hub.catalog.tool(i).manifest, &work2))
                        .map(|p| vec![FileProfile::read(&p)])
                        .unwrap_or_default();
                    d.change(app_win, move |a, window, cx| {
                        a.open_tool(i, files, Vec::new(), false, window, cx)
                    });
                    d.settle(app_win).await;
                    d.draw(app_win);
                    if let Some(Ok(img)) = d.win(app_win, |window, _| window.render_to_image()) {
                        let (w, h) = (img.width() / 3, img.height() / 3);
                        shots.push(image::imageops::resize(
                            &img,
                            w,
                            h,
                            image::imageops::FilterType::Lanczos3,
                        ));
                    }
                }
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                if let Some(first) = shots.first() {
                    let (w, h) = (first.width(), first.height());
                    let cols = 6u32;
                    let rows = (shots.len() as u32).div_ceil(cols);
                    let gap = 8u32;
                    let bg = if dark {
                        image::Rgba([24, 24, 27, 255])
                    } else {
                        image::Rgba([214, 217, 224, 255])
                    };
                    let mut sheet = image::RgbaImage::from_pixel(
                        cols * w + (cols + 1) * gap,
                        rows * h + (rows + 1) * gap,
                        bg,
                    );
                    for (n, s) in shots.iter().enumerate() {
                        let (c, r) = (n as u32 % cols, n as u32 / cols);
                        image::imageops::overlay(
                            &mut sheet,
                            s,
                            (gap + c * (w + gap)) as i64,
                            (gap + r * (h + gap)) as i64,
                        );
                    }
                    let _ = sheet.save(out2.join(format!("all-panels-{tag}.png")));
                }
            }
            let _ = std::fs::write(out2.join("review-log.txt"), log.join("\n"));
            // Steady-state frame cost of every screen: the median of five draws, in milliseconds.
            let mut table = String::from("state\twarm frame ms\n");
            for (name, ms) in &times {
                table.push_str(&format!("{name}\t{ms:.2}\n"));
            }
            let _ = std::fs::write(out2.join("frame-times.tsv"), table);
            println!("{} screens", log.len());
            let _ = std::fs::remove_dir_all(&work2);
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

/// `--selfcheck-frames --out DIR`: the showcase run. The same offscreen windows as `run`, but it
/// saves the states a README or a launch post wants, in light and dark, as raw RGBA PNGs (no flat
/// desktop colour under them, so a page can set its own backdrop), and the frame sequence of one
/// whole job: typing "under 50 kb", the results updating, the run, the result card. Nothing is
/// shown on screen, and no budget is measured here.
pub(crate) fn run_frames(out: PathBuf) {
    let _ = std::fs::create_dir_all(&out);
    let work = std::env::temp_dir().join(format!("panoply-frames-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        platform::set_trash_dir(work.join("trash"));
        // The renders are README and launch images, so they show a fresh install: the built-in
        // tools and no community modules. A shipped release carries none (the release workflow does
        // not build modules/examples-wasm), and the machine's own module folders, PANOPLY_MODULES
        // and the source-tree examples must not leak in either, or the tool count in the search
        // field changes with whatever happens to be compiled or installed here.
        let catalog = Arc::new(Catalog::with_dirs(&[]));
        let now = crate::units::now_secs();
        // Two recent jobs, so the list ends above the footer and no row is cut by it.
        let recents = cx.new(|_| Recents {
            entries: seeded_recents(now, Path::new("/Users/me/Documents"))
                .into_iter()
                .take(2)
                .collect(),
            loaded: true,
        });
        let (finder, find_status) = find_off(cx);
        let hub = Hub {
            catalog,
            history: History::at(work.join("history.jsonl")),
            recents,
            finder,
            find_status,
        };
        cx.set_global(hub.clone());
        let bar_hub = hub.clone();
        let bar = cx
            .open_window(bar_window_options(BarMode::Offscreen), move |window, cx| {
                cx.new(|cx| Bar::new(window, cx, bar_hub, BarMode::Offscreen))
            })
            .expect("open the offscreen bar");
        let app_hub = hub.clone();
        let app_win = cx
            .open_window(app_window_options_at(true, None), move |window, cx| {
                cx.new(|cx| AppView::new(window, cx, app_hub))
            })
            .expect("open the offscreen app window");
        let out2 = out.clone();
        let work2 = work.clone();
        cx.spawn(async move |cx| {
            let mut d = Drv {
                cx,
                dark: false,
                update_ms: vec![],
                draw_ms: vec![],
                tag: String::new(),
                slow: vec![],
            };
            d.settle(bar).await;
            d.settle(app_win).await;
            let raw = |d: &mut Drv, h: WindowHandle<Bar>, name: &str, out: &Path| {
                d.draw(h);
                if let Some(Ok(img)) = d.win(h, |window, _| window.render_to_image()) {
                    let _ = img.save(out.join(name));
                }
            };
            let pdf = work2.join("Lease.pdf");
            sample_pdf(&pdf, 4_200_000);
            let sig = work2.join("signature.png");
            sample_png(&sig, 1200, 480);
            // A batch of pictures, so a job that reports progress file by file has a progress bar.
            let batch: Vec<PathBuf> = (1..=14)
                .map(|n| {
                    let p = work2.join(format!("Trip {n:02}.png"));
                    sample_png(&p, 1800 + n * 24, 1200 + n * 16);
                    p
                })
                .collect();
            let mut tools = 0usize;
            // One appearance per process, so the dark run starts from the same clean history as the light one.
            let only = std::env::args().skip_while(|a| a != "--appearance").nth(1);
            for dark in [false, true] {
                if only.as_deref().is_some_and(|o| (o == "dark") != dark) {
                    continue;
                }
                let tag = if dark { "dark" } else { "light" };
                crate::ui::theme::force_appearance(Some(dark));
                d.dark = dark;
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(Vec::new(), cx);
                    b.set_query("", window, cx);
                });
                d.settle(bar).await;
                d.settle(app_win).await;
                tools = hub.catalog.tools().len();

                raw(&mut d, bar, &format!("bar-rest-{tag}.png"), &out2);
                d.change(bar, |b, _, cx| {
                    b.set_files(vec![profile_with(&pdf, 4_200_000)], cx)
                });
                d.settle(bar).await;
                raw(&mut d, bar, &format!("bar-pdf-{tag}.png"), &out2);

                // The app window while history is still only the seeded rows.
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                d.settle(app_win).await;
                let _ = d
                    .win(app_win, |window, _| window.render_to_image())
                    .and_then(|r| r.ok())
                    .map(|img| img.save(out2.join(format!("app-home-{tag}.png"))));
                d.change(app_win, |a, _, cx| {
                    a.go(Route::Category(crate::catalog::Category::Pdf), cx)
                });
                d.settle(app_win).await;
                let _ = d
                    .win(app_win, |window, _| window.render_to_image())
                    .and_then(|r| r.ok())
                    .map(|img| img.save(out2.join(format!("app-pdf-{tag}.png"))));
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                d.settle(app_win).await;

                // One whole job, typed character by character. Frames are numbered; the light
                // run's frames are the ones the animated demo is cut from.
                d.change(bar, |b, window, cx| {
                    b.set_files(vec![FileProfile::read(&sig)], cx);
                    b.set_query("", window, cx);
                });
                d.settle(bar).await;
                let phrase = "under 50 kb";
                let mut n = 0;
                raw(&mut d, bar, &format!("demo-{tag}-{n:02}.png"), &out2);
                for end in phrase.char_indices().map(|(i, c)| i + c.len_utf8()) {
                    n += 1;
                    let q = phrase[..end].to_string();
                    d.change(bar, move |b, window, cx| b.set_query(&q, window, cx));
                    d.settle(bar).await;
                    raw(&mut d, bar, &format!("demo-{tag}-{n:02}.png"), &out2);
                }
                n += 1;
                raw(&mut d, bar, &format!("bar-words-{tag}.png"), &out2);
                d.change(bar, |b, window, cx| b.activate(window, cx));
                d.settle(bar).await;
                raw(&mut d, bar, &format!("demo-{tag}-{n:02}.png"), &out2);
                raw(&mut d, bar, &format!("bar-options-{tag}.png"), &out2);
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(bar, |window, cx| p.update(cx, |p, cx| p.run(window, cx)));
                }
                let ran_at = Instant::now();
                let mut run_n = 0;
                for _ in 0..400 {
                    d.cx.background_executor()
                        .timer(Duration::from_millis(25))
                        .await;
                    d.draw(bar);
                    let done = d
                        .change(bar, |b, _, cx| {
                            b.panel
                                .as_ref()
                                .map(|p| p.read(cx).is_done())
                                .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    if !done {
                        run_n += 1;
                        raw(&mut d, bar, &format!("run-{tag}-{run_n:02}.png"), &out2);
                    }
                    if done || ran_at.elapsed() > Duration::from_secs(20) {
                        break;
                    }
                }
                d.settle(bar).await;
                raw(&mut d, bar, &format!("bar-result-{tag}.png"), &out2);

                // A longer job with a progress bar: a batch, file by file.
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(batch.iter().map(|p| FileProfile::read(p)).collect(), cx);
                    b.set_query("compress", window, cx);
                });
                d.settle(bar).await;
                d.change(bar, |b, window, cx| b.activate(window, cx));
                d.settle(bar).await;
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                if let Some(p) = panel {
                    d.win(bar, |window, cx| p.update(cx, |p, cx| p.run(window, cx)));
                }
                let ran_at = Instant::now();
                let mut vn = 0;
                for _ in 0..800 {
                    d.cx.background_executor()
                        .timer(Duration::from_millis(60))
                        .await;
                    d.draw(bar);
                    let done = d
                        .change(bar, |b, _, cx| {
                            b.panel
                                .as_ref()
                                .map(|p| p.read(cx).is_done() || p.read(cx).failure().is_some())
                                .unwrap_or(true)
                        })
                        .unwrap_or(true);
                    if !done {
                        vn += 1;
                        raw(&mut d, bar, &format!("vrun-{tag}-{vn:02}.png"), &out2);
                    }
                    if done || ran_at.elapsed() > Duration::from_secs(60) {
                        break;
                    }
                }

                // The question before a download: put to the panel the way a job puts it.
                panoply_core::assets::set_data_dir_override(Some(out2.join("consent-data")));
                panoply_core::assets::ignore_environment_yes(true);
                d.change(bar, |b, window, cx| {
                    b.close_panel(window, cx);
                    b.set_files(vec![FileProfile::read(&sig)], cx);
                    b.set_query("remove background", window, cx);
                });
                d.settle(bar).await;
                d.change(bar, |b, window, cx| b.activate(window, cx));
                d.settle(bar).await;
                let wanted = panoply_host::assets::find("isnet").map(|e| e.info);
                let panel = d.change(bar, |b, _, _| b.panel.clone()).flatten();
                let mut _answers = None;
                if let (Some(p), Some(info)) = (panel.clone(), wanted) {
                    _answers = d.win(bar, |window, cx| {
                        p.update(cx, |p, cx| p.preview_consent(info, window, cx))
                    });
                }
                d.settle(bar).await;
                raw(&mut d, bar, &format!("bar-consent-{tag}.png"), &out2);
                if let Some(p) = panel {
                    d.win(bar, |_, cx| {
                        p.update(cx, |p, cx| p.answer_consent(false, cx))
                    });
                }
                panoply_core::assets::set_data_dir_override(None);
                panoply_core::assets::ignore_environment_yes(false);
                let _ = std::fs::remove_dir_all(out2.join("consent-data"));
            }
            println!("{{\"tools\": {tools}}}");
            let _ = std::fs::remove_dir_all(&work2);
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

/// `--selfcheck-userpass --manifest FILE --out DIR`: the user pass. For every entry of the manifest
/// (`[{"label", "tool", "files": [...], "options": {...}, "cancel_ms": null}]`) it opens the tool in
/// the offscreen app window, runs it through the same job path a person's Return key takes, and
/// saves the options, running, done, failed or cancelled screen in light and dark at the display's
/// 2x. `userpass.jsonl` records the outcome, the time and the slowest frame while the job ran.
/// Nothing is shown on screen. Files are written next to the inputs, so point it at copies.
pub(crate) fn run_userpass(out: PathBuf, manifest: PathBuf) {
    let _ = std::fs::create_dir_all(&out);
    let work = std::env::temp_dir().join(format!("panoply-userpass-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&work);
    let entries: Vec<Value> = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        platform::set_trash_dir(work.join("trash"));
        let mut dirs = crate::paths::module_search_dirs();
        dirs.push(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../modules/examples-wasm"),
        );
        let catalog = Arc::new(Catalog::with_dirs(&dirs));
        let recents = cx.new(|_| Recents {
            entries: Vec::new(),
            loaded: true,
        });
        let (finder, find_status) = find_off(cx);
        let hub = Hub {
            catalog,
            history: History::at(work.join("history.jsonl")),
            recents: recents.clone(),
            finder,
            find_status,
        };
        cx.set_global(hub.clone());
        let app_hub = hub.clone();
        let app_win = cx
            .open_window(app_window_options_at(true, None), move |window, cx| {
                cx.new(|cx| AppView::new(window, cx, app_hub))
            })
            .expect("open the offscreen app window");
        let out2 = out.clone();
        let work2 = work.clone();
        cx.spawn(async move |cx| {
            let mut d = Drv {
                cx,
                dark: false,
                update_ms: vec![],
                draw_ms: vec![],
                tag: String::new(),
                slow: vec![],
            };
            d.settle(app_win).await;
            let only_dark = std::env::args().skip_while(|a| a != "--appearance").nth(1);
            let only_tool = std::env::args().skip_while(|a| a != "--only").nth(1);
            let mut jsonl = String::new();
            for entry in &entries {
                let label = entry["label"].as_str().unwrap_or("x").to_string();
                let tool_id = entry["tool"].as_str().unwrap_or("").to_string();
                if only_tool.as_deref().is_some_and(|o| !label.starts_with(o)) {
                    continue;
                }
                let Some(ti) = hub.catalog.index_of(&tool_id) else {
                    jsonl.push_str(&format!(
                        "{}\n",
                        json!({"label": label, "tool": tool_id, "state": "unknown tool"})
                    ));
                    continue;
                };
                let files: Vec<PathBuf> = entry["files"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_str().map(PathBuf::from))
                            .collect()
                    })
                    .unwrap_or_default();
                let preset: Vec<(String, Value)> = entry["options"]
                    .as_object()
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let cancel_ms = entry["cancel_ms"].as_u64();
                for dark in [false, true] {
                    if only_dark.as_deref().is_some_and(|o| (o == "dark") != dark) {
                        continue;
                    }
                    let tag = if dark { "dark" } else { "light" };
                    crate::ui::theme::force_appearance(Some(dark));
                    d.dark = dark;
                    let file = |name: &str| out2.join(format!("{label}-{name}-{tag}.png"));
                    let profiles: Vec<FileProfile> =
                        files.iter().map(|p| FileProfile::read(p)).collect();
                    let t_open = Instant::now();
                    let p2 = preset.clone();
                    d.change(app_win, move |a, window, cx| {
                        a.open_tool(ti, profiles, p2, false, window, cx)
                    });
                    d.draw(app_win);
                    let first_frame_ms = t_open.elapsed().as_secs_f64() * 1000.0;
                    d.settle(app_win).await;
                    let mut log = Vec::new();
                    let mut times = Vec::new();
                    shot(&mut d, app_win, "options", &file("options"), &mut log, &mut times);
                    // The pass is for the job; run it once, in light.
                    if dark {
                        continue;
                    }
                    d.draw_ms.clear();
                    let panel = d.change(app_win, |a, _, _| a.panel.clone()).flatten();
                    let Some(p) = panel else { continue };
                    let blocker = d
                        .win(app_win, |_, cx| p.read(cx).blocker())
                        .flatten();
                    if let Some(b) = blocker {
                        jsonl.push_str(&format!(
                            "{}\n",
                            json!({"label": label, "tool": tool_id, "state": "blocked", "message": b, "first_frame_ms": first_frame_ms})
                        ));
                        continue;
                    }
                    let p_run = p.clone();
                    d.win(app_win, move |window, cx| {
                        p_run.update(cx, |p, cx| p.run(window, cx))
                    });
                    let ran_at = Instant::now();
                    let mut saw_running = false;
                    let mut cancelled_sent = false;
                    let mut state = "timeout";
                    let mut headline = String::new();
                    let mut notes: Vec<String> = Vec::new();
                    let mut outputs: Vec<String> = Vec::new();
                    for _ in 0..4800 {
                        d.cx.background_executor()
                            .timer(Duration::from_millis(25))
                            .await;
                        d.draw(app_win);
                        let snapshot = d
                            .win(app_win, |_, cx| {
                                let r = p.read(cx);
                                if r.is_done() {
                                    let h = r.result_headline().unwrap_or_default();
                                    Some(("done", h, r.outputs()))
                                } else if let Some(f) = r.failure() {
                                    Some((
                                        "error",
                                        format!("{} || {}", f.message, f.next_step),
                                        Vec::new(),
                                    ))
                                } else if r.is_options() && cancelled_sent {
                                    Some(("cancelled", String::new(), Vec::new()))
                                } else {
                                    None
                                }
                            })
                            .flatten();
                        if let Some((s, h, o)) = snapshot {
                            state = s;
                            headline = h;
                            outputs = o.iter().map(|p| p.display().to_string()).collect();
                            break;
                        }
                        let el = ran_at.elapsed();
                        if !saw_running && el > Duration::from_millis(400) {
                            saw_running = true;
                            shot(&mut d, app_win, "running", &file("running"), &mut log, &mut times);
                        }
                        if let Some(c) = cancel_ms
                            && !cancelled_sent
                            && el > Duration::from_millis(c)
                        {
                            cancelled_sent = true;
                            d.win(app_win, |_, cx| p.update(cx, |p, _| p.cancel()));
                        }
                    }
                    let elapsed_ms = ran_at.elapsed().as_secs_f64() * 1000.0;
                    let max_draw = d.draw_ms.iter().cloned().fold(0.0, f64::max);
                    d.settle(app_win).await;
                    let name = match state {
                        "done" => "done",
                        "error" => "error",
                        "cancelled" => "cancelled",
                        _ => "timeout",
                    };
                    shot(&mut d, app_win, name, &file(name), &mut log, &mut times);
                    // Notes live on the card; read them from the report via the clipboard path is
                    // not possible offscreen, so the CLI log has the full list.
                    let _ = &mut notes;
                    jsonl.push_str(&format!(
                        "{}\n",
                        json!({
                            "label": label, "tool": tool_id, "state": state,
                            "elapsed_ms": elapsed_ms.round(), "max_draw_ms": (max_draw * 100.0).round() / 100.0,
                            "first_frame_ms": (first_frame_ms * 100.0).round() / 100.0,
                            "headline": headline, "outputs": outputs,
                        })
                    ));
                    let _ = std::fs::write(out2.join("userpass.jsonl"), &jsonl);
                    // Back to the home screen so the next tool opens clean.
                    d.change(app_win, |a, _, cx| a.go(Route::Home, cx));
                }
            }
            let _ = std::fs::write(out2.join("userpass.jsonl"), &jsonl);
            let _ = std::fs::remove_dir_all(&work2);
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

//! `--selfcheck-find --out DIR`: every state of the Find page and of the command bar's file rows,
//! in light and dark, rendered offscreen at 2x (nothing is shown on screen), plus the numbers that
//! matter for the feature: how long a keystroke takes to reach a drawn frame and whether the tool
//! rows were on the list before the files.
//!
//! The index is a small folder tree made in a temp directory; nothing here ever scans a real
//! folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use gpui::*;
use serde_json::{Value, json};

use super::{Drv, percentile};
use crate::catalog::Catalog;
use crate::finder::{Finder, State};
use crate::history::History;
use crate::platform;
use crate::ui::app::{AppView, Route};
use crate::ui::bar::{Bar, BarMode};
use crate::ui::icons::Icons;
use crate::ui::live::{app_window_options_at, bar_window_options};
use crate::ui::{FindStatus, Hub, Recents};
use panoply_find::{Exclusions, Index, LiveIndex, WalkOptions};

/// Files for the fixture: path under the home folder and age in seconds.
fn fixture_files() -> Vec<(String, u64)> {
    const DAY: u64 = 86_400;
    let mut v: Vec<(String, u64)> = [
        ("Downloads/Quarterly report final.pdf", 2 * DAY),
        ("Documents/Taxes/Quarterly report 2025.pdf", 5 * DAY),
        ("Documents/Taxes/Quarterly report 2024.pdf", 120 * DAY),
        ("Documents/Work/Q3 report draft.docx", 2 * DAY),
        ("Documents/Work/Report template.docx", 40 * DAY),
        ("Downloads/annual_report_summary.pdf", 90 * DAY),
        ("Downloads/backup-2026.zip", 5 * DAY),
        ("Desktop/Screenshot 2026-10-12 at 9.14.02 AM.png", DAY),
        ("Desktop/resize-test.png", 4 * 3600),
        ("Documents/Resize photos for web.pdf", 12 * DAY),
        ("Documents/Notes/resize-notes.txt", 20 * DAY),
        ("Pictures/Goa 2025/IMG_0412.heic", 330 * DAY),
        ("Pictures/Goa 2025/IMG_0413.heic", 330 * DAY),
        ("Music/Albums/Kind of Blue/01 So What.flac", 700 * DAY),
        ("Movies/Wedding/ceremony-cut-v3.mov", 60 * DAY),
        ("Documents/Finance/Quarterly budget.xlsx", 9 * DAY),
        ("Documents/Finance/Quarterly review slides.key", 21 * DAY),
        ("Downloads/Quarterly newsletter Q3.pdf", 14 * DAY),
        ("Documents/Finance/quarter-end checklist.txt", 45 * DAY),
        ("projects/panoply/README.md", 6 * 3600),
        ("projects/panoply/src/main.rs", 3 * DAY),
        (
            "Documents/Clients/Northwind Traders/2026/Contracts and amendments/Signed copies/Final versions for archive/Northwind Traders master services agreement amended and restated and signed.pdf",
            9 * DAY,
        ),
    ]
    .iter()
    .map(|(p, a)| ((*p).to_string(), *a))
    .collect();
    // A long list: more matches than the page will show.
    for i in 1..=240u64 {
        v.push((
            format!("Documents/Invoices/invoice-{i:04}.pdf"),
            i * 3600 * 5,
        ));
    }
    v
}

/// Make the fixture home folder and an index over it. Returns the service and the home path.
pub(super) fn demo_finder(work: &Path) -> Finder {
    let home = fs::canonicalize({
        let h = work.join("home");
        let _ = fs::create_dir_all(&h);
        h
    })
    .unwrap_or_else(|_| work.join("home"));
    let now = SystemTime::now();
    for (rel, age) in fixture_files() {
        let p = home.join(&rel);
        if let Some(dir) = p.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if fs::write(&p, b"x").is_ok()
            && let Ok(f) = fs::OpenOptions::new().write(true).open(&p)
        {
            let _ = f.set_modified(now - Duration::from_secs(age));
        }
    }
    // Launch film: a believable index. About 412,000 files in all, as plain empty files with names that hold no "q",
    // so a query for "quarter" still matches only the real fixture names above. The disk map in the film has the same count.
    let filler = 412_000usize.saturating_sub(fixture_files().len());
    for i in 0..filler {
        if i % 1000 == 0 {
            let _ = fs::create_dir_all(home.join(format!("Archive/set{:03}", i / 1000)));
        }
        let _ =
            fs::File::create(home.join(format!("Archive/set{:03}/f{:05}.txt", i / 1000, i % 1000)));
    }
    let opts = WalkOptions {
        threads: 2,
        ..WalkOptions::default()
    };
    let (idx, _) = Index::build(std::slice::from_ref(&home), &opts).expect("index the fixture");
    Finder::with_index(LiveIndex::new(idx, Exclusions::none()), home)
}

fn tag(d: &mut Drv, t: &str) {
    d.tag = t.to_string();
}

/// Set the look: every PNG of a state is made once per look.
fn look(d: &mut Drv, dark: bool) {
    d.dark = dark;
    crate::ui::theme::force_appearance(Some(dark));
}

pub(crate) fn run_find(out: PathBuf) {
    let _ = fs::create_dir_all(&out);
    let work = std::env::temp_dir().join(format!("panoply-selfcheck-find-{}", std::process::id()));
    let _ = fs::create_dir_all(&work);
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        platform::set_trash_dir(work.join("trash"));
        let mut dirs = crate::paths::module_search_dirs();
        dirs.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../modules/examples-wasm"));
        let catalog = Arc::new(Catalog::with_dirs(&dirs));
        let finder = demo_finder(&work);
        let entries = match finder.state() {
            State::Ready { entries, .. } => entries,
            _ => 0,
        };
        let find_status = cx.new(|_| FindStatus {
            state: finder.state(),
        });
        let recents = cx.new(|_| Recents {
            entries: Vec::new(),
            loaded: true,
        });
        let hub = Hub {
            catalog,
            history: History::at(work.join("history.jsonl")),
            recents,
            finder,
            find_status: find_status.clone(),
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
            let mut log: Vec<String> = Vec::new();
            let mut keystroke_ms: Vec<f64> = Vec::new();
            let mut tools_first = true;
            let mut files_arrived = true;
            let mut long_hits = 0usize;
            d.draw(bar);
            d.draw(app_win);
            d.settle(bar).await;
            d.settle(app_win).await;
            d.update_ms.clear();
            d.draw_ms.clear();

            for dark in [false, true] {
                look(&mut d, dark);
                let sfx = if dark { "-dark" } else { "" };
                let png = |name: &str| out2.join(format!("{name}{sfx}.png"));
                d.settle(bar).await;
                d.settle(app_win).await;

                // ---- the Find page ----
                tag(&mut d, "find empty");
                d.change(app_win, |a, _, cx| a.go(Route::Find, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("18-find-empty")));

                tag(&mut d, "find typing");
                // Type it one letter at a time, as a person does, and time each keystroke to a frame.
                let mut typed = String::new();
                for ch in "quarterly".chars() {
                    typed.push(ch);
                    let q = typed.clone();
                    let t0 = Instant::now();
                    d.change(app_win, |a, window, cx| a.find_show(&q, window, cx));
                    d.draw(app_win);
                    keystroke_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                    // One plate per keystroke, for the launch film (docs/launch): film-find-type-1 .. 6.
                    if typed.chars().count() >= 2 {
                        let _ = d.png(app_win, &png(&format!("film-find-type-{}", typed.chars().count())));
                    }
                    d.cx.background_executor()
                        .timer(Duration::from_millis(15))
                        .await;
                }
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("19-find-results")));

                tag(&mut d, "find selected");
                d.change(app_win, |a, _, cx| a.find_select(2, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("20-find-selected")));

                tag(&mut d, "find no results");
                d.change(app_win, |a, window, cx| a.find_show("zqxjv", window, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("21-find-no-results")));

                tag(&mut d, "find long path");
                d.change(app_win, |a, window, cx| a.find_show("northwind", window, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("24-find-long-path")));

                tag(&mut d, "find long list");
                d.change(app_win, |a, window, cx| a.find_show("invoice", window, cx));
                d.settle(app_win).await;
                long_hits = d.change(app_win, |a, _, _| a.find_hit_count()).unwrap_or(0);
                log.push(d.png(app_win, &png("25-find-long-list")));
                d.change(app_win, |a, _, cx| a.find_scroll_to_end(cx));
                d.settle(app_win).await;
                d.change(app_win, |a, _, cx| a.find_select(199, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("26-find-long-list-end")));

                // The two states that come from the service, not the query.
                tag(&mut d, "find indexing");
                d.change(app_win, |a, window, cx| a.find_show("report", window, cx));
                find_status.update(d.cx, |s, cx| {
                    s.state = State::Indexing { entries: 412_331 };
                    cx.notify();
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("22-find-indexing")));
                // Stale hits from the last query must not leak a count into the field.
                tag(&mut d, "find permission note");
                find_status.update(d.cx, |s, cx| {
                    s.state = State::Ready {
                        entries,
                        fresh: true,
                    };
                    cx.notify();
                });
                d.change(app_win, |a, window, cx| a.find_show("report", window, cx));
                d.settle(app_win).await;
                d.change(app_win, |a, _, cx| a.find_show_note("Quarterly report final.pdf", true, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("31-find-note")));
                tag(&mut d, "find unavailable");
                find_status.update(d.cx, |s, cx| {
                    s.state = State::Unavailable {
                        reason: "Your files could not be read: permission denied.".into(),
                    };
                    cx.notify();
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("23-find-unavailable")));
                find_status.update(d.cx, |s, cx| {
                    s.state = State::Ready {
                        entries,
                        fresh: true,
                    };
                    cx.notify();
                });
                d.change(app_win, |a, window, cx| a.find_show("", window, cx));
                d.change(app_win, |a, _, cx| a.go(Route::Home, cx));

                // ---- the command bar ----
                tag(&mut d, "bar tools only");
                d.change(bar, |b, window, cx| b.set_query("split", window, cx));
                d.settle(bar).await;
                log.push(d.png(bar, &png("27-bar-tools-only")));

                tag(&mut d, "bar no results");
                d.change(bar, |b, window, cx| b.set_query("zqxjvkw", window, cx));
                d.settle(bar).await;
                log.push(d.png(bar, &png("32-bar-no-results")));

                tag(&mut d, "bar tools and files");
                let t0 = Instant::now();
                d.change(bar, |b, window, cx| b.set_query("resize", window, cx));
                let first = d.change(bar, |b, _, _| b.section_titles()).unwrap_or_default();
                keystroke_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                // Right after the keystroke the tools are on the list and the files are still coming.
                if first.0.iter().any(|t| t == "Files") || !first.0.iter().any(|t| t != "Files") {
                    tools_first = false;
                }
                d.settle(bar).await;
                let after = d.change(bar, |b, _, _| b.section_titles()).unwrap_or_default();
                if !after.0.iter().any(|t| t == "Files") || after.1 {
                    files_arrived = false;
                }
                log.push(d.png(bar, &png("28-bar-tools-and-files")));

                tag(&mut d, "bar file selected");
                let tool_rows = d
                    .change(bar, |b, _, _| b.results.sections.iter().take_while(|s| s.title != "Files").map(|s| s.rows.len()).sum::<usize>())
                    .unwrap_or(0);
                d.change(bar, |b, _, cx| b.select_row(tool_rows, cx));
                d.settle(bar).await;
                log.push(d.png(bar, &png("30-bar-file-selected")));

                tag(&mut d, "bar files only");
                d.change(bar, |b, window, cx| b.set_query("quarterly", window, cx));
                d.settle(bar).await;
                log.push(d.png(bar, &png("29-bar-files-only")));
                d.change(bar, |b, window, cx| b.set_query("", window, cx));
                d.settle(bar).await;
            }
            look(&mut d, false);

            let report = json!({
                "build": if cfg!(debug_assertions) { "debug" } else { "release" },
                "fixture_entries": entries,
                "long_list_hits": long_hits,
                "keystroke_to_frame_ms": {
                    "p50": percentile(&keystroke_ms, 50.0),
                    "p95": percentile(&keystroke_ms, 95.0),
                    "max": keystroke_ms.iter().cloned().fold(0.0, f64::max),
                    "samples": keystroke_ms.len(),
                },
                "ui_state_change_ms": {
                    "p50": percentile(&d.update_ms, 50.0),
                    "p95": percentile(&d.update_ms, 95.0),
                    "max": d.update_ms.iter().cloned().fold(0.0, f64::max),
                },
                "ui_frame_ms": {
                    "p50": percentile(&d.draw_ms, 50.0),
                    "p95": percentile(&d.draw_ms, 95.0),
                    "max": d.draw_ms.iter().cloned().fold(0.0, f64::max),
                },
                "frames_over_4ms": d.slow.iter().map(|(t, ms)| json!({"state": t, "ms": ms})).collect::<Vec<Value>>(),
                "bar_tools_on_list_before_files": tools_first,
                "bar_files_arrived_after_settle": files_arrived,
                "screens": log,
            });
            let text = serde_json::to_string_pretty(&report).unwrap_or_default();
            let _ = fs::write(out2.join("find-report.json"), &text);
            println!("{text}");
            let _ = fs::remove_dir_all(&work2);
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

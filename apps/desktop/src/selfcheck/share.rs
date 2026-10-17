//! `--selfcheck-share --out DIR`: every state of the Share page, in light and dark, rendered
//! offscreen (nothing is shown on screen, no socket is opened, no file is sent). The page runs with
//! its engine switched off and the view model is set by hand, the same way the live engine sets it.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::*;
use serde_json::json;

use super::{Drv, find_off};
use crate::catalog::Catalog;
use crate::history::History;
use crate::platform;
use crate::share::{
    DeviceKind, DeviceRow, EngineEvent, IncomingFileRow, IncomingView, Net, Problem, QrClosed,
    QrView, SendPhase,
};
use crate::ui::app::{AppView, Route};
use crate::ui::icons::Icons;
use crate::ui::live::app_window_options_at;
use crate::ui::{Hub, Recents};
use panoply_share::{Direction, TransferKind};

fn device(id: &str, alias: &str, model: &str, kind: DeviceKind, ip: &str) -> DeviceRow {
    DeviceRow {
        id: id.into(),
        alias: alias.into(),
        model: model.into(),
        kind,
        addr: ip.into(),
    }
}

fn devices() -> Vec<DeviceRow> {
    vec![
        device(
            "d1",
            "Anzal's iPhone",
            "iOS",
            DeviceKind::Phone,
            "192.168.1.24",
        ),
        device(
            "d2",
            "Kitchen Mac mini",
            "macOS",
            DeviceKind::Computer,
            "192.168.1.31",
        ),
        device(
            "d3",
            "Priya's very long laptop name that keeps going past any reasonable width",
            "Windows",
            DeviceKind::Computer,
            "192.168.1.52",
        ),
        device("d4", "Pixel 9", "", DeviceKind::Phone, "192.168.1.77"),
    ]
}

/// A QR-looking pattern with the right size. The real code comes from the share crate in the
/// loopback test; here the page only needs something to draw.
fn qr(url: &str) -> QrView {
    let (side, modules) = panoply_share::qr::qr_modules(url).unwrap();
    QrView {
        url: url.into(),
        side,
        modules,
        on_lan: true,
        files: 2,
        visitors: 0,
        downloads: 0,
        closed: None,
    }
}

fn prompt(risky: bool, verified: bool) -> IncomingView {
    IncomingView {
        id: "in-1".into(),
        sender: "Anzal's iPhone".into(),
        check_code: "river stone lamp oak".into(),
        verified,
        files: vec![
            IncomingFileRow {
                name: "IMG_0412.HEIC".into(),
                size: 3_400_000,
                risky: false,
            },
            IncomingFileRow {
                name: "IMG_0413.HEIC".into(),
                size: 3_900_000,
                risky: false,
            },
            IncomingFileRow {
                name: if risky {
                    "installer.command".into()
                } else {
                    "Boarding pass.pdf".into()
                },
                size: 182_000,
                risky,
            },
            IncomingFileRow {
                name: "a-file-with-a-name-that-is-far-too-long-to-fit-on-one-line-of-the-prompt-card.mov"
                    .into(),
                size: 48_000_000,
                risky: false,
            },
            IncomingFileRow {
                name: "Notes.txt".into(),
                size: 1_200,
                risky: false,
            },
            IncomingFileRow {
                name: "Tickets.zip".into(),
                size: 9_000_000,
                risky: false,
            },
        ],
        total: 64_000_000,
        text: None,
    }
}

pub(crate) fn run_share(out: PathBuf) {
    let _ = fs::create_dir_all(&out);
    let work = std::env::temp_dir().join(format!("panoply-selfcheck-share-{}", std::process::id()));
    let _ = fs::create_dir_all(&work);
    // Files to put on the waiting list: sparse, so nothing big is written.
    let trip = work.join("Goa trip 2025.zip");
    let report = work.join("Quarterly report final.pdf");
    let album = work.join("Wedding photos");
    let _ = fs::create_dir_all(&album);
    for (p, n) in [
        (&trip, 412_000_000u64),
        (&report, 1_800_000),
        (&album.join("a.jpg"), 5_200_000),
    ] {
        if let Ok(f) = fs::File::create(p) {
            let _ = f.set_len(n);
        }
    }
    let staged = vec![trip.clone(), report.clone(), album.clone()];
    let app = gpui_platform::application().with_assets(Icons);
    app.run(move |cx: &mut App| {
        gpui_base::init(cx);
        crate::ui::live::bind_input_keys(cx);
        cx.set_activation_policy(ActivationPolicy::Accessory);
        crate::ui::theme::force_appearance(Some(false));
        platform::set_trash_dir(work.join("trash"));
        let catalog = Arc::new(Catalog::with_dirs(&[]));
        let (finder, find_status) = find_off(cx);
        let recents = cx.new(|_| Recents {
            entries: Vec::new(),
            loaded: true,
        });
        let hub = Hub {
            catalog,
            history: History::at(work.join("history.jsonl")),
            recents,
            finder,
            find_status,
        };
        cx.set_global(hub.clone());
        let app_win = cx
            .open_window(app_window_options_at(true, None), move |window, cx| {
                cx.new(|cx| {
                    let mut a = AppView::new(window, cx, hub);
                    a.share_offline();
                    a
                })
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
            d.draw(app_win);
            d.settle(app_win).await;

            macro_rules! model {
                ($f:expr) => {{
                    d.change(app_win, |a, _, cx| {
                        let m = a.share_model_mut(cx);
                        #[allow(clippy::redundant_closure_call)]
                        ($f)(m);
                    });
                }};
            }
            let on = |m: &mut crate::share::ShareModel| {
                m.net = Net::On {
                    alias: "Anzal's MacBook".into(),
                };
            };

            for dark in [false, true] {
                d.dark = dark;
                crate::ui::theme::force_appearance(Some(dark));
                let sfx = if dark { "-dark" } else { "" };
                let png = |name: &str| out2.join(format!("{name}{sfx}.png"));
                d.change(app_win, |a, _, cx| a.go(Route::Share, cx));
                // The page is opened with the engine off: reset the model for this pass.
                let staged_paths = staged.clone();
                model!(|m: &mut crate::share::ShareModel| {
                    *m = crate::share::ShareModel::new(PathBuf::from("/Users/anzal/Downloads"));
                    m.net = Net::Starting;
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-01-starting")));

                // 1. Empty: still looking, then nothing found.
                model!(on);
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-02-empty-looking")));
                model!(|m: &mut crate::share::ShareModel| m.apply(EngineEvent::ScanFinished));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-03-empty-none-found")));

                // 2. Devices found, nothing chosen yet.
                model!(|m: &mut crate::share::ShareModel| m.apply(EngineEvent::Devices(devices())));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-04-devices")));

                // 3. Files waiting, receive on.
                let sp = staged_paths.clone();
                model!(move |m: &mut crate::share::ShareModel| {
                    m.stage(&sp);
                    m.receiving = true;
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-05-ready-to-send")));

                // 4. Sending: waiting for the other side, then moving bytes.
                model!(|m: &mut crate::share::ShareModel| {
                    m.begin_send("Kitchen Mac mini");
                    m.apply(EngineEvent::SendStarted { id: "t1".into() });
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-06-send-waiting")));
                model!(|m: &mut crate::share::ShareModel| {
                    m.apply(EngineEvent::Transfer {
                        id: "t1".into(),
                        direction: Direction::Send,
                        peer: "Kitchen Mac mini".into(),
                        kind: TransferKind::Started {
                            files: 3,
                            bytes: 419_000_000,
                        },
                    });
                    m.apply(EngineEvent::Transfer {
                        id: "t1".into(),
                        direction: Direction::Send,
                        peer: "Kitchen Mac mini".into(),
                        kind: TransferKind::Progress {
                            file: "Goa trip 2025.zip".into(),
                            file_done: 150_000_000,
                            file_size: 412_000_000,
                            total_done: 150_000_000,
                            total: 419_000_000,
                        },
                    });
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-07-sending")));

                // 5. Done, and the ways it can end badly.
                model!(
                    |m: &mut crate::share::ShareModel| m.apply(EngineEvent::SendEnded {
                        id: "t1".into(),
                        result: Ok((3, 419_000_000)),
                    })
                );
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-08-send-done")));
                for (name, p) in [
                    ("share-09-error-no-answer", Problem::Unreachable),
                    ("share-10-error-declined", Problem::Declined),
                    ("share-11-cancelled", Problem::Cancelled),
                ] {
                    model!(move |m: &mut crate::share::ShareModel| {
                        m.dismiss_send();
                        m.begin_send("Kitchen Mac mini");
                        m.apply(EngineEvent::SendEnded {
                            id: String::new(),
                            result: Err(p.clone()),
                        });
                        if let Some(s) = &m.send {
                            debug_assert!(matches!(
                                s.phase,
                                SendPhase::Failed(_) | SendPhase::Cancelled
                            ));
                        }
                    });
                    d.settle(app_win).await;
                    log.push(d.png(app_win, &png(name)));
                }
                model!(|m: &mut crate::share::ShareModel| m.dismiss_send());

                // 6. An incoming request: the prompt, then progress and done.
                model!(|m: &mut crate::share::ShareModel| m
                    .apply(EngineEvent::Incoming(prompt(false, true))));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-12-incoming-prompt")));
                model!(|m: &mut crate::share::ShareModel| m
                    .apply(EngineEvent::Incoming(prompt(true, false))));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-13-incoming-prompt-warnings")));
                model!(|m: &mut crate::share::ShareModel| {
                    m.answered(true);
                    m.apply(EngineEvent::Transfer {
                        id: "in-1".into(),
                        direction: Direction::Receive,
                        peer: "Anzal's iPhone".into(),
                        kind: TransferKind::Progress {
                            file: "IMG_0413.HEIC".into(),
                            file_done: 1_000_000,
                            file_size: 3_900_000,
                            total_done: 24_000_000,
                            total: 64_000_000,
                        },
                    });
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-14-receiving")));
                model!(|m: &mut crate::share::ShareModel| {
                    m.apply(EngineEvent::Transfer {
                        id: "in-1".into(),
                        direction: Direction::Receive,
                        peer: "Anzal's iPhone".into(),
                        kind: TransferKind::FileDone {
                            file: "IMG_0412.HEIC".into(),
                            path: Some(PathBuf::from("/Users/anzal/Downloads/IMG_0412.HEIC")),
                        },
                    });
                    m.apply(EngineEvent::Transfer {
                        id: "in-1".into(),
                        direction: Direction::Receive,
                        peer: "Anzal's iPhone".into(),
                        kind: TransferKind::Finished {
                            files: 6,
                            bytes: 64_000_000,
                            elapsed: Duration::from_secs(9),
                        },
                    });
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-15-received")));
                model!(|m: &mut crate::share::ShareModel| m.dismiss_receive());

                // 7. The QR page, open and closed.
                model!(|m: &mut crate::share::ShareModel| {
                    m.apply(EngineEvent::QrReady(qr(
                        "http://192.168.1.40:49231/9f2c4a7e1b3d58a06c4e",
                    )));
                    m.apply(EngineEvent::QrActivity {
                        visit: true,
                        download: false,
                    });
                });
                d.settle(app_win).await;
                d.change(app_win, |a, _, cx| a.share_scroll(true, cx));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-16-qr")));
                model!(|m: &mut crate::share::ShareModel| m
                    .apply(EngineEvent::QrClosed(QrClosed::Expired)));
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-17-qr-expired")));
                d.change(app_win, |a, _, cx| a.share_scroll(false, cx));
                model!(|m: &mut crate::share::ShareModel| m.qr = None);

                // 8. The network could not start.
                model!(
                    |m: &mut crate::share::ShareModel| m.apply(EngineEvent::NetFailed(
                        Problem::CannotListen(
                            "cannot listen on 0.0.0.0:53317: permission denied".into()
                        )
                    ))
                );
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-18-network-failed")));

                // 9. No files, no devices, receive off: the first thing a person sees.
                model!(|m: &mut crate::share::ShareModel| {
                    *m = crate::share::ShareModel::new(PathBuf::from("/Users/anzal/Downloads"));
                    m.net = Net::On {
                        alias: "Anzal's MacBook".into(),
                    };
                    m.scanned = true;
                });
                d.settle(app_win).await;
                log.push(d.png(app_win, &png("share-19-first-visit")));
            }

            let report = json!({ "screens": log });
            let text = serde_json::to_string_pretty(&report).unwrap_or_default();
            let _ = fs::write(out2.join("share-report.json"), &text);
            println!("{text}");
            let _ = fs::remove_dir_all(&work2);
            d.cx.update(|cx| cx.quit());
        })
        .detach();
    });
}

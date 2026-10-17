//! The Share page: devices on the network, files waiting to be sent, an incoming prompt, and a QR
//! code for phones that do not run Panoply.
//!
//! All of the logic is in `crate::share` (view model and engine). This file draws it. The engine
//! exists only while this page is open or receiving is on: [`AppView::share_sync`] starts it and
//! drops it, so a closed page has no thread, no socket and no timer.

use std::path::PathBuf;

use futures::StreamExt;
use gpui::prelude::*;
use gpui::*;

use super::{AppView, empty_state, plain_tile};
use crate::share::{
    DeviceKind, DeviceRow, Engine, EngineConfig, EngineEvent, IncomingView, Net, Problem, QrClosed,
    QrView, ReceivePhase, ReceiveView, SendPhase, SendView, ShareModel, Staged, device_sub,
    empty_copy, files_word, progress_label, staged_label,
};
use crate::ui::theme::{Theme, radius, sp, ty};
use crate::ui::widgets::{
    ButtonKind, Ramp, button, button_ex, card, icon, icon_button, note, notice, progress_bar, rule,
    switch,
};
use crate::units::format_bytes;

/// Height of a device row and of a file row.
const ROW: f32 = 56.0;
/// Rows of an incoming request listed before "and N more".
const PROMPT_ROWS: usize = 4;
/// The QR code is drawn at most this many points wide.
const QR_BOX: f32 = 232.0;

/// What the Share page remembers.
pub(crate) struct ShareState {
    pub(crate) model: ShareModel,
    engine: Option<Engine>,
    /// Bumped on every start, so events of an engine that was dropped are ignored.
    generation: u64,
    /// The page is on screen.
    visible: bool,
    /// False in the snapshot renders: the model is driven by hand and no network is touched.
    live: bool,
    focus_receive: FocusHandle,
}

impl ShareState {
    pub(crate) fn new(cx: &mut App) -> ShareState {
        ShareState {
            model: ShareModel::new(crate::share::downloads_dir()),
            engine: None,
            generation: 0,
            visible: false,
            live: true,
            focus_receive: cx.focus_handle(),
        }
    }
}

impl AppView {
    /// The page was opened or left. Starts or stops the engine.
    pub(crate) fn share_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.share.visible != visible {
            self.share.visible = visible;
            self.share_sync(cx);
        }
    }

    /// The engine runs while the page is open or receive mode is on, and not otherwise.
    fn share_sync(&mut self, cx: &mut Context<Self>) {
        let want = self.share.visible || self.share.model.receiving;
        if !self.share.live {
            return;
        }
        match (want, self.share.engine.is_some()) {
            (true, false) => self.share_start(cx),
            (false, true) => {
                let engine = self.share.engine.take();
                self.share.generation += 1;
                self.share.model.stopped();
                // Stopping waits for the node's thread, so it happens off the UI thread.
                cx.background_executor()
                    .spawn(async move { drop(engine) })
                    .detach();
                cx.notify();
            }
            _ => {}
        }
    }

    fn share_start(&mut self, cx: &mut Context<Self>) {
        self.share.model.starting();
        self.share.generation += 1;
        let generation = self.share.generation;
        let (engine, mut rx) = Engine::start(EngineConfig::app(self.share.model.receiving));
        self.share.engine = Some(engine);
        cx.spawn(async move |this, cx| {
            while let Some(ev) = rx.next().await {
                let alive = this
                    .update(cx, |app, cx| {
                        if app.share.generation != generation {
                            return false;
                        }
                        app.share_event(ev, cx);
                        true
                    })
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn share_event(&mut self, ev: EngineEvent, cx: &mut Context<Self>) {
        let failed = matches!(ev, EngineEvent::NetFailed(_));
        self.share.model.apply(ev);
        if failed {
            // Nothing is listening: drop the engine so "Try again" starts a clean one.
            if let Some(engine) = self.share.engine.take() {
                cx.background_executor()
                    .spawn(async move { drop(engine) })
                    .detach();
            }
        }
        cx.notify();
    }

    /// Put files on the waiting list and open the page (the Share button on a result, a drop).
    pub(crate) fn share_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.share.model.stage(&paths);
        if self.route != super::Route::Share {
            self.go(super::Route::Share, cx);
        } else {
            cx.notify();
        }
    }

    fn share_choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |app, cx| {
                    app.share.model.stage(&paths);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn share_send_to(&mut self, device: &DeviceRow, cx: &mut Context<Self>) {
        let Some(engine) = &self.share.engine else {
            return;
        };
        if self.share.model.staged.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = self
            .share
            .model
            .staged
            .iter()
            .map(|s| s.path.clone())
            .collect();
        self.share.model.begin_send(&device.alias);
        engine.send(&device.id, paths);
        cx.notify();
    }

    fn share_decide(&mut self, accept: bool, cx: &mut Context<Self>) {
        if let (Some(engine), Some(i)) = (&self.share.engine, &self.share.model.incoming) {
            engine.decide(&i.id, accept);
        }
        self.share.model.answered(accept);
        cx.notify();
    }

    fn share_toggle_receive(&mut self, cx: &mut Context<Self>) {
        let on = !self.share.model.receiving;
        self.share.model.receiving = on;
        if let Some(engine) = &self.share.engine {
            engine.set_receiving(on);
        }
        if !on {
            self.share.model.incoming = None;
        }
        self.share_sync(cx);
        cx.notify();
    }

    fn share_qr(&mut self, start: bool, cx: &mut Context<Self>) {
        let Some(engine) = &self.share.engine else {
            return;
        };
        if start {
            let paths = self
                .share
                .model
                .staged
                .iter()
                .map(|s| s.path.clone())
                .collect();
            engine.start_qr(paths);
        } else {
            engine.stop_qr();
            self.share.model.qr = None;
        }
        cx.notify();
    }

    fn share_cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(engine) = &self.share.engine {
            engine.cancel_send();
        }
        cx.notify();
    }

    fn share_retry(&mut self, cx: &mut Context<Self>) {
        if self.share.engine.is_none() {
            self.share_start(cx);
        } else if let Some(engine) = &self.share.engine {
            engine.rescan();
            self.share.model.scanned = false;
        }
        cx.notify();
    }

    // ---- snapshot hooks ----------------------------------------------------------------------

    /// Run without a network: the model is set by hand.
    #[cfg(feature = "snapshot")]
    pub(crate) fn share_offline(&mut self) {
        self.share.live = false;
    }

    /// Scroll the page to its end or back to its top (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn share_scroll(&mut self, end: bool, cx: &mut Context<Self>) {
        let y = if end {
            -self.page_scroll.max_offset().y.as_f32().abs()
        } else {
            0.0
        };
        self.page_scroll.set_offset(point(px(0.), px(y)));
        cx.notify();
    }

    /// Change the model by hand (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn share_model_mut(&mut self, cx: &mut Context<Self>) -> &mut ShareModel {
        cx.notify();
        &mut self.share.model
    }

    // ---- drawing -----------------------------------------------------------------------------

    pub(crate) fn share_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let m = self.share.model.clone();
        let entity = cx.entity();
        let receive = switch(
            "share-receive",
            "Receive",
            m.receiving,
            &self.share.focus_receive,
            t,
            move |_, cx| entity.update(cx, |app, cx| app.share_toggle_receive(cx)),
        );

        let body = if let Net::Failed(p) = &m.net {
            self.share_failed(p, t, cx)
        } else {
            let mut col = div().flex().flex_col().gap(px(sp::XL)).w_full();
            if let Some(i) = &m.incoming {
                col = col.child(self.share_prompt(i, t, cx));
            }
            if let Some(r) = &m.receive {
                col = col.child(self.share_received(r, &m, t, cx));
            }
            if !m.receiving {
                col = col.child(note(
                    "radio",
                    t.ink3,
                    format!(
                        "Receiving is off, so other devices cannot send to this computer. Turn on Receive to let them. It turns off again whenever Panoply starts. You are asked before anything is saved to {}.",
                        folder_name(&m)
                    ),
                    t,
                ));
            } else if m.incoming.is_none() && m.receive.is_none() {
                col = col.child(note(
                    "radio",
                    t.good,
                    match &m.net {
                        Net::On { alias } => format!(
                            "Other devices see this computer as {alias}. Anything sent here asks you first and is saved to {}.",
                            folder_name(&m)
                        ),
                        _ => "Getting ready to receive.".to_string(),
                    },
                    t,
                ));
            }
            col = col.child(self.share_send_section(&m, t, cx));
            col = col.child(self.share_qr_section(&m, t, cx));
            col.into_any_element()
        };
        self.page("share", "Share", Some(receive.into_any_element()), body, t)
    }

    fn share_failed(&mut self, p: &Problem, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let (head, text) = p.words("");
        empty_state(
            "triangle-alert",
            &head,
            &text,
            Some(
                button(
                    "share-retry",
                    "Try again",
                    ButtonKind::Primary,
                    Some("refresh-cw"),
                    t,
                    cx.listener(|app, _, _, cx| app.share_retry(cx)),
                )
                .into_any_element(),
            ),
            t,
        )
        .into_any_element()
    }

    /// The question "who is sending what", with the two buttons.
    fn share_prompt(&mut self, i: &IncomingView, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let title = match (&i.text, i.files.len()) {
            (Some(_), 0) => format!("{} sent you a message", i.sender),
            (_, 1) => format!("{} wants to send you 1 file", i.sender),
            (_, n) => format!("{} wants to send you {n} files", i.sender),
        };
        let more = i.files.len().saturating_sub(PROMPT_ROWS);
        let risky: Vec<&str> = i
            .files
            .iter()
            .filter(|f| f.risky)
            .map(|f| f.name.as_str())
            .collect();
        card(t)
            .id("share-prompt")
            .role(Role::Dialog)
            .aria_label(title.clone())
            .p(px(sp::L))
            .gap(px(sp::M))
            .border_color(t.accent)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(sp::M))
                    .child(plain_tile("radio", 40., t))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .ramp(ty::HEADLINE)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.ink)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .ramp(ty::BODY)
                                    .text_color(t.ink2)
                                    .child(format!(
                                        "{}. Nothing is saved unless you accept.",
                                        format_bytes(i.total)
                                    )),
                            ),
                    ),
            )
            .children(i.files.iter().take(PROMPT_ROWS).map(|f| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(sp::S))
                    .h(px(28.))
                    .child(icon("file", 14., t.ink3))
                    .child(file_name(&f.name, t))
                    .child(
                        div()
                            .ramp(ty::SMALL)
                            .text_color(t.ink2)
                            .child(format_bytes(f.size)),
                    )
            }))
            .when(more > 0, |d| {
                d.child(
                    div()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(format!("and {more} more")),
                )
            })
            .when_some(i.text.clone(), |d, text| {
                d.child(
                    div()
                        .p(px(sp::M))
                        .rounded(px(radius::ROW))
                        .bg(t.fill8)
                        .ramp(ty::BODY)
                        .text_color(t.ink)
                        .max_h(px(96.))
                        .overflow_hidden()
                        .child(text.chars().take(400).collect::<String>()),
                )
            })
            .when(!i.check_code.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(sp::S))
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child("Check code")
                        .child(
                            div()
                                .px(px(sp::S))
                                .py(px(2.))
                                .rounded(px(radius::KEY))
                                .bg(t.fill8)
                                .ramp(ty::BODY)
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(i.check_code.clone()),
                        )
                        .child("The sender's screen shows the same words."),
                )
            })
            .when(!i.verified, |d| {
                d.child(notice(
                    "triangle-alert",
                    t.warn,
                    "This device could not prove who it is. Accept only if you know it is yours.",
                    t,
                ))
            })
            .when(!risky.is_empty(), |d| {
                d.child(notice(
                    "triangle-alert",
                    t.warn,
                    format!(
                        "{} runs code when opened. Check that you expected it.",
                        risky.join(", ")
                    ),
                    t,
                ))
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .gap(px(sp::S))
                    .child(button(
                        "share-decline",
                        "Decline",
                        ButtonKind::Secondary,
                        None,
                        t,
                        cx.listener(|app, _, _, cx| app.share_decide(false, cx)),
                    ))
                    .child(button(
                        "share-accept",
                        "Accept",
                        ButtonKind::Primary,
                        Some("check"),
                        t,
                        cx.listener(|app, _, _, cx| app.share_decide(true, cx)),
                    )),
            )
            .into_any_element()
    }

    /// A transfer coming in: progress, then where it landed.
    fn share_received(
        &mut self,
        r: &ReceiveView,
        m: &ShareModel,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (head, sub, fraction, tone) = match &r.phase {
            ReceivePhase::Receiving { done, total } => (
                format!("Receiving from {}", r.peer),
                progress_label(*done, *total),
                Some(if *total > 0 {
                    (*done as f64 / *total as f64) as f32
                } else {
                    0.0
                }),
                None,
            ),
            ReceivePhase::Done { files, bytes } => (
                format!("Received {} from {}", files_word(*files), r.peer),
                format!("{}, saved to {}.", format_bytes(*bytes), folder_name(m)),
                Some(1.0),
                Some(("circle-check", t.good)),
            ),
            ReceivePhase::Cancelled => (
                format!("{} cancelled", r.peer),
                "Files that already arrived stay in your folder.".to_string(),
                None,
                Some(("info", t.ink2)),
            ),
            ReceivePhase::Failed(e) => (
                "The transfer failed".to_string(),
                format!("{e}."),
                None,
                Some(("triangle-alert", t.bad)),
            ),
        };
        let finished = !matches!(r.phase, ReceivePhase::Receiving { .. });
        let first = r.first.clone();
        card(t)
            .id("share-received")
            .p(px(sp::L))
            .gap(px(sp::M))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(sp::M))
                    .child(match tone {
                        Some((name, c)) => div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(40.))
                            .rounded(px(10.))
                            .bg(c.opacity(0.14))
                            .child(icon(name, 20., c))
                            .into_any_element(),
                        None => plain_tile("radio", 40., t).into_any_element(),
                    })
                    .child(heading_lines(&head, &sub, false, t))
                    .when(finished, |d| {
                        d.when_some(
                            first.filter(|_| matches!(r.phase, ReceivePhase::Done { .. })),
                            |d, p| {
                                d.child(button(
                                    "share-reveal",
                                    "Show in folder",
                                    ButtonKind::Secondary,
                                    Some("folder-open"),
                                    t,
                                    move |_, _, cx| cx.reveal_path(&p),
                                ))
                            },
                        )
                        .child(icon_button(
                            "share-receive-dismiss",
                            "Dismiss",
                            "x",
                            false,
                            t,
                            cx.listener(|app, _, _, cx| {
                                app.share.model.dismiss_receive();
                                cx.notify();
                            }),
                        ))
                    }),
            )
            .when_some(fraction.filter(|_| !finished), |d, f| {
                d.child(progress_bar(Some(f), "Receiving", t))
            })
            .into_any_element()
    }

    /// Files waiting, the send in progress or its result, and the devices.
    fn share_send_section(&mut self, m: &ShareModel, t: Theme, cx: &mut Context<Self>) -> Div {
        let sending = m.send.as_ref().is_some_and(SendView::active);
        let can_send = !m.staged.is_empty() && !sending && m.net_on();
        div()
            .flex()
            .flex_col()
            .gap(px(sp::M))
            .child(section_head(
                "Send",
                Some(
                    button(
                        "share-choose",
                        if m.staged.is_empty() {
                            "Choose files"
                        } else {
                            "Add files"
                        },
                        ButtonKind::Secondary,
                        Some("plus"),
                        t,
                        cx.listener(|app, _, _, cx| app.share_choose(cx)),
                    )
                    .into_any_element(),
                ),
                t,
            ))
            .child(self.share_staged(m, t, cx))
            .when_some(m.send.clone(), |d, s| {
                d.child(self.share_send_card(&s, t, cx))
            })
            .child(section_head(
                "Nearby",
                Some(
                    icon_button(
                        "share-rescan",
                        "Look again",
                        "refresh-cw",
                        true,
                        t,
                        cx.listener(|app, _, _, cx| app.share_retry(cx)),
                    )
                    .into_any_element(),
                ),
                t,
            ))
            .child(self.share_devices(m, can_send, t, cx))
    }

    fn share_staged(&mut self, m: &ShareModel, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        if m.staged.is_empty() {
            return card(t)
                .id("share-drop")
                .flex_row()
                .items_center()
                .gap(px(sp::M))
                .p(px(sp::L))
                .child(plain_tile("upload", 40., t))
                .child(heading_lines(
                    "Choose what to send",
                    "Drop files or folders here, or use Choose files. Pick a device below to send them.",
                    false,
                    t,
                ))
                .into_any_element();
        }
        card(t)
            .id("share-staged")
            .child(
                div()
                    .px(px(sp::L))
                    .pt(px(sp::M))
                    .pb(px(sp::S))
                    .ramp(ty::SMALL)
                    .text_color(t.ink2)
                    .child(staged_label(&m.staged)),
            )
            .children(m.staged.iter().enumerate().map(|(i, s)| {
                let path = s.path.clone();
                staged_row(i, s, t).child(icon_button(
                    format!("share-unstage-{i}"),
                    "Remove",
                    "x",
                    false,
                    t,
                    cx.listener(move |app, _, _, cx| {
                        app.share.model.unstage(&path);
                        cx.notify();
                    }),
                ))
            }))
            .into_any_element()
    }

    fn share_send_card(&mut self, s: &SendView, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let (name, tone, head, sub) = match &s.phase {
            SendPhase::Waiting => (
                "radio",
                t.accent,
                format!("Waiting for {} to accept", s.peer),
                match &s.check_code {
                    Some(code) => format!(
                        "Check code {code}. The other screen shows the same words. It has to say yes first."
                    ),
                    None => format!(
                        "{} ready. The other device has to say yes first.",
                        files_word(s.files)
                    ),
                },
            ),
            SendPhase::Sending { file, done, total } => (
                "upload",
                t.accent,
                format!("Sending to {}", s.peer),
                match (file.is_empty(), total) {
                    (_, 0) => file.clone(),
                    (true, _) => progress_label(*done, *total),
                    (false, _) => format!("{file}. {}", progress_label(*done, *total)),
                },
            ),
            SendPhase::Done { files, bytes } => (
                "circle-check",
                t.good,
                format!("Sent {} to {}", files_word(*files), s.peer),
                format!("{} arrived.", format_bytes(*bytes)),
            ),
            SendPhase::Cancelled => {
                let (h, b) = Problem::Cancelled.words(&s.peer);
                ("info", t.ink2, h, b)
            }
            SendPhase::Failed(p) => {
                let (h, b) = p.words(&s.peer);
                ("triangle-alert", t.bad, h, b)
            }
        };
        let active = s.active();
        card(t)
            .id("share-send")
            .p(px(sp::L))
            .gap(px(sp::M))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(sp::M))
                    .child(
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(40.))
                            .rounded(px(10.))
                            .bg(tone.opacity(0.14))
                            .child(icon(name, 20., tone)),
                    )
                    .child(heading_lines(&head, &sub, false, t))
                    .child(if active {
                        button(
                            "share-cancel",
                            "Cancel",
                            ButtonKind::Secondary,
                            None,
                            t,
                            cx.listener(|app, _, _, cx| app.share_cancel(cx)),
                        )
                    } else {
                        button(
                            "share-send-dismiss",
                            if matches!(s.phase, SendPhase::Done { .. }) {
                                "Done"
                            } else {
                                "Dismiss"
                            },
                            ButtonKind::Secondary,
                            None,
                            t,
                            cx.listener(|app, _, _, cx| {
                                app.share.model.dismiss_send();
                                cx.notify();
                            }),
                        )
                    }),
            )
            .when(active, |d| {
                d.child(progress_bar(s.fraction(), "Sending", t))
            })
            .into_any_element()
    }

    fn share_devices(
        &mut self,
        m: &ShareModel,
        can_send: bool,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some((title, text)) = empty_copy(m) {
            let looking = !m.scanned;
            return card(t)
                .id("share-devices-empty")
                .child(empty_state(
                    if looking { "radio" } else { "triangle-alert" },
                    title,
                    text,
                    (!looking).then(|| {
                        button(
                            "share-look-again",
                            "Look again",
                            ButtonKind::Secondary,
                            Some("refresh-cw"),
                            t,
                            cx.listener(|app, _, _, cx| app.share_retry(cx)),
                        )
                        .into_any_element()
                    }),
                    t,
                ))
                .into_any_element();
        }
        let n = m.devices.len();
        card(t)
            .id("share-devices")
            .children(m.devices.iter().enumerate().map(|(i, d)| {
                let device = d.clone();
                let label = if can_send {
                    "Send"
                } else if m.staged.is_empty() {
                    "Choose files first"
                } else {
                    "Busy"
                };
                div()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(SharedString::from(format!("share-device-{i}")))
                            .role(Role::ListItem)
                            .aria_label(format!("{}, {}", d.alias, device_sub(d)))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(sp::M))
                            .h(px(ROW))
                            .px(px(sp::L))
                            .child(plain_tile(
                                match d.kind {
                                    DeviceKind::Phone => "smartphone",
                                    DeviceKind::Computer => "monitor",
                                    _ => "radio",
                                },
                                36.,
                                t,
                            ))
                            .child(heading_lines(&d.alias, &device_sub(d), true, t))
                            .child(button_ex(
                                format!("share-send-{i}"),
                                label,
                                ButtonKind::Primary,
                                Some("share"),
                                can_send,
                                t,
                                cx.listener(move |app, _, _, cx| app.share_send_to(&device, cx)),
                            )),
                    )
                    .when(i + 1 < n, |d| {
                        d.child(div().w_full().pl(px(sp::L + 36. + sp::M)).child(rule(t)))
                    })
            }))
            .into_any_element()
    }

    fn share_qr_section(&mut self, m: &ShareModel, t: Theme, cx: &mut Context<Self>) -> Div {
        let body: AnyElement = match &m.qr {
            Some(q) => self.share_qr_card(q, t, cx),
            None => card(t)
                .id("share-qr-off")
                .flex_row()
                .items_center()
                .gap(px(sp::M))
                .p(px(sp::L))
                .child(plain_tile("qr-code", 40., t))
                .child(heading_lines(
                    "A phone without the app",
                    if m.staged.is_empty() {
                        "Choose files above, then show a code. Any phone camera opens a page that downloads them."
                    } else {
                        "Show a code. Any phone camera on this Wi-Fi opens a page that downloads them. It closes after ten minutes."
                    },
                    false,
                    t,
                ))
                .child(button_ex(
                    "share-qr-show",
                    "Show QR code",
                    ButtonKind::Secondary,
                    Some("qr-code"),
                    !m.staged.is_empty() && m.net_on(),
                    t,
                    cx.listener(|app, _, _, cx| app.share_qr(true, cx)),
                ))
                .into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap(px(sp::M))
            .child(section_head("Browser", None, t))
            .when_some(m.note.clone(), |d, n| {
                d.child(notice("triangle-alert", t.warn, n, t))
            })
            .child(body)
    }

    fn share_qr_card(&mut self, q: &QrView, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let n = q.side + 8;
        let cell = (QR_BOX / n as f32).floor().max(2.0);
        let dim = cell * n as f32;
        let side = q.side;
        let modules = q.modules.clone();
        let (head, sub): (String, String) = match q.closed {
            None => (
                "Scan with a phone camera".into(),
                if q.on_lan {
                    format!(
                        "The phone must be on the same Wi-Fi. {} to download. The page closes after ten minutes.",
                        files_word(q.files)
                    )
                } else {
                    "This computer is not on a network, so only this computer can open the page."
                        .into()
                },
            ),
            Some(QrClosed::Completed) => (
                "Downloaded".into(),
                "The page is closed. Show a new code to share again.".into(),
            ),
            Some(QrClosed::Expired) => (
                "The code expired".into(),
                "Nobody finished downloading in ten minutes. Show a new code to try again.".into(),
            ),
            Some(QrClosed::Revoked) => ("Sharing stopped".into(), "The page is closed.".into()),
        };
        let open = q.closed.is_none();
        card(t)
            .id("share-qr")
            .flex_row()
            .gap(px(sp::XL))
            .p(px(sp::L))
            .child(
                div()
                    .flex_none()
                    .size(px(dim))
                    .rounded(px(radius::ROW))
                    .overflow_hidden()
                    .when(!open, |d| d.opacity(0.25))
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                window.paint_quad(fill(bounds, white()));
                                for (i, dark) in modules.iter().enumerate() {
                                    if !*dark {
                                        continue;
                                    }
                                    let (x, y) = ((i % side) as f32, (i / side) as f32);
                                    let o = bounds.origin
                                        + point(px((x + 4.0) * cell), px((y + 4.0) * cell));
                                    window.paint_quad(fill(
                                        Bounds::new(o, size(px(cell), px(cell))),
                                        black(),
                                    ));
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(sp::S))
                    .child(
                        div()
                            .ramp(ty::HEADLINE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(head),
                    )
                    .child(div().ramp(ty::BODY).text_color(t.ink2).child(sub))
                    .when(open, |d| {
                        d.child(
                            div()
                                .ramp(ty::SMALL)
                                .text_color(t.ink2)
                                .child(q.url.clone()),
                        )
                        .child(
                            div().ramp(ty::SMALL).text_color(t.ink2).child(format!(
                                "{} opened the page. {} downloaded.",
                                count(q.visitors, "device"),
                                count(q.downloads, "file")
                            )),
                        )
                    })
                    .child(div().flex_1())
                    .child(div().flex().flex_row().child(button(
                        "share-qr-stop",
                        if open { "Stop sharing" } else { "Close" },
                        ButtonKind::Secondary,
                        None,
                        t,
                        cx.listener(|app, _, _, cx| app.share_qr(false, cx)),
                    ))),
            )
            .into_any_element()
    }
}

impl ShareModel {
    /// The node is listening.
    fn net_on(&self) -> bool {
        matches!(self.net, Net::On { .. })
    }
}

fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// "Downloads" for the usual folder, the folder's name for any other.
fn folder_name(m: &ShareModel) -> String {
    m.save_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "your folder".to_string())
}

fn section_head(title: &str, trailing: Option<AnyElement>, t: Theme) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .h(px(32.))
        .child(
            div()
                .ramp(ty::HEADLINE)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.ink)
                .child(title.to_string()),
        )
        .children(trailing)
}

/// A name on one line, cut with an ellipsis, taking the room that is left.
fn file_name(name: &str, t: Theme) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .ramp(ty::BODY)
        .text_color(t.ink)
        .child(name.to_string())
}

/// A title and a line under it that take the room that is left and cut long text.
fn heading_lines(title: &str, sub: &str, one_line: bool, t: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .child(
            div()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .ramp(ty::BODY)
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.ink)
                .child(title.to_string()),
        )
        .child(
            div()
                .when(one_line, |d| {
                    d.overflow_hidden().whitespace_nowrap().text_ellipsis()
                })
                .ramp(ty::SMALL)
                .text_color(t.ink2)
                .child(sub.to_string()),
        )
}

fn staged_row(i: usize, s: &Staged, t: Theme) -> Stateful<Div> {
    div()
        .id(SharedString::from(format!("share-file-{i}")))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(sp::M))
        .h(px(44.))
        .px(px(sp::L))
        .child(icon(if s.folder { "folder" } else { "file" }, 16., t.ink2))
        .child(file_name(&s.name, t))
        .child(
            div()
                .ramp(ty::SMALL)
                .text_color(t.ink2)
                .child(s.size.map(format_bytes).unwrap_or_default()),
        )
}

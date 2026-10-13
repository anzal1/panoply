//! The command bar view: state, layout and motion.

use crate::model::*;
use crate::native::{self, now_ms, Obj};
use futures::channel::oneshot;
use gpui::prelude::*;
use gpui::*;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// Critically damped spring, ~220 ms settle (SwiftUI spring(response: 0.22, dampingFraction: 1)).
const OMEGA: f32 = 2.0 * std::f32::consts::PI / 0.22;
const GLIDE: SpringConfig = SpringConfig::new(OMEGA * OMEGA, 2.0 * OMEGA, 1.0);
const SETTLE_EPS: f32 = 0.05;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Phase {
    Hidden,
    Showing,
    Shown,
    Hiding,
}

struct VisAnim {
    showing: bool,
    from: f64,
    duration: f64,
    curve: (f64, f64, f64, f64),
    reduce: bool,
    start: Option<Instant>,
}

/// CSS-style cubic bezier easing.
fn bezier(c: (f64, f64, f64, f64), u: f64) -> f64 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let bx = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * c.0 + 3.0 * (1.0 - t) * t * t * c.2 + t * t * t;
    let by = |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * c.1 + 3.0 * (1.0 - t) * t * t * c.3 + t * t * t;
    let (mut lo, mut hi, mut t) = (0.0, 1.0, u);
    for _ in 0..24 {
        t = (lo + hi) / 2.0;
        if bx(t) < u {
            lo = t
        } else {
            hi = t
        }
    }
    by(t)
}

const SHOW_CURVE: (f64, f64, f64, f64) = (0.2, 0.9, 0.25, 1.0);
const HIDE_CURVE: (f64, f64, f64, f64) = (0.42, 0.0, 1.0, 1.0);
const REDUCE_CURVE: (f64, f64, f64, f64) = (0.25, 0.1, 0.25, 1.0);

fn col(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    Rgba { r, g, b, a }.into()
}
fn rgb8(c: (u8, u8, u8), a: f32) -> Hsla {
    col(c.0 as f32 / 255.0, c.1 as f32 / 255.0, c.2 as f32 / 255.0, a)
}

#[derive(Clone, Copy)]
struct Theme {
    dark: bool,
    ink: Hsla,
    ink2: Hsla,
    ink3: Hsla,
    fill8: Hsla,
    fill9: Hsla,
    hair: Hsla,
    base: Hsla,
}

impl Theme {
    fn new(dark: bool) -> Self {
        let w = if dark { 1.0 } else { 0.0 };
        let ink = |a: f32| col(w, w, w, a);
        Self {
            dark,
            ink: ink(0.85),
            ink2: ink(0.85 * 0.6),
            ink3: ink(0.85 * 0.4),
            fill8: ink(0.85 * 0.08),
            fill9: ink(0.85 * 0.09),
            hair: if dark { col(1.0, 1.0, 1.0, 0.12) } else { col(0.0, 0.0, 0.0, 0.08) },
            // Tint over the blurred backdrop so text keeps contrast; the blur itself comes from the window.
            base: if dark { col(0.11, 0.11, 0.125, 0.62) } else { col(0.965, 0.965, 0.975, 0.66) },
        }
    }
}

pub struct Bar {
    model: Rc<Model>,
    focus: FocusHandle,
    win: Option<Obj>,
    metrics: Arc<Mutex<Vec<f64>>>,

    query: String,
    cursor: usize, // char index
    file: Option<DroppedFile>,
    list: ItemList,
    selected: usize,
    glide: bool,

    pill: SpringState,
    pill_target: f32,
    scroll: SpringState,
    scroll_target: f32,
    last_step: Option<Instant>,
    was_moving: bool,

    flash_at: Option<Instant>,
    footer_msg: Option<String>,
    msg_token: u64,
    actions_pressed: bool,
    drag_ext: Option<String>,
    debug_drag: bool,

    pub phase: Phase,
    vis: f64,
    anim: Option<VisAnim>,
    pub suppress_autohide: bool,

    launch_pending: bool,
    pending_show: Option<f64>,
    pub pending_mark: Option<oneshot::Sender<f64>>,
}

impl Bar {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, model: Rc<Model>, metrics: Arc<Mutex<Vec<f64>>>) -> Self {
        let focus = cx.focus_handle();
        let win = native::ns_window(window);
        if let Some(w) = win {
            native::setup_panel(w, RADIUS as f64);
            native::set_alpha(w, 0.0);
            native::set_ignores_mouse(w, true);
            native::position(w, WIDTH as f64, HEIGHT as f64, 0.22);
            native::order_front_regardless(w);
        }
        window.focus(&focus, cx);
        cx.observe_window_activation(window, |bar, window, cx| {
            if !window.is_window_active() && matches!(bar.phase, Phase::Showing | Phase::Shown) && !bar.suppress_autohide {
                bar.hide(window, cx);
            }
        })
        .detach();
        let entity = cx.entity();
        window
            .observe_window_appearance(move |_, cx| {
                entity.update(cx, |_, cx| cx.notify());
            })
            .detach();

        let list = model.build("", None);
        let mut bar = Self {
            model,
            focus,
            win,
            metrics,
            query: String::new(),
            cursor: 0,
            file: None,
            list,
            selected: 0,
            glide: false,
            pill: SpringState::default(),
            pill_target: 0.0,
            scroll: SpringState::default(),
            scroll_target: 0.0,
            last_step: None,
            was_moving: false,
            flash_at: None,
            footer_msg: None,
            msg_token: 0,
            actions_pressed: false,
            drag_ext: None,
            debug_drag: false,
            phase: Phase::Hidden,
            vis: 0.0,
            anim: None,
            suppress_autohide: false,
            launch_pending: true,
            pending_show: None,
            pending_mark: None,
        };
        bar.snap_to_selection();
        bar
    }

    // ---- state ----

    fn snap_to_selection(&mut self) {
        self.pill_target = self.list.row_ys.get(self.selected).copied().unwrap_or(0.0);
        self.pill = SpringState { position: self.pill_target, velocity: 0.0 };
        self.scroll_target = 0.0;
        self.scroll = SpringState::default();
    }

    fn rebuild(&mut self) {
        self.glide = false;
        self.list = self.model.build(&self.query, self.file.as_ref().map(|f| f.ext.as_str()));
        self.selected = 0;
        self.snap_to_selection();
    }

    pub fn set_query(&mut self, q: &str, cx: &mut Context<Self>) {
        if q == self.query {
            cx.notify();
            return;
        }
        self.query = q.to_string();
        self.cursor = q.chars().count();
        self.rebuild();
        cx.notify();
    }

    fn set_file(&mut self, f: Option<DroppedFile>, cx: &mut Context<Self>) {
        self.file = f;
        self.rebuild();
        cx.notify();
    }

    fn reset(&mut self) {
        self.query.clear();
        self.cursor = 0;
        self.file = None;
        self.footer_msg = None;
        self.flash_at = None;
        self.drag_ext = None;
        self.rebuild();
    }

    fn move_sel(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.list.rows.is_empty() {
            return;
        }
        let n = (self.selected as i32 + delta).clamp(0, self.list.rows.len() as i32 - 1) as usize;
        if n == self.selected {
            return;
        }
        self.select(n, cx);
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.list.rows.len() || i == self.selected {
            return;
        }
        self.glide = true;
        self.selected = i;
        self.pill_target = self.list.row_ys[i];
        self.reveal(i);
        cx.notify();
    }

    /// Keep the selected row visible with a small margin, using the same spring.
    fn reveal(&mut self, sel: usize) {
        let margin = 8.0;
        let max_y = (self.list.content_height - VIEWPORT).max(0.0);
        let mut top = self.list.row_ys[sel];
        if self.list.row_after_header[sel] {
            top -= HEADER;
        }
        let bottom = self.list.row_ys[sel] + ROW;
        let mut t = self.scroll_target;
        if sel == 0 {
            t = 0.0;
        } else if top - margin < t {
            t = top - margin;
        } else if bottom + margin > t + VIEWPORT {
            t = bottom + margin - VIEWPORT;
        }
        self.scroll_target = t.clamp(0.0, max_y);
    }

    fn run(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.list.rows.get(self.selected) else { return };
        let name = self.model.tools[row.tool].name.clone();
        self.flash_at = Some(Instant::now());
        self.msg_token += 1;
        let token = self.msg_token;
        self.footer_msg = Some(format!("Opened {name}"));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1200)).await;
            let _ = this.update(cx, |bar, cx| {
                if bar.msg_token == token {
                    bar.footer_msg = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn press_actions(&mut self, cx: &mut Context<Self>) {
        self.actions_pressed = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(140)).await;
            let _ = this.update(cx, |bar, cx| {
                bar.actions_pressed = false;
                cx.notify();
            });
        })
        .detach();
    }

    // ---- text editing ----

    fn byte_at(&self, char_ix: usize) -> usize {
        self.query.char_indices().nth(char_ix).map(|(b, _)| b).unwrap_or(self.query.len())
    }

    fn edit(&mut self, new_query: String, new_cursor: usize, cx: &mut Context<Self>) {
        if new_query != self.query {
            self.query = new_query;
            self.rebuild();
        }
        self.cursor = new_cursor;
        cx.notify();
    }

    fn insert(&mut self, s: &str, cx: &mut Context<Self>) {
        let clean: String = s.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        let b = self.byte_at(self.cursor);
        let mut q = self.query.clone();
        q.insert_str(b, &clean);
        let c = self.cursor + clean.chars().count();
        self.edit(q, c, cx);
    }

    fn word_start(&self, from: usize) -> usize {
        let chars: Vec<char> = self.query.chars().collect();
        let mut i = from;
        while i > 0 && chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = ev.keystroke.key.as_str();
        let m = ev.keystroke.modifiers;
        let len = self.query.chars().count();
        match k {
            "escape" => {
                if !self.query.is_empty() {
                    self.set_query("", cx);
                } else {
                    self.hide(window, cx);
                }
            }
            "up" => self.move_sel(-1, cx),
            "down" => self.move_sel(1, cx),
            "n" if m.control => self.move_sel(1, cx),
            "p" if m.control => self.move_sel(-1, cx),
            "enter" => self.run(cx),
            "backspace" => {
                if self.cursor == 0 {
                    return;
                }
                let start = if m.platform { 0 } else if m.alt { self.word_start(self.cursor) } else { self.cursor - 1 };
                let (a, b) = (self.byte_at(start), self.byte_at(self.cursor));
                let mut q = self.query.clone();
                q.replace_range(a..b, "");
                self.edit(q, start, cx);
            }
            "delete" => {
                if self.cursor >= len {
                    return;
                }
                let (a, b) = (self.byte_at(self.cursor), self.byte_at(self.cursor + 1));
                let mut q = self.query.clone();
                q.replace_range(a..b, "");
                let c = self.cursor;
                self.edit(q, c, cx);
            }
            "left" => {
                self.cursor = if m.platform { 0 } else if m.alt { self.word_start(self.cursor) } else { self.cursor.saturating_sub(1) };
                cx.notify();
            }
            "right" => {
                self.cursor = if m.platform { len } else { (self.cursor + 1).min(len) };
                cx.notify();
            }
            "home" => {
                self.cursor = 0;
                cx.notify();
            }
            "end" => {
                self.cursor = len;
                cx.notify();
            }
            "k" if m.platform => self.press_actions(cx),
            "v" if m.platform => {
                if let Some(t) = cx.read_from_clipboard().and_then(|c| c.text()) {
                    self.insert(&t.replace('\n', " "), cx);
                }
            }
            _ => {
                if !m.platform && !m.control {
                    if let Some(ch) = &ev.keystroke.key_char {
                        self.insert(ch, cx);
                    }
                }
            }
        }
    }

    // ---- show / hide ----

    pub fn is_visible_phase(&self) -> bool {
        matches!(self.phase, Phase::Showing | Phase::Shown)
    }

    pub fn toggle(&mut self, t0: f64, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_visible_phase() {
            self.hide(window, cx);
        } else {
            self.show(Some(t0), window, cx);
        }
    }

    pub fn show(&mut self, t0: Option<f64>, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.phase, Phase::Hidden | Phase::Hiding) {
            return;
        }
        let Some(w) = self.win else { return };
        let reduce = native::reduce_motion();
        let from = if self.phase == Phase::Hiding { self.vis } else { 0.0 };
        if self.phase == Phase::Hidden {
            native::position(w, WIDTH as f64, HEIGHT as f64, 0.22);
            self.apply_vis(0.0, true, reduce);
        }
        self.phase = Phase::Showing;
        if keep_warm() {
            native::set_ignores_mouse(w, false);
        }
        native::order_front_key(w);
        window.focus(&self.focus, cx);
        self.anim = Some(VisAnim {
            showing: true,
            from,
            duration: if reduce { 0.12 } else { 0.18 },
            curve: if reduce { REDUCE_CURVE } else { SHOW_CURVE },
            reduce,
            start: None,
        });
        self.pending_show = t0;
        cx.notify();
        self.schedule_vis(window, cx);
    }

    pub fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible_phase() {
            return;
        }
        let reduce = native::reduce_motion();
        self.phase = Phase::Hiding;
        self.anim = Some(VisAnim {
            showing: false,
            from: self.vis,
            duration: if reduce { 0.1 } else { 0.12 },
            curve: HIDE_CURVE,
            reduce,
            start: None,
        });
        self.schedule_vis(window, cx);
    }

    fn apply_vis(&mut self, vis: f64, showing: bool, reduce: bool) {
        self.vis = vis;
        let Some(w) = self.win else { return };
        native::set_alpha(w, vis);
        if reduce {
            native::set_motion(w, 1.0, 0.0);
        } else {
            let scale = if showing { 0.985 + 0.015 * vis } else { 0.99 + 0.01 * vis };
            let dy = if showing { 6.0 * (1.0 - vis) } else { 0.0 };
            native::set_motion(w, scale, dy);
        }
    }

    fn schedule_vis(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        window.on_next_frame(move |window, cx| {
            this.update(cx, |bar, cx| bar.vis_step(window, cx));
        });
    }

    /// One visibility animation frame (runs at the start of a GPUI frame request).
    fn vis_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = self.anim.as_mut() else { return };
        let now = Instant::now();
        let start = *a.start.get_or_insert(now);
        let u = (now.duration_since(start).as_secs_f64() / a.duration).min(1.0);
        let e = bezier(a.curve, u);
        let (showing, from, reduce) = (a.showing, a.from, a.reduce);
        let vis = if showing { from + (1.0 - from) * e } else { from * (1.0 - e) };
        self.apply_vis(vis, showing, reduce);
        if u < 1.0 {
            self.schedule_vis(window, cx);
            return;
        }
        self.anim = None;
        if showing {
            self.phase = Phase::Shown;
            if let Some(w) = self.win {
                native::invalidate_shadow(w);
            }
        } else {
            if let Some(w) = self.win {
                if keep_warm() {
                    // Experiment: stay on screen fully transparent so GPUI's display link never stops.
                    native::set_ignores_mouse(w, true);
                } else {
                    native::order_out(w);
                }
            }
            self.phase = Phase::Hidden;
            self.reset();
        }
    }

    /// Debug hook for offscreen snapshots: put the bar into a given state.
    pub fn debug_state(&mut self, q: &str, sel: usize, file: Option<DroppedFile>, drag: Option<String>, cx: &mut Context<Self>) {
        self.file = file;
        self.query = q.to_string();
        self.cursor = q.chars().count();
        self.rebuild();
        if sel > 0 {
            self.select(sel, cx);
            self.scroll = SpringState { position: self.scroll_target, velocity: 0.0 };
            self.pill = SpringState { position: self.pill_target, velocity: 0.0 };
        }
        self.debug_drag = drag.is_some();
        self.drag_ext = drag;
        cx.notify();
    }

    pub fn state_json(&self) -> String {
        let (visible, key, fr, alpha, level, mask, blur) = match self.win {
            Some(w) => (native::is_visible(w), native::is_key(w), native::frame(w), native::alpha(w), native::level(w), native::style_mask(w), native::blur_view_count(w)),
            None => (false, false, Default::default(), 0.0, 0, 0, 0),
        };
        format!(
            "{{\"phase\":\"{:?}\",\"visible\":{},\"key\":{},\"alpha\":{:.3},\"level\":{},\"style_mask\":{},\"blur_views\":{},\"frame\":[{},{},{},{}],\"query\":{:?},\"rows\":{}}}",
            self.phase, visible, key, alpha, level, mask, blur, fr.o.x, fr.o.y, fr.s.w, fr.s.h, self.query, self.list.rows.len()
        )
    }

    // ---- per-frame ----

    fn step_springs(&mut self) -> bool {
        let now = Instant::now();
        // After an idle gap the first step uses one 120 Hz frame, so a new glide never starts with a jump.
        let dt = match (self.was_moving, self.last_step) {
            (true, Some(t)) => now.duration_since(t).as_secs_f32().clamp(0.0, 0.05),
            _ => 1.0 / 120.0,
        };
        self.last_step = Some(now);
        let mut moving = false;
        if !GLIDE.is_settled(self.pill, self.pill_target, SETTLE_EPS) {
            self.pill = GLIDE.step(self.pill, self.pill_target, dt);
            if GLIDE.is_settled(self.pill, self.pill_target, SETTLE_EPS) {
                self.pill = SpringState { position: self.pill_target, velocity: 0.0 };
            } else {
                moving = true;
            }
        }
        if !GLIDE.is_settled(self.scroll, self.scroll_target, SETTLE_EPS) {
            self.scroll = GLIDE.step(self.scroll, self.scroll_target, dt);
            if GLIDE.is_settled(self.scroll, self.scroll_target, SETTLE_EPS) {
                self.scroll = SpringState { position: self.scroll_target, velocity: 0.0 };
            } else {
                moving = true;
            }
        }
        self.was_moving = moving;
        moving
    }
}

fn keycap(text: &str, pressed: bool, t: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(if text.chars().count() > 1 { 5. } else { 0. }))
        .rounded(px(5.))
        .bg(if pressed { col(if t.dark { 1.0 } else { 0.0 }, if t.dark { 1.0 } else { 0.0 }, if t.dark { 1.0 } else { 0.0 }, 0.85 * 0.16) } else { t.fill8 })
        .text_size(px(11.))
        .line_height(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.ink2)
        .child(text.to_string())
}

fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg().path(format!("icons/{name}.svg")).size(px(size)).flex_none().text_color(color)
}

fn highlighted(name: &str, matched: &[usize]) -> StyledText {
    let mut runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = vec![];
    if !matched.is_empty() {
        let offs: Vec<usize> = name.char_indices().map(|(b, _)| b).chain(std::iter::once(name.len())).collect();
        let mut i = 0;
        while i < matched.len() {
            let mut j = i;
            while j + 1 < matched.len() && matched[j + 1] == matched[j] + 1 {
                j += 1;
            }
            runs.push((offs[matched[i]]..offs[matched[j] + 1], HighlightStyle { font_weight: Some(FontWeight::SEMIBOLD), ..Default::default() }));
            i = j + 1;
        }
    }
    StyledText::new(name.to_string()).with_highlights(runs)
}

/// Experiment switch (ARSENAL_KEEP_WARM=1): hide by going transparent instead of ordering the panel out.
fn keep_warm() -> bool {
    static F: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *F.get_or_init(|| std::env::var("ARSENAL_KEEP_WARM").is_ok_and(|v| v == "1"))
}

/// Dev override for appearance checks (ARSENAL_APPEARANCE=light|dark).
fn forced_dark() -> Option<bool> {
    static F: std::sync::OnceLock<Option<bool>> = std::sync::OnceLock::new();
    *F.get_or_init(|| match std::env::var("ARSENAL_APPEARANCE").ok().as_deref() {
        Some("light") => Some(false),
        Some("dark") => Some(true),
        _ => None,
    })
}

fn snap(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

impl Render for Bar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = forced_dark().unwrap_or_else(|| matches!(window.appearance(), WindowAppearance::Dark | WindowAppearance::VibrantDark));
        let t = Theme::new(dark);

        if self.step_springs() {
            window.request_animation_frame();
        }
        let flash = self.flash_at.map(|s| {
            let e = s.elapsed().as_secs_f32() / 0.15;
            if e >= 1.0 { 0.0 } else { 1.0 - e }
        });
        let flash_k = flash.unwrap_or(0.0);
        if flash_k > 0.0 {
            window.request_animation_frame();
        }

        // Frame-presented probes: the callback runs at the start of the next frame request,
        // which is after this frame has been drawn and presented.
        if let Some(t0) = self.pending_show.take() {
            let m = self.metrics.clone();
            window.on_next_frame(move |_, _| m.lock().unwrap().push(now_ms() - t0));
        }
        if let Some(tx) = self.pending_mark.take() {
            window.on_next_frame(move |_, _| {
                let _ = tx.send(now_ms());
            });
        }
        if self.launch_pending {
            self.launch_pending = false;
            let this = cx.entity();
            window.on_next_frame(move |_, cx| {
                let t = now_ms();
                crate::record_launch(t);
                this.update(cx, |bar, _| {
                    if let Some(w) = bar.win {
                        native::order_out(w);
                        native::set_ignores_mouse(w, false);
                    }
                });
            });
        }

        let model = self.model.clone();
        let scroll = snap(self.scroll.position);
        let pill_y = self.pill.position;

        // ---- search row ----
        let before: String = self.query.chars().take(self.cursor).collect();
        let after: String = self.query.chars().skip(self.cursor).collect();
        // Zero-width anchor so the caret never shifts the text origin (glyphs start at x = 50).
        let caret = div()
            .relative()
            .flex_none()
            .w(px(0.))
            .h(px(24.))
            .child(div().absolute().left(px(-0.75)).top_0().w(px(1.5)).h(px(24.)).rounded(px(0.75)).bg(t.ink));
        let field = div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_row()
            .items_center()
            .text_size(px(20.))
            .line_height(px(24.))
            .whitespace_nowrap()
            .child(div().flex_none().text_color(t.ink).child(before))
            .child(caret)
            .child(div().flex_none().text_color(t.ink).child(after))
            .when(self.query.is_empty(), |d| {
                d.child(div().flex_none().text_color(t.ink3).child(model.placeholder.clone()))
            });

        let chip = self.file.as_ref().map(|f| {
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(6.))
                .h(px(24.))
                .max_w(px(200.))
                .pl(px(10.))
                .pr(px(5.))
                .ml(px(12.))
                .mr(px(16.))
                .rounded(px(8.))
                .bg(t.fill8)
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(f.name.clone()),
                )
                .child(
                    div()
                        .id("clear-file")
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(14.))
                        .rounded_full()
                        .bg(t.fill8)
                        .cursor_pointer()
                        .on_click(cx.listener(|bar, _, _, cx| bar.set_file(None, cx)))
                        .child(icon("x", 8., t.ink2)),
                )
        });

        let search_row = div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(SEARCH_ROW))
            .w_full()
            .pl(px(20.))
            .child(icon("search", 18., t.ink2))
            .child(div().flex_none().w(px(12.)))
            .child(field)
            .when_some(chip, |d, c| d.child(c))
            .when(self.file.is_none(), |d| d.child(div().flex_none().w(px(20.))))
            .child(div().absolute().left_0().right_0().bottom_0().h(px(0.5)).bg(t.fill8));

        // ---- list ----
        let top = scroll - ROW;
        let bottom = scroll + VIEWPORT;
        let mut items: Vec<AnyElement> = vec![];
        for e in &self.list.entries {
            match e {
                Entry::Header { y, title } => {
                    if *y + HEADER < top || *y > bottom {
                        continue;
                    }
                    items.push(
                        div()
                            .absolute()
                            .top(px(*y))
                            .left_0()
                            .w(px(WIDTH))
                            .h(px(HEADER))
                            .pl(px(20.))
                            .pb(px(6.))
                            .flex()
                            .items_end()
                            .text_size(px(11.))
                            .line_height(px(14.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink3)
                            .child(title.clone())
                            .into_any_element(),
                    );
                }
                Entry::Row { y, row } => {
                    if *y + ROW < top || *y > bottom {
                        continue;
                    }
                    let r = &self.list.rows[*row];
                    let tool = &model.tools[r.tool];
                    let tc = tint(&tool.category, dark);
                    let selected = r.ord == self.selected;
                    let ord = r.ord;
                    items.push(
                        div()
                            .id(("row", ord))
                            .absolute()
                            .top(px(*y))
                            .left_0()
                            .w(px(WIDTH))
                            .h(px(ROW))
                            .px(px(20.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |bar, _, _, cx| {
                                    bar.select(ord, cx);
                                    bar.run(cx);
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .size(px(28.))
                                    .rounded(px(7.))
                                    .bg(rgb8(tc, 0.16))
                                    .child(icon(&tool.lucide, 15., rgb8(tc, 1.0))),
                            )
                            .child(div().flex_none().w(px(12.)))
                            .child(
                                div()
                                    .flex_none()
                                    .whitespace_nowrap()
                                    .text_size(px(13.5))
                                    .line_height(px(18.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink)
                                    .child(highlighted(&tool.name, &r.matched)),
                            )
                            .child(div().flex_none().w(px(8.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(t.ink2)
                                    .child(tool.subtitle.clone()),
                            )
                            .child(div().flex_none().w(px(12.)))
                            .child(if selected {
                                keycap("\u{21B5}", false, t).into_any_element()
                            } else {
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .line_height(px(14.))
                                    .text_color(t.ink3)
                                    .child(tool.category.clone())
                                    .into_any_element()
                            })
                            .into_any_element(),
                    );
                }
            }
        }

        let flash_alpha = 0.85 * (0.09 + 0.11 * flash_k);
        let w = if dark { 1.0 } else { 0.0 };
        let pill = div()
            .absolute()
            .left(px(8.))
            .top(px(pill_y))
            .w(px(WIDTH - 16.))
            .h(px(ROW))
            .rounded(px(10.))
            .bg(col(w, w, w, flash_alpha));

        let dragging = cx.has_active_drag() || self.debug_drag;
        let drag_label = if dragging {
            Some(match self.drag_ext.as_deref() {
                Some(e) if !e.is_empty() => format!("Drop to see tools for .{e}"),
                _ => "Drop to see tools for this file".to_string(),
            })
        } else {
            None
        };

        let trimmed = self.query.trim().to_string();
        let list_body: AnyElement = if self.list.rows.is_empty() {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(6.))
                .pb(px(8.))
                .child(div().text_size(px(13.)).text_color(t.ink2).child(if trimmed.is_empty() {
                    "No tools".to_string()
                } else {
                    format!("No tools match \u{201C}{trimmed}\u{201D}")
                }))
                .child(div().text_size(px(12.)).text_color(t.ink3).child("Try \u{201C}compress\u{201D} or drop a file"))
                .into_any_element()
        } else {
            div()
                .absolute()
                .top(px(-scroll))
                .left_0()
                .w(px(WIDTH))
                .h(px(self.list.content_height))
                .child(pill)
                .children(items)
                .into_any_element()
        };

        let list = div()
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|bar, ev: &ScrollWheelEvent, _, cx| {
                let dy = ev.delta.pixel_delta(px(20.)).y.as_f32();
                let max_y = (bar.list.content_height - VIEWPORT).max(0.0);
                let p = (bar.scroll.position - dy).clamp(0.0, max_y);
                bar.scroll = SpringState { position: p, velocity: 0.0 };
                bar.scroll_target = p;
                cx.notify();
            }))
            .child(
                div()
                    .size_full()
                    .opacity(if dragging { 0.18 } else { 1.0 })
                    .child(list_body),
            )
            .when_some(drag_label, |d, label| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink2)
                        .child(label),
                )
            });

        // ---- footer ----
        let footer = div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(FOOTER))
            .w_full()
            .px(px(20.))
            .child(div().absolute().left_0().right_0().top_0().h(px(0.5)).bg(t.fill8))
            .child(
                div()
                    .flex_none()
                    .size(px(16.))
                    .rounded(px(4.5))
                    .bg(linear_gradient(180., linear_color_stop(col(w, w, w, 0.85 * 0.85), 0.), linear_color_stop(col(w, w, w, 0.85 * 0.55), 1.))),
            )
            .child(div().flex_none().w(px(8.)))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(t.ink2)
                    .child(self.footer_msg.clone().unwrap_or_else(|| "Arsenal".to_string())),
            )
            .child(div().flex_1().min_w(px(16.)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_size(px(12.)).line_height(px(16.)).text_color(t.ink2).child("Open"))
                            .child(keycap("\u{21B5}", false, t)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(div().text_size(px(12.)).line_height(px(16.)).text_color(t.ink2).child("Actions"))
                            .child(
                                div()
                                    .flex()
                                    .gap(px(4.))
                                    .child(keycap("\u{2318}", self.actions_pressed, t))
                                    .child(keycap("K", self.actions_pressed, t)),
                            ),
                    ),
            );

        let (ar, ag, ab) = if dragging { native::accent_rgb() } else { (0.0, 0.478, 1.0) };

        div()
            .id("bar")
            .track_focus(&self.focus)
            .key_context("Bar")
            .on_key_down(cx.listener(|bar, ev: &KeyDownEvent, window, cx| bar.on_key(ev, window, cx)))
            .on_drag_move::<ExternalPaths>(cx.listener(|bar, ev: &DragMoveEvent<ExternalPaths>, _, cx| {
                let ext = ev.drag(cx).paths().first().map(|p| DroppedFile::from_path(p).ext);
                if bar.drag_ext != ext {
                    bar.drag_ext = ext;
                    cx.notify();
                }
            }))
            .on_drop::<ExternalPaths>(cx.listener(|bar, paths: &ExternalPaths, _, cx| {
                bar.drag_ext = None;
                if let Some(p) = paths.paths().first() {
                    bar.set_file(Some(DroppedFile::from_path(p)), cx);
                }
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(RADIUS))
            .bg(t.base)
            .text_color(t.ink)
            .child(search_row)
            .child(list)
            .child(footer)
            // 0.5 pt inner hairline
            .child(div().absolute().inset_0().rounded(px(RADIUS)).border(px(0.5)).border_color(t.hair))
            // drag ring: 2 pt accent at 40%
            .when(dragging, |d| {
                d.child(div().absolute().inset_0().rounded(px(RADIUS)).border(px(2.)).border_color(col(ar, ag, ab, 0.4)))
            })
    }
}

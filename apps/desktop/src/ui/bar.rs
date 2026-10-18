//! The command bar: a 720 by 460 translucent panel, created hidden at start and shown by the
//! global hotkey. Search, file drop, ranked tools, plain-words requests and the conversion router
//! all come from `crate::search`; this file only draws rows and moves the selection.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::*;
use gpui_base::input::{Input as KitInput, InputEvent, InputState};

use super::panel::{PanelEvent, ToolPanel};
use super::theme::{HIT, Theme, is_dark, radius, sp, ty};
use super::widgets::{
    Ramp, brand_mark, category_tile, chip, cmd, editor_style, file_category, file_icon,
    focus_border, icon, keycap, snap,
};
use super::{Hub, Recents};
use crate::finder::{self, FileHit};
use crate::platform::{self, Panel};
use crate::profile::{FileProfile, expand_paths, files_label};
use crate::search::{self, Action, Results, Row};
use crate::units::{format_bytes, now_secs};

pub(crate) const WIDTH: f32 = 720.0;
pub(crate) const HEIGHT: f32 = 460.0;
const RADIUS: f32 = radius::BAR;
const SEARCH_ROW: f32 = 56.0;
const FOOTER: f32 = 40.0;
const LIST_TOP: f32 = 8.0;
const HEADER: f32 = 28.0;
const ROW: f32 = 44.0;
const CHIP_ROW: f32 = 40.0;
const VIEWPORT: f32 = HEIGHT - SEARCH_ROW - FOOTER;
/// The footer label for showing a file in the file manager. A constant, so the key handler can
/// match on it.
const SHOW_FILE: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else if cfg!(windows) {
    "Show in Explorer"
} else {
    "Show in folder"
};

// Critically damped spring, about 220 ms to settle.
const OMEGA: f32 = 2.0 * std::f32::consts::PI / 0.22;
const GLIDE: SpringConfig = SpringConfig::new(OMEGA * OMEGA, 2.0 * OMEGA, 1.0);
const SETTLE_EPS: f32 = 0.05;

/// Where the bar is in its show and hide cycle.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Phase {
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

/// Files are looked up for a typed query of two letters or more, when no files are dropped on the
/// bar and the words were not read as a request ("make this under 50 kb"). A calculator answer
/// does not stop it: "2024-10" may be a calculation or the name of a file.
fn wants_file_search(no_files_on_bar: bool, query: &str, understood: bool) -> bool {
    no_files_on_bar && query.trim().chars().count() >= 2 && !understood
}

/// CSS-style cubic bezier easing.
fn bezier(c: (f64, f64, f64, f64), u: f64) -> f64 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return 1.0;
    }
    let bx =
        |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * c.0 + 3.0 * (1.0 - t) * t * t * c.2 + t * t * t;
    let by =
        |t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * c.1 + 3.0 * (1.0 - t) * t * t * c.3 + t * t * t;
    let (mut lo, mut hi, mut t) = (0.0, 1.0, u);
    for _ in 0..24 {
        t = (lo + hi) / 2.0;
        if bx(t) < u { lo = t } else { hi = t }
    }
    by(t)
}

const SHOW_CURVE: (f64, f64, f64, f64) = (0.2, 0.9, 0.25, 1.0);
const HIDE_CURVE: (f64, f64, f64, f64) = (0.42, 0.0, 1.0, 1.0);
const REDUCE_CURVE: (f64, f64, f64, f64) = (0.25, 0.1, 0.25, 1.0);

/// One drawn item of the list.
#[derive(Debug, Clone)]
enum Entry {
    Header { y: f32, title: String },
    Row { y: f32, h: f32, row: usize },
}

#[derive(Debug, Default)]
struct ListLayout {
    entries: Vec<Entry>,
    row_ys: Vec<f32>,
    row_hs: Vec<f32>,
    row_after_header: Vec<bool>,
    content_height: f32,
}

fn layout(results: &Results) -> ListLayout {
    let mut l = ListLayout::default();
    let mut y = LIST_TOP;
    let mut ord = 0;
    for s in &results.sections {
        l.entries.push(Entry::Header {
            y,
            title: s.title.clone(),
        });
        y += HEADER;
        for (k, r) in s.rows.iter().enumerate() {
            let h = if r.chips.is_empty() {
                ROW
            } else {
                ROW + CHIP_ROW
            };
            l.entries.push(Entry::Row { y, h, row: ord });
            l.row_ys.push(y);
            l.row_hs.push(h);
            l.row_after_header.push(k == 0);
            y += h;
            ord += 1;
        }
    }
    l.content_height = y + LIST_TOP;
    l
}

/// How the bar is created.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BarMode {
    /// Normal: shown by the hotkey.
    Live,
    /// Dev and tests: lives at x = -20000, never ordered front, never key.
    Offscreen,
}

pub(crate) struct Bar {
    hub: Hub,
    mode: BarMode,
    focus: FocusHandle,
    win: Option<Panel>,

    pub(crate) query: String,
    input: Entity<InputState>,
    _subs: Vec<Subscription>,
    pub(crate) files: Vec<FileProfile>,
    pub(crate) panel: Option<Entity<ToolPanel>>,
    _panel_sub: Option<Subscription>,
    pub(crate) results: Results,
    /// Files from the index shown under the tools (at most [`finder::BAR_FILES`]).
    file_hits: Vec<FileHit>,
    /// Bumped for every query; a file answer for an older number is dropped.
    file_gen: u64,
    /// True from a keystroke until its file answer arrives.
    files_pending: bool,
    typing: search::Typing,
    list: ListLayout,
    pub(crate) selected: usize,
    glide: bool,
    pill: SpringState,
    pill_target: f32,
    scroll: SpringState,
    scroll_target: f32,
    last_step: Option<Instant>,
    was_moving: bool,
    flash_at: Option<Instant>,
    pub(crate) footer_msg: Option<String>,
    msg_token: u64,
    drag_active: bool,

    pub(crate) phase: Phase,
    vis: f64,
    anim: Option<VisAnim>,
    pub(crate) suppress_autohide: bool,
    launch_pending: bool,
    /// Linux/X11: the window that had keyboard focus before the bar took it.
    #[cfg(target_os = "linux")]
    prev_focus: Option<u32>,
    /// Linux/X11: when losing focus last started a hide. See [`Bar::toggle`].
    #[cfg(target_os = "linux")]
    deactivated_at: Option<Instant>,
}

impl Bar {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        hub: Hub,
        mode: BarMode,
    ) -> Self {
        let focus = cx.focus_handle();
        let win = Panel::of(window);
        if let Some(w) = win {
            w.setup_bar(RADIUS as f64);
            w.set_alpha(0.0);
            w.set_ignores_mouse(true);
            if mode == BarMode::Offscreen {
                w.set_origin(-20000.0, 400.0);
            } else {
                w.position(WIDTH as f64, HEIGHT as f64, 0.22);
                w.order_front_regardless();
            }
        }
        let placeholder = format!(
            "Search {} tools, or drop anything",
            hub.catalog.tools().len()
        );
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let input_sub = cx.subscribe_in(
            &input,
            window,
            |bar, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    let v = input.read(cx).value().to_string();
                    if v != bar.query {
                        bar.query = v;
                        bar.rebuild(cx);
                        cx.notify();
                    }
                }
                // Return on a file is handled by the key handler (Return opens, Command+Return
                // shows it), which also sees this key; one of the two must not act.
                InputEvent::PressEnter { .. } if bar.selected_is_file() => {}
                InputEvent::PressEnter { .. } => bar.activate(window, cx),
                _ => {}
            },
        );
        input.update(cx, |s, cx| s.focus(window, cx));
        let hide_sub = cx.observe_window_activation(window, |bar, window, cx| {
            if window.is_window_active() && bar.is_visible_phase() {
                // Windows and X11 activate the window after `show` has run, and the activation can
                // reset which element holds the keyboard, so put it on the search box again.
                bar.focus_input(window, cx);
            }
            if !window.is_window_active()
                && matches!(bar.phase, Phase::Showing | Phase::Shown)
                && !bar.suppress_autohide
            {
                #[cfg(target_os = "linux")]
                {
                    bar.deactivated_at = Some(Instant::now());
                }
                bar.hide(window, cx);
            }
        });
        let entity = cx.entity();
        let appearance_sub = window.observe_window_appearance(move |_, cx| {
            entity.update(cx, |_, cx| cx.notify());
        });
        let recents_sub = cx.observe(&hub.recents, |bar, _, cx| {
            if bar.query.is_empty() && bar.files.is_empty() {
                bar.rebuild(cx);
                cx.notify();
            }
        });
        let mut bar = Self {
            hub,
            mode,
            focus,
            win,
            query: String::new(),
            input,
            _subs: vec![input_sub, hide_sub, appearance_sub, recents_sub],
            files: Vec::new(),
            panel: None,
            _panel_sub: None,
            results: Results::default(),
            file_hits: Vec::new(),
            file_gen: 0,
            files_pending: false,
            typing: search::Typing::default(),
            list: ListLayout::default(),
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
            drag_active: false,
            phase: Phase::Hidden,
            vis: 0.0,
            anim: None,
            suppress_autohide: false,
            launch_pending: mode == BarMode::Live,
            #[cfg(target_os = "linux")]
            prev_focus: None,
            #[cfg(target_os = "linux")]
            deactivated_at: None,
        };
        bar.rebuild(cx);
        bar
    }

    // ---- state ----

    fn recents(&self, cx: &App) -> Vec<crate::history::Entry> {
        let r: &Recents = self.hub.recents.read(cx);
        r.entries.clone()
    }

    fn rebuild(&mut self, cx: &mut Context<Self>) {
        let history = self.recents(cx);
        self.glide = false;
        self.results = self.typing.search(
            &search::Context {
                catalog: &self.hub.catalog,
                files: &self.files,
                history: &history,
                now: now_secs(),
            },
            &self.query,
        );
        // Tools are in place and drawn from this call on. Files come after them: the ones from the
        // last keystroke stay until the index answers, so the list does not blink.
        let wanted = self.wants_files();
        if !wanted {
            self.file_hits.clear();
        }
        finder::append_files(&mut self.results, &self.file_hits, now_secs());
        self.list = layout(&self.results);
        self.selected = 0;
        self.snap_to_selection();
        self.search_files(wanted, cx);
    }

    /// Files are looked up for a typed query of two letters or more, when no files are dropped on
    /// the bar and the words were not read as a request ("make this under 50 kb").
    fn wants_files(&self) -> bool {
        wants_file_search(
            self.files.is_empty(),
            &self.query,
            self.results.understood.is_some(),
        )
    }

    /// Ask the index for files on a worker and put them under the tools when it answers. The tool
    /// rows never wait for this.
    fn search_files(&mut self, wanted: bool, cx: &mut Context<Self>) {
        self.file_gen += 1;
        let ready = matches!(self.hub.finder.state(), finder::State::Ready { .. });
        if !wanted || !ready {
            self.files_pending = false;
            return;
        }
        self.files_pending = true;
        let generation = self.file_gen;
        let service = self.hub.finder.clone();
        let q = self.query.trim().to_string();
        cx.spawn(async move |this, cx| {
            let hits = cx
                .background_executor()
                .spawn(async move { service.search(&q, finder::BAR_FILES) })
                .await;
            let _ = this.update(cx, |bar, cx| {
                if bar.file_gen != generation {
                    return;
                }
                bar.files_pending = false;
                bar.file_hits = hits;
                bar.put_files_under_tools(cx);
            });
        })
        .detach();
    }

    /// Replace the Files section in place. The tool rows keep their places and the selection stays
    /// on the row it was on.
    fn put_files_under_tools(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected;
        finder::append_files(&mut self.results, &self.file_hits, now_secs());
        self.list = layout(&self.results);
        self.selected = selected.min(self.results.len().saturating_sub(1));
        self.pill_target = self.list.row_ys.get(self.selected).copied().unwrap_or(0.0);
        self.pill = SpringState {
            position: self.pill_target,
            velocity: 0.0,
        };
        let max_y = (self.list.content_height - VIEWPORT).max(0.0);
        self.scroll_target = self.scroll_target.min(max_y);
        self.scroll = SpringState {
            position: self.scroll_target,
            velocity: 0.0,
        };
        cx.notify();
    }

    /// True when the chosen row is a file from the index.
    fn selected_is_file(&self) -> bool {
        matches!(
            self.results.get(self.selected).map(|r| &r.action),
            Some(Action::OpenFile { .. })
        )
    }

    /// Open the chosen file with its default app, or show it in the file manager. On a worker.
    fn open_selected_file(&mut self, reveal: bool, cx: &mut Context<Self>) {
        let Some(Action::OpenFile { path }) =
            self.results.get(self.selected).map(|r| r.action.clone())
        else {
            return;
        };
        self.flash_at = Some(Instant::now());
        cx.spawn(async move |this, cx| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let r = cx
                .background_executor()
                .spawn(async move {
                    if !path.exists() {
                        return Err("is gone");
                    }
                    let r = if reveal {
                        platform::reveal_path(&path)
                    } else {
                        platform::open_path(&path)
                    };
                    r.map_err(|_| "could not be opened")
                })
                .await;
            match r {
                Ok(()) => {
                    let _ = this.update_in(cx, |bar, window, cx| bar.hide(window, cx));
                }
                Err(why) => {
                    let _ = this.update(cx, |bar, cx| bar.say(format!("{name} {why}."), cx));
                }
            }
        })
        .detach();
    }

    fn snap_to_selection(&mut self) {
        self.pill_target = self.list.row_ys.get(self.selected).copied().unwrap_or(0.0);
        self.pill = SpringState {
            position: self.pill_target,
            velocity: 0.0,
        };
        self.scroll_target = 0.0;
        self.scroll = SpringState::default();
    }

    pub(crate) fn set_query(&mut self, q: &str, window: &mut Window, cx: &mut Context<Self>) {
        if q == self.query {
            cx.notify();
            return;
        }
        self.input
            .update(cx, |s, cx| s.set_value(q.to_string(), window, cx));
        self.query = q.to_string();
        self.rebuild(cx);
        cx.notify();
    }

    /// Put files on the bar (a drop, or a chain from a result card).
    pub(crate) fn set_files(&mut self, files: Vec<FileProfile>, cx: &mut Context<Self>) {
        self.files = files;
        self.rebuild(cx);
        cx.notify();
    }

    /// A drop: read the files on a worker, then show them.
    fn dropped(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let files = cx
                .background_executor()
                .spawn(async move {
                    expand_paths(&paths)
                        .iter()
                        .map(|p| FileProfile::read(p))
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = this.update(cx, |bar, cx| bar.set_files(files, cx));
        })
        .detach();
    }

    fn reset(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |s, cx| s.set_value("", window, cx));
        self.query.clear();
        self.files.clear();
        self.footer_msg = None;
        self.flash_at = None;
        self.rebuild(cx);
    }

    fn move_sel(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.results.len();
        if n == 0 {
            return;
        }
        let to = (self.selected as i32 + delta).clamp(0, n as i32 - 1) as usize;
        self.select(to, cx);
    }

    /// Choose a row and scroll to it (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn select_row(&mut self, i: usize, cx: &mut Context<Self>) {
        self.select(i, cx);
        self.pill = SpringState {
            position: self.pill_target,
            velocity: 0.0,
        };
        self.scroll = SpringState {
            position: self.scroll_target,
            velocity: 0.0,
        };
    }

    /// The titles of the sections now in the list, and whether a file answer is still on its way.
    #[cfg(feature = "snapshot")]
    pub(crate) fn section_titles(&self) -> (Vec<String>, bool) {
        (
            self.results
                .sections
                .iter()
                .map(|s| s.title.clone())
                .collect(),
            self.files_pending,
        )
    }

    fn select(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.results.len() || i == self.selected {
            return;
        }
        self.glide = true;
        self.selected = i;
        self.pill_target = self.list.row_ys[i];
        self.reveal(i);
        cx.notify();
    }

    fn reveal(&mut self, sel: usize) {
        let margin = 8.0;
        let max_y = (self.list.content_height - VIEWPORT).max(0.0);
        let mut top = self.list.row_ys[sel];
        if self.list.row_after_header[sel] {
            top -= HEADER;
        }
        let bottom = self.list.row_ys[sel] + self.list.row_hs[sel];
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

    fn say(&mut self, msg: String, cx: &mut Context<Self>) {
        self.msg_token += 1;
        let token = self.msg_token;
        self.footer_msg = Some(msg);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(2400))
                .await;
            let _ = this.update(cx, |bar, cx| {
                if bar.msg_token == token {
                    bar.footer_msg = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Return: act on the selected row.
    pub(crate) fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_with(true, window, cx);
    }

    /// Open the selected row. With `instant`, a pattern A tool runs at once; Tab and Command+K pass
    /// false to see the options first.
    pub(crate) fn activate_with(
        &mut self,
        instant: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(row) = self.results.get(self.selected).cloned() else {
            return;
        };
        self.flash_at = Some(Instant::now());
        match row.action {
            Action::Unavailable(why) => self.say(why, cx),
            Action::CopyText { text } => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                self.say("Copied".into(), cx);
                self.hide(window, cx);
            }
            Action::OpenFile { .. } => self.open_selected_file(false, cx),
            Action::OpenTool { tool, preset } => {
                let files = self.files.clone();
                self.open_tool(tool, files, preset, instant, window, cx);
            }
            Action::ReRun { entry } => {
                let Some(e) = self.recents(cx).get(entry).cloned() else {
                    return;
                };
                let Some(tool) = self.hub.catalog.index_of(&e.tool_id) else {
                    self.say("That tool is no longer installed.".into(), cx);
                    return;
                };
                let preset: Vec<(String, serde_json::Value)> = e
                    .options
                    .as_object()
                    .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                    .unwrap_or_default();
                let inputs: Vec<PathBuf> =
                    e.inputs.iter().filter(|p| p.exists()).cloned().collect();
                cx.spawn_in(window, async move |this, cx| {
                    let files = cx
                        .background_executor()
                        .spawn(async move {
                            inputs
                                .iter()
                                .map(|p| FileProfile::read(p))
                                .collect::<Vec<_>>()
                        })
                        .await;
                    let _ = this.update_in(cx, |bar, window, cx| {
                        bar.open_tool(tool, files, preset, true, window, cx)
                    });
                })
                .detach();
            }
        }
    }

    /// Open a tool's panel inside the bar.
    pub(crate) fn open_tool(
        &mut self,
        tool: usize,
        files: Vec<FileProfile>,
        preset: Vec<(String, serde_json::Value)>,
        instant: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hub = self.hub.clone();
        let panel = cx.new(|cx| ToolPanel::new(hub, tool, files, preset, window, cx));
        if instant {
            panel.update(cx, |p, cx| p.run_if_instant(window, cx));
        }
        let sub = cx.subscribe_in(
            &panel,
            window,
            |bar, _, ev: &PanelEvent, window, cx| match ev {
                PanelEvent::Close => bar.close_panel(window, cx),
                PanelEvent::Chain(paths) => {
                    bar.close_panel(window, cx);
                    bar.dropped(paths.clone(), cx);
                }
                PanelEvent::Ran => {}
            },
        );
        self.panel = Some(panel);
        self._panel_sub = Some(sub);
        cx.notify();
    }

    /// Back from a panel to the list, with the files still on the bar.
    pub(crate) fn close_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.panel = None;
        self._panel_sub = None;
        let focus = self.input.read(cx).focus_handle(cx);
        window.focus(&focus, cx);
        self.rebuild(cx);
        cx.notify();
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel.is_some() {
            return; // the panel handles its own keys
        }
        let k = ev.keystroke.key.as_str();
        let m = ev.keystroke.modifiers;
        match k {
            "escape" => {
                if !self.query.is_empty() {
                    self.set_query("", window, cx);
                } else if !self.files.is_empty() {
                    self.set_files(Vec::new(), cx);
                } else {
                    self.hide(window, cx);
                }
            }
            "up" => self.move_sel(-1, cx),
            "down" => self.move_sel(1, cx),
            "n" if m.control => self.move_sel(1, cx),
            "p" if m.control => self.move_sel(-1, cx),
            "enter" if self.selected_is_file() => self.open_selected_file(m.secondary(), cx),
            "enter" => self.activate(window, cx),
            "tab" => self.activate_with(false, window, cx),
            "k" if m.secondary() => self.activate_with(false, window, cx),
            _ => {}
        }
    }

    // ---- show and hide ----

    fn is_visible_phase(&self) -> bool {
        matches!(self.phase, Phase::Showing | Phase::Shown)
    }

    pub(crate) fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // X11 reports the hotkey's own key grab to the focused bar as a focus loss, which starts a
        // hide; the hotkey press that caused it then arrives and would turn that hide back into a
        // show. When the hide is that fresh, the press has already had its effect.
        #[cfg(target_os = "linux")]
        if self.phase == Phase::Hiding
            && self
                .deactivated_at
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250))
        {
            return;
        }
        if self.is_visible_phase() {
            self.hide(window, cx);
        } else {
            self.show(window, cx);
        }
    }

    pub(crate) fn show(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.phase, Phase::Hidden | Phase::Hiding) || self.mode == BarMode::Offscreen {
            return;
        }
        let Some(w) = self.win else { return };
        let reduce = platform::reduce_motion();
        let from = if self.phase == Phase::Hiding {
            self.vis
        } else {
            0.0
        };
        if self.phase == Phase::Hidden {
            w.position(WIDTH as f64, HEIGHT as f64, 0.22);
            self.apply_vis(0.0, true, reduce);
        }
        self.phase = Phase::Showing;
        w.set_ignores_mouse(false);
        w.show_key();
        #[cfg(not(target_os = "macos"))]
        window.activate_window();
        // Windows applies the window's opening position on its first activation, which is queued
        // behind this call and would undo the placement above (the bar landed at 200,200). Place
        // it again once that has run.
        #[cfg(windows)]
        cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(40))
                .await;
            w.position(WIDTH as f64, HEIGHT as f64, 0.22);
        })
        .detach();
        #[cfg(target_os = "linux")]
        self.take_x11_focus(window, cx);
        self.focus_input_after_activation(window, cx);
        self.anim = Some(VisAnim {
            showing: true,
            from,
            duration: if reduce { 0.12 } else { 0.18 },
            curve: if reduce { REDUCE_CURVE } else { SHOW_CURVE },
            reduce,
            start: None,
        });
        cx.notify();
        self.schedule_vis(window, cx);
    }

    /// Put the keyboard in the search box. The root handle (`self.focus`) only receives keys that no
    /// input wants, and it holds the focus when the box does not, which is how typing after the hotkey
    /// went nowhere on Windows and Linux.
    fn focus_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel.is_some() {
            return; // the panel owns the keyboard while it is open
        }
        let focus = self.input.read(cx).focus_handle(cx);
        if !focus.is_focused(window) {
            window.focus(&focus, cx);
        }
    }

    /// Focus the search box every time the bar is shown, on every OS: at once, on the next frame, and
    /// once more after the platform's own activation has had time to run (Windows applies it after
    /// `show` returns). Each step is a no-op when the box already has the keyboard.
    fn focus_input_after_activation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_input(window, cx);
        let this = cx.entity();
        window.on_next_frame(move |window, cx| {
            this.update(cx, |bar, cx| {
                if bar.is_visible_phase() {
                    bar.focus_input(window, cx);
                }
            });
        });
        cx.spawn_in(window, async move |this, cx| {
            for ms in [50u64, 150, 400] {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(ms))
                    .await;
                let live = this.update_in(cx, |bar, window, cx| {
                    if bar.is_visible_phase() {
                        bar.focus_input(window, cx);
                    }
                    bar.is_visible_phase()
                });
                if !matches!(live, Ok(true)) {
                    return;
                }
            }
        })
        .detach();
    }

    /// X11 only: the bar's window is not viewable for the first few milliseconds after it is created,
    /// and the server will not focus it until it is, so keep trying for about a second.
    #[cfg(target_os = "linux")]
    fn take_x11_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            for _ in 0..50 {
                let done = this.update_in(cx, |bar, window, cx| {
                    if !bar.is_visible_phase() {
                        return true;
                    }
                    match platform::x11_take_focus(window) {
                        platform::Focus::Taken(prev) => {
                            bar.prev_focus = prev;
                            bar.focus_input(window, cx);
                            true
                        }
                        platform::Focus::Unavailable => true,
                        platform::Focus::NotYet => false,
                    }
                });
                if !matches!(done, Ok(false)) {
                    return;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(20))
                    .await;
            }
            eprintln!("command bar focus: gave up after 50 tries");
        })
        .detach();
    }

    pub(crate) fn hide(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_visible_phase() {
            return;
        }
        let reduce = platform::reduce_motion();
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
        w.set_alpha(vis);
        if reduce {
            w.set_motion(1.0, 0.0);
        } else {
            let scale = if showing {
                0.985 + 0.015 * vis
            } else {
                0.99 + 0.01 * vis
            };
            let dy = if showing { 6.0 * (1.0 - vis) } else { 0.0 };
            w.set_motion(scale, dy);
        }
    }

    fn schedule_vis(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity();
        window.on_next_frame(move |window, cx| {
            this.update(cx, |bar, cx| bar.vis_step(window, cx));
        });
    }

    fn vis_step(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(a) = self.anim.as_mut() else { return };
        let now = Instant::now();
        let start = *a.start.get_or_insert(now);
        let u = (now.duration_since(start).as_secs_f64() / a.duration).min(1.0);
        let e = bezier(a.curve, u);
        let (showing, from, reduce) = (a.showing, a.from, a.reduce);
        let vis = if showing {
            from + (1.0 - from) * e
        } else {
            from * (1.0 - e)
        };
        self.apply_vis(vis, showing, reduce);
        if u < 1.0 {
            self.schedule_vis(window, cx);
            return;
        }
        self.anim = None;
        if showing {
            self.phase = Phase::Shown;
            if let Some(w) = self.win {
                w.invalidate_shadow();
            }
        } else {
            if let Some(w) = self.win {
                w.order_out();
            }
            self.phase = Phase::Hidden;
            self.reset(window, cx);
            #[cfg(target_os = "linux")]
            {
                // X11 never unmaps a pop-up and ignores minimize for it, so on Linux the bar window
                // exists only while it is up: closed here, created again by the next hotkey.
                if let Some(prev) = self.prev_focus.take() {
                    platform::x11_restore_focus(prev);
                }
                window.remove_window();
            }
            #[cfg(not(any(target_os = "macos", target_os = "linux")))]
            window.minimize_window();
        }
    }

    // ---- per frame ----

    fn step_springs(&mut self) -> bool {
        let now = Instant::now();
        let dt = match (self.was_moving, self.last_step) {
            (true, Some(t)) => now.duration_since(t).as_secs_f32().clamp(0.0, 0.05),
            _ => 1.0 / 120.0,
        };
        self.last_step = Some(now);
        let mut moving = false;
        if !GLIDE.is_settled(self.pill, self.pill_target, SETTLE_EPS) {
            self.pill = GLIDE.step(self.pill, self.pill_target, dt);
            if GLIDE.is_settled(self.pill, self.pill_target, SETTLE_EPS) {
                self.pill = SpringState {
                    position: self.pill_target,
                    velocity: 0.0,
                };
            } else {
                moving = true;
            }
        }
        if !GLIDE.is_settled(self.scroll, self.scroll_target, SETTLE_EPS) {
            self.scroll = GLIDE.step(self.scroll, self.scroll_target, dt);
            if GLIDE.is_settled(self.scroll, self.scroll_target, SETTLE_EPS) {
                self.scroll = SpringState {
                    position: self.scroll_target,
                    velocity: 0.0,
                };
            } else {
                moving = true;
            }
        }
        self.was_moving = moving;
        moving
    }

    fn highlighted(name: &str, matched: &[usize]) -> StyledText {
        super::widgets::matched_text(name, matched)
    }

    fn render_row(
        &self,
        r: &Row,
        ord: usize,
        y: f32,
        h: f32,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = ord == self.selected;
        let head = div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(ROW))
            .w_full()
            .child(category_tile(
                r.category,
                r.icon,
                28.,
                t,
                t.palette.bar_over_mid(),
            ))
            .child(div().flex_none().w(px(sp::M)))
            .child(
                div()
                    .flex_none()
                    .whitespace_nowrap()
                    .ramp(ty::BODY)
                    .font_weight(if matches!(r.action, Action::CopyText { .. }) {
                        // The answer reads at the weight of a matched tool name.
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(t.ink)
                    .child(Self::highlighted(&r.title, &r.matched)),
            )
            .child(div().flex_none().w(px(sp::S)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ramp(ty::SMALL)
                    .text_color(t.ink2)
                    .child(r.subtitle.clone()),
            )
            .child(div().flex_none().w(px(sp::M)))
            .child(if selected {
                keycap("\u{21B5}", t).into_any_element()
            } else {
                div()
                    .flex_none()
                    .ramp(ty::CAPTION)
                    .text_color(t.ink3)
                    .child(r.trailing.clone())
                    .into_any_element()
            });
        let chips = (!r.chips.is_empty()).then(|| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .flex_none()
                .h(px(CHIP_ROW))
                .w_full()
                .pl(px(28. + sp::M))
                .pb(px(sp::S))
                .gap(px(sp::S))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .gap(px(sp::S))
                        .children(
                            r.chips
                                .iter()
                                .map(|(l, v)| chip(l, v, t).into_any_element()),
                        ),
                )
                .when_some(r.estimate.clone(), |d, e| {
                    d.child(
                        div()
                            .flex_none()
                            .ramp(ty::SMALL)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.good)
                            .child(e),
                    )
                })
        });
        div()
            .id(("row", ord))
            .role(Role::ListBoxOption)
            .aria_label(r.title.clone())
            .aria_description(r.subtitle.clone())
            .aria_selected(selected)
            .aria_position_in_set(ord + 1)
            .aria_size_of_set(self.results.len())
            .on_a11y_action(AccessibleAction::Click, {
                let this = cx.entity().downgrade();
                move |_, window, cx| {
                    this.update(cx, |bar, cx| {
                        bar.select(ord, cx);
                        bar.activate(window, cx);
                    })
                    .ok();
                }
            })
            .absolute()
            .top(px(y))
            .left_0()
            .w(px(WIDTH))
            .h(px(h))
            .px(px(20.))
            .flex()
            .flex_col()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |bar, _, window, cx| {
                    bar.select(ord, cx);
                    bar.activate(window, cx);
                }),
            )
            .child(head)
            .children(chips)
            .into_any_element()
    }
}

impl Render for Bar {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(is_dark(window));
        let reduce = platform::reduce_motion();
        if self.step_springs() {
            if reduce {
                // Reduce Motion: jump instead of gliding.
                self.pill = SpringState {
                    position: self.pill_target,
                    velocity: 0.0,
                };
                self.scroll = SpringState {
                    position: self.scroll_target,
                    velocity: 0.0,
                };
                self.was_moving = false;
            } else {
                window.request_animation_frame();
            }
        }
        let flash_k = self.flash_at.map_or(0.0, |s| {
            let e = s.elapsed().as_secs_f32() / 0.15;
            if e >= 1.0 { 0.0 } else { 1.0 - e }
        });
        if flash_k > 0.0 && !reduce {
            window.request_animation_frame();
        }
        if self.launch_pending {
            // Normal builds: the panel was ordered front invisibly so GPUI renders a first frame,
            // then it goes away. Nothing is ever visible (alpha 0).
            self.launch_pending = false;
            let this = cx.entity();
            window.on_next_frame(move |_, cx| {
                this.update(cx, |bar, _| {
                    if let Some(w) = bar.win {
                        w.order_out();
                        w.set_ignores_mouse(false);
                    }
                });
            });
        }

        let scroll = snap(self.scroll.position);
        let pill_y = self.pill.position;
        let placeholder: SharedString = format!(
            "Search {} tools, or drop anything",
            self.hub.catalog.tools().len()
        )
        .into();

        // ---- search row ----
        self.input
            .update(cx, |s, _| s.set_editor_style(editor_style(t)));
        let ph = if self.files.is_empty() {
            placeholder.clone()
        } else {
            SharedString::from(format!(
                "What do you want to do with {}?",
                if self.files.len() == 1 {
                    "this file"
                } else {
                    "these files"
                }
            ))
        };
        self.input
            .update(cx, |s, cx| s.set_placeholder(ph.clone(), window, cx));
        let input_focus = self.input.read(cx).focus_handle(cx);
        let field = div()
            .id("search-field")
            .track_focus(&input_focus)
            .role(Role::SearchInput)
            .aria_label("Search tools")
            .aria_placeholder(ph)
            .aria_value(self.query.clone())
            .flex_1()
            .min_w_0()
            .h(px(24.))
            .overflow_hidden()
            .ramp(ty::INPUT)
            .text_color(t.ink)
            .child(KitInput::new(&self.input));

        let file_chip = self.files.first().map(|first| {
            let total: u64 = self.files.iter().map(|f| f.bytes).sum();
            let category = file_category(first);
            div()
                .flex()
                .flex_none()
                .items_center()
                .gap(px(sp::S))
                .h(px(36.))
                .max_w(px(280.))
                .pl(px(sp::S))
                .pr(px(2.))
                .ml(px(sp::M))
                .mr(px(sp::M))
                .rounded(px(radius::ROW))
                .bg(t.fill8)
                .child(category_tile(
                    category,
                    file_icon(first),
                    24.,
                    t,
                    t.palette.bar_over_mid(),
                ))
                .child(
                    div()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .ramp(ty::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(files_label(&self.files)),
                )
                .child(
                    div()
                        .flex_none()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(format_bytes(total)),
                )
                .child(
                    focus_border(div().id("clear-file"), t)
                        .role(Role::Button)
                        .aria_label("Remove dropped files")
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(HIT))
                        .rounded(px(radius::CONTROL))
                        .hover(move |s| s.bg(t.fill12))
                        .tooltip(super::widgets::tip("Remove the files", &[]))
                        .on_click(cx.listener(|bar, _, _, cx| bar.set_files(Vec::new(), cx)))
                        .child(icon("x", 12., t.ink2)),
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
            .child(div().flex_none().w(px(sp::M)))
            .child(field)
            .when_some(file_chip, |d, c| d.child(c))
            .when(self.files.is_empty(), |d| {
                d.child(div().flex_none().w(px(20.)))
            })
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(0.5))
                    .bg(t.hair),
            );

        // ---- list ----
        let top = scroll - 90.0;
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
                            .ramp(ty::CAPTION)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink3)
                            .child(title.clone())
                            .into_any_element(),
                    );
                }
                Entry::Row { y, h, row } => {
                    if *y + *h < top || *y > bottom {
                        continue;
                    }
                    if let Some(r) = self.results.get(*row) {
                        let r = r.clone();
                        items.push(self.render_row(&r, *row, *y, *h, t, cx));
                    }
                }
            }
        }
        let pill_h = self.list.row_hs.get(self.selected).copied().unwrap_or(ROW);
        let base = if t.dark { 0.16 } else { 0.10 };
        let pill = div()
            .absolute()
            .left(px(8.))
            .top(px(pill_y))
            .w(px(WIDTH - 16.))
            .h(px(pill_h))
            .rounded(px(radius::ROW))
            .bg(t.palette.accent_text.hsla(base + 0.10 * flash_k));

        let dragging = self.drag_active;
        let trimmed = self.query.trim().to_string();
        let searched_files =
            self.wants_files() && matches!(self.hub.finder.state(), finder::State::Ready { .. });
        let list_body: AnyElement = if self.results.is_empty() && self.files_pending {
            // The tools had nothing and the index has not answered yet: a blank list for a few
            // milliseconds, not a "nothing found" that is about to change.
            div().size_full().into_any_element()
        } else if self.results.is_empty() {
            div()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(sp::S))
                .pb(px(sp::S))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .size(px(40.))
                        .rounded(px(10.))
                        .bg(t.fill8)
                        .child(icon("search", 18., t.ink2)),
                )
                .child(
                    div()
                        .pt(px(sp::XS))
                        .ramp(ty::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(if trimmed.is_empty() {
                            "No tools".to_string()
                        } else if searched_files {
                            format!("No tools or files match \u{201C}{trimmed}\u{201D}")
                        } else {
                            format!("No tools match \u{201C}{trimmed}\u{201D}")
                        }),
                )
                .child(div().ramp(ty::SMALL).text_color(t.ink2).child(
                    "Try \u{201C}compress\u{201D} or \u{201C}heic to jpg\u{201D}, or drop a file",
                ))
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
            .id("results")
            .role(Role::ListBox)
            .aria_label("Tools and files")
            .relative()
            .flex_1()
            .w_full()
            .overflow_hidden()
            .on_scroll_wheel(cx.listener(|bar, ev: &ScrollWheelEvent, _, cx| {
                let dy = ev.delta.pixel_delta(px(20.)).y.as_f32();
                let max_y = (bar.list.content_height - VIEWPORT).max(0.0);
                let p = (bar.scroll.position - dy).clamp(0.0, max_y);
                bar.scroll = SpringState {
                    position: p,
                    velocity: 0.0,
                };
                bar.scroll_target = p;
                cx.notify();
            }))
            .child(
                div()
                    .size_full()
                    .opacity(if dragging { 0.18 } else { 1.0 })
                    .child(list_body),
            )
            .children(super::widgets::thumb(
                scroll,
                VIEWPORT,
                self.list.content_height,
                t,
            ))
            .when(dragging, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .gap(px(sp::S))
                        .ramp(ty::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(icon("upload", 18., t.accent_text))
                        .child("Drop to see the tools for this file"),
                )
            });

        // ---- footer ----
        let has_chips = self
            .results
            .get(self.selected)
            .is_some_and(|r| !r.chips.is_empty());
        let (left, keys): (String, Vec<(&str, Vec<&str>)>) = {
            let left = self
                .footer_msg
                .clone()
                .or_else(|| self.results.understood.clone())
                .or_else(|| self.results.footer.clone())
                .unwrap_or_else(|| "Everything runs on this computer".into());
            let c = cmd();
            let keys = if self.results.is_empty() {
                Vec::new()
            } else if matches!(
                self.results.get(self.selected).map(|r| &r.action),
                Some(Action::CopyText { .. })
            ) {
                vec![("Copy", vec!["\u{21B5}"])]
            } else if self.selected_is_file() {
                vec![("Open", vec!["\u{21B5}"]), (SHOW_FILE, vec![c, "\u{21B5}"])]
            } else if has_chips {
                vec![("Edit option", vec!["\u{21E5}"]), ("Run", vec!["\u{21B5}"])]
            } else if !self.files.is_empty() {
                vec![
                    ("Run", vec!["\u{21B5}"]),
                    ("Chain tools", vec![c, "\u{21B5}"]),
                ]
            } else {
                vec![("Open", vec!["\u{21B5}"]), ("Actions", vec![c, "K"])]
            };
            (left, keys)
        };
        let show_brand =
            self.files.is_empty() && self.footer_msg.is_none() && self.results.understood.is_none();
        let footer = div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(FOOTER))
            .w_full()
            .px(px(20.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_0()
                    .h(px(0.5))
                    .bg(t.hair),
            )
            .when(show_brand, |d| {
                d.child(brand_mark(24., t.window, t))
                    .child(div().flex_none().w(px(sp::S)))
                    .child(
                        div()
                            .flex_none()
                            .ramp(ty::SMALL)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink2)
                            .child("Panoply"),
                    )
                    .child(div().flex_none().w(px(sp::M)))
            })
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ramp(ty::SMALL)
                    .text_color(
                        if self.results.understood.is_some() && self.footer_msg.is_none() {
                            t.ink2
                        } else {
                            t.ink3
                        },
                    )
                    .child(left),
            )
            .child(div().flex_1().min_w(px(sp::L)))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(sp::M))
                    .children(keys.into_iter().enumerate().map(|(i, (label, caps))| {
                        let this = cx.entity().downgrade();
                        let this2 = cx.entity().downgrade();
                        focus_border(div().id(("footer-key", i)), t)
                            .role(Role::Button)
                            .aria_label(label.to_string())
                            .tab_stop(true)
                            .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
                                this.update(cx, |bar, cx| match label {
                                    "Actions" | "Edit option" => {
                                        bar.activate_with(false, window, cx)
                                    }
                                    SHOW_FILE => bar.open_selected_file(true, cx),
                                    _ => bar.activate(window, cx),
                                })
                                .ok();
                            })
                            .on_click(move |_, window, cx| {
                                this2
                                    .update(cx, |bar, cx| match label {
                                        "Actions" | "Edit option" => {
                                            bar.activate_with(false, window, cx)
                                        }
                                        SHOW_FILE => bar.open_selected_file(true, cx),
                                        _ => bar.activate(window, cx),
                                    })
                                    .ok();
                            })
                            .flex()
                            .items_center()
                            .gap(px(sp::S))
                            .h(px(HIT))
                            .px(px(sp::S))
                            .rounded(px(radius::CONTROL))
                            .hover(move |s| s.bg(t.fill8))
                            .child(div().ramp(ty::SMALL).text_color(t.ink2).child(label))
                            .child(
                                div().flex().gap(px(sp::XS)).children(
                                    caps.into_iter()
                                        .map(move |c| keycap(c, t).into_any_element()),
                                ),
                            )
                    })),
            );

        if let Some(panel) = self.panel.clone() {
            return div()
                .id("bar")
                .key_context("Bar")
                .on_key_down(
                    cx.listener(|bar, ev: &KeyDownEvent, window, cx| bar.on_key(ev, window, cx)),
                )
                .relative()
                .size_full()
                .flex()
                .flex_col()
                .overflow_hidden()
                .rounded(px(RADIUS))
                .bg(t.bar_base)
                .text_color(t.ink)
                .child(panel)
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(RADIUS))
                        .border(px(0.5))
                        .border_color(t.hair),
                )
                .into_any_element();
        }
        div()
            .id("bar")
            .track_focus(&self.focus)
            .key_context("Bar")
            .on_action(
                cx.listener(|bar, _: &super::live::FocusNextControl, window, cx| {
                    bar.activate_with(false, window, cx)
                }),
            )
            .on_action(
                cx.listener(|bar, _: &super::live::FocusPrevControl, window, cx| {
                    bar.move_sel(-1, cx);
                    let _ = window;
                }),
            )
            .on_key_down(
                cx.listener(|bar, ev: &KeyDownEvent, window, cx| bar.on_key(ev, window, cx)),
            )
            .on_drag_move::<ExternalPaths>(cx.listener(
                |bar, _ev: &DragMoveEvent<ExternalPaths>, _, cx| {
                    if !bar.drag_active {
                        bar.drag_active = true;
                        cx.notify();
                    }
                },
            ))
            .on_drop::<ExternalPaths>(cx.listener(|bar, paths: &ExternalPaths, _, cx| {
                bar.drag_active = false;
                bar.dropped(paths.paths().to_vec(), cx);
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(RADIUS))
            .bg(t.bar_base)
            .text_color(t.ink)
            .child(search_row)
            .child(list)
            .child(footer)
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .rounded(px(RADIUS))
                    .border(px(0.5))
                    .border_color(t.hair),
            )
            .when(dragging, |d| {
                d.child(
                    div()
                        .absolute()
                        .inset_0()
                        .rounded(px(RADIUS))
                        .border(px(2.))
                        .border_color(t.accent_text),
                )
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Action;

    #[test]
    fn files_still_load_under_a_calculator_answer() {
        // "2024-10" and "15% of 2400" are not answers the file index should be shut out of.
        for q in ["2024-10", "2+2", "15% of 2400", "report"] {
            assert!(wants_file_search(true, q, false), "query: {q}");
        }
        assert!(crate::calc::answer("15% of 2400").is_some());
        assert!(matches!(
            crate::calc::answer("2+2").map(|a| Action::CopyText { text: a.copy }),
            Some(Action::CopyText { .. })
        ));
        // Unchanged: a request, a dropped file or one letter do not look up files.
        assert!(!wants_file_search(true, "under 50 kb", true));
        assert!(!wants_file_search(false, "report", false));
        assert!(!wants_file_search(true, "r", false));
    }
}

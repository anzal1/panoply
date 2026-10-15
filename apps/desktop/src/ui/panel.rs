//! The tool panel: the screen for one module, drawn entirely from its manifest.
//!
//! Options (number, choice as a segmented control, toggle, size, text, file) come from
//! [`Form`], the estimate from [`crate::estimate`], and the run goes to a worker thread through
//! [`crate::jobs`]. The same view is hosted in the command bar and in the app window. No module
//! has any code in this file.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use futures::StreamExt;
use futures::channel::mpsc;
use gpui::prelude::*;
use gpui::*;
use gpui_base::input::{Input as KitInput, InputEvent, InputState};
use serde_json::Value;

use super::Hub;
use super::theme::{Theme, is_dark, radius, sp, ty};
use super::widgets::{
    ButtonKind, Ramp, button, button_ex, caption, card, category_tile, cmd, editor_style,
    file_category, file_icon, fits_segmented, focus_ring, icon, icon_button, keycap, note, notice,
    popup_button, popup_menu, progress_bar, rule, scroll_thumb, segmented, switch, tip,
};
use crate::catalog::Tool;
use crate::form::{Control, Field, Form, display, number};
use crate::history::Entry;
use crate::jobs::{
    self, ConsentReply, Done, Failure, JobMsg, JobReport, JobRequest, RunningJob, UndoReport,
};
use crate::profile::{FileProfile, expand_paths};
use crate::units::{format_bytes, now_secs, size_input_text};

/// The first segment of a choice that has no default.
const AUTO: &str = "Auto";

/// What the panel tells its host.
pub(crate) enum PanelEvent {
    /// Go back to the list.
    Close,
    /// Use these result files as the input of another tool ("Then...").
    Chain(Vec<PathBuf>),
    /// A job finished and was recorded in the history.
    Ran,
}

impl EventEmitter<PanelEvent> for ToolPanel {}

enum Phase {
    Options,
    Running {
        started: Instant,
        fraction: f32,
        label: String,
    },
    /// The job is paused until the person says whether a model or engine may be downloaded.
    Consent {
        info: panoply_core::assets::AssetInfo,
        reply: ConsentReply,
        /// Where the progress bar was, so a yes goes back to it.
        started: Instant,
    },
    Done {
        report: Box<JobReport>,
        done: Done,
        undone: Option<UndoReport>,
    },
    Failed(Failure),
}

/// The panel for one tool.
pub(crate) struct ToolPanel {
    hub: Hub,
    tool: Tool,
    files: Vec<FileProfile>,
    form: Form,
    inputs: BTreeMap<String, Entity<InputState>>,
    focus: FocusHandle,
    field_focus: BTreeMap<String, FocusHandle>,
    more_open: bool,
    /// The pop-up menu that is open: the option's key and the highlighted row.
    pub(crate) menu: Option<(String, usize)>,
    scroll_left: ScrollHandle,
    scroll_options: ScrollHandle,
    scroll_result: ScrollHandle,
    estimate: Option<String>,
    est_token: u64,
    phase: Phase,
    job: Option<RunningJob>,
    /// True when the person answered Not now to a download, so the job that stops because of it
    /// goes back to the options quietly instead of showing a failure.
    declined: bool,
    _subs: Vec<Subscription>,
}

/// Step for a number field: a tenth of a percent of its range, in a round size.
fn step_for(min: Option<f64>, max: Option<f64>, integral: bool) -> f64 {
    let range = match (min, max) {
        (Some(lo), Some(hi)) if hi > lo => hi - lo,
        _ => 100.0,
    };
    let mut step = 10f64.powf((range / 100.0).log10().floor());
    if integral {
        step = step.max(1.0);
    }
    step
}

fn tidy(n: f64, step: f64) -> f64 {
    let decimals = if step >= 1.0 {
        0
    } else {
        (-step.log10()).ceil() as i32
    };
    let m = 10f64.powi(decimals);
    (n * m).round() / m
}

fn raw_text(control: &Control, v: &Value) -> String {
    match (control, v) {
        (Control::Size, Value::Number(n)) => size_input_text(n.as_f64().unwrap_or(0.0) as u64),
        (Control::Size, Value::String(s)) => match s.parse::<u64>() {
            Ok(b) => size_input_text(b),
            Err(_) => s.clone(),
        },
        (_, Value::String(s)) => s.clone(),
        (_, Value::Number(n)) => n.to_string(),
        (_, other) => other.to_string(),
    }
}

impl ToolPanel {
    /// Open the panel for a tool, on these files, with options a request already filled in.
    pub(crate) fn new(
        hub: Hub,
        tool_index: usize,
        files: Vec<FileProfile>,
        preset: Vec<(String, Value)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let tool = hub.catalog.tool(tool_index).clone();
        let mut form = Form::from_manifest(&tool.manifest);
        for (k, v) in &preset {
            form.set_value(k, v.clone());
        }
        form.float_set_fields();
        let mut inputs = BTreeMap::new();
        let mut field_focus = BTreeMap::new();
        let mut subs = Vec::new();
        for f in form.fields().to_vec() {
            match &f.control {
                Control::Number { .. } | Control::Size | Control::Text | Control::File => {
                    let default_hint = f
                        .default
                        .as_ref()
                        .map(|d| raw_text(&f.control, d))
                        .unwrap_or_default();
                    let hint = match (&f.control, default_hint.is_empty()) {
                        (_, false) => default_hint,
                        (Control::Size, true) => "for example 200kb".to_string(),
                        (Control::File, true) => "No file chosen".to_string(),
                        _ => "Automatic".to_string(),
                    };
                    let input = cx.new(|cx| InputState::new(window, cx).placeholder(hint));
                    if let Some(v) = form.value(&f.key).filter(|_| form.is_set(&f.key)) {
                        let text = raw_text(&f.control, &v);
                        input.update(cx, |s, cx| s.set_value(text, window, cx));
                    }
                    let key = f.key.clone();
                    subs.push(cx.subscribe_in(
                        &input,
                        window,
                        move |p, input, ev: &InputEvent, window, cx| match ev {
                            InputEvent::Change => {
                                let v = input.read(cx).value().to_string();
                                let _ = p.form.set_text(&key, &v);
                                p.refresh_estimate(cx);
                                cx.notify();
                            }
                            InputEvent::PressEnter { .. } => p.run(window, cx),
                            _ => {}
                        },
                    ));
                    inputs.insert(f.key.clone(), input);
                }
                Control::Choice(_) | Control::Toggle => {
                    field_focus.insert(f.key.clone(), cx.focus_handle().tab_stop(true));
                }
            }
        }
        let focus = cx.focus_handle();
        // The first option on screen, when it is a text box (see `focus_default`).
        match form.primary().first().and_then(|f| inputs.get(&f.key)) {
            Some(first) => first.update(cx, |s, cx| s.focus(window, cx)),
            None => window.focus(&focus, cx),
        }
        let mut panel = Self {
            hub,
            tool,
            files,
            form,
            inputs,
            focus,
            field_focus,
            more_open: false,
            menu: None,
            scroll_left: ScrollHandle::new(),
            scroll_options: ScrollHandle::new(),
            scroll_result: ScrollHandle::new(),
            estimate: None,
            est_token: 0,
            phase: Phase::Options,
            declined: false,
            job: None,
            _subs: subs,
        };
        panel.refresh_estimate(cx);
        panel
    }

    /// Put keyboard focus where the current screen needs it: the first text field while editing
    /// options, the panel itself otherwise, so Return, Escape and Command+Z reach it.
    fn focus_default(&self, window: &mut Window, cx: &mut Context<Self>) {
        // The first option on screen, when it is a text box. `inputs` is keyed by name, so its first
        // entry is the alphabetically first option, which is often one further down the panel (or
        // one that does not apply to the chosen target) and drew the focus ring there.
        let first_shown = self
            .form
            .primary()
            .first()
            .and_then(|f| self.inputs.get(&f.key));
        match (&self.phase, first_shown) {
            (Phase::Options, Some(first)) => first.update(cx, |s, cx| s.focus(window, cx)),
            _ => window.focus(&self.focus, cx),
        }
    }

    /// Pattern A (instant transform) runs right away with the best defaults, as FLOWS.md says.
    /// Other patterns wait for Return. Does nothing when the files or options are not ready.
    pub(crate) fn run_if_instant(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tool.pattern == panoply_manifest::Pattern::InstantTransform
            && self.tool.takes_files()
            && self.blocker().is_none()
        {
            self.run(window, cx);
        }
    }

    pub(crate) fn is_options(&self) -> bool {
        matches!(self.phase, Phase::Options)
    }

    #[cfg(feature = "snapshot")]
    pub(crate) fn is_done(&self) -> bool {
        matches!(self.phase, Phase::Done { .. })
    }

    #[cfg(feature = "snapshot")]
    pub(crate) fn result_headline(&self) -> Option<String> {
        match &self.phase {
            Phase::Done { done, report, .. } => Some(done.headline(report.request.inputs.len())),
            _ => None,
        }
    }

    #[cfg(feature = "snapshot")]
    pub(crate) fn failure(&self) -> Option<&Failure> {
        match &self.phase {
            Phase::Failed(f) => Some(f),
            _ => None,
        }
    }

    /// Open the consent sheet as a job that needs `info` would, and hand back where the answer
    /// goes. For the headless check, which has no real download to wait on.
    #[cfg(feature = "snapshot")]
    pub(crate) fn preview_consent(
        &mut self,
        info: panoply_core::assets::AssetInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> std::sync::mpsc::Receiver<bool> {
        let (reply, rx) = ConsentReply::pair();
        self.on_msg(JobMsg::NeedsConsent { info, reply }, window, cx);
        rx
    }

    /// The item the consent sheet is asking about, while it is open.
    #[cfg(feature = "snapshot")]
    pub(crate) fn consent_item(&self) -> Option<panoply_core::assets::AssetInfo> {
        match &self.phase {
            Phase::Consent { info, .. } => Some(info.clone()),
            _ => None,
        }
    }

    pub(crate) fn outputs(&self) -> Vec<PathBuf> {
        match &self.phase {
            Phase::Done { done, .. } => done.outputs.clone(),
            _ => Vec::new(),
        }
    }

    // ---- files ----

    /// Add files (a drop, or the file picker).
    pub(crate) fn add_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
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
            let _ = this.update(cx, |p, cx| {
                for f in files {
                    if !p.files.iter().any(|x| x.path == f.path) {
                        p.files.push(f);
                    }
                }
                p.refresh_estimate(cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn choose_files(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Choose".into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = rx.await {
                let _ = this.update(cx, |p, cx| p.add_files(paths, cx));
            }
        })
        .detach();
    }

    fn remove_file(&mut self, i: usize, cx: &mut Context<Self>) {
        if i < self.files.len() {
            self.files.remove(i);
            self.refresh_estimate(cx);
            cx.notify();
        }
    }

    fn unaccepted(&self) -> Vec<&FileProfile> {
        self.files
            .iter()
            .filter(|f| {
                !self
                    .tool
                    .manifest
                    .accepts_file(&f.sniffed.mime, &f.sniffed.ext)
            })
            .collect()
    }

    /// Why Run is not available, in plain words.
    pub(crate) fn blocker(&self) -> Option<String> {
        if self.tool.takes_files() && self.files.is_empty() {
            return Some("Add a file to run this tool.".into());
        }
        if let Some(f) = self.unaccepted().first() {
            return Some(format!(
                "{} does not take {} files. Remove {} or pick another tool.",
                self.tool.name,
                if f.sniffed.ext.is_empty() {
                    "this kind of".to_string()
                } else {
                    f.sniffed.ext.to_ascii_uppercase()
                },
                f.name
            ));
        }
        if let Some(e) = self.form.to_options().err() {
            return Some(e);
        }
        None
    }

    // ---- options ----

    fn refresh_estimate(&mut self, cx: &mut Context<Self>) {
        self.est_token += 1;
        let token = self.est_token;
        let Ok(options) = self.form.to_options() else {
            self.estimate = None;
            return;
        };
        let manifest = self.tool.manifest.clone();
        let bytes: u64 = self.files.iter().map(|f| f.bytes).sum();
        cx.spawn(async move |this, cx| {
            let est = cx
                .background_executor()
                .spawn(async move { crate::estimate::for_job(&manifest, bytes, &options) })
                .await;
            let _ = this.update(cx, |p, cx| {
                if p.est_token == token {
                    p.estimate = est.map(|e| e.text());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn pick_choice(&mut self, key: &str, value: String, cx: &mut Context<Self>) {
        if value == AUTO {
            self.form.clear(key);
        } else {
            self.form.set_value(key, Value::String(value));
        }
        self.refresh_estimate(cx);
        cx.notify();
    }

    fn step_choice(&mut self, key: &str, delta: i32, cx: &mut Context<Self>) {
        let Some(Field {
            control: Control::Choice(values),
            default,
            ..
        }) = self.form.field(key).cloned()
        else {
            return;
        };
        let mut all = values;
        if default.is_none() {
            all.insert(0, AUTO.to_string());
        }
        let cur = self
            .form
            .value(key)
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| AUTO.to_string());
        let at = all.iter().position(|v| *v == cur).unwrap_or(0) as i32;
        let i = (at + delta).clamp(0, all.len() as i32 - 1) as usize;
        self.pick_choice(key, all[i].clone(), cx);
    }

    fn toggle(&mut self, key: &str, cx: &mut Context<Self>) {
        let now = self
            .form
            .value(key)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.form.set_value(key, Value::Bool(!now));
        self.refresh_estimate(cx);
        cx.notify();
    }

    fn step_number(&mut self, key: &str, dir: f64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(Field {
            control: Control::Number { min, max },
            default,
            ..
        }) = self.form.field(key).cloned()
        else {
            return;
        };
        let integral = default
            .as_ref()
            .is_none_or(|d| d.as_f64().is_some_and(|n| n.fract() == 0.0))
            && min.is_none_or(|m| m.fract() == 0.0);
        let step = step_for(min, max, integral);
        let cur = self
            .form
            .value(key)
            .and_then(|v| v.as_f64())
            .or(min)
            .unwrap_or(0.0);
        let mut next = tidy(cur + dir * step, step);
        if let Some(lo) = min {
            next = next.max(lo);
        }
        if let Some(hi) = max {
            next = next.min(hi);
        }
        self.form.set_value(key, number(next));
        if let Some(input) = self.inputs.get(key) {
            let text = raw_text(&Control::Text, &number(next));
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        }
        self.refresh_estimate(cx);
        cx.notify();
    }

    // ---- running ----

    /// Return: run the job.
    pub(crate) fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_options() || self.blocker().is_some() {
            return;
        }
        let Ok(options) = self.form.to_options() else {
            return;
        };
        let request = JobRequest {
            tool_id: self.tool.id.clone(),
            tool_name: self.tool.name.clone(),
            inputs: self.files.iter().map(|f| f.path.clone()).collect(),
            options,
            out_dir: None,
        };
        let (tx, mut rx) = mpsc::unbounded::<JobMsg>();
        let job = jobs::spawn(self.hub.catalog.registry().clone(), request, move |m| {
            let _ = tx.unbounded_send(m);
        });
        self.job = Some(job);
        self.declined = false;
        self.phase = Phase::Running {
            started: Instant::now(),
            fraction: 0.0,
            label: "Starting".into(),
        };
        self.focus_default(window, cx);
        cx.notify();
        // Progress shows after 300 ms; nothing redraws before that unless the job reports.
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(
                    panoply_core::budgets::PROGRESS_AFTER_MS,
                ))
                .await;
            let _ = this.update(cx, |_, cx| cx.notify());
        })
        .detach();
        cx.spawn_in(window, async move |this, cx| {
            while let Some(msg) = rx.next().await {
                let end = matches!(msg, JobMsg::Finished(_));
                if this
                    .update_in(cx, |p, window, cx| p.on_msg(msg, window, cx))
                    .is_err()
                    || end
                {
                    break;
                }
            }
        })
        .detach();
    }

    /// The consent sheet's two answers. Download lets the job go on; Not now stops it with
    /// nothing fetched and nothing written.
    pub(crate) fn answer_consent(&mut self, yes: bool, cx: &mut Context<Self>) {
        let Phase::Consent { reply, started, .. } = &self.phase else {
            return;
        };
        reply.answer(yes);
        self.declined = !yes;
        self.phase = Phase::Running {
            started: *started,
            fraction: 0.0,
            label: if yes {
                "Downloading".into()
            } else {
                "Stopping".into()
            },
        };
        cx.notify();
    }

    fn on_msg(&mut self, msg: JobMsg, window: &mut Window, cx: &mut Context<Self>) {
        match msg {
            JobMsg::NeedsConsent { info, reply } => {
                let started = match &self.phase {
                    Phase::Running { started, .. } => *started,
                    _ => Instant::now(),
                };
                self.phase = Phase::Consent {
                    info,
                    reply,
                    started,
                };
                self.focus_default(window, cx);
                cx.notify();
            }
            JobMsg::Progress { fraction, label } => {
                if let Phase::Running {
                    fraction: f,
                    label: l,
                    ..
                } = &mut self.phase
                {
                    *f = fraction;
                    *l = label;
                    cx.notify();
                }
            }
            JobMsg::Finished(report) => {
                self.job = None;
                match report.result.clone() {
                    Ok(done) => {
                        let headline = done.headline(report.request.inputs.len());
                        self.hub.record(
                            Entry {
                                at: now_secs(),
                                tool_id: report.request.tool_id.clone(),
                                tool_name: report.request.tool_name.clone(),
                                inputs: report.request.inputs.clone(),
                                options: report.request.options.clone(),
                                outputs: done.outputs.clone(),
                                headline,
                            },
                            cx,
                        );
                        self.phase = Phase::Done {
                            report,
                            done,
                            undone: None,
                        };
                        cx.emit(PanelEvent::Ran);
                    }
                    Err(f) => {
                        if f.cancelled || std::mem::take(&mut self.declined) {
                            self.phase = Phase::Options;
                        } else {
                            self.phase = Phase::Failed(f);
                        }
                    }
                }
                self.focus_default(window, cx);
                cx.notify();
            }
        }
    }

    /// Escape while running: stop at once.
    pub(crate) fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.cancel();
        }
    }

    /// Command+Z on a result card: the new files go to the Trash (the Recycle Bin on Windows). A file
    /// the system will not take stays where it is and the card says so.
    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        let Phase::Done { done, undone, .. } = &self.phase else {
            return;
        };
        // Undo again is allowed after a refusal: what was moved is skipped, the rest is retried.
        if undone.as_ref().is_some_and(|u| u.is_complete()) || done.outputs.is_empty() {
            return;
        }
        let outputs = done.outputs.clone();
        cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move { jobs::undo_outputs(&outputs, crate::platform::trash) })
                .await;
            let _ = this.update(cx, |p, cx| {
                if let Phase::Done { undone, .. } = &mut p.phase {
                    *undone = Some(report);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Back to the options with everything as it was.
    fn again(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.phase = Phase::Options;
        self.focus_default(window, cx);
        cx.notify();
    }

    fn open_first(&mut self, cx: &mut Context<Self>) {
        if let Phase::Done { done, .. } = &self.phase
            && let Some(p) = done.outputs.first()
        {
            cx.open_with_system(p);
        }
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        if let Phase::Done { done, .. } = &self.phase
            && let Some(p) = done.outputs.first()
        {
            cx.reveal_path(p);
        }
    }

    fn chain(&mut self, cx: &mut Context<Self>) {
        let outs = self.outputs();
        if !outs.is_empty() {
            cx.emit(PanelEvent::Chain(outs));
        }
    }

    fn copy_report(&mut self, cx: &mut Context<Self>) {
        if let Phase::Done { done, .. } = &self.phase {
            cx.write_to_clipboard(ClipboardItem::new_string(done.notes.join("\n")));
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = ev.keystroke.key.as_str();
        let m = ev.keystroke.modifiers;
        // An open pop-up menu takes the arrow keys, Return, Space and Escape.
        if let Some((key, at)) = self.menu.clone() {
            let Some(field) = self.form.field(&key).cloned() else {
                self.menu = None;
                return;
            };
            let (shown, _) = self.shown_for(&field);
            match k {
                "up" => self.menu = Some((key, at.saturating_sub(1))),
                "down" => self.menu = Some((key, (at + 1).min(shown.len().saturating_sub(1)))),
                "enter" | "space" => {
                    if let Some(v) = shown.get(at).cloned() {
                        self.pick_choice(&key, v, cx);
                    }
                    self.menu = None;
                }
                "escape" | "tab" => self.menu = None,
                _ => {}
            }
            cx.notify();
            return;
        }
        if k == "tab" {
            if m.shift {
                window.focus_prev(cx);
            } else {
                window.focus_next(cx);
            }
            return;
        }
        match &self.phase {
            Phase::Options => match k {
                "enter" => self.run(window, cx),
                "escape" => cx.emit(PanelEvent::Close),
                _ => {}
            },
            Phase::Running { .. } => {
                if k == "escape" {
                    self.cancel();
                }
            }
            Phase::Consent { .. } => match k {
                "enter" => self.answer_consent(true, cx),
                "escape" => self.answer_consent(false, cx),
                _ => {}
            },
            Phase::Done { .. } => match k {
                "enter" if m.secondary() => self.chain(cx),
                "enter" => self.open_first(cx),
                "r" if m.secondary() => self.reveal(cx),
                "escape" => cx.emit(PanelEvent::Close),
                _ => {}
            },
            Phase::Failed(_) => match k {
                "enter" => self.again(window, cx),
                "escape" => cx.emit(PanelEvent::Close),
                _ => {}
            },
        }
    }
}

// ---- drawing ----

/// Width of the options column.
const OPTIONS_W: f32 = 344.0;
/// The panel's margin, the same as the app's page margin.
const MARGIN: f32 = sp::XL;

impl ToolPanel {
    /// The values a choice shows, with "Auto" in front when it has no default, and the current one.
    fn shown_for(&self, f: &Field) -> (Vec<String>, Option<String>) {
        let Control::Choice(values) = &f.control else {
            return (Vec::new(), None);
        };
        let sel = self
            .form
            .value(&f.key)
            .and_then(|v| v.as_str().map(str::to_string));
        let mut shown = values.clone();
        if f.default.is_none() {
            // A choice with no default starts on "Auto": the module picks for each file.
            shown.insert(0, AUTO.to_string());
            let sel = sel.or_else(|| Some(AUTO.to_string()));
            return (shown, sel);
        }
        (shown, sel)
    }

    /// Open the pop-up menu of an option, for the review renders.
    #[cfg(feature = "snapshot")]
    pub(crate) fn preview_menu(&mut self, key: &str, cx: &mut Context<Self>) {
        self.open_menu(key, cx);
    }

    /// Open a pop-up menu, with the current value highlighted.
    fn open_menu(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(f) = self.form.field(key).cloned() else {
            return;
        };
        let (shown, sel) = self.shown_for(&f);
        let at = sel
            .and_then(|s| shown.iter().position(|v| *v == s))
            .unwrap_or(0);
        self.menu = Some((key.to_string(), at));
        cx.notify();
    }

    fn field_view(&mut self, f: &Field, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let key = f.key.clone();
        let this = cx.entity().downgrade();
        let control: AnyElement = match &f.control {
            Control::Choice(_) => {
                let focus = self
                    .field_focus
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| cx.focus_handle());
                let (shown, sel) = self.shown_for(f);
                if !fits_segmented(&shown) {
                    // More than four values, or long ones: a pop-up button and its menu.
                    let current = sel
                        .as_deref()
                        .map(super::widgets::segment_label)
                        .unwrap_or_default();
                    let open_at = self
                        .menu
                        .as_ref()
                        .filter(|(k, _)| *k == key)
                        .map(|(_, h)| *h);
                    let (k_open, k_key, k_pick) = (key.clone(), key.clone(), key.clone());
                    let (t_open, t_pick, t_close) = (this.clone(), this.clone(), this.clone());
                    let button = popup_button(
                        format!("choice-{key}"),
                        &f.label,
                        &current,
                        open_at.is_some(),
                        &focus,
                        t,
                        move |_, cx| {
                            let k = k_open.clone();
                            t_open.update(cx, |p, cx| p.open_menu(&k, cx)).ok();
                        },
                    )
                    .on_key_down(cx.listener(
                        move |p, ev: &KeyDownEvent, _, cx| {
                            if matches!(ev.keystroke.key.as_str(), "space" | "down" | "up") {
                                p.open_menu(&k_key, cx);
                            }
                        },
                    ));
                    let menu = open_at.map(|hl| {
                        popup_menu(
                            format!("choice-{key}"),
                            &shown,
                            sel.as_deref(),
                            hl,
                            OPTIONS_W - 2.0 * MARGIN,
                            t,
                            move |v, _, cx| {
                                let k = k_pick.clone();
                                t_pick
                                    .update(cx, |p, cx| {
                                        p.pick_choice(&k, v, cx);
                                        p.menu = None;
                                    })
                                    .ok();
                            },
                            move |_, cx| {
                                t_close
                                    .update(cx, |p, cx| {
                                        p.menu = None;
                                        cx.notify();
                                    })
                                    .ok();
                            },
                        )
                    });
                    div()
                        .relative()
                        .w_full()
                        .child(button)
                        .children(menu)
                        .into_any_element()
                } else {
                    let k2 = key.clone();
                    let this2 = this.clone();
                    let k3 = key.clone();
                    segmented(
                        format!("choice-{key}"),
                        &f.label,
                        &shown,
                        sel.as_deref(),
                        &focus,
                        t,
                        move |v, _, cx| {
                            let k = k2.clone();
                            this2.update(cx, |p, cx| p.pick_choice(&k, v, cx)).ok();
                        },
                    )
                    .on_key_down(cx.listener(move |p, ev: &KeyDownEvent, _, cx| {
                        match ev.keystroke.key.as_str() {
                            "left" => p.step_choice(&k3, -1, cx),
                            "right" => p.step_choice(&k3, 1, cx),
                            _ => {}
                        }
                    }))
                    .into_any_element()
                }
            }
            Control::Toggle => {
                let on = self
                    .form
                    .value(&key)
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                let focus = self
                    .field_focus
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| cx.focus_handle());
                let k2 = key.clone();
                let k3 = key.clone();
                switch(
                    format!("toggle-{key}"),
                    &f.label,
                    on,
                    &focus,
                    t,
                    move |_, cx| {
                        let k = k2.clone();
                        this.update(cx, |p, cx| p.toggle(&k, cx)).ok();
                    },
                )
                .on_key_down(cx.listener(move |p, ev: &KeyDownEvent, _, cx| {
                    if ev.keystroke.key == "space" {
                        p.toggle(&k3, cx);
                    }
                }))
                .into_any_element()
            }
            Control::Number { .. } | Control::Size | Control::Text | Control::File => {
                let Some(input) = self.inputs.get(&key).cloned() else {
                    return div().into_any_element();
                };
                input.update(cx, |s, _| s.set_editor_style(editor_style(t)));
                let raw = input.read(cx).value().to_string();
                let has_err = self.form.error(&key).is_some();
                let focus = input.read(cx).focus_handle(cx);
                // The ring is the outer element's border, 2 pt outside the field and pulled back
                // in by a negative margin, so the field's footprint never changes.
                let boxed = div()
                    .id(format!("input-{key}"))
                    .role(Role::TextInput)
                    .aria_label(f.label.clone())
                    .aria_value(raw)
                    .track_focus(&focus)
                    .flex_1()
                    .min_w_0()
                    .m(px(-2.))
                    .rounded(px(radius::CONTROL + 2.))
                    .border_2()
                    .border_color(gpui::transparent_black())
                    .focus(move |s| s.border_color(t.accent_text))
                    .child(
                        div()
                            .w_full()
                            .h(px(32.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(radius::CONTROL))
                            .bg(t.field)
                            .border_1()
                            .border_color(if has_err { t.bad } else { t.hair })
                            .ramp(ty::BODY)
                            .text_color(t.ink)
                            .overflow_hidden()
                            .child(
                                div()
                                    .w_full()
                                    .h(px(18.))
                                    .overflow_hidden()
                                    .child(KitInput::new(&input)),
                            ),
                    );
                let k_minus = key.clone();
                let k_plus = key.clone();
                let k_up = key.clone();
                let k_pick = key.clone();
                let row = div().flex().flex_row().items_center().gap(px(6.)).w_full();
                match &f.control {
                    Control::Number { .. } => row
                        .child(boxed)
                        .child(icon_button(
                            format!("minus-{key}"),
                            &format!("Decrease {}", f.label),
                            "minus",
                            true,
                            t,
                            cx.listener(move |p, _, window, cx| {
                                p.step_number(&k_minus, -1.0, window, cx)
                            }),
                        ))
                        .child(icon_button(
                            format!("plus-{key}"),
                            &format!("Increase {}", f.label),
                            "plus",
                            true,
                            t,
                            cx.listener(move |p, _, window, cx| {
                                p.step_number(&k_plus, 1.0, window, cx)
                            }),
                        ))
                        .on_key_down(cx.listener(move |p, ev: &KeyDownEvent, window, cx| {
                            match ev.keystroke.key.as_str() {
                                "up" => p.step_number(&k_up, 1.0, window, cx),
                                "down" => p.step_number(&k_up, -1.0, window, cx),
                                _ => {}
                            }
                        }))
                        .into_any_element(),
                    Control::File => row
                        .child(boxed)
                        .child(button(
                            format!("pick-{key}"),
                            "Choose",
                            ButtonKind::Secondary,
                            None,
                            t,
                            cx.listener(move |p, _, _, cx| {
                                let rx = cx.prompt_for_paths(PathPromptOptions {
                                    files: true,
                                    directories: false,
                                    multiple: false,
                                    prompt: Some("Choose".into()),
                                });
                                let key = k_pick.clone();
                                cx.spawn(async move |this, cx| {
                                    if let Ok(Ok(Some(paths))) = rx.await
                                        && let Some(path) = paths.first()
                                    {
                                        let text = path.display().to_string();
                                        let _ = this.update(cx, |p, cx| {
                                            p.form.set_value(&key, Value::String(text));
                                            cx.notify();
                                        });
                                    }
                                })
                                .detach();
                                let _ = p;
                            }),
                        ))
                        .into_any_element(),
                    _ => row.child(boxed).into_any_element(),
                }
            }
        };
        let show_label = !matches!(f.control, Control::Toggle);
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .when(show_label, |d| d.child(caption(&f.label, t)))
            .child(control)
            .when_some(self.form.error(&f.key).map(str::to_string), |d, e| {
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .items_start()
                        .ramp(ty::SMALL)
                        .text_color(t.bad)
                        .child(div().pt(px(1.)).child(icon("triangle-alert", 13., t.bad)))
                        .child(e),
                )
            })
            .when_some(
                f.help.clone().filter(|_| self.form.error(&f.key).is_none()),
                |d, h| d.child(div().ramp(ty::SMALL).text_color(t.ink3).child(h)),
            )
            .into_any_element()
    }

    fn files_card(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let bad: Vec<bool> = self
            .files
            .iter()
            .map(|f| {
                !self
                    .tool
                    .manifest
                    .accepts_file(&f.sniffed.mime, &f.sniffed.ext)
            })
            .collect();
        let last = self.files.len().saturating_sub(1);
        let rows: Vec<AnyElement> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, f)| {
                div()
                    .id(("file", i))
                    .role(Role::ListItem)
                    .aria_label(format!("{}, {}", f.name, format_bytes(f.bytes)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .min_h(px(44.))
                    .pl(px(10.))
                    .pr(px(sp::S))
                    .when(i != last, |d| d.border_b_1().border_color(t.hair))
                    .child(if bad[i] {
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .size(px(28.))
                            .rounded(px(7.))
                            .bg(t.bad.opacity(0.12))
                            .child(icon("triangle-alert", 14., t.bad))
                    } else {
                        category_tile(file_category(f), file_icon(f), 28., t, t.palette.card)
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis_middle()
                            .ramp(ty::BODY)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink)
                            .child(f.name.clone()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .ramp(ty::SMALL)
                            .text_color(t.ink2)
                            .child(format!("{}  {}", f.kind_label(), format_bytes(f.bytes))),
                    )
                    .child(icon_button(
                        format!("remove-{i}"),
                        &format!("Remove {}", f.name),
                        "x",
                        false,
                        t,
                        cx.listener(move |p, _, _, cx| p.remove_file(i, cx)),
                    ))
                    .into_any_element()
            })
            .collect();
        let empty = self.files.is_empty();
        let takes = self.tool.takes_files();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .h(px(28.))
                    .child(caption(
                        if takes {
                            "Files"
                        } else {
                            "This tool needs no file"
                        },
                        t,
                    ))
                    .when(takes && !empty, |d| {
                        d.child(button(
                            "add-files",
                            "Add files",
                            ButtonKind::Ghost,
                            Some("plus"),
                            t,
                            cx.listener(|p, _, _, cx| p.choose_files(cx)),
                        ))
                    }),
            )
            .when(takes, |d| {
                d.child(
                    card(t)
                        .id("files-card")
                        .role(Role::List)
                        .aria_label("Files")
                        .overflow_hidden()
                        .when(empty, |d| {
                            d.child(
                                div()
                                    .py(px(sp::XL))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(sp::S))
                                    .child(icon("upload", 20., t.ink2))
                                    .child(
                                        div()
                                            .ramp(ty::BODY)
                                            .text_color(t.ink2)
                                            .child("Drop files here"),
                                    )
                                    .child(button(
                                        "add-files",
                                        "Choose files",
                                        ButtonKind::Secondary,
                                        None,
                                        t,
                                        cx.listener(|p, _, _, cx| p.choose_files(cx)),
                                    )),
                            )
                        })
                        .children(rows),
                )
            })
            .into_any_element()
    }

    fn header(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let network = self.tool.manifest.capabilities.network;
        let (status, status_icon) = if network {
            ("Uses the network", "radio")
        } else {
            ("Processed on this computer", "lock")
        };
        let full = if network {
            format!(
                "Uses the network: {}",
                self.tool
                    .manifest
                    .capabilities
                    .network_reason
                    .clone()
                    .unwrap_or_default()
            )
        } else {
            "Processed on this computer. Nothing is uploaded.".to_string()
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(52.))
            .w_full()
            .px(px(MARGIN))
            .gap(px(sp::S))
            .child(
                icon_button(
                    "panel-back",
                    "Back to all tools",
                    "arrow-left",
                    false,
                    t,
                    cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
                )
                .ml(px(-sp::S)),
            )
            .child(
                div()
                    .flex_none()
                    .ramp(ty::BODY)
                    .text_color(t.ink2)
                    .child(self.tool.category.label()),
            )
            .child(icon("chevron-right", 12., t.ink3))
            .child(
                div()
                    .id("panel-title")
                    .role(Role::Heading)
                    .aria_label(self.tool.name.clone())
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ramp(ty::HEADLINE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child(self.tool.name.clone()),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("privacy-status")
                    .flex()
                    .flex_none()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .h(px(28.))
                    .ramp(ty::SMALL)
                    .text_color(t.ink2)
                    .tooltip(tip(&full, &[]))
                    .child(icon(status_icon, 13., t.ink2))
                    .child(status),
            )
            .into_any_element()
    }

    fn options_body(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let primary: Vec<Field> = self.form.primary().to_vec();
        let more: Vec<Field> = self.form.more().to_vec();
        let primary_views: Vec<AnyElement> =
            primary.iter().map(|f| self.field_view(f, t, cx)).collect();
        let more_views: Vec<AnyElement> = if self.more_open {
            more.iter().map(|f| self.field_view(f, t, cx)).collect()
        } else {
            Vec::new()
        };
        let none = self.form.fields().is_empty();
        let n_more = more.len();
        let more_open = self.more_open;
        let options = div()
            .id("options")
            .role(Role::Group)
            .aria_label("Options")
            .flex()
            .flex_col()
            .size_full()
            .gap(px(sp::L))
            .p(px(MARGIN))
            .overflow_y_scroll()
            .track_scroll(&self.scroll_options)
            .child(
                div()
                    .h(px(28.))
                    .flex()
                    .items_center()
                    .child(caption("Options", t)),
            )
            .when(none, |d| {
                d.child(div().ramp(ty::BODY).text_color(t.ink2).child(
                    "This tool has no options. It runs with the best settings for each file.",
                ))
            })
            .children(primary_views)
            .when(n_more > 0, |d| {
                d.child(
                    div()
                        .id("more-options")
                        .role(Role::DisclosureTriangle)
                        .aria_label("More options")
                        .aria_expanded(more_open)
                        .focusable()
                        .tab_stop(true)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .h(px(28.))
                        .rounded(px(radius::CONTROL))
                        .ramp(ty::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink2)
                        .focus_visible(move |s| s.shadow(focus_ring(t)))
                        .on_click(cx.listener(|p, _, _, cx| {
                            p.more_open = !p.more_open;
                            cx.notify();
                        }))
                        .child(icon(
                            if more_open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            13.,
                            t.ink2,
                        ))
                        .child(format!("More options ({n_more})")),
                )
            })
            .children(more_views);
        let blocker = self.blocker();
        let estimate = self.estimate.clone();
        let network = self.tool.manifest.capabilities.network;
        let reason = self
            .tool
            .manifest
            .capabilities
            .network_reason
            .clone()
            .unwrap_or_default();
        let left = div()
            .id("panel-left")
            .flex()
            .flex_col()
            .size_full()
            .gap(px(sp::L))
            .p(px(MARGIN))
            .overflow_y_scroll()
            .track_scroll(&self.scroll_left)
            .child(self.files_card(t, cx))
            .when_some(estimate, |d, e| {
                d.child(
                    notice("circle-check", t.good, format!("Expected: {e}"), t)
                        .id("estimate")
                        .role(Role::Status)
                        .aria_label(format!("Estimate: {e}")),
                )
            })
            .when(network, |d| {
                d.child(note(
                    "radio",
                    t.warn,
                    if reason.is_empty() {
                        "Uses the network.".to_string()
                    } else {
                        format!("Uses the network. {reason}")
                    },
                    t,
                ))
            })
            .when_some(
                blocker.filter(|_| !self.files.is_empty() || !self.tool.takes_files()),
                |d, b| d.child(notice("triangle-alert", t.warn, b, t)),
            );
        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(left)
                    .children(scroll_thumb(&self.scroll_left, t)),
            )
            .child(
                div()
                    .relative()
                    .flex_none()
                    .w(px(OPTIONS_W))
                    .h_full()
                    .border_l_1()
                    .border_color(t.hair)
                    .child(options)
                    .children(scroll_thumb(&self.scroll_options, t)),
            )
            .into_any_element()
    }

    fn footer(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let can_run = self.is_options() && self.blocker().is_none();
        let (hint, actions): (String, AnyElement) = match &self.phase {
            Phase::Options => (
                "Return runs it. Tab moves between options. Esc goes back.".into(),
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        button_ex(
                            "run-button",
                            "Run",
                            ButtonKind::Primary,
                            None,
                            can_run,
                            t,
                            cx.listener(|p, _, window, cx| p.run(window, cx)),
                        )
                        .aria_keyshortcuts("Enter"),
                    )
                    .child(keycap("\u{21B5}", t))
                    .into_any_element(),
            ),
            Phase::Running { .. } => (
                "Esc cancels. Cancel is instant and leaves nothing half-written.".into(),
                button(
                    "cancel-button",
                    "Cancel",
                    ButtonKind::Secondary,
                    None,
                    t,
                    cx.listener(|p, _, _, _| p.cancel()),
                )
                .into_any_element(),
            ),
            Phase::Consent { .. } => (
                "Return downloads it. Esc says not now.".into(),
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(button(
                        "consent-no",
                        "Not now",
                        ButtonKind::Secondary,
                        None,
                        t,
                        cx.listener(|p, _, _, cx| p.answer_consent(false, cx)),
                    ))
                    .child(button(
                        "consent-yes",
                        "Download",
                        ButtonKind::Primary,
                        None,
                        t,
                        cx.listener(|p, _, _, cx| p.answer_consent(true, cx)),
                    ))
                    .child(keycap("\u{21B5}", t))
                    .into_any_element(),
            ),
            Phase::Done { done, undone, .. } => {
                let has_out = !done.outputs.is_empty();
                let undone_now = undone.as_ref().is_some_and(|u| u.is_complete());
                (
                    format!("Return opens it. {}Z undoes. Esc closes.", cmd()),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(sp::S))
                        .when(has_out && !undone_now, |d| {
                            d.child(
                                button(
                                    "undo",
                                    "Undo",
                                    ButtonKind::Ghost,
                                    Some("undo-2"),
                                    t,
                                    cx.listener(|p, _, _, cx| p.undo(cx)),
                                )
                                .tooltip(tip("Undo", &[cmd(), "Z"])),
                            )
                            .child(button(
                                "show-folder",
                                "Show in folder",
                                ButtonKind::Secondary,
                                Some("folder-open"),
                                t,
                                cx.listener(|p, _, _, cx| p.reveal(cx)),
                            ))
                            .child(
                                button(
                                    "then",
                                    "Then\u{2026}",
                                    ButtonKind::Secondary,
                                    Some("arrow-right"),
                                    t,
                                    cx.listener(|p, _, _, cx| p.chain(cx)),
                                )
                                .tooltip(tip(
                                    "Use the result in another tool",
                                    &[cmd(), "\u{21B5}"],
                                )),
                            )
                            .child(button(
                                "open-result",
                                "Open",
                                ButtonKind::Primary,
                                Some("external-link"),
                                t,
                                cx.listener(|p, _, _, cx| p.open_first(cx)),
                            ))
                        })
                        .when(!has_out, |d| {
                            d.child(button(
                                "copy-report",
                                "Copy",
                                ButtonKind::Secondary,
                                Some("copy"),
                                t,
                                cx.listener(|p, _, _, cx| p.copy_report(cx)),
                            ))
                        })
                        .when(undone_now || !has_out, |d| {
                            d.child(button(
                                "done",
                                "Done",
                                ButtonKind::Primary,
                                None,
                                t,
                                cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
                            ))
                        })
                        .into_any_element(),
                )
            }
            Phase::Failed(_) => (
                "Nothing was changed. Return tries again. Esc closes.".into(),
                div()
                    .flex()
                    .gap(px(sp::S))
                    .child(button(
                        "close-failed",
                        "Close",
                        ButtonKind::Secondary,
                        None,
                        t,
                        cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
                    ))
                    .child(button(
                        "again",
                        "Try again",
                        ButtonKind::Primary,
                        None,
                        t,
                        cx.listener(|p, _, window, cx| p.again(window, cx)),
                    ))
                    .into_any_element(),
            ),
        };
        div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(56.))
            .w_full()
            .px(px(MARGIN))
            .gap(px(sp::M))
            .child(rule(t).absolute().left_0().right_0().top_0())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .ramp(ty::SMALL)
                    .text_color(t.ink3)
                    .child(hint),
            )
            .child(actions)
            .into_any_element()
    }

    fn running_body(&self, t: Theme) -> AnyElement {
        let Phase::Running {
            started,
            fraction,
            label,
        } = &self.phase
        else {
            return div().into_any_element();
        };
        let show = started.elapsed()
            >= Duration::from_millis(panoply_core::budgets::PROGRESS_AFTER_MS)
            || *fraction > 0.0;
        div()
            .id("running")
            .role(Role::Status)
            .aria_label(format!("Running {}", self.tool.name))
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(sp::M))
            .px(px(60.))
            .child(
                div()
                    .ramp(ty::HEADLINE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child(format!("Running {}", self.tool.name)),
            )
            .when(show, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(sp::S))
                        .w_full()
                        .max_w(px(420.))
                        .child(progress_bar(Some(*fraction), label, t))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_between()
                                .ramp(ty::SMALL)
                                .text_color(t.ink2)
                                .child(label.clone())
                                .child(format!("{:.0}%", fraction * 100.0)),
                        ),
                )
            })
            .into_any_element()
    }

    /// The consent sheet: what would be downloaded, how big, from where, under which license,
    /// and the promise that no file leaves the computer. Nothing starts until Download.
    fn consent_body(&self, t: Theme) -> AnyElement {
        let Phase::Consent { info, .. } = &self.phase else {
            return div().into_any_element();
        };
        let facts = [
            ("What for", info.purpose.clone()),
            ("Size", format!("{}, once", info.size_label())),
            ("From", info.source.clone()),
            ("License", info.license.clone()),
        ];
        let last = facts.len() - 1;
        div()
            .id("consent-sheet")
            .role(Role::Alert)
            .aria_label(format!(
                "Download the {}? {} from {}.",
                info.name,
                info.size_label(),
                info.source
            ))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .py(px(sp::L))
            .px(px(40.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(sp::M))
                    .w_full()
                    .max_w(px(460.))
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
                                    .size(px(36.))
                                    .rounded(px(9.))
                                    .bg(t.accent_text.opacity(0.12))
                                    .child(icon("shield-check", 18., t.accent_text)),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .ramp(ty::TITLE)
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(t.ink)
                                    .child(format!("Download the {}?", info.name)),
                            ),
                    )
                    .child(div().ramp(ty::BODY).text_color(t.ink2).child(format!(
                        "{} needs it to work. It is fetched once and kept on this computer.",
                        self.tool.name
                    )))
                    .child(card(t).children(facts.into_iter().enumerate().map(
                        |(n, (label, value))| {
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(sp::M))
                                .px(px(14.))
                                .py(px(sp::S))
                                .when(n != last, |d| d.border_b_1().border_color(t.hair))
                                .child(
                                    div()
                                        .flex_none()
                                        .w(px(76.))
                                        .ramp(ty::BODY)
                                        .text_color(t.ink2)
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .ramp(ty::BODY)
                                        .text_color(t.ink)
                                        .child(value),
                                )
                        },
                    )))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(sp::S))
                            .ramp(ty::SMALL)
                            .text_color(t.ink2)
                            .child(div().pt(px(1.)).child(icon("lock", 13., t.ink2)))
                            .child(
                                "None of your files are sent anywhere. You can remove it later from Modules.",
                            ),
                    ),
            )
            .into_any_element()
    }

    fn done_body(&self, t: Theme) -> AnyElement {
        let Phase::Done {
            report,
            done,
            undone,
        } = &self.phase
        else {
            return div().into_any_element();
        };
        let inputs = report.request.inputs.len();
        let headline = done.headline(inputs);
        let fully_undone = undone.as_ref().is_some_and(|u| u.is_complete());
        let tile = |label: &str, value: String, accent: bool| {
            card(t)
                .flex_1()
                .gap(px(sp::XS))
                .p(px(14.))
                .child(
                    div()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .ramp(ty::DISPLAY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if accent { t.good } else { t.ink })
                        .child(value),
                )
        };
        let outputs: Vec<PathBuf> = done.outputs.iter().take(5).cloned().collect();
        let last = outputs.len().saturating_sub(1);
        let card_body = div()
            .id("result-card")
            .role(Role::Group)
            .aria_label(format!("Result: {headline}"))
            .flex()
            .flex_col()
            .size_full()
            .gap(px(sp::L))
            .p(px(MARGIN))
            .overflow_y_scroll()
            .track_scroll(&self.scroll_result)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(match undone {
                        Some(u) if u.is_complete() => icon("undo-2", 20., t.ink2),
                        Some(_) => icon("triangle-alert", 20., t.warn),
                        None => icon("circle-check", 20., t.good),
                    })
                    .child(
                        div()
                            .ramp(ty::TITLE)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(match undone {
                                Some(u) => u.title().to_string(),
                                None => headline.clone(),
                            }),
                    ),
            )
            .when_some(done.after_bytes.filter(|_| !fully_undone), |d, after| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(sp::M))
                        .child(tile("Before", format_bytes(done.before_bytes), false))
                        .child(icon("arrow-right", 16., t.ink3))
                        .child(tile(
                            "After",
                            format_bytes(after),
                            after < done.before_bytes,
                        )),
                )
            })
            .when_some(undone.clone(), |d, u| {
                d.child(
                    div().flex().flex_col().gap(px(sp::XS)).children(
                        u.lines()
                            .into_iter()
                            .map(|l| div().ramp(ty::BODY).text_color(t.ink2).child(l)),
                    ),
                )
            })
            .children(
                done.notes
                    .iter()
                    .filter(|n| !n.contains(&headline) && !n.contains(": wrote "))
                    .take(6)
                    .map(|n| div().ramp(ty::BODY).text_color(t.ink2).child(n.clone())),
            )
            .when(!outputs.is_empty() && !fully_undone, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(sp::S))
                        .child(caption("Written next to the original", t))
                        .child(card(t).children(outputs.iter().enumerate().map(|(n, p)| {
                            let name = p
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.))
                                .min_h(px(40.))
                                .px(px(14.))
                                .when(n != last, |d| d.border_b_1().border_color(t.hair))
                                .child(icon("file", 14., t.ink2))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .whitespace_nowrap()
                                        .text_ellipsis_middle()
                                        .ramp(ty::BODY)
                                        .text_color(t.ink)
                                        .child(name),
                                )
                        }))),
                )
            })
            .when(done.outputs.len() > 5, |d| {
                d.child(
                    div()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(format!("and {} more", done.outputs.len() - 5)),
                )
            })
            .child(div().ramp(ty::SMALL).text_color(t.ink3).child(format!(
                "Done in {:.1} s. Processed on this computer.",
                report.elapsed.as_secs_f32()
            )));
        div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .child(card_body)
            .children(scroll_thumb(&self.scroll_result, t))
            .into_any_element()
    }

    fn failed_body(&self, t: Theme) -> AnyElement {
        let Phase::Failed(f) = &self.phase else {
            return div().into_any_element();
        };
        div()
            .id("failure")
            .role(Role::Alert)
            .aria_label(format!("{} {}", f.message, f.next_step))
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(sp::S))
            .px(px(50.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .size(px(44.))
                    .rounded(px(11.))
                    .bg(t.warn.opacity(0.12))
                    .child(icon("triangle-alert", 22., t.warn)),
            )
            .child(
                div()
                    .pt(px(sp::XS))
                    .ramp(ty::TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_center()
                    .text_color(t.ink)
                    .child("That did not work"),
            )
            .child(
                div()
                    .max_w(px(480.))
                    .ramp(ty::BODY)
                    .text_center()
                    .text_color(t.ink)
                    .child(f.message.clone()),
            )
            .child(
                div()
                    .max_w(px(480.))
                    .ramp(ty::BODY)
                    .text_center()
                    .text_color(t.ink2)
                    .child(f.next_step.clone()),
            )
            .into_any_element()
    }
}

impl Render for ToolPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(is_dark(window));
        let header = self.header(t, cx);
        let body = match &self.phase {
            Phase::Options => self.options_body(t, cx),
            Phase::Running { .. } => self.running_body(t),
            Phase::Consent { .. } => self.consent_body(t),
            Phase::Done { .. } => self.done_body(t),
            Phase::Failed(_) => self.failed_body(t),
        };
        let footer = self.footer(t, cx);
        let _ = display;
        div()
            .id("tool-panel")
            .key_context("ToolPanel")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|p, ev: &KeyDownEvent, window, cx| p.on_key(ev, window, cx)))
            .on_action(cx.listener(|p, _: &gpui_base::input::Undo, _, cx| p.undo(cx)))
            .on_action(
                cx.listener(|_, _: &super::live::FocusNextControl, window, cx| {
                    window.focus_next(cx)
                }),
            )
            .on_action(
                cx.listener(|_, _: &super::live::FocusPrevControl, window, cx| {
                    window.focus_prev(cx)
                }),
            )
            .on_drop::<ExternalPaths>(cx.listener(|p, paths: &ExternalPaths, _, cx| {
                if p.is_options() {
                    p.add_files(paths.paths().to_vec(), cx);
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .text_color(t.ink)
            .child(header)
            .child(rule(t))
            .child(body)
            .child(footer)
    }
}

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
use super::theme::{Theme, is_dark, rgb8, tint};
use super::widgets::{
    ButtonKind, button, editor_style, icon, icon_button, keycap, progress_bar, segmented, switch,
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
        match inputs.values().next() {
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
        match (&self.phase, self.inputs.values().next()) {
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

    /// Command+Z on a result card: the new files go to the Trash.
    pub(crate) fn undo(&mut self, cx: &mut Context<Self>) {
        let Phase::Done { done, undone, .. } = &self.phase else {
            return;
        };
        if undone.is_some() || done.outputs.is_empty() {
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
                "enter" if m.platform => self.chain(cx),
                "enter" => self.open_first(cx),
                "z" if m.platform => self.undo(cx),
                "r" if m.platform => self.reveal(cx),
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

impl ToolPanel {
    fn label(text: &str, t: Theme) -> Div {
        div()
            .text_size(px(12.))
            .line_height(px(16.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(t.ink2)
            .child(text.to_string())
    }

    fn field_view(&mut self, f: &Field, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let key = f.key.clone();
        let this = cx.entity().downgrade();
        let control: AnyElement = match &f.control {
            Control::Choice(values) => {
                let sel = self
                    .form
                    .value(&key)
                    .and_then(|v| v.as_str().map(str::to_string));
                let focus = self
                    .field_focus
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| cx.focus_handle());
                let k2 = key.clone();
                let this2 = this.clone();
                let k3 = key.clone();
                // A choice with no default starts on "Auto": the module picks for each file.
                let with_auto = f.default.is_none();
                let mut shown: Vec<String> = values.clone();
                if with_auto {
                    shown.insert(0, AUTO.to_string());
                }
                let sel = if with_auto && sel.is_none() {
                    Some(AUTO.to_string())
                } else {
                    sel
                };
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
                let boxed = div()
                    .id(format!("input-{key}"))
                    .role(Role::TextInput)
                    .aria_label(f.label.clone())
                    .aria_value(raw)
                    .track_focus(&focus)
                    .flex_1()
                    .min_w_0()
                    .h(px(32.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .rounded(px(8.))
                    .bg(t.field)
                    .border_1()
                    .border_color(if has_err { t.bad.opacity(0.7) } else { t.hair })
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.ink)
                    .overflow_hidden()
                    .child(
                        div()
                            .w_full()
                            .h(px(18.))
                            .overflow_hidden()
                            .child(KitInput::new(&input)),
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
                            t,
                            cx.listener(move |p, _, window, cx| {
                                p.step_number(&k_minus, -1.0, window, cx)
                            }),
                        ))
                        .child(icon_button(
                            format!("plus-{key}"),
                            &format!("Increase {}", f.label),
                            "plus",
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
            .when(show_label, |d| d.child(Self::label(&f.label, t)))
            .child(control)
            .when_some(self.form.error(&f.key).map(str::to_string), |d, e| {
                d.child(
                    div()
                        .flex()
                        .gap(px(6.))
                        .items_start()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(t.bad)
                        .child(icon("triangle-alert", 13., t.bad))
                        .child(e),
                )
            })
            .when_some(
                f.help.clone().filter(|_| self.form.error(&f.key).is_none()),
                |d, h| {
                    d.child(
                        div()
                            .text_size(px(11.5))
                            .line_height(px(15.))
                            .text_color(t.ink3)
                            .child(h),
                    )
                },
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
        let cat = rgb8(tint(self.tool.category, t.dark), 1.0);
        let rows = self.files.iter().enumerate().map(|(i, f)| {
            div()
                .id(("file", i))
                .role(Role::ListItem)
                .aria_label(format!("{}, {}", f.name, format_bytes(f.bytes)))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .h(px(40.))
                .px(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .size(px(26.))
                        .rounded(px(6.))
                        .bg(if bad[i] {
                            t.bad.opacity(0.14)
                        } else {
                            cat.opacity(0.16)
                        })
                        .child(icon(
                            if bad[i] { "triangle-alert" } else { "file" },
                            14.,
                            if bad[i] { t.bad } else { cat },
                        )),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(f.name.clone()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(t.ink3)
                        .child(format!("{}  {}", f.kind_label(), format_bytes(f.bytes))),
                )
                .child(icon_button(
                    format!("remove-{i}"),
                    &format!("Remove {}", f.name),
                    "x",
                    t,
                    cx.listener(move |p, _, _, cx| p.remove_file(i, cx)),
                ))
                .into_any_element()
        });
        let empty = self.files.is_empty();
        let takes = self.tool.takes_files();
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(10.))
            .child(Self::label(
                if takes {
                    "Files"
                } else {
                    "This tool needs no file"
                },
                t,
            ))
            .when(takes, |d| {
                d.child(
                    div()
                        .id("files-card")
                        .role(Role::List)
                        .aria_label("Files")
                        .flex()
                        .flex_col()
                        .rounded(px(12.))
                        .bg(t.card)
                        .border_1()
                        .border_color(t.hair)
                        .overflow_hidden()
                        .when(empty, |d| {
                            d.child(
                                div()
                                    .h(px(96.))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .justify_center()
                                    .gap(px(6.))
                                    .child(icon("upload", 20., t.ink3))
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .text_color(t.ink2)
                                            .child("Drop files here"),
                                    ),
                            )
                        })
                        .children(rows),
                )
                .child(div().flex().flex_row().gap(px(8.)).child(button(
                    "add-files",
                    if empty { "Choose files" } else { "Add files" },
                    ButtonKind::Secondary,
                    Some("plus"),
                    t,
                    cx.listener(|p, _, _, cx| p.choose_files(cx)),
                )))
            })
            .into_any_element()
    }

    fn header(&self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let network = self.tool.manifest.capabilities.network;
        let privacy = if network {
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
            "Processed on this computer. Nothing uploaded.".to_string()
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(52.))
            .w_full()
            .px(px(14.))
            .gap(px(10.))
            .child(icon_button(
                "panel-back",
                "Back to all tools",
                "arrow-left",
                t,
                cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
            ))
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(t.ink3)
                    .child(self.tool.category.label()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(13.))
                    .text_color(t.ink3)
                    .child("/"),
            )
            .child(
                div()
                    .id("panel-title")
                    .role(Role::Heading)
                    .aria_label(self.tool.name.clone())
                    .flex_none()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child(self.tool.name.clone()),
            )
            .child(div().flex_1())
            .child(icon(
                if network { "triangle-alert" } else { "lock" },
                13.,
                if network { t.warn } else { t.ink3 },
            ))
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
                    .text_color(t.ink3)
                    .child(privacy),
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
            .flex_none()
            .w(px(330.))
            .h_full()
            .gap(px(16.))
            .p(px(16.))
            .border_l_1()
            .border_color(t.hair)
            .overflow_y_scroll()
            .child(Self::label("Options", t))
            .when(none, |d| {
                d.child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.ink3)
                        .child("This tool has no options. It runs with the best settings for each file."),
                )
            })
            .children(primary_views)
            .when(n_more > 0, |d| {
                d.child(
                    div()
                        .id("more-options")
                        .role(Role::DisclosureTriangle)
                        .aria_label("More options")
                        .aria_expanded(more_open)
                        .tab_stop(true)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .cursor_pointer()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink2)
                        .on_click(cx.listener(|p, _, _, cx| {
                            p.more_open = !p.more_open;
                            cx.notify();
                        }))
                        .child(icon(if more_open { "chevron-down" } else { "chevron-right" }, 13., t.ink2))
                        .child(format!("More options ({n_more})")),
                )
            })
            .children(more_views);
        let blocker = self.blocker();
        let estimate = self.estimate.clone();
        let left = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(14.))
            .p(px(16.))
            .child(self.files_card(t, cx))
            .when_some(estimate, |d, e| {
                d.child(
                    div()
                        .id("estimate")
                        .role(Role::Status)
                        .aria_label(format!("Estimate: {e}"))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .h(px(36.))
                        .px(px(12.))
                        .rounded(px(10.))
                        .bg(t.good.opacity(0.10))
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.good)
                        .child(icon("circle-check", 14., t.good))
                        .child(format!("Expected: {e}")),
                )
            })
            .when_some(
                blocker.filter(|_| !self.files.is_empty() || !self.tool.takes_files()),
                |d, b| {
                    d.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(px(8.))
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .text_color(t.warn)
                            .child(icon("triangle-alert", 14., t.warn))
                            .child(b),
                    )
                },
            );
        div()
            .flex()
            .flex_row()
            .flex_1()
            .min_h_0()
            .child(left)
            .child(options)
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
                        div()
                            .id("run")
                            .opacity(if can_run { 1.0 } else { 0.45 })
                            .child(button(
                                "run-button",
                                "Run",
                                ButtonKind::Primary,
                                None,
                                t,
                                cx.listener(|p, _, window, cx| p.run(window, cx)),
                            )),
                    )
                    .child(keycap("\u{21B5}", false, t))
                    .into_any_element(),
            ),
            Phase::Running { .. } => (
                "Cancel is instant and leaves nothing half-written.".into(),
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
                    .child(keycap("\u{21B5}", false, t))
                    .into_any_element(),
            ),
            Phase::Done { done, undone, .. } => {
                let has_out = !done.outputs.is_empty();
                let undone_now = undone.is_some();
                (
                    "Return opens it. \u{2318}Z undoes.".into(),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .when(has_out && !undone_now, |d| {
                            d.child(button(
                                "show-folder",
                                "Show in folder",
                                ButtonKind::Secondary,
                                Some("folder-open"),
                                t,
                                cx.listener(|p, _, _, cx| p.reveal(cx)),
                            ))
                            .child(button(
                                "open-result",
                                "Open",
                                ButtonKind::Primary,
                                Some("external-link"),
                                t,
                                cx.listener(|p, _, _, cx| p.open_first(cx)),
                            ))
                            .child(button(
                                "then",
                                "Then\u{2026}",
                                ButtonKind::Secondary,
                                Some("arrow-right"),
                                t,
                                cx.listener(|p, _, _, cx| p.chain(cx)),
                            ))
                            .child(button(
                                "undo",
                                "Undo",
                                ButtonKind::Ghost,
                                Some("undo-2"),
                                t,
                                cx.listener(|p, _, _, cx| p.undo(cx)),
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
                        .child(button(
                            "done",
                            "Done",
                            ButtonKind::Secondary,
                            None,
                            t,
                            cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
                        ))
                        .into_any_element(),
                )
            }
            Phase::Failed(_) => (
                "Nothing was changed.".into(),
                div()
                    .flex()
                    .gap(px(8.))
                    .child(button(
                        "again",
                        "Try again",
                        ButtonKind::Primary,
                        None,
                        t,
                        cx.listener(|p, _, window, cx| p.again(window, cx)),
                    ))
                    .child(button(
                        "close-failed",
                        "Close",
                        ButtonKind::Secondary,
                        None,
                        t,
                        cx.listener(|_, _, _, cx| cx.emit(PanelEvent::Close)),
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
            .px(px(16.))
            .gap(px(12.))
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .top_0()
                    .h(px(0.5))
                    .bg(t.fill8),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(12.))
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
            .flex()
            .flex_col()
            .flex_1()
            .items_center()
            .justify_center()
            .gap(px(14.))
            .px(px(60.))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child(format!("Running {}", self.tool.name)),
            )
            .when(show, |d| {
                d.child(div().w_full().max_w(px(420.)).child(progress_bar(
                    Some(*fraction),
                    label,
                    t,
                )))
                .child(
                    div().text_size(px(12.5)).text_color(t.ink3).child(format!(
                        "{}  {:.0}%",
                        label,
                        fraction * 100.0
                    )),
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
        let row = |label: &str, value: String| {
            div()
                .flex()
                .flex_row()
                .gap(px(12.))
                .child(
                    div()
                        .flex_none()
                        .w(px(84.))
                        .text_size(px(13.))
                        .text_color(t.ink3)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.ink)
                        .child(value),
                )
        };
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
            .items_center()
            .justify_center()
            .px(px(40.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(14.))
                    .w_full()
                    .max_w(px(440.))
                    .child(icon("triangle-alert", 22., t.warn))
                    .child(
                        div()
                            .text_size(px(16.))
                            .line_height(px(22.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(format!("Download the {}?", info.name)),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .line_height(px(18.))
                            .text_color(t.ink2)
                            .child(format!(
                                "{} needs it to work. It is fetched once and kept on this computer.",
                                self.tool.name
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(row("What for", info.purpose.clone()))
                            .child(row("Size", format!("{}, once", info.size_label())))
                            .child(row("From", info.source.clone()))
                            .child(row("License", info.license.clone())),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .text_color(t.ink3)
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
        let tile = |label: &str, value: String, accent: bool| {
            div()
                .flex()
                .flex_col()
                .flex_1()
                .gap(px(4.))
                .p(px(14.))
                .rounded(px(12.))
                .bg(t.card)
                .border_1()
                .border_color(t.hair)
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.ink3)
                        .child(label.to_string()),
                )
                .child(
                    div()
                        .text_size(px(22.))
                        .line_height(px(26.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if accent { t.good } else { t.ink })
                        .child(value),
                )
        };
        div()
            .id("result-card")
            .role(Role::Group)
            .aria_label(format!("Result: {headline}"))
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .gap(px(14.))
            .p(px(20.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(icon("circle-check", 20., t.good))
                    .child(
                        div()
                            .text_size(px(17.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(t.ink)
                            .child(match undone {
                                Some(_) => "Undone".to_string(),
                                None => headline.clone(),
                            }),
                    ),
            )
            .when_some(done.after_bytes.filter(|_| undone.is_none()), |d, after| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(12.))
                        .child(tile("Before", format_bytes(done.before_bytes), false))
                        .child(tile(
                            "After",
                            format_bytes(after),
                            after < done.before_bytes,
                        )),
                )
            })
            .when_some(undone.clone(), |d, u| {
                d.child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(18.))
                        .text_color(t.ink2)
                        .child(if u.failed.is_empty() {
                            "The new files went to the Trash. Your originals were never touched."
                                .to_string()
                        } else {
                            format!("Could not remove: {}", u.failed.join(", "))
                        }),
                )
            })
            .children(
                done.notes
                    .iter()
                    .filter(|n| !n.contains(&headline) && !n.contains(": wrote "))
                    .take(6)
                    .map(|n| {
                        div()
                            .text_size(px(12.5))
                            .line_height(px(17.))
                            .text_color(t.ink2)
                            .child(n.clone())
                    }),
            )
            .when(!done.outputs.is_empty() && undone.is_none(), |d| {
                d.child(Self::label("Written next to the original", t))
                    .children(done.outputs.iter().take(5).map(|p| {
                        div()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis_middle()
                            .text_size(px(12.5))
                            .text_color(t.ink)
                            .child(
                                p.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            )
                    }))
            })
            .when(done.outputs.len() > 5, |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(t.ink3)
                        .child(format!("and {} more", done.outputs.len() - 5)),
                )
            })
            .child(div().text_size(px(12.)).text_color(t.ink3).child(format!(
                "Done in {:.1} s. Processed on this computer.",
                report.elapsed.as_secs_f32()
            )))
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
            .gap(px(10.))
            .px(px(50.))
            .child(icon("triangle-alert", 26., t.warn))
            .child(
                div()
                    .text_size(px(15.))
                    .line_height(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_center()
                    .text_color(t.ink)
                    .child(f.message.clone()),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
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
            .child(div().h(px(0.5)).w_full().bg(t.fill8))
            .child(body)
            .child(footer)
    }
}

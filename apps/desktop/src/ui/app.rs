//! The app window: sidebar of categories (counts from the registry), the home screen from the
//! wireframe (search, drop area, quick actions, recent jobs), category and module lists, history,
//! and the same tool panel the bar uses.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::*;
use gpui_base::input::{Input as KitInput, InputEvent, InputState};

use super::Hub;
use super::panel::{PanelEvent, ToolPanel};
use super::theme::{Theme, is_dark, rgb8, tint};
use super::widgets::{editor_style, icon, keycap};
use crate::catalog::{Category, Source};
use crate::history::Entry;
use crate::profile::{FileProfile, expand_paths, files_label};
use crate::search::{self, Action, Results};
use crate::units::{format_bytes, now_secs, relative_time};

pub(crate) const APP_WIDTH: f32 = 1160.0;
pub(crate) const APP_HEIGHT: f32 = 720.0;
const SIDEBAR: f32 = 232.0;

/// What the main area shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Route {
    Home,
    Find,
    History,
    Nearby,
    Modules,
    Category(Category),
    /// Tools for a search or for dropped files.
    Results,
    /// A tool's panel.
    Tool,
}

/// The app window view.
pub(crate) struct AppView {
    hub: Hub,
    pub(crate) route: Route,
    focus: FocusHandle,
    search: Entity<InputState>,
    query: String,
    files: Vec<FileProfile>,
    results: Results,
    selected: usize,
    pub(crate) panel: Option<Entity<ToolPanel>>,
    _panel_sub: Option<Subscription>,
    drag_active: bool,
    /// The downloadable models and engines, read from disk when the Modules page opens.
    downloads: Vec<crate::downloads::Row>,
    /// The last thing Remove did, in words.
    downloads_note: Option<String>,
    _subs: Vec<Subscription>,
}

impl AppView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>, hub: Hub) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx).placeholder(format!(
                "Search {} tools and files",
                hub.catalog.tools().len()
            ))
        });
        let search_sub = cx.subscribe_in(
            &search,
            window,
            |app, input, ev: &InputEvent, window, cx| match ev {
                InputEvent::Change => {
                    let v = input.read(cx).value().to_string();
                    app.set_query(v, cx);
                }
                InputEvent::PressEnter { .. } => app.activate(window, cx),
                _ => {}
            },
        );
        let recents_sub = cx.observe(&hub.recents, |_, _, cx| cx.notify());
        let entity = cx.entity();
        let appearance = window.observe_window_appearance(move |_, cx| {
            entity.update(cx, |_, cx| cx.notify());
        });
        Self {
            hub,
            route: Route::Home,
            focus: cx.focus_handle(),
            search,
            query: String::new(),
            files: Vec::new(),
            results: Results::default(),
            selected: 0,
            panel: None,
            _panel_sub: None,
            drag_active: false,
            downloads: Vec::new(),
            downloads_note: None,
            _subs: vec![search_sub, recents_sub, appearance],
        }
    }

    fn recents(&self, cx: &App) -> Vec<Entry> {
        self.hub.recents.read(cx).entries.clone()
    }

    fn rebuild(&mut self, cx: &App) {
        let history = self.recents(cx);
        self.results = search::search(
            &search::Context {
                catalog: &self.hub.catalog,
                files: &self.files,
                history: &history,
                now: now_secs(),
            },
            &self.query,
        );
        self.selected = 0;
    }

    pub(crate) fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        self.panel = None;
        self._panel_sub = None;
        self.route = route;
        if route == Route::Modules {
            // Reading the disk once on arrival keeps drawing free.
            self.downloads = crate::downloads::rows();
            self.downloads_note = None;
        }
        cx.notify();
    }

    fn remove_download(&mut self, id: &str, cx: &mut Context<Self>) {
        self.downloads_note = Some(match crate::downloads::remove(id) {
            Ok(line) | Err(line) => line,
        });
        self.downloads = crate::downloads::rows();
        cx.notify();
    }

    pub(crate) fn set_query(&mut self, q: String, cx: &mut Context<Self>) {
        self.query = q;
        if self.query.trim().is_empty() && self.files.is_empty() {
            if self.route == Route::Results {
                self.route = Route::Home;
            }
        } else {
            self.rebuild(cx);
            self.panel = None;
            self.route = Route::Results;
        }
        cx.notify();
    }

    /// Files dropped on the window or chosen: show the tools that take them.
    pub(crate) fn set_files(&mut self, files: Vec<FileProfile>, cx: &mut Context<Self>) {
        self.files = files;
        if self.files.is_empty() && self.query.trim().is_empty() {
            self.route = Route::Home;
        } else {
            self.rebuild(cx);
            self.panel = None;
            self.route = Route::Results;
        }
        cx.notify();
    }

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
            let _ = this.update(cx, |app, cx| app.set_files(files, cx));
        })
        .detach();
    }

    /// Files handed to the app from outside (Dock drop, "open with", command line).
    pub(crate) fn open_files(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.dropped(paths, cx);
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
                let _ = this.update(cx, |app, cx| app.dropped(paths, cx));
            }
        })
        .detach();
    }

    /// Open a tool's panel in the main area.
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
            |app, _, ev: &PanelEvent, window, cx| match ev {
                PanelEvent::Close => {
                    app.panel = None;
                    app._panel_sub = None;
                    app.route = if app.query.trim().is_empty() && app.files.is_empty() {
                        Route::Home
                    } else {
                        Route::Results
                    };
                    window.focus(&app.focus, cx);
                    cx.notify();
                }
                PanelEvent::Chain(paths) => {
                    app.query.clear();
                    app.panel = None;
                    app._panel_sub = None;
                    app.route = Route::Home;
                    app.dropped(paths.clone(), cx);
                }
                PanelEvent::Ran => {}
            },
        );
        self.panel = Some(panel);
        self._panel_sub = Some(sub);
        self.route = Route::Tool;
        cx.notify();
    }

    fn open_action(&mut self, action: Action, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            Action::OpenTool { tool, preset } => {
                let files = self.files.clone();
                self.open_tool(tool, files, preset, true, window, cx);
            }
            Action::ReRun { entry } => {
                let Some(e) = self.recents(cx).get(entry).cloned() else {
                    return;
                };
                self.rerun(&e, window, cx);
            }
            Action::Unavailable(_) => {}
        }
    }

    fn rerun(&mut self, e: &Entry, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tool) = self.hub.catalog.index_of(&e.tool_id) else {
            return;
        };
        let preset = e
            .options
            .as_object()
            .map(|o| o.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let inputs: Vec<PathBuf> = e.inputs.iter().filter(|p| p.exists()).cloned().collect();
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
            let _ = this.update_in(cx, |app, window, cx| {
                app.open_tool(tool, files, preset, true, window, cx)
            });
        })
        .detach();
    }

    fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.route != Route::Results {
            return;
        }
        if let Some(row) = self.results.get(self.selected).cloned() {
            self.open_action(row.action, window, cx);
        }
    }

    fn on_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.panel.is_some() {
            return;
        }
        let k = ev.keystroke.key.as_str();
        let m = ev.keystroke.modifiers;
        match k {
            "down" if self.route == Route::Results => {
                let n = self.results.len();
                if n > 0 {
                    self.selected = (self.selected + 1).min(n - 1);
                    cx.notify();
                }
            }
            "up" if self.route == Route::Results => {
                self.selected = self.selected.saturating_sub(1);
                cx.notify();
            }
            "escape" => {
                if !self.query.is_empty() || !self.files.is_empty() {
                    self.query.clear();
                    self.files.clear();
                    self.search.update(cx, |s, cx| s.set_value("", window, cx));
                    self.go(Route::Home, cx);
                }
            }
            "f" if m.platform => {
                let f = self.search.read(cx).focus_handle(cx);
                window.focus(&f, cx);
            }
            "tab" => {
                if m.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            }
            _ => {}
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn side_item(
    id: impl Into<ElementId>,
    label: &str,
    icon_name: &str,
    tint_color: Hsla,
    count: Option<usize>,
    active: bool,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::ListBoxOption)
        .aria_label(match count {
            Some(n) => format!("{label}, {n} tools"),
            None => label.to_string(),
        })
        .aria_selected(active)
        .tab_stop(true)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .h(px(34.))
        .px(px(12.))
        .rounded(px(8.))
        .cursor_pointer()
        .when(active, |d| d.bg(t.fill12))
        .hover(move |s| s.bg(t.fill8))
        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
        .on_click(on_click)
        .child(icon(icon_name, 16., tint_color))
        .child(
            div()
                .flex_1()
                .text_size(px(13.5))
                .line_height(px(18.))
                .font_weight(if active {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .text_color(t.ink)
                .child(label.to_string()),
        )
        .when_some(count, |d, n| {
            d.child(
                div()
                    .text_size(px(12.))
                    .text_color(t.ink3)
                    .child(n.to_string()),
            )
        })
}

fn section_label(text: &str, t: Theme) -> Div {
    div()
        .px(px(12.))
        .pt(px(14.))
        .pb(px(4.))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.ink3)
        .child(text.to_string())
}

fn tile_icon(c: Category, name: &str, size: f32, t: Theme) -> Div {
    let tc = tint(c, t.dark);
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size / 4.0))
        .bg(rgb8(tc, 0.16))
        .child(icon(name, size * 0.52, rgb8(tc, 1.0)))
}

impl AppView {
    fn sidebar(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let counts = self.hub.catalog.category_counts();
        let route = self.route;
        let go = |r: Route| cx.listener(move |app, _: &ClickEvent, _, cx| app.go(r, cx));
        div()
            .id("sidebar")
            .role(Role::ListBox)
            .aria_label("Sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR))
            .h_full()
            .pt(px(52.))
            .px(px(10.))
            .pb(px(12.))
            .bg(t.sidebar)
            .border_r_1()
            .border_color(t.hair)
            .child(side_item(
                "nav-home",
                "Home",
                "house",
                t.ink2,
                None,
                route == Route::Home,
                t,
                go(Route::Home),
            ))
            .child(side_item(
                "nav-find",
                "Find",
                "search",
                t.ink2,
                None,
                route == Route::Find,
                t,
                go(Route::Find),
            ))
            .child(side_item(
                "nav-history",
                "History",
                "history",
                t.ink2,
                None,
                route == Route::History,
                t,
                go(Route::History),
            ))
            .child(section_label("Tools", t))
            .children(counts.into_iter().map(|(c, n)| {
                side_item(
                    format!("nav-{}", c.label()),
                    c.label(),
                    c.icon(),
                    rgb8(tint(c, t.dark), 1.0),
                    Some(n),
                    route == Route::Category(c),
                    t,
                    go(Route::Category(c)),
                )
            }))
            .child(section_label("Panoply", t))
            .child(side_item(
                "nav-nearby",
                "Nearby",
                "radio",
                t.ink2,
                None,
                route == Route::Nearby,
                t,
                go(Route::Nearby),
            ))
            .child(side_item(
                "nav-modules",
                "Modules",
                "layout-grid",
                t.ink2,
                None,
                route == Route::Modules,
                t,
                go(Route::Modules),
            ))
            .into_any_element()
    }

    fn search_box(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        self.search
            .update(cx, |s, _| s.set_editor_style(editor_style(t)));
        let focus = self.search.read(cx).focus_handle(cx);
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .h(px(46.))
            .px(px(16.))
            .gap(px(12.))
            .rounded(px(12.))
            .bg(t.fill8)
            .child(icon("search", 16., t.ink3))
            .child(
                div()
                    .id("app-search")
                    .role(Role::SearchInput)
                    .aria_label("Search tools and files")
                    .aria_value(self.query.clone())
                    .track_focus(&focus)
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(t.ink)
                    .child(KitInput::new(&self.search)),
            )
            .child(
                div().flex().gap(px(4.)).children(
                    super::live::hotkey_caps()
                        .iter()
                        .map(|c| keycap(c, false, t).into_any_element()),
                ),
            )
            .into_any_element()
    }

    fn drop_area(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let active = self.drag_active;
        div()
            .id("drop-area")
            .role(Role::Button)
            .aria_label("Drop anything here, or choose files")
            .tab_stop(true)
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(px(190.))
            .rounded(px(16.))
            .bg(if active {
                t.accent.opacity(0.10)
            } else {
                t.fill8
            })
            .border_1()
            .border_color(if active {
                t.accent.opacity(0.6)
            } else {
                t.hair
            })
            .cursor_pointer()
            .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
            .on_click(cx.listener(|app, _, _, cx| app.choose_files(cx)))
            .child(icon("upload", 26., t.accent))
            .child(
                div()
                    .text_size(px(17.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("Drop anything here"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(t.ink3)
                    .child("PDFs, photos, videos, documents, folders. It stays on this computer."),
            )
            .into_any_element()
    }

    fn quick_actions(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let recent_ids: Vec<String> = {
            let mut v: Vec<String> = Vec::new();
            for e in self.recents(cx) {
                if !v.contains(&e.tool_id) {
                    v.push(e.tool_id);
                }
            }
            v
        };
        let mut picks: Vec<usize> = recent_ids
            .iter()
            .filter_map(|id| self.hub.catalog.index_of(id))
            .take(3)
            .collect();
        for (i, tool) in self.hub.catalog.tools().iter().enumerate() {
            if picks.len() >= 6 {
                break;
            }
            if tool.on_bar()
                && tool.takes_files()
                && !picks.contains(&i)
                && !matches!(tool.pattern, panoply_manifest::Pattern::Inspect)
            {
                picks.push(i);
            }
        }
        picks.truncate(6);
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(12.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("Quick actions"),
            )
            .child(div().flex().flex_row().flex_wrap().gap(px(12.)).children(
                picks.into_iter().map(|i| {
                    let tool = self.hub.catalog.tool(i).clone();
                    div()
                        .id(("quick", i))
                        .role(Role::Button)
                        .aria_label(tool.name.clone())
                        .tab_stop(true)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.))
                        .w(px(196.))
                        .h(px(60.))
                        .px(px(14.))
                        .rounded(px(12.))
                        .bg(t.card)
                        .border_1()
                        .border_color(t.hair)
                        .cursor_pointer()
                        .hover(move |s| s.bg(t.fill8))
                        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
                        .on_click(cx.listener(move |app, _, window, cx| {
                            let files = app.files.clone();
                            app.open_tool(i, files, Vec::new(), true, window, cx);
                        }))
                        .child(tile_icon(tool.category, tool.category.icon(), 32., t))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(tool.name.clone()),
                        )
                }),
            ))
            .into_any_element()
    }

    fn recent_list(&mut self, t: Theme, limit: usize, cx: &mut Context<Self>) -> AnyElement {
        let now = now_secs();
        let entries: Vec<Entry> = self.recents(cx).into_iter().take(limit).collect();
        let empty = entries.is_empty();
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(px(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("Recent"),
            )
            .when(empty, |d| {
                d.child(
                    div()
                        .text_size(px(13.))
                        .text_color(t.ink3)
                        .py(px(10.))
                        .child(
                            "Nothing yet. Drop a file above and run a tool, and it appears here.",
                        ),
                )
            })
            .children(entries.into_iter().enumerate().map(|(n, e)| {
                let category = Category::from_module_id(&e.tool_id);
                let reveal_path = e
                    .outputs
                    .first()
                    .cloned()
                    .or_else(|| e.inputs.first().cloned());
                let e2 = e.clone();
                div()
                    .id(("recent", n))
                    .role(Role::ListItem)
                    .aria_label(format!("{}, {}", e.tool_name, e.headline))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(14.))
                    .h(px(56.))
                    .border_b_1()
                    .border_color(t.hair)
                    .child(tile_icon(category, category.icon(), 32., t))
                    .child(
                        div()
                            .w(px(220.))
                            .flex_none()
                            .text_size(px(13.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink)
                            .child(match e.inputs.as_slice() {
                                [one] => one
                                    .file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                                many => format!("{} files", many.len()),
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_size(px(13.))
                            .text_color(t.ink3)
                            .child(format!("{}. {}", e.tool_name, e.headline)),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.5))
                            .text_color(t.ink3)
                            .child(relative_time(now, e.at)),
                    )
                    .child(
                        div()
                            .id(("recent-show", n))
                            .role(Role::Link)
                            .aria_label("Show in folder")
                            .tab_stop(true)
                            .flex_none()
                            .text_size(px(13.))
                            .text_color(t.accent)
                            .cursor_pointer()
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if let Some(p) = &reveal_path {
                                    cx.reveal_path(p);
                                }
                            }))
                            .child("Show in folder"),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |app, ev: &MouseDownEvent, window, cx| {
                            if ev.click_count >= 2 {
                                app.rerun(&e2, window, cx);
                            }
                        }),
                    )
            }))
            .into_any_element()
    }

    fn home(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search_box(t, cx);
        let drop = self.drop_area(t, cx);
        let quick = self.quick_actions(t, cx);
        let recent = self.recent_list(t, 5, cx);
        div()
            .id("home")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(24.))
            .p(px(24.))
            .overflow_y_scroll()
            .child(search)
            .child(drop)
            .child(
                div().flex().flex_row().gap(px(28.)).child(quick).child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_none()
                        .w(px(300.))
                        .gap(px(12.))
                        .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).text_color(t.ink).child("Nearby"))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(6.))
                                .p(px(16.))
                                .rounded(px(12.))
                                .bg(t.card)
                                .border_1()
                                .border_color(t.hair)
                                .child(div().text_size(px(13.5)).font_weight(FontWeight::MEDIUM).text_color(t.ink).child("No devices yet"))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .line_height(px(17.))
                                        .text_color(t.ink3)
                                        .child("Sending to your phone arrives with sharing, a later phase."),
                                ),
                        ),
                ),
            )
            .child(recent)
            .into_any_element()
    }

    fn row_list(
        &mut self,
        rows: Vec<(Action, String, String, String, &'static str, Category)>,
        t: Theme,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(rows.into_iter().enumerate().map(
                move |(n, (action, title, subtitle, trailing, icon_name, category))| {
                    let selected = self.route == Route::Results && n == self.selected;
                    div()
                        .id(("result-row", n))
                        .role(Role::ListBoxOption)
                        .aria_label(title.clone())
                        .aria_description(subtitle.clone())
                        .aria_selected(selected)
                        .tab_stop(true)
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(14.))
                        .h(px(52.))
                        .px(px(12.))
                        .rounded(px(10.))
                        .cursor_pointer()
                        .when(selected, |d| d.bg(t.fill12))
                        .hover(move |s| s.bg(t.fill8))
                        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
                        .on_click(cx.listener(move |app, _, window, cx| {
                            app.open_action(action.clone(), window, cx)
                        }))
                        .child(tile_icon(category, icon_name, 30., t))
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(t.ink)
                                .child(title),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(px(12.5))
                                .text_color(t.ink2)
                                .child(subtitle),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(t.ink3)
                                .child(trailing),
                        )
                },
            ))
    }

    fn page(&self, title: &str, subtitle: Option<String>, t: Theme) -> Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(14.))
            .p(px(24.))
            .child(
                div()
                    .id("page-title")
                    .role(Role::Heading)
                    .aria_label(title.to_string())
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child(title.to_string()),
            )
            .when_some(subtitle, |d, s| {
                d.child(div().text_size(px(13.)).text_color(t.ink3).child(s))
            })
    }

    fn results_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search_box(t, cx);
        let mut rows = Vec::new();
        let sections: Vec<(String, Vec<crate::search::Row>)> = self
            .results
            .sections
            .iter()
            .map(|s| (s.title.clone(), s.rows.clone()))
            .collect();
        let mut blocks: Vec<AnyElement> = Vec::new();
        for (title, srows) in sections {
            rows.clear();
            for r in srows {
                let sub = if r.chips.is_empty() {
                    r.subtitle.clone()
                } else {
                    format!(
                        "{}   {}",
                        r.chips
                            .iter()
                            .map(|(l, v)| format!("{l} {v}"))
                            .collect::<Vec<_>>()
                            .join(", "),
                        r.estimate.clone().unwrap_or_default()
                    )
                };
                rows.push((
                    r.action.clone(),
                    r.title.clone(),
                    sub,
                    r.trailing.clone(),
                    r.icon,
                    r.category,
                ));
            }
            let list = self.row_list(std::mem::take(&mut rows), t, cx);
            blocks.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink3)
                            .child(title),
                    )
                    .child(list)
                    .into_any_element(),
            );
        }
        let none = self.results.is_empty();
        let files_note = (!self.files.is_empty()).then(|| {
            format!(
                "{}, {}",
                files_label(&self.files),
                format_bytes(self.files.iter().map(|f| f.bytes).sum())
            )
        });
        div()
            .id("results-page")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(16.))
            .p(px(24.))
            .overflow_y_scroll()
            .child(search)
            .when_some(files_note, |d, n| {
                d.child(div().text_size(px(13.)).text_color(t.ink2).child(format!("Files: {n}")))
            })
            .when(none, |d| {
                d.child(div().text_size(px(13.5)).text_color(t.ink3).child("No tools match. Try \u{201C}compress\u{201D}, \u{201C}heic to jpg\u{201D} or drop a file."))
            })
            .children(blocks)
            .when_some(self.results.understood.clone(), |d, u| d.child(div().text_size(px(12.5)).text_color(t.ink3).child(u)))
            .into_any_element()
    }

    fn category_page(&mut self, c: Category, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let idxs = self.hub.catalog.in_category(c);
        let rows: Vec<_> = idxs
            .iter()
            .map(|&i| {
                let tool = self.hub.catalog.tool(i);
                (
                    Action::OpenTool {
                        tool: i,
                        preset: Vec::new(),
                    },
                    tool.name.clone(),
                    tool.summary.clone(),
                    format!("Pattern {}", tool.pattern.letter()),
                    c.icon(),
                    c,
                )
            })
            .collect();
        let list = self.row_list(rows, t, cx);
        self.page(c.label(), Some(format!("{} tools", idxs.len())), t)
            .id("category-page")
            .overflow_y_scroll()
            .child(list)
            .into_any_element()
    }

    fn modules_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let tools = self.hub.catalog.tools().to_vec();
        let community = tools
            .iter()
            .filter(|t| matches!(t.source, Source::Community(_)))
            .count();
        let notes = self.hub.catalog.notes.clone();
        let rows: Vec<_> = tools
            .iter()
            .enumerate()
            .map(|(i, tool)| {
                let badge = match &tool.source {
                    Source::BuiltIn => "Built in",
                    Source::Community(_) => "Community, sandboxed",
                };
                let perms = if tool.manifest.capabilities.network {
                    "Network"
                } else if tool.takes_files() {
                    "Chosen files only"
                } else {
                    "No files"
                };
                (
                    Action::OpenTool {
                        tool: i,
                        preset: Vec::new(),
                    },
                    tool.name.clone(),
                    format!("{}  \u{00B7}  {}", tool.id, perms),
                    badge.to_string(),
                    tool.category.icon(),
                    tool.category,
                )
            })
            .collect();
        let list = self.row_list(rows, t, cx);
        let downloads = self.downloads_section(t, cx);
        self.page("Modules", Some(format!("{} installed, {} from the community. Every module runs with only the files you choose.", tools.len(), community)), t)
            .id("modules-page")
            .overflow_y_scroll()
            .child(list)
            .child(downloads)
            .children(notes.into_iter().map(|n| div().text_size(px(12.)).text_color(t.warn).child(format!("Could not load: {n}"))))
            .into_any_element()
    }

    /// The models and engines Panoply downloaded or may download: size, source, license and a
    /// Remove button for each one that is on this computer.
    fn downloads_section(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.downloads.clone();
        let on_disk = rows.iter().filter(|r| r.installed).count();
        let items: Vec<AnyElement> = rows
            .into_iter()
            .map(|r| {
                let id = r.id.clone();
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .py(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(px(13.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink)
                                    .child(r.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(t.ink3)
                                    .child(format!("{}, {}", r.purpose, r.origin)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.5))
                            .text_color(t.ink2)
                            .child(r.size_text.clone()),
                    )
                    .when(r.installed, |d| {
                        d.child(super::widgets::button(
                            SharedString::from(format!("remove-{id}")),
                            "Remove",
                            super::widgets::ButtonKind::Secondary,
                            None,
                            t,
                            cx.listener(move |app, _, _, cx| app.remove_download(&id, cx)),
                        ))
                    })
                    .into_any_element()
            })
            .collect();
        div()
            .id("downloads")
            .flex()
            .flex_col()
            .gap(px(6.))
            .mt(px(10.))
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("Downloaded models and engines"),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(t.ink3)
                    .child(format!(
                        "Each one is downloaded once, only after you allow it, and used offline. {on_disk} on this computer. Removing one frees the space; Panoply asks again the next time it is needed."
                    )),
            )
            .when_some(self.downloads_note.clone(), |d, n| {
                d.child(div().text_size(px(12.5)).text_color(t.ink2).child(n))
            })
            .children(items)
            .into_any_element()
    }

    fn history_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let list = self.recent_list(t, 64, cx);
        self.page(
            "History",
            Some("Every job, with its files and options. Double-click one to run it again.".into()),
            t,
        )
        .id("history-page")
        .overflow_y_scroll()
        .child(list)
        .into_any_element()
    }

    fn stub_page(&self, title: &str, text: &str, t: Theme) -> AnyElement {
        self.page(title, None, t)
            .child(
                div()
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(t.ink3)
                    .max_w(px(520.))
                    .child(text.to_string()),
            )
            .into_any_element()
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(is_dark(window));
        let sidebar = self.sidebar(t, cx);
        let main: AnyElement = match (self.route, self.panel.clone()) {
            (Route::Tool, Some(p)) => div().flex_1().min_w_0().h_full().child(p).into_any_element(),
            (Route::Home, _) => self.home(t, cx),
            (Route::Results, _) => self.results_page(t, cx),
            (Route::Category(c), _) => self.category_page(c, t, cx),
            (Route::Modules, _) => self.modules_page(t, cx),
            (Route::History, _) => self.history_page(t, cx),
            (Route::Find, _) => self.stub_page(
                "Find",
                "Search inside your files, screenshots and photos arrives with the file index, a later phase. Until then the search box on Home finds tools and takes files.",
                t,
            ),
            (Route::Nearby, _) => self.stub_page(
                "Nearby",
                "Sending to your phone and other computers arrives with LocalSend sharing, a later phase.",
                t,
            ),
            (Route::Tool, None) => self.home(t, cx),
        };
        div()
            .id("app")
            .key_context("App")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|app, ev: &KeyDownEvent, window, cx| app.on_key(ev, window, cx)),
            )
            .on_action(
                cx.listener(|_, _: &super::live::CloseWindow, window, _| window.remove_window()),
            )
            .on_drag_move::<ExternalPaths>(cx.listener(
                |app, _: &DragMoveEvent<ExternalPaths>, _, cx| {
                    if !app.drag_active {
                        app.drag_active = true;
                        cx.notify();
                    }
                },
            ))
            .on_drop::<ExternalPaths>(cx.listener(|app, paths: &ExternalPaths, _, cx| {
                app.drag_active = false;
                if app.panel.is_none() {
                    app.dropped(paths.paths().to_vec(), cx);
                }
            }))
            .size_full()
            .flex()
            .flex_row()
            .bg(t.window)
            .text_color(t.ink)
            .child(sidebar)
            .child(main)
    }
}

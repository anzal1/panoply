//! The app window: a sidebar (Home, Find, History, the categories, Share, Modules, Settings), a 52 pt
//! toolbar zone with the page title, and the page. The pages are Home (search, drop area, quick
//! actions, recent jobs), search results, a category, History, Modules, Settings, two "later"
//! pages, and the same tool panel the bar uses.
//!
//! Layout rules: the 4 pt grid (`theme::sp`), one type ramp (`theme::ty`), pages at most 920 pt
//! wide and centred, a 24 pt page margin, rows at least 44 pt, controls at least 28 pt.

use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::*;
use gpui_base::input::{Input as KitInput, InputEvent, InputState};

mod find_page;
mod share_page;

use super::Hub;
use super::chrome;
use super::disk::DiskView;
use super::live;
use super::panel::{PanelEvent, ToolPanel};
use super::theme::{Theme, is_dark, radius, sp, tile_colors, ty};
use super::widgets::{
    ButtonKind, Ramp, badge, button, card, category_tile, chip, cmd, editor_style, file_icon,
    focus_border, heading, icon, icon_button, keycap, link, network_words, notice, rule,
    scroll_thumb, tip,
};
use crate::catalog::{Category, Source};
use crate::history::Entry;
use crate::profile::{FileProfile, expand_paths, files_label};
use crate::search::{self, Action, Results};
use crate::units::{format_bytes, now_secs, relative_time};

pub(crate) const APP_WIDTH: f32 = 1160.0;
pub(crate) const APP_HEIGHT: f32 = 720.0;
/// The smallest the window can be made.
pub(crate) const APP_MIN: (f32, f32) = (860.0, 560.0);
const SIDEBAR: f32 = 232.0;
/// The widest a page's column gets, margins included: 920 pt of content between 24 pt margins.
const PAGE_MAX: f32 = 968.0;

/// What the main area shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Route {
    Home,
    Find,
    History,
    /// Send and receive files with other devices.
    Share,
    Modules,
    /// The disk map: what is using the space.
    Disk,
    Settings,
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
    typing: search::Typing,
    selected: usize,
    pub(crate) panel: Option<Entity<ToolPanel>>,
    /// The disk map page. It lives as long as the window, so a scan survives a visit to another page.
    disk: Entity<DiskView>,
    _panel_sub: Option<Subscription>,
    drag_active: bool,
    /// The downloadable models and engines, read from disk when the Modules page opens.
    downloads: Vec<crate::downloads::Row>,
    /// The last thing Remove did, in words.
    downloads_note: Option<String>,
    /// Save the window's frame when it changes (the live window; never the offscreen ones).
    persist_bounds: bool,
    bounds_token: u64,
    /// The page's scroll position, reset when the page changes.
    page_scroll: ScrollHandle,
    /// The Find page: its own search field, the latest answer and what is chosen.
    find: find_page::FindState,
    /// The Share page: the model, and the engine while the page is open or receiving is on.
    share: share_page::ShareState,
    _subs: Vec<Subscription>,
}

impl AppView {
    pub(crate) fn new(window: &mut Window, cx: &mut Context<Self>, hub: Hub) -> Self {
        let search = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(format!("Search {} tools", hub.catalog.tools().len()))
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
        let (find, find_subs) = find_page::FindState::new(window, cx, &hub);
        let entity = cx.entity();
        let appearance = window.observe_window_appearance(move |_, cx| {
            entity.update(cx, |_, cx| cx.notify());
        });
        let bounds_sub =
            cx.observe_window_bounds(window, |app, window, cx| app.bounds_changed(window, cx));
        // The window opens with keyboard focus on the page, so Tab, Escape and the shortcuts work
        // before anything is clicked.
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let disk = cx.new(DiskView::new);
        let share = share_page::ShareState::new(cx);
        Self {
            hub,
            route: Route::Home,
            focus,
            search,
            query: String::new(),
            files: Vec::new(),
            results: Results::default(),
            typing: search::Typing::default(),
            selected: 0,
            panel: None,
            disk,
            _panel_sub: None,
            drag_active: false,
            downloads: Vec::new(),
            downloads_note: None,
            persist_bounds: false,
            bounds_token: 0,
            page_scroll: ScrollHandle::new(),
            find,
            share,
            _subs: {
                let mut subs = vec![search_sub, recents_sub, appearance, bounds_sub];
                subs.extend(find_subs);
                subs
            },
        }
    }

    /// Put keyboard focus back on the page itself.
    #[cfg(feature = "snapshot")]
    pub(crate) fn reset_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    /// Start saving this window's size and position (the live window only).
    pub(crate) fn remember_bounds(&mut self) {
        self.persist_bounds = true;
    }

    /// The frame changed: save it once it has stopped changing for half a second.
    fn bounds_changed(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist_bounds {
            return;
        }
        self.bounds_token += 1;
        let token = self.bounds_token;
        cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(500))
                .await;
            let _ = this.update_in(cx, |app, window, _| {
                if app.bounds_token != token {
                    return;
                }
                let (b, maximized) = match window.window_bounds() {
                    WindowBounds::Windowed(b) => (b, false),
                    WindowBounds::Maximized(b) => (b, true),
                    WindowBounds::Fullscreen(b) => (b, false),
                };
                crate::window_state::save(
                    "app",
                    &crate::window_state::Saved {
                        x: b.origin.x.as_f32(),
                        y: b.origin.y.as_f32(),
                        w: b.size.width.as_f32(),
                        h: b.size.height.as_f32(),
                        maximized,
                    },
                );
            });
        })
        .detach();
    }

    fn recents(&self, cx: &App) -> Vec<Entry> {
        self.hub.recents.read(cx).entries.clone()
    }

    fn rebuild(&mut self, cx: &App) {
        let history = self.recents(cx);
        self.results = self.typing.search(
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

    /// The disk map page, for the review renders.
    #[cfg(feature = "snapshot")]
    pub(crate) fn disk_view(&self) -> Entity<DiskView> {
        self.disk.clone()
    }

    /// Dev only: scroll the Modules page so the rows of the tools that download a model once are on
    /// screen (`first` of them, counted in rows), for the review pictures.
    #[cfg(feature = "snapshot")]
    pub(crate) fn scroll_modules_to_model_tools(&mut self, cx: &mut Context<Self>) {
        let first = self
            .hub
            .catalog
            .tools()
            .iter()
            .position(|t| {
                t.manifest.capabilities.network_use() == panoply_manifest::NetworkUse::ModelDownload
            })
            .unwrap_or(0);
        let y = (first as f32 * 52.0).max(0.0);
        self.page_scroll.set_offset(point(px(0.), px(-y)));
        cx.notify();
    }

    pub(crate) fn go(&mut self, route: Route, cx: &mut Context<Self>) {
        self.panel = None;
        self._panel_sub = None;
        self.route = route;
        self.page_scroll.set_offset(point(px(0.), px(0.)));
        if route == Route::Find {
            self.find.arrive();
        }
        self.share_visible(route == Route::Share, cx);
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

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.route, Route::Home | Route::Results) {
            self.go(Route::Home, cx);
        }
        let f = self.search.read(cx).focus_handle(cx);
        window.focus(&f, cx);
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
                PanelEvent::Share(paths) => {
                    app.query.clear();
                    app.panel = None;
                    app._panel_sub = None;
                    app.share_paths(paths.clone(), cx);
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
            Action::OpenFile { path } => self.find_open_path(path, false, cx),
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
        if self.route == Route::Find && self.find_key(ev, cx) {
            return;
        }
        match ev.keystroke.key.as_str() {
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
            "backspace" if self.route == Route::Disk => {
                self.disk.update(cx, |d, cx| d.go_up(cx));
            }
            "escape" => {
                if !self.query.is_empty() || !self.files.is_empty() {
                    self.query.clear();
                    self.files.clear();
                    self.search.update(cx, |s, cx| s.set_value("", window, cx));
                    self.go(Route::Home, cx);
                }
            }
            "tab" => {
                if ev.keystroke.modifiers.shift {
                    window.focus_prev(cx);
                } else {
                    window.focus_next(cx);
                }
            }
            _ => {}
        }
    }
}

// ---- small pieces ----

fn tile_icon(c: Category, name: &str, size: f32, t: Theme, surface: super::theme::Rgb) -> Div {
    category_tile(c, name, size, t, surface)
}

/// A neutral tile for pages that are not a tool category.
fn plain_tile(name: &str, size: f32, t: Theme) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size / 4.0))
        .bg(t.fill8)
        .child(icon(name, (size * 0.5).round(), t.ink2))
}

#[allow(clippy::too_many_arguments)]
fn side_item(
    id: impl Into<ElementId>,
    label: &str,
    icon_name: &str,
    icon_color: Hsla,
    count: Option<usize>,
    active: bool,
    shortcut: Option<&'static str>,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let el = focus_border(div().id(id), t)
        .role(Role::ListBoxOption)
        .aria_label(match count {
            Some(n) => format!("{label}, {n} {}", if n == 1 { "tool" } else { "tools" }),
            None => label.to_string(),
        })
        .aria_selected(active)
        .tab_stop(true)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .h(px(32.))
        .px(px(sp::M - 2.))
        .rounded(px(radius::CONTROL))
        .when(active, |d| d.bg(t.selection))
        .when(!active, |d| d.hover(move |s| s.bg(t.fill8)))
        .active(move |s| s.bg(t.fill12))
        .on_click(on_click)
        .child(icon(icon_name, 16., icon_color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .ramp(ty::BODY)
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
                    .ramp(ty::SMALL)
                    .text_color(t.ink2)
                    .child(n.to_string()),
            )
        });
    match shortcut {
        Some(k) => el.aria_keyshortcuts(k),
        None => el,
    }
}

fn section_label(text: &str, t: Theme) -> Div {
    div()
        .px(px(sp::M))
        .pt(px(sp::L))
        .pb(px(sp::XS))
        .ramp(ty::CAPTION)
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.ink3)
        .child(text.to_string())
}

/// One row of a list: tile, title, one line under it, something at the right. 52 pt tall.
#[allow(clippy::too_many_arguments)]
fn list_row(
    id: impl Into<ElementId>,
    tile: Div,
    title: String,
    subtitle: Option<String>,
    trailing: AnyElement,
    selected: bool,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    focus_border(div().id(id), t)
        .flex()
        .flex_row()
        .items_center()
        .gap(px(sp::M))
        .min_h(px(52.))
        .px(px(sp::M - 2.))
        .rounded(px(radius::ROW))
        .when(selected, |d| d.bg(t.selection))
        .when(!selected, |d| d.hover(move |s| s.bg(t.fill8)))
        .active(move |s| s.bg(t.fill12))
        .on_click(on_click)
        .child(tile)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(2.))
                .child(
                    div()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .ramp(ty::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(t.ink)
                        .child(title),
                )
                .when_some(subtitle, |d, s| {
                    d.child(
                        div()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .ramp(ty::SMALL)
                            .text_color(t.ink2)
                            .child(s),
                    )
                }),
        )
        .child(trailing)
}

fn chevron(t: Theme) -> AnyElement {
    icon("chevron-right", 14., t.ink3).into_any_element()
}

/// A calm empty or "later" state: a tile, a title, a line of help, an optional action.
fn empty_state(
    icon_name: &str,
    title: &str,
    body: &str,
    action: Option<AnyElement>,
    t: Theme,
) -> Div {
    div()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(sp::S))
        .py(px(sp::XXL))
        .px(px(sp::XL))
        .child(plain_tile(icon_name, 44., t))
        .child(
            div()
                .pt(px(sp::XS))
                .ramp(ty::TITLE)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.ink)
                .child(title.to_string()),
        )
        .child(
            div()
                .max_w(px(420.))
                .text_center()
                .ramp(ty::BODY)
                .text_color(t.ink2)
                .child(body.to_string()),
        )
        .when_some(action, |d, a| d.child(div().pt(px(sp::S)).child(a)))
}

/// The 52 pt toolbar zone at the top of a page: the title, and anything for the right.
pub(super) fn toolbar(title: &str, trailing: Option<AnyElement>, t: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .w_full()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .w_full()
                .max_w(px(PAGE_MAX))
                .mx_auto()
                .h(px(chrome::MAC_TOOLBAR))
                .px(px(sp::XL))
                .gap(px(sp::M))
                .child(
                    div()
                        .id("page-title")
                        .role(Role::Heading)
                        .aria_label(title.to_string())
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .ramp(ty::HEADLINE)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.ink)
                        .child(title.to_string()),
                )
                .children(trailing),
        )
        .child(rule(t))
}

impl AppView {
    /// A page: the toolbar zone, then a scrolling column at most 920 pt wide, centred.
    fn page(
        &self,
        id: &'static str,
        title: &str,
        trailing: Option<AnyElement>,
        body: impl IntoElement,
        t: Theme,
    ) -> AnyElement {
        div()
            .id(id)
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(toolbar(title, trailing, t))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .child(
                        div()
                            .id(SharedString::from(format!("{id}-scroll")))
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.page_scroll)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .w_full()
                                    .max_w(px(PAGE_MAX))
                                    .mx_auto()
                                    .gap(px(sp::XL))
                                    .p(px(sp::XL))
                                    .child(body),
                            ),
                    )
                    .children(scroll_thumb(&self.page_scroll, t)),
            )
            .into_any_element()
    }

    /// Which rows of a long, fixed-height list to build: the ones on screen plus four above and
    /// below, with the heights of the rows left out as `(first, end, space_above, space_below)`.
    /// `prefix` is roughly how far down the page the list starts. A list of hundreds of modules
    /// then costs the same frame time as one of a dozen.
    fn window_rows(&self, n: usize, row_h: f32, prefix: f32) -> (usize, usize, f32, f32) {
        let viewport = self.page_scroll.bounds().size.height.as_f32();
        let viewport = if viewport < 100.0 {
            APP_HEIGHT
        } else {
            viewport
        };
        let offset = (-self.page_scroll.offset().y.as_f32()).max(0.0);
        let first = (((offset - prefix) / row_h).floor().max(0.0) as usize).saturating_sub(4);
        let end = ((((offset + viewport - prefix) / row_h).ceil().max(0.0)) as usize + 4).min(n);
        let first = first.min(end);
        (first, end, first as f32 * row_h, (n - end) as f32 * row_h)
    }

    fn sidebar(&mut self, t: Theme, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let counts = self.hub.catalog.category_counts();
        let route = self.route;
        let go = |r: Route| cx.listener(move |app, _: &ClickEvent, _, cx| app.go(r, cx));
        let sc = |n: &'static str| Some(n);
        let nav = div()
            .id("sidebar-scroll")
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .px(px(10.))
            .overflow_y_scroll()
            .child(side_item(
                "nav-home",
                "Home",
                "house",
                t.ink2,
                None,
                route == Route::Home,
                sc("Meta+1"),
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
                sc("Meta+2"),
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
                sc("Meta+3"),
                t,
                go(Route::History),
            ))
            .child(section_label("Tools", t))
            .children(counts.into_iter().map(|(c, n)| {
                let (_, glyph) = tile_colors(c, t, t.palette.sidebar);
                side_item(
                    format!("nav-{}", c.label()),
                    c.label(),
                    c.icon(),
                    glyph,
                    Some(n),
                    route == Route::Category(c),
                    None,
                    t,
                    go(Route::Category(c)),
                )
            }))
            .child(section_label("Panoply", t))
            .child(
                side_item(
                    "nav-share",
                    "Share",
                    "share",
                    t.ink2,
                    None,
                    route == Route::Share,
                    None,
                    t,
                    go(Route::Share),
                )
                .relative()
                .when(self.share.needs_you() && route != Route::Share, |d| {
                    // Somebody is waiting for an answer on the Share page.
                    d.aria_description("Someone wants to send you files").child(
                        div()
                            .absolute()
                            .top(px(12.))
                            .right(px(12.))
                            .size(px(8.))
                            .rounded_full()
                            .bg(t.accent),
                    )
                }),
            )
            .child(side_item(
                "nav-disk",
                "Disk map",
                "hard-drive",
                t.ink2,
                None,
                route == Route::Disk,
                None,
                t,
                go(Route::Disk),
            ))
            .child(side_item(
                "nav-modules",
                "Modules",
                "layout-grid",
                t.ink2,
                None,
                route == Route::Modules,
                sc("Meta+4"),
                t,
                go(Route::Modules),
            ));
        div()
            .id("sidebar")
            .role(Role::ListBox)
            .aria_label("Sidebar")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(SIDEBAR))
            .h_full()
            .pt(px(chrome::top_inset(window).min(chrome::MAC_TOOLBAR)))
            .bg(t.sidebar)
            .border_r_1()
            .border_color(t.hair)
            .child(nav)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .px(px(10.))
                    .pb(px(10.))
                    .gap(px(sp::S))
                    .child(rule(t))
                    .child(
                        side_item(
                            "nav-settings",
                            "Settings",
                            "settings-2",
                            t.ink2,
                            None,
                            route == Route::Settings,
                            sc("Meta+,"),
                            t,
                            go(Route::Settings),
                        )
                        .tooltip(tip("Settings", &[cmd(), ","])),
                    ),
            )
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
            .h(px(44.))
            .px(px(sp::L))
            .gap(px(sp::M))
            .rounded(px(radius::CARD))
            .bg(t.fill8)
            .child(icon("search", 16., t.ink2))
            .child(
                div()
                    .id("app-search")
                    .role(Role::SearchInput)
                    .aria_label("Search tools")
                    .aria_keyshortcuts("Meta+F")
                    .aria_value(self.query.clone())
                    .track_focus(&focus)
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .ramp(ty::HEADLINE)
                    .text_color(t.ink)
                    .child(KitInput::new(&self.search)),
            )
            .child(
                div().flex().gap(px(sp::XS)).children(
                    live::hotkey_caps()
                        .iter()
                        .map(|c| keycap(c, t).into_any_element()),
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
            .aria_keyshortcuts("Meta+O")
            .focusable()
            .tab_stop(true)
            .flex()
            .flex_col()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(sp::S))
            .h(px(168.))
            .rounded(px(radius::AREA))
            .bg(t.sunken)
            .border_1()
            .border_color(if active { t.accent_text } else { t.hair })
            .focus_visible(move |s| s.shadow(super::widgets::ring(t)))
            .on_click(cx.listener(|app, _, _, cx| app.choose_files(cx)))
            .child(icon("upload", 24., t.accent_text))
            .child(
                div()
                    .ramp(ty::TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(t.ink)
                    .child("Drop anything here"),
            )
            .child(
                div()
                    .ramp(ty::BODY)
                    .text_color(t.ink2)
                    .child("PDFs, photos, videos, documents, folders. It stays on this computer."),
            )
            .child(
                div()
                    .pt(px(sp::XS))
                    .ramp(ty::SMALL)
                    .text_color(t.accent_text)
                    .child("or click to choose files"),
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
            .gap(px(sp::M))
            .child(heading("Quick actions", t))
            .child(
                div()
                    .grid()
                    .grid_cols(3)
                    .gap(px(sp::M))
                    .children(picks.into_iter().map(|i| {
                        let tool = self.hub.catalog.tool(i).clone();
                        div()
                            .id(("quick", i))
                            .role(Role::Button)
                            .aria_label(tool.name.clone())
                            .focusable()
                            .tab_stop(true)
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(sp::M))
                            .min_w_0()
                            .h(px(60.))
                            .px(px(14.))
                            .rounded(px(radius::CARD))
                            .bg(t.card)
                            .border_1()
                            .border_color(t.hair)
                            .hover(move |s| s.bg(t.fill8))
                            .active(move |s| s.bg(t.fill12))
                            .focus_visible(move |s| s.shadow(super::widgets::ring(t)))
                            .on_click(cx.listener(move |app, _, window, cx| {
                                let files = app.files.clone();
                                app.open_tool(i, files, Vec::new(), true, window, cx);
                            }))
                            .child(tile_icon(
                                tool.category,
                                tool.category.icon(),
                                32.,
                                t,
                                t.palette.card,
                            ))
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
                                            .ramp(ty::BODY)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(t.ink)
                                            .child(tool.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .ramp(ty::SMALL)
                                            .text_color(t.ink2)
                                            .child(tool.category.label()),
                                    ),
                            )
                    })),
            )
            .into_any_element()
    }

    /// The recent jobs: a file name, what was done, when, and two quiet actions.
    fn recent_list(&mut self, t: Theme, limit: usize, cx: &mut Context<Self>) -> AnyElement {
        let now = now_secs();
        let entries: Vec<Entry> = self.recents(cx).into_iter().take(limit).collect();
        let loaded = self.hub.recents.read(cx).loaded;
        let n = entries.len();
        let (first, end, above, below) = if limit >= 64 {
            self.window_rows(n, 52.0, 72.0)
        } else {
            (0, n, 0.0, 0.0)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(sp::S))
            .when(limit < 64, |d| d.child(heading("Recent", t)))
            .when(!loaded, |d| {
                // The history is still being read: three quiet placeholder rows.
                d.children((0..3usize).map(|i| {
                    div()
                        .id(("recent-loading", i))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(sp::M))
                        .h(px(52.))
                        .px(px(sp::M))
                        .child(div().size(px(32.)).rounded(px(8.)).bg(t.fill8))
                        .child(div().w(px(180.)).h(px(10.)).rounded(px(5.)).bg(t.fill8))
                }))
            })
            .when(loaded && n == 0, |d| {
                d.child(
                    card(t).child(empty_state(
                        "history",
                        "No jobs yet",
                        if limit >= 64 {
                            "Run a tool on a file and the job is listed here, so you can run it again."
                        } else {
                            "Drop a file above and run a tool. Every job you run is listed here, and you can run it again."
                        },
                        None,
                        t,
                    )),
                )
            })
            .child(div().flex().flex_col().child(div().h(px(above))).children(entries.into_iter().enumerate().skip(first).take(end - first).map(
                |(n, e)| {
                    let category = Category::from_module_id(&e.tool_id);
                    let reveal_path = e
                        .outputs
                        .first()
                        .cloned()
                        .or_else(|| e.inputs.first().cloned());
                    let e2 = e.clone();
                    let name = match e.inputs.as_slice() {
                        [one] => one
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                        many => format!("{} files", many.len()),
                    };
                    let can_rerun = self.hub.catalog.index_of(&e.tool_id).is_some();
                    list_row(
                        ("recent", n),
                        tile_icon(category, category.icon(), 32., t, t.palette.window),
                        name,
                        Some(format!("{}. {}", e.tool_name, e.headline)),
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(sp::XS))
                            .child(
                                div()
                                    .pr(px(sp::S))
                                    .ramp(ty::SMALL)
                                    .text_color(t.ink2)
                                    .child(relative_time(now, e.at)),
                            )
                            .when(can_rerun, |d| {
                                let e3 = e2.clone();
                                d.child(icon_button(
                                    ("recent-again", n),
                                    "Run again",
                                    "rotate-cw",
                                    false,
                                    t,
                                    cx.listener(move |app, _, window, cx| {
                                        app.rerun(&e3, window, cx)
                                    }),
                                ))
                            })
                            .child(icon_button(
                                ("recent-show", n),
                                "Show in folder",
                                "folder-open",
                                false,
                                t,
                                cx.listener(move |_, _, _, cx| {
                                    if let Some(p) = &reveal_path {
                                        cx.reveal_path(p);
                                    }
                                }),
                            ))
                            .into_any_element(),
                        false,
                        t,
                        cx.listener(move |app, ev: &ClickEvent, window, cx| {
                            if ev.click_count() >= 2 {
                                app.rerun(&e2, window, cx);
                            }
                        }),
                    )
                    .h(px(52.))
                },
            )).child(div().h(px(below))))
            .into_any_element()
    }

    fn home(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search_box(t, cx);
        let drop = self.drop_area(t, cx);
        let quick = self.quick_actions(t, cx);
        let recent = self.recent_list(t, 5, cx);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(sp::XL))
            .child(search)
            .child(drop)
            .child(quick)
            .child(recent);
        self.page("home", "Home", None, body, t)
    }

    /// Rows for search results and dropped files, with the filled-in options as chips.
    fn results_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let search = self.search_box(t, cx);
        let sections: Vec<(String, Vec<crate::search::Row>)> = self
            .results
            .sections
            .iter()
            .map(|s| (s.title.clone(), s.rows.clone()))
            .collect();
        let mut blocks: Vec<AnyElement> = Vec::new();
        let mut ord = 0usize;
        for (title, rows) in sections {
            let items: Vec<AnyElement> = rows
                .into_iter()
                .map(|r| {
                    let n = ord;
                    ord += 1;
                    let selected = n == self.selected;
                    let action = r.action.clone();
                    let (tile, chips, estimate) = (
                        tile_icon(r.category, r.icon, 32., t, t.palette.window),
                        r.chips.clone(),
                        r.estimate.clone(),
                    );
                    let row = list_row(
                        ("result-row", n),
                        tile,
                        r.title.clone(),
                        Some(r.subtitle.clone()),
                        div()
                            .ramp(ty::SMALL)
                            .text_color(t.ink3)
                            .child(r.trailing.clone())
                            .into_any_element(),
                        selected,
                        t,
                        cx.listener(move |app, _, window, cx| {
                            app.open_action(action.clone(), window, cx)
                        }),
                    )
                    .role(Role::ListBoxOption)
                    .aria_label(r.title.clone())
                    .aria_description(r.subtitle.clone())
                    .aria_selected(selected)
                    .tab_stop(true);
                    if chips.is_empty() {
                        return row.into_any_element();
                    }
                    div()
                        .flex()
                        .flex_col()
                        .child(row)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .flex_wrap()
                                .items_center()
                                .gap(px(sp::S))
                                .pl(px(sp::M + 32. + sp::M))
                                .pb(px(sp::S))
                                .children(
                                    chips.iter().map(|(l, v)| chip(l, v, t).into_any_element()),
                                )
                                .when_some(estimate, |d, e| {
                                    d.child(
                                        div()
                                            .ramp(ty::SMALL)
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(t.good)
                                            .child(e),
                                    )
                                }),
                        )
                        .into_any_element()
                })
                .collect();
            blocks.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(sp::XS))
                    .child(
                        div()
                            .px(px(sp::M))
                            .ramp(ty::CAPTION)
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(t.ink3)
                            .child(title),
                    )
                    .child(div().flex().flex_col().children(items))
                    .into_any_element(),
            );
        }
        let none = self.results.is_empty();
        let trimmed = self.query.trim().to_string();
        let files_note = (!self.files.is_empty()).then(|| {
            format!(
                "{}, {}",
                files_label(&self.files),
                format_bytes(self.files.iter().map(|f| f.bytes).sum())
            )
        });
        let first_file_icon = self.files.first().map(file_icon);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(sp::L))
            .child(search)
            .when_some(files_note.zip(first_file_icon), |d, (n, ic)| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(sp::S))
                        .ramp(ty::BODY)
                        .text_color(t.ink2)
                        .child(icon(ic, 16., t.ink2))
                        .child(n),
                )
            })
            .when(none, |d| {
                d.child(empty_state(
                    "search",
                    &if trimmed.is_empty() {
                        "No tools for this".to_string()
                    } else {
                        format!("No tools match \u{201C}{trimmed}\u{201D}")
                    },
                    "Try \u{201C}compress\u{201D} or \u{201C}heic to jpg\u{201D}, or drop a file to see what can be done with it.",
                    Some(
                        button(
                            "clear-search",
                            "Clear search",
                            ButtonKind::Secondary,
                            None,
                            t,
                            cx.listener(|app, _, window, cx| {
                                app.query.clear();
                                app.files.clear();
                                app.search.update(cx, |s, cx| s.set_value("", window, cx));
                                app.go(Route::Home, cx);
                            }),
                        )
                        .into_any_element(),
                    ),
                    t,
                ))
            })
            .children(blocks)
            .when_some(self.results.understood.clone(), |d, u| {
                d.child(
                    div()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(u),
                )
            });
        self.page("results", "Search", None, body, t)
    }

    fn category_page(&mut self, c: Category, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let idxs = self.hub.catalog.in_category(c);
        let rows: Vec<AnyElement> = idxs
            .iter()
            .map(|&i| {
                let tool = self.hub.catalog.tool(i);
                list_row(
                    ("tool", i),
                    tile_icon(c, c.icon(), 32., t, t.palette.window),
                    tool.name.clone(),
                    Some(tool.summary.clone()),
                    chevron(t),
                    false,
                    t,
                    cx.listener(move |app, _, window, cx| {
                        app.open_tool(i, Vec::new(), Vec::new(), false, window, cx)
                    }),
                )
                .role(Role::ListBoxOption)
                .aria_label(tool.name.clone())
                .aria_description(tool.summary.clone())
                .tab_stop(true)
                .into_any_element()
            })
            .collect();
        let body = div()
            .flex()
            .flex_col()
            .gap(px(sp::M))
            .child(div().ramp(ty::BODY).text_color(t.ink2).child(format!(
                "{} {}",
                idxs.len(),
                if idxs.len() == 1 { "tool" } else { "tools" }
            )))
            .child(div().flex().flex_col().children(rows));
        self.page("category", c.label(), None, body, t)
    }

    fn modules_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let tools = self.hub.catalog.tools().to_vec();
        let community = tools
            .iter()
            .filter(|t| matches!(t.source, Source::Community(_)))
            .count();
        let notes = self.hub.catalog.notes.clone();
        let (first, end, above, below) = self.window_rows(tools.len(), 52.0, 96.0);
        let rows: Vec<AnyElement> = tools
            .iter()
            .enumerate()
            .skip(first)
            .take(end - first)
            .map(|(i, tool)| {
                let network = network_words(&tool.manifest.capabilities);
                let community = matches!(tool.source, Source::Community(_));
                let perms = if let Some(n) = &network {
                    n.meta
                } else if tool.takes_files() {
                    "Chosen files only"
                } else {
                    "No files"
                };
                list_row(
                    ("module", i),
                    tile_icon(
                        tool.category,
                        tool.category.icon(),
                        32.,
                        t,
                        t.palette.window,
                    ),
                    tool.name.clone(),
                    Some(format!("{}  \u{00B7}  {}", tool.id, perms)),
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(sp::S))
                        .when_some(network, |d, n| {
                            d.child(badge(n.badge, n.warn.then_some(t.warn), t))
                        })
                        .child(badge(
                            if community {
                                "Community, sandboxed"
                            } else {
                                "Built in"
                            },
                            None,
                            t,
                        ))
                        .into_any_element(),
                    false,
                    t,
                    cx.listener(move |app, _, window, cx| {
                        app.open_tool(i, Vec::new(), Vec::new(), false, window, cx)
                    }),
                )
                .h(px(52.))
                .role(Role::ListBoxOption)
                .aria_label(tool.name.clone())
                .aria_position_in_set(i + 1)
                .aria_size_of_set(tools.len())
                .tab_stop(true)
                .into_any_element()
            })
            .collect();
        let downloads = self.downloads_section(t, cx);
        let body = div()
            .flex()
            .flex_col()
            .gap(px(sp::XL))
            .child(
                div()
                    .ramp(ty::BODY)
                    .text_color(t.ink2)
                    .child(format!(
                        "{} installed, {} from the community. Every module runs with only the files you choose.",
                        tools.len(),
                        community
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(div().h(px(above)))
                    .children(rows)
                    .child(div().h(px(below))),
            )
            .child(downloads)
            .children(notes.into_iter().map(|n| {
                notice("triangle-alert", t.warn, format!("Could not load: {n}"), t)
                    .into_any_element()
            }));
        self.page("modules", "Modules", None, body, t)
    }

    /// The models and engines Panoply downloaded or may download: size, source, license and a
    /// Remove button for each one that is on this computer.
    fn downloads_section(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.downloads.clone();
        let on_disk = rows.iter().filter(|r| r.installed).count();
        let last = rows.len().saturating_sub(1);
        let items: Vec<AnyElement> = rows
            .into_iter()
            .enumerate()
            .map(|(n, r)| {
                let id = r.id.clone();
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(sp::M))
                    .min_h(px(56.))
                    .px(px(sp::L))
                    .py(px(10.))
                    .when(n != last, |d| d.border_b_1().border_color(t.hair))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(px(2.))
                            .child(
                                div()
                                    .ramp(ty::BODY)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(t.ink)
                                    .child(r.name.clone()),
                            )
                            .child(
                                div()
                                    .ramp(ty::SMALL)
                                    .text_color(t.ink2)
                                    .child(format!("{}, {}", r.purpose, r.origin)),
                            ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .ramp(ty::SMALL)
                            .text_color(t.ink2)
                            .child(r.size_text.clone()),
                    )
                    .when(r.installed, |d| {
                        d.child(button(
                            SharedString::from(format!("remove-{id}")),
                            "Remove",
                            ButtonKind::Secondary,
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
            .gap(px(sp::S))
            .child(heading("Downloaded models and engines", t))
            .child(
                div()
                    .ramp(ty::BODY)
                    .text_color(t.ink2)
                    .child(format!(
                        "Each one is downloaded once, only after you allow it, and used offline. {on_disk} on this computer. Removing one frees the space; Panoply asks again the next time it is needed."
                    )),
            )
            .when_some(self.downloads_note.clone(), |d, n| {
                d.child(notice("info", t.accent_text, n, t))
            })
            .child(card(t).children(items))
            .into_any_element()
    }

    fn history_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let list = self.recent_list(t, 64, cx);
        let body =
            div()
                .flex()
                .flex_col()
                .gap(px(sp::M))
                .child(div().ramp(ty::BODY).text_color(t.ink2).child(
                    "Every job, with its files and options. Choose the arrow to run one again.",
                ))
                .child(list);
        self.page("history", "History", None, body, t)
    }

    // ---- settings ----

    fn settings_page(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let row = |label: &str, detail: Option<String>, trailing: AnyElement, last: bool| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(sp::L))
                .min_h(px(52.))
                .px(px(sp::L))
                .py(px(10.))
                .when(!last, |d| d.border_b_1().border_color(t.hair))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(px(2.))
                        .child(
                            div()
                                .ramp(ty::BODY)
                                .text_color(t.ink)
                                .child(label.to_string()),
                        )
                        .when_some(detail, |d, s| {
                            d.child(div().ramp(ty::SMALL).text_color(t.ink2).child(s))
                        }),
                )
                .child(trailing)
                .into_any_element()
        };
        let value = |s: String| {
            div()
                .flex_none()
                .ramp(ty::BODY)
                .text_color(t.ink2)
                .child(s)
                .into_any_element()
        };
        let keys = |caps: &[&str]| {
            div()
                .flex()
                .gap(px(sp::XS))
                .children(caps.iter().map(|c| keycap(c, t).into_any_element()))
                .into_any_element()
        };
        let data = crate::paths::data_dir();
        let modules = crate::paths::user_modules_dir();
        let (d1, m1) = (data.clone(), modules.clone());
        let reveal = |id: &'static str, path: PathBuf, cx: &mut Context<Self>| {
            button(
                id,
                "Show in folder",
                ButtonKind::Secondary,
                Some("folder-open"),
                t,
                cx.listener(move |_, _, _, cx| {
                    let _ = std::fs::create_dir_all(&path);
                    cx.reveal_path(&path)
                }),
            )
            .into_any_element()
        };
        let appearance = match std::env::var("PANOPLY_APPEARANCE").ok().as_deref() {
            Some("light") => "Light (set by PANOPLY_APPEARANCE)",
            Some("dark") => "Dark (set by PANOPLY_APPEARANCE)",
            _ => "Follows the system",
        };
        let on_off = |b: bool| if b { "On, from the system" } else { "Off" }.to_string();
        let general = card(t)
            .child(row(
                "Command bar shortcut",
                Some("Press it anywhere to search and run a tool.".into()),
                keys(live::hotkey_caps()),
                false,
            ))
            .child(row("Appearance", None, value(appearance.into()), false))
            .child(row(
                "Reduce Motion",
                Some("Panoply moves less when the system asks for it.".into()),
                value(on_off(crate::platform::reduce_motion())),
                false,
            ))
            .child(row(
                "Increase Contrast",
                Some("Text and borders get stronger when the system asks for it.".into()),
                value(on_off(crate::platform::increase_contrast())),
                true,
            ));
        let privacy = card(t)
            .child(row(
                "Where the work runs",
                Some("Every tool runs on this computer. Your files are never uploaded.".into()),
                icon("lock", 16., t.ink2).into_any_element(),
                false,
            ))
            .child(row(
                "Network",
                Some("Used only to download a model or engine, and only after you say yes.".into()),
                value("Ask first".into()),
                true,
            ));
        let storage = card(t)
            .child(row(
                "History and settings",
                Some(d1.display().to_string()),
                reveal("reveal-data", d1, cx),
                false,
            ))
            .child(row(
                "Your modules",
                Some(m1.display().to_string()),
                reveal("reveal-modules", m1, cx),
                true,
            ));
        let c = cmd();
        let shortcuts: Vec<(&str, Vec<&str>)> = vec![
            ("Open the command bar", live::hotkey_caps().to_vec()),
            ("Search tools", vec![c, "F"]),
            ("Open files", vec![c, "O"]),
            ("Home, Find, History, Modules", vec![c, "1\u{2013}4"]),
            ("Settings", vec![c, ","]),
            ("Run, or open the result", vec!["\u{21B5}"]),
            ("Undo the last result", vec![c, "Z"]),
            ("Back, or cancel a running job", vec!["Esc"]),
            ("Move between controls", vec!["\u{21E5}"]),
            ("Close the window", vec![c, "W"]),
            ("Quit", vec![c, "Q"]),
        ];
        let last = shortcuts.len() - 1;
        let shortcut_card = card(t).children(
            shortcuts
                .into_iter()
                .enumerate()
                .map(|(n, (label, caps))| row(label, None, keys(&caps), n == last)),
        );
        let about = card(t)
            .child(row(
                "Panoply",
                Some("Every utility, on your computer. MIT or Apache-2.0.".into()),
                value(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                false,
            ))
            .child(row(
                "Documentation",
                None,
                link("open-docs", "Open the docs", t, |_, _, cx| {
                    cx.open_url("https://github.com/anzal1/panoply/blob/main/docs/README.md")
                })
                .into_any_element(),
                false,
            ))
            .child(row(
                "Report a problem",
                None,
                link("open-issues", "Open an issue", t, |_, _, cx| {
                    cx.open_url("https://github.com/anzal1/panoply/issues/new")
                })
                .into_any_element(),
                true,
            ));
        let section = |title: &str, c: Div| {
            div()
                .flex()
                .flex_col()
                .gap(px(sp::S))
                .child(heading(title, t))
                .child(c)
        };
        let body = div()
            .flex()
            .flex_col()
            .gap(px(sp::XL))
            .child(section("General", general))
            .child(section("Privacy", privacy))
            .child(section("Storage", storage))
            .child(section("Keyboard shortcuts", shortcut_card))
            .child(section("About", about));
        self.page("settings", "Settings", None, body, t)
    }
}

impl Render for AppView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(is_dark(window));
        let sidebar = self.sidebar(t, window, cx);
        let main: AnyElement = match (self.route, self.panel.clone()) {
            (Route::Tool, Some(p)) => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(p)
                .into_any_element(),
            (Route::Home, _) => self.home(t, cx),
            (Route::Results, _) => self.results_page(t, cx),
            (Route::Category(c), _) => self.category_page(c, t, cx),
            (Route::Disk, _) => div()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(self.disk.clone())
                .into_any_element(),
            (Route::Modules, _) => self.modules_page(t, cx),
            (Route::History, _) => self.history_page(t, cx),
            (Route::Settings, _) => self.settings_page(t, cx),
            (Route::Find, _) => self.find_page(t, window, cx),
            (Route::Share, _) => self.share_page(t, cx),
            (Route::Tool, None) => self.home(t, cx),
        };
        let title_bar = chrome::title_bar(window, t, "Panoply");
        let edges = chrome::resize_edges(window);
        let framed = !edges.is_empty();
        div()
            .id("app")
            .key_context("App")
            .track_focus(&self.focus)
            .on_key_down(
                cx.listener(|app, ev: &KeyDownEvent, window, cx| app.on_key(ev, window, cx)),
            )
            .on_action(cx.listener(|_, _: &live::CloseWindow, window, _| window.remove_window()))
            .on_action(cx.listener(|_, _: &live::Minimize, window, _| window.minimize_window()))
            .on_action(cx.listener(|_, _: &live::Zoom, window, _| window.zoom_window()))
            .on_action(
                cx.listener(|_, _: &live::ToggleFullScreen, window, _| window.toggle_fullscreen()),
            )
            .on_action(cx.listener(|app, _: &live::GoHome, _, cx| app.go(Route::Home, cx)))
            .on_action(cx.listener(|app, _: &live::GoFind, _, cx| app.go(Route::Find, cx)))
            .on_action(cx.listener(|app, _: &live::GoHistory, _, cx| app.go(Route::History, cx)))
            .on_action(cx.listener(|app, _: &live::GoModules, _, cx| app.go(Route::Modules, cx)))
            .on_action(
                cx.listener(|app, _: &live::FocusSearch, window, cx| app.focus_search(window, cx)),
            )
            .on_action(cx.listener(|app, _: &live::OpenFiles, _, cx| app.choose_files(cx)))
            .on_action(
                cx.listener(|_, _: &live::FocusNextControl, window, cx| window.focus_next(cx)),
            )
            .on_action(
                cx.listener(|_, _: &live::FocusPrevControl, window, cx| window.focus_prev(cx)),
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
                if app.route == Route::Share {
                    app.share_paths(paths.paths().to_vec(), cx);
                } else if app.panel.is_none() {
                    app.dropped(paths.paths().to_vec(), cx);
                }
            }))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(t.window)
            .text_color(t.ink)
            .when(framed, |d| d.border_1().border_color(t.hair))
            .children(title_bar)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(main),
            )
            .children(edges)
    }
}

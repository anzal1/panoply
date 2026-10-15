//! The Find page: a search field, per-keystroke results from the file index, and the states around
//! them (nothing typed, still indexing, no match, no index).
//!
//! The search runs on a worker for every keystroke and an answer that arrives after a newer keystroke
//! is dropped, so the page never shows results for a query that is no longer in the field. Rows are
//! 52 pt tall and only the ones on screen (plus four above and below) are built, so 200 results
//! cost the same frame time as ten.

use gpui::prelude::*;
use gpui::*;
use gpui_base::input::{Input as KitInput, InputEvent, InputState};

use super::{AppView, PAGE_MAX, empty_state, toolbar};
use crate::finder::{self, FileHit, State};
use crate::ui::Hub;
use crate::ui::chrome;
use crate::ui::theme::{Theme, radius, sp, ty};
use crate::ui::widgets::{
    Ramp, category_tile, editor_style, icon, icon_button, keycap, matched_text, scroll_thumb,
};

/// Most results the page asks the index for.
pub(crate) const LIMIT: usize = 200;
/// Height of one result row.
const ROW: f32 = 52.0;
/// Rows built above and below the visible ones.
const SPARE: usize = 4;

/// What the Find page remembers.
pub(crate) struct FindState {
    input: Entity<InputState>,
    pub(crate) query: String,
    pub(crate) hits: Vec<FileHit>,
    pub(crate) selected: usize,
    /// Bumped on every keystroke; an answer for an older number is dropped.
    generation: u64,
    /// A line under the list when opening a file failed.
    note: Option<String>,
    scroll: ScrollHandle,
    /// Put the cursor in the field the next time the page is drawn.
    focus_pending: bool,
}

/// The next selection after moving `delta` rows in a list of `n`, staying inside it.
pub(crate) fn step(selected: usize, delta: i32, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    (selected as i64 + i64::from(delta)).clamp(0, n as i64 - 1) as usize
}

/// The rows to build: `(first, end, space_above, space_below)` for a list of `n` rows of `row_h`
/// points, scrolled to `offset`, with a window `viewport` points tall.
pub(crate) fn visible_rows(
    n: usize,
    row_h: f32,
    offset: f32,
    viewport: f32,
) -> (usize, usize, f32, f32) {
    let first = ((offset / row_h).floor().max(0.0) as usize).saturating_sub(SPARE);
    let end = (((offset + viewport) / row_h).ceil().max(0.0) as usize + SPARE).min(n);
    let first = first.min(end);
    (first, end, first as f32 * row_h, (n - end) as f32 * row_h)
}

/// "1,284,331": a count with thousands separators.
pub(crate) fn thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// What "show in the file manager" is called on this system.
pub(crate) fn file_manager() -> &'static str {
    if cfg!(target_os = "macos") {
        "Finder"
    } else if cfg!(windows) {
        "Explorer"
    } else {
        "the file manager"
    }
}

impl FindState {
    pub(crate) fn new(
        window: &mut Window,
        cx: &mut Context<AppView>,
        hub: &Hub,
    ) -> (FindState, Vec<Subscription>) {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search files by name"));
        let input_sub = cx.subscribe_in(
            &input,
            window,
            |app, input, ev: &InputEvent, _window, cx| match ev {
                InputEvent::Change => {
                    let v = input.read(cx).value().to_string();
                    app.find_set_query(v, cx);
                }
                InputEvent::PressEnter { secondary, .. } => app.find_activate(*secondary, cx),
                _ => {}
            },
        );
        // The index finishing (or counting up) redraws the page; when it becomes ready and
        // something is already typed, the query runs again.
        let status_sub = cx.observe(&hub.find_status, |app, status, cx| {
            let ready = matches!(status.read(cx).state, State::Ready { .. });
            if ready && !app.find.query.trim().is_empty() && app.find.hits.is_empty() {
                let q = app.find.query.clone();
                app.find_set_query(q, cx);
            }
            cx.notify();
        });
        (
            FindState {
                input,
                query: String::new(),
                hits: Vec::new(),
                selected: 0,
                generation: 0,
                note: None,
                scroll: ScrollHandle::new(),
                focus_pending: false,
            },
            vec![input_sub, status_sub],
        )
    }

    /// The page was opened: the cursor goes to the field when it is next drawn.
    pub(crate) fn arrive(&mut self) {
        self.focus_pending = true;
    }

    /// Put the query in the field and the answer in the list at once (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn input(&self) -> &Entity<InputState> {
        &self.input
    }
}

impl AppView {
    /// A new query: ask the index on a worker.
    pub(crate) fn find_set_query(&mut self, q: String, cx: &mut Context<Self>) {
        self.find.query = q;
        self.find.generation += 1;
        self.find.note = None;
        let q = self.find.query.trim().to_string();
        if q.is_empty() {
            self.find.hits.clear();
            self.find.selected = 0;
            cx.notify();
            return;
        }
        let generation = self.find.generation;
        let finder = self.hub.finder.clone();
        cx.spawn(async move |this, cx| {
            let hits = cx
                .background_executor()
                .spawn(async move { finder.search(&q, LIMIT) })
                .await;
            let _ = this.update(cx, |app, cx| {
                if app.find.generation != generation {
                    return;
                }
                app.find.hits = hits;
                app.find.selected = 0;
                app.find.scroll.set_offset(point(px(0.), px(0.)));
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Arrow keys on the Find page. True when the key was used.
    pub(crate) fn find_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let n = self.find.hits.len();
        let delta = match ev.keystroke.key.as_str() {
            "down" => 1,
            "up" => -1,
            "n" if ev.keystroke.modifiers.control => 1,
            "p" if ev.keystroke.modifiers.control => -1,
            _ => return false,
        };
        if n > 0 {
            self.find.selected = step(self.find.selected, delta, n);
            self.find_reveal_selection();
            cx.notify();
        }
        true
    }

    /// Scroll so the chosen row is on screen.
    fn find_reveal_selection(&mut self) {
        let viewport = self.find.scroll.bounds().size.height.as_f32();
        if viewport < 1.0 {
            return;
        }
        let top = self.find.selected as f32 * ROW;
        let offset = (-self.find.scroll.offset().y.as_f32()).max(0.0);
        let want = if top < offset {
            top
        } else if top + ROW > offset + viewport {
            top + ROW - viewport
        } else {
            return;
        };
        self.find.scroll.set_offset(point(px(0.), px(-want)));
    }

    /// Return opens the chosen file; Command+Return (Control elsewhere) shows it in the file manager.
    fn find_activate(&mut self, reveal: bool, cx: &mut Context<Self>) {
        if let Some(h) = self.find.hits.get(self.find.selected) {
            let path = h.path.clone();
            self.find_open_path(path, reveal, cx);
        }
    }

    /// Open or reveal on a worker, so a slow launcher never stalls a frame.
    pub(crate) fn find_open_path(
        &mut self,
        path: std::path::PathBuf,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |this, cx| {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let r = cx
                .background_executor()
                .spawn(async move {
                    if !path.exists() {
                        return Err(false);
                    }
                    let r = if reveal {
                        crate::platform::reveal_path(&path)
                    } else {
                        crate::platform::open_path(&path)
                    };
                    r.map_err(|_| true)
                })
                .await;
            if let Err(exists) = r {
                let _ = this.update(cx, |app, cx| {
                    app.find.note = Some(if exists {
                        format!("Could not open {name}.")
                    } else {
                        format!("{name} is gone. The list updates in a moment.")
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Put a query on the Find page without typing (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn find_show(&mut self, query: &str, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.find.input().clone();
        input.update(cx, |s, cx| s.set_value(query.to_string(), window, cx));
        self.find_set_query(query.to_string(), cx);
    }

    /// Scroll the result list to its last row (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn find_scroll_to_end(&mut self, cx: &mut Context<Self>) {
        let n = self.find.hits.len();
        let end = (n as f32 + 1.0) * ROW;
        let max = self.find.scroll.max_offset().y.as_f32().abs().max(end);
        self.find.scroll.set_offset(point(px(0.), px(-max)));
        cx.notify();
    }

    /// The current answer, for the renderer to wait on.
    #[cfg(feature = "snapshot")]
    pub(crate) fn find_hit_count(&self) -> usize {
        self.find.hits.len()
    }

    /// Choose a row (the snapshot renderer).
    #[cfg(feature = "snapshot")]
    pub(crate) fn find_select(&mut self, i: usize, cx: &mut Context<Self>) {
        self.find.selected = i.min(self.find.hits.len().saturating_sub(1));
        self.find_reveal_selection();
        cx.notify();
    }

    pub(super) fn find_page(
        &mut self,
        t: Theme,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status = self.hub.find_status.read(cx).state.clone();
        let input_focus = self.find.input.read(cx).focus_handle(cx);
        if self.find.focus_pending {
            self.find.focus_pending = false;
            window.focus(&input_focus, cx);
        }
        self.find
            .input
            .update(cx, |s, _| s.set_editor_style(editor_style(t)));

        let count = match &status {
            State::Ready { entries, .. } => Some(format!("{} files indexed", thousands(*entries as u64))),
            State::Indexing { entries } => Some(format!("Indexing, {} so far", thousands(*entries))),
            State::Unavailable { .. } => None,
        };
        let trailing = count.map(|c| {
            div()
                .flex_none()
                .ramp(ty::SMALL)
                .text_color(t.ink2)
                .child(c)
                .into_any_element()
        });

        let typed = !self.find.query.trim().is_empty();
        let ready = matches!(status, State::Ready { .. });
        let field = self.find_field(t, &input_focus);

        let body: AnyElement = match (&status, typed) {
            (State::Unavailable { reason }, _) => empty_state(
                "triangle-alert",
                "File search is not available",
                reason,
                None,
                t,
            )
            .into_any_element(),
            (State::Indexing { entries }, _) => indexing_state(*entries, typed, t),
            (State::Ready { entries, .. }, false) => ready_state(*entries, t),
            (State::Ready { .. }, true) if self.find.hits.is_empty() => {
                no_results_state(self.find.query.trim(), t)
            }
            (State::Ready { .. }, true) => self.find_list(t, cx),
        };

        let showing_list = ready && typed && !self.find.hits.is_empty();
        let footer = self.find_footer(showing_list, t);

        div()
            .id("find")
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(toolbar("Find", trailing, t))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .max_w(px(PAGE_MAX))
                    .mx_auto()
                    .px(px(sp::XL))
                    .pt(px(sp::L))
                    .gap(px(sp::M))
                    .child(field)
                    .child(
                        div()
                            .relative()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_h_0()
                            .child(body),
                    ),
            )
            .child(footer)
            .into_any_element()
    }

    fn find_field(&self, t: Theme, focus: &FocusHandle) -> Div {
        let n = self.find.hits.len();
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
                    .id("find-search")
                    .role(Role::SearchInput)
                    .aria_label("Search files by name")
                    .aria_value(self.find.query.clone())
                    .track_focus(focus)
                    .flex_1()
                    .min_w_0()
                    .h(px(20.))
                    .overflow_hidden()
                    .ramp(ty::HEADLINE)
                    .text_color(t.ink)
                    .child(KitInput::new(&self.find.input)),
            )
            .when(n > 0, |d| {
                d.child(
                    div()
                        .flex_none()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(if n >= LIMIT {
                            format!("Best {LIMIT}")
                        } else if n == 1 {
                            "1 match".to_string()
                        } else {
                            format!("{n} matches")
                        }),
                )
            })
    }

    /// The rows. Only the ones on screen, plus a few either side, are built.
    fn find_list(&mut self, t: Theme, cx: &mut Context<Self>) -> AnyElement {
        let n = self.find.hits.len();
        let viewport = {
            let h = self.find.scroll.bounds().size.height.as_f32();
            if h < 100.0 { 480.0 } else { h }
        };
        let offset = (-self.find.scroll.offset().y.as_f32()).max(0.0);
        let (first, end, above, below) = visible_rows(n, ROW, offset, viewport);
        let now = crate::units::now_secs();
        let selected = self.find.selected;
        let rows: Vec<AnyElement> = (first..end)
            .map(|i| {
                let hit = self.find.hits[i].clone();
                let path = hit.path.clone();
                let path2 = hit.path.clone();
                let open = cx.listener(move |app, ev: &ClickEvent, _, cx| {
                    app.find.selected = i;
                    if ev.click_count() >= 2 {
                        app.find_open_path(path.clone(), false, cx);
                    } else {
                        cx.notify();
                    }
                });
                let reveal = cx.listener(move |app, _: &ClickEvent, _, cx| {
                    app.find_open_path(path2.clone(), true, cx)
                });
                file_row(i, n, &hit, i == selected, now, t, open, reveal)
            })
            .collect();
        let list = div()
            .id("find-results")
            .role(Role::ListBox)
            .aria_label("Files")
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.find.scroll)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .child(div().h(px(above)))
                    .children(rows)
                    .child(div().h(px(below)))
                    .when(n >= LIMIT, |d| {
                        d.child(
                            div()
                                .h(px(ROW))
                                .flex()
                                .items_center()
                                .justify_center()
                                .ramp(ty::SMALL)
                                .text_color(t.ink3)
                                .child(format!(
                                    "Showing the best {LIMIT}. Type more of the name to narrow it down."
                                )),
                        )
                    }),
            );
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(list)
            .children(scroll_thumb(&self.find.scroll, t))
            .into_any_element()
    }

    /// The strip under the list: what the keys do, or what went wrong with the last open.
    fn find_footer(&self, showing_list: bool, t: Theme) -> Div {
        let c = crate::ui::widgets::cmd();
        let key_hint = |label: String, caps: Vec<&'static str>| {
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(sp::S))
                .child(div().ramp(ty::SMALL).text_color(t.ink2).child(label))
                .child(
                    div()
                        .flex()
                        .gap(px(sp::XS))
                        .children(caps.into_iter().map(|k| keycap(k, t).into_any_element())),
                )
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .flex_none()
            .w_full()
            .h(px(40.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .max_w(px(PAGE_MAX))
                    .mx_auto()
                    .px(px(sp::XL))
                    .gap(px(sp::L))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .ramp(ty::SMALL)
                            .text_color(if self.find.note.is_some() { t.warn } else { t.ink3 })
                            .child(
                                self.find
                                    .note
                                    .clone()
                                    .unwrap_or_else(|| "Names only. Nothing leaves this computer.".into()),
                            ),
                    )
                    .when(showing_list, |d| {
                        d.child(key_hint("Move".into(), vec!["\u{2191}", "\u{2193}"]))
                            .child(key_hint("Open".into(), vec!["\u{21B5}"]))
                            .child(key_hint(
                                format!("Show in {}", file_manager()),
                                vec![c, "\u{21B5}"],
                            ))
                    }),
            )
            .border_t_1()
            .border_color(t.hair)
    }
}

/// One result: tile, name with the match in bold, the folder (middle truncated), when it changed.
#[allow(clippy::too_many_arguments)]
fn file_row(
    i: usize,
    n: usize,
    hit: &FileHit,
    selected: bool,
    now: u64,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_reveal: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let kind = hit.kind();
    let when = finder::modified_label(now, hit.mtime);
    div()
        .id(("find-row", i))
        .role(Role::ListBoxOption)
        .aria_label(hit.name.clone())
        .aria_description(format!("{}. {}", hit.parent, when))
        .aria_selected(selected)
        .aria_position_in_set(i + 1)
        .aria_size_of_set(n)
        .flex()
        .flex_row()
        .items_center()
        .flex_none()
        .gap(px(sp::M))
        .h(px(ROW))
        .px(px(sp::M))
        .rounded(px(radius::ROW))
        .when(selected, |d| d.bg(t.selection))
        .when(!selected, |d| d.hover(move |s| s.bg(t.fill8)))
        .on_click(on_click)
        .child(category_tile(
            kind.category,
            kind.icon,
            32.,
            t,
            t.palette.window,
        ))
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
                        .text_color(t.ink)
                        .child(matched_text(&hit.name, &hit.matched)),
                )
                .child(
                    div()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis_middle()
                        .ramp(ty::SMALL)
                        .text_color(t.ink2)
                        .child(hit.parent.clone()),
                ),
        )
        .child(
            div()
                .flex_none()
                .ramp(ty::SMALL)
                .text_color(t.ink3)
                .child(when),
        )
        .when(selected, |d| {
            d.child(icon_button(
                ("find-reveal", i),
                &format!("Show in {}", file_manager()),
                "folder-open",
                false,
                t,
                on_reveal,
            ))
        })
}

fn ready_state(entries: usize, t: Theme) -> AnyElement {
    state_block(
        "search",
        "Find a file by name",
        &format!(
            "Start typing. {} files on this computer are ready to search, and the list follows every letter.",
            thousands(entries as u64)
        ),
        Some("Two words narrow it down, like \u{201C}tax report\u{201D}. A few letters of the name are enough."),
        t,
    )
}

fn indexing_state(entries: u64, typed: bool, t: Theme) -> AnyElement {
    state_block(
        "files",
        "Reading your files",
        &format!(
            "{} files so far. Panoply does this once, in the background, and keeps the list current after that.",
            thousands(entries)
        ),
        Some(if typed {
            "Your search runs as soon as this is done."
        } else {
            "You can keep using Panoply. Search works when this finishes."
        }),
        t,
    )
}

fn no_results_state(query: &str, t: Theme) -> AnyElement {
    state_block(
        "search",
        &format!("No files match \u{201C}{query}\u{201D}"),
        "Check the spelling, or type fewer letters.",
        Some("Folders such as node_modules and .git are not indexed."),
        t,
    )
}

fn state_block(icon_name: &str, title: &str, body: &str, hint: Option<&str>, t: Theme) -> AnyElement {
    let mut block = empty_state(icon_name, title, body, None, t);
    if let Some(h) = hint {
        block = block.child(
            div()
                .max_w(px(420.))
                .text_center()
                .ramp(ty::SMALL)
                .text_color(t.ink3)
                .child(h.to_string()),
        );
    }
    div()
        .flex()
        .flex_col()
        .flex_1()
        .items_center()
        .justify_center()
        .pb(px(chrome::MAC_TOOLBAR))
        .child(block)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_selection_stays_inside_the_list() {
        assert_eq!(step(0, -1, 5), 0);
        assert_eq!(step(0, 1, 5), 1);
        assert_eq!(step(4, 1, 5), 4);
        assert_eq!(step(3, 1, 0), 0);
    }

    #[test]
    fn counts_have_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1_000), "1,000");
        assert_eq!(thousands(1_284_331), "1,284,331");
    }

    #[test]
    fn a_long_list_builds_only_the_rows_on_screen() {
        // 10,000 rows, a 500 pt window at the top: about ten rows and the spare four.
        let (first, end, above, below) = visible_rows(10_000, ROW, 0.0, 500.0);
        assert_eq!(first, 0);
        assert!(end <= 20, "{end}");
        assert_eq!(above, 0.0);
        assert!((below - (10_000 - end) as f32 * ROW).abs() < 0.01);
        // Scrolled to the middle, the window moves and the spacers keep the height.
        let (first, end, above, below) = visible_rows(10_000, ROW, 52_000.0, 500.0);
        assert!(first >= 990 && end <= 1_020, "{first}..{end}");
        let total = above + (end - first) as f32 * ROW + below;
        assert!((total - 10_000.0 * ROW).abs() < 0.01);
        // Past the end it clamps.
        let (first, end, _, _) = visible_rows(10, ROW, 9_000.0, 500.0);
        assert_eq!((first, end), (10, 10));
    }
}

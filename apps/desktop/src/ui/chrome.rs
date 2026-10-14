//! The window frame, per platform.
//!
//! macOS keeps the system frame: the traffic lights sit inside a 52 pt toolbar zone that the
//! sidebar and the page start below. Windows gets a 32 pt title bar on a solid Mica-like surface
//! with the Fluent caption buttons (46 by 32). Linux uses the compositor's decorations when it
//! provides them, and on GNOME under Wayland, which draws none, a 46 pt header bar with the close,
//! minimise and maximise buttons and 6 pt resize edges.

use gpui::prelude::*;
use gpui::*;

use super::theme::{Theme, sp};
use super::widgets::{Ramp, brand_mark, icon};

/// The macOS toolbar zone, where the traffic lights live.
pub(crate) const MAC_TOOLBAR: f32 = 52.0;
const WIN_BAR: f32 = 32.0;
const GNOME_BAR: f32 = 46.0;
const EDGE: f32 = 6.0;

/// Where the traffic lights go on macOS: 20 pt in from the left, centred in the 52 pt zone.
pub(crate) const TRAFFIC_LIGHTS: (f32, f32) = (20.0, 20.0);

/// Whether the app has to draw its own title bar in this window.
pub(crate) fn draws_title_bar(window: &Window) -> bool {
    if cfg!(target_os = "macos") {
        false
    } else if cfg!(windows) {
        true
    } else {
        matches!(window.window_decorations(), Decorations::Client { .. })
    }
}

/// The height the page must leave above itself for the frame: the macOS toolbar zone, or the
/// title bar the app draws, or nothing when the system draws it.
pub(crate) fn top_inset(window: &Window) -> f32 {
    if cfg!(target_os = "macos") {
        MAC_TOOLBAR
    } else if draws_title_bar(window) {
        if cfg!(windows) { WIN_BAR } else { GNOME_BAR }
    } else {
        sp::M
    }
}

fn caption_button(
    id: &'static str,
    glyph: &'static str,
    area: WindowControlArea,
    t: Theme,
    danger: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let hover_bg = if danger {
        super::theme::rgb8((196, 43, 28), 1.0)
    } else {
        t.fill12
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(match id {
            "caption-min" => "Minimise",
            "caption-max" => "Maximise",
            _ => "Close",
        })
        .window_control_area(area)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .w(px(46.))
        .h(px(WIN_BAR))
        .text_color(t.ink)
        .hover(move |s| {
            let s = s.bg(hover_bg);
            if danger {
                s.text_color(gpui::white())
            } else {
                s
            }
        })
        .on_click(on_click)
        .child(icon(glyph, 10., t.ink))
}

/// The title bar the app draws on Windows, and on Linux when the compositor draws none.
pub(crate) fn title_bar(window: &Window, t: Theme, title: &str) -> Option<AnyElement> {
    if !draws_title_bar(window) {
        return None;
    }
    let maximized = window.is_maximized();
    if cfg!(windows) {
        // Solid surface in the Mica colours, the same as the sidebar so the frame reads as one.
        Some(
            div()
                .id("title-bar")
                .flex()
                .flex_row()
                .items_center()
                .flex_none()
                .w_full()
                .h(px(WIN_BAR))
                .bg(t.sidebar)
                .child(
                    div()
                        .id("title-drag")
                        .window_control_area(WindowControlArea::Drag)
                        .flex()
                        .flex_row()
                        .flex_1()
                        .items_center()
                        .gap(px(sp::S))
                        .h_full()
                        .pl(px(sp::M))
                        .child(brand_mark(20., t.sidebar, t))
                        .child(
                            div()
                                .ramp(super::theme::ty::SMALL)
                                .text_color(t.ink)
                                .child(title.to_string()),
                        ),
                )
                .child(caption_button(
                    "caption-min",
                    "win-min",
                    WindowControlArea::Min,
                    t,
                    false,
                    |_, _, _| {},
                ))
                .child(caption_button(
                    "caption-max",
                    if maximized { "win-restore" } else { "win-max" },
                    WindowControlArea::Max,
                    t,
                    false,
                    |_, _, _| {},
                ))
                .child(caption_button(
                    "caption-close",
                    "win-close",
                    WindowControlArea::Close,
                    t,
                    true,
                    |_, _, _| {},
                ))
                .into_any_element(),
        )
    } else {
        let controls = window.window_controls();
        let round = |id: &'static str, name: &'static str, label: &'static str| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .flex()
                .items_center()
                .justify_center()
                .size(px(28.))
                .rounded_full()
                .bg(t.fill8)
                .hover(move |s| s.bg(t.fill12))
                .child(icon(name, 12., t.ink))
        };
        Some(
            div()
                .id("title-bar")
                .flex()
                .flex_row()
                .items_center()
                .flex_none()
                .w_full()
                .h(px(GNOME_BAR))
                .px(px(sp::M))
                .gap(px(sp::S))
                .bg(t.sidebar)
                .border_b_1()
                .border_color(t.hair)
                .child(
                    div()
                        .id("title-drag")
                        .window_control_area(WindowControlArea::Drag)
                        .flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .h_full()
                        .on_mouse_down(MouseButton::Left, |ev, window, _| {
                            if ev.click_count >= 2 {
                                window.zoom_window();
                            } else {
                                window.start_window_move();
                            }
                        })
                        .on_mouse_down(MouseButton::Right, |ev, window, _| {
                            window.show_window_menu(ev.position);
                        })
                        .ramp(super::theme::ty::BODY)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(t.ink)
                        .child(title.to_string()),
                )
                .when(controls.minimize, |d| {
                    d.child(
                        round("caption-min", "win-min", "Minimise")
                            .on_click(|_, window, _| window.minimize_window()),
                    )
                })
                .when(controls.maximize, |d| {
                    d.child(
                        round(
                            "caption-max",
                            if maximized { "win-restore" } else { "win-max" },
                            "Maximise",
                        )
                        .on_click(|_, window, _| window.zoom_window()),
                    )
                })
                .child(
                    round("caption-close", "win-close", "Close")
                        .on_click(|_, window, _| window.remove_window()),
                )
                .into_any_element(),
        )
    }
}

/// The 6 pt resize edges and corners, for a window whose frame the app draws (Linux, Client
/// decorations). Empty when the system frame is in use.
pub(crate) fn resize_edges(window: &Window) -> Vec<AnyElement> {
    if cfg!(target_os = "macos") || cfg!(windows) || !draws_title_bar(window) {
        return Vec::new();
    }
    let edge = |id: &'static str, e: ResizeEdge, cursor: CursorStyle| {
        let base = div()
            .id(id)
            .absolute()
            .cursor(cursor)
            .on_mouse_down(MouseButton::Left, move |_, window, _| {
                window.start_window_resize(e)
            });
        match e {
            ResizeEdge::Top => base.top_0().left(px(EDGE)).right(px(EDGE)).h(px(EDGE)),
            ResizeEdge::Bottom => base.bottom_0().left(px(EDGE)).right(px(EDGE)).h(px(EDGE)),
            ResizeEdge::Left => base.left_0().top(px(EDGE)).bottom(px(EDGE)).w(px(EDGE)),
            ResizeEdge::Right => base.right_0().top(px(EDGE)).bottom(px(EDGE)).w(px(EDGE)),
            ResizeEdge::TopLeft => base.top_0().left_0().size(px(EDGE)),
            ResizeEdge::TopRight => base.top_0().right_0().size(px(EDGE)),
            ResizeEdge::BottomLeft => base.bottom_0().left_0().size(px(EDGE)),
            ResizeEdge::BottomRight => base.bottom_0().right_0().size(px(EDGE)),
        }
        .into_any_element()
    };
    vec![
        edge("edge-t", ResizeEdge::Top, CursorStyle::ResizeUpDown),
        edge("edge-b", ResizeEdge::Bottom, CursorStyle::ResizeUpDown),
        edge("edge-l", ResizeEdge::Left, CursorStyle::ResizeLeftRight),
        edge("edge-r", ResizeEdge::Right, CursorStyle::ResizeLeftRight),
        edge(
            "edge-tl",
            ResizeEdge::TopLeft,
            CursorStyle::ResizeUpLeftDownRight,
        ),
        edge(
            "edge-tr",
            ResizeEdge::TopRight,
            CursorStyle::ResizeUpRightDownLeft,
        ),
        edge(
            "edge-bl",
            ResizeEdge::BottomLeft,
            CursorStyle::ResizeUpRightDownLeft,
        ),
        edge(
            "edge-br",
            ResizeEdge::BottomRight,
            CursorStyle::ResizeUpLeftDownRight,
        ),
    ]
}

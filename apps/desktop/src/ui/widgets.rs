//! The small native component set. Every control carries an accessibility role and label, and
//! every action is reachable from the keyboard. Nothing here knows about any module.

use gpui::prelude::*;
use gpui::*;

use super::theme::Theme;

/// A Lucide icon.
pub(crate) fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// A small key cap such as ↵ or ⌘.
pub(crate) fn keycap(text: &str, pressed: bool, t: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(if text.chars().count() > 1 { 5. } else { 0. }))
        .rounded(px(5.))
        .bg(if pressed { t.fill12 } else { t.fill8 })
        .text_size(px(11.))
        .line_height(px(14.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.ink2)
        .child(text.to_string())
}

/// The kit input's colours, from the theme.
pub(crate) fn editor_style(t: Theme) -> gpui_base::input::InputEditorStyle {
    gpui_base::input::InputEditorStyle {
        foreground: t.ink,
        muted_foreground: t.ink3,
        background: super::theme::col(0.0, 0.0, 0.0, 0.0),
        border: super::theme::col(0.0, 0.0, 0.0, 0.0),
        caret: t.ink,
        selection: if t.dark {
            super::theme::col(0.25, 0.5, 1.0, 0.45)
        } else {
            super::theme::col(0.0, 0.478, 1.0, 0.25)
        },
        ..Default::default()
    }
}

/// Round `v` to half a point so one-pixel lines stay crisp.
pub(crate) fn snap(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

/// Which look a button has.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

/// A button: labelled, focusable, with an optional icon.
pub(crate) fn button(
    id: impl Into<ElementId>,
    label: &str,
    kind: ButtonKind,
    icon_name: Option<&str>,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let (bg, fg) = match kind {
        ButtonKind::Primary => (t.accent, t.on_accent),
        ButtonKind::Secondary => (t.fill8, t.ink),
        ButtonKind::Ghost => (gpui::transparent_black(), t.ink2),
    };
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.to_string())
        .tab_stop(true)
        .flex()
        .flex_none()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(32.))
        .px(px(14.))
        .rounded(px(8.))
        .bg(bg)
        .text_size(px(13.))
        .line_height(px(16.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .cursor_pointer()
        .hover(move |s| s.opacity(0.88))
        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.6)))
        .when_some(icon_name, |d, n| d.child(icon(n, 14., fg)))
        .child(label.to_string())
        .on_click(on_click)
}

/// A pill showing "label value", for the options a request filled in.
pub(crate) fn chip(label: &str, value: &str, t: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .h(px(26.))
        .px(px(12.))
        .rounded_full()
        .border_1()
        .border_color(t.hair)
        .bg(t.field)
        .text_size(px(12.5))
        .line_height(px(16.))
        .text_color(t.ink2)
        .child(label.to_string())
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(t.ink)
                .child(value.to_string()),
        )
}

/// A segmented control: one of several values. Left and Right move the selection when it has focus.
pub(crate) fn segmented(
    id: String,
    label: &str,
    values: &[String],
    selected: Option<&str>,
    focus: &FocusHandle,
    t: Theme,
    on_pick: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    let many = values.len() > 4 || values.iter().any(|v| v.chars().count() > 12);
    div()
        .id(SharedString::from(id.clone()))
        .role(Role::RadioGroup)
        .aria_label(label.to_string())
        .track_focus(focus)
        .tab_stop(true)
        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
        .flex()
        .flex_row()
        .when(many, |d| d.flex_wrap())
        .gap(px(2.))
        .p(px(2.))
        .rounded(px(9.))
        .bg(t.fill8)
        .children(values.iter().enumerate().map(|(i, v)| {
            let on = selected == Some(v.as_str());
            let pick = on_pick.clone();
            let value = v.clone();
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .role(Role::RadioButton)
                .aria_label(crate::form::humanize_value(v))
                .aria_selected(on)
                .on_a11y_action(AccessibleAction::Click, {
                    let pick = on_pick.clone();
                    let value = v.clone();
                    move |_, window, cx| pick(value.clone(), window, cx)
                })
                .flex()
                .items_center()
                .justify_center()
                .when(!many, |d| d.flex_1())
                .h(px(28.))
                .px(px(12.))
                .rounded(px(7.))
                .text_size(px(12.5))
                .line_height(px(16.))
                .font_weight(if on {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if on { t.ink } else { t.ink2 })
                .when(on, |d| d.bg(t.field).shadow_sm())
                .cursor_pointer()
                .on_click(move |_, window, cx| pick(value.clone(), window, cx))
                .child(crate::form::humanize_value(v))
        }))
}

/// A switch with its label to the right.
pub(crate) fn switch(
    id: impl Into<ElementId>,
    label: &str,
    on: bool,
    focus: &FocusHandle,
    t: Theme,
    on_toggle: impl Fn(&mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    let toggle = on_toggle.clone();
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label.to_string())
        .aria_toggled(if on { Toggled::True } else { Toggled::False })
        .on_a11y_action(AccessibleAction::Click, move |_, window, cx| {
            on_toggle(window, cx)
        })
        .track_focus(focus)
        .tab_stop(true)
        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .rounded(px(8.))
        .cursor_pointer()
        .on_click(move |_, window, cx| toggle(window, cx))
        .child(
            div()
                .flex_none()
                .relative()
                .w(px(34.))
                .h(px(20.))
                .rounded_full()
                .bg(if on { t.accent } else { t.fill12 })
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(if on { 16. } else { 2. }))
                        .size(px(16.))
                        .rounded_full()
                        .bg(gpui::white())
                        .shadow_sm(),
                ),
        )
        .child(
            div()
                .text_size(px(13.))
                .line_height(px(18.))
                .text_color(t.ink)
                .child(label.to_string()),
        )
}

/// A thin progress bar. `None` is indeterminate (a quiet resting bar, no shimmer).
pub(crate) fn progress_bar(fraction: Option<f32>, label: &str, t: Theme) -> Stateful<Div> {
    let f = fraction.unwrap_or(0.0).clamp(0.0, 1.0);
    div()
        .id("progress")
        .role(Role::ProgressIndicator)
        .aria_label(label.to_string())
        .aria_numeric_value(f64::from(f) * 100.0)
        .aria_min_numeric_value(0.0)
        .aria_max_numeric_value(100.0)
        .w_full()
        .h(px(6.))
        .rounded_full()
        .bg(t.fill12)
        .overflow_hidden()
        .child(
            div()
                .h_full()
                .w(relative(if fraction.is_some() { f } else { 0.08 }))
                .rounded_full()
                .bg(t.accent),
        )
}

/// A small round icon button (the number stepper's plus and minus, close buttons).
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    label: &str,
    icon_name: &str,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label.to_string())
        .tab_stop(true)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(28.))
        .rounded(px(7.))
        .bg(t.fill8)
        .cursor_pointer()
        .hover(move |s| s.bg(t.fill12))
        .focus_visible(move |s| s.border_2().border_color(t.accent.opacity(0.5)))
        .child(icon(icon_name, 14., t.ink2))
        .on_click(on_click)
}

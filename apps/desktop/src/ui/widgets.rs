//! The small native component set. Every control carries an accessibility role and label, every
//! action is reachable from the keyboard, every control is at least 28 pt tall, and every focusable
//! control shows the same 2 pt focus ring (drawn as a shadow, so focusing never moves the layout).
//! Nothing here knows about any module.

use gpui::prelude::*;
use gpui::*;

use super::theme::{HIT, Theme, radius, sp, ty};

/// Set a text size from the type ramp.
pub(crate) trait Ramp: Styled + Sized {
    /// Apply a (size, line height) pair from [`ty`].
    fn ramp(self, r: (f32, f32)) -> Self {
        self.text_size(px(r.0)).line_height(px(r.1))
    }
}

impl<T: Styled + Sized> Ramp for T {}

/// A Lucide icon.
pub(crate) fn icon(name: &str, size: f32, color: Hsla) -> Svg {
    svg()
        .path(format!("icons/{name}.svg"))
        .size(px(size))
        .flex_none()
        .text_color(color)
}

/// Give a focusable control a focus ring that cannot shift its layout or show through its
/// translucent fill: the control always has a 2 pt border, transparent at rest and the accent
/// when it has keyboard focus. The border is inside the control's size.
pub(crate) fn focus_border(el: Stateful<Div>, t: Theme) -> Stateful<Div> {
    el.focusable()
        .tab_stop(true)
        .border_2()
        .border_color(gpui::transparent_black())
        .focus_visible(move |s| s.border_color(t.accent_text))
}

/// The focus ring: 2 pt in the accent, outside the control, so it never shifts the layout.
/// Only for controls with an opaque fill or none (a shadow shows through a translucent fill).
pub(crate) fn focus_ring(t: Theme) -> Vec<BoxShadow> {
    ring(t)
}

/// The focus ring: see [`focus_ring`].
pub(crate) fn ring(t: Theme) -> Vec<BoxShadow> {
    vec![BoxShadow::new(px(0.), px(0.), t.accent_text).spread_radius(px(2.))]
}

/// A small key cap such as ↵ or ⌘.
pub(crate) fn keycap(text: &str, t: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .min_w(px(18.))
        .h(px(18.))
        .px(px(if text.chars().count() > 1 { 5. } else { 0. }))
        .rounded(px(radius::KEY))
        .bg(t.fill8)
        .ramp(ty::CAPTION)
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.ink2)
        .child(text.to_string())
}

/// A tooltip: the name of the control and, when it has one, its shortcut.
struct Tip {
    label: SharedString,
    keys: Vec<SharedString>,
}

impl Render for Tip {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let t = Theme::new(super::theme::is_dark(window));
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(sp::S))
            .h(px(28.))
            .px(px(sp::M))
            .rounded(px(radius::CONTROL))
            .bg(t.window)
            .border_1()
            .border_color(t.hair)
            .shadow_md()
            .ramp(ty::SMALL)
            .text_color(t.ink)
            .child(self.label.clone())
            .children(self.keys.iter().map(|k| keycap(k, t).into_any_element()))
    }
}

/// A tooltip builder for `.tooltip(...)`: a label and the keys that do the same thing.
pub(crate) fn tip(
    label: &str,
    keys: &[&str],
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let label = SharedString::from(label.to_string());
    let keys: Vec<SharedString> = keys
        .iter()
        .map(|k| SharedString::from(k.to_string()))
        .collect();
    move |_, cx| {
        let (label, keys) = (label.clone(), keys.clone());
        cx.new(|_| Tip { label, keys }).into()
    }
}

/// The platform's command key as a key cap: ⌘ on a Mac, Ctrl elsewhere.
pub(crate) fn cmd() -> &'static str {
    if cfg!(target_os = "macos") {
        "\u{2318}"
    } else {
        "Ctrl"
    }
}

/// The kit input's colours, from the theme.
pub(crate) fn editor_style(t: Theme) -> gpui_base::input::InputEditorStyle {
    gpui_base::input::InputEditorStyle {
        foreground: t.ink,
        muted_foreground: t.ink3,
        background: super::theme::col(0.0, 0.0, 0.0, 0.0),
        border: super::theme::col(0.0, 0.0, 0.0, 0.0),
        caret: t.accent_text,
        selection: t.accent_text.opacity(if t.dark { 0.38 } else { 0.24 }),
        ..Default::default()
    }
}

/// Round `v` to half a point so one-pixel lines stay crisp.
pub(crate) fn snap(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

/// A card: a raised surface with a hairline.
pub(crate) fn card(t: Theme) -> Div {
    div()
        .flex()
        .flex_col()
        .rounded(px(radius::CARD))
        .bg(t.card)
        .border_1()
        .border_color(t.hair)
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
    button_ex(id, label, kind, icon_name, true, t, on_click)
}

/// A button that can be switched off. A disabled button is painted flat, takes no hover, keeps its
/// place and says so to assistive technology.
pub(crate) fn button_ex(
    id: impl Into<ElementId>,
    label: &str,
    kind: ButtonKind,
    icon_name: Option<&str>,
    enabled: bool,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let toward = if t.dark {
        super::theme::Rgb(1.0, 1.0, 1.0)
    } else {
        super::theme::Rgb(0.0, 0.0, 0.0)
    };
    let fill = t.palette.accent_fill;
    let (bg, hover, press, fg) = match (kind, enabled) {
        (_, false) => (t.fill8, t.fill8, t.fill8, t.ink3),
        (ButtonKind::Primary, true) => (
            t.accent,
            fill.mix(toward, 0.08).hsla(1.0),
            fill.mix(toward, 0.16).hsla(1.0),
            t.on_accent,
        ),
        (ButtonKind::Secondary, true) => (t.fill8, t.fill12, t.fill12.opacity(1.6), t.ink),
        (ButtonKind::Ghost, true) => (gpui::transparent_black(), t.fill8, t.fill12, t.ink2),
    };
    focus_border(div().id(id), t)
        .role(Role::Button)
        .aria_label(label.to_string())
        .when(!enabled, |d| d.aria_description("Not available right now"))
        .when(enabled, |d| d.tab_stop(true))
        .flex()
        .flex_none()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .h(px(32.))
        .px(px(12.))
        .rounded(px(radius::CONTROL))
        .bg(bg)
        .ramp(ty::BODY)
        .font_weight(FontWeight::MEDIUM)
        .text_color(fg)
        .when(enabled, |d| {
            d.hover(move |s| s.bg(hover))
                .active(move |s| s.bg(press))
                .on_click(on_click)
        })
        .when_some(icon_name, |d, n| d.child(icon(n, 14., fg)))
        .child(label.to_string())
}

/// A link: the accent as text, underlined on hover, the pointing hand (the only place it is used).
pub(crate) fn link(
    id: impl Into<ElementId>,
    label: &str,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    focus_border(div().id(id), t)
        .role(Role::Link)
        .aria_label(label.to_string())
        .tab_stop(true)
        .flex()
        .flex_none()
        .items_center()
        .h(px(HIT))
        .px(px(sp::XS))
        .rounded(px(radius::KEY))
        .ramp(ty::BODY)
        .text_color(t.accent_text)
        .cursor_pointer()
        .hover(|s| s.underline())
        .on_click(on_click)
        .child(label.to_string())
}

/// A pill showing "label value", for the options a request filled in.
pub(crate) fn chip(label: &str, value: &str, t: Theme) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .h(px(HIT))
        .px(px(sp::M))
        .rounded_full()
        .border_1()
        .border_color(t.hair)
        .bg(t.field)
        .ramp(ty::SMALL)
        .text_color(t.ink2)
        .child(label.to_string())
        .child(
            div()
                .font_weight(FontWeight::MEDIUM)
                .text_color(t.ink)
                .child(value.to_string()),
        )
}

/// A small status badge ("Built in", "Network").
pub(crate) fn badge(text: &str, tone: Option<Hsla>, t: Theme) -> impl IntoElement {
    let c = tone.unwrap_or(t.ink2);
    div()
        .flex()
        .flex_none()
        .items_center()
        .h(px(20.))
        .px(px(sp::S))
        .rounded_full()
        .bg(tone.map_or(t.fill8, |c| c.opacity(0.12)))
        .ramp(ty::CAPTION)
        .font_weight(FontWeight::MEDIUM)
        .text_color(c)
        .child(text.to_string())
}

/// A calm status row: a coloured icon, the message in the primary text colour, a faint tinted
/// ground. Colour carries the kind; the words carry the meaning.
pub(crate) fn notice(
    icon_name: &str,
    tone: Hsla,
    message: impl Into<SharedString>,
    t: Theme,
) -> Div {
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(10.))
        .px(px(sp::M))
        .py(px(10.))
        .rounded(px(radius::ROW))
        .bg(tone.opacity(0.12))
        .ramp(ty::BODY)
        .text_color(t.ink)
        .child(div().pt(px(2.)).child(icon(icon_name, 14., tone)))
        .child(div().flex_1().min_w_0().child(message.into()))
}

/// How a choice value reads on a segment: sentence case, file formats in capitals, a few names
/// spelled as their makers spell them ("auto" becomes "Auto", "jpg" becomes "JPG").
pub(crate) fn segment_label(v: &str) -> String {
    const FORMATS: &[&str] = &[
        "jpg", "jpeg", "png", "webp", "avif", "heic", "heif", "tiff", "tif", "gif", "bmp", "pdf",
        "mp3", "mp4", "wav", "flac", "aac", "ogg", "m4a", "mov", "mkv", "webm", "h264", "hevc",
        "av1", "docx", "txt", "csv", "json", "svg", "ico", "zip", "exr", "dng", "raw", "srt",
        "vtt", "html", "epub", "odt", "rtf", "hdr", "sdr", "ocr", "sha256", "sha1", "md5",
    ];
    let spaced = crate::form::humanize_value(v);
    match spaced.as_str() {
        "yunet" => return "YuNet".to_string(),
        "isnet" => return "IS-Net".to_string(),
        s if FORMATS.contains(&s) => return s.to_ascii_uppercase(),
        _ => {}
    }
    let mut chars = spaced.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => spaced,
    }
}

/// A quiet note: a small icon and a smaller line of text on the faintest ground. For facts a
/// person may want to know, not a problem to fix.
pub(crate) fn note(icon_name: &str, tone: Hsla, message: impl Into<SharedString>, t: Theme) -> Div {
    div()
        .flex()
        .flex_row()
        .items_start()
        .gap(px(10.))
        .px(px(sp::M))
        .py(px(10.))
        .rounded(px(radius::ROW))
        .bg(t.fill8)
        .ramp(ty::SMALL)
        .text_color(t.ink2)
        .child(div().pt(px(1.)).child(icon(icon_name, 14., tone)))
        .child(div().flex_1().min_w_0().child(message.into()))
}

/// A segmented control: one of several values, all the same width, in as many equal columns as
/// fit. Left and Right move the selection when it has focus.
pub(crate) fn segmented(
    id: String,
    label: &str,
    values: &[String],
    selected: Option<&str>,
    focus: &FocusHandle,
    t: Theme,
    on_pick: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
) -> Stateful<Div> {
    let long = values.iter().any(|v| v.chars().count() > 12);
    let cols = if long { 2 } else { values.len().clamp(1, 4) };
    let cell = 1.0 / cols as f32;
    focus_border(div().id(SharedString::from(id.clone())), t)
        .role(Role::RadioGroup)
        .aria_label(label.to_string())
        .track_focus(focus)
        .tab_stop(true)
        .flex()
        .flex_row()
        .flex_wrap()
        .rounded(px(radius::ROW))
        .bg(t.fill8)
        .children(values.iter().enumerate().map(|(i, v)| {
            let on = selected == Some(v.as_str());
            let pick = on_pick.clone();
            let value = v.clone();
            let shown = segment_label(v);
            div()
                .id(SharedString::from(format!("{id}-{i}")))
                .role(Role::RadioButton)
                .aria_label(shown.clone())
                .aria_selected(on)
                .on_a11y_action(AccessibleAction::Click, {
                    let pick = on_pick.clone();
                    let value = v.clone();
                    move |_, window, cx| pick(value.clone(), window, cx)
                })
                .w(relative(cell))
                .h(px(HIT))
                .p(px(1.))
                .on_click(move |_, window, cx| pick(value.clone(), window, cx))
                .child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(sp::S))
                        .rounded(px(radius::CONTROL - 2.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .ramp(ty::BODY)
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(if on { t.ink } else { t.ink2 })
                        .when(on, |d| {
                            d.bg(if t.dark { t.fill12 } else { t.field }).shadow_sm()
                        })
                        .when(!on, |d| d.hover(move |s| s.bg(t.fill8)))
                        .child(shown),
                )
        }))
}

/// A switch with its label to the right. The whole row is the target.
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
        .focus_visible(move |s| s.shadow(ring(t)))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(10.))
        .min_h(px(HIT))
        .rounded(px(radius::CONTROL))
        .on_click(move |_, window, cx| toggle(window, cx))
        .child(
            div()
                .flex_none()
                .relative()
                .w(px(38.))
                .h(px(22.))
                .rounded_full()
                .bg(if on { t.accent } else { t.fill12 })
                .when(!on && t.high_contrast, |d| {
                    d.border_1().border_color(t.hair)
                })
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(if on { 18. } else { 2. }))
                        .size(px(18.))
                        .rounded_full()
                        .bg(if on { t.on_accent } else { gpui::white() })
                        .shadow_sm(),
                ),
        )
        .child(
            div()
                .ramp(ty::BODY)
                .text_color(t.ink)
                .child(label.to_string()),
        )
}

/// A thin progress bar.
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
        .child(div().h_full().w(relative(f)).rounded_full().bg(t.accent))
}

/// A round icon button: the number stepper's plus and minus, back, remove. 28 pt, a tooltip that
/// names it, filled when it lives in a form and plain when it lives in a toolbar.
pub(crate) fn icon_button(
    id: impl Into<ElementId>,
    label: &str,
    icon_name: &str,
    filled: bool,
    t: Theme,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    focus_border(div().id(id), t)
        .role(Role::Button)
        .aria_label(label.to_string())
        .tab_stop(true)
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(HIT))
        .rounded(px(radius::CONTROL))
        .when(filled, |d| d.bg(t.fill8))
        .hover(move |s| s.bg(t.fill12))
        .active(move |s| s.bg(t.fill12.opacity(1.6)))
        .tooltip(tip(label, &[]))
        .child(icon(icon_name, 14., t.ink2))
        .on_click(on_click)
}

/// A hairline rule.
pub(crate) fn rule(t: Theme) -> Div {
    div().w_full().h(px(0.5)).flex_none().bg(t.hair)
}

/// A section heading on a page.
pub(crate) fn heading(text: &str, t: Theme) -> Div {
    div()
        .ramp(ty::HEADLINE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(t.ink)
        .child(text.to_string())
}

/// A small label above a group of controls.
pub(crate) fn caption(text: &str, t: Theme) -> Div {
    div()
        .ramp(ty::SMALL)
        .font_weight(FontWeight::MEDIUM)
        .text_color(t.ink2)
        .child(text.to_string())
}

/// The Panoply mark: one accent square and three ink squares, the same four-square mark as the
/// icon. `size` is the side of the whole mark.
pub(crate) fn brand_mark(size: f32, t: Theme) -> impl IntoElement {
    let gap = (size * 0.1).max(1.0);
    let sq = (size - gap) / 2.0;
    let r = sq * 0.27;
    let blue = super::theme::rgb8((45, 108, 223), 1.0);
    let ink = if t.dark {
        super::theme::rgb8((226, 228, 235), 1.0)
    } else {
        super::theme::rgb8((31, 34, 42), 1.0)
    };
    let cell = move |c: Hsla| div().size(px(sq)).rounded(px(r)).bg(c);
    div()
        .flex()
        .flex_none()
        .flex_col()
        .gap(px(gap))
        .child(div().flex().gap(px(gap)).child(cell(blue)).child(cell(ink)))
        .child(div().flex().gap(px(gap)).child(cell(ink)).child(cell(ink)))
}

/// The icon for a file, from what its bytes say it is.
pub(crate) fn file_icon(f: &crate::profile::FileProfile) -> &'static str {
    use panoply_core::FamilyHint;
    match f.sniffed.family_hint {
        FamilyHint::Image => "file-image",
        FamilyHint::Document | FamilyHint::Text => "file-text",
        FamilyHint::Audio => "file-audio",
        FamilyHint::Video => "file-video",
        FamilyHint::Archive => "file-archive",
        FamilyHint::Font | FamilyHint::Unknown => "file",
    }
}

/// The category a file's type is shown in. One colour per type everywhere: the chip on the bar,
/// the file rows, the result card.
pub(crate) fn file_category(f: &crate::profile::FileProfile) -> crate::catalog::Category {
    use crate::catalog::Category;
    use panoply_core::FamilyHint;
    match f.sniffed.family_hint {
        FamilyHint::Image => Category::Images,
        FamilyHint::Document if f.sniffed.ext == "pdf" => Category::Pdf,
        FamilyHint::Document => Category::Documents,
        FamilyHint::Audio => Category::Audio,
        FamilyHint::Video => Category::Video,
        FamilyHint::Text => Category::Text,
        FamilyHint::Archive | FamilyHint::Font | FamilyHint::Unknown => Category::Files,
    }
}

/// A category tile: the tint behind the glyph, the glyph passing 3:1 on it. Every tile in the app
/// is drawn by this one function, so the glyph is always half the tile, the corner a quarter of
/// it, and the stroke the same weight.
pub(crate) fn category_tile(
    c: crate::catalog::Category,
    name: &str,
    size: f32,
    t: Theme,
    surface: super::theme::Rgb,
) -> Div {
    let (tile, glyph) = super::theme::tile_colors(c, t, surface);
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px((size / 4.0).round()))
        .bg(tile)
        .child(icon(name, (size * 0.5).round(), glyph))
}

/// A thin scroll indicator for a column that scrolls: `offset` and `viewport` and `content` in
/// points. `None` when everything fits. Put it as an absolute child of a `relative` wrapper that
/// holds the scroller, so it stays in place while the content moves.
pub(crate) fn thumb(offset: f32, viewport: f32, content: f32, t: Theme) -> Option<Div> {
    if viewport <= 0.0 || content <= viewport + 0.5 {
        return None;
    }
    let len = (viewport * viewport / content).max(32.0).min(viewport);
    let travel = content - viewport;
    let top = (offset / travel).clamp(0.0, 1.0) * (viewport - len - 8.0) + 4.0;
    Some(
        div()
            .absolute()
            .right(px(3.))
            .top(px(top))
            .w(px(3.))
            .h(px(len - 8.0))
            .rounded_full()
            .bg(t.ink3.opacity(0.6)),
    )
}

/// [`thumb`] for a GPUI scroll handle (reads last frame's layout).
pub(crate) fn scroll_thumb(h: &ScrollHandle, t: Theme) -> Option<Div> {
    let viewport = h.bounds().size.height.as_f32();
    let max = h.max_offset().y.as_f32().abs();
    let offset = (-h.offset().y.as_f32()).clamp(0.0, max);
    thumb(offset, viewport, viewport + max, t)
}

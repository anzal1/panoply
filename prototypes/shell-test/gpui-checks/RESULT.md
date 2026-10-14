# GPUI vs SwiftUI on macOS for Panoply: two pass/fail checks

Status: the two bars below were written before any source was inspected for these checks (only the prototype README had been read).

## The bars

### Check 1: text input (IME)
PASS if GPUI itself, or GPUI Kit (github.com/longbridge/gpui-kit, Apache-2.0), gives a text field with all of the following
without us writing the machinery:
- macOS input method system via NSTextInputClient: marked text, composition (Hindi/Devanagari, Bengali, Chinese pinyin, Japanese), dead keys and accents
- text selection
- copy, cut, paste
- undo
- caret blink

Method: read GPUI's platform text input path (InputHandler, EntityInputHandler, marked text range, macOS window NSTextInputClient impl)
and the kit's Input/TextInput. Swap the prototype's hand-drawn field for the kit's input (or an EntityInputHandler field), keep the bench
control port working, build at low priority, confirm it compiles. Run bench.py only if the window never becomes visible.

### Check 2: accessibility
PASS if GPUI exposes UI elements to the macOS accessibility tree (NSAccessibility) well enough for VoiceOver to read the search
field, list rows and buttons, or if a working integration (for example AccessKit) exists that we can adopt.

Method: source inspection at the pinned commit (a63db9fd) and recent main, Zed issue tracker and discussions, GPUI Kit status.
VoiceOver is not run. If partial: estimate work to add what Panoply needs, and whether it is upstreamable.

### Recommendation options
GPUI for all desktops, GPUI for Windows/Linux only, or SwiftUI stays on Mac.

## Verdicts at a glance

| Check | Verdict | Basis |
|---|---|---|
| 1. Text input / IME | PASS on source evidence, human IME run still pending | GPUI core owns the NSTextInputClient bridge; GPUI Kit's Input supplies the field logic |
| 2. Accessibility | PASS with gaps (field text runs, combobox pattern) | AccessKit is in GPUI core with a macOS adapter, merged 2026-05-27, and Zed is adopting it |

Recommendation: **GPUI for all desktops, gated on the two human checks below.** If the Hindi test fails, the fallback is SwiftUI stays on Mac.

## What was and was not run

- Compiled: `kit-variant` passes `cargo check --release` (nice 19, `-j2`, 1m33s cold). No binary was built or launched, no window was opened,
  nothing touched System Settings, VoiceOver was not run.
- `bench.py` was **skipped**. Every bench path launches the app, and `/selftest` calls `bar.show()` and waits for `Phase::Shown`
  (`gpui/src/main.rs`, `run_selftest`), which makes the panel visible. The spec numbers for the kit variant are therefore not measured.
  The risk to those numbers is small but real: the kit Input does shaping and layout per keystroke, so the typing-to-frame p95 needs a re-run.
- IME behaviour was not exercised on a real input method. The kit's own composition tests are explicit that they simulate the protocol
  and "do not drive a native OS IME" (`crates/kit/tests/integration/input/composition.rs`, lines 1 to 5).

## Check 1: text input (IME)

### GPUI core (pinned commit a63db9fd)

Source root: `~/.cargo/git/checkouts/zed-a70e2ad075855582/a63db9f/crates/` (same tree as github.com/zed-industries/zed at that commit).

- The macOS view class adopts the real protocol: `decl.add_protocol(Protocol::get("NSTextInputClient"))` and registers `validAttributesForMarkedText`,
  `hasMarkedText`, `markedRange`, `selectedRange`, `firstRectForCharacterRange:actualRange:`, `insertText:replacementRange:`,
  `setMarkedText:selectedRange:replacementRange:`, `unmarkText`, `attributedSubstringForProposedRange:actualRange:`, `doCommandBySelector:`,
  `characterIndexForPoint:`. `gpui_macos/src/window.rs` lines 236 to 296, implementations at 3366 to 3600.
- Key routing hands events to `inputContext handleEvent:` while composing, for non-printing keys, and for printable keys when an IME source
  is active, so dead keys, pinyin, kana and Devanagari transliteration go through AppKit's input system (`window.rs` around 2690 to 2790).
  `doCommandBySelector` routes unhandled commands back to GPUI keybindings.
- The portable side is `gpui/src/input.rs`: `EntityInputHandler` (text_for_range, selected_text_range, marked_text_range, unmark_text,
  replace_text_in_range, replace_and_mark_text_in_range, bounds_for_range for the candidate window, character_index_for_point) plus
  `ElementInputHandler`, installed per frame with `Window::handle_input`.
- GPUI core does **not** ship a text field. `gpui/examples/input.rs` (784 lines) is a reference field: marked range with underline, UTF-16
  conversion, selection, mouse selection, copy/cut/paste. It has no undo and no caret blink. Writing those two on top of it is small but it
  is our code to own, and it is single-line only.
- The prototype in `gpui/src/view.rs` never called `handle_input`, so as built it bypasses the IME entirely (it inserts `key_char`).

### GPUI Kit (github.com/longbridge/gpui-kit, commit c5f8eca, 2026-10-13)

- License: Apache-2.0 (`LICENSE-APACHE`, `crates/base/Cargo.toml`). Not NOASSERTION despite what aggregators show.
- The editing engine is `gpui-base`, `crates/base/src/input/base/state.rs`: `impl EntityInputHandler for InputBaseState<M>` at line 4109,
  `ime_marked_range` state, marked underline painted in `element.rs` (around line 2662), `bounds_for_range` at 4522 for the candidate window,
  `blink_cursor.rs` (500 ms interval, 300 ms pause after typing), `undo_manager.rs` (601 lines, IME composition commits as one undo step),
  selection, copy/cut/paste, word and line movement, masking. State is 11.5k lines, which is a lot of surface to depend on.
- `gpui-base` exports an unstyled `Input` and `InputState` (`crates/base/src/input/input/mod.rs`). `gpui-component` adds the styled field, a
  16-file input folder, and the AccessKit wiring. For Panoply, `gpui-base` alone is enough.
- Dependency catch: the kit pins the crates.io snapshot `gpui-pre = "=0.3.9"` (published 2026-10-12, one day before our git pin), not
  the git commit. Two GPUI copies cannot mix, so adopting the kit means moving the whole app to `gpui-pre` and `gpui-pre-platform`.
  Source diff for our prototype was nil apart from the crate rename. The kit says each snapshot may change GPUI's API, so we inherit that churn
  through its exact-version pins.

### What I built

`prototypes/shell-test/gpui/kit-variant/` (new, inside `gpui/`): a copy of the prototype with the hand-drawn field replaced by
`gpui_base::input::Input` over an `InputState`. Pinned commit build in `gpui/` is untouched.

- `Cargo.toml`: `gpui-pre` and `gpui-pre-platform` 0.3.9, `gpui-base` 0.7.3, everything else as before.
- `view.rs`: `query`/`cursor` hand editing and all text key arms removed (about 90 lines); `Bar` keeps a `query` mirror that follows
  `InputEvent::Change`, `PressEnter` runs the selected tool, bar-level keys (up, down, ctrl-n, ctrl-p, escape, cmd-k) stay in `on_key`.
  The kit propagates keys it does not consume (`state.rs` escape ends with `cx.propagate()`), so these still reach the bar. This
  propagation is read from source, not observed.
- Palette projected onto the kit through `set_editor_style` so caret, selection and ink use the bar's tokens.
- Bench control port kept: `/toggle`, `/metrics`, `/selftest`, `/state`, `/snapshot` unchanged. `set_query` now takes a window and calls
  `InputState::set_value` (which does not emit Change, so it also mirrors and rebuilds). Launch time goes to `results/gpui-kit-launch.txt`
  so it cannot pollute the original numbers.
- `build.sh` builds at `nice -n 19`, `-j2`, wraps `ArsenalKit.app` (separate name; same port 47814 and hotkey, so quit the original first).
- Result: compiles clean (4 warnings, all inherited: unused fields and an unexpected-cfg for the dev-only snapshot feature).

### Check 1 verdict: PASS (provisional)

The bar is "supports the macOS input method system ... without us writing that machinery". NSTextInputClient is implemented by GPUI core,
the field logic including marked text, selection, clipboard, undo and blink is implemented by the kit, and our integration was about
60 lines. Provisional because:

1. No real input method was driven. The Hindi run below is the proof.
2. The panel is a non-activating `NSPanel` in an accessory app. AppKit input methods generally need the window to be key and the view to be
   first responder. The prototype's `/state` reports `key`, so this is probably fine, but candidate window placement via
   `firstRectForCharacterRange` on a blurred panel is unproven.
3. `validAttributesForMarkedText` returns an empty array, so GPUI draws the composition underline itself instead of letting the IME style it.
   Fine for Devanagari and pinyin, may look different from native fields for Japanese clause segmentation.

### Human test (about 5 minutes)

Build and launch (low priority, one time):

```sh
pkill -f Arsenal.app/Contents/MacOS 2>/dev/null   # the original build holds the same port and hotkey
cd prototypes/shell-test/gpui/kit-variant
./build.sh                                         # nice 19, -j2, a few minutes cold
open -g prototypes/shell-test/gpui/kit-variant/ArsenalKit.app
# quit afterwards:  pkill -f ArsenalKit.app/Contents/MacOS
```

Hotkey: Control+Option+4 toggles the bar.

Enable the Hindi keyboard temporarily (your own change, not made by me): System Settings, Keyboard, Text Input, Input Sources, Edit...,
the plus button, Hindi, choose **Hindi - Transliteration**, Add. Switch with the input menu in the menu bar (or Control+Space). Remove it
the same way when done.

Type with Hindi - Transliteration active, pressing Space or Return to commit each word (exact candidates can differ slightly; the point is the ligatures):

- `kshetra` should become क्षेत्र
- `shree` should become श्री
- `shraddha` should become श्रद्धा
- `namaste duniya` should become नमस्ते दुनिया

Also check, with the ABC or US keyboard: Option+E then E gives é, Option+` then A gives à (dead keys), and paste
`क्षत्रिय श्रीमान` with Command+V (tests conjunct shaping with no IME involved).

What to look for:

- While typing, the uncommitted text has a composition underline and the candidate strip appears next to the field, not at the screen corner.
- Conjuncts are joined: क्ष and श्र are single ligature clusters, not क + ् + ष with visible halant gaps. Caret never lands inside a cluster
  when moving with Left and Right.
- Backspace removes a whole cluster during composition and a whole syllable unit after commit, as in TextEdit.
- Shift+Left/Right selects, Command+A selects all, Command+C then Command+V round trips, Command+Z undoes a whole composed word in one step.
- Caret blinks (about 500 ms) and stops blinking while you type.
- Up, Down, Return and Escape still drive the list and hide the bar; list rows re-filter as composition commits.
- The panel does not lose key status when the candidate strip opens (it should not hide the bar).

## Check 2: accessibility

### GPUI core at a63db9fd (main of 2026-10-13, so "recent main" is the same tree)

AccessKit is fully integrated, on macOS as well as Windows and Linux. This is new (merged May 2026) and is not in older write-ups.

- Guide: `gpui/src/_accessibility.rs` (295 lines). Implementation: `gpui/src/window/a11y.rs` (905 lines) plus `a11y/debug.rs`.
  Example: `gpui/examples/a11y.rs` (heading, spin button with increment and decrement actions, switch, buttons, list).
- Deps: `accesskit 0.24`, `accesskit_macos 0.26` (root `Cargo.toml` lines 526 to 527), adapter in `gpui_macos/src/window.rs`:
  `a11y_init` at 2367 (`accesskit_macos::SubclassingAdapter::for_window`), `a11y_tree_update` at 2386, focus updates at 3178.
- Per frame GPUI builds a TreeUpdate from elements that have both an `.id()` and a `.role()` (`window.rs` 3673 to 3795), only when a screen reader is
  active (`is_a11y_active`), so there is no cost otherwise. `Application::new_inaccessible` opts out.
- Builder API on `div`: `role`, `aria_label`, `aria_description`, `aria_value`, `aria_placeholder`, `aria_selected`, `aria_expanded`,
  `aria_toggled`, numeric values, `aria_level`, `aria_position_in_set`, `aria_size_of_set`, row and column indices,
  `aria_active_descendant`, `on_a11y_action` (Click is auto-registered by `on_click`), and `a11y_synthetic_children` for custom
  elements such as text fields (`gpui/src/elements/div.rs` lines 1310 to 1510, 3530 to 3600).
- The macOS adapter itself supports text input semantics: `accessibilityValue`, `setAccessibilityValue:` (SetValue action),
  `accessibilitySelectedTextRange`, `accessibilityNumberOfCharacters` (`accesskit_macos-0.26.3/src/node.rs` lines 536, 555, 730, 756).
- History and field evidence (github.com/zed-industries/zed):
  - PR #56065 "gpui: Accesskit support" by cameron1024, merged 2026-05-27. Author's note: macOS VoiceOver "working well", NVDA on
    Windows works, AT-SPI nodes visible on Wayland and X11. A VoiceOver user (sportfloh) tested the example on macOS 26.5: first launch
    exposed only the title bar until app switching, fixed by a follow-up, then "all labels and controls are accessible".
  - PR #60397 landmarks and menus (merged 2026-07-13), #59429 Settings UI (2026-06-17), #64143 "Release the accessibility adapter when a
    window is dropped" (2026-09-20, a retain cycle fix), #64214 aria_current.
  - Open: #41138 (Windows screen reader tracking issue, 12 comments, still open), #57579 (Full Keyboard Access text input on macOS),
    #65111 (a dictation tool reports "No text field focused" in Zed's agent input, 2026-10-03, so the main editor does not publish a
    focused text field yet).
  - Older issues #7895 (VoiceOver) and #47186 (macOS accessibility tools cannot see UI) are closed without a fix (#7895 was closed in Dec 2025 with no explanation; #47186 the day it was filed).
- Zed is the proof of pattern, not just the library: `crates/settings_ui/src/components/input_field.rs` lines 220 to 260 and 365 to 560
  publish a text field with `Role::TextInput`, label, `aria_value`, placeholder, and `a11y_synthetic_children` producing `TextRun` nodes
  plus a `TextSelection` (caret tracking, review commands, typed-character echo). Rows use `Role` in `crates/ui/src/components/list/list_item.rs`
  and buttons in `button_like.rs`.

### GPUI Kit

- Claims AccessKit roles, names, states, relationships and actions in the interaction layer with tests (README).
- The styled `Input` in `crates/component/src/input/input.rs` lines 761 to 780 picks the role from the content type (text, password, multi-line; `accessibility_role` at line 27), `aria_label`, `aria_placeholder`, `aria_value`, a Focus action and a SetValue action. `crates/kit/tests/integration/input/constraints.rs`
  checks `Role::PasswordInput`.
- Gaps: no `a11y_synthetic_children` and no `set_text_selection` anywhere in the kit (grepped), so the field is announced and its value
  read, but there is no per-character caret tracking or typed-character echo as in Zed's settings field. The bare `gpui_base::input::Input`
  publishes no node at all, so using `gpui-base` alone means we add the wrapper ourselves.
- The kit adds `crates/base/src/macos_accessibility.rs`, an `accessibilityHitTest:` forwarder installed on the window class. Reason not documented;
  worth knowing about if hit testing misbehaves on our panel.

### What I added to the prototype

In `kit-variant/src/view.rs`, about 30 lines: search field wrapper with `Role::SearchInput`, label "Search tools", placeholder, live `aria_value`
and the input's focus handle; results container `Role::ListBox`; each row `Role::ListBoxOption` with name, subtitle as description, selected state,
position and set size, and a Click action; the remove-file chip as `Role::Button`. It compiles. It has not been observed in a tree, because
dumping the tree (`Window::debug_a11y_tree_json`) needs a live window with an assistive technology connected.

### Check 2 verdict: PASS, with three gaps

Bar: "exposes UI elements to the macOS accessibility tree well enough for VoiceOver to read the search field, list rows and buttons, or a working
integration we can adopt". AccessKit in GPUI core is that integration, maintainers and a VoiceOver user report it working on macOS, and
the node types Panoply needs exist. The gaps:

1. **Text field depth.** Value plus placeholder is announced; caret and typed-character echo need text runs and selection. Zed already wrote
   this for its settings field; porting to `InputState` (text, selected range, UTF-16 and byte offsets, marked range) is about 150 to 250 lines,
   roughly 1 to 2 days with a VoiceOver test loop. Upstreamable to gpui-kit (Apache-2.0, they already carry the Input a11y); the GPUI core
   primitives are already upstream.
2. **Selected row while typing.** The right pattern is a focused search field that owns an active descendant (combobox). GPUI's
   `aria_active_descendant` only works when the focused node is an ancestor of the row, but our field and list are siblings. Fix is to restructure
   so one focusable container wraps both and reports the field as its text, or to announce selection through a live-region node. About 0.5 to 1 day,
   and it is application code. A GPUI core change would only be needed to allow sibling references (AccessKit supports `active_descendant`
   by id; GPUI's guard is a self-imposed ancestor check in `window/a11y.rs`), which is a small upstream PR.
3. **Launcher-specific behaviour, untested anywhere.** Our window is a non-activating accessory-app `NSPanel` that is shown and hidden by a
   global hotkey. The first-launch "only the title bar reachable" bug in #56065 was fixed for ordinary windows; whether VoiceOver
   follows focus into a panel that appears without activating the app is unknown, and this is exactly the case a human VoiceOver
   pass must cover. Zero upstream coverage for it.

## Recommendation

**GPUI for all desktops, gated.** Both checks pass on source, the cost to us is bounded (an integration of about 100 lines now, and 2 to 3
days of accessibility work), and the single-codebase upside on Windows and Linux is only real if Mac joins. Gates, in order:

1. Run the Hindi test above. If composition, candidate placement or conjunct shaping fails in the panel and the cause is GPUI core, SwiftUI stays on Mac.
2. A 15 minute VoiceOver pass by the user on `ArsenalKit.app` (search field, arrowing the list, the remove-file chip, show and hide via
   hotkey). If the panel is not exposed at all, stay SwiftUI on Mac until it is.
3. Re-run `bench.py` for the kit variant when the machine is free (needs the window visible).

Why not "Windows/Linux only": nothing found is Mac-specific and broken. The Mac-specific parts of GPUI, the NSTextInputClient bridge and the
AccessKit adapter, are the best covered. Why the gating anyway: GPUI Kit rides weekly `gpui-pre` snapshots with exact pins, the a11y stack is
four months old, and a launcher panel is not the case anyone has tested. SwiftUI gives IME, VoiceOver and Liquid Glass with no work, so
GPUI has to earn the Mac slot with real runs, not with source reading.

Deviations already known and unrelated to these checks: no Liquid Glass (real blur only), no global mouse monitor for click-outside.

## References

- GPUI pinned tree: github.com/zed-industries/zed at a63db9fd901bedf1e0b52acfc1f60f30e5495449 (`crates/gpui/src/input.rs`, `crates/gpui/src/window/a11y.rs`,
  `crates/gpui/src/_accessibility.rs`, `crates/gpui/examples/input.rs`, `crates/gpui/examples/a11y.rs`, `crates/gpui_macos/src/window.rs`,
  `crates/settings_ui/src/components/input_field.rs`)
- GPUI Kit: github.com/longbridge/gpui-kit at c5f8eca4a0759f8c0c7ed624eddfd03f1c93a5f2 (`crates/base/src/input/base/state.rs`, `blink_cursor.rs`,
  `undo_manager.rs`, `crates/component/src/input/input.rs`, `crates/kit/tests/integration/input/composition.rs`)
- Zed issues and PRs: #56065, #60397, #59429, #64143, #64214, #41138, #57579, #65111, #7895, #47186
- Code touched: `prototypes/shell-test/gpui/kit-variant/` (Cargo.toml, build.sh, src/view.rs, src/main.rs). Nothing committed.

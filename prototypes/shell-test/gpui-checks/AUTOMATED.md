# GPUI text input and accessibility: automated run

Run date 2026-10-13. Companion to `RESULT.md` (checks 1 and 2 there). Everything below was produced by the `selftest-input` build of
`prototypes/shell-test/gpui/kit-variant`, with nothing shown on screen. Raw JSON for every endpoint is in `gpui-checks/automated/`
(`stock-*.json` and `patched-*.json`). Re-run with `kit-variant/selftest.sh` (stock) or `kit-variant/selftest.sh patched`.

## The bar

Same bar as `RESULT.md` "Human test" and "What to look for", turned into asserts:

1. Text input: composition through the macOS input method protocol for Hindi (Devanagari), Bengali, dead keys and pinyin; the committed
   text equals the expected text exactly and is NFC; marked range reported during composition and cleared after; Left and Right move by
   cluster and never stop inside क्ष; Backspace removes one whole cluster; Shift+Left selects a whole cluster; Cmd+Z undoes the
   whole composed word in one step; list filtering re-runs after commit.
2. Shaping: conjuncts are single clusters, no visible halant.
3. Accessibility: every interactive element has a role and a meaningful label, and the search field exposes its value.

## What ran, and why nothing was visible

- The test window is the bar's own panel, created at origin x = -20000 and never ordered front, never key. `/state` before and after every
  run reported `visible:false, key:false, alpha:0, frame:[-20000,400,720,460]`. The hotkey is not registered, `Bar::show` is not called, and the
  launch step that briefly orders the panel front (normal builds) is skipped in this build.
- Frames are produced by calling `Window::draw` directly. That lays elements out and registers the input handler exactly like a normal frame,
  and presents nothing. The process is an accessory app (LSUIElement bundle `ArsenalKitSelftest.app`), niced to 19, built with `-j2`.
- No system setting, input source, VoiceOver state or pasteboard was touched. The app was quit after each run (`pgrep` empty).
- IME driver: the real `NSTextInputClient` selectors sent to GPUI's NSView: `setMarkedText:selectedRange:replacementRange:` (as an
  attributed string, replacement range NSNotFound), `insertText:replacementRange:`, `unmarkText`, `markedRange`, `selectedRange`, `hasMarkedText`,
  `firstRectForCharacterRange:actualRange:`, `attributedSubstringForProposedRange:actualRange:`. These go through GPUI's macOS bridge
  (UTF-16 conversion included) into the kit's `EntityInputHandler`. Keys go through `Window::dispatch_keystroke` (GPUI keymap, kit actions).
- Paste: the system pasteboard is deliberately not used (it would change the user's clipboard). The paste case sends the text through
  `insertText` with no marked range, which is where the kit's `Paste` action ends (`replace_text_in_range_silent`).

## Check 1 results: IME and editing (11 cases, 2 variants)

"stock" is gpui-base 0.7.3 from crates.io as the kit variant ships. "patched" adds `patches/gpui-base-grapheme-and-unmark.diff`
(about 40 changed lines in `input/base/state.rs`, applied by `selftest.sh patched` through a cargo `patch` override; Cargo.lock restored after).

| Case | Expected | stock | patched |
|---|---|---|---|
| hindi_kshetra | क्षेत्र | FAIL (caret, selection, backspace) | PASS |
| hindi_shree | श्री | FAIL (same four) | PASS |
| hindi_shraddha | श्रद्धा | FAIL (same four) | PASS |
| hindi_namaste_duniya | नमस्ते दुनिया | FAIL (same four) | PASS |
| bengali_kshama | ক্ষমা | FAIL (same four) | PASS |
| bengali_shree | শ্রী | FAIL (same four) | PASS |
| deadkey_acute_e | é (U+00E9) | PASS | PASS |
| pinyin_nihao | 你好 | PASS | PASS |
| paste_conjuncts | क्षत्रिय श्रीमान | FAIL (caret only) | PASS |
| shrink_then_unmark | क्ष | FAIL (4 checks plus list stale) | PASS |
| latin_filter_control | compress (control) | PASS | PASS |

Totals: stock 3 of 11 cases pass, patched 11 of 11. Per check, across all cases, stock fails only these: caret left by cluster 8 of 11,
caret right by cluster 8 of 11, Shift+Left selects a whole cluster 7 of 11, Backspace removes a whole cluster 7 of 11, list re-filter after
`unmarkText` 1 of 11. Everything else passes in both variants, on every case:

- committed text exactly equals expected and is NFC (11 of 11)
- marked range reported on every `setMarkedText` step, equal to (composition start, UTF-16 length) (49 of 49 steps), marked text present in the
  buffer (49 of 49), caret inside the marked range (49 of 49)
- marked range cleared after `insertText` commit (12 of 12) and after `unmarkText` (1 of 1)
- Cmd+Z undoes the whole composed word in one step (11 of 11; for "नमस्ते दुनिया" one undo removes only the last word and leaves "नमस्ते ")
- Cmd+Shift+Z restores (11 of 11)
- `firstRectForCharacterRange` for the marked range lands on the field inside the window frame (10 of 10 composing cases); for example the rect
  starts 50 pt from the window left edge, exactly where the field starts after the icon, and moves right as the marked text grows
- list filtering re-runs after commit for `insertText` commits (the "compress" control returns 4 matches, equal to a fresh filter)

What stock gets wrong, with numbers (see `stock-imetest.json`):

- The kit moves the caret, extends selection and deletes by code point, not by grapheme cluster. `previous_boundary` and `next_boundary`
  in `state.rs` only clip to a char boundary; unicode-segmentation is imported but used only to validate inline tokens.
- क्षेत्र (7 UTF-16 units): Left from the end stops at 7, 6, 5, 4, 3, 2, 1, 0; the cluster stops are 7, 4, 0. Stops at 6, 5, 3, 2, 1 are inside
  त्र and क्षे. Shift+Left selects "र" instead of "त्र". Backspace leaves "क्षेत्" (one code point removed) instead of "क्षे".
  Same pattern for Bengali (ক্ষমা: Backspace leaves "ক্ষম" instead of "ক্ষ", Shift+Left selects "া" instead of "মা").
- The `unicode-segmentation` 1.13.3 in the lock file already implements the Indic conjunct rule (GB9c): it segments क्षेत्र as [क्षे, त्र] and
  ক্ষমা as [ক্ষ, মা], so the library is fine and the kit does not use it for movement. The test's own conjunct merge gives the same boundaries.
- `unmarkText` (commit without `insertText`) neither repaints nor emits `InputEvent::Change`, so the bar's mirrored query and the list stay
  stale: bar query "" with 75 rows after the field holds "क्ष". Rare in practice (AppKit uses it on focus loss), one-line fix.

What the patch does: `previous_boundary` and `next_boundary` snap to the grapheme boundary of the line with `GraphemeCursor`; `unmark_text`
emits `Change` and notifies when text was marked. With it, every stop list equals the cluster list (क्षेत्र: 7, 4, 0), Backspace removes
"त्र" (3 code points), Shift+Left selects "त्र", and Cmd+Z after Backspace restores. Forward Delete shares `next_boundary` but was not asserted.

Bar-owned keys through the kit input (`/keystest`, both variants, 5 of 5): Down, Up, Ctrl+N, Ctrl+P propagate from the focused kit input to
the bar (this was "read from source, not observed" in RESULT.md; now observed), Return runs the selected tool, Escape clears the query first.

## Shaping and the PNG (`stock-shapetest.json`)

Text "क्षेत्र श्री श्रद्धा নমস্কার ক্ষমা", 20 pt, `.SystemUIFont` with CoreText fallback (fonts id 28 for Devanagari, 29 for Bengali; GPUI does not
expose their names), through `TextSystem::shape_line`: 34 scalars become 20 glyphs.

| Conjunct | Scalars | Glyphs | Glyph starting inside it | Lone halant glyph present |
|---|---|---|---|---|
| क्ष | 3 | 1 (id 125) | none | no |
| त्र | 3 | 1 (206) | none | no |
| श्र (twice) | 3 | 1 (218) | none | no |
| द्ध | 3 | 1 (158) | none | no |
| স্ক | 3 | 1 (121) | none | no |
| ক্ষ | 3 | 1 (64) | none | no |

Visible halant glyph ids (the last glyph of a lone U+094D and U+09CD, ids 923 and 137) appear nowhere in the line. Shaper clusters per word:
क्षेत्र is [क्ष, े, त्र], श्री is [श्र, ी], श्रद्धा is [श्र, द्ध, ा], নমস্কার is [ন, ম, স্ক, া, র], ক্ষমা is [ক্ষ, ম, া].
Matras are separate glyphs, which is normal; the caret clusters merge them (क्षे).

PNG: `gpui-checks/hindi-field.png` (760 by 150). It is the real kit Input rendered by `Window::render_to_image` in the off-screen window, light
appearance, text 30 pt on the 2.0 backing store = 60 device pixels per em, which is the pixel size a 3x display gives 20 pt text. GPUI cannot
set a scale factor of 3 on a macOS window, so this is the equivalent, not a literal 3.0. I inspected it at 3x enlargement: all conjuncts
are joined, no halant gap anywhere, headline bar continuous. Cosmetic only: faint darker dots on the Devanagari headline where two glyph
outlines overlap (alpha blending per glyph); worth a look on the real screen.

## Check 2 results: accessibility (`*-a11ydump.json`, identical in both variants)

Method: GPUI has no switch to force the tree, so the adapter is woken the way an assistive technology wakes it, by asking the content view
for `accessibilityChildren`. GPUI then flags accessibility active, a `Window::draw` builds the `TreeUpdate`, and the tree is read back twice:
from GPUI (`debug_a11y_tree_json`) and from AppKit through the NSAccessibility selectors of accesskit_macos 0.26.3's nodes.

GPUI tree (query "pdf", dropped file report.pdf), 14 nodes:

```
Window (root, no title: accesskit drops it on purpose)
  SearchInput   label "Search tools"  value "pdf"  placeholder "Search 75 tools or drop a file"  actions Focus
  Button        label "Remove dropped file"                      actions Click
  ListBox       label "Tools"
    ListBoxOption "PDF to Word"    selected true   position 1 of 18  description "Editable DOCX from PDF"
    ListBoxOption "PDF to Images"  selected false  position 2 of 18  ... 8 rows in view
  Button        label "Open selected tool"   shortcut Return   actions Click
  Button        label "Actions"              shortcut Meta+K   actions Click
```

AppKit view of the same tree: AXGroup (window) > AXTextField "Search tools" value "pdf" placeholder; AXButton "Remove dropped file";
AXList "Tools" > AXStaticText rows (title, help = subtitle, `isAccessibilitySelected` true on the first); AXButton "Open selected tool";
AXButton "Actions". All three buttons answer `isAccessibilitySelectorAllowed(accessibilityPerformPress)`.

| Check | Result |
|---|---|
| Adapter activates with no screen reader, GPUI builds tree | PASS |
| Search field has role and label | PASS (GPUI SearchInput, AppKit AXTextField, "Search tools") |
| Search field exposes its value | PASS in GPUI and AppKit, for "pdf" and for committed "क्षेत्र" |
| Result list and rows: role, label, selected state | PASS (8 rows, exactly one selected) |
| Selected state follows the arrow key | PASS (row 1 true to false, row 2 false to true) |
| Footer buttons: role and label | PASS (added in this run: they had no node before) |
| Remove-file button: role and label | PASS |
| Every interactive node has a non-empty label | PASS (none missing) |
| `accessibilityPerformPress` on "Open selected tool" | PASS, runs the tool ("Opened PDF to Word") |
| `accessibilityPerformPress` on "Remove dropped file" | PASS, file removed |
| AppKit `accessibilitySelectedTextRange` and character count on the field | FAIL: returns (0,0) and 0 for a caret at 3 of 3 |

The one FAIL is expected and is not part of the required bar: accesskit_macos reports a text range only when the node has text-run children and a
text selection (`a11y_synthetic_children`), and the bare kit Input publishes neither. So a screen reader gets the field's value but no caret
position or typed-character echo. Estimate unchanged from RESULT.md: about 150 to 250 lines.

Other things the dump shows:

- `ListBoxOption` maps to AXStaticText on macOS (accesskit_macos has no option mapping), so rows are announced as selected static text in a list.
  A `Row` role inside a table-like parent would map to AXRow and enable `accessibilitySelectedRows`. Not tested for VoiceOver wording.
- GPUI's own focus is the search field. The AppKit `accessibilityFocusedUIElement` is nil because the window was never key, which is what
  this run guarantees, so focus-follow cannot be judged here.
- Section headers and the "Opened X" footer message are not exposed (no role). There is no live region, so the result of running a tool
  is not announced.
- Code changed for accessibility: the two footer hint groups became `Role::Button` nodes with labels, key shortcuts and Click actions.

## Verdict

- Text input: GPUI core passes. The NSTextInputClient bridge, marked-range bookkeeping, UTF-16 handling, NFC output, one-step undo of a composed
  word, candidate rect anchoring and list filtering all pass on Hindi, Bengali, dead key and pinyin. The stock kit does not pass the cluster
  bar: caret, selection and Backspace move by code point and split क्ष, त्र, श्र. That is a defect in gpui-base's input, not in GPUI, and
  the 40-line patch in `patches/` fixes it and also fixes the `unmarkText` staleness (11 of 11). Treat check 1 as PASS conditional on carrying
  (and upstreaming) that patch. Without it, a Hindi user presses Left over क्षेत्र and watches the caret sit inside a letter.
- Accessibility: PASS on the required bar (every interactive element has a role and label, the field exposes its value, selection and press
  actions work end to end through AppKit's own selectors), with the known gaps: no caret or selected-range for the field, rows are static text
  to AppKit, no live announcement of results.
- Net for Panoply: GPUI is viable on Mac for the command bar. It stays gated on the human checks below.

## What still truly needs a human

1. Real input method: the actual Hindi - Transliteration call sequence (it may pass non-NotFound replacement ranges, or call
   `attributedSubstringForProposedRange`, `selectedRange` and `characterIndexForPoint` between steps). This run proves the handler handles the
   standard sequence, not that Apple's IME sends only that sequence.
2. Candidate window placement and behaviour in the real non-activating panel: whether the strip opens at the field, whether the panel
   stays key and visible while it is open. This run proves only that `firstRectForCharacterRange` returns a rect on the field.
3. The AppKit key route: keys here enter at `Window::dispatch_keystroke`, not through `interpretKeyEvents` and `doCommandBySelector`
   with a live input context (Left, Backspace and Cmd+Z while an IME is active).
4. Real Cmd+V, Cmd+C, Cmd+X (pasteboard not touched on purpose) and Cmd+A beyond the one use here.
5. Caret blink (500 ms, pauses while typing): not asserted; the window is never active, and the kit shows the caret only for an active window.
6. VoiceOver itself: speech wording, focus following into a panel that appears without activating the app (first launch and hotkey show),
   `accessibilityFocusedUIElement` with a key window, how rows read as AXStaticText, and typed-character echo (known missing).
7. Native feel of Backspace on Indic text. The bar here is "one whole cluster", as written in RESULT.md; if TextEdit on this Mac removes a
   conjunct differently, say so and the patch is a few lines to adjust.
8. The shirorekha dots in `hindi-field.png` at the real screen scale.

## Files

- `kit-variant/Cargo.toml`: feature `selftest-input` (implies `snapshot`), `[profile.selftest]`.
- `kit-variant/src/selftest.rs`: new, the four endpoints. `src/main.rs`: routes `/imetest /keystest /shapetest /a11ydump`, off-screen window
  origin, hotkey skipped (all behind the feature). `src/view.rs`: feature-gated accessors and off-screen placement, footer button nodes
  (always on), field-only zoom render used for the PNG. `src/native.rs`: `set_origin`, `ns_view`, `content_view`.
- `kit-variant/selftest.sh`, `kit-variant/patches/gpui-base-grapheme-and-unmark.diff`, `kit-variant/patches/patched/` (patched crate copy).
- `gpui-checks/hindi-field.png`, `gpui-checks/automated/*.json`. Nothing committed.

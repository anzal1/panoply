# UI audit

Panoply's desktop UI against Apple's Human Interface Guidelines (macOS), Microsoft Fluent 2 (Windows) and the GNOME HIG (Linux), for what applies to a GPUI app. **55 findings: 47 fixed, 8 open.** Open items are listed at the end with the reason each was left.

The renders in [review/before](review/before) and [review/after](review/after) come from `panoply-desktop --selfcheck-review` (macOS, offscreen, 2x, nothing is shown on screen). Colour, hit-size and window-state rules are unit tests in `apps/desktop/src/ui/theme.rs` and `window_state.rs`; the keyboard reach check is part of `--selfcheck`.

## How it was measured

- **Contrast.** `Palette` holds every colour as plain data. The tests compute WCAG 2.2 ratios for text on every surface (4.5:1, primary 7:1), the accent button label (4.5:1), the focus ring and the switch (3:1), category glyphs on their tiles (3:1), and the bar's text over a black, a grey and a white desktop. They run for ten accent colours in light, dark and high contrast. Primary text in dark mode is asserted to sit between 13.5 and 15.5:1 and never to be pure white.
- **Grid and ramp.** A test checks that the spacing steps are multiples of 4 pt, the hit size is at least 28 pt and every line height is an even number at least 2 pt over its size.
- **Renders.** Every state in light and dark at 2x, inspected at 3x for clipping, alignment and spacing. `review/after/frame-times.tsv` has the steady-state frame cost of each.
- **Keyboard.** The headless run presses Tab sixteen times on Home and records the label of what takes focus.
- **Not measured.** The native traffic lights, the Windows and Linux frames and the Windows and GNOME accessibility settings are not in an offscreen macOS render. That code compiles in CI on all three systems and has not been run on a Windows or Linux desktop.

## Findings

| # | Area | Finding | Guideline | Fix | Status |
|---|---|---|---|---|---|
| 1 | Layout | Spacing was ad hoc (6, 10, 14, 18 pt and more) with no shared steps. | HIG layout, Fluent 2 spacing (4/8 pt), GNOME HIG (6/12 px) | `theme::sp` has the 4 pt steps and every page margin, gap and row uses them; whole screens sit on the 8 pt grid. The one exception is the 6 and 10 pt gaps inside buttons, chips and rows (icon to label), which are kept for optical balance. | Fixed |
| 2 | Layout | Pages ran the full window width and the toolbar title did not line up with the content below it. | HIG layout margins | One centred column, 920 pt of content between 24 pt margins; the 52 pt toolbar zone uses the same column, so the title and the content share one left edge at every window size. | Fixed |
| 3 | Layout | Home had a fixed-width tile grid that left a gap on the right, and a stub Nearby column that said "later phase". | HIG layout | Quick actions are a three-column grid that fills the width; the placeholder moved to the Nearby page (a designed "arrives later" state). | Fixed |
| 4 | Layout | The estimate row sat at the bottom of the files column, far from the files it describes. | HIG layout, proximity | It follows the file list. | Fixed |
| 5 | Layout | The consent sheet was taller than the bar and its icon was cut by the header line. | HIG sheets, no clipped content | Icon and title share a row, rows are tighter, the body scrolls if the window is small. | Fixed |
| 6 | Layout | Scrolling columns (options, results, long pages, the bar list) cut a control in half at the edge with no hint that there was more. | HIG scroll views, WCAG 1.4.10 | A thin scroll indicator appears whenever a column scrolls. It is an indicator, not a draggable thumb (see open items). | Fixed |
| 7 | Layout | Long lists drew every row on every frame (Modules took 4.8 ms warm for 31 rows). | Performance budget, 120 fps | Modules and History build only the rows on screen plus four above and below; 2.9 ms warm. | Fixed |
| 8 | Type | Twelve text sizes (11, 11.5, 12, 12.5, 13, 13.5, 14, 15, 16, 17, 20, 22). | HIG typography, Fluent 2 type ramp | One ramp, `theme::ty`: 11, 12, 13 (body), 15, 17, 22 and 20 for the bar's search field, each with a line height. | Fixed |
| 9 | Type | Choice labels mixed case ("auto" next to "Auto", "jpg", "yunet"). | HIG writing, Fluent 2 content design | Sentence case, file formats in capitals (JPG, PNG), brand spellings (YuNet). | Fixed |
| 10 | Type | A failed job showed a long technical sentence as a 17 pt bold title. | HIG alerts | A fixed short title, the message in body text, the next step in secondary text. | Fixed |
| 11 | Colour | Tertiary text (placeholders, help text, footers, metadata) measured 2.6 to 2.9:1. | WCAG 2.2 1.4.3 (4.5:1), HIG color, Fluent 2 contrast | Text tokens are opaque colours; every text token is at least 4.5:1 on window, sidebar, card and field in light and dark. Unit tests (`ui::theme::tests`) fail the build otherwise. | Fixed |
| 12 | Colour | Dark text was white at 85% alpha, so its contrast changed with the surface (about 12:1) and it was close to pure white. | HIG dark mode, owner brief (14 to 15:1, never pure white) | Primary text is #ECECF1 on #1C1C1F, 14.4:1, asserted by a test; light mode is #1D1D1F (16:1). | Fixed |
| 13 | Colour | Links and the accent as text measured 4.0:1 (system blue on white) and white on a system-blue button 4.0:1. | WCAG 1.4.3 | The button fill and the text accent are adjusted per surface until they pass, and the label is dark or white, whichever changes the accent least. Tested against ten accent colours including yellow, graphite and white. | Fixed |
| 14 | Colour | Success, warning and error text fell under 4.5:1 on their own tinted grounds. | WCAG 1.4.3 | Adjusted at construction and tested on every surface and tint. | Fixed |
| 15 | Colour | Category glyphs on their tiles were 2:1 (orange) and decorative-only colour carried the meaning. | WCAG 1.4.11 (3:1) | Tile glyphs move toward the text colour until they reach 3:1; the name is always beside the icon. | Fixed |
| 16 | Colour | The bar's contrast depended on the desktop behind its translucent base. | HIG materials, WCAG 1.4.3 | A denser base (90% light, 88% dark); text passes over a black, a mid grey and a white desktop (tested). | Fixed |
| 17 | Colour | The selected row was a flat mid-grey slab, heavy in light mode. | HIG lists (selection follows the accent) | The accent at 10% (light) and 16% (dark) opacity with crisp primary text; 24% in high contrast. | Fixed |
| 18 | Colour | A dropped PDF showed a purple swatch while PDF rows were red. | HIG consistency | One file-type to colour mapping used by the chip, the file rows and the result card. | Fixed |
| 19 | Colour | The footer brand mark had two accent squares. | Brand (one accent, three ink) | One accent square, three ink squares, the same mark as the icon. | Fixed |
| 20 | Icons | Tiles were drawn in three places with different glyph sizes and corner ratios. | HIG icons, Fluent 2 iconography | One `category_tile` function: glyph half the tile, corner a quarter, one stroke weight (Lucide, 1.75). | Fixed |
| 21 | Controls | The clear-file button was a 16 pt target and the footer key hints were plain text. | HIG (28 pt minimum), Fluent 2 (32 pt) | 28 pt targets with hover and focus states; every control is at least 28 pt tall, rows 44 pt or more. | Fixed |
| 22 | Controls | Every button and row showed the pointing hand. | HIG pointing-hand cursor (links only) | Only links use it. | Fixed |
| 23 | Controls | Buttons had a hover fade only: no pressed state, and a disabled Run was the enabled button at 45% opacity. | HIG, Fluent 2 control states | Rest, hover, pressed, focus and disabled for every button kind; a disabled button is flat, takes no hover and says so to assistive technology. | Fixed |
| 24 | Controls | A segmented control made the selected label bolder, so the layout moved when you chose. | HIG segmented controls | Constant weight; cells are equal width in up to four columns. | Fixed |
| 25 | Controls | The switch was 34 by 20 pt and only the track was clickable. | HIG switches (38 by 22) | 38 by 22 pt, and the whole row is the target. | Fixed |
| 26 | Controls | Status messages (estimate, blocker, network) were loud coloured slabs, and a three-line paragraph filled the header. | HIG restraint | A calm notice style (coloured icon, primary text, faint ground), a quiet note for facts, a short header status with the reason in a tooltip. | Fixed |
| 27 | Keyboard | Buttons, rows, sidebar items and quick actions could not be reached with Tab: they were marked as tab stops but were not focusable, so only the text fields took focus. | HIG keyboard navigation, Fluent 2 focus, WCAG 2.1.1 | All are focusable; the headless run Tabs through Home and records what takes focus (16 distinct controls, in reading order) and fails if fewer than 12. | Fixed |
| 28 | Keyboard | The focus ring was a 2 px border that moved the layout, drawn at 50% accent, and a shadow ring would have shown through translucent fills. | WCAG 2.4.7 and 1.4.11, Fluent 2 (2 px ring) | A 2 pt accent border that is transparent at rest, so nothing moves; at least 3:1 against every surface (tested). Text fields get an outer ring pulled back by a negative margin. | Fixed |
| 29 | Keyboard | Tab in a one-line text field inserted a tab character. | HIG keyboard navigation | Tab and Shift+Tab move between controls in the search and option fields (in the bar, Tab still opens the options). | Fixed |
| 30 | Keyboard | The app window opened with nothing focused, so Tab, Escape and shortcuts did nothing until a click. | HIG keyboard, WCAG 2.1.1 | It opens with keyboard focus on the page. | Fixed |
| 31 | Keyboard | Command+K, Command+Return, Command+R and Command+Z tested the platform modifier, which is the Windows key on Windows and Linux. | Fluent 2 and GNOME shortcut conventions | They use the secondary modifier: Command on a Mac, Control elsewhere. | Fixed |
| 32 | Keyboard | Undo and Redo did nothing in text fields on Windows and Linux (the kit binds them on macOS only). | Fluent 2, GNOME HIG | Ctrl+Z, Ctrl+Shift+Z and Ctrl+Y are bound; Undo on a result card is an action, so the Edit menu item works. | Fixed |
| 33 | Keyboard | Running a job again from History needed a double-click. | WCAG 2.1.1 | A "Run again" button on every row; double-click still works. | Fixed |
| 34 | Keyboard | No Settings (Command+comma), no Open, no Minimize, no Hide, no full screen. | HIG keyboard shortcuts, macOS menus | Command+, Settings (a real page), Command+O, Command+F, Command+1 to 4, Command+M, Command+H, Option+Command+H, Control+Command+F, Command+W, Command+Q; F11 and F1 off macOS. Cut, Copy, Paste and Select All come from the text field and were checked. | Fixed |
| 35 | Keyboard | Icon-only buttons had no tooltip. | HIG help tags, Fluent 2 tooltips | Tooltips with the shortcut where there is one (Settings, Undo, Then, Back, Remove, steppers). Buttons with text rely on their label and the footer hint. | Fixed |
| 36 | Menus | The macOS menu bar had one item, Quit. | HIG menus | App (About, Settings, Services, Hide, Hide Others, Show All, Quit), File, Edit (Undo, Redo, Cut, Copy, Paste, Select All), View, Window, Help. | Fixed |
| 37 | Menus | Windows and Linux have no menu bar in this GPUI app. | Fluent 2 menu bars, GNOME primary menu | Every command has a shortcut and the full list is on the Settings page. There is no in-window menu or hamburger button yet. | Open |
| 38 | Window | On Windows the window had no title bar, no caption buttons and no way to drag it (the client area covered the caption). | Fluent 2 title bar (32 px, 46 px caption buttons, Mica surface) | A drawn 32 pt title bar on a solid Mica-like surface with the Fluent caption buttons (minimise, maximise or restore, close in red on hover). Compiled by CI; not run on a Windows desktop. | Fixed |
| 39 | Window | On GNOME under Wayland (no server-side decorations) the window had no frame. | GNOME HIG header bar | A 46 pt header bar with window buttons and 6 pt resize edges when the compositor asks for client decorations; the system frame is used elsewhere. Compiled by CI; not run on a Linux desktop. | Fixed |
| 40 | Window | Size and position were not remembered. | HIG windows | Saved to the data folder, restored, refitted when a display is gone (six unit tests). | Fixed |
| 41 | Window | Traffic lights at (18, 18) did not sit in the centre of a 52 pt toolbar zone. | HIG window controls | Placed at (20, 20): 20 pt in, centred on the 52 pt zone that the sidebar and the page both reserve. Derived from the layout, not measured, because the native buttons are not in an offscreen render. | Fixed |
| 42 | Window | The minimum window size is 860 by 560. | HIG windows | Kept; the sidebar scrolls so the smallest window still reaches Settings. | Fixed |
| 43 | Motion and access | Reduce Motion was read on macOS and GNOME only. | HIG accessibility, Fluent 2, WCAG 2.3.3 | Windows is asked for the client-area animation setting; the GNOME check moved off the UI thread (it ran a process on every frame). | Fixed |
| 44 | Motion and access | Increase Contrast and High Contrast were ignored. | HIG accessibility, Windows contrast themes, GNOME high contrast | Read on macOS, Windows and GNOME: secondary text becomes primary, hairlines go from 8% to 32%, selection and washes strengthen. Rendered and checked on macOS (`app-home-contrast`, `bar-rest-contrast`). | Fixed |
| 45 | Motion and access | Reduce Transparency is not read; the bar keeps its translucent base. | HIG accessibility | The base is already dense enough to pass contrast, but the blur is not switched off. | Open |
| 46 | Motion and access | List rows on Modules had no role, label or position. | WCAG 4.1.2 | Role, label, position and set size; singular and plural fixed ("1 tool"). | Fixed |
| 47 | Motion and access | Interface text does not follow a system text-size setting. | HIG Dynamic Type (iOS), Windows text scaling | GPUI draws fixed point sizes. Not addressed. | Open |
| 48 | Motion and access | Scroll indicators are not draggable and do not auto-hide. | HIG scrolling | Wheel and trackpad scrolling is native (momentum comes from the system); dragging a thumb needs a custom scrollbar. | Open |
| 49 | Motion and access | Right-to-left layout is not mirrored. | HIG, Fluent 2 bidirectional layouts | Not addressed. | Open |
| 50 | States | Loading, empty, progress, error and done were designed only for some screens. | HIG, owner brief | Designed for the bar, Home, Results, History, Modules, Find and Nearby, the tool panel (options, no file, running, consent, failed, done) and the file-history load; all are in `review/after`. | Fixed |
| 51 | States | Progress showed a bare bar and a percentage that moved as the label changed length. | HIG progress indicators | Title, bar, then the label on the left and the percentage on the right of one row. | Fixed |
| 52 | Icon | The app icon was a flat gradient on a plain rounded square with one drawing for 16 to 1024 px. | macOS 26 icon grid, Fluent 2 app icons, GNOME icon template | Redrawn: continuous-corner squircle, quiet plate, squares lifted on contact shadows, hand-tuned 16 to 64 px drawings, a Fluent tile for Windows, a GNOME tile and SVG for Linux; `docs/design/icon-sheet.png`. | Fixed |
| 53 | Icon | The Windows icon is not embedded in the .exe, so Explorer and the taskbar show the generic icon without a shortcut. | Fluent 2 app icons | Needs a resource compiler (a build script and a new dependency). | Open |
| 54 | Icon | macOS 26 also offers dark and tinted icon variants (an .icon bundle from Icon Composer). | macOS 26 icon design | A dark variant is drawn (`panoply-macos-dark.svg`, `icon-dark-variant.png`) but not packaged as an .icon bundle. | Open |
| 55 | Safety | On Windows, Undo deletes the new file for good because the recycle bin is not wired. | HIG and Fluent 2 (reversible actions) | Not a design change; noted in `packaging/README.md`. | Open |

## Open items

- **37. Windows and Linux have no menu bar in this GPUI app.** Every command has a shortcut and the full list is on the Settings page. There is no in-window menu or hamburger button yet.
- **45. Reduce Transparency is not read; the bar keeps its translucent base.** The base is already dense enough to pass contrast, but the blur is not switched off.
- **47. Interface text does not follow a system text-size setting.** GPUI draws fixed point sizes. Not addressed.
- **48. Scroll indicators are not draggable and do not auto-hide.** Wheel and trackpad scrolling is native (momentum comes from the system); dragging a thumb needs a custom scrollbar.
- **49. Right-to-left layout is not mirrored.** Not addressed.
- **53. The Windows icon is not embedded in the .exe, so Explorer and the taskbar show the generic icon without a shortcut.** Needs a resource compiler (a build script and a new dependency).
- **54. macOS 26 also offers dark and tinted icon variants (an .icon bundle from Icon Composer).** A dark variant is drawn (`panoply-macos-dark.svg`, `icon-dark-variant.png`) but not packaged as an .icon bundle.
- **55. On Windows, Undo deletes the new file for good because the recycle bin is not wired.** Not a design change; noted in `packaging/README.md`.

## Performance budgets

`--selfcheck` before and after, same machine, same non-LTO release build, five runs of each alternated, medians. The machine was not idle, so the spread between runs is about 20%.

| Measure | Before | After | Budget |
|---|---|---|---|
| UI frame, median | 0.78 ms | 0.96 ms | under 4 ms |
| UI frame, 95th percentile | 3.53 ms | 3.47 ms | under 4 ms |
| UI frame, worst | 7.54 ms | 6.76 ms | under 4 ms |
| Frames over 4 ms in the run | 8 | 8 | 0 |
| Keystroke to frame, 95th percentile | 4.24 ms | 3.80 ms | under 4 ms |
| State change, worst | 1.11 ms | 0.92 ms | under 4 ms |
| Panel open, worst of 31 tools | 5.12 ms | 5.01 ms | under 50 ms |
| First frame of the app window | 0.40 ms | 0.72 ms | none |
| Idle memory, fresh process (`--selfcheck-idle`) | 66.7 MB | 67.3 MB | under 100 MB |
| Idle memory after every state was visited | 150.5 MB | 150.3 MB | under 100 MB |
| Idle CPU | 0.04% | 0.04% | 0% |

The 4 ms rule is not met before or after in this build: the same frames go over (a page opened for the first time, a tool panel opened, the first keystroke) and the worst case came down from 7.5 to 6.8 ms. The new design draws a little more per frame (a median of 0.2 ms), which windowing the long lists pays for. The memory number a fresh process holds is 67 MB; the 150 MB is the run after all 31 panels, the review states and the text caches have been visited, and it is the same before and after.

Steady-state frame cost per screen, from `review/after/frame-times.tsv` (median of five draws): Settings 3.2 ms, Modules 2.9, Home 1.7 (2.0 dark), Category 1.3, every bar state under 1.1.


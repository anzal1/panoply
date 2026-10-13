# UI shell kill test: the command bar

Goal: build the SAME command bar three times (SwiftUI reference, Tauri 2,
Flutter) on macOS 26, then measure speed and judge feel side by side. The bar
is "premium, Apple grade". The test asks which cross-platform shell gets
closest to native, so every build must be crafted to the same standard. Do not
cut corners in one build.

Data: `shared/utilities.json` (75 tools: id, name, subtitle, category,
sfSymbol, lucide, accepts[]). Load it at startup; do not hardcode.

## Window

- Borderless floating panel, 720 x 460 pt, corner radius 20, centered
  horizontally, top edge at 22% of the screen height. Floats above other
  windows, joins the active Space, does not show in the Dock (accessory app).
- Material: real macOS translucency behind the content. Prefer macOS 26 Liquid
  Glass if the framework can reach it (SwiftUI `glassEffect`, or
  `NSGlassEffectView`), otherwise `NSVisualEffectView` with material `.hudWindow`
  or `.popover`, state active, blending behindWindow. Never a fake CSS blur over
  a screenshot.
- 0.5 pt inner hairline: white 12% in dark, black 8% in light. System window
  shadow.
- Follows system light/dark live.
- Hides on Escape (when query is empty), on losing focus, and on the hotkey.

## Layout (points)

- **Search row**, 56 tall. Magnifying glass icon 18 at x 20, vertically
  centered. Text field starts at x 50, system font 20 regular, placeholder
  "Search 75 tools or drop a file" in tertiary color. Caret always visible on
  show. When a file is dropped, a chip sits at the trailing edge (x end - 16):
  12 pt medium, file name truncated in the middle, max 200 wide, radius 8,
  fill primary 8%, with a small x to clear it.
- Hairline separator under the search row, 0.5 pt, primary 8%.
- **Results list**, scrolls, 8 pt top padding.
  - Section header per category: 28 tall, text at x 20, 11 pt medium,
    tertiary color, sentence case ("PDF", "Image").
  - Row: 44 tall. Selection pill inset 8 left and right, radius 10.
    Icon tile at x 20: 28 x 28, radius 7, fill = category tint at 16% opacity,
    glyph 15 in the tint color. Name at x 60, 13.5 pt medium, primary. Subtitle
    follows the name after an 8 pt gap, 12 pt regular, secondary, single line,
    truncates. Trailing at x end - 20: category name 11 pt tertiary; on the
    selected row this becomes a return-key cap "↵".
  - Fuzzy match highlight: matched characters in the name render semibold.
- **Footer**, 40 tall, hairline on top. Leading at x 20: 16 pt app glyph
  (a simple rounded square) and "Arsenal" 12 pt secondary. Trailing: two
  actions "Open ↵" and "Actions ⌘K", each label 12 pt secondary plus key caps
  (11 pt, radius 5, fill primary 8%, min 18 x 18).

Colors: primary text = system label (not pure white in dark), secondary = 60%,
tertiary = 40%. Category tints (system colors): PDF red, Image purple, Video
orange, Audio pink, Files blue, Text green, Capture teal. Font: system (SF Pro),
tabular numbers.

Icons: SwiftUI uses `sfSymbol`. Tauri and Flutter use Lucide icons by the
`lucide` name (bundle the SVGs locally, no CDN).

## Behaviour

- Typing filters instantly with fuzzy scoring: subsequence match, bonus for
  prefix, word-boundary starts and consecutive runs. Results keep category
  sections when the query is empty; when typing, show one flat list sorted by
  score under a "Best matches" header.
- Up/Down move selection (clamped, scroll follows with a little margin).
  Return "runs" the tool: the row briefly flashes and the footer leading text
  reads "Opened Compress PDF" for 1.2 s. Escape clears the query, then hides.
- Drag a file over the window: a soft accent-colored inner ring (2 pt, 40%
  opacity) fades in with the label "Drop to see tools for .pdf" centered in the
  list. On drop, show the chip and filter to tools whose accepts includes the
  extension or "*".
- Empty state: "No tools match “xyz”" centered, 13 pt secondary, and below it
  "Try “compress” or drop a file" 12 pt tertiary.

## Motion (calm and precise, no bounce, no overshoot)

- Show: opacity 0 to 1, scale 0.985 to 1, y offset -6 to 0, 180 ms,
  cubic-bezier(0.2, 0.9, 0.25, 1). Hide: 120 ms ease-in, opacity and scale to
  0.99.
- Selection pill glides between rows: critically damped spring (no overshoot),
  about 220 ms settle. It must never snap or jump.
- List content updates while typing are instant (no stagger). Rows that appear
  or disappear crossfade over 90 ms at most.
- Respect Reduce Motion: replace movement with short fades.
- Must hold 120 fps on ProMotion displays during the glide and scrolling.

## Global hotkey and control port (for measurement)

| Build | Hotkey | Control port |
|---|---|---|
| SwiftUI | Control+Option+1 | 47811 |
| Tauri | Control+Option+2 | 47812 |
| Flutter | Control+Option+3 | 47813 |

Each app runs a tiny HTTP server on 127.0.0.1 at its port:

- `GET /toggle` calls exactly the same show/hide function as the hotkey and
  records `t0` (wall clock ms with sub-ms precision) at the moment the request
  arrives, before doing anything. When the bar's first frame after showing is
  presented, record `t1` and keep `show_ms = t1 - t0`. Use the earliest
  callback that runs after that frame is on screen: two nested
  requestAnimationFrame calls in a webview, `addPostFrameCallback` plus
  `endOfFrame` in Flutter, a CATransaction completion block after
  `orderFront` and display in AppKit. Respond immediately; do not wait.
- `GET /metrics` returns JSON `{"show_ms": [...all recorded...]}`.
- `GET /selftest` makes the bar visible, then programmatically drives the
  query through this exact sequence, one update at a time: type
  "compress pdf" one char at a time, backspace to empty, type "heic to jpg",
  backspace to empty, type "trim video", backspace to empty. For each update
  measure from the state change to the next presented frame (same callback
  rules as above). Respond with JSON
  `{"n":..,"p50":..,"p95":..,"max":..}` in ms.
- On launch, when the first frame is presented, append the wall clock ms to
  `results/<build>-launch.txt` (build = swiftui | tauri | flutter), relative to
  this SPEC's folder, as a single number per line.

## Build hygiene

- Release builds only for measurement.
- This is a shared laptop: build with low priority (`nice -n 10`, cargo
  `-j4`), no long full-core loops.
- Do not change any system settings. Do not register login items.
- Put everything for your build in its own folder: `swiftui/`, `tauri/`,
  `flutter/`. Write a short `README.md` there with exact build and run
  commands, and the path of the built .app.

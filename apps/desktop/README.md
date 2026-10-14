# Panoply desktop

One GPUI app for macOS, Windows and Linux, built directly on the Rust core (no FFI). Binary:
`panoply-desktop`. Phase 2 of [docs/PLAN.md](../../docs/PLAN.md), grown from the measured shell
prototype in `prototypes/shell-test/gpui/kit-variant`.

## What is in it

- **Command bar** (720 by 460, translucent, created hidden at start, shown by the global hotkey).
  Searches the real module registry (built-in modules and community WASM modules), takes dropped files,
  understands plain-words requests ("make this under 50 kb") and "x to y" conversions through the
  conversion router.
- **Tool panel**, drawn only from the module manifest: number (with stepper), choice (segmented
  control), toggle, size, text and file controls; an estimate where the options give one; run on a worker
  thread with progress and instant cancel; a result card with before and after numbers, Show in folder,
  Open, Then..., Undo (new files go to the Trash). Pattern A tools run the moment Return is pressed.
- **App window**: sidebar of categories with counts from the registry, search, drop area, quick actions,
  recent jobs from the history, category pages, a History page, a Modules page, and the same tool panel.
- Keyboard first, Reduce Motion respected, a role and a label on every control, GPUI Kit's text input
  with our grapheme patch (`patches/`), light and dark.

## Layout

| Path | What |
|---|---|
| `src/catalog.rs`, `profile.rs`, `search.rs`, `intent.rs`, `form.rs`, `estimate.rs`, `jobs.rs`, `history.rs` | The logic. No UI types, unit tested. |
| `src/ui/` | The GPUI views: `bar`, `app`, `panel`, `widgets` (the component set), `theme` (colour, type ramp, spacing, contrast tests), `chrome` (title bar and window frame per OS), `live` (start-up, hotkey, menu bar, shortcuts). |
| `src/window_state.rs` | The remembered window size and position, refitted to the displays that exist. Unit tested. |
| `src/platform/` | Everything that differs per OS. macOS has the full set; Windows and Linux compile the same surface. |
| `src/selfcheck.rs` | Dev only (`--features snapshot`): the headless run. |
| `patches/gpui-base/` | GPUI Kit's `gpui-base` 0.7.3 with `patches/gpui-base-grapheme-and-unmark.diff` applied (Apache-2.0). Kept out of the cargo workspace by `exclude` in the root `Cargo.toml`. |
| `tools/` | `selfcheck.sh` (headless run), `linux-check.sh` (Docker), `gen_icons.mjs` (Lucide icons, ISC), `gen_app_icon.mjs` (the app icon: SVG to .icns, .ico and PNGs). |
| `packaging/` | macOS bundle script, Linux `.desktop`, the app icon in every format, notes for Windows and Linux. |
| `screens/` | PNGs rendered offscreen by the headless run, plus `selfcheck-report.json`. |

## Run the checks

```bash
cargo test -p panoply-desktop                     # logic tests
cargo test -p panoply-desktop --features snapshot # plus the headless run as a test (about 20 s)
apps/desktop/tools/selfcheck.sh                   # release build, PNGs in screens/, budget report
apps/desktop/tools/linux-check.sh check           # Linux compile check in Docker
panoply-desktop --selfcheck-review --out DIR      # every screen and state, light and dark, 2x, plus frame costs (with --features snapshot)
```

The headless run opens the bar and the app window at x = -20000, never ordered front and never key,
draws frames with `Window::draw` and renders PNGs with `render_to_image`. Nothing is shown on screen.
`panoply-desktop --selfcheck-idle` starts a fresh process with both windows and holds idle, for
`footprint` and `ps`.

## Behaviour notes

- Hotkey: Option+Space on macOS, Ctrl+Alt+Space elsewhere. `PANOPLY_HOTKEY="ctrl+alt+p"` overrides it.
- Data: history in `~/Library/Application Support/Panoply/history.jsonl` (`$XDG_DATA_HOME/panoply`,
  `%APPDATA%\Panoply`), user modules in its `modules/` folder, `PANOPLY_DATA_DIR` moves it all,
  `PANOPLY_MODULES` adds module folders. `PANOPLY_APPEARANCE=light|dark` forces a look,
  `PANOPLY_REDUCE_MOTION=1` and `PANOPLY_HIGH_CONTRAST=1` force those settings.
- The window remembers its size and position (`window-app.json` in the data folder). Command+comma (Ctrl+comma
  off macOS) opens Settings, which also lists every shortcut. The design rules, the audit and the measured
  numbers are in [docs/design/UI-AUDIT.md](../../docs/design/UI-AUDIT.md).
- The estimate comes from the manifest's size-target option. `Module` has no estimate hook yet; when
  panoply-host grows one, `estimate::for_job` should prefer it.
- Options order: the manifest keeps options in a sorted map, so the panel orders them by what the names
  say (`form.rs`, `priority`). A `module.toml` that kept its order would make that exact.

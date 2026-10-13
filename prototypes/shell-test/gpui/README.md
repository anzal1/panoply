# Arsenal command bar: GPUI build

GPUI (Zed's UI framework) build of the shell kill test. Plain GPUI, no component kit. Spec: `../SPEC.md`.
Standalone Cargo project (empty `[workspace]` table), not part of the Panoply workspace.

## Pins

- GPUI: `github.com/zed-industries/zed` at `a63db9fd901bedf1e0b52acfc1f60f30e5495449` (main, 2026-10-13), crates `gpui` and `gpui_platform` (features `font-kit`, `runtime_shaders`).
- `async-task` and `calloop` patched to Zed's forks, as Zed's own workspace does.
- Hotkey: `global-hotkey` 0.7 (MIT or Apache-2.0). Control server: `tiny_http`.
- Rust 1.98.1. No Xcode on this machine, so there is no `metal` compiler. The `runtime_shaders` feature compiles GPUI's
  Metal shaders at launch instead of at build time, which costs cold start (see Notes).

## Build

```sh
cd /Users/anzalhussainabidi/personal/panoply/prototypes/shell-test/gpui
./build.sh        # nice -n 15 cargo build --release -j3, wraps and ad-hoc signs Arsenal.app
```

Output: `/Users/anzalhussainabidi/personal/panoply/prototypes/shell-test/gpui/Arsenal.app` (LSUIElement accessory app, 6.1 MB;
`shared/utilities.json` is copied into `Contents/Resources` and loaded at startup).
Clean build: about 70 s wall at `-j3` after the git dependencies are fetched.

`gen_icons.py` regenerates `src/icons_gen.rs` (71 Lucide SVGs, taken from the markup the Tauri build already extracted).

## Run

```sh
open -g /Users/anzalhussainabidi/personal/panoply/prototypes/shell-test/gpui/Arsenal.app
pkill -f Arsenal.app/Contents/MacOS     # quit
```

Hotkey: Control+Option+4 (Carbon `RegisterEventHotKey`, no permissions). Control server on 127.0.0.1:47814:

```sh
curl 127.0.0.1:47814/toggle     # same code path as the hotkey
curl 127.0.0.1:47814/metrics    # {"show_ms":[...]}
curl 127.0.0.1:47814/selftest   # {"n":66,"p50":..,"p95":..,"max":..}
curl 127.0.0.1:47814/state      # extra: phase, key window, level, frame, blur view count
```

Measure: `python3 prototypes/shell-test/bench.py gpui prototypes/shell-test/gpui/Arsenal.app` from the repo root.
Launch time is appended to `../results/gpui-launch.txt`.

## Offscreen preview (dev only)

`cargo build --release --features snapshot --target-dir target-snap` enables `/snapshot?q=comp+pdf&sel=1&file=report.pdf&path=out.png`
(`file=drag` shows the drag ring). It renders the live frame through GPUI's offscreen Metal path. The blur is not in the image, only the
translucent tint. `ARSENAL_APPEARANCE=light|dark` forces a theme for these checks. Never used for measurement.

## How timing is measured

- `t0`: wall clock ms taken as soon as `tiny_http` returns the request (or at the top of the Carbon hotkey handler).
- `show_ms`: `t1 - t0`. GPUI has no post-present hook, so `t1` is an `on_next_frame` callback registered from inside `render()`.
  GPUI runs those callbacks at the start of the next frame request, after the frame containing the change has been drawn and
  presented. That is the earliest callback that runs after the frame is drawn, but it is inherently one vsync (8.3 ms at 120 Hz)
  later than a display link tick that can fire in the same frame.
- `/selftest`: for each of the 66 query updates, `start` is taken right before `set_query`, `end` is the same `on_next_frame`
  probe, with 12 to 30 ms of random idle between steps.
- Launch: the panel is ordered front fully transparent for one frame, the first drawn frame's probe appends the wall clock to the
  results file, then the panel is ordered out.
- `inactive_frame_interval: None`: without it GPUI caps an unfocused window to about 30 fps, which inflated typing latency to 40 ms
  when the app is driven through the control port without being the key window.

## Window and material

GPUI opens a `WindowKind::PopUp` panel (`NSPanel`, non-activating) with `WindowBackgroundAppearance::Blurred`. After creation `native.rs`
sets: borderless style mask, floating level, move-to-active-space, system shadow, and a 20 pt rounded mask on the content layer so the
blur is clipped to the same radius as the drawn card. Show and hide animate `NSWindow.alphaValue` plus a scale and offset transform on
the content layer (same curves as SPEC.md). The hairline, card tint, drag ring and everything else is drawn by GPUI.

`Blurred` is a plain `NSVisualEffectView` (material UnderWindowBackground) with GPUI stripping the tint and saturation. It is a real
backdrop blur, not Liquid Glass: there is no refraction or specular edge, and the card needs its own tint (62% to 66% opaque) for contrast.

## Deviations from the spec

- Material: real blur only, no Liquid Glass (GPUI does not expose `NSGlassEffectView`; it could be injected through raw AppKit calls, not done).
- Text field is hand drawn (insertion, backspace, option and command backspace, arrows, home, end, paste), not an `NSTextField`. No IME,
  dead keys, selection, undo or caret blink.
- File chip truncates with GPUI's middle ellipsis. Row appear and disappear crossfades (90 ms) and the return-cap crossfade are instant.
- Corner mask uses circular corners to match GPUI's drawn card; SwiftUI uses continuous corners.
- Hide on click outside relies on key window loss, there is no global mouse monitor.
- Drag and drop onto the panel, the hotkey press and live keyboard input could not be exercised here (no accessibility or screen
  recording permission); the code paths follow GPUI's `ExternalPaths` and key event APIs. Layout was checked with offscreen renders only.
- Collection behavior is move-to-active-space; level is `NSFloatingWindowLevel` like the SwiftUI build.

## Notes

- Show latency is dominated by GPUI stopping its `CVDisplayLink` when the window is ordered out: about 25 ms pass between
  `makeKeyAndOrderFront` and the first frame request. `ARSENAL_KEEP_WARM=1` hides by going transparent instead of ordering out, which keeps the link
  running and brought show to about 12 to 16 ms in a 10 cycle spot check (not the default, costs idle wakeups).
- Cold start includes compiling GPUI's Metal shaders from source at launch (`runtime_shaders`). A machine with the Metal toolchain would
  use the precompiled library and start faster.

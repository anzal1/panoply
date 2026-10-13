# Arsenal command bar, SwiftUI reference build

Native macOS 26 build: SwiftUI content hosted in an `NSPanel`, backed by Liquid Glass
(`NSGlassEffectView`, regular style, corner radius 20). Accessory app (`LSUIElement`), no Dock icon.

## Build

```sh
cd /Users/anzalhussainabidi/personal/arsenal/shell-test/swiftui
./build.sh        # nice -n 10 swift build -c release, wraps and ad-hoc signs Arsenal.app
```

Output: `/Users/anzalhussainabidi/personal/arsenal/shell-test/swiftui/Arsenal.app`
(`shared/utilities.json` is copied into `Contents/Resources` and loaded at startup).

## Run

```sh
open /Users/anzalhussainabidi/personal/arsenal/shell-test/swiftui/Arsenal.app
```

Hotkey: Control+Option+1 (Carbon `RegisterEventHotKey`, no permissions).
Control server on 127.0.0.1:47811:

```sh
curl 127.0.0.1:47811/toggle     # same code path as the hotkey
curl 127.0.0.1:47811/metrics    # {"show_ms":[...]}
curl 127.0.0.1:47811/selftest   # {"n":66,"p50":..,"p95":..,"max":..}
```

Launch time is appended to `../results/swiftui-launch.txt`. Quit with `pkill -x Arsenal`.

## Offscreen preview (ImageRenderer, 3x)

```sh
.build/release/Arsenal --render previews
```

Writes PNGs of the default, query, empty, chip, drag and run states in light and dark.
Glass and the real `NSTextField` do not render offscreen; a flat stand-in and `Text` replace them.

## How timing is measured

- `t0`: wall clock ms (`Date().timeIntervalSince1970 * 1000`) at the top of the network receive callback
  (or the Carbon hotkey handler), before anything else runs.
- `show_ms`: `t1 - t0`, where `t1` is the first `CADisplayLink` (`NSView.displayLink`) callback after
  `makeKeyAndOrderFront`, `displayIfNeeded` and `CATransaction.flush()` have committed the show.
  A CATransaction completion block was avoided because it would wait on the animation or fire
  before the frame; the display link ticks at vsync, so the value sits within one vsync of scanout.
- `/selftest`: for each of the 66 query updates, `start` is taken right before `Store.setQuery`,
  `end` is the first display link tick at which the SwiftUI body has re-evaluated for that update
  (a version counter guards against a tick that lands before the update is rendered). 12 to 30 ms of
  random idle is inserted between steps so the start lands at varied vsync phases.
- Launch: the panel is ordered in fully transparent for one frame (this also warms the window server
  surface, glass and view tree), and the first display link tick appends the wall clock to the results file.

# Panoply command bar prototype: Tauri 2 build

Tauri 2.12 shell, plain TypeScript + vanilla DOM frontend built with Vite. Spec: `../SPEC.md`.

## Build

```sh
cd prototypes/shell-test/tauri
source ~/.nvm/nvm.sh            # Node 24
npm install
CARGO_BUILD_JOBS=4 nice -n 10 npx tauri build --bundles app
```

`npm run build` (run by `tauri build`) regenerates the inline Lucide icons from
`../shared/utilities.json`, type-checks, then bundles the frontend. Release only, `.app` bundle only.

Built app:

```
prototypes/shell-test/tauri/src-tauri/target/release/bundle/macos/Arsenal.app
```

## Run

```sh
open -n src-tauri/target/release/bundle/macos/Arsenal.app
```

Hotkey Control+Option+2. Control server on 127.0.0.1:47812:

```sh
curl localhost:47812/toggle     # same code path as the hotkey
curl localhost:47812/metrics    # {"show_ms":[...]}
curl localhost:47812/selftest   # {"n":66,"p50":..,"p95":..,"max":..}
curl localhost:47812/state      # extra: visible, key window, frame, material in use
```

The launch timestamp is appended to `../results/tauri-launch.txt` on the first presented frame.
Stop it with `pkill -f Arsenal.app/Contents/MacOS/arsenal-tauri`.

Debug switches: `ARSENAL_DEBUG=1` traces show/hide on stderr, `ARSENAL_MATERIAL=vibrancy` forces the
vibrancy fallback, `ARSENAL_RESULTS_DIR` overrides where the launch file is written.

## Design preview in a browser

```sh
npm run dev      # http://127.0.0.1:1420, no native glass, same DOM and CSS
```

## How it works

- **Material.** `NSGlassEffectView` (macOS 26 Liquid Glass) with `cornerRadius = 20`, inserted behind the
  transparent `WKWebView` through objc2. Falls back to `window-vibrancy` `HudWindow` (radius 20) if the
  class is missing. The hairline, rows and text are HTML on top.
- **Always warm.** The window is created hidden in config, then parked at launch: ordered in, glass hidden,
  page transparent, click-through. A show is a native fade of the glass plus the page animation; hide
  reverses it and returns activation to the previously frontmost app. Nothing is created on the hotkey.
- **WebKit tuning (private API).** `_setWindowOcclusionDetectionEnabled:NO` so a transparent parked window
  keeps presenting frames (otherwise the first show after idle costs ~240 ms), and the experimental
  feature `PreferPageRenderingUpdatesNear60FPSEnabled` is turned off so `requestAnimationFrame` runs at
  the 120 Hz display rate.
- **Measurement.** `/toggle` stamps `t0` (SystemTime ms) on arrival in the HTTP thread, shows via the main
  thread and answers at once. The page reports `t1 = performance.timeOrigin + performance.now()` from a
  double `requestAnimationFrame` after the show. `/selftest` runs in JS and the HTTP thread waits on a
  channel for the JSON.
- **Pill.** Critically damped spring (closed form, no overshoot, velocity kept on retarget), omega 26.5 rad/s,
  about 220 ms to 2%. Transform only. The scroll follow uses the same spring.

# Arsenal command bar, Flutter build

Flutter 3.47.7 (Impeller, Metal), macOS 26. Zero plugins: the window, material,
hotkey and file drops are a small Swift Runner (`macos/Runner`), the only Dart
dependency is `lucide_icons_flutter` (the Lucide font, bundled and tree-shaken).

## Build

    ./build.sh          # release .app, nice -n 10
    ./snap.sh           # offscreen 3x PNG renders (light + dark) into ./snapshots

`flutter build macos --release` needs `xcodebuild`, i.e. a full Xcode. This Mac only
has the Command Line Tools, so `build.sh` performs the same steps itself:
`flutter assemble` (AOT arm64 release App.framework and assets), `swiftc` for the
Runner against FlutterMacOS.framework, bundle, ad-hoc codesign. With Xcode installed,
`flutter build macos --release` builds the same sources.

App: `build/macos/Build/Products/Release/arsenal_bar.app`

## Run and measure

    open -g build/macos/Build/Products/Release/arsenal_bar.app
    curl localhost:47813/toggle      # same function as Control+Option+3
    curl localhost:47813/selftest
    curl localhost:47813/metrics
    python3 ../bench.py flutter "$PWD/build/macos/Build/Products/Release/arsenal_bar.app"

Launch time is appended to `../results/flutter-launch.txt` (override the folder with
`ARSENAL_RESULTS_DIR`). Quit with `pkill -f arsenal_bar`.

## Implementation notes

- Material: `NSGlassEffectView` (Liquid Glass, style regular, cornerRadius 20) hosting the
  transparent Flutter view. Fallback below macOS 26, or with `ARSENAL_MATERIAL=vibrancy`:
  `NSVisualEffectView` hudWindow, active, behindWindow, rounded mask image.
- Window: borderless non-activating `NSPanel`, floating level, moveToActiveSpace, system
  shadow, LSUIElement. Pre-created at launch (ordered in at alpha 0 so the engine renders and
  warms everything, ordered out after the first presented frame). Top edge at 22% of the
  screen frame height, on the screen under the pointer.
- Show: native window alpha 0 to 1 over 180 ms with cubic-bezier(0.2, 0.9, 0.25, 1), Flutter
  content scale 0.985 to 1 and y -6 to 0 on the same curve. Hide: 120 ms ease-in, alpha and
  scale 0.99. Reduce Motion (AppKit flag or `accessibilityFeatures.disableAnimations`): fades only.
- Selection pill: closed-form critically damped spring (damping ratio 1, omega 34 rad/s),
  velocity preserved when retargeted, painted by a `CustomPainter`.
- Timing: `t0 = DateTime.now().microsecondsSinceEpoch / 1000`. `t1` is the raster finish of the
  first frame whose build started after t0 (`FrameTiming`, `FramePhase.rasterFinishWallTime`,
  a wall clock). The engine batches timing reports (up to 1 s in release), so `/metrics` and
  `/selftest` wait for them. Fallback if a report never arrives: `addPostFrameCallback` +
  `endOfFrame`. In all measured runs every probe resolved via raster timings.
- Hotkey: Carbon `RegisterEventHotKey` (no Accessibility permission needed).
- File drop: `NSDraggingDestination` on the root view, forwarded over a method channel.
- Icon `files-merge` is not in the bundled Lucide font, `merge` is used.

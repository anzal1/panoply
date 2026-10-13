# Apps

One native app per platform on the shared Rust core (ADR 0001).

| Folder | Platform | UI | Phase |
|---|---|---|---|
| `macos/` | macOS (and later iOS) | SwiftUI, Liquid Glass | 2 |
| `android/` | Android | Jetpack Compose | after Mac launch |
| `desktop/` | Windows and Linux | GPUI, Slint or Tauri, picked by rerunning the shell kill test | after Android |

The Mac app starts from the measured SwiftUI command bar in
`prototypes/shell-test/swiftui` (182 ms cold start, 30 MB idle). It talks to
the core through `crates/panoply-ffi` (UniFFI). Apps never contain module
logic; they render module manifests with a native component set.

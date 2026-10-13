# Apps

One desktop app for macOS, Windows and Linux, plus native mobile apps, all on
the shared Rust core (ADR 0001 and its amendment).

| Folder | Platform | UI | Phase |
|---|---|---|---|
| `desktop/` | macOS, Windows, Linux | GPUI, one app (ADR 0001 amendment) | 2 |
| `android/` | Android | Jetpack Compose | after Mac launch |
| `ios/` | iOS | SwiftUI | after Windows |

The desktop app starts from the measured GPUI command bar in
`prototypes/shell-test/gpui/kit-variant` (33 MB idle, 152 to 190 ms cold start)
and calls the Rust core directly. iOS and Android reach the core through
`crates/panoply-ffi` (UniFFI). Apps never contain module logic; they render
module manifests with one component set per platform.

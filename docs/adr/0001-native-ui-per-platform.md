# 0001. Native UI per platform on one Rust core

- Status: accepted
- Date: 2026-10-13

## Context

Panoply must feel Apple grade, open instantly and stay light in memory on six platforms. On 2026-10-13 the same command bar was built in SwiftUI, Tauri 2 and Flutter and measured on macOS 26 (prototypes/shell-test). Idle memory: SwiftUI 30 MB, Tauri about 91 MB with WebKit helpers, Flutter about 297 MB. Cold start: 182, 397 and 328 ms. Tauri also needed private WebKit calls for 120 Hz and instant show, which rules it out of the App Store.

## Decision

All shared logic lives in one Rust core. Each platform gets a native UI: SwiftUI on macOS and iOS, Jetpack Compose on Android, and a renderer for Windows and Linux picked by rerunning the same kill test there (GPUI, Slint or Tauri). The core reaches the UIs through UniFFI.

## Consequences

Three UI codebases instead of one. This stays sustainable because modules never ship UI: each platform renders a small native component set from module manifests (ADR 0007).

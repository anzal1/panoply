# 0001. Native UI per platform on one Rust core

- Status: accepted, amended 2026-10-13 (desktop UI is GPUI, see Amendment)
- Date: 2026-10-13

## Context

Panoply must feel Apple grade, open instantly and stay light in memory on six platforms. On 2026-10-13 the same command bar was built in SwiftUI, Tauri 2 and Flutter and measured on macOS 26 (prototypes/shell-test). Idle memory: SwiftUI 30 MB, Tauri about 91 MB with WebKit helpers, Flutter about 297 MB. Cold start: 182, 397 and 328 ms. Tauri also needed private WebKit calls for 120 Hz and instant show, which rules it out of the App Store.

## Decision

All shared logic lives in one Rust core. Each platform gets a native UI: SwiftUI on macOS and iOS, Jetpack Compose on Android, and a renderer for Windows and Linux picked by rerunning the same kill test there (GPUI, Slint or Tauri). The core reaches the UIs through UniFFI.

## Consequences

Three UI codebases instead of one. This stays sustainable because modules never ship UI: each platform renders a small native component set from module manifests (ADR 0007).

## Amendment (2026-10-13): GPUI for all desktops

The same command bar was built in GPUI (Zed's Rust UI framework) and measured
with the same harness: 33 MB idle footprint, 152 to 190 ms cold start, 11 to
13 ms keystroke to frame, 6.1 MB bundle, against SwiftUI's 30 MB and 182 ms.
In a side-by-side hotkey test the user could not tell the GPUI and SwiftUI bars
apart.

Text input and accessibility were then checked headless
(prototypes/shell-test/gpui-checks/AUTOMATED.md). GPUI core implements the
macOS NSTextInputClient bridge and AccessKit; GPUI Kit's Apache-2.0 input
handles composition, undo and selection. Hindi, Bengali, dead-key and pinyin
composition all commit correctly and conjuncts shape as single clusters. The
stock kit moves the caret and deletes by code point instead of grapheme
cluster; a roughly 40-line patch fixes it and all 11 cases pass. Every
interactive element exposes a role and label to the accessibility tree.

Decision: macOS, Windows and Linux share one GPUI desktop app built directly on
the Rust core (no FFI on desktop). We carry the grapheme patch and upstream it
to GPUI Kit. SwiftUI remains for iOS, Compose for Android; UniFFI is only for
mobile. Small native Swift targets are still needed on macOS for Finder and
share extensions.

Still to verify by a person before launch: the real Hindi input method's
candidate strip placement in the non-activating panel, and a VoiceOver pass.

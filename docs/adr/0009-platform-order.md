# 0009. Release order: all three desktops together, then Android, then iOS

- Status: accepted 2026-10-13, amended 2026-10-14 (desktops launch together), carried out 2026-10-14 (v0.1.0-beta)
- Date: 2026-10-13

## Context

Shipping six native apps at once delays launch by months. The largest utility audiences (Bangladesh and India) are mostly Android and Windows.

## Decision

macOS, Windows and Linux launch together as one release with the CLI. Android follows, then iOS. This is the 2026-10-14 amendment below; the order first proposed on 2026-10-13 was the CLI and MCP server, then the Mac app, then Android, Windows, iOS and Linux.

## Consequences

One GPUI desktop app covers three systems, so the audience that is mostly on Windows and Android is reached from day one. Developers and AI agents get the CLI in the same release.

## Amendment (2026-10-14): all three desktops launch together

Shipped: v0.1.0-beta released macOS, Windows and Linux (x64 and arm64) and the CLI together on 2026-10-14. The MCP transport is not shipped yet.

With one GPUI desktop app (ADR 0001 amendment) there is no extra UI codebase
for Windows and Linux, and they cover a large share of the users, especially
Windows in India and Bangladesh. macOS, Windows and Linux launch together.
Android follows, then iOS. Platform-specific gaps are closed before launch:
file-manager integrations per OS, and codecs that macOS provides but the others
do not (HEIC through Windows' HEIF extension where installed, otherwise an
LGPL decoder shipped as a separate helper process, never linked).

# 0009. Release order: CLI and MCP, then Mac, then Android, Windows, iOS, Linux

- Status: accepted, amended 2026-10-14 (desktops launch together)
- Date: 2026-10-13

## Context

Shipping six native apps at once delays launch by months. The largest utility audiences (Bangladesh and India) are mostly Android and Windows.

## Decision

First public drop is the CLI and MCP server (all desktops). Then the Mac app launches. Then Android, Windows, iOS and Linux in that order.

## Consequences

Developers and AI agents get Panoply early and build the audience. Mac users first, mass-market platforms next.

## Amendment (2026-10-14): all three desktops launch together

With one GPUI desktop app (ADR 0001 amendment) there is no extra UI codebase
for Windows and Linux, and they cover a large share of the users, especially
Windows in India and Bangladesh. macOS, Windows and Linux launch together.
Android follows, then iOS. Platform-specific gaps are closed before launch:
file-manager integrations per OS, and codecs that macOS provides but the others
do not (HEIC through Windows' HEIF extension where installed, otherwise an
LGPL decoder shipped as a separate helper process, never linked).

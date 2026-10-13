# 0006. LocalSend v2 protocol for sharing

- Status: accepted
- Date: 2026-10-13

## Context

Device-to-device sharing is a core feature. Starting a new protocol means starting a new network from zero. AirDrop interop on Linux exists but is narrow (one Wi-Fi chip, slow, unlicensed) and impossible on Windows.

## Decision

Implement the open LocalSend v2 protocol. Devices without Panoply get a QR code that opens a page served on the local network. AirDrop interop is out of scope.

## Consequences

Panoply interoperates with LocalSend's existing apps on every platform from day one. Guest and office networks that isolate clients need the hotspot fallback.

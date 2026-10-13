# Prototypes

Kill-test code that informed decisions. Not shipped, not maintained, kept so
the measurements can be rerun.

| Folder | Question it answered | Result |
|---|---|---|
| `video-killtest/` | Can on-device video conversion be fast? | Hardware path (WebCodecs) 1.42 s vs native 1.07 s for a 20 s 1080p clip; ffmpeg.wasm over 45 s |
| `shell-test/` | Which UI shell is Apple grade and light? | SwiftUI 30 MB idle, 182 ms cold start; Tauri about 91 MB, 397 ms; Flutter about 297 MB, 328 ms. See ADR 0001 |
| `llm-bench/` | Which small language model fits? | Stopped: Panoply ships no language model (ADR 0003). Harness kept for reference |

The apps inside still use the old working name "Arsenal". Build outputs are
not committed; see each folder's README to rebuild.

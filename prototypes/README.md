# Prototypes

Kill-test code from before Panoply was built. Each folder answered one question that decided a design
choice, and its `RESULT.md` (or ADR) records the answer. Nothing here ships in the app, nothing here is
maintained, and none of it is part of the Cargo workspace. It stays so the measurements can be rerun and
the decisions checked.

Start with the result, not the code.

| Folder | Question it answered | Result |
|---|---|---|
| `shell-test/` | Which UI shell is Apple grade and light? | SwiftUI 30 MB idle, 182 ms cold start; Tauri about 91 MB, 397 ms; Flutter about 297 MB, 328 ms. GPUI was tested too and is what the desktop app uses. See [ADR 0001](../docs/adr/0001-native-ui-per-platform.md) and `shell-test/gpui-checks/RESULT.md` |
| `video-killtest/` | Can on-device video conversion be fast? | Hardware path (WebCodecs) 1.42 s vs native 1.07 s for a 20 s 1080p clip; ffmpeg.wasm over 45 s |
| `module-killtest/` | Can a new author add a module in minutes? | Pass on the second author after one README fix: 104 s and 45 lines of module code. See `module-killtest/RESULT.md` |
| `agent-killtest/` | Do AI agents need Panoply's file tools to do everyday file jobs? | Agents with only command line tools passed 10 of 10, so MCP is a bonus, not a pillar. See `agent-killtest/RESULT.md` for the conditions |
| `llm-bench/` | Which small language model fits? | Stopped: Panoply ships no language model ([ADR 0003](../docs/adr/0003-no-bundled-language-model.md)). Harness kept for reference |
| `ilovepdf-compare/` | Does PDF compression beat a popular web tool at equal quality? | Not run. The web side could not be driven from the sandboxed browser, and no figure is claimed. See `ilovepdf-compare/RESULT.md` |

## Old names

These apps were built under the working name "Arsenal" before the project became Panoply. The READMEs
here use the Panoply name, but code, bundle names, environment variables (`ARSENAL_*`) and build outputs
(`Arsenal.app`, `arsenal_bar.app`) keep the old name on purpose, so the recorded commands and results
still match what you build.

## Rebuilding

Build outputs, downloaded models and generated fixtures are not committed (see the root `.gitignore`).
Each folder's README lists how to rebuild it. The prompts and fixtures in `agent-killtest/` are kept
as they were run, including the machine paths they were run from.

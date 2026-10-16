# Ledger

The running list of what Panoply has shipped, what is being built, and what has to be true before the
public launch. Updated whenever something lands. Newest first in each section.

## Public launch checklist

Target: **Tuesday, October 20, 2026.** Every box is checked before the launch film and post go out. The
quality bar does not move to make the date. Owner is who drives it; "crew" means the agents Claude runs.

### Product

| | Item | Owner | Status |
|---|---|---|---|
| [x] | User pass of every tool, every state, light and dark, keyboard only: zero open P0 and P1 | crew (user pass) | 15 P1 found, 15 fixed (#13, #15) |
| [x] | Command bar: no "No tools match" flash or list jitter while typing a request | crew (user pass) | fixed on qa/user-pass |
| [x] | Instant search: under 10 ms per keystroke on 1M files, live index, Find page, files in the command bar | crew (search) | merged (#14) |
| [x] | Instant search on Windows (ReadDirectoryChangesW, USN opt-in) and Linux (inotify) | crew (search) | merged (#14) |
| [x] | Disk map: scan, treemap, review and move to trash with Undo, protected system folders, CLI | crew (disk map) | merged (#12) |
| [x] | Disk map home scan under 300 MB peak | crew (disk map) | 289 MB on 8.1M files |
| [x] | Three real community plugins, bundled in every package | crew (plugins) | merged (#10) |
| [ ] | v0.2.0 release with all of the above, smoke-tested on real Windows and Linux desktops | Claude | release candidate building, journeys queued |
| [x] | Scripted user journeys on real Windows and Linux desktops in CI | crew (journeys) | on main; caught the v0.1.2 typing P0, fixed in v0.1.3 |

### Launch assets

| | Item | Owner | Status |
|---|---|---|---|
| [x] | Teaser posted on X (16:9, Adrenaline) | Anzal | posted 2026-10-15 |
| [ ] | Teaser final: zero P0 and P1 in review, vertical cut for Reels and Shorts | crew (video) | fixing pass-2 review |
| [ ] | Launch film on Shinji with real search and disk map captures, zero P0 and P1 | crew (video) | draft 3 with real captures, under review |
| [ ] | Launch post in Anzal's voice, every number checked against the v0.2.0 build | Claude | drafted |
| [ ] | README, social preview and download links on v0.2.0 | Claude | not started |
| [ ] | Music license proof saved (Shinji, Adrenaline) | crew (video) | done in docs/launch/music-proof |

### Engineering

| | Item | Owner | Status |
|---|---|---|---|
| [x] | Local checks for macOS, Linux and Windows before every push | Claude | on main |
| [ ] | Local Linux check builds the wasm fixtures (lld flag scoped to the Linux target) | crew (disk map) | fixing |
| [x] | Real-desktop smoke on every release tag | Claude | on main |
| [ ] | Windows CI under 6 min (workspace crate opt-level, sccache) | crew (CI) | paused until launch work frees runners |

### Decided

- No paid services for launch: no code signing, no paid runners. The first-open steps stay in the README
  and KNOWN-LIMITATIONS.
- Real-device testing beyond CI comes from users after launch; CI runs scripted user journeys on GitHub's
  hosted Windows and Linux desktops before every release.

## In progress

- **User pass** (crew). Findings in `docs/research/user-pass-macos.md` (the tools, run on a Mac) and `docs/research/user-pass.md` (the scripted journeys on Windows and Linux), fixes on branch `qa/user-pass`.
- **Instant search** (crew). Branch `feat/instant-search`, results in `docs/research/instant-search-index.md`.
- **Disk map** (crew, branch `feat/disk-map`). Scanner built and measured: a 440 GiB home folder (8.1 million
  files) in about 38 s, 9x faster than `du`, clones and hard links counted once, peak memory 289 MB. The
  treemap view, the largest-items sidebar, the Review and move to trash flow with Undo, the Full Disk Access
  explainer and `panoply disk <path>` are built. Left before the gate: a run on a real Windows PC, and the
  view on a full disk by hand.
- **Launch film and teaser** (crew). Branch `launch/video`, plans in `docs/launch/`.
- **Windows CI speed-up** (paused).

## Shipped since the checklist

- v0.1.3-beta (2026-10-16): type straight after the hotkey on Windows and Linux, photo PNGs under a target size, 150% scaling, the Linux hotkey race.

## Shipped

- **v0.1.2-beta** (2026-10-15). Linux opens a window at last. Undo moves results to the Recycle Bin or trash.
  Windows tray icon and a centred command bar. Escape hides the Linux bar. Wasmtime 49.0.3 for
  RUSTSEC-2026-0352 to 0357. Narrower fan icon. First window fits small screens.
- **v0.1.1-beta** (2026-10-14). Fan icon, one design system across the app (UI audit), Windows OCR crash and
  QR share fixes, passport head tilt fix off macOS, rustls advisories.
- **v0.1.0-beta** (2026-10-14). First public beta, 31 tools, macOS, Windows and Linux, command line.

## Engineering

- Local checks before every push: macOS, Linux (colima, about 18 s warm) and Windows (cargo-xwin, about
  13 s warm), one shared build lock. See `docs/CI.md`.
- Real-desktop smoke on every release tag (`.github/workflows/desktop-smoke.yml`).
- CI about 10.5 min warm, main runs never cancelled.

## Known issues

See [KNOWN-LIMITATIONS.md](KNOWN-LIMITATIONS.md) and the open rows in
[design/UI-AUDIT.md](design/UI-AUDIT.md).

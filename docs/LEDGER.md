# Ledger

The running list of what Panoply has shipped, what is being built, and what has to be true before the
public launch. Updated whenever something lands. Newest first in each section.

## Public launch gate

Panoply is in public beta. The launch for everyone (the video, the post, the push) happens when every
line below is checked.

- [ ] **User pass, every tool.** Every tool used like a person would use it, on macOS, Windows and Linux:
      every state, every option, light and dark, keyboard only, small and large windows. Zero open P0 or P1
      findings. The quality bar does not move to make the date.
- [ ] **Instant search.** Every file on the disk by name, results as you type, under 10 ms per keystroke on a
      million files, index kept fresh by the OS change journal, low idle cost.
- [ ] **Disk map.** What is eating your space, a full scan of a large disk in seconds, drill down, reveal
      and send to the trash safely.
- [ ] **Real desktops.** The release-tag smoke test green on Windows and Linux, and the app opened by hand on
      at least one real Windows PC.
- [ ] **Launch video.** Motion graphic demo cut to a licensed track, reviewed second by second.
- [ ] **Launch post.** Written in Anzal's voice, every number checked against the shipped build.

## In progress

- **User pass of every tool** (agent). Findings go to `docs/research/user-pass.md`, fixes on branches.
- **Instant search engine** (agent). Index design and benchmark against the fastest tools, then the module.
- **Disk map** (agent, branch `feat/disk-map`). Scanner built and measured: a 440 GiB home folder (7.7 million
  files) in 36 to 40 s, 9x faster than `du`, clones and hard links counted once, peak memory 287 MB. The
  treemap view, the largest-items sidebar, the Review and move to trash flow with Undo, the Full Disk Access
  explainer and `panoply disk <path>` are built. Left before the gate: a run on a real Windows PC, and the
  view on a full disk by hand.
- **Launch video pre-production** (agent). Track shortlist with licenses, beat map, storyboard.
- **Windows CI speed-up** (paused). Next: lighter optimisation for workspace crates in the CI profile, then
  sccache.

## Ready for the next release (v0.1.3)

- Linux: the hotkey always hides the command bar, also with a compositing window manager.

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

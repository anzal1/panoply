# User pass: scripted journeys on Windows and Linux desktops

Version tested: v0.1.2-beta, the shipped files (the .msi installed for real, the .AppImage run).
Runners: GitHub-hosted `windows-latest` (interactive session, real keyboard and mouse input, Windows.Media.Ocr)
and `ubuntu-latest` (Xvfb, openbox, software Vulkan, xdotool, tesseract). Nothing ran on a personal Mac.
Workflow: Desktop journeys, branch `ci/user-journeys`. Full run
https://github.com/anzal1/panoply/actions/runs/38791137704 (all journeys except 2), journey 2 rerun with its final
script in https://github.com/anzal1/panoply/actions/runs/38792550978. The artifacts `journeys-windows` and
`journeys-linux` of each run hold every screenshot, OCR text and file listing.

## Result by journey

| Journey | Windows | Linux |
|---|---|---|
| 1 First launch | pass (first frame 1.3 s, 24 named UI Automation elements) | warn (first frame 5.3 s on software rendering) |
| 2 Hotkey, "compress", panel opens | FAIL, bug P0-1 (typing does not land until the box is clicked) | FAIL, bug P0-1 |
| 3 PNG "under 50 kb", output under 50 KB | pass (103 KB to 40 KB, `(compressed)` name) | pass (104 KB to 38 KB) |
| 3b Photo saved as PNG, "under 50 kb" | FAIL, bug P1-1 | FAIL, bug P1-1 |
| 4a Merge two PDFs, page count | pass (5 pages) | pass (5 pages) |
| 4b OCR a scanned PDF, text extractable | pass (the page text comes out) | pass (the page text comes out) |
| 5 Undo, file in the Recycle Bin or trash | pass (Recycle Bin, Ctrl+Z) | pass (freedesktop trash, Ctrl+Z) |
| 6 Close window, tray, Quit | pass (window close, tray icon, tray menu, Ctrl+Q) | pass (no tray on Linux; close ends the app; Ctrl+Q quits) |
| 7 Light and dark, scaling | FAIL at 150 percent, bug P1-2 (light, dark and 125 percent pass) | pass (light, dark, 125 and 150 percent) |
| 8 Idle memory and CPU after 60 s | pass (47 MB working set, 82 MB private, 0.05 percent CPU) | warn (151 MB resident against the 100 MB budget, 0.43 percent CPU; software Vulkan) |

Every other failure from the first runs was the script, not the app, and is fixed on the branch: the Windows
script did not parse (`Data` is a PowerShell keyword), the PNG fixture was a grainy photo (so "under 50 kb" on it
is bug P1-1, kept as journey 3b, while journey 3 now uses a flat graphic), the hotkey journey expected only
Compress image, and OCR needs Return on the options step plus two download sheets.

## Bugs

### P0-1: after the hotkey the command bar shows but typing does nothing until the search box is clicked (Windows, and Linux under the test desktop)

- Journey 2. Ctrl+Alt+Space opens the bar, the script waits 2 s, types "compress", and waits up to 7 s more. The box
  still shows the placeholder "Search 31 tools and your files, or drop anything" and the "Suggested" list, unchanged
  (`user-pass/win-bar-typed-nothing.png`, `user-pass/linux-bar-typed-nothing.png`). Enter then opens whatever row is
  first, Compress PDF, which only looks right because the Suggested list happens to start with a Compress row.
- After one click into the search box the same typing works at once: "compress" shows and the list becomes "Best
  matches" with Compress PDF, Compress image and Compress video (`user-pass/win-bar-typed-after-click.png`,
  `user-pass/linux-bar-typed-after-click.png`).
- On Windows the bar is the foreground window when the typing starts (`bar_has_focus_before_typing` is True) and Enter
  does reach it, so the keyboard events arrive and the search field is not the focused element. A person who presses
  the hotkey and types, the headline flow, gets nothing.
- Linux caveat: the bar is an unmanaged window there and the script focuses it with xdotool, so the Linux result is
  likely but less certain than the Windows one.
- Expected: the bar puts keyboard focus in its search field when it appears.

### P1-1: "under 50 kb" on a photo saved as PNG promises a result, then fails with a dead end (Windows and Linux)

- Journey 3b. Open a 2.2 MB photo PNG, type "under 50 kb", Enter.
- The Compress image options page shows a green line "Expected: 2.2 MB to at most 50 KB" (screenshot
  `user-pass/win-under-50kb-photo-promise.png`), Run is pressed, and the result is "That did not work. photo.png could
  not be compressed with these settings. Try a larger size or another format." with only Close and Try again
  (`user-pass/win-under-50kb-photo-failed.png`, `user-pass/linux-under-50kb-photo-failed.png`). Nothing is written.
  Try again repeats the same failure.
- Why: Format `keep` plus a PNG with more than 5000 sampled colors counts as photographic, so only lossless PNG is
  tried (`looks_photographic` in `crates/panoply-image/src/ops/search.rs`, `kinds_for` in
  `crates/panoply-image/src/ops/compress.rs`), and a lossless photo never fits. The "Expected" line and the
  "Format keep" chip say it will work. The same request on a flat PNG works (journey 3), so it is the common
  exam-portal case (a photo saved as PNG) that breaks.
- Expected: when the size cannot be met in the kept format, either try JPG or WebP and say so in the result, or
  say it on the options page and offer one click to switch format.

### P1-2: first-launch window is larger than the screen at 150 percent scaling on a 1920 by 1080 display (Windows)

- Journey 7. Display scale set to 150 percent (the default recommendation on many 1080p laptops), app started with
  no saved window.
- The window opens at 259,131 and is 1762 by 1091 pixels on a 1920 by 1080 screen: the right side (the close button,
  the end of the search bar) and the bottom are off screen (`user-pass/win-150pct-window-off-screen.png`). At 100
  and 125 percent it fits.
- Likely cause (a reading of the code, not yet confirmed on a Windows machine): `window_state::first_frame`
  already shrinks the window for a 1280 by 720 point display, and its tests pass, but `ui/live.rs` feeds it
  `cx.displays()` bounds. On Windows those look to be physical pixels (1920 by 1080) and not points, so no shrink
  happens (the window is 1175 by 727 points, exactly the default 1176 by 728). Dividing the display bounds by the
  display scale factor, or reading them in points, would fix it.
- Linux at Xft.dpi 144 is fine (1740 by 1053 on 1920 by 1080).

## Notes that are not bugs

1. The word "compress" matches three tools (PDF, image, video) and Enter opens the first, Compress PDF. The journey
   accepts any Compress panel and records which one opened.
2. Linux first frame (5 to 8 s) and 151 MB resident are measured on software Vulkan. Compare on a machine with a GPU
   before treating either as a verdict.
3. Not covered: following the system theme on Linux (forced with `PANOPLY_APPEARANCE`), the AT-SPI tree on Linux,
   physical high-DPI hardware.

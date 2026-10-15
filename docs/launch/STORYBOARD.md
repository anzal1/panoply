---
compositionId: panoply-launch
duration_s: 36.92         # 80 beats at 130.0 BPM (the pick, see MUSIC.md). Final value = audiomap.audio.duration_sec after the track is cut.
canvas: { w: 1920, h: 1080, fps: 60 }
style:                    # the owner picks one of three, see style-frames/. Recommended: A for the cold open, C from the drop on.
  font: "Geist / Geist Mono"
  palette: ["#0C0C0F", "#1C1C1F", "#ECECF1", "#2D6CDF", "#0A84FF"]   # ground, app window, ink, mark blue, accent
assets: "docs/design/review/after (real app renders), docs/assets (mark, logos), docs/launch/fonts (Geist)"
build_notes: ["one paused timeline per frame", "no remote assets", "every product pixel is a real selfcheck render or a component rebuilt from real tokens and real module names"]
avoid: ["shockwaves", "screen shakes", "bounce or overshoot easing", "burst rays", "game-like SFX", "flash frames", "naming a competitor", "em dashes", "claims we have not measured"]
---

# Panoply launch film, storyboard

36.9 seconds at 130.0 BPM (20 bars), 16:9, 60 fps, cut to a licensed human-made track. Ships at public launch,
once instant search and the disk map exist (see `docs/LEDGER.md`, the launch gate). Frames 4 and 5 are
placeholders until those two ship, and they stay placeholders in every review cut.

The cue sheet for any tempo is generated, never typed by hand:

```
node docs/launch/tools/cues.mjs --bpm 130 --offset 25.884 --md     # seconds and frame numbers for every cue below
```

## The idea in one line

Chaos you know, then one calm hotkey, then everything lands on the beat.

The film has one argument: the pain is a pile of tabs that take your file somewhere else, the answer is one
keystroke on your own machine, and the moat is that anyone can add a tool. Nothing in the film is louder than the
music. The track brings the energy, the picture brings the clarity.

## Taste rules, written down so a worker cannot miss them

1. Alive and clear, never louder. Awe comes from showing the real mechanism: a file landing on the bar, the list
   re-sorting, a number counting down, a folder becoming a tool. Calm precise motion, continuous slow camera drift.
2. Things arrive so that they settle on the beat. Start the move early, end it on the beat. The music is the
   impact. No bounce, no overshoot, no shockwave ring, no shake, no burst rays, no white flash frame.
3. Easing: entrances `cubic-bezier(0.16, 1, 0.3, 1)` (expo-out), exits `cubic-bezier(0.7, 0, 0.84, 0)`. Moves are
   360 to 700 ms, never longer than one bar. Transform and opacity only, plus clip-path draws.
4. Verbs: GLIDES, SETTLES, DRAWS, FILLS, COUNTS UP, SORTS, LOCKS IN, DRIFTS, BREATHES. Not: SLAMS, SHATTERS, PUNCHES.
5. Sound: the track only. Optional, off by default, one quiet keyboard tick bed under the typing in frame 2. No
   whooshes, thuds or buzzers. Most people watch X muted, so every beat also reads with the sound off.
6. Never open on a competitor and never name one in the film. The pain is generic: no logos, no brand colors, no
   recognisable site layouts. A brand name appears only as a feature, never.
7. Copy is short, human, plain. No em dashes, no colon constructions, no three-item AI cadence, no "seamless" or
   "powerful". Sentence case, one idea per card.
8. Every number on screen is from the shipped build or a measured run. See "Claims to verify" below.

## Tempo and structure

All timing is in beats, so the film re-times itself to any track from 120 to 150 BPM.

| BPM | 120 | 124 | 128 | 130 | 140 | 150 |
|---|---|---|---|---|---|---|
| Film length | 40.0 s | 38.7 s | 37.5 s | 36.9 s | 34.3 s | 32.0 s |

Track requirements the shortlist was filtered on: a clear build into a hard drop, the drop on a downbeat, 4 to 8
bars of build before it. The cut puts the drop on beat 16 (bar 5, 7.38 s at 130). If the track's intro is longer, the
edit keeps the last 4 bars of the build and drops the rest. If it is shorter, the first 2 bars loop. The cut is a
single `ffmpeg` trim at a measured downbeat, done by the music-to-video analyzer's offset, never by ear.

The pick (see `MUSIC.md`) is "Shinji, Hyperpop Type Beat" at **130.0 BPM**, cut from track second **25.884** (the beat analyzer's 129.2 is a 23 ms
quantisation artefact: a fine onset fit gives 129.94, and at 129.2 the cuts would drift 230 ms by the end). The track's measured hits are at beats
8, 16 (the drop), 33, 46.5, 68, 72, 74 (the strongest) and 76, and its measured stops at 13.3 to 15.9, 32.6 to 32.9, 73.5 to 73.9 and 75.5 to 75.9, so every cut
lands on one of them. `cues-shinji-130bpm.md` has every cue with its track second; `music-analysis/hits-shinji.json` has the raw hit and stop map. Style decision: option A for frame 1, option C from
the drop on. Frames 4 and 5 stay in.

Beats, with the music situation each scene sits on:

| # | Frame | Beats | Bars | Seconds at 130 | Music |
|---|---|---|---|---|---|
| 1 | Cold open, the upload chaos | 0 to 16 | 4 | 0.0 to 7.38 | filtered build, onsets accelerating, hard stop on the last 8th |
| 2 | The hotkey | 16 to 24 | 2 | 7.38 to 11.08 | DROP lands on beat 16 |
| 3 | The tools fly in | 24 to 40 | 4 | 11.08 to 18.46 | full groove, steady kick |
| 4 | Instant search (placeholder) | 40 to 48 | 2 | 18.46 to 22.15 | groove, hats open |
| 5 | Disk map (placeholder) | 48 to 56 | 2 | 22.15 to 25.85 | groove, filter lift |
| 6 | The moat, community plugins | 56 to 68 | 3 | 25.85 to 31.39 | second build, ends in a drum fill |
| 7 | Free, open source, on your device | 68 to 76 | 2 | 31.39 to 35.08 | last full bars |
| 8 | End card | 76 to 80 | 1 | 35.08 to 36.92, plus a 1.4 s ring-out | final hit, reverb tail |

## Review rules learned from the first film review

1. Every entrance STARTS on its beat (one frame early at most). With an expo-out move the eye reads the arrival at the start of the move, so leading the beat by 0.4 s put the first draft's words 0.38 s early.
2. Grounds are Midnight for the cold open and brand blue from the drop to the end. No paper or white ground (it reads as a flash), no second blue.
3. No error states from the real app on screen. The real app flashes "No tools match" for the partial queries "under 5" and "under 50", so the typed frames are composited (`tools/fix_typed_frames.py`) until the app is fixed (a task is queued for that).
4. No claim that is not true yet: no "community" until real community modules exist.
5. Counters never run backwards and never show a number below the cards on screen; a result rests for about half a second before anything else moves.
6. Opacity gets its own tween, so nothing pops on the first frame.

## Copy deck

Everything that is ever on screen, so it can be reviewed in one place.

| Frame | Text |
|---|---|
| 1 | Upload. Wait. Sign up. Pay. / Just to shrink one PDF. / Your file just left your computer. |
| 2 | One hotkey. Any file. (keycaps: Option Space, then Ctrl Alt Space) |
| 3 | 31 tools. (the number is read from the build at render time) |
| 4 | Every file on your disk. Before you finish typing. |
| 5 | See what is eating your space. |
| 6 | Anyone can add a tool. Add yours in an afternoon. ("Built by the people." comes back only when at least three real community modules exist) |
| 7 | Free. Open source. On your device. macOS, Windows, Linux |
| 8 | Panoply, macOS Windows Linux, github.com/anzal1/panoply (the url appears only here) |

Not used, on purpose: "blazing", "seamless", "supercharge", any price comparison, any competitor, any sentence with an
em dash, "No uploads. No limits. No catch." (three-item cadence), "100 percent private" (the privacy page allows
downloads of models after consent, so the claim we make is the one `docs/PRIVACY.md` makes: your files are never
uploaded).

---

## Frame 1: pain

- src: compositions/frames/01-pain.html
- duration: 7.385s
- span_sec: [0.000, 7.385]
- pacing: beat_cut
- mood: [tense, dark]
- feel: filtered build, onsets accelerating, a hard stop on the last 8th before the drop

### Groups

- **g1: the chaos builds** (beats 0 to 12)
  - free_design: { dominant_system: "tab chaos on the beat", primitives: ["staggered-entrance", "grid-fill"], density_topology: "accumulate" }
  - anchors: beat 0, then every beat to beat 4, then every 8th to beat 12 (cue sheet, "windows on every beat" and "on 8ths")
  - Picture: frame 0 is already mid-trouble, one browser window with "Drop your PDF here". Windows GLIDE in from slightly
    larger (scale 1.04 to 1) on each beat, then twice per beat, until fifteen windows cover the frame. Each window is a
    different generic page: a dashed drop zone, an upload bar stuck at 43 percent, "Daily limit reached", a watermarked page,
    "Select all squares with traffic lights", "Where should we send your file?", a cookie banner, "File too large".
    Slow continuous camera drift over the whole wall (1.5 percent scale over the scene). Palette is the style option's.
  - Text: the four words land on beats 4, 6, 8 and 10, one each, set big in two lines:
    "Upload. Wait." / "Sign up. Pay." Earlier words dim as the next one arrives, the last word carries the accent.
  - Real or rebuilt: rebuilt HTML windows, deliberately unbranded. Reference frames: `style-frames/*-pain.png`.
- **g2: the freeze and the line** (beats 12 to 16)
  - free_design: { dominant_system: "scrim and single line", primitives: ["scrim-in", "text-reveal"], density_topology: "reduce" }
  - anchors: beat 13.32 (the measured stop begins: scrim), 14.0 (line, inside the silence), 15.93 (the stop ends, then the drop on 16)
  - Picture: the wall stops moving at beat 13.32, where the track goes silent. A scrim DRAWS in over it, the four words slide up and fade to a quiet
    line, "Your file just left your computer." appears and holds. The music is silent until beat 15.93 and the picture is as still as it:
    one beat of still frame, then black on beat 16. Silence is the build.
  - Text: "Your file just left your computer."

## Frame 2: hotkey

- src: compositions/frames/02-hotkey.html
- duration: 3.692s
- span_sec: [7.385, 11.077]
- pacing: beat_cut
- mood: [hype, elegant]
- feel: the drop, then a full groove

### Groups

- **g1: the drop** (beats 16 to 18)
  - free_design: { dominant_system: "calm ground, keycaps, command bar", primitives: ["scale-settle", "ground-swap"], density_topology: "reveal" }
  - anchors: beat 16 (drop), 16.5 (bar opens), 17 (keycaps swap to Ctrl Alt Space)
  - Picture: black cuts to a calm, bright ground (brand blue in option C, soft ink in A and B). Two keycaps, Option and Space,
    are already down on beat 16, no flash and no ring. The ground is the change: the noise of frame 1 is gone. Half a beat
    later the real command bar opens in place (opacity 0 to 1, scale 0.97 to 1, 8 frames, expo-out). On beat 17 the keycaps
    cross-fade to Ctrl Alt Space, a quiet nod that this is the same on Windows and Linux.
  - Text: "One hotkey. Any file."
  - Real captures: `bar-rest-{dark|light}` (empty bar), RGBA with transparent corners from `--selfcheck-frames`.
- **g2: drop a file, say what you want** (beats 18 to 24)
  - free_design: { dominant_system: "real UI sequence", primitives: ["frame-sequence", "list-sort"], density_topology: "steady" }
  - anchors: 18 (file lands), 18.5 (list sorts), 20 to 22.75 (eleven typed characters on 16ths), 23 (understood chip), 23.5 (Return)
  - Picture: a file chip GLIDES onto the bar on beat 18 and the tool list sorts in on 16ths, best match first. The camera
    pushes in 6 percent over the whole group. The owner types "under 50 kb" for real, character by character, one per 16th
    note, using the captured typing frames. The "Understood as a target size" line and the three option chips resolve on
    beat 23, Return on 23.5.
  - Real captures: `bar-pdf-*`, `demo-*-01..NN` (typed frames), `bar-words-*`, `bar-options-*`.

## Frame 3: tools

- src: compositions/frames/03-tools.html
- duration: 7.385s
- span_sec: [11.077, 18.462]
- pacing: beat_cut
- mood: [hype, elegant]
- feel: full groove, steady kick

### Groups

- **g1: the result** (beat 24 to 26)
  - free_design: { dominant_system: "result card", primitives: ["frame-sequence", "count-up"], density_topology: "hold" }
  - anchors: 24 (result), 25 (numbers count)
  - Picture: the run frames play and the real result lands on beat 24, "67 KB to 43 KB, 35% smaller", the after number
    COUNTS UP in the app's green. Hold for two beats so the payoff reads. This is the only still-ish moment in the first
    half. The numbers are those of the real run, re-captured at render time.
  - Real captures: `run-*`, `bar-result-*`, `app-tool-result-*`.
- **g2: everything else, on the beat** (beats 26 to 40)
  - free_design: { dominant_system: "tiles and counter", primitives: ["staggered-entrance", "count-up", "grid-fill", "row-highlight"], density_topology: "accumulate" }
  - anchors: 26 (first tile), one per beat to 31, 8ths to 32.6, the stop 32.61 to 32.93, 33 (HIT .79: the rest ripple in, counter locks), 34 to 38 (rows light), 39 (resolve)
  - Picture: the bar slides down and shrinks to a small anchor. Real tool tiles (icon, name, category tint, rebuilt from
    `modules/builtin/*/module.toml` and the app's palette) GLIDE in from below on every beat, then on 8ths, and settle into
    a grid that is the shape of the real Home screen. A big counter counts up with the tiles and locks on the real tool
    count on beat 33, the measured hit. From beat 34 the categories light in sequence, one per half beat: PDF, Images, Video, ID photos, Files.
    Slow orbit of the whole grid, 3 degrees, no tilt past that. On beat 39 the grid resolves into the real `app-home` render by
    cross-fade, so the viewer ends on the actual product.
  - Text: "31 tools." (read from the build)
  - Rebuilt from real data: the tile list is generated from the module manifests, so the film cannot show a tool that is not in
    the build. Reference: the pills in `style-frames/b-paper-beat.png`.
  - Real captures: `app-home-*`.

## Frame 4: find (PLACEHOLDER until instant search ships)

- src: compositions/frames/04-find.html
- duration: 3.692s
- span_sec: [18.462, 22.154]
- pacing: beat_cut
- mood: [hype]
- feel: groove, open hats

### Groups

- **g1: type three letters, the list is already there** (beats 40 to 48)
  - free_design: { dominant_system: "real UI sequence, one measured number", primitives: ["frame-sequence", "list-sort", "count-up"], density_topology: "steady" }
  - anchors: 40 (Find view), 41 to 41.5 (three keystrokes on 16ths), 44 (line one), 46.5 (HIT .53: latency pill and line two)
  - Picture: the Find view opens. Three letters are typed on three 16ths. After each one, the result list re-sorts at once,
    and a small pill shows the real measured time for that keystroke and the real file count of the scanned disk.
    The camera drifts left to right across the result list.
  - Text: "Every file on your disk." then "Before you finish typing."
  - Gate: every digit on this frame comes from the engine's own benchmark JSON (the launch gate says under 10 ms per keystroke
    on a million files). If the shipped number is higher, the pill shows the shipped number. If instant search is not ready, this
    frame is cut and the film is 33.2 s at 130 BPM (18 bars, cues re-run).
  - Placeholder now: `app-find-light.png` from the review set, plus a typed-frame capture mode to add (see PRODUCTION.md).

## Frame 5: diskmap (PLACEHOLDER until the disk map ships)

- src: compositions/frames/05-diskmap.html
- duration: 3.692s
- span_sec: [22.154, 25.846]
- pacing: beat_cut
- mood: [hype, elegant]
- feel: groove, filter lift

### Groups

- **g1: the biggest block** (beats 48 to 56)
  - free_design: { dominant_system: "treemap from real scan data", primitives: ["grid-fill", "zoom-drill", "count-down"], density_topology: "accumulate then focus" }
  - anchors: 48 (rects grow on 8ths), 50 (drill), 52 (move to trash), 53 (freed counter), 54 (line)
  - Picture: a treemap of a real scan FILLS in on 8ths, rectangles growing from the corners with expo-out. On beat 50 the camera
    zooms into the biggest rectangle, which splits into its folders, then into one file. On beat 52 the file is sent to the
    trash with the app's real confirm sheet, the rectangle dissolves, and the freed number COUNTS DOWN on the disk bar.
  - Text: "See what is eating your space."
  - Gate: scan data and sizes come from the real scanner on the demo machine. If the disk map is not ready, this frame is cut.
  - Placeholder now: a generated treemap (squarified, labelled PLACEHOLDER in the review cut only).

## Frame 6: plugins

- src: compositions/frames/06-plugins.html
- duration: 5.539s
- span_sec: [25.846, 31.385]
- pacing: beat_cut
- mood: [elegant, hype]
- feel: a second build ending in a drum fill

### Groups

- **g1: a tool is a folder** (beats 56 to 62)
  - free_design: { dominant_system: "fan mark, folder, module card", primitives: ["draw-on", "stagger", "slide-into"], density_topology: "build" }
  - anchors: 56 (UI recedes), 57 (line), 58, 59, 60 (three files), 61 (slides into Modules), 62 (badge)
  - Picture: the grid from frame 3 pulls back into the fan mark (the three cards of the icon, each card carrying a screen).
    On beat 58, 59 and 60 three small file chips DRAW into a folder: `module.toml`, `tool.wasm`, `icon.svg`. On beat 61 the
    folder becomes a module card and SLIDES into the real Modules list, which updates from "31 installed, 0 from the
    community" to "32 installed, 1 from the community" with the app's own badge. A COUNT UP on the badge on beat 62.
  - Text: "Anyone can add a tool."
  - Real captures: `app-modules-*` with a real community module installed (see PRODUCTION.md, community plugin prerequisite).
- **g2: the wall** (beats 62 to 68)
  - free_design: { dominant_system: "wall of community tools", primitives: ["grid-fill", "staggered-entrance", "text-reveal"], density_topology: "accumulate" }
  - anchors: 63 (wall starts), 65 (line), 67.5 (wall fills), 68 (HIT: the wall locks and the film cuts to frame 7)
  - Picture: the Modules list zooms out into a wall of tool cards in the app's category tints, each from a real community
    module (at launch, the real ones; the number is real). Cards arrive faster into the drum fill. Camera pulls back
    continuously. No fake modules: if the wall is thin at launch, the frame shows the real few, large, and the line carries it.
  - Text: "Built by the people."
  - Gate: this is the moat. It needs at least three real community modules in the repo before the film is cut.

## Frame 7: promise

- src: compositions/frames/07-promise.html
- duration: 3.692s
- span_sec: [31.385, 35.077]
- pacing: beat_cut
- mood: [elegant, hype]
- feel: last full bars before the end

### Groups

- **g1: three phrases** (beats 68 to 76)
  - free_design: { dominant_system: "full-bleed type, one phrase per two beats", primitives: ["text-reveal", "wipe"], density_topology: "one thing at a time" }
  - anchors: 68 (HIT, "Free."), 72 (HIT .70, "Open source."), the stop 73.54 to 73.93, 74 (HIT 1.00, the strongest, "On your device."), the stop 75.54 to 75.92, 76 (the final hit, frame 8)
  - Picture: each phrase GLIDES in at full width and holds for two beats, the previous one slides up and out. Under
    each, one quiet real thing: "Free." with the release page showing the download buttons (real
    screenshot of the release assets list), "Open source." with the repo's licence line (MIT OR Apache-2.0) and the
    commit graph, "On your device." with a file icon that stays inside a laptop outline while a network line beside it stays
    flat. The platform row "macOS, Windows, Linux" sets in with "Free." on beat 68.
  - Text: "Free." / "Open source." / "On your device." / "macOS, Windows, Linux"
  - Real captures: the repo page and the release page, taken at launch day from the live GitHub pages by `capture`.

## Frame 8: end

- src: compositions/frames/08-end.html
- duration: 1.846s
- span_sec: [35.077, 36.923]
- pacing: beat_cut
- mood: [elegant]
- feel: the final hit, then reverb ring-out

### Groups

- **g1: the mark** (beats 76 to 79, then 3 beats of ring-out)
  - free_design: { dominant_system: "fan mark and wordmark", primitives: ["scale-settle", "text-reveal"], density_topology: "hold" }
  - anchors: 76 (mark and wordmark on the downbeat), 77 (url)
  - Picture: the fan mark's three cards open from a single card (the blue one) to the full fan, ending exactly on the final
    hit. "Panoply" settles under it, the url appears one beat later. Then nothing moves. The picture holds still for the whole
    ring-out, and the track fades over the last 1.4 s (an extra 3 beats beyond the video's last bar). No logo bounce, no glow
    pulse.
  - Text: "Panoply" and "github.com/anzal1/panoply"
  - Reference: `style-frames/*-end.png`.

---

## Claims to verify before render (a worker must fill these from the build, not from this file)

| On-screen claim | Where | Source of truth |
|---|---|---|
| "31 tools" | frame 3 | count of `modules/builtin/*/module.toml` plus built-ins at the release tag |
| "67 KB to 43 KB, 35% smaller" and the run time | frame 3 | the real captured run (`bar-result-*`); re-captured by `--selfcheck-frames` at the release tag |
| the keystroke latency and file count | frame 4 | the instant search benchmark JSON of the release |
| freed space number | frame 5 | the real scan and trash run on the demo machine |
| the Modules count and the wall | frame 6 | the draft says "32 installed" and shows the repo's two example modules as examples. Any "from the community" wording needs three real community modules in the capture build |
| licence line | frame 7 | `LICENSE-MIT` and `LICENSE-APACHE` at the release tag |
| "On your device" | frame 7 | `docs/PRIVACY.md` (files are never uploaded, models download only after a yes) |

## What the owner decides

1. The track (see `MUSIC.md`). The film re-times to any of the six.
2. The style (see `style-frames/contact-sheet.png`): A Midnight, B Paper, C Fan, or the recommended hybrid.
3. Whether frames 4 and 5 are in the first cut, or the film ships at 16 bars (29.5 s at 130 BPM) without them.

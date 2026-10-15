# Launch film, production plan

Pre-production only. Nothing here has been rendered. The storyboard is `STORYBOARD.md`, the track shortlist is
`MUSIC.md`, the three looks are in `style-frames/`.

The shape of the plan: the real app is captured once, on a runner. The film is built from those captures and from
components rebuilt out of real tokens and real module data. The picture renders on GitHub Actions in this public
repo, so it costs nothing and does not touch the owner's Mac. The licensed track is added in a few seconds at the end.

## Pipeline

```
 1 capture        2 data            3 build              4 check            5 render           6 finish
 real app    ->   tools.json   ->   hyperframes     ->    lint + check  ->   silent picture ->  mux track
 frames           claims.json       project, one         + snapshots        master, 60 fps      loudness
 (RGBA PNGs)      cues for BPM      file per frame       + 2nd by 2nd       (Actions)           X export
```

1. **Capture** (needs macOS and the app). `panoply-desktop --selfcheck-frames --out FRAMES --appearance dark|light`
   already writes raw RGBA frames with transparent corners, including the typed "under 50 kb", the run frames and the
   result. Section "Captures" lists what is there and what to add. Output is a zip artifact, never committed to git.
2. **Data.** `tools/build-data.mjs` (to write) reads every `modules/builtin/*/module.toml` and emits `tools.json` (id,
   name, category, summary, and the tool count the film prints). A second script writes `claims.json` from the release
   tag: licence lines, the tool count, and later the search and disk map benchmark JSON. The film reads these at build
   time, so it cannot show a tool that is not in the build or a number nobody measured. `tools/cues.mjs` (exists) turns the
   beat plan into seconds and frames for the real BPM.
3. **Build.** One HyperFrames project in `docs/launch/video/`, made with the `music-to-video` workflow's skeleton and the
   `product-launch-video` motion rules. One composition file per storyboard frame (`compositions/frames/NN-*.html`),
   eight files, built by one worker each in parallel, assembled by the deterministic `assemble-index.mjs`. Style tokens
   come from the chosen option in `style-frames/frames.html`.
4. **Check.** `npx hyperframes check . --snapshots --strict` on the assembled project (structure, runtime, layout, motion
   and contrast in one pass), a contact sheet at every cue, then a second-by-second review (see "Review gates").
5. **Render.** The silent picture master on Actions (see "Render on GitHub Actions").
6. **Finish.** Download the picture, mux the track, normalise loudness, export for X (see "Audio" and "Export for X").

## Captures

Everything product-shaped is a real render of the real app. Names are those the self-check already writes into
`FRAMES/` (see `apps/desktop/src/selfcheck.rs`, `run_frames`).

| Frame | Needs | Source | Status |
|---|---|---|---|
| 1 pain | nothing from the app | rebuilt HTML windows, unbranded | design done in `style-frames/frames.html` |
| 2 hotkey | empty bar, PDF dropped, typed "under 50 kb", chips, options | `bar-rest-*`, `bar-pdf-*`, `demo-*-NN` (typed frames), `bar-words-*`, `bar-options-*` | exists |
| 3 result | run frames, result card, result panel | `run-*-NN`, `bar-result-*`, `app-tool-result-*` | exists, but see "The PDF result" below |
| 3 tools | Home screen, the module list | `app-home-*`, `modules/builtin/*/module.toml` | exists, tiles rebuilt from data |
| 4 find | typed frames of the Find view with real results and a measured time | new `--selfcheck-find` | blocked on instant search |
| 5 disk map | treemap states at 3 drill levels, the trash sheet, the freed bar | new `--selfcheck-diskmap`, scan data as JSON | blocked on the disk map |
| 6 plugins | Modules screen before and after a community module is installed | `app-modules-*` plus a second capture with the module installed | needs real community modules |
| 7 promise | the repo page, the releases page, the licence files | live GitHub pages, captured on launch day | launch day |
| 8 end | mark and wordmark | `docs/assets/panoply-mark*.svg`, Geist | exists |

Capture rules:

- Dark and light of everything, 2x. The film uses whichever the chosen style needs; capturing both is free.
- The captures are re-run at the release tag the film is rendered from, so the UI in the film is the UI that ships.
- Raw frames are premultiplied: undo that exactly like `apps/desktop/tools/readme_assets.py` does (`load`). The existing
  review renders in `docs/design/review/after/` are flattened onto a desktop colour, which is fine for style frames but
  not for the film, where the corners must stay transparent over a moving ground.
- No seeded fake data on screen. The recents list in the self-check is seeded text, not real runs, so the film never uses
  a number from it.

### The PDF result

The cold open is about a PDF, so the strongest payoff is a real PDF getting smaller. The self-check's typed demo uses
`signature.png` (67 KB to 43 KB, 35 percent). Add one capture that compresses a real sample PDF (the corpus has them) and
use the actual before and after sizes and time from that run. Do not reuse "4.2 MB to 880 KB, 79 percent" from the
seeded recents: that is demo text. If the real PDF run is less dramatic, use the signature result, it is real.

### Capture modes to add (small, in `selfcheck.rs`)

1. `--selfcheck-find`: types a query into the Find view one character per step and writes a frame per step, plus a JSON
   with the measured time per keystroke and the file count. Needs the instant search module.
2. `--selfcheck-diskmap`: a scan of a prepared folder tree, frames at overview, drill 1, drill 2, trash sheet, after trash,
   and the scan JSON (name, size, children) so the film can animate the treemap from data. Needs the disk map module.
3. A real PDF compress run in the typed sequence, as above.
4. Optional, only if cross-fading settled states looks stiff: a fixed-clock `--selfcheck-film` that steps the UI
   timeline at 60 fps and writes every frame. This is the upgrade that turns "settled states" into true UI motion. It is
   more work and is not on the critical path. The storyboard is built to work without it.

### Where captures run

GPUI's offscreen render uses Metal, so captures need macOS. Two options, in order:

1. **A `macos-latest` runner in this public repo.** Free. Whether GPUI's offscreen Metal render works on a virtualised
   runner GPU is not verified. The first task is a ten-minute `workflow_dispatch` probe that builds with the `snapshot`
   feature and runs `--selfcheck-frames`. CI does not run this today (`ci.yml` has no snapshot build), so there is no
   evidence either way.
2. **Once, locally, with the owner's yes.** A release build with `--features snapshot` plus a 1 to 2 minute run, with
   `nice -n 19`, `-j2`, and the build folder named `target.noindex` so Spotlight stays out. Only if option 1 fails. Ask before
   starting, as with any build on the Mac.

The frames go up as an artifact named `launch-captures-<tag>` (or a draft release asset), not into git.

## What animates

Everything that moves is `transform`, `opacity` or a `clip-path` draw, driven by one paused GSAP timeline per frame, so
every frame is seekable and deterministic. Nothing moves faster than the music asks for and nothing overshoots.

| Element | Motion | Frame |
|---|---|---|
| Browser windows | enter on beats from scale 1.04, wall drifts 1.5 percent over the scene, scrim draws in | 1 |
| Four words | one per beat, earlier words dim | 1 |
| Keycaps | cut in on the drop, cross-fade to the other platform's keys on beat 17 | 2 |
| Command bar | opacity and 0.97 to 1 scale, then a 6 percent push-in across the group | 2 |
| Typed text | the captured typing frames, one per 16th note | 2 |
| Result numbers | count up, 400 ms, expo-out | 3 |
| Tool tiles | glide up and settle on each beat, then on 8ths, into the Home grid shape | 3 |
| Counter | counts with the tiles, locks on the real count | 3 |
| Grid | 3 degree slow orbit, then cross-fade into the real Home render | 3 |
| Find results | list re-sorts per keystroke, latency pill updates | 4 |
| Treemap | rectangles grow on 8ths, camera zooms into the biggest, freed number counts down | 5 |
| Fan mark | the grid pulls back into the three cards, each carrying a screen | 6 |
| File chips and folder | three chips draw into a folder, folder becomes a module card, slides into Modules | 6 |
| Module badge | counts up from 0 to the real community count | 6 |
| Wall of modules | grid fills, camera pulls back | 6 |
| Three phrases | glide in at full width, one per two beats | 7 |
| End mark | the three cards open from the blue one, wordmark settles, then everything holds | 8 |

What does not move: the real UI plates inside themselves (they are captures), the end card after the final hit, and the
type once it has settled.

### Built to render fast on a runner

The runner has no GPU, so Chrome uses software GL, and the HyperFrames docs warn that heavy compositions stall on it.
Rules for every frame file:

- No WebGL, no shader transitions, no `filter: blur()` or `backdrop-filter` on large layers. Bake soft shadows and glows
  into PNG plates once (the style frames show the look: a blurred copy of the card's outline, 30 px down).
- The grain is a small pre-baked tiling PNG at 12 to 16 percent, not a live canvas. It also keeps the dark grounds from
  banding after H.264.
- Images are the exact size they are shown at, 2x only where a push-in needs it. No image above 4096 px.
- `will-change: transform` only on the layers that move. No more than about 40 moving layers per frame.

## Render on GitHub Actions

The workflow draft is `render-launch-film.workflow.yml.txt` in this folder. It is not under `.github/workflows` on
purpose: copy it there when the HyperFrames project exists. It is `workflow_dispatch` only, so it never runs on a push.

| Item | Plan |
|---|---|
| Where | `anzal1/panoply`, public, `ubuntu-latest` (public repos run free, so this does not touch the 3,000 private minutes) |
| When | by hand: `quality` (draft, looks, delivery), `fps` (30 or 60), `tag` (release the captures belong to) |
| Inputs | the `launch-captures-<tag>` artifact, `tools.json` and `claims.json` generated in the job, fonts from `docs/launch/fonts` |
| Steps | checkout, Node 22, `npx hyperframes browser ensure`, build data, fetch captures, `check --strict --snapshots`, `render` |
| Output | `picture.mp4` (silent), `snapshots/contact-sheet.jpg`, `cues-<bpm>.md`, all as one artifact, 14 days |
| Time cap | `timeout-minutes: 45` |
| Estimate | 37.2 s at 60 fps is 2,229 frames. At a pessimistic 4 to 6 frames per second on software GL with 3 workers, about 6 to 10 minutes of capture plus encode. At 30 fps half that. This is an estimate, not a measurement: the first draft run replaces it. |
| Cost | zero. A full month of iteration is under 2 hours of public-runner time. |
| Not on the Mac | the Mac only does the 10 second mux and, if the owner wants it, a one-frame snapshot. The preview server is never left running. |

Iteration loop: `draft` at 30 fps for layout and timing (about 2 to 3 minutes), `looks` at 30 fps for the first
review, then `delivery` at 60 fps once for the master. Snapshot contact sheets are the review medium, so nobody
needs to run a render locally to review a change.

## Audio

The track never goes into the public repo or the CI. The Pixabay license bars distributing the track as a standalone
file and a public repo is that. So:

1. Locally, after the owner picks a track: `analyze-beatgrid.py` measures the real BPM and downbeats (a few seconds of
   CPU), a single `ffmpeg` trim puts the drop exactly on beat 16 and fades the tail, and `tools/cues.mjs --bpm <bpm>
   --offset <s>` writes the cue sheet the build uses.
2. The picture renders silent on Actions. A project variable `track` is empty in CI, so no audio element is mounted.
3. Locally, `ffmpeg -i picture.mp4 -i cut.wav -map 0:v -map 1:a -c:v copy -c:a aac -b:a 256k -ar 48000 -shortest final.mp4`
   muxes it. Copying the video stream means it takes seconds and costs no CPU to speak of.
4. Loudness: integrated about -14 LUFS, true peak under -1 dBTP, measured with `ffmpeg -af loudnorm=print_format=json`, not
   by ear. The drop is the loudest point; the build is about 6 dB under it, which the track already does.
5. A second `final-silent.mp4` is kept, for the owner's own commercial-track option (see `MUSIC.md`).

## Export for X

- 1920x1080, 16:9, progressive, H.264 High profile, yuv420p, BT.709, 60 fps master (30 fps copy if X's re-encode looks
  worse at 60, test both).
- AAC-LC 48 kHz stereo, 256 kbps. Length 37.2 s plus the ring-out is well under X's 2 minute 20 limit for standard accounts.
- Upload the highest bitrate X accepts, around 25 Mbps for the master, because X re-encodes and a clean source survives it
  best. File size will be about 120 MB, under X's 512 MB cap.
- Most X viewers see it muted and autoplaying, so frame 0 has to hold on its own, and each beat must read without sound.
  The first frame is the cold open with one window and one headline word. Set a custom thumbnail from the style frame of the
  hotkey beat, not the auto frame.
- Check the first 3 seconds on a phone-sized view and on a dark and a light timeline before posting.
- Optional later cuts, same project, different canvas: 1:1 (1080x1080) and 9:16 (1080x1920). The frames use CSS variables
  for the canvas, but each needs its own layout pass, so they are not in the first cut.

## Review gates

A change is not done until each of these has been run, in this order:

1. `npx hyperframes check . --snapshots --strict` is clean (lint, layout, contrast, motion).
2. Contact sheet at every cue from `tools/cues.mjs`, and 3x zoom crops of every state of every UI plate, as in
   `docs/design/UI-AUDIT.md`: nothing clipped, nothing touching a frame edge, optical alignment of icons and text.
3. The second-by-second review by the film reviewer agent, with the cue sheet next to it, P0 and P1 findings fixed.
4. Claims table in `STORYBOARD.md` filled from `claims.json`, every row checked by a person.
5. Silent first watch, then the muted-phone check, then with sound.
6. Taste check against the rules at the top of the storyboard: no shockwave, shake, bounce, burst, game-like sound, flash
   frame, em dash, competitor name. If a frame would look at home in a mobile game, it is cut.

## Work breakdown

Frames 1, 2, 3, 6 (apart from its wall), 7 and 8 depend only on what already ships and can be built now. Frames 4 and 5 are
built last, on the real modules.

| Step | Who | Depends on | Size |
|---|---|---|---|
| Owner picks track and style | owner | `MUSIC.md`, `style-frames/` | minutes |
| Analyze the track, write the cue sheet, trim the track | local, a few seconds of CPU | the pick | 10 min |
| Capture probe on a macOS runner | agent | nothing | 10 min |
| Real PDF compress capture and `tools.json` builder | agent | nothing | 1 hour |
| Build frames 1, 2, 3, 7, 8 | one worker per frame, parallel | style, captures, cues | 2 to 3 hours |
| Real community modules (3 or more) and Modules capture | agent | nothing | 2 hours |
| Build frame 6 | worker | community modules | 1 hour |
| Instant search, disk map, their capture modes | the engine agents | launch gate | owned elsewhere |
| Build frames 4 and 5 | workers | the two modules | 1 to 2 hours each |
| Review, fixes, delivery render, mux, X export | agent plus owner | everything | half a day |

The film is cut the day the last launch-gate box in `docs/LEDGER.md` is ticked. If search or the disk map slips, the
film still ships at 16 bars (29.5 s at 130 BPM) without frames 4 and 5 (`cues.json` changes by deleting two scenes).

## Risks

| Risk | What reduces it |
|---|---|
| GPUI offscreen render does not run on a CI macOS runner | the ten-minute probe first, then the one-time local run with the owner's yes |
| Software GL makes the render slow or flaky | no blur, backdrop-filter or WebGL; baked plates; the draft loop |
| The track's drop is not where the tags say | owner audition before anything is built; the cut moves the drop to beat 16 |
| A track claim on YouTube or Instagram | no-badge tracks first, proof saved in `music-proof/`, post to X first |
| Banding on the dark grounds after X re-encodes | baked grain at 12 to 16 percent, high-bitrate source, a test on a throwaway account |
| Numbers on screen go stale before launch | `claims.json` built from the release tag, claims table checked by hand |
| Too thin a plugin wall at launch | the frame shows the real few, large; no invented modules |

# Launch film

Pre-production for the public launch video. Nothing is rendered yet.

| File | What it is |
|---|---|
| [STORYBOARD.md](STORYBOARD.md) | The 37.5 second film, frame by frame, in beats, with the copy deck and the claims to verify |
| [MUSIC.md](MUSIC.md) | Six licensed, human-made track candidates, the license terms, the risks, and what to do for a commercial hit |
| [PRODUCTION.md](PRODUCTION.md) | Captures needed from the real app, what animates, the render pipeline on GitHub Actions, the review gates |
| [style-frames/](style-frames/contact-sheet.png) | Three looks, three stills each, rendered by `style-frames/render.mjs` from `style-frames/frames.html` |
| `cues.json`, `tools/cues.mjs` | The beat plan and the script that turns it into seconds and frames for any BPM |
| `fonts/` | Geist and Geist Mono, SIL OFL |

Re-render the style frames (silent, headless, no window):

```
npm i --no-save playwright && npx playwright install chromium
node docs/launch/style-frames/render.mjs
```

Cue sheet for a tempo: `node docs/launch/tools/cues.mjs --bpm 128 --md`.

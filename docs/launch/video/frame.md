---
name: Panoply launch, midnight to blue
description: Cold open in Midnight (dark, claustrophobic), then from the drop on a calm saturated brand blue. Geist, real app renders, restraint.
canvas: { w: 1920, h: 1080, fps: 60 }
colors:
  midnight: "#0c0c0f"
  app-window: "#1c1c1f"
  app-sidebar: "#161619"
  ink: "#ececf1"
  ink-2: "#a9a9b3"
  blue: "#2d6cdf"
  blue-deep: "#1f4fb0"
  accent: "#0a84ff"
  paper: "#f3f4f6"
  white: "#ffffff"
  warn: "#ff6961"
  good: "#30d158"
  cat-pdf: "#ff453a"
  cat-image: "#bf5af2"
  cat-video: "#ff7828"
  cat-id: "#ff9f0a"
  cat-files: "#1a84f3"
typography:
  display: { family: "Geist", weight: 700, tracking: "-0.04em", leading: 0.98, sizes: [128, 148, 168, 190] }
  body: { family: "Geist", weight: 500, tracking: "-0.015em", sizes: [44, 56] }
  label: { family: "Geist Mono", weight: 500, sizes: [30, 36] }
motion:
  enter-ease: "expo.out (cubic-bezier(0.16, 1, 0.3, 1))"
  exit-ease: "power3.in"
  move-ms: [360, 700]
  forbidden: ["shockwave rings", "screen shake", "bounce or back/elastic overshoot", "burst rays", "white flash frames", "game-like sound"]
---

# Panoply launch, design notes

One argument: the pain is a pile of tabs, the answer is one keystroke, the moat is that anyone can add a tool.
Frame 1 lives in Midnight. The drop (beat 16) swaps the ground to brand blue and it stays blue to the end card, except
frame 7 where each phrase may change ground by a straight clip-path wipe on a hit.

Read `docs/launch/STORYBOARD.md` (what each frame shows) and `docs/launch/style-frames/frames.html` (the look, rendered
as stills: option A for frame 1, option C from the drop on) before building. The stills are the target.

Do: real app renders as the product, rebuilt components from real tokens and real module names, big type (display 128 to 190 px),
generous space, one idea per card, slow continuous camera drift (1.5 to 6 percent over a scene), things that glide in and settle on
the beat, soft baked shadows, a 12 to 16 percent grain plate on flat grounds.

Don't: shockwaves, shakes, bounces, overshoot, burst rays, flashes, glows that pulse, gradient fills across a whole dark frame
(banding), live `filter: blur()` or `backdrop-filter` on large layers, WebGL, anything unseeded or time-based.

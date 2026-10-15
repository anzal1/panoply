# Teaser, 15 seconds

Posts a day or two before launch. Same film, same look (Midnight cold open, brand blue from the drop), cut hard to the
runner-up track and built around its biggest hit. A 16:9 cut (`teaser/`) and a vertical 9:16 cut (`teaser-vertical/`) for
Reels, Shorts and the X vertical feed. Nothing here is rendered yet. It is a silent picture, the track is added locally.

Track: "Adrenaline, Hyperpop" by JayStacksBeats, Pixabay Content License (proof in `music-proof/`).

## The track, measured (not the analyzer's rounded number)

The beat analyzer reported 143.6 BPM. It quantizes beats to 23 ms frames and that drifts. A finer fit of the track's onsets gives
**146.0 BPM** (error against 146.0 is half the error against 143.6), so the beat is 0.41096 s and 36 beats are 14.795 s.

| | |
|---|---|
| Biggest hit of the whole track | the drop at track second 54.266 (+25.7 dB over the previous 1.5 s). The next biggest is +15.7 dB at 117.6 s |
| Edit | start at track second **50.978**, length **15.0 s** (36 beats plus 0.205 s), fade the last 0.6 s. The drop then lands on **beat 8** (3.288 s) |
| Measured hits in the edit (beat, strength) | 8 (.88, the drop), 9 (.76), 11 (.71), 12 (.85), **15 (1.00, the strongest)**, 16.5 (.58), then only small ones: 22, 30, 32.5 (.33), 34 (.37) |
| Measured stops | **beats 4.39 to 7.92, a 1.45 s hard stop right before the drop**, and a short one at 8.54 to 8.93 |

Data: `music-analysis/hits-adrenaline-teaser.json`. Cut file (outside git): `panoply-video-audio.noindex/cuts/adrenaline-teaser.wav`.

## Beats

Beat `b` is at `b * 60/146` seconds. Everything lands on the measured hits and stops above.

| Beats | Seconds | Picture | Text |
|---|---|---|---|
| 0 to 4.4 | 0.0 to 1.8 | The upload chaos, fast. A window per half beat. The four words land on beats 0, 1, 2, 3. | Upload. Wait. Sign up. Pay. |
| 4.4 to 8.0 | 1.8 to 3.3 | STOP. The wall freezes, the scrim deepens, one line glides in during the silence and holds. | Your file just left your computer. |
| 8.0 (HIT .88) | 3.29 | THE DROP. The ground wipes to brand blue, the keycaps are down, the real bar opens. | One hotkey. |
| 9.0 (HIT .76) | 3.70 | A file chip lands on the bar. | |
| 9.25 to 12.0 | 3.8 to 4.9 | "under 50 kb" typed on 16ths from the real captured frames. | |
| 12.0 (HIT .85) | 4.93 | Return. The real result card lands, 35 percent counts up. | |
| 12.0 to 15.0 | 4.9 to 6.2 | The bar shrinks away, tool cards glide in, one per half beat. | |
| 15.0 (HIT 1.00, the strongest) | 6.16 | All remaining cards land together in one ripple, the counter locks on the real tool count. | 31 tools. |
| 16.5 (HIT .58) | 6.78 | The categories light in a row. | |
| 18 to 24 | 7.4 to 9.9 | The fan mark, three file chips draw into a folder, the folder slides into Modules, the badge counts up. | Anyone can add a tool. |
| 24 to 30 | 9.9 to 12.3 | The wall of community slots fills. | Built by the people. |
| 30 to 32.5 | 12.3 to 13.4 | Three phrases on a beat each, a clip-path wipe between grounds. | Free. Open source. On your device. |
| 32.5 (HIT .33) | 13.36 | End card. The fan opens, the name settles. | Panoply. Coming {DATE}. |
| 34 (HIT .37) | 13.97 | The url. | github.com/anzal1/panoply |
| 34 to 36.5 | 13.97 to 15.0 | Hold. The track fades out over the last 0.6 s. | |

`{DATE}` is a project variable `launchDate`, empty until the date is set. When empty the end card reads "Coming soon."

## Vertical cut (9:16, 1080 x 1920)

Same beats, re-composed, not cropped: the headline sits in the top third (display 150 to 190 px, left aligned in a 120 px gutter), the product plate
in the middle third at full width minus gutters, tiles in a 2 column grid, the end card centred. Safe zones: nothing important in the top 200 px or the bottom 380 px
(app UI and captions overlay there on Reels and Shorts).

## Rules

Same as the main film (`STORYBOARD.md`): no shockwave, shake, bounce, flash, game SFX, competitor name, em dash. Every cut on a measured hit or stop.

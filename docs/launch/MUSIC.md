# Launch film, music

Brief (updated 2026-10-15): none of the first shortlist excited the owner. The target is the sound of "Hard" by
SOPHIE, which is hyperpop and bubblegum bass: plastic and metallic percussion with sharp transients, huge distorted
sub hits, sparse glossy arrangements with sudden silences and hard stops, chopped vocal stabs, playful but aggressive.
That track is copyrighted and was never downloaded or used. This is a search for an existing, commercially licensed
track in that style, picked by measurement.

## Result

**Pick: "Shinji, Hyperpop Type Beat" by JayStacksBeats.** Pixabay Content License. 2:20 long.
https://pixabay.com/music/trap-shinji-hyperpop-type-beat-492110/

**Runner-up: "Adrenaline, Hyperpop" by JayStacksBeats.** Same license, 3:09.
https://pixabay.com/music/upbeat-adrenaline-hyperpop-467041/

Third: "Solo Levelling, Hyperpop Type Beat", same artist, 2:05.
https://pixabay.com/music/trap-solo-levelling-hyperpop-type-beat-492107/

Nobody has listened to these, least of all me. Every claim below is a measurement of the audio file or a line from the
track page. A tag or a model score is not an ear. Please play the first 40 seconds of the edit (below) before the film
is built on it. The plan is built to survive a swap: the whole film is in beats, and `tools/cues.mjs` re-times it.

### Why Shinji, in numbers

48 tracks were measured: the six from the first shortlist, 23 more Pixabay hyperpop, glitch and experimental tracks, and 19
Free Music Archive tracks under CC BY or CC0. Full table: [music-analysis/results.md](music-analysis/results.md).

| | Shinji (pick) | Adrenaline (runner-up) | Solo Levelling |
|---|---|---|---|
| Score (0 to 100) | 72, rank 2 of 48 | 69, rank 4 | 67, rank 6 |
| BPM, fine fit of the onsets (the analyzer's own number was 129.2, 143.6 and 129.2, rounded by its 23 ms frames) | 130.0 | 146.0 | 130.0 |
| Biggest hit | +27.7 dB jump, from silence | +25.7 dB jump | +14.6 dB jump |
| Silent gaps in the 37 s edit | 6 (analyzer), 4 over 120 ms (own measure) | 6 and 5 | 5 and 2 |
| Sub-bass share (below 120 Hz) | 53 percent | 71 percent | 64 percent |
| Spectral flux sharpness (p99 over median) | 14 | 19 | 21 |
| Loudness | -15.5 LUFS (quiet, will be normalised) | -15.0 LUFS | -13.9 LUFS |
| CLAP, hyperpop-family labels | 0.78 | 0.32 | 0.38 |
| CLAP, corporate, chill, generic, cinematic | 0.04 | 0.08 | 0.20 |
| 80 beats at its own tempo | 36.92 s (38.31 s with the ring-out) | 32.9 s | 36.92 s |

Plain words, from the top CLAP labels and the measurements. Shinji reads as hyperpop first, then playful aggressive pop,
with almost nothing that sounds corporate or lo-fi. It runs a steady 130 BPM, and in the edit the energy drops to nothing
several times (the analyzer finds silences at 6.1, 15.1, 20.1, 26.1, 29.1 and 37.1 s) before hard hits. That sparse, sudden
stop and start is the quality that matches "Hard". It has less sub-bass than the others, which is its weakest number.
Adrenaline is the heavier option: 71 percent sub-bass, a +25.7 dB hit, longer silences, but the hyperpop read is weaker (it became the 15 s teaser, see `TEASER.md`). Solo Levelling is a close cousin of Shinji with a smaller hit.

How the ranking was done, so it can be argued with. The banger score is a percentile blend: hyperpop CLAP score minus half
the negative score (28 percent), drop size (15), sub-bass share (10), silent gaps plus level contrast in the edit (17),
transient sharpness from spectral flux (18), onset density after the drop (7), and a 120 to 165 BPM steady grid (5). By the
formula alone the top track is "Sub Pressure, Experimental Bass" (72.8, against Shinji's 71.8, a tie). It was not picked:
it has no silences or stops, so it is a wall of bass and not the sparse, stop-start shape the brief asks for, and its
hyperpop CLAP score is 0.16. The pick is Shinji on the brief, not on the sum.

What the measurement cannot see: timbre. Whether the percussion is rubbery and metallic, whether there are chopped vocal
stabs, whether it is playful. CLAP's zero-shot labels are a rough proxy, and its "hyperpop" label leans toward any bright
synthetic track (it gave a sped-up Pixabay pop remix 0.54). Only an ear settles it.

### The exact edit (corrected 2026-10-15 after a finer tempo measurement)

The beat analyzer rounds beats to 23 ms frames, which made Shinji read 129.2 BPM and Adrenaline 143.6. A fit of the tracks' onsets at 1.4 ms
resolution gives **Shinji 130.0 BPM** (best fit 129.94, error 0.0724 against 0.1082 at 129.2) and **Adrenaline 146.0 BPM** (0.0483 against 0.1005 at 143.6).
At 129.2 the cuts would have drifted 230 ms by the end of the film. All cue sheets use the corrected tempo. The drops are the strongest onsets after a silence:
Shinji at track second 33.269, Adrenaline at 54.266.

| | Shinji, main film | Adrenaline, teaser |
|---|---|---|
| Tempo | 130.0 BPM, beat 0.46154 s | 146.0 BPM, beat 0.41096 s |
| Start offset in the track | **25.884 s** (the drop on beat 16, 33.269 minus 16 beats) | **50.978 s** (the drop on beat 8) |
| Length of cut | 38.308 s (80 beats plus a 3 beat ring-out) | 15.000 s |
| Fade out | last 1.4 s | last 0.6 s |
| Measured hits (beat, strength) | 8 (.63), 16 (.64, the drop), 33 (.79), 46.5 (.53), 68 (.41), 72 (.70), 74 (1.00), 76 (.66) | 8 (.88, the drop), 9 (.76), 11 (.71), 12 (.85), 15 (1.00), 16.5 (.58) |
| Measured stops (beats) | 13.32 to 15.93, 32.61 to 32.93, 73.54 to 73.93, 75.54 to 75.92 | 4.39 to 7.92, 8.54 to 8.93 |

```
ffmpeg -ss 25.884 -t 38.308 -i shinji.mp3 -af "afade=t=out:st=36.9:d=1.4,loudnorm=I=-14:TP=-1.5" -ar 48000 shinji-main.wav
ffmpeg -ss 50.978 -t 15.0  -i adrenaline.mp3 -af "afade=t=out:st=14.4:d=0.6,loudnorm=I=-14:TP=-1.5" -ar 48000 adrenaline-teaser.wav
```

(The real cuts were made two-pass linear: Shinji measured -15.3 LUFS in, Adrenaline -13.1 LUFS in, both written at -14 LUFS, true peak under -1.5 dB.) The cuts sit outside
git in `panoply-video-audio.noindex/cuts/`. The film is 36.92 s at 130 BPM, 38.31 s with the ring-out. The cue sheet is
[cues-shinji-130bpm.md](cues-shinji-130bpm.md) and the raw hit and stop maps are `music-analysis/hits-shinji.json` and
`music-analysis/hits-adrenaline-teaser.json`.

## License evidence

All three picks: Pixabay Content License. Evidence fetched 2026-10-15.

- License summary page, https://pixabay.com/service/license-summary/. The line that matters: "Use Content without having to
  attribute the author". Prohibited: selling or distributing the track on a standalone basis, misleading use, commercial use of
  recognisable trademarks in the content, and using the content as a trademark. It does not name X, and it puts no limit on
  promotional use inside a video.
- Each track page says "Free for use under the Pixabay Content License", shows no "Content ID Registered" badge and no "AI
  generated" label (read from each page's text on 2026-10-15; none of the three pages contains "Content ID" or "AI generated").
- Human-artist evidence: JayStacksBeats has a Pixabay profile with 148 followers, a sales site (JayStacksBeats.com, "Purchase
  Beats Here") and a Spotify artist link
  (https://open.spotify.com/artist/4tOdQubkpjym2wQOG3btox). That is likely human, not proven. Pixabay says a missing AI label does not
  prove a human made a track, and a producer with a store and a Spotify page can still use AI tools.
- Save dated screenshots of the three pages into `docs/launch/music-proof/` when the track is final. They are what you send
  if anyone asks.

### The Free Music Archive and CC BY option

FMA tracks carry an explicit CC license line and an "AI generated?" field on the track page, which is better evidence than
Pixabay gives. None of the tracks found were as close to the target as the JayStacksBeats ones, so none is in the top
three. The best of them, for the record:

| Track | Artist | License evidence | Note |
|---|---|---|---|
| Rushing Hearts | Elijah_K | "licensed under a Attribution 4.0 International License", https://creativecommons.org/licenses/by/4.0, AI generated: No. Page: https://freemusicarchive.org/music/elijah-k/single/rushing-hearts/ | CLAP hyperpop 0.87, +17.3 dB hit at 13.7 s, but only 69 s long and the tempo reads as 92, 140 or 184 BPM depending on the tool, so the cues cannot be mapped cleanly. Needs a credit line. |
| JOUHATSU (electronica, glitch, deep, moody) | Soularflair | CC BY 4.0. The artist adds a condition: "let me know" where the music is used. https://freemusicarchive.org/music/Soularflair/cc-by-free-to-use-for-anything-1/ | Dark glitch electronica, +19.3 dB hit at 9.7 s, 8 silent gaps, CLAP style 0.09 (reads as trap). A credit and an email to the artist. |
| EPHEMERA (In-Silico Remix) | Soularflair | CC BY 4.0, same condition | CLAP style 0.83, but the hit is only +8.1 dB and it is 2:38 of busy industrial electronica. |
| Astrology | 1000 Handz | CC BY 4.0, "Please credit '1000Handz.com' in the video description" | CLAP style 0.92 (hyperpop 0.73), 2:24, +6.9 dB hit at 10.1 s, no silences. Needs a credit. |
| Kitsune, Tengu | HoliznaCC0 | CC0 | No credit needed. Reads as hip hop, not hyperpop. |

Excluded on license: CODESCORN (CC BY-NC), Waveloom and SyncLab (CC BY-ND), Pumpupthemind (CC BY-NC-ND), and anything
ShareAlike, because a sync of audio to picture can count as an adaptation and share-alike would then pull the whole film
under it. CC BY needs a credit. On X there is no description field, so the credit goes on the end card or in the post.

Other sources tried:

| Source | Result |
|---|---|
| Jamendo | The site's free music is licensed non-commercially for the most part, and commercial use goes through a paid Jamendo Licensing plan. Its search is a script-driven page I could not enumerate. Not pursued. |
| Bandcamp and SoundCloud | No license filter exists, so finding CC BY or CC0 hyperpop there is a manual crawl that did not fit the time. A download is also only offered where the artist enables it. Leads welcome. |
| Pixabay | 10 queries (hyperpop, bubblegum bass, pc music, glitch pop, experimental bass, deconstructed club, metallic percussion, hyper pop, glitchcore, bubblegum pop electronic), 96 pages read. Many were labelled AI-generated and were dropped. |
| Free Music Archive | 12 genres (Glitch, Experimental Pop, Electropop, Synth Pop, Chiptune, IDM, Dubstep, Breakcore, Electro-Punk, Hip-Hop Beats, Drum n Bass, Club), 245 tracks read, 115 under CC BY or CC0 and not marked AI. |

Real hyperpop, in the PC Music sense, is not on these sites. What there is, is "hyperpop type beats", producer beats in
the genre's sound. They are the closest licensed thing found.

## The six from the first brief, measured

Superseded, and none in the top 5. For the record: Millenia ranked 10th (score 59, +8.8 dB hit at 7.1 s, no silent gaps), Don't Talk
15th, Energy Trap Bass 17th, Dance For Me, Darkness and BLACK BOX lower. They were chosen by tags, and they rank lower on the
sparse, stop-start qualities the new brief asks for.

## Risk, honestly

- **X.** No sign that X fingerprints audio like YouTube. It removes content after a valid copyright notice (secondary
  sources, X's own help page was not readable). For a properly licensed Pixabay track the realistic X risk is low, not zero.
- **YouTube and Instagram.** Pixabay says some contributors register tracks with YouTube's Content ID. A claim is not a strike
  and is cleared with "I have permission or a license", the track URL and the license summary. The three picks show no badge.
- **A "type beat" label.** These tracks are named after the style they imitate. Pixabay's license covers the recording. The
  music is the producer's own. Nothing about the track is a cover, but if any of them turns out to use a sample, the license
  would not cover that. Nobody has checked for samples.
- **Not checked.** JayStacksBeats sells beats on his own site and streams on Spotify, so copies may sit in other rights
  databases that Pixabay does not mark.

## If you want a specific commercial hit

Then you must license it, or accept the risk and add it yourself. To license a major-label song for a promo on X you
need a sync license from the publisher and a master license from the label, a negotiated paid deal. Without one, X can
mute or remove the post after a notice, and a notice against the account is a mark on it. If you accept that risk, do it
outside this repo: the film's picture is rendered silent anyway (see "Audio" in PRODUCTION.md), so you lay the track in an
editor and post it yourself. The film is cut in beats, so only the cue sheet needs the hit's real BPM and drop.

If you already pay for Artlist: only the Pro and Business license lists X (Twitter) and promoted videos; the Social license
does not. Epidemic Sound's Creator plan marks business work as not included. Check your tier before posting a product promo.

Never commit audio to this public repo, free tracks included. The Pixabay license bars distributing a track as a standalone
file, and a public repo is that. The audio lives in `personal/panoply-video-audio.noindex/`, outside every repo.
Only the analysis (`music-analysis/`) and the cue sheets are committed.

## Files

| File | What |
|---|---|
| `music-analysis/results.md` | Every track measured, ranked, with the CLAP read |
| `music-analysis/analysis.json` | All numbers: metrics, CLAP label probabilities, ranking, edits |
| `music-analysis/sources.json` | The page URL and license evidence of each track |
| `music-analysis/audiomap-*.json` | The beat analyzer's output for the three picks (events removed to keep it small) |
| `cues-shinji-130bpm.md` | The cue sheet at the corrected 130 BPM, with the track second of every cue |
| `music-analysis/hits-*.json` | Measured hits and stops of the two edits |

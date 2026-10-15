// Turns cues.json (beats) into seconds and frame numbers for a given tempo.
//   node docs/launch/tools/cues.mjs --bpm 128 [--offset 0] [--fps 60] [--md]
// --offset is the second in the track where video bar 1 starts (the cut of the intro, so the drop lands on beat 16).
// Use the BPM and downbeats reported by music-to-video's analyze-beatgrid.py, never an estimate by ear.
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

const arg = (k, d) => { const i = process.argv.indexOf('--' + k); return i > 0 ? process.argv[i + 1] : d; };
const bpm = Number(arg('bpm', 128)), offset = Number(arg('offset', 0)), fps = Number(arg('fps', 60));
const md = process.argv.includes('--md');
const here = dirname(fileURLToPath(import.meta.url));
const plan = JSON.parse(readFileSync(join(here, '..', 'cues.json'), 'utf8'));
const beat = 60 / bpm;
const sec = (b) => b * beat;
const total = sec(plan.bars * 4);
const f = (n) => n.toFixed(3);
const out = [];
out.push(`bpm ${bpm}, beat ${f(beat)} s, bar ${f(beat * 4)} s, video ${f(total)} s (${f(total + sec(plan.tail_beats))} s with the ring-out), track offset ${offset} s`);
for (const s of plan.scenes) {
  const a = sec(s.start), b = sec(s.start + s.bars * 4);
  out.push(md ? `\n### ${s.name}  (${f(a)} to ${f(b)} s, ${s.bars} bars)\n\n| cue | beat | film second | frame@${fps} | track second |\n|---|---|---|---|---|` : `\n${s.name}  ${f(a)} to ${f(b)} s`);
  for (const [name, bt] of s.cues) {
    const t = sec(bt);
    out.push(md ? `| ${name} | ${bt} | ${f(t)} | ${Math.round(t * fps)} | ${f(t + offset)} |` : `  ${String(bt).padStart(5)}  ${f(t)} s  f${Math.round(t * fps)}  track ${f(t + offset)} s  ${name}`);
  }
}
console.log(out.join('\n'));
if (process.argv.includes('--track-times')) console.log(`\ntrack seconds = video second + ${offset}`);

/**
 * Fuzzy scoring: subsequence match with bonuses for a prefix hit, word-boundary starts
 * and consecutive runs. Dynamic programming over (query char, name char), O(m*n), so the
 * reported highlight positions are the best alignment, not merely the leftmost one.
 */

const B_START = 16; // first character of the name
const B_WORD = 12; // first character after a space, dash, slash, dot
const B_CAMEL = 6;
const B_PREFIX = 14; // query starts exactly at the start of the name
const B_RUN = 9; // consecutive with the previous match
const GAP_OPEN = 3;
const GAP_EXT = 1;
const BASE = 10;
const NEG = -1e9;

export interface Match {
  score: number;
  pos: number[];
}

function boundary(name: string, lower: string, j: number): number {
  if (j === 0) return B_START;
  const p = lower.charCodeAt(j - 1);
  // space, - _ / . ( :
  if (p === 32 || p === 45 || p === 95 || p === 47 || p === 46 || p === 40 || p === 58) return B_WORD;
  const c = name.charCodeAt(j);
  const pc = name.charCodeAt(j - 1);
  if (c >= 65 && c <= 90 && pc >= 97 && pc <= 122) return B_CAMEL;
  return 0;
}

/** `q` must be lowercase with no spaces. */
export function fuzzy(q: string, name: string, lower: string): Match | null {
  const m = q.length;
  const n = name.length;
  if (m === 0) return { score: 0, pos: [] };
  if (m > n) return null;

  // Fast reject: plain subsequence test.
  let qi = 0;
  for (let j = 0; j < n && qi < m; j++) if (lower.charCodeAt(j) === q.charCodeAt(qi)) qi++;
  if (qi < m) return null;

  const dp: Float64Array[] = [];
  const from: Int16Array[] = [];
  for (let i = 0; i < m; i++) {
    dp.push(new Float64Array(n).fill(NEG));
    from.push(new Int16Array(n).fill(-1));
  }

  for (let i = 0; i < m; i++) {
    const qc = q.charCodeAt(i);
    // Running best of dp[i-1][k] + k*GAP_EXT for k <= j-2 (gap >= 1).
    let runBest = NEG;
    let runIdx = -1;
    for (let j = 0; j < n; j++) {
      if (i > 0 && j >= 2) {
        const k = j - 2;
        const v = dp[i - 1][k];
        if (v > NEG / 2 && v + k * GAP_EXT > runBest) {
          runBest = v + k * GAP_EXT;
          runIdx = k;
        }
      }
      if (lower.charCodeAt(j) !== qc) continue;
      const bonus = boundary(name, lower, j);
      let best: number;
      let prev = -1;
      if (i === 0) {
        best = BASE + bonus + (j === 0 ? B_PREFIX : 0);
      } else {
        best = NEG;
        // consecutive
        if (j >= 1 && dp[i - 1][j - 1] > NEG / 2) {
          const v = dp[i - 1][j - 1] + B_RUN;
          if (v > best) {
            best = v;
            prev = j - 1;
          }
        }
        // gapped: gap g = j-k-1 >= 1, penalty = GAP_OPEN + GAP_EXT*(g-1)
        //   dp[k] - GAP_OPEN - GAP_EXT*(j-k-2)  =  (dp[k] + GAP_EXT*k) - GAP_OPEN - GAP_EXT*(j-2)
        if (runIdx >= 0) {
          const g = runBest - GAP_OPEN - GAP_EXT * (j - 2);
          if (g > best) {
            best = g;
            prev = runIdx;
          }
        }
        if (best <= NEG / 2) continue;
        best += BASE + bonus;
      }
      dp[i][j] = best;
      from[i][j] = prev;
    }
  }

  let bestScore = NEG;
  let bestJ = -1;
  for (let j = 0; j < n; j++) {
    const v = dp[m - 1][j];
    if (v > bestScore) {
      bestScore = v;
      bestJ = j;
    }
  }
  if (bestJ < 0) return null;

  const pos = new Array<number>(m);
  let j = bestJ;
  for (let i = m - 1; i >= 0; i--) {
    pos[i] = j;
    j = from[i][j];
  }
  // Mild preference for tighter names.
  return { score: bestScore - n * 0.15, pos };
}

// Fuzzy scoring: subsequence match with bonuses for a prefix hit, word-boundary
// starts and consecutive runs (fzy style dynamic programming, so the best
// alignment wins instead of the first one found).

class FuzzyResult {
  const FuzzyResult(this.score, this.positions);
  final double score;
  final List<int> positions;
}

const double _scoreGap = -1.0;
const double _scoreGapLeading = -0.6;
const double _scoreConsecutive = 14.0;
const double _scoreBoundary = 16.0;
const double _scorePrefix = 24.0;
const double _scoreMatch = 10.0;
const double _neg = -1e9;

bool _isBoundary(String s, int i) {
  if (i == 0) return true;
  final p = s.codeUnitAt(i - 1);
  // space, dash, underscore, slash, dot
  return p == 0x20 || p == 0x2d || p == 0x5f || p == 0x2f || p == 0x2e;
}

/// [needle] must already be lower case with spaces removed.
FuzzyResult? fuzzyMatch(String needle, String haystack) {
  final n = needle.length;
  final h = haystack.length;
  if (n == 0) return const FuzzyResult(0, []);
  if (n > h) return null;
  final lower = haystack.toLowerCase();

  // Quick subsequence reject.
  var qi = 0;
  for (var i = 0; i < h && qi < n; i++) {
    if (lower.codeUnitAt(i) == needle.codeUnitAt(qi)) qi++;
  }
  if (qi < n) return null;

  final bonus = List<double>.filled(h, 0);
  for (var j = 0; j < h; j++) {
    if (_isBoundary(lower, j)) {
      bonus[j] = j == 0 ? _scorePrefix : _scoreBoundary;
    } else {
      final c = haystack.codeUnitAt(j);
      final p = haystack.codeUnitAt(j - 1);
      // camelCase hump
      if (c >= 0x41 && c <= 0x5a && p >= 0x61 && p <= 0x7a) bonus[j] = _scoreBoundary * 0.6;
    }
  }

  // d[i][j]: best score with needle[i] matched exactly at haystack[j].
  // m[i][j]: best score for needle[0..i] within haystack[0..j].
  final d = List.generate(n, (_) => List<double>.filled(h, _neg));
  final m = List.generate(n, (_) => List<double>.filled(h, _neg));
  final fromConsecutive = List.generate(n, (_) => List<bool>.filled(h, false));

  for (var i = 0; i < n; i++) {
    var prevBest = _neg;
    final nc = needle.codeUnitAt(i);
    for (var j = 0; j < h; j++) {
      var score = _neg;
      var consec = false;
      if (lower.codeUnitAt(j) == nc) {
        if (i == 0) {
          score = _scoreMatch + bonus[j] + j * _scoreGapLeading;
        } else if (j > 0) {
          final viaGap = m[i - 1][j - 1] == _neg ? _neg : m[i - 1][j - 1] + _scoreMatch + bonus[j];
          final viaRun = d[i - 1][j - 1] == _neg ? _neg : d[i - 1][j - 1] + _scoreMatch + _scoreConsecutive;
          if (viaRun >= viaGap) {
            score = viaRun;
            consec = true;
          } else {
            score = viaGap;
          }
        }
      }
      d[i][j] = score;
      fromConsecutive[i][j] = consec;
      prevBest = prevBest == _neg ? _neg : prevBest + _scoreGap;
      if (score > prevBest) prevBest = score;
      m[i][j] = prevBest;
    }
  }

  if (m[n - 1][h - 1] == _neg) return null;

  // Backtrack positions.
  final pos = List<int>.filled(n, 0);
  var j = h - 1;
  var matchRequired = false;
  for (var i = n - 1; i >= 0; i--) {
    for (; j >= 0; j--) {
      if (d[i][j] != _neg && (matchRequired || d[i][j] == m[i][j])) {
        matchRequired = i > 0 && fromConsecutive[i][j];
        pos[i] = j;
        j--;
        break;
      }
    }
  }
  // Slightly prefer shorter targets so exact names beat long ones.
  return FuzzyResult(m[n - 1][h - 1] - h * 0.05, pos);
}

/// Contiguous substring hit used for the secondary fields (subtitle, category).
double substringScore(String needle, String haystack) {
  final i = haystack.toLowerCase().indexOf(needle);
  if (i < 0) return -1;
  final atBoundary = _isBoundary(haystack.toLowerCase(), i);
  return 8.0 * needle.length + (atBoundary ? _scoreBoundary : 0) - i * 0.2;
}

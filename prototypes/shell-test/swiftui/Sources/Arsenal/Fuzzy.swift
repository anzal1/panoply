import Foundation

/// Pre-indexed search entry for one tool. Built once at load.
struct IndexEntry {
    let tool: Tool
    let order: Int
    let nameLower: [UInt32]
    let boundary: [Bool]
    let subtitleLower: String
    let categoryLower: String
}

enum Fuzzy {
    // Scoring constants. Tuned so that prefix > word-start run > consecutive run > scattered.
    static let base = 10
    static let consecutive = 12
    static let wordStart = 10
    static let prefix = 14
    static let gapPenalty = 2
    static let gapCap = 8
    static let leadPenalty = 1
    static let leadCap = 6

    static func scalars(lowercased s: String) -> [UInt32] {
        s.lowercased().unicodeScalars.map { $0.value }
    }

    static func boundaries(_ name: String) -> [Bool] {
        let u = Array(name.unicodeScalars)
        var out = [Bool](repeating: false, count: u.count)
        for j in 0..<u.count {
            if j == 0 { out[j] = true; continue }
            let p = u[j - 1], c = u[j]
            let pAlnum = CharacterSet.alphanumerics.contains(p)
            if !pAlnum { out[j] = true; continue }
            if CharacterSet.lowercaseLetters.contains(p), CharacterSet.uppercaseLetters.contains(c) { out[j] = true }
        }
        return out
    }

    /// Best-alignment subsequence match of `q` (lowercased scalars, no whitespace) in the name.
    /// Returns score and matched indices, or nil when q is not a subsequence.
    static func match(_ q: [UInt32], in e: IndexEntry) -> (score: Int, indices: [Int])? {
        let m = q.count, t = e.nameLower, n = t.count
        if m == 0 { return (0, []) }
        if m > n { return nil }
        let none = Int.min
        var dp = [Int](repeating: none, count: m * n)
        var par = [Int](repeating: -1, count: m * n)
        for j in 0..<n where t[j] == q[0] {
            var s = base
            if e.boundary[j] { s += wordStart }
            if j == 0 { s += prefix }
            s -= min(j * leadPenalty, leadCap)
            dp[j] = s
        }
        if m > 1 {
            for i in 1..<m {
                if i >= n { return nil }
                for j in i..<n where t[j] == q[i] {
                    var best = none, bk = -1
                    for k in (i - 1)..<j {
                        let prev = dp[(i - 1) * n + k]
                        if prev == none { continue }
                        let s = k == j - 1 ? prev + consecutive : prev - min((j - k - 1) * gapPenalty, gapCap)
                        if s > best { best = s; bk = k }
                    }
                    if best == none { continue }
                    var s = best + base
                    if e.boundary[j] { s += wordStart }
                    dp[i * n + j] = s
                    par[i * n + j] = bk
                }
            }
        }
        var bestEnd = -1, bestScore = none
        for j in 0..<n {
            let s = dp[(m - 1) * n + j]
            if s != none && s > bestScore { bestScore = s; bestEnd = j }
        }
        if bestEnd < 0 { return nil }
        var idx = [Int](repeating: 0, count: m)
        var j = bestEnd
        for i in stride(from: m - 1, through: 0, by: -1) {
            idx[i] = j
            if i > 0 { j = par[i * n + j] }
        }
        return (bestScore, idx)
    }
}

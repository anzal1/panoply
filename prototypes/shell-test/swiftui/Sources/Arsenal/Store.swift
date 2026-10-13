import SwiftUI
import AppKit
import Observation

struct Tool: Codable, Identifiable, Hashable, Sendable {
    let id: String
    let name: String
    let subtitle: String
    let category: String
    let sfSymbol: String
    let lucide: String
    let accepts: [String]
}

struct DroppedFile: Equatable {
    let url: URL
    var name: String { url.lastPathComponent }
    var ext: String {
        let e = url.pathExtension.lowercased()
        return e == "jpeg" ? "jpg" : e
    }
}

struct RowItem: Identifiable {
    let id: String
    let tool: Tool
    let name: AttributedString
    let ord: Int
}

enum ListItem: Identifiable {
    case header(String)
    case row(RowItem)
    var id: String {
        switch self {
        case .header(let t): return "h:" + t
        case .row(let r): return r.id
        }
    }
}

enum Layout {
    static let width: CGFloat = 720
    static let height: CGFloat = 460
    static let radius: CGFloat = 20
    static let searchRow: CGFloat = 56
    static let footer: CGFloat = 40
    static let listTop: CGFloat = 8
    static let header: CGFloat = 28
    static let row: CGFloat = 44
    static var viewport: CGFloat { height - searchRow - footer }
}

enum Data_ {
    static func locate() -> URL? {
        let fm = FileManager.default
        var c: [URL] = []
        if let e = ProcessInfo.processInfo.environment["ARSENAL_DATA"] { c.append(URL(fileURLWithPath: e)) }
        if let r = Bundle.main.url(forResource: "utilities", withExtension: "json") { c.append(r) }
        let exe = URL(fileURLWithPath: CommandLine.arguments[0]).resolvingSymlinksInPath()
        var d = exe.deletingLastPathComponent()
        for _ in 0..<6 { c.append(d.appendingPathComponent("shared/utilities.json")); d = d.deletingLastPathComponent() }
        c.append(URL(fileURLWithPath: "/Users/anzalhussainabidi/personal/arsenal/shell-test/shared/utilities.json"))
        return c.first { fm.fileExists(atPath: $0.path) }
    }

    static func load() -> [Tool] {
        guard let u = locate(), let d = try? Data(contentsOf: u),
              let t = try? JSONDecoder().decode([Tool].self, from: d) else { return [] }
        return t
    }
}

@MainActor @Observable
final class Store {
    let tools: [Tool]
    let categories: [String]
    let placeholder: String
    @ObservationIgnored private let index: [IndexEntry]

    private(set) var query = ""
    private(set) var file: DroppedFile?
    private(set) var items: [ListItem] = []
    private(set) var rows: [RowItem] = []
    private(set) var rowYs: [CGFloat] = []
    private(set) var rowAfterHeader: [Bool] = []
    private(set) var contentHeight: CGFloat = 0
    private(set) var selected = 0
    /// True while selection changes come from navigation (animated). False for typing (instant).
    private(set) var glide = false
    private(set) var listVersion = 0

    var flash = false
    var footerMessage: String?
    var dragExt: String?
    var dragging = false
    var actionsPressed = false
    var offscreen = false

    @ObservationIgnored var renderedVersion = 0
    @ObservationIgnored weak var field: NSTextField?
    @ObservationIgnored private var messageToken = 0
    @ObservationIgnored private var plain: [String: AttributedString] = [:]

    init(tools: [Tool] = Data_.load()) {
        self.tools = tools
        var cats: [String] = []
        for t in tools where !cats.contains(t.category) { cats.append(t.category) }
        categories = cats
        placeholder = "Search \(tools.count) tools or drop a file"
        index = tools.enumerated().map { i, t in
            IndexEntry(tool: t, order: i, nameLower: Fuzzy.scalars(lowercased: t.name),
                       boundary: Fuzzy.boundaries(t.name), subtitleLower: t.subtitle.lowercased(),
                       categoryLower: t.category.lowercased())
        }
        for t in tools { plain[t.id] = AttributedString(t.name) }
        rebuild()
    }

    var pillY: CGFloat { rowYs.isEmpty ? 0 : rowYs[min(selected, rowYs.count - 1)] }
    var selectedTool: Tool? { rows.indices.contains(selected) ? rows[selected].tool : nil }
    var trimmedQuery: String { query.trimmingCharacters(in: .whitespaces) }

    // MARK: State changes

    func setQuery(_ q: String) {
        guard q != query else { return }
        query = q
        rebuild()
    }

    func setFile(_ f: DroppedFile?) {
        file = f
        rebuild()
    }

    func reset() {
        query = ""
        file = nil
        footerMessage = nil
        flash = false
        dragExt = nil
        dragging = false
        field?.stringValue = ""
        rebuild()
    }

    func beginDrag(ext: String) { dragExt = ext; dragging = true }
    func endDrag() { dragging = false }

    func move(_ delta: Int) {
        guard !rows.isEmpty else { return }
        let n = min(max(selected + delta, 0), rows.count - 1)
        guard n != selected else { return }
        glide = true
        selected = n
    }

    func select(_ i: Int) {
        guard rows.indices.contains(i), i != selected else { return }
        glide = true
        selected = i
    }

    func run() {
        guard let t = selectedTool else { return }
        flash = true
        messageToken += 1
        let token = messageToken
        footerMessage = "Opened \(t.name)"
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 150_000_000)
            if self.messageToken == token { self.flash = false }
            try? await Task.sleep(nanoseconds: 1_050_000_000)
            if self.messageToken == token { self.footerMessage = nil }
        }
    }

    func noteRendered() { renderedVersion = listVersion }

    // MARK: Filtering

    private func rebuild() {
        glide = false
        listVersion += 1
        let ext = file?.ext
        let pool = index.filter { e in
            guard let ext else { return true }
            return e.tool.accepts.contains("*") || e.tool.accepts.contains(ext)
        }

        var items: [ListItem] = []
        var rows: [RowItem] = []
        var ys: [CGFloat] = []
        var after: [Bool] = []
        var y = Layout.listTop

        let qs = Fuzzy.scalars(lowercased: query).filter { !CharacterSet.whitespaces.contains(Unicode.Scalar($0)!) }
        let qtrim = query.lowercased().trimmingCharacters(in: .whitespaces)

        if qs.isEmpty {
            for cat in categories {
                let group = pool.filter { $0.tool.category == cat }
                if group.isEmpty { continue }
                items.append(.header(cat)); y += Layout.header
                for (k, e) in group.enumerated() {
                    let r = RowItem(id: e.tool.id, tool: e.tool, name: plain[e.tool.id] ?? AttributedString(e.tool.name), ord: rows.count)
                    items.append(.row(r)); rows.append(r); ys.append(y); after.append(k == 0)
                    y += Layout.row
                }
            }
        } else {
            struct Hit { let e: IndexEntry; let score: Int; let idx: [Int] }
            var hits: [Hit] = []
            for e in pool {
                if let m = Fuzzy.match(qs, in: e) {
                    hits.append(Hit(e: e, score: m.score * 4 - e.nameLower.count, idx: m.indices))
                } else if !qtrim.isEmpty {
                    // Weak secondary matches: contiguous text in the subtitle or the category.
                    if e.subtitleLower.contains(qtrim) {
                        hits.append(Hit(e: e, score: 10 - e.nameLower.count, idx: []))
                    } else if e.categoryLower.hasPrefix(qtrim) {
                        hits.append(Hit(e: e, score: 4 - e.nameLower.count, idx: []))
                    }
                }
            }
            hits.sort { a, b in a.score != b.score ? a.score > b.score : a.e.order < b.e.order }
            if !hits.isEmpty { items.append(.header("Best matches")); y += Layout.header }
            for (k, h) in hits.enumerated() {
                let r = RowItem(id: h.e.tool.id, tool: h.e.tool, name: highlighted(h.e.tool.name, h.idx), ord: rows.count)
                items.append(.row(r)); rows.append(r); ys.append(y); after.append(k == 0)
                y += Layout.row
            }
        }
        self.items = items
        self.rows = rows
        self.rowYs = ys
        self.rowAfterHeader = after
        self.contentHeight = y + Layout.listTop
        self.selected = 0
    }

    private func highlighted(_ name: String, _ idx: [Int]) -> AttributedString {
        var a = AttributedString(name)
        guard !idx.isEmpty else { return a }
        var i = 0
        while i < idx.count {
            var j = i
            while j + 1 < idx.count && idx[j + 1] == idx[j] + 1 { j += 1 }
            let s = a.index(a.startIndex, offsetByCharacters: idx[i])
            let e = a.index(a.startIndex, offsetByCharacters: idx[j] + 1)
            a[s..<e].font = .system(size: 13.5, weight: .semibold)
            i = j + 1
        }
        return a
    }

    func tint(for category: String) -> Color {
        switch category {
        case "PDF": return .red
        case "Image": return .purple
        case "Video": return .orange
        case "Audio": return .pink
        case "Files": return .blue
        case "Text": return .green
        case "Capture": return .teal
        default: return .gray
        }
    }
}

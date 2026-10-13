import SwiftUI
import AppKit

extension ShapeStyle where Self == Color {
    static var ink2: Color { Color.primary.opacity(0.6) }   // secondary text
    static var ink3: Color { Color.primary.opacity(0.4) }   // tertiary text
}

let glideSpring = Animation.spring(response: 0.22, dampingFraction: 1)

struct Hairline: View {
    var body: some View {
        Rectangle().fill(Color.primary.opacity(0.08)).frame(height: 0.5)
    }
}

struct CommandBarView: View {
    let store: Store
    var onEscapeEmpty: () -> Void = {}
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        VStack(spacing: 0) {
            SearchRow(store: store, onEscapeEmpty: onEscapeEmpty)
                .frame(height: Layout.searchRow)
                .overlay(alignment: .bottom) { Hairline() }
            ResultsArea(store: store)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            FooterView(store: store)
                .frame(height: Layout.footer)
                .overlay(alignment: .top) { Hairline() }
        }
        .frame(width: Layout.width, height: Layout.height)
        .background {
            if store.offscreen {
                // Glass does not render offscreen; stand in for it so contrast can be judged.
                RoundedRectangle(cornerRadius: Layout.radius, style: .continuous)
                    .fill(scheme == .dark ? Color(white: 0.17) : Color(white: 0.96))
            }
        }
        .overlay {
            // 0.5 pt inner hairline.
            RoundedRectangle(cornerRadius: Layout.radius, style: .continuous)
                .strokeBorder(scheme == .dark ? Color.white.opacity(0.12) : Color.black.opacity(0.08), lineWidth: 0.5)
                .allowsHitTesting(false)
        }
        .overlay {
            // Drag ring: 2 pt accent at 40%.
            RoundedRectangle(cornerRadius: Layout.radius, style: .continuous)
                .strokeBorder(Color.accentColor.opacity(0.4), lineWidth: 2)
                .opacity(store.dragging ? 1 : 0)
                .animation(.easeOut(duration: 0.15), value: store.dragging)
                .allowsHitTesting(false)
        }
    }
}

// MARK: Search row

struct SearchRow: View {
    let store: Store
    var onEscapeEmpty: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Image(systemName: "magnifyingglass")
                .font(.system(size: 18, weight: .regular))
                .foregroundStyle(.ink2)
                .frame(width: 18, height: 18)
                .padding(.leading, 20)
            Color.clear.frame(width: 12, height: 1)
            ZStack(alignment: .leading) {
                if store.query.isEmpty {
                    Text(store.placeholder)
                        .font(.system(size: 20))
                        .foregroundStyle(Color.ink3)
                        .lineLimit(1)
                        .allowsHitTesting(false)
                }
                if store.offscreen {
                    Text(store.query)
                        .font(.system(size: 20))
                        .lineLimit(1)
                        .frame(maxWidth: .infinity, alignment: .leading)
                } else {
                    // NSTextField text sits 2 pt inside its frame (line-fragment padding); pull the frame left
                    // so the glyph origin lands exactly at x = 50.
                    SearchField(store: store, onEscapeEmpty: onEscapeEmpty)
                        .frame(maxWidth: .infinity)
                        .padding(.leading, -2)
                }
            }
            if let f = store.file {
                FileChip(name: f.name) { store.setFile(nil) }
                    .padding(.leading, 12)
                    .padding(.trailing, 16)
                    .transition(.opacity)
            } else {
                Color.clear.frame(width: 20, height: 1)
            }
        }
        .animation(.easeOut(duration: 0.12), value: store.file)
    }
}

struct FileChip: View {
    let name: String
    let clear: () -> Void
    var body: some View {
        HStack(spacing: 6) {
            Text(name)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.primary)
                .lineLimit(1)
                .truncationMode(.middle)
            Button(action: clear) {
                Image(systemName: "xmark")
                    .font(.system(size: 8, weight: .bold))
                    .foregroundStyle(Color.ink2)
                    .frame(width: 14, height: 14)
                    .background(Circle().fill(Color.primary.opacity(0.08)))
                    .contentShape(Circle())
            }
            .buttonStyle(.plain)
        }
        .padding(.leading, 10)
        .padding(.trailing, 5)
        .frame(height: 24)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.primary.opacity(0.08)))
        .frame(maxWidth: 200)
    }
}

// MARK: Results

struct ResultsArea: View {
    let store: Store

    var body: some View {
        let _ = store.noteRendered()
        let version = store.listVersion
        ZStack {
            if store.rows.isEmpty {
                EmptyState(query: store.trimmedQuery)
            } else {
                ResultsList(store: store)
                    .id(store.offscreen ? 0 : 1)
                    .opacity(store.dragging ? 0.18 : 1)
                    .animation(.easeOut(duration: 0.15), value: store.dragging)
            }
            if store.dragging {
                Text(store.dragExt.map { $0.isEmpty ? "Drop to see tools for this file" : "Drop to see tools for .\($0)" } ?? "")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(Color.ink2)
                    .transition(.opacity)
            }
        }
        .animation(.easeOut(duration: 0.15), value: store.dragging)
        .clipped()
        .accessibilityHidden(version < 0)
    }
}

struct EmptyState: View {
    let query: String
    var body: some View {
        VStack(spacing: 6) {
            Text(query.isEmpty ? "No tools" : "No tools match \u{201C}\(query)\u{201D}")
                .font(.system(size: 13))
                .foregroundStyle(Color.ink2)
            Text("Try \u{201C}compress\u{201D} or drop a file")
                .font(.system(size: 12))
                .foregroundStyle(Color.ink3)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(.bottom, 8)
    }
}

struct ResultsList: View {
    let store: Store
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var position = ScrollPosition(y: 0)
    @State private var liveY: CGFloat = 0
    @State private var targetY: CGFloat = 0
    @State private var animating = false

    var body: some View {
        if store.offscreen {
            content(lazy: false)
                .frame(width: Layout.width, height: Layout.viewport, alignment: .top)
                .clipped()
        } else {
            ScrollView(.vertical) {
                content(lazy: true)
            }
            .scrollIndicators(.hidden)
            .scrollPosition($position)
            .onScrollGeometryChange(for: CGFloat.self, of: { $0.contentOffset.y }) { _, y in
                liveY = y
                if !animating { targetY = y }
            }
            .onScrollPhaseChange { _, phase in
                animating = phase == .animating
                if phase != .animating { targetY = liveY }
            }
            .onChange(of: store.selected) { _, new in
                guard store.glide else { return }
                reveal(new)
            }
            .onChange(of: store.listVersion) {
                targetY = 0
                position.scrollTo(y: 0)
            }
        }
    }

    @ViewBuilder
    private func content(lazy: Bool) -> some View {
        ZStack(alignment: .topLeading) {
            if !store.rows.isEmpty { SelectionPill(store: store, reduceMotion: reduceMotion) }
            if lazy {
                LazyVStack(spacing: 0) { rowsView }
                    .padding(.top, Layout.listTop)
                    .padding(.bottom, Layout.listTop)
            } else {
                VStack(spacing: 0) { rowsView }
                    .padding(.top, Layout.listTop)
                    .padding(.bottom, Layout.listTop)
            }
        }
        .frame(width: Layout.width, alignment: .topLeading)
    }

    @ViewBuilder
    private var rowsView: some View {
        let sel = store.selected
        ForEach(Array(store.items.enumerated()), id: \.element.id) { _, item in
            switch item {
            case .header(let title):
                Text(title)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(Color.ink3)
                    .padding(.leading, 20)
                    .padding(.bottom, 6)
                    .frame(width: Layout.width, height: Layout.header, alignment: .bottomLeading)
            case .row(let r):
                let ord = r.ord
                ToolRow(item: r, tint: store.tint(for: r.tool.category), selected: ord == sel, animate: store.glide)
                    .contentShape(Rectangle())
                    .onTapGesture { store.select(ord); store.run() }
            }
        }
    }

    /// Keep the selected row visible with a small margin, animating with the same critically damped spring.
    private func reveal(_ sel: Int) {
        guard store.rowYs.indices.contains(sel) else { return }
        let viewport = Layout.viewport
        let margin: CGFloat = 8
        let maxY = max(0, store.contentHeight - viewport)
        var top = store.rowYs[sel]
        if store.rowAfterHeader[sel] { top -= Layout.header }
        let bottom = store.rowYs[sel] + Layout.row
        var t = targetY
        if sel == 0 { t = 0 }
        else if top - margin < t { t = top - margin }
        else if bottom + margin > t + viewport { t = bottom + margin - viewport }
        t = min(max(t, 0), maxY)
        guard abs(t - targetY) > 0.5 else { return }
        targetY = t
        if reduceMotion {
            position.scrollTo(y: t)
        } else {
            withAnimation(glideSpring) { position.scrollTo(y: t) }
        }
    }
}

struct SelectionPill: View {
    let store: Store
    let reduceMotion: Bool
    var body: some View {
        let y = store.pillY
        RoundedRectangle(cornerRadius: 10, style: .continuous)
            .fill(Color.primary.opacity(store.flash ? 0.20 : 0.09))
            .animation(.easeOut(duration: 0.14), value: store.flash)
            .frame(width: Layout.width - 16, height: Layout.row)
            .id(reduceMotion ? Int(y) : 0)
            .transition(.opacity)
            .offset(x: 8, y: y)
            .animation(reduceMotion ? .easeOut(duration: 0.1) : (store.glide ? glideSpring : nil), value: y)
    }
}

struct ToolRow: View {
    let item: RowItem
    let tint: Color
    let selected: Bool
    let animate: Bool

    var body: some View {
        HStack(spacing: 0) {
            RoundedRectangle(cornerRadius: 7, style: .continuous)
                .fill(tint.opacity(0.16))
                .frame(width: 28, height: 28)
                .overlay {
                    Image(systemName: item.tool.sfSymbol)
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(tint)
                }
            Color.clear.frame(width: 12, height: 1)
            Text(item.name)
                .font(.system(size: 13.5, weight: .medium))
                .foregroundStyle(.primary)
                .lineLimit(1)
                .fixedSize()
            Color.clear.frame(width: 8, height: 1)
            Text(item.tool.subtitle)
                .font(.system(size: 12))
                .foregroundStyle(Color.ink2)
                .lineLimit(1)
                .truncationMode(.tail)
                .layoutPriority(-1)
            Spacer(minLength: 12)
            ZStack(alignment: .trailing) {
                if selected {
                    KeyCap(text: "\u{21B5}").transition(.opacity)
                } else {
                    Text(item.tool.category)
                        .font(.system(size: 11))
                        .foregroundStyle(Color.ink3)
                        .transition(.opacity)
                }
            }
            .animation(animate ? .easeOut(duration: 0.09) : nil, value: selected)
        }
        .padding(.horizontal, 20)
        .frame(width: Layout.width, height: Layout.row)
    }
}

struct KeyCap: View {
    let text: String
    var pressed = false
    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .medium))
            .monospacedDigit()
            .foregroundStyle(Color.ink2)
            .padding(.horizontal, text.count > 1 ? 5 : 0)
            .frame(minWidth: 18, minHeight: 18)
            .background(RoundedRectangle(cornerRadius: 5, style: .continuous).fill(Color.primary.opacity(pressed ? 0.16 : 0.08)))
    }
}

// MARK: Footer

struct FooterView: View {
    let store: Store
    var body: some View {
        HStack(spacing: 0) {
            RoundedRectangle(cornerRadius: 4.5, style: .continuous)
                .fill(LinearGradient(colors: [Color.primary.opacity(0.85), Color.primary.opacity(0.55)],
                                     startPoint: .top, endPoint: .bottom))
                .frame(width: 16, height: 16)
                .padding(.trailing, 8)
            ZStack(alignment: .leading) {
                Text(store.footerMessage ?? "Arsenal")
                    .id(store.footerMessage ?? "Arsenal")
                    .transition(.opacity)
            }
            .font(.system(size: 12))
            .foregroundStyle(Color.ink2)
            .lineLimit(1)
            .animation(.easeOut(duration: 0.12), value: store.footerMessage)
            Spacer(minLength: 16)
            HStack(spacing: 16) {
                HStack(spacing: 8) {
                    Text("Open").font(.system(size: 12)).foregroundStyle(Color.ink2)
                    KeyCap(text: "\u{21B5}")
                }
                HStack(spacing: 8) {
                    Text("Actions").font(.system(size: 12)).foregroundStyle(Color.ink2)
                    HStack(spacing: 4) {
                        KeyCap(text: "\u{2318}", pressed: store.actionsPressed)
                        KeyCap(text: "K", pressed: store.actionsPressed)
                    }
                    .animation(.easeOut(duration: 0.1), value: store.actionsPressed)
                }
            }
        }
        .padding(.horizontal, 20)
    }
}

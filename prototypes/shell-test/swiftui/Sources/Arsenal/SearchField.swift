import SwiftUI
import AppKit

final class QueryField: NSTextField {
    override var alignmentRectInsets: NSEdgeInsets { NSEdgeInsets() }
    override init(frame: NSRect) {
        super.init(frame: frame)
        isBordered = false
        isBezeled = false
        drawsBackground = false
        focusRingType = .none
        font = .systemFont(ofSize: 20, weight: .regular)
        textColor = .labelColor
        usesSingleLineMode = true
        cell?.isScrollable = true
        cell?.wraps = false
        lineBreakMode = .byTruncatingTail
        allowsEditingTextAttributes = false
    }
    required init?(coder: NSCoder) { fatalError() }

}

/// Field editor that never registers for drags, so file drops reach the panel's drop view
/// instead of being inserted into the text as a path.
final class FieldEditor: NSTextView {
    override func registerForDraggedTypes(_ newTypes: [NSPasteboard.PasteboardType]) {}
    override func updateDragTypeRegistration() { unregisterDraggedTypes() }
}

struct SearchField: NSViewRepresentable {
    let store: Store
    var onEscapeEmpty: () -> Void

    func makeNSView(context: Context) -> QueryField {
        let f = QueryField(frame: .zero)
        f.delegate = context.coordinator
        store.field = f
        return f
    }

    func updateNSView(_ f: QueryField, context: Context) {
        let q = store.query
        if f.stringValue != q { f.stringValue = q }
        context.coordinator.onEscapeEmpty = onEscapeEmpty
    }

    func sizeThatFits(_ proposal: ProposedViewSize, nsView: QueryField, context: Context) -> CGSize? {
        let w = proposal.width.flatMap { $0.isFinite ? $0 : nil } ?? 100
        return CGSize(width: w, height: 24)   // one 20 pt line; SwiftUI centers it in the 56 pt row
    }

    func makeCoordinator() -> Coordinator { Coordinator(store: store, onEscapeEmpty: onEscapeEmpty) }

    @MainActor
    final class Coordinator: NSObject, NSTextFieldDelegate {
        let store: Store
        var onEscapeEmpty: () -> Void
        init(store: Store, onEscapeEmpty: @escaping () -> Void) {
            self.store = store
            self.onEscapeEmpty = onEscapeEmpty
        }

        func controlTextDidChange(_ obj: Notification) {
            guard let f = obj.object as? NSTextField else { return }
            store.setQuery(f.stringValue)
        }

        func control(_ control: NSControl, textView: NSTextView, doCommandBy sel: Selector) -> Bool {
            switch sel {
            case #selector(NSResponder.moveUp(_:)):
                store.move(-1); return true
            case #selector(NSResponder.moveDown(_:)):
                store.move(1); return true
            case #selector(NSResponder.insertNewline(_:)):
                store.run(); return true
            case #selector(NSResponder.cancelOperation(_:)):
                if store.query.isEmpty { onEscapeEmpty() } else { store.setQuery(""); (control as? NSTextField)?.stringValue = "" }
                return true
            case #selector(NSResponder.insertTab(_:)), #selector(NSResponder.insertBacktab(_:)):
                return true
            default:
                return false
            }
        }
    }
}

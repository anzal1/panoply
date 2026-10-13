import AppKit
import SwiftUI

final class ArsenalPanel: NSPanel {
    override var canBecomeKey: Bool { true }
    override var canBecomeMain: Bool { false }
    private var editor: FieldEditor?

    override func fieldEditor(_ createFlag: Bool, for object: Any?) -> NSText? {
        if object is QueryField {
            if editor == nil && createFlag {
                let e = FieldEditor(frame: .zero)
                e.isFieldEditor = true
                editor = e
            }
            return editor
        }
        return super.fieldEditor(createFlag, for: object)
    }
}

/// Content view of the panel. Owns file-drag handling (AppKit-level so the text field cannot swallow drops).
final class DropContainerView: NSView {
    weak var store: Store?

    override init(frame: NSRect) {
        super.init(frame: frame)
        registerForDraggedTypes([.fileURL])
    }
    required init?(coder: NSCoder) { fatalError() }

    private func fileURL(_ info: NSDraggingInfo) -> URL? {
        let opts: [NSPasteboard.ReadingOptionKey: Any] = [.urlReadingFileURLsOnly: true]
        return (info.draggingPasteboard.readObjects(forClasses: [NSURL.self], options: opts) as? [URL])?.first
    }

    override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
        guard let u = fileURL(sender) else { return [] }
        MainActor.assumeIsolated {
            let f = DroppedFile(url: u)
            store?.beginDrag(ext: u.hasDirectoryPath ? "" : f.ext)
        }
        return .copy
    }
    override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { .copy }
    override func draggingExited(_ sender: NSDraggingInfo?) {
        MainActor.assumeIsolated { store?.endDrag() }
    }
    override func draggingEnded(_ sender: NSDraggingInfo) {
        MainActor.assumeIsolated { store?.endDrag() }
    }
    override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
        guard let u = fileURL(sender) else { return false }
        MainActor.assumeIsolated {
            store?.endDrag()
            store?.setFile(DroppedFile(url: u))
        }
        return true
    }
}

/// CADisplayLink wrapper. One link, paused when idle. Callbacks run on the main thread at the
/// first display-link tick after they are armed.
@MainActor
final class FrameClock: NSObject {
    private weak var view: NSView?
    private var link: CADisplayLink?
    private var oneShots: [(CADisplayLink) -> Void] = []
    var perFrame: ((CADisplayLink) -> Bool)?

    init(view: NSView) { self.view = view }

    /// (Re)create the link so it follows the screen the panel is currently on.
    func rebuild() {
        link?.invalidate()
        guard let view, view.window != nil else { link = nil; return }
        let l = view.displayLink(target: self, selector: #selector(tick(_:)))
        l.preferredFrameRateRange = CAFrameRateRange(minimum: 60, maximum: 120, preferred: 120)
        l.isPaused = true
        l.add(to: .main, forMode: .common)
        link = l
    }

    func once(_ cb: @escaping (CADisplayLink) -> Void) {
        if link == nil { rebuild() }
        oneShots.append(cb)
        link?.isPaused = false
    }

    func startPerFrame(_ cb: @escaping (CADisplayLink) -> Bool) {
        if link == nil { rebuild() }
        perFrame = cb
        link?.isPaused = false
    }

    /// Resolves with wall-clock ms at the first tick where `cond` holds.
    func frame(until cond: @escaping () -> Bool) async -> Double {
        await withCheckedContinuation { (cont: CheckedContinuation<Double, Never>) in
            self.arm(cond, cont)
        }
    }

    private func arm(_ cond: @escaping () -> Bool, _ cont: CheckedContinuation<Double, Never>) {
        once { [weak self] _ in
            let t = nowMs()
            if cond() { cont.resume(returning: t) } else { self?.arm(cond, cont) }
        }
    }

    @objc private func tick(_ l: CADisplayLink) {
        let cbs = oneShots
        oneShots = []
        for cb in cbs { cb(l) }
        if let pf = perFrame, !pf(l) { perFrame = nil }
        if oneShots.isEmpty && perFrame == nil { l.isPaused = true }
    }
}

@inline(__always) func nowMs() -> Double { Date().timeIntervalSince1970 * 1000 }

/// CSS-style cubic bezier easing.
struct Bezier {
    let x1, y1, x2, y2: Double
    func eval(_ u: Double) -> Double {
        if u <= 0 { return 0 }
        if u >= 1 { return 1 }
        func bx(_ t: Double) -> Double { 3 * (1 - t) * (1 - t) * t * x1 + 3 * (1 - t) * t * t * x2 + t * t * t }
        func by(_ t: Double) -> Double { 3 * (1 - t) * (1 - t) * t * y1 + 3 * (1 - t) * t * t * y2 + t * t * t }
        var lo = 0.0, hi = 1.0, t = u
        for _ in 0..<24 {
            t = (lo + hi) / 2
            if bx(t) < u { lo = t } else { hi = t }
        }
        return by(t)
    }
}

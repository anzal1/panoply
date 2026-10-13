import AppKit
import SwiftUI

@MainActor
final class AppController: NSObject, NSApplicationDelegate {
    static var shared: AppController!

    enum Phase { case hidden, showing, shown, hiding }
    private struct Anim {
        var showing: Bool
        var from: Double
        var duration: Double
        var curve: Bezier
        var reduce: Bool
        var start: CFTimeInterval?
    }

    let store = Store()
    let metrics = Metrics()
    private var panel: ArsenalPanel!
    private var motion: NSView!
    private var glass: NSView!
    private var hosting: NSHostingView<CommandBarView>!
    private var clock: FrameClock!
    private var server: ControlServer?
    private(set) var phase = Phase.hidden
    private var v: Double = 0                 // visibility 0...1
    private var anim: Anim?
    private var suppressAutoHide = false
    private var busy = false
    private weak var linkScreen: NSScreen?
    private var monitors: [Any] = []

    private static let showCurve = Bezier(x1: 0.2, y1: 0.9, x2: 0.25, y2: 1)
    private static let hideCurve = Bezier(x1: 0.42, y1: 0, x2: 1, y2: 1)
    private let size = NSSize(width: Layout.width, height: Layout.height)

    // MARK: Lifecycle

    func applicationDidFinishLaunching(_ n: Notification) {
        installMainMenu()
        buildPanel()
        installHotkeyAndMonitors()
        startServer()
        warmUpAndRecordLaunch()
    }

    private func installMainMenu() {
        // Accessory apps have no menu bar, but key equivalents for the field (copy, paste, select all) need a menu.
        let main = NSMenu()
        let appItem = NSMenuItem(); main.addItem(appItem)
        let appMenu = NSMenu(); appItem.submenu = appMenu
        appMenu.addItem(withTitle: "Quit Arsenal", action: #selector(NSApplication.terminate(_:)), keyEquivalent: "q")
        let editItem = NSMenuItem(); main.addItem(editItem)
        let edit = NSMenu(title: "Edit"); editItem.submenu = edit
        edit.addItem(withTitle: "Undo", action: Selector(("undo:")), keyEquivalent: "z")
        let redo = edit.addItem(withTitle: "Redo", action: Selector(("redo:")), keyEquivalent: "z")
        redo.keyEquivalentModifierMask = [.command, .shift]
        edit.addItem(.separator())
        edit.addItem(withTitle: "Cut", action: #selector(NSText.cut(_:)), keyEquivalent: "x")
        edit.addItem(withTitle: "Copy", action: #selector(NSText.copy(_:)), keyEquivalent: "c")
        edit.addItem(withTitle: "Paste", action: #selector(NSText.paste(_:)), keyEquivalent: "v")
        edit.addItem(withTitle: "Select All", action: #selector(NSText.selectAll(_:)), keyEquivalent: "a")
        NSApp.mainMenu = main
    }

    private func buildPanel() {
        let p = ArsenalPanel(contentRect: NSRect(origin: .zero, size: size),
                             styleMask: [.borderless, .nonactivatingPanel],
                             backing: .buffered, defer: false)
        p.isFloatingPanel = true
        p.level = .floating
        p.hidesOnDeactivate = false
        p.isOpaque = false
        p.backgroundColor = .clear
        p.hasShadow = true
        p.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary, .ignoresCycle]
        p.animationBehavior = .none
        p.isMovable = false
        p.isReleasedWhenClosed = false
        p.titleVisibility = .hidden

        let root = DropContainerView(frame: NSRect(origin: .zero, size: size))
        root.store = store
        root.wantsLayer = true

        let m = NSView(frame: root.bounds)
        m.wantsLayer = true
        m.autoresizingMask = [.width, .height]
        root.addSubview(m)

        let h = NSHostingView(rootView: CommandBarView(store: store, onEscapeEmpty: { [weak self] in self?.hide() }))
        h.sizingOptions = []
        h.safeAreaRegions = []
        h.frame = root.bounds
        h.autoresizingMask = [.width, .height]

        // Liquid Glass: NSGlassEffectView hosts the SwiftUI content.
        let g = NSGlassEffectView(frame: root.bounds)
        g.autoresizingMask = [.width, .height]
        g.cornerRadius = Layout.radius
        g.style = .regular
        g.contentView = h
        m.addSubview(g)

        p.contentView = root
        p.setContentSize(size)
        h.layoutSubtreeIfNeeded()

        panel = p; motion = m; glass = g; hosting = h
        clock = FrameClock(view: m)

        NotificationCenter.default.addObserver(forName: NSWindow.didResignKeyNotification, object: p, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.suppressAutoHide, self.phase == .shown || self.phase == .showing else { return }
                self.hide()
            }
        }
    }

    private func installHotkeyAndMonitors() {
        Hotkey.register { [weak self] t0 in
            MainActor.assumeIsolated { self?.toggle(t0: t0) }
        }
        if let m = NSEvent.addLocalMonitorForEvents(matching: .keyDown, handler: { [weak self] e in
            guard let self else { return e }
            let flags = e.modifierFlags.intersection(.deviceIndependentFlagsMask)
            if flags == .command, e.charactersIgnoringModifiers == "k", self.panel.isKeyWindow {
                MainActor.assumeIsolated { self.pressActions() }
                return nil
            }
            return e
        }) { monitors.append(m) }
        // Clicks outside the panel (other apps, desktop) hide it. Mouse events need no permission.
        if let m = NSEvent.addGlobalMonitorForEvents(matching: [.leftMouseDown, .rightMouseDown, .otherMouseDown], handler: { [weak self] _ in
            MainActor.assumeIsolated {
                guard let self, !self.suppressAutoHide, self.phase == .shown || self.phase == .showing else { return }
                self.hide()
            }
        }) { monitors.append(m) }
    }

    private func pressActions() {
        store.actionsPressed = true
        Task { @MainActor in
            try? await Task.sleep(nanoseconds: 140_000_000)
            self.store.actionsPressed = false
        }
    }

    private func startServer() {
        let s = ControlServer(port: 47811)
        s.route = { [weak self] path, t0, reply in
            guard let self else { reply(""); return }
            switch path {
            case "/toggle":
                DispatchQueue.main.async { MainActor.assumeIsolated { self.toggle(t0: t0) } }
                reply("{\"ok\":true}")
            case "/metrics":
                reply(self.metrics.json())
            case "/selftest":
                Task { @MainActor in reply(await self.runSelfTest()) }
            default:
                reply("")
            }
        }
        do { try s.start() } catch { FileHandle.standardError.write(Data("control server failed: \(error)\n".utf8)) }
        server = s
    }

    // MARK: Launch timestamp

    private var specFolder: URL {
        let fm = FileManager.default
        var u = Bundle.main.bundleURL
        for _ in 0..<2 { u = u.deletingLastPathComponent() }
        if fm.fileExists(atPath: u.appendingPathComponent("SPEC.md").path) { return u }
        return URL(fileURLWithPath: "/Users/anzalhussainabidi/personal/arsenal/shell-test")
    }

    private func recordLaunch(_ t: Double) {
        let dir = specFolder.appendingPathComponent("results", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let f = dir.appendingPathComponent("swiftui-launch.txt")
        let line = String(format: "%.3f\n", t)
        if let h = try? FileHandle(forWritingTo: f) {
            h.seekToEndOfFile(); h.write(Data(line.utf8)); try? h.close()
        } else {
            try? line.write(to: f, atomically: true, encoding: .utf8)
        }
    }

    /// Put the panel on screen fully transparent for one frame so the window server surface, the glass
    /// and the SwiftUI view tree are all warm, and record the first presented frame as the launch time.
    private func warmUpAndRecordLaunch() {
        position()
        panel.alphaValue = 0
        panel.ignoresMouseEvents = true
        panel.orderFrontRegardless()
        panel.displayIfNeeded()
        clock.once { [weak self] _ in
            let t = nowMs()
            guard let self else { return }
            self.recordLaunch(t)
            self.panel.orderOut(nil)
            self.panel.ignoresMouseEvents = false
        }
    }

    // MARK: Show and hide

    private func position() {
        let mouse = NSEvent.mouseLocation
        let screen = NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main ?? NSScreen.screens[0]
        let f = screen.frame
        let origin = NSPoint(x: (f.midX - size.width / 2).rounded(), y: (f.maxY - f.height * 0.22 - size.height).rounded())
        panel.setFrameOrigin(origin)
    }

    private func apply(_ vis: Double, showing: Bool, reduce: Bool) {
        v = vis
        panel.alphaValue = CGFloat(vis)
        guard let layer = motion.layer else { return }
        var t = CATransform3DIdentity
        if !reduce {
            let scale = showing ? 0.985 + 0.015 * vis : 0.99 + 0.01 * vis
            let dy = showing ? 6.0 * (1 - vis) : 0   // layer space is y-up: positive moves the bar up, it settles down
            let cx = size.width / 2, cy = size.height / 2
            t = CATransform3DTranslate(t, cx, cy + dy, 0)
            t = CATransform3DScale(t, scale, scale, 1)
            t = CATransform3DTranslate(t, -cx, -cy, 0)
        }
        CATransaction.begin()
        CATransaction.setDisableActions(true)
        layer.transform = t
        CATransaction.commit()
    }

    func toggle(t0: Double) {
        if phase == .shown || phase == .showing { hide() } else { show(t0: t0, record: true) }
    }

    func show(t0: Double, record: Bool) {
        guard phase == .hidden || phase == .hiding else { return }
        let reduce = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        let from = phase == .hiding ? v : 0
        if phase == .hidden {
            position()
            apply(0, showing: true, reduce: reduce)
        }
        phase = .showing
        panel.makeKeyAndOrderFront(nil)
        if let f = store.field { panel.makeFirstResponder(f) }
        anim = Anim(showing: true, from: from, duration: reduce ? 0.12 : 0.18,
                    curve: reduce ? Bezier(x1: 0.25, y1: 0.1, x2: 0.25, y2: 1) : Self.showCurve, reduce: reduce, start: nil)
        panel.displayIfNeeded()
        CATransaction.flush()
        if panel.screen !== linkScreen { clock.rebuild(); linkScreen = panel.screen }
        if record {
            clock.once { [metrics] _ in metrics.add(nowMs() - t0) }
        }
        clock.startPerFrame { [weak self] l in self?.step(l) ?? false }
    }

    func hide() {
        guard phase == .shown || phase == .showing else { return }
        let reduce = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
        phase = .hiding
        anim = Anim(showing: false, from: v, duration: reduce ? 0.1 : 0.12, curve: Self.hideCurve, reduce: reduce, start: nil)
        clock.startPerFrame { [weak self] l in self?.step(l) ?? false }
    }

    /// One animation frame, evaluated for the frame's target presentation time.
    private func step(_ l: CADisplayLink) -> Bool {
        guard var a = anim else { return false }
        if a.start == nil { a.start = l.timestamp; anim = a }
        let u = min(1, (l.targetTimestamp - a.start!) / a.duration)
        let e = a.curve.eval(u)
        let vis = a.showing ? a.from + (1 - a.from) * e : a.from * (1 - e)
        apply(vis, showing: a.showing, reduce: a.reduce)
        if u < 1 { return true }
        anim = nil
        if a.showing {
            phase = .shown
            panel.invalidateShadow()
        } else {
            panel.orderOut(nil)
            phase = .hidden
            store.reset()
        }
        return false
    }

    // MARK: Self test

    func runSelfTest() async -> String {
        guard !busy else { return "{\"error\":\"busy\"}" }
        busy = true
        suppressAutoHide = true
        defer { busy = false; suppressAutoHide = false }

        if phase == .hidden || phase == .hiding { show(t0: nowMs(), record: false) }
        _ = await clock.frame { [weak self] in self?.phase == .shown }
        try? await Task.sleep(nanoseconds: 200_000_000)
        store.setQuery("")
        store.setFile(nil)
        let warm = store.listVersion
        _ = await clock.frame { [weak self] in (self?.store.renderedVersion ?? 0) >= warm }

        var steps: [String] = []
        for phrase in ["compress pdf", "heic to jpg", "trim video"] {
            var cur = ""
            for ch in phrase { cur.append(ch); steps.append(cur) }
            while !cur.isEmpty { cur.removeLast(); steps.append(cur) }
        }

        var samples: [Double] = []
        for q in steps {
            try? await Task.sleep(nanoseconds: UInt64(Double.random(in: 12...30) * 1_000_000))
            let start = nowMs()
            store.setQuery(q)
            let want = store.listVersion
            let end = await clock.frame { [weak self] in (self?.store.renderedVersion ?? 0) >= want }
            samples.append(end - start)
        }
        let s = samples.sorted()
        func pct(_ p: Double) -> Double { s[max(0, Int((p / 100 * Double(s.count)).rounded(.up)) - 1)] }
        return String(format: "{\"n\":%d,\"p50\":%.3f,\"p95\":%.3f,\"max\":%.3f}", s.count, pct(50), pct(95), s.last ?? 0)
    }
}

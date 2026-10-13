import Carbon.HIToolbox
import Cocoa
import FlutterMacOS

// MARK: - Panel

/// Borderless, non-activating floating panel that can still take keyboard focus.
final class BarPanel: NSPanel {
  override var canBecomeKey: Bool { true }
  override var canBecomeMain: Bool { false }
}

// MARK: - Container with file drop

/// Root view of the window. Registered for file drags; forwards them to Dart.
final class DropContainerView: NSView {
  var onEnter: ((String, Int) -> Void)?
  var onExit: (() -> Void)?
  var onDrop: ((String, String) -> Void)?

  override init(frame: NSRect) {
    super.init(frame: frame)
    registerForDraggedTypes([.fileURL])
  }

  required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

  private func firstFile(_ info: NSDraggingInfo) -> URL? {
    let urls = info.draggingPasteboard.readObjects(
      forClasses: [NSURL.self], options: [.urlReadingFileURLsOnly: true]) as? [URL]
    return urls?.first
  }

  private func accentARGB() -> Int {
    let c = NSColor.controlAccentColor.usingColorSpace(.sRGB) ?? .systemBlue
    let a = Int(round(c.alphaComponent * 255)), r = Int(round(c.redComponent * 255))
    let g = Int(round(c.greenComponent * 255)), b = Int(round(c.blueComponent * 255))
    return (a << 24) | (r << 16) | (g << 8) | b
  }

  override func draggingEntered(_ sender: NSDraggingInfo) -> NSDragOperation {
    guard let url = firstFile(sender) else { return [] }
    onEnter?(url.pathExtension.lowercased(), accentARGB())
    return .copy
  }

  override func draggingUpdated(_ sender: NSDraggingInfo) -> NSDragOperation { .copy }

  override func draggingExited(_ sender: NSDraggingInfo?) { onExit?() }

  override func performDragOperation(_ sender: NSDraggingInfo) -> Bool {
    guard let url = firstFile(sender) else { return false }
    onDrop?(url.lastPathComponent, url.pathExtension.lowercased())
    return true
  }
}

/// Hosts the material and the Flutter view as a child controller (the same
/// arrangement macos_window_utils uses), so the engine gets its normal lifecycle.
final class ContainerViewController: NSViewController {
  let flutter: FlutterViewController
  let drop = DropContainerView(frame: NSRect(x: 0, y: 0, width: BarWindow.width, height: BarWindow.height))
  private(set) var materialName = "none"

  init(flutter: FlutterViewController) {
    self.flutter = flutter
    super.init(nibName: nil, bundle: nil)
  }

  required init?(coder: NSCoder) { fatalError("init(coder:) is not used") }

  override func loadView() {
    drop.autoresizingMask = [.width, .height]
    drop.wantsLayer = true
    view = drop
  }

  override func viewDidLoad() {
    super.viewDidLoad()
    addChild(flutter)
    let bounds = drop.bounds
    let radius = BarWindow.radius
    let forced = ProcessInfo.processInfo.environment["ARSENAL_MATERIAL"]

    if #available(macOS 26.0, *), forced != "vibrancy" {
      // Liquid Glass, rounded by the glass itself.
      let glass = NSGlassEffectView(frame: bounds)
      glass.autoresizingMask = [.width, .height]
      glass.cornerRadius = radius
      glass.style = .regular
      flutter.view.frame = glass.bounds
      flutter.view.autoresizingMask = [.width, .height]
      glass.contentView = flutter.view
      drop.addSubview(glass)
      materialName = "NSGlassEffectView(regular)"
    } else {
      // Fallback: behind-window vibrancy, material clipped by a rounded mask.
      let vev = NSVisualEffectView(frame: bounds)
      vev.autoresizingMask = [.width, .height]
      vev.material = .hudWindow
      vev.state = .active
      vev.blendingMode = .behindWindow
      vev.maskImage = Self.roundedMask(radius: radius)
      flutter.view.frame = vev.bounds
      flutter.view.autoresizingMask = [.width, .height]
      vev.addSubview(flutter.view)
      drop.addSubview(vev)
      materialName = "NSVisualEffectView(hudWindow)"
    }
  }

  static func roundedMask(radius: CGFloat) -> NSImage {
    let edge = radius * 2 + 1
    let image = NSImage(size: NSSize(width: edge, height: edge), flipped: false) { rect in
      NSColor.black.setFill()
      NSBezierPath(roundedRect: rect, xRadius: radius, yRadius: radius).fill()
      return true
    }
    image.capInsets = NSEdgeInsets(top: radius, left: radius, bottom: radius, right: radius)
    image.resizingMode = .stretch
    return image
  }
}

// MARK: - Window owner

private final class HotkeyBox {
  let fire: () -> Void
  init(_ fire: @escaping () -> Void) { self.fire = fire }
}

final class BarWindow {
  static let width: CGFloat = 720
  static let height: CGFloat = 460
  static let radius: CGFloat = 20
  /// Top edge of the bar sits at 22% of the screen height.
  static let topFraction: CGFloat = 0.22

  let panel: BarPanel
  let flutter: FlutterViewController
  let container: ContainerViewController
  let channel: FlutterMethodChannel

  private var generation = 0
  private var shownAt = Date.distantPast
  private var hotKeyRef: EventHotKeyRef?

  init() {
    flutter = FlutterViewController()
    flutter.backgroundColor = .clear  // let the native material show through
    container = ContainerViewController(flutter: flutter)

    panel = BarPanel(
      contentRect: NSRect(x: 0, y: 0, width: Self.width, height: Self.height),
      styleMask: [.borderless, .nonactivatingPanel],
      backing: .buffered,
      defer: false)
    panel.isOpaque = false
    panel.backgroundColor = .clear
    panel.hasShadow = true  // system window shadow follows the rounded material
    panel.level = .floating
    panel.isFloatingPanel = true
    panel.hidesOnDeactivate = false
    panel.isReleasedWhenClosed = false
    panel.isMovable = false
    panel.animationBehavior = .none
    panel.collectionBehavior = [.moveToActiveSpace, .fullScreenAuxiliary, .ignoresCycle]
    panel.contentViewController = container
    panel.setContentSize(NSSize(width: Self.width, height: Self.height))

    channel = FlutterMethodChannel(
      name: "arsenal/native", binaryMessenger: flutter.engine.binaryMessenger)
    channel.setMethodCallHandler { [weak self] call, result in
      guard let self else { return result(nil) }
      switch call.method {
      case "show": self.show(); result(nil)
      case "hide": self.hide(); result(nil)
      case "ready":
        // First Flutter frame is presented: the pre-created window goes away.
        if ProcessInfo.processInfo.environment["ARSENAL_DEBUG"] != nil {
          let c = self.container
          NSLog("arsenal: material=%@ panel=%@ drop=%@ flutter=%@ sub=%@ layerOpaque=%d windowOpaque=%d",
                c.materialName, NSStringFromRect(self.panel.frame), NSStringFromRect(c.drop.frame),
                NSStringFromRect(self.flutter.view.frame), c.drop.subviews.map { "\(type(of: $0))" }.joined(separator: ","),
                self.flutter.view.layer?.isOpaque ?? false ? 1 : 0, self.panel.isOpaque ? 1 : 0)
        }
        self.panel.orderOut(nil)
        self.panel.ignoresMouseEvents = false
        result(["reduceMotion": NSWorkspace.shared.accessibilityDisplayShouldReduceMotion,
                "material": self.container.materialName])
      default: result(FlutterMethodNotImplemented)
      }
    }

    container.drop.onEnter = { [weak self] ext, accent in
      self?.channel.invokeMethod("dragEnter", arguments: ["ext": ext, "accent": accent])
    }
    container.drop.onExit = { [weak self] in self?.channel.invokeMethod("dragExit", arguments: nil) }
    container.drop.onDrop = { [weak self] name, ext in
      self?.channel.invokeMethod("drop", arguments: ["name": name, "ext": ext])
    }

    NotificationCenter.default.addObserver(
      forName: NSWindow.didResignKeyNotification, object: panel, queue: .main
    ) { [weak self] _ in
      guard let self, Date().timeIntervalSince(self.shownAt) > 0.2 else { return }
      self.channel.invokeMethod("resignKey", arguments: nil)
    }
    NSWorkspace.shared.notificationCenter.addObserver(
      forName: NSWorkspace.accessibilityDisplayOptionsDidChangeNotification, object: nil, queue: .main
    ) { [weak self] _ in
      self?.channel.invokeMethod(
        "reduceMotion", arguments: NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)
    }

    registerHotkey()

    // Pre-create: order the window in invisibly so the engine renders its first
    // frame and everything (fonts, glyphs, layers) is warm. It is ordered out
    // once Dart reports that frame ("ready").
    positionOnActiveScreen()
    panel.alphaValue = 0
    panel.ignoresMouseEvents = true
    panel.orderFrontRegardless()
  }

  // MARK: Show / hide

  func show() {
    generation += 1
    let reduce = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    positionOnActiveScreen()
    shownAt = Date()
    panel.alphaValue = 0
    panel.ignoresMouseEvents = false
    panel.makeKeyAndOrderFront(nil)
    panel.makeFirstResponder(flutter.view)
    panel.invalidateShadow()
    NSAnimationContext.runAnimationGroup { ctx in
      ctx.duration = reduce ? 0.10 : 0.18
      ctx.timingFunction = CAMediaTimingFunction(controlPoints: 0.2, 0.9, 0.25, 1)
      ctx.allowsImplicitAnimation = true
      panel.animator().alphaValue = 1
    }
  }

  func hide() {
    generation += 1
    let mine = generation
    let reduce = NSWorkspace.shared.accessibilityDisplayShouldReduceMotion
    NSAnimationContext.runAnimationGroup(
      { ctx in
        ctx.duration = reduce ? 0.08 : 0.12
        ctx.timingFunction = CAMediaTimingFunction(name: .easeIn)
        ctx.allowsImplicitAnimation = true
        panel.animator().alphaValue = 0
      },
      completionHandler: { [weak self] in
        guard let self, self.generation == mine else { return }
        self.panel.orderOut(nil)
      })
  }

  private func positionOnActiveScreen() {
    let mouse = NSEvent.mouseLocation
    let screen = NSScreen.screens.first { NSMouseInRect(mouse, $0.frame, false) } ?? NSScreen.main
    guard let f = screen?.frame else { return }
    let x = (f.midX - Self.width / 2).rounded()
    let top = f.maxY - f.height * Self.topFraction
    panel.setFrame(
      NSRect(x: x, y: (top - Self.height).rounded(), width: Self.width, height: Self.height),
      display: false)
  }

  // MARK: Hotkey (Control+Option+3, Carbon: no Accessibility permission needed)

  private func registerHotkey() {
    var spec = EventTypeSpec(
      eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed))
    let box = Unmanaged.passRetained(HotkeyBox { [weak self] in
      self?.channel.invokeMethod("hotkey", arguments: nil)
    }).toOpaque()
    InstallEventHandler(
      GetApplicationEventTarget(),
      { _, _, userData -> OSStatus in
        guard let userData else { return noErr }
        Unmanaged<HotkeyBox>.fromOpaque(userData).takeUnretainedValue().fire()
        return noErr
      }, 1, &spec, box, nil)
    let status = RegisterEventHotKey(
      UInt32(kVK_ANSI_3), UInt32(controlKey | optionKey),
      EventHotKeyID(signature: 0x4152_534E, id: 1), GetApplicationEventTarget(), 0, &hotKeyRef)
    if ProcessInfo.processInfo.environment["ARSENAL_DEBUG"] != nil { NSLog("arsenal: hotkey status=%d", status) }
  }
}

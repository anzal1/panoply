import SwiftUI
import AppKit

/// Offscreen ImageRenderer snapshots at 3x. Glass and the real NSTextField do not render offscreen,
/// so the field is replaced by Text and the glass by a flat stand-in.
@MainActor
enum Renderer {
    static func run(outDir: String) {
        _ = NSApplication.shared
        let dir = URL(fileURLWithPath: outDir, isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)

        func shot(_ name: String, dark: Bool, _ setup: (Store) -> Void) {
            let s = Store()
            s.offscreen = true
            setup(s)
            let view = CommandBarView(store: s)
                .environment(\.colorScheme, dark ? .dark : .light)
            let r = ImageRenderer(content: view)
            r.scale = 3
            guard let img = r.nsImage, let tiff = img.tiffRepresentation, let rep = NSBitmapImageRep(data: tiff),
                  let png = rep.representation(using: .png, properties: [:]) else { print("render failed: \(name)"); return }
            try? png.write(to: dir.appendingPathComponent(name + ".png"))
            print("wrote \(name).png \(rep.pixelsWide)x\(rep.pixelsHigh)")
        }

        shot("render-default-dark", dark: true) { _ in }
        shot("render-default-light", dark: false) { s in s.select(2) }
        shot("render-query-dark", dark: true) { s in s.setQuery("compress") ; s.select(1) }
        shot("render-query-light", dark: false) { s in s.setQuery("pdf") }
        shot("render-empty-dark", dark: true) { s in s.setQuery("xyz") }
        shot("render-chip-light", dark: false) { s in s.setFile(DroppedFile(url: URL(fileURLWithPath: "/tmp/Quarterly-report-final-version-3-reviewed.pdf"))) }
        shot("render-drag-dark", dark: true) { s in s.beginDrag(ext: "pdf") }
        shot("render-run-dark", dark: true) { s in s.select(2); s.run() }
    }
}

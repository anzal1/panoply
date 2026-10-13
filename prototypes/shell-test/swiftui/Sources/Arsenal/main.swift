import AppKit

MainActor.assumeIsolated {
    if CommandLine.arguments.contains("--render") {
        Renderer.run(outDir: CommandLine.arguments.drop { $0 != "--render" }.dropFirst().first ?? ".")
        exit(0)
    }
    let app = NSApplication.shared
    let c = AppController()
    AppController.shared = c
    app.delegate = c
    app.setActivationPolicy(.accessory)
    app.run()
}

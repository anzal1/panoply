import Foundation
import Network

final class Metrics: @unchecked Sendable {
    private let lock = NSLock()
    private var showMs: [Double] = []
    func add(_ v: Double) { lock.lock(); showMs.append(v); lock.unlock() }
    func json() -> String {
        lock.lock(); let a = showMs; lock.unlock()
        return "{\"show_ms\":[" + a.map { String(format: "%.3f", $0) }.joined(separator: ",") + "]}"
    }
}

/// Tiny HTTP/1.1 server on 127.0.0.1. `t0` is captured at the top of the receive callback,
/// the first user-space instant the request is visible to the process.
final class ControlServer: @unchecked Sendable {
    typealias Reply = @Sendable (String) -> Void
    private let port: UInt16
    private let queue = DispatchQueue(label: "arsenal.control", qos: .userInteractive)
    private var listener: NWListener?
    var route: (@Sendable (_ path: String, _ t0: Double, _ reply: @escaping Reply) -> Void)?

    init(port: UInt16) { self.port = port }

    func start() throws {
        let p = NWParameters.tcp
        p.allowLocalEndpointReuse = true
        p.requiredLocalEndpoint = NWEndpoint.hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port)!)
        let l = try NWListener(using: p)
        l.newConnectionHandler = { [weak self] c in self?.accept(c) }
        l.start(queue: queue)
        listener = l
    }

    private func accept(_ c: NWConnection) {
        c.start(queue: queue)
        c.receive(minimumIncompleteLength: 1, maximumLength: 16384) { [weak self] data, _, _, _ in
            let t0 = nowMs()
            guard let self, let data, let text = String(data: data, encoding: .utf8) else { c.cancel(); return }
            let first = text.split(separator: "\r\n", maxSplits: 1, omittingEmptySubsequences: true).first ?? ""
            let parts = first.split(separator: " ")
            guard parts.count >= 2, parts[0] == "GET" else { self.send(c, 405, "{\"error\":\"method\"}"); return }
            let path = String(parts[1].split(separator: "?").first ?? "")
            guard let route = self.route else { self.send(c, 503, "{}"); return }
            route(path, t0) { [weak self] body in
                if body.isEmpty { self?.send(c, 404, "{\"error\":\"not found\"}") } else { self?.send(c, 200, body) }
            }
        }
    }

    private func send(_ c: NWConnection, _ code: Int, _ body: String) {
        let status = code == 200 ? "OK" : (code == 404 ? "Not Found" : "Error")
        let head = "HTTP/1.1 \(code) \(status)\r\nContent-Type: application/json\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n"
        c.send(content: Data((head + body).utf8), completion: .contentProcessed { _ in c.cancel() })
    }
}

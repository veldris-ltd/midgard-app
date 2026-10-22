import Foundation
import Network

/// A small static file server for the home page and the four consoles' builds, on
/// 127.0.0.1 only. One request per connection, GET and HEAD only: the consoles are
/// plain files and the WKWebView is the only client, so nothing more is needed.
///
///     /                      root/home/index.html
///     /home/…                root/home/…
///     /<product>/…           root/consoles/<product>/…   (midgard, heimdall, mimir, forge)
///
/// A directory serves its index.html. Traversal (`..`, NUL, backslash, also when
/// percent-encoded) is refused with 403 before the file system is touched.
public final class WebServer: @unchecked Sendable {
    public struct StartFailure: Error, CustomStringConvertible {
        public let description: String
    }

    static let products: Set<String> = ["midgard", "heimdall", "mimir", "forge"]
    static let maxHead = 16 * 1024
    static let chunk = 256 * 1024

    public let root: URL
    public let port: UInt16
    private let queue = DispatchQueue(label: "midgard.web")
    // Touched only on `queue`.
    private var listener: NWListener?
    private var connections: [ObjectIdentifier: NWConnection] = [:]

    public init(root: URL, port: UInt16) {
        self.root = root.standardizedFileURL
        self.port = port
    }

    /// Returns once the listener is ready, or throws if it cannot listen (port in use…).
    public func start() throws {
        guard let nwPort = NWEndpoint.Port(rawValue: port) else {
            throw StartFailure(description: "invalid port \(port)")
        }
        let params = NWParameters.tcp
        // Bind the loopback address itself, so other machines cannot connect at all.
        params.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: nwPort)
        params.allowLocalEndpointReuse = false
        let listener = try NWListener(using: params)

        let ready = DispatchSemaphore(value: 0)
        let outcome = Outcome()
        listener.stateUpdateHandler = { state in
            switch state {
            case .ready:
                outcome.set(nil); ready.signal()
            case .failed(let error), .waiting(let error):
                outcome.set(error); ready.signal()
            case .cancelled:
                outcome.set(StartFailure(description: "cancelled")); ready.signal()
            default:
                break
            }
        }
        listener.newConnectionHandler = { [weak self] connection in
            self?.accept(connection)
        }
        listener.start(queue: queue)
        if ready.wait(timeout: .now() + 5) == .timedOut {
            listener.cancel()
            throw StartFailure(description: "listener on 127.0.0.1:\(port) did not become ready")
        }
        if let error = outcome.error {
            listener.cancel()
            throw StartFailure(description: "cannot listen on 127.0.0.1:\(port): \(error)")
        }
        // Later state changes (a failure while running) are not the caller's to wait on.
        listener.stateUpdateHandler = nil
        queue.sync { self.listener = listener }
    }

    /// Stops listening and drops any connection still being served.
    public func stop() {
        queue.sync {
            listener?.cancel()
            listener = nil
            connections.values.forEach { $0.cancel() }
            connections.removeAll()
        }
    }

    // MARK: Connections (all on `queue`)

    private func accept(_ connection: NWConnection) {
        let id = ObjectIdentifier(connection)
        connections[id] = connection
        connection.stateUpdateHandler = { [weak self] state in
            switch state {
            case .failed, .cancelled: self?.connections[id] = nil
            default: break
            }
        }
        connection.start(queue: queue)
        readHead(connection, buffer: Data())
    }

    /// Reads until the blank line that ends the request head (the body, if any, is ignored).
    private func readHead(_ connection: NWConnection, buffer: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) { [weak self] data, _, isComplete, error in
            guard let self else { return }
            var buffer = buffer
            if let data { buffer.append(data) }
            if let end = buffer.range(of: Data("\r\n\r\n".utf8)), end.lowerBound <= Self.maxHead {
                self.respond(connection, head: buffer[..<end.lowerBound])
            } else if buffer.count > Self.maxHead {
                self.sendStatus(connection, 431, method: "GET")
            } else if isComplete || error != nil {
                connection.cancel()
            } else {
                self.readHead(connection, buffer: buffer)
            }
        }
    }

    private func respond(_ connection: NWConnection, head: Data) {
        let line = String(decoding: head.prefix { $0 != 0x0D }, as: UTF8.self)
        let parts = line.split(separator: " ", omittingEmptySubsequences: false)
        guard parts.count == 3, parts[2].hasPrefix("HTTP/") else {
            return sendStatus(connection, 400, method: "GET")
        }
        let method = String(parts[0])
        guard method == "GET" || method == "HEAD" else {
            return sendStatus(connection, 405, method: method, extra: ["Allow": "GET, HEAD"])
        }
        switch Self.resolve(target: String(parts[1]), root: root) {
        case .failure(let status):
            sendStatus(connection, status, method: method)
        case .success(let file):
            sendFile(connection, file, method: method)
        }
    }

    private func sendFile(_ connection: NWConnection, _ file: URL, method: String) {
        guard let handle = try? FileHandle(forReadingFrom: file),
              let size = (try? FileManager.default.attributesOfItem(atPath: file.path))?[.size] as? NSNumber
        else { return sendStatus(connection, 404, method: method) }
        let head = Self.head(200, type: Self.contentType(for: file), length: size.intValue)
        guard method == "GET" else {
            try? handle.close()
            return finish(connection, head)
        }
        connection.send(content: head, completion: .contentProcessed { [weak self] error in
            guard error == nil else { try? handle.close(); return connection.cancel() }
            self?.stream(connection, handle)
        })
    }

    /// Sends the file a chunk at a time, so a large map tile set never sits in memory.
    private func stream(_ connection: NWConnection, _ handle: FileHandle) {
        let data = (try? handle.read(upToCount: Self.chunk)) ?? nil
        guard let data, !data.isEmpty else {
            try? handle.close()
            return finish(connection, nil)
        }
        connection.send(content: data, completion: .contentProcessed { [weak self] error in
            guard error == nil else { try? handle.close(); return connection.cancel() }
            self?.stream(connection, handle)
        })
    }

    private func sendStatus(_ connection: NWConnection, _ status: Int, method: String,
                            extra: [String: String] = [:]) {
        let body = Data("\(status) \(Self.reason(status))\n".utf8)
        var data = Self.head(status, type: "text/plain; charset=utf-8", length: body.count, extra: extra)
        if method != "HEAD" { data.append(body) }
        finish(connection, data)
    }

    /// Sends the last bytes with FIN, then closes: one response per connection.
    private func finish(_ connection: NWConnection, _ data: Data?) {
        connection.send(content: data, contentContext: .finalMessage, isComplete: true,
                        completion: .contentProcessed { _ in connection.cancel() })
    }

    // MARK: Pure helpers

    /// Maps a request target to a file under `root`, or the status to answer with.
    static func resolve(target: String, root: URL) -> Resolution {
        let raw = target.prefix { $0 != "?" && $0 != "#" }
        guard raw.hasPrefix("/"), let path = raw.removingPercentEncoding else { return .failure(400) }
        if path.contains("\0") || path.contains("\\") { return .failure(403) }
        let segments = path.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
        if segments.contains("..") { return .failure(403) }

        var file: URL
        if segments.isEmpty {
            file = root.appendingPathComponent("home/index.html")
        } else if segments[0] == "home" {
            file = root.appendingPathComponent("home")
            segments.dropFirst().forEach { file.appendPathComponent($0) }
        } else if products.contains(segments[0]) {
            file = root.appendingPathComponent("consoles")
            segments.forEach { file.appendPathComponent($0) }
        } else {
            return .failure(404)
        }
        file = file.standardizedFileURL
        guard file.path.hasPrefix(root.path + "/") else { return .failure(403) }

        var isDirectory: ObjCBool = false
        if (!segments.isEmpty && path.hasSuffix("/")) || (FileManager.default.fileExists(atPath: file.path, isDirectory: &isDirectory)
                                   && isDirectory.boolValue) {
            file.appendPathComponent("index.html")
        }
        guard FileManager.default.fileExists(atPath: file.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else { return .failure(404) }
        return .success(file)
    }

    enum Resolution: Equatable {
        case success(URL)
        case failure(Int)
    }

    static func contentType(for file: URL) -> String {
        switch file.pathExtension.lowercased() {
        case "html", "htm": return "text/html; charset=utf-8"
        case "js", "mjs": return "text/javascript"
        case "css": return "text/css"
        case "json", "map": return "application/json"
        case "svg": return "image/svg+xml"
        case "png": return "image/png"
        case "jpg", "jpeg": return "image/jpeg"
        case "ttf": return "font/ttf"
        case "woff2": return "font/woff2"
        case "wasm": return "application/wasm"
        case "ico": return "image/x-icon"
        case "txt": return "text/plain; charset=utf-8"
        default: return "application/octet-stream"
        }
    }

    static func head(_ status: Int, type: String, length: Int, extra: [String: String] = [:]) -> Data {
        var lines = [
            "HTTP/1.1 \(status) \(reason(status))",
            "Content-Type: \(type)",
            "Content-Length: \(length)",
            "Cache-Control: no-store",
            "Connection: close",
            "X-Content-Type-Options: nosniff",
        ]
        lines += extra.sorted { $0.key < $1.key }.map { "\($0.key): \($0.value)" }
        return Data((lines.joined(separator: "\r\n") + "\r\n\r\n").utf8)
    }

    static func reason(_ status: Int) -> String {
        switch status {
        case 200: return "OK"
        case 400: return "Bad Request"
        case 403: return "Forbidden"
        case 404: return "Not Found"
        case 405: return "Method Not Allowed"
        case 431: return "Request Header Fields Too Large"
        default: return "Error"
        }
    }
}

/// The listener's first state, handed from its queue to the thread waiting in `start()`.
private final class Outcome: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: Error?
    private var isSet = false

    func set(_ error: Error?) {
        lock.lock(); defer { lock.unlock() }
        if !isSet { stored = error; isSet = true }
    }

    var error: Error? {
        lock.lock(); defer { lock.unlock() }
        return stored
    }
}

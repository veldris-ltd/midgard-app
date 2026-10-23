import Foundation
import Network

/// A small static file server for the home page and the four consoles' builds, on
/// 127.0.0.1 only. One request per connection, GET and HEAD only: the consoles are
/// plain files and the WKWebView is the only client, so nothing more is needed.
///
///     /                      root/home/index.html
///     /home/…                root/home/…
///     /<product>/…           root/consoles/<product>/…   (midgard, heimdall, mimir, forge)
///     /media/…               root/recording/media/…      (the recording's camera stills and clips)
///
/// A directory serves its index.html; without its trailing slash it is redirected first,
/// because the consoles load `./assets/…` relative to their own URL. Traversal (`..`,
/// NUL, backslash, also when percent-encoded) is refused with 403 before the file system
/// is touched, and a `Host` other than this server (DNS rebinding) gets 421. A single
/// `Range: bytes=…` is answered with 206 (WebKit plays a <video> only from a server that
/// does); anything else gets the whole file.
public final class WebServer: @unchecked Sendable {
    public struct StartFailure: Error, CustomStringConvertible {
        public let description: String
    }

    static let products: Set<String> = ["midgard", "heimdall", "mimir", "forge"]
    static let maxHead = 16 * 1024
    static let chunk = 256 * 1024

    public let root: URL
    /// The port asked for; 0 lets the system choose (see `boundPort`).
    public let port: UInt16
    let headTimeout: TimeInterval
    let stallTimeout: TimeInterval
    let onFailure: (@Sendable (Error) -> Void)?
    private let queue = DispatchQueue(label: "midgard.web")
    // Touched only on `queue`.
    private var listener: NWListener?
    private var listenerCancelled: DispatchSemaphore?
    private var bound: UInt16 = 0
    private var sessions: [ObjectIdentifier: Session] = [:]

    /// - Parameters:
    ///   - headTimeout: a connection that has not sent a whole request head by then is closed.
    ///   - stallTimeout: a response the client stops reading for this long is abandoned.
    ///   - onFailure: called (on the server's queue) if the listener fails after `start()`.
    public init(root: URL, port: UInt16 = 0, headTimeout: TimeInterval = 15,
                stallTimeout: TimeInterval = 30, onFailure: (@Sendable (Error) -> Void)? = nil) {
        self.root = root.standardizedFileURL
        self.port = port
        self.headTimeout = headTimeout
        self.stallTimeout = stallTimeout
        self.onFailure = onFailure
    }

    deinit {
        listener?.cancel()
    }

    /// The port actually listened on, once `start()` has returned (0 before).
    public var boundPort: UInt16 {
        dispatchPrecondition(condition: .notOnQueue(queue))
        return queue.sync { bound }
    }

    /// Returns once the listener is ready, or throws if it cannot listen (port in use…).
    public func start() throws {
        dispatchPrecondition(condition: .notOnQueue(queue))
        let params = NWParameters.tcp
        // Bind the loopback address itself, so other machines cannot connect at all.
        params.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: NWEndpoint.Port(rawValue: port) ?? .any)
        params.allowLocalEndpointReuse = false
        let listener = try NWListener(using: params)

        let ready = DispatchSemaphore(value: 0)
        let cancelled = DispatchSemaphore(value: 0)
        let outcome = Outcome()
        listener.stateUpdateHandler = { [weak self, weak listener] state in
            switch state {
            case .ready:
                // On the queue, before any connection is accepted, so the Host check sees it.
                self?.bound = listener?.port?.rawValue ?? 0
                if outcome.set(nil) { ready.signal() }
            case .failed(let error), .waiting(let error):
                if outcome.set(error) {
                    ready.signal()
                } else {
                    self?.onFailure?(error)
                    listener?.cancel()
                }
            case .cancelled:
                if outcome.set(StartFailure(description: "cancelled")) { ready.signal() }
                cancelled.signal()
            default:
                break
            }
        }
        listener.newConnectionHandler = { [weak self] connection in
            guard let self else { return connection.cancel() }
            self.accept(connection)
        }
        listener.start(queue: queue)
        let failure: String?
        if ready.wait(timeout: .now() + 5) == .timedOut {
            failure = "did not become ready"
        } else {
            failure = outcome.error.map { "\($0)" }
        }
        if let failure {
            listener.cancel()
            _ = cancelled.wait(timeout: .now() + 2)
            throw StartFailure(description: "cannot listen on 127.0.0.1:\(port): \(failure)")
        }
        queue.sync {
            self.listener = listener
            self.listenerCancelled = cancelled
        }
    }

    /// Stops listening, drops any connection still being served, and returns once the
    /// port is released (so the same port can be started again straight away).
    public func stop() {
        dispatchPrecondition(condition: .notOnQueue(queue))
        let cancelled: DispatchSemaphore? = queue.sync {
            listener?.cancel()
            listener = nil
            sessions.values.forEach { $0.connection.cancel() }
            sessions.removeAll()
            bound = 0
            defer { listenerCancelled = nil }
            return listenerCancelled
        }
        _ = cancelled?.wait(timeout: .now() + 2)
    }

    // MARK: Connections (all on `queue`)

    /// One connection and its deadline; used only on `queue`.
    private final class Session: @unchecked Sendable {
        let connection: NWConnection
        var timer: DispatchWorkItem?
        init(_ connection: NWConnection) { self.connection = connection }

        /// (Re)starts the deadline; when it passes, the connection is dropped.
        func arm(_ seconds: TimeInterval, on queue: DispatchQueue) {
            timer?.cancel()
            let connection = connection
            let item = DispatchWorkItem { connection.cancel() }
            timer = item
            queue.asyncAfter(deadline: .now() + seconds, execute: item)
        }
    }

    private func accept(_ connection: NWConnection) {
        let session = Session(connection)
        let id = ObjectIdentifier(connection)
        sessions[id] = session
        connection.stateUpdateHandler = { [weak self] state in
            switch state {
            case .failed, .cancelled:
                session.timer?.cancel()
                self?.sessions[id] = nil
            default: break
            }
        }
        session.arm(headTimeout, on: queue)
        connection.start(queue: queue)
        readHead(session, buffer: Data())
    }

    /// Reads until the blank line that ends the request head (the body, if any, is ignored).
    private func readHead(_ session: Session, buffer: Data) {
        session.connection.receive(minimumIncompleteLength: 1, maximumLength: 8192) {
            [weak self] data, _, isComplete, error in
            guard let self else { return session.connection.cancel() }
            var buffer = buffer
            if let data { buffer.append(data) }
            if let end = buffer.range(of: Data("\r\n\r\n".utf8)), end.lowerBound <= Self.maxHead {
                self.respond(session, head: buffer[..<end.lowerBound])
            } else if buffer.count > Self.maxHead {
                self.sendStatus(session, 431, method: "GET")
            } else if isComplete || error != nil {
                session.connection.cancel()
            } else {
                self.readHead(session, buffer: buffer)
            }
        }
    }

    private func respond(_ session: Session, head: Data) {
        let lines = String(decoding: head, as: UTF8.self).components(separatedBy: "\r\n")
        let parts = lines[0].split(separator: " ", omittingEmptySubsequences: false)
        guard parts.count == 3, parts[2].hasPrefix("HTTP/") else {
            return sendStatus(session, 400, method: "GET")
        }
        let method = String(parts[0])
        guard let host = Self.header("host", in: lines.dropFirst()) else {
            return sendStatus(session, 400, method: method)
        }
        guard Self.isOwnHost(host, port: bound) else {
            return sendStatus(session, 421, method: method)
        }
        guard method == "GET" || method == "HEAD" else {
            return sendStatus(session, 405, method: method, extra: ["Allow": "GET, HEAD"])
        }
        switch Self.resolve(target: String(parts[1]), root: root) {
        case .failure(let status):
            sendStatus(session, status, method: method)
        case .redirect(let location):
            sendStatus(session, 301, method: method, extra: ["Location": location])
        case .success(let file):
            sendFile(session, file, method: method, range: Self.header("range", in: lines.dropFirst()))
        }
    }

    private func sendFile(_ session: Session, _ file: URL, method: String, range: String?) {
        guard let handle = try? FileHandle(forReadingFrom: file),
              let size = (try? FileManager.default.attributesOfItem(atPath: file.path))?[.size] as? NSNumber
        else { return sendStatus(session, 404, method: method) }
        let total = size.intValue
        let type = Self.contentType(for: file)
        var bytes = 0..<total
        var head = Self.head(200, type: type, length: total, extra: ["Accept-Ranges": "bytes"])
        switch range.map({ Self.byteRange($0, size: total) }) {
        case .some(.unsatisfiable):
            try? handle.close()
            return sendStatus(session, 416, method: method, extra: ["Content-Range": "bytes */\(total)"])
        case .some(.part(let part)):
            bytes = part
            head = Self.head(206, type: type, length: part.count, extra: [
                "Accept-Ranges": "bytes",
                "Content-Range": "bytes \(part.lowerBound)-\(part.upperBound - 1)/\(total)",
            ])
        case .some(.whole), .none:
            break
        }
        guard method == "GET" else {
            try? handle.close()
            return finish(session, head)
        }
        do {
            try handle.seek(toOffset: UInt64(bytes.lowerBound))
        } catch {
            try? handle.close()
            return session.connection.cancel()
        }
        session.arm(stallTimeout, on: queue)
        session.connection.send(content: head, completion: .contentProcessed { [weak self] error in
            guard error == nil, let self else { try? handle.close(); return session.connection.cancel() }
            self.stream(session, handle, remaining: bytes.count)
        })
    }

    /// Sends `remaining` bytes of the file a chunk at a time, so a large map tile set never
    /// sits in memory.
    private func stream(_ session: Session, _ handle: FileHandle, remaining: Int) {
        let data = remaining > 0 ? ((try? handle.read(upToCount: min(Self.chunk, remaining))) ?? nil) : nil
        guard let data, !data.isEmpty else {
            try? handle.close()
            return finish(session, nil)
        }
        session.arm(stallTimeout, on: queue)
        session.connection.send(content: data, completion: .contentProcessed { [weak self] error in
            guard error == nil, let self else { try? handle.close(); return session.connection.cancel() }
            self.stream(session, handle, remaining: remaining - data.count)
        })
    }

    private func sendStatus(_ session: Session, _ status: Int, method: String,
                            extra: [String: String] = [:]) {
        let body = Data("\(status) \(Self.reason(status))\n".utf8)
        var data = Self.head(status, type: "text/plain; charset=utf-8", length: body.count, extra: extra)
        if method != "HEAD" { data.append(body) }
        finish(session, data)
    }

    /// Sends the last bytes with FIN, then closes: one response per connection.
    private func finish(_ session: Session, _ data: Data?) {
        session.arm(stallTimeout, on: queue)
        let connection = session.connection
        connection.send(content: data, contentContext: .finalMessage, isComplete: true,
                        completion: .contentProcessed { _ in connection.cancel() })
    }

    // MARK: Pure helpers

    enum Resolution: Equatable {
        case success(URL)
        case redirect(String)
        case failure(Int)
    }

    /// Maps a request target to a file under `root`, a redirect, or the status to answer with.
    static func resolve(target: String, root: URL) -> Resolution {
        let withoutFragment = target.prefix { $0 != "#" }
        let raw = withoutFragment.prefix { $0 != "?" }
        let query = withoutFragment.dropFirst(raw.count)
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
        } else if segments[0] == "media" {
            // Files only: no index page, no redirect, nothing hidden (dot files).
            guard segments.count > 1, !segments.contains(where: { $0.hasPrefix(".") }) else {
                return .failure(404)
            }
            file = root.appendingPathComponent("recording/media")
            segments.dropFirst().forEach { file.appendPathComponent($0) }
            file = file.standardizedFileURL
            var isDirectory: ObjCBool = false
            guard file.path.hasPrefix(root.path + "/") else { return .failure(403) }
            guard FileManager.default.fileExists(atPath: file.path, isDirectory: &isDirectory),
                  !isDirectory.boolValue else { return .failure(404) }
            return .success(file)
        } else {
            return .failure(404)
        }
        file = file.standardizedFileURL
        guard file.path.hasPrefix(root.path + "/") else { return .failure(403) }

        var isDirectory: ObjCBool = false
        let exists = FileManager.default.fileExists(atPath: file.path, isDirectory: &isDirectory)
        if !segments.isEmpty && !path.hasSuffix("/") && exists && isDirectory.boolValue {
            return .redirect(raw + "/" + query)
        }
        if !segments.isEmpty && path.hasSuffix("/") || (exists && isDirectory.boolValue) {
            file.appendPathComponent("index.html")
        }
        guard FileManager.default.fileExists(atPath: file.path, isDirectory: &isDirectory),
              !isDirectory.boolValue else { return .failure(404) }
        return .success(file)
    }

    enum ByteRange: Equatable {
        case whole
        case part(Range<Int>)
        case unsatisfiable
    }

    /// A `Range` header against a file of `size` bytes: one `bytes=a-b`, `a-` or `-n` range
    /// (clamped to the file); anything else (several ranges, other units, nonsense) is
    /// answered with the whole file, as HTTP allows. A range starting past the end is 416.
    static func byteRange(_ header: String, size: Int) -> ByteRange {
        let value = header.trimmingCharacters(in: .whitespaces)
        guard value.lowercased().hasPrefix("bytes=") else { return .whole }
        let spec = value.dropFirst(6).trimmingCharacters(in: .whitespaces)
        guard !spec.contains(","), let dash = spec.firstIndex(of: "-") else { return .whole }
        let first = spec[..<dash].trimmingCharacters(in: .whitespaces)
        let last = spec[spec.index(after: dash)...].trimmingCharacters(in: .whitespaces)
        let digits = { (s: String) in !s.isEmpty && s.allSatisfy(\.isASCII) && s.allSatisfy(\.isNumber) }
        if first.isEmpty {
            guard digits(last), let n = Int(last) else { return .whole }
            guard n > 0, size > 0 else { return .unsatisfiable }
            return .part(max(0, size - n)..<size)
        }
        guard digits(first), let a = Int(first) else { return .whole }
        guard a < size else { return .unsatisfiable }
        if last.isEmpty { return .part(a..<size) }
        guard digits(last), let b = Int(last) else { return .whole }
        guard b >= a else { return .whole }
        return .part(a..<(min(b, size - 1) + 1))
    }

    /// The value of the first header called `name` (lower case), if any.
    static func header(_ name: String, in lines: ArraySlice<String>) -> String? {
        for line in lines {
            guard let colon = line.firstIndex(of: ":"),
                  line[..<colon].lowercased() == name else { continue }
            return line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
        }
        return nil
    }

    /// Only our own origin may be asked for, so a page on another site that rebinds its
    /// name to 127.0.0.1 cannot read the consoles.
    static func isOwnHost(_ host: String, port: UInt16) -> Bool {
        let host = host.lowercased()
        return host == "127.0.0.1:\(port)" || host == "localhost:\(port)"
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
        case "gif": return "image/gif"
        case "mp4", "m4v": return "video/mp4"
        case "webm": return "video/webm"
        case "webp": return "image/webp"
        case "ttf": return "font/ttf"
        case "woff": return "font/woff"
        case "woff2": return "font/woff2"
        case "wasm": return "application/wasm"
        case "pbf": return "application/x-protobuf"
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
        case 206: return "Partial Content"
        case 301: return "Moved Permanently"
        case 400: return "Bad Request"
        case 403: return "Forbidden"
        case 404: return "Not Found"
        case 405: return "Method Not Allowed"
        case 416: return "Range Not Satisfiable"
        case 421: return "Misdirected Request"
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

    /// Records the first outcome only; true if this call was the first.
    func set(_ error: Error?) -> Bool {
        lock.lock(); defer { lock.unlock() }
        guard !isSet else { return false }
        stored = error
        isSet = true
        return true
    }

    var error: Error? {
        lock.lock(); defer { lock.unlock() }
        return stored
    }
}

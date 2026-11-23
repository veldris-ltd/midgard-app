import Foundation

/// What one request to a user-added host came to.
public enum UserHTTPOutcome: Equatable, Sendable {
    /// A 2xx answer's body, within the byte cap.
    case ok(Data)
    /// An answer that is not 2xx, with its body (capped): the gateway's `{"error": …}`.
    case status(Int, Data)
    /// The body, or its declared length, passed the byte cap.
    case oversize
    /// No usable answer: no connection, the time limit, a redirect, a dropped body.
    case network
    /// Not sent: the address is not one the user added, or breaks the address rule.
    case refused
}

/// The app's only network use besides map tiles: the hosts the user adds in Your Data
/// (an enclosure gateway, feed URLs), and only while they are in use. The same hardening
/// as the tile loader (`TileHTTPClient`): an ephemeral session with no cookies,
/// credentials store, URL cache or referrer; the generic User-Agent `MidgardMaps/0.1`,
/// naming no user, machine or address; no redirect is ever followed; 15 s per request;
/// the body read as it arrives and stopped past 8 MiB, and a larger declared length
/// refused before reading. HTTPS, except plain HTTP to the loopback interface (the SDK's
/// address rule, checked again here), under App Transport Security's default. A host
/// is reachable only while it is on the user's list (`allow`); the tile hosts are a
/// separate list. [NFR-PRV-1] [RC-24] [VLD-ANB-MIDGARD-0014]
public final class UserHTTPClient: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    /// Generic: the app and nothing about who or what runs it.
    public static let userAgent = "MidgardMaps/0.1"
    public static let maxBytes = 8 * 1024 * 1024
    public static let timeout: TimeInterval = 15
    static let loopback: Set<String> = ["127.0.0.1", "localhost", "::1"]

    private var session: URLSession!
    private let lock = NSLock()
    private var states: [Int: Pending] = [:]
    private var allowed: Set<String> = []

    private final class Pending {
        let origin: String
        var data = Data()
        var status = 0
        var continuation: CheckedContinuation<UserHTTPOutcome, Never>?
        init(origin: String, continuation: CheckedContinuation<UserHTTPOutcome, Never>) {
            self.origin = origin
            self.continuation = continuation
        }
    }

    /// The session's settings, on their own so the tests check them.
    public static func configuration() -> URLSessionConfiguration {
        let c = URLSessionConfiguration.ephemeral
        c.httpCookieAcceptPolicy = .never
        c.httpShouldSetCookies = false
        c.httpCookieStorage = nil
        c.urlCredentialStorage = nil
        c.urlCache = nil
        c.requestCachePolicy = .reloadIgnoringLocalCacheData
        c.timeoutIntervalForRequest = timeout
        c.timeoutIntervalForResource = timeout
        c.waitsForConnectivity = false
        c.httpMaximumConnectionsPerHost = 2
        c.httpAdditionalHeaders = ["User-Agent": userAgent]
        c.tlsMinimumSupportedProtocolVersion = .TLSv12
        return c
    }

    public override convenience init() {
        self.init(protocolClasses: nil)
    }

    /// `protocolClasses` stand in for the network in the tests.
    init(protocolClasses: [AnyClass]?) {
        super.init()
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        let config = Self.configuration()
        if let protocolClasses { config.protocolClasses = protocolClasses }
        session = URLSession(configuration: config, delegate: self, delegateQueue: queue)
    }

    deinit { session.invalidateAndCancel() }

    /// `scheme://host:port`, the key a host is allowed under.
    static func origin(_ url: URL) -> String? {
        guard let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased() else { return nil }
        let port = url.port ?? (scheme == "https" ? 443 : 80)
        return "\(scheme)://\(host):\(port)"
    }

    /// Replace the user's list of reachable addresses.
    public func setAllowed(_ urls: [URL]) {
        lock.lock()
        allowed = Set(urls.compactMap(Self.origin))
        lock.unlock()
    }

    public var allowedOrigins: Set<String> {
        lock.lock(); defer { lock.unlock() }
        return allowed
    }

    /// The request for `url`, or nil when it may not be sent: on the user's list, HTTPS
    /// or loopback HTTP, with no credentials in the address.
    public func request(_ method: String, _ url: URL, body: Data? = nil, bearer: String? = nil) -> URLRequest? {
        guard let scheme = url.scheme?.lowercased(), let host = url.host?.lowercased(), url.user == nil,
              url.password == nil, let origin = Self.origin(url) else { return nil }
        guard scheme == "https" || (scheme == "http" && Self.loopback.contains(host)) else { return nil }
        guard allowedOrigins.contains(origin) else { return nil }
        var r = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: Self.timeout)
        r.httpMethod = method
        r.httpShouldHandleCookies = false
        r.setValue(Self.userAgent, forHTTPHeaderField: "User-Agent")
        r.setValue("application/json", forHTTPHeaderField: "Accept")
        if let body {
            r.httpBody = body
            r.setValue("application/json", forHTTPHeaderField: "Content-Type")
        }
        if let bearer { r.setValue("Bearer \(bearer)", forHTTPHeaderField: "Authorization") }
        return r
    }

    public func send(_ method: String, _ url: URL, body: Data? = nil, bearer: String? = nil) async -> UserHTTPOutcome {
        guard let req = request(method, url, body: body, bearer: bearer), let origin = Self.origin(url) else {
            return .refused
        }
        let task = session.dataTask(with: req)
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (cont: CheckedContinuation<UserHTTPOutcome, Never>) in
                lock.lock()
                states[task.taskIdentifier] = Pending(origin: origin, continuation: cont)
                lock.unlock()
                if Task.isCancelled { task.cancel() }
                task.resume()
            }
        } onCancel: {
            task.cancel()
        }
    }

    private func finish(_ task: URLSessionTask, _ outcome: UserHTTPOutcome) {
        lock.lock()
        let p = states.removeValue(forKey: task.taskIdentifier)
        lock.unlock()
        p?.continuation?.resume(returning: outcome)
        p?.continuation = nil
    }

    private func pending(_ task: URLSessionTask) -> Pending? {
        lock.lock(); defer { lock.unlock() }
        return states[task.taskIdentifier]
    }

    // MARK: URLSessionDataDelegate

    public func urlSession(_ session: URLSession, task: URLSessionTask, willPerformHTTPRedirection response: HTTPURLResponse,
                           newRequest request: URLRequest, completionHandler: @escaping (URLRequest?) -> Void) {
        // Never followed: a redirect could lead off the user's list, carrying the token.
        completionHandler(nil)
    }

    public func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                           completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let p = pending(dataTask), let http = response as? HTTPURLResponse, let url = http.url,
              Self.origin(url) == p.origin else {
            finish(dataTask, .network)
            return completionHandler(.cancel)
        }
        if (300..<400).contains(http.statusCode) {
            finish(dataTask, .network)
            return completionHandler(.cancel)
        }
        if response.expectedContentLength > Int64(Self.maxBytes) {
            finish(dataTask, .oversize)
            return completionHandler(.cancel)
        }
        p.status = http.statusCode
        completionHandler(.allow)
    }

    public func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard let p = pending(dataTask) else { return }
        p.data.append(data)
        if p.data.count > Self.maxBytes {
            finish(dataTask, .oversize)
            dataTask.cancel()
        }
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let p = pending(task) else { return }
        guard error == nil else { return finish(task, .network) }
        finish(task, (200..<300).contains(p.status) ? .ok(p.data) : .status(p.status, p.data))
    }
}

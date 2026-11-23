import Foundation
import Network

/// What one tile request came to.
public enum FetchOutcome: Equatable, Sendable {
    /// A 2xx answer's body, within the byte cap.
    case body(Data)
    /// An answer that is not 2xx (a 3xx is never followed: it is `.network`).
    case status(Int)
    /// The body, or its declared length, passed the byte cap.
    case oversize
    /// No usable answer: no connection, the time limit, a redirect, a dropped body.
    case network
    /// Not sent: the URL is not an allowed tile request.
    case refused
}

/// Sends a tile request. The loader takes one, so the tests stand in for the network.
public protocol TileFetching: AnyObject, Sendable {
    func fetch(_ url: URL, maxBytes: Int, timeout: TimeInterval) async -> FetchOutcome
}

/// The only way the app reaches the network: map tiles, from the fixed hosts, over HTTPS
/// (App Transport Security's default, with no exception). No cookies, credentials, cache
/// or referrer; a generic User-Agent naming no user, machine or address; no redirect is
/// ever followed; each request ends after 15 s; the body is read as it arrives and
/// stopped once past the source's byte cap, and a larger declared length is refused
/// before reading. [FR-H-1] [NFR-PRV-1]
public final class TileHTTPClient: NSObject, TileFetching, URLSessionDataDelegate, @unchecked Sendable {
    /// Generic: the app and nothing about who or what runs it.
    public static let userAgent = "MidgardMaps/0.1"

    public let allowedHosts: Set<String>
    private var session: URLSession!
    private let lock = NSLock()
    private var states: [Int: Pending] = [:]

    private final class Pending {
        let maxBytes: Int
        let host: String
        var data = Data()
        var continuation: CheckedContinuation<FetchOutcome, Never>?
        init(maxBytes: Int, host: String, continuation: CheckedContinuation<FetchOutcome, Never>) {
            self.maxBytes = maxBytes
            self.host = host
            self.continuation = continuation
        }
    }

    /// The session's settings, on their own so the tests check them.
    public static func configuration(timeout: TimeInterval = TileLimits.fetchTimeout) -> URLSessionConfiguration {
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
        c.httpMaximumConnectionsPerHost = 6
        c.httpAdditionalHeaders = ["User-Agent": userAgent]
        c.tlsMinimumSupportedProtocolVersion = .TLSv12
        return c
    }

    public convenience init(hosts: Set<String>) {
        self.init(hosts: hosts, protocolClasses: nil)
    }

    /// `protocolClasses` stand in for the network in the tests.
    init(hosts: Set<String>, protocolClasses: [AnyClass]?) {
        allowedHosts = hosts
        super.init()
        let queue = OperationQueue()
        queue.maxConcurrentOperationCount = 1
        let config = Self.configuration()
        if let protocolClasses { config.protocolClasses = protocolClasses }
        session = URLSession(configuration: config, delegate: self, delegateQueue: queue)
    }

    deinit { session.invalidateAndCancel() }

    /// The request for `url`, or nil when it may not be sent: HTTPS and an allowed host
    /// only, with no credentials or port.
    public func request(_ url: URL, timeout: TimeInterval = TileLimits.fetchTimeout) -> URLRequest? {
        guard url.scheme == "https", let host = url.host, allowedHosts.contains(host), url.user == nil,
              url.password == nil, url.port == nil else { return nil }
        var r = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: timeout)
        r.httpMethod = "GET"
        r.httpShouldHandleCookies = false
        r.setValue(Self.userAgent, forHTTPHeaderField: "User-Agent")
        return r
    }

    public func fetch(_ url: URL, maxBytes: Int, timeout: TimeInterval) async -> FetchOutcome {
        guard let req = request(url, timeout: timeout), let host = url.host else { return .refused }
        let task = session.dataTask(with: req)
        return await withTaskCancellationHandler {
            await withCheckedContinuation { (cont: CheckedContinuation<FetchOutcome, Never>) in
                lock.lock()
                states[task.taskIdentifier] = Pending(maxBytes: maxBytes, host: host, continuation: cont)
                lock.unlock()
                if Task.isCancelled { task.cancel() }
                task.resume()
            }
        } onCancel: {
            task.cancel()
        }
    }

    private func finish(_ task: URLSessionTask, _ outcome: FetchOutcome) {
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
        // Never followed: a redirect could lead off the list.
        completionHandler(nil)
    }

    public func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive response: URLResponse,
                           completionHandler: @escaping (URLSession.ResponseDisposition) -> Void) {
        guard let p = pending(dataTask), let http = response as? HTTPURLResponse, http.url?.host == p.host,
              http.url?.scheme == "https" else {
            finish(dataTask, .network)
            return completionHandler(.cancel)
        }
        switch http.statusCode {
        case 200..<300:
            if response.expectedContentLength > Int64(p.maxBytes) {
                finish(dataTask, .oversize)
                return completionHandler(.cancel)
            }
            completionHandler(.allow)
        case 300..<400:
            finish(dataTask, .network)
            completionHandler(.cancel)
        default:
            finish(dataTask, .status(http.statusCode))
            completionHandler(.cancel)
        }
    }

    public func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        guard let p = pending(dataTask) else { return }
        p.data.append(data)
        if p.data.count > p.maxBytes {
            finish(dataTask, .oversize)
            dataTask.cancel()
        }
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        guard let p = pending(task) else { return }
        finish(task, error == nil ? .body(p.data) : .network)
    }

    public func urlSession(_ session: URLSession, task: URLSessionTask, willBeginDelayedRequest request: URLRequest,
                           completionHandler: @escaping (URLSession.DelayedRequestDisposition, URLRequest?) -> Void) {
        completionHandler(.continueLoading, nil)
    }
}

/// Whether the device has a network path (the browser's `navigator.onLine`).
final class NetworkPath: @unchecked Sendable {
    static let shared = NetworkPath()
    private let monitor = NWPathMonitor()
    private let lock = NSLock()
    private var satisfied = true

    private init() {
        monitor.pathUpdateHandler = { [weak self] path in
            guard let self else { return }
            self.lock.lock()
            self.satisfied = path.status == .satisfied
            self.lock.unlock()
        }
        monitor.start(queue: DispatchQueue(label: "midgard.tiles.path"))
    }

    var online: Bool {
        lock.lock(); defer { lock.unlock() }
        return satisfied
    }
}

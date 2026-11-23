import Foundation

// The tile loader: every tile request goes through here, as the web's `tiles/loader.ts`.
// [FR-H-1] [NFR-PRV-1]
//
// - Only URLs on the allow-list (`TileSet.allowed`), so HTTPS only, through a fetcher
//   that sends no cookies and follows no redirect (`TileHTTPClient`).
// - Each request ends after 15 s; the body is capped by source (1 MB terrain, 2 MB
//   imagery and vector); an oversize tile is "missing", like one that will not decode.
// - Decoding has a time limit (2 s): past it the tile is a failure retried later.
// - A bounded cache of decoded tiles, and the tiles' bytes on disk (`DiskTileCache`).
//   A 4xx answer (but 429) is "missing"; a 5xx or 429, or a decode out of time, is
//   retried after the retry time.
// - A few requests at a time, most urgent first; `retain` cancels whatever the view no
//   longer needs, queued or in flight.
// - Paused, it sends nothing and cancels what it has (after a camera move the tracks
//   decide, `TilePause`).
// - Offline: after a few network failures in a row, when nothing has ever loaded and
//   every request has failed with none left, or when the device (or the session's
//   OFFLINE chip) says so, it stops sending and reports `offline`; after the retry time
//   it lets one request through as a probe.

public typealias TileDecoder = @Sendable ([UInt8]) throws -> Any

@MainActor
public final class TileLoader {
    public enum State: Equatable, Sendable { case online, offline }
    public enum Status: Equatable, Sendable { case none, pending, ready, missing, failed }

    public struct Stats: Equatable { public var fetched = 0, cancelled = 0, failed = 0, oversize = 0, fromDisk = 0 }

    public struct Options {
        public var maxInFlight = 6
        public var cacheEntries = 256
        public var cacheBytes = Int.max
        public var offlineAfter = 3
        public var retryAfter: TimeInterval = 30
        public var fetchTimeout = TileLimits.fetchTimeout
        public var decodeTimeout = TileLimits.decodeTimeout
        public init() {}
    }

    public private(set) var stats = Stats()
    public private(set) var state: State = .online
    public private(set) var isPaused = false

    private let tiles: TileSet
    private let fetcher: TileFetching
    private let disk: DiskTileCache?
    private let options: Options
    private let now: () -> Date
    private let online: () -> Bool
    private let weigh: (Any) -> Int
    public var onChange: (() -> Void)?

    private struct Cached {
        let value: Any?
        let dispose: ((Any) -> Void)?
    }

    private final class Pending {
        let url: String
        var priority: Int
        let decode: TileDecoder
        let dispose: ((Any) -> Void)?
        var task: Task<Void, Never>?
        var sentAt = Date.distantPast
        init(url: String, priority: Int, decode: @escaping TileDecoder, dispose: ((Any) -> Void)?) {
            self.url = url
            self.priority = priority
            self.decode = decode
            self.dispose = dispose
        }
    }

    private let cache: LruCache<Cached>
    private var pending: [String: Pending] = [:]
    private var failedAt: [String: Date] = [:]
    private var inFlight = 0
    private var failuresInARow = 0
    private var offlineSince = Date.distantPast
    private var retryTask: Task<Void, Never>?
    private var scheduled = false
    private var destroyed = false

    public init(tiles: TileSet, fetcher: TileFetching, disk: DiskTileCache? = nil, options: Options = Options(),
                weigh: @escaping (Any) -> Int = { _ in 0 }, now: @escaping () -> Date = Date.init,
                online: @escaping () -> Bool = { true }) {
        self.tiles = tiles
        self.fetcher = fetcher
        self.disk = disk
        self.options = options
        self.now = now
        self.online = online
        self.weigh = weigh
        cache = LruCache(capacity: options.cacheEntries, maxWeight: options.cacheBytes,
                         weigh: { c in c.value.map(weigh) ?? 0 },
                         onEvict: { _, c in if let v = c.value { c.dispose?(v) } })
    }

    /// Cache entries this returns true for (tiles on screen) are not evicted.
    public var protect: (String) -> Bool {
        get { cache.protect }
        set { cache.protect = newValue }
    }

    public func status(_ url: String) -> Status {
        if let c = cache.peek(url) { return c.value == nil ? .missing : .ready }
        if pending[url] != nil { return .pending }
        if let f = failedAt[url] {
            if now().timeIntervalSince(f) < options.retryAfter { return .failed }
            failedAt[url] = nil
        }
        return .none
    }

    /// The decoded tile, `.some(nil)` when the source has none, nil when not loaded.
    public func get<T>(_ url: String, as: T.Type = T.self) -> T?? {
        guard let c = cache.get(url) else { return nil }
        guard let v = c.value else { return .some(nil) }
        return .some(v as? T)
    }

    /// Failures still remembered (a test diagnostic).
    public var failureRecords: Int { failedAt.count }

    /// Paused, nothing is sent and every queued or in-flight request is cancelled; what is
    /// cached stays readable. Resuming tells the view to ask again.
    public func setPaused(_ paused: Bool) {
        guard paused != isPaused else { return }
        isPaused = paused
        if paused {
            retain([])
        } else {
            onChange?()
            schedule()
        }
    }

    /// Asks for a tile; lower priority numbers go first. A URL off the list is refused
    /// (false) and never sent.
    @discardableResult
    public func request(_ url: String, priority: Int, decode: @escaping TileDecoder, dispose: ((Any) -> Void)? = nil) -> Bool {
        guard tiles.allowed(url) != nil else { return false }
        if destroyed || isPaused { return true }
        if cache.has(url) {
            _ = cache.get(url)
            return true
        }
        if let p = pending[url] {
            p.priority = min(p.priority, priority)
            schedule()
            return true
        }
        if status(url) == .failed && state == .online { return true }
        if !online() { goOffline() }
        pending[url] = Pending(url: url, priority: priority, decode: decode, dispose: dispose)
        schedule()
        return true
    }

    /// Cancels every queued or in-flight request not in `keep`.
    public func retain(_ keep: Set<String>) {
        for (url, p) in pending where !keep.contains(url) {
            pending[url] = nil
            if let t = p.task {
                t.cancel()
                stats.cancelled += 1
            }
        }
    }

    public func destroy() {
        destroyed = true
        retryTask?.cancel()
        retryTask = nil
        retain([])
        cache.protect = { _ in false }
        cache.clear()
    }

    private func schedule() {
        guard !scheduled else { return }
        scheduled = true
        Task { @MainActor [weak self] in
            guard let self else { return }
            self.scheduled = false
            self.pump()
        }
    }

    private func pump() {
        guard !destroyed, !isPaused else { return }
        var slots = options.maxInFlight - inFlight
        if state == .offline {
            if !online() {
                if retryTask == nil { armRetry() }
                return
            }
            let wait = options.retryAfter - now().timeIntervalSince(offlineSince)
            if wait > 0 {
                if retryTask == nil { armRetry(wait) }
                return
            }
            slots = min(slots, 1 - inFlight)
        }
        guard slots > 0 else { return }
        let queued = pending.values.filter { $0.task == nil }.sorted { $0.priority < $1.priority }
        for p in queued.prefix(slots) { start(p) }
    }

    private enum Outcome {
        case value(Any), missing, server, network

        var isNetwork: Bool { if case .network = self { return true } else { return false } }
    }

    private struct Loaded {
        let outcome: Outcome
        var fetched = false
        var fromDisk = false
        var oversize = false
    }

    private func start(_ p: Pending) {
        p.sentAt = now()
        inFlight += 1
        let source = tiles.allowed(p.url)?.0 ?? .vector
        let maxBytes = TileLimits.maxBytes[source] ?? 1024 * 1024
        let fetcher = self.fetcher
        let disk = self.disk
        let timeout = options.fetchTimeout
        let decodeTimeout = options.decodeTimeout
        p.task = Task { @MainActor [weak self] in
            let loaded = await Self.load(p.url, decode: p.decode, maxBytes: maxBytes, fetcher: fetcher, disk: disk,
                                         timeout: timeout, decodeTimeout: decodeTimeout)
            guard let self else { return }
            if loaded.oversize { self.stats.oversize += 1 }
            if loaded.fromDisk { self.stats.fromDisk += 1 }
            // Cancelled by the view: no outcome at all. Anything else that stops the
            // request (the time limit, a dropped connection) is a network failure.
            let cancelled = Task.isCancelled && loaded.outcome.isNetwork
            self.settle(p, cancelled ? nil : loaded.outcome, fetched: loaded.fetched, fromDisk: loaded.fromDisk)
        }
    }

    /// The tile's bytes from the disk cache or the network, then decoded.
    private static func load(_ raw: String, decode: @escaping TileDecoder, maxBytes: Int, fetcher: TileFetching,
                             disk: DiskTileCache?, timeout: TimeInterval, decodeTimeout: TimeInterval) async -> Loaded {
        var bytes: [UInt8]
        var fromDisk = false
        if let disk, let d = await offMain({ disk.read(raw) }), d.count <= maxBytes {
            bytes = [UInt8](d)
            fromDisk = true
        } else {
            guard let url = URL(string: raw) else { return Loaded(outcome: .network) }
            switch await fetcher.fetch(url, maxBytes: maxBytes, timeout: timeout) {
            case let .body(d): bytes = [UInt8](d)
            case let .status(code): return Loaded(outcome: code >= 500 || code == 429 ? .server : .missing, fetched: true)
            case .oversize: return Loaded(outcome: .missing, fetched: true, oversize: true)
            case .network, .refused: return Loaded(outcome: .network)
            }
        }
        if Task.isCancelled { return Loaded(outcome: .network, fetched: !fromDisk, fromDisk: fromDisk) }
        switch await Self.decode(bytes, decode, timeout: decodeTimeout) {
        case let .success(value):
            if !fromDisk, let disk {
                let data = Data(bytes)
                offMainDetached { disk.write(raw, data) }
            }
            return Loaded(outcome: .value(value), fetched: !fromDisk, fromDisk: fromDisk)
        case .failure(.timeout): return Loaded(outcome: .server, fetched: !fromDisk, fromDisk: fromDisk)
        case .failure(.refused): return Loaded(outcome: .missing, fetched: !fromDisk, fromDisk: fromDisk)
        }
    }

    /// What came back, or nil when the request was cancelled by the view.
    private func settle(_ p: Pending, _ outcome: Outcome?, fetched: Bool, fromDisk: Bool) {
        defer {
            inFlight -= 1
            // Nothing has ever loaded, every request has failed and none is left to try:
            // that is offline too, however few tiles the view asked for.
            if stats.fetched == 0 && stats.fromDisk == 0 && failuresInARow > 0 && inFlight == 0 && pending.isEmpty {
                goOffline()
            }
            onChange?()
            schedule()
        }
        if fetched { stats.fetched += 1 }
        let current = pending[p.url] === p
        guard let outcome else { return }
        switch outcome {
        case .network:
            stats.failed += 1
            markFailed(p, current)
            failuresInARow += 1
            if state == .offline {
                // A failed probe waits another retry time; a request sent before going
                // offline, failing only now, does not push the next probe further away.
                if p.sentAt >= offlineSince {
                    offlineSince = now()
                    armRetry()
                }
            } else if failuresInARow >= options.offlineAfter {
                goOffline()
            }
        case .server:
            if !fromDisk { answered() }
            markFailed(p, current)
        case .missing:
            if !fromDisk { answered() }
            if current { store(p, nil) }
        case let .value(v):
            if !fromDisk { answered() }
            if current { store(p, v) } else { p.dispose?(v) }
        }
    }

    private func markFailed(_ p: Pending, _ current: Bool) {
        if current { pending[p.url] = nil }
        let t = now()
        for (u, at) in failedAt where t.timeIntervalSince(at) >= options.retryAfter { failedAt[u] = nil }
        failedAt[p.url] = t
    }

    private func armRetry(_ delay: TimeInterval? = nil) {
        retryTask?.cancel()
        guard !destroyed else { return }
        let wait = max(0.001, delay ?? options.retryAfter)
        retryTask = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: UInt64(wait * 1_000_000_000))
            guard let self, !Task.isCancelled, !self.destroyed else { return }
            self.retryTask = nil
            self.onChange?()
            self.schedule()
        }
    }

    private func store(_ p: Pending, _ value: Any?) {
        pending[p.url] = nil
        failedAt[p.url] = nil
        cache.set(p.url, Cached(value: value, dispose: value == nil ? nil : p.dispose))
    }

    private func answered() {
        failuresInARow = 0
        if state == .offline {
            state = .online
            retryTask?.cancel()
            retryTask = nil
        }
    }

    private func goOffline() {
        guard state != .offline else { return }
        state = .offline
        offlineSince = now()
        armRetry()
        onChange?()
    }

    // MARK: off the main thread

    enum DecodeFailure: Error { case refused, timeout }

    static let decodeQueue = DispatchQueue(label: "midgard.tiles.decode", qos: .userInitiated, attributes: .concurrent)

    /// Decodes on a background queue; past `timeout` the tile fails as out of time (the
    /// work's result, if it ever comes, is dropped).
    static func decode(_ bytes: [UInt8], _ decode: @escaping TileDecoder, timeout: TimeInterval) async -> Result<Any, DecodeFailure> {
        await withCheckedContinuation { (cont: CheckedContinuation<Result<Any, DecodeFailure>, Never>) in
            let once = Once()
            decodeQueue.async {
                let r: Result<Any, DecodeFailure>
                do { r = .success(try decode(bytes)) } catch { r = .failure(.refused) }
                if once.claim() { cont.resume(returning: r) }
            }
            DispatchQueue.global().asyncAfter(deadline: .now() + timeout) {
                if once.claim() { cont.resume(returning: .failure(.timeout)) }
            }
        }
    }

    static func offMain<T: Sendable>(_ work: @escaping @Sendable () -> T) async -> T {
        await withCheckedContinuation { cont in
            DispatchQueue.global(qos: .userInitiated).async { cont.resume(returning: work()) }
        }
    }

    static func offMainDetached(_ work: @escaping @Sendable () -> Void) {
        DispatchQueue.global(qos: .utility).async(execute: work)
    }
}

/// Claimed once, from any thread.
final class Once: @unchecked Sendable {
    private let lock = NSLock()
    private var done = false
    func claim() -> Bool {
        lock.lock(); defer { lock.unlock() }
        if done { return false }
        done = true
        return true
    }
}

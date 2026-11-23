import CryptoKit
import Foundation

/// A least-recently-used cache for decoded tiles and what is built from them, as the
/// web's `tiles/cache.ts`: bounded by entry count and, optionally, by total weight;
/// `protect` keeps entries still on screen from eviction; every value that leaves goes
/// through `onEvict` exactly once. [FR-H-1]
public final class LruCache<V> {
    public let capacity: Int
    public var protect: (String) -> Bool = { _ in false }
    private var order: [String] = []
    private var map: [String: V] = [:]
    private var weights: [String: Int] = [:]
    private let onEvict: (String, V) -> Void
    private let maxWeight: Int
    private let weigh: (V) -> Int
    public private(set) var weight = 0

    public init(capacity: Int, maxWeight: Int = .max, weigh: @escaping (V) -> Int = { _ in 0 },
                onEvict: @escaping (String, V) -> Void = { _, _ in }) {
        self.capacity = max(1, capacity)
        self.maxWeight = maxWeight
        self.weigh = weigh
        self.onEvict = onEvict
    }

    public var count: Int { map.count }
    public var keys: [String] { order }

    private var over: Bool { map.count > capacity || weight > maxWeight }

    public func has(_ key: String) -> Bool { map[key] != nil }

    /// The value, marked as most recently used.
    public func get(_ key: String) -> V? {
        guard let v = map[key] else { return nil }
        touch(key)
        return v
    }

    /// The value, without marking it as used.
    public func peek(_ key: String) -> V? { map[key] }

    private func touch(_ key: String) {
        if let i = order.lastIndex(of: key) { order.remove(at: i) }
        order.append(key)
    }

    public func set(_ key: String, _ value: V) {
        if let old = map[key] {
            weight -= weights[key] ?? 0
            map[key] = nil
            weights[key] = nil
            if let i = order.lastIndex(of: key) { order.remove(at: i) }
            onEvict(key, old)
        }
        let w = max(0, weigh(value))
        map[key] = value
        weights[key] = w
        weight += w
        order.append(key)
        trim()
    }

    @discardableResult
    public func delete(_ key: String) -> Bool {
        guard let v = map[key] else { return false }
        map[key] = nil
        weight -= weights[key] ?? 0
        weights[key] = nil
        if let i = order.lastIndex(of: key) { order.remove(at: i) }
        onEvict(key, v)
        return true
    }

    /// Evicts, oldest first, until within capacity and weight, or only protected entries
    /// (and the most recent one, never evicted by its own arrival) remain.
    public func trim() {
        guard over else { return }
        for key in order.dropLast() {
            if !over { break }
            if protect(key) { continue }
            delete(key)
        }
    }

    public func clear() {
        for key in order { delete(key) }
    }
}

/// The tiles' bytes on disk, in the app's caches directory (`<Caches>/<bundle id>/tiles`,
/// inside the sandbox container on the Mac), least recently used out first once the
/// total passes the cap (500 MB). Files are named by the SHA-256 of their URL; a file's
/// modification date is its last use. Nothing but tile bodies that passed the loader's
/// checks is written. [FR-H-1]
public final class DiskTileCache: @unchecked Sendable {
    public static let defaultCapacity = 500 * 1024 * 1024

    public let directory: URL
    public let capacity: Int
    private let lock = NSLock()
    private var entries: [String: (size: Int, used: Date)] = [:]
    private var total = 0
    private var loaded = false

    public init(directory: URL, capacity: Int = DiskTileCache.defaultCapacity) {
        self.directory = directory
        self.capacity = capacity
    }

    /// The app's own tile cache.
    public static func appDefault() -> DiskTileCache? {
        guard let caches = FileManager.default.urls(for: .cachesDirectory, in: .userDomainMask).first else { return nil }
        let bundle = Bundle.main.bundleIdentifier ?? "io.veldris.midgardmaps"
        return DiskTileCache(directory: caches.appendingPathComponent(bundle, isDirectory: true)
            .appendingPathComponent("tiles", isDirectory: true))
    }

    public var totalBytes: Int {
        lock.lock(); defer { lock.unlock() }
        loadIfNeeded()
        return total
    }

    static func name(_ url: String) -> String {
        SHA256.hash(data: Data(url.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    private func loadIfNeeded() {
        guard !loaded else { return }
        loaded = true
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let keys: [URLResourceKey] = [.fileSizeKey, .contentModificationDateKey]
        let files = (try? FileManager.default.contentsOfDirectory(at: directory, includingPropertiesForKeys: keys)) ?? []
        for f in files {
            let v = try? f.resourceValues(forKeys: Set(keys))
            let size = v?.fileSize ?? 0
            entries[f.lastPathComponent] = (size, v?.contentModificationDate ?? .distantPast)
            total += size
        }
    }

    public func read(_ url: String, now: Date = Date()) -> Data? {
        lock.lock(); defer { lock.unlock() }
        loadIfNeeded()
        let key = Self.name(url)
        guard entries[key] != nil else { return nil }
        let file = directory.appendingPathComponent(key)
        guard let data = try? Data(contentsOf: file) else {
            remove(key)
            return nil
        }
        entries[key]?.used = now
        try? FileManager.default.setAttributes([.modificationDate: now], ofItemAtPath: file.path)
        return data
    }

    public func write(_ url: String, _ data: Data, now: Date = Date()) {
        lock.lock(); defer { lock.unlock() }
        loadIfNeeded()
        guard data.count <= capacity else { return }
        let key = Self.name(url)
        if entries[key] != nil { remove(key) }
        let file = directory.appendingPathComponent(key)
        do {
            try data.write(to: file, options: [.atomic])
            try? FileManager.default.setAttributes([.modificationDate: now], ofItemAtPath: file.path)
        } catch {
            return
        }
        entries[key] = (data.count, now)
        total += data.count
        evict()
    }

    private func remove(_ key: String) {
        guard let e = entries.removeValue(forKey: key) else { return }
        total -= e.size
        try? FileManager.default.removeItem(at: directory.appendingPathComponent(key))
    }

    /// Least recently used out first until the total is within the cap.
    private func evict() {
        guard total > capacity else { return }
        for (key, _) in entries.sorted(by: { $0.value.used < $1.value.used }) {
            if total <= capacity { break }
            remove(key)
        }
    }

    public func clear() {
        lock.lock(); defer { lock.unlock() }
        loadIfNeeded()
        for key in Array(entries.keys) { remove(key) }
    }
}

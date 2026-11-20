#if DEBUG
import Foundation
import MidgardCore

/// The `-MidgardUITesting` launch mode of both apps, compiled into Debug builds only
/// (Release has none of this: the package is built without `DEBUG` there). The labelled
/// software key root, which never prompts here, over secrets kept in memory for the
/// launch, and a fresh store in a temporary directory, so UI tests and the
/// accessibility audit start from enrolment every launch, without a passcode prompt and
/// without touching the keychain. The data source is still the one Info.plist names.
/// Each launch first removes the stores earlier launches left (an app killed by the
/// test runner never cleans up after itself). [RC-49] [AC-C-11]
public enum UITestingEnvironment {
    public static let argument = "-MidgardUITesting"
    static let prefix = "midgard-uitesting-"
    public static let custodyLabel = "Software key (UI testing)"

    /// This launch's store, once made.
    nonisolated(unsafe) private static var current: URL?
    private static let lock = NSLock()

    public static func requested(_ arguments: [String] = ProcessInfo.processInfo.arguments) -> Bool {
        arguments.contains(argument)
    }

    /// The environment for a UI-testing launch. Call it off the main thread, as
    /// `AppEnvironment.live`, through `AppModel.start(making:)`.
    public static func make(reason: String, device: DeviceInfo, bundle: Bundle = .main,
                            temporaryDirectory: URL = FileManager.default.temporaryDirectory) throws -> AppEnvironment {
        let dataSource = try AppEnvironment.dataSourceChoice(in: bundle)
        removeStores(in: temporaryDirectory)
        let base = temporaryDirectory.appendingPathComponent(prefix + UUID().uuidString, isDirectory: true)
        lock.withLock { current = base }
        let secrets = MemorySecretStore()
        let root = SoftwareKeyRoot(reason: reason, label: custodyLabel, secrets: secrets, presence: NoPrompt())
        return AppEnvironment(dataSource: dataSource, storeDirectory: try StorePaths.storeDirectory(base: base),
                              secrets: secrets, keyRoot: root, device: device, replicaId: UUID().uuidString)
    }

    /// Removes every UI-testing store in `directory` (earlier launches').
    static func removeStores(in directory: URL) {
        let fm = FileManager.default
        for old in (try? fm.contentsOfDirectory(at: directory, includingPropertiesForKeys: nil)) ?? []
        where old.lastPathComponent.hasPrefix(prefix) {
            try? fm.removeItem(at: old)
        }
    }

    /// Removes this launch's store, if it made one (on quitting).
    public static func removeCurrentStore() {
        guard let base = lock.withLock({ current }) else { return }
        try? FileManager.default.removeItem(at: base)
    }

    /// Confirms presence without asking.
    struct NoPrompt: UserPresence {
        func confirm(reason: String) throws {}
    }

    /// Secrets in memory for one launch, safe to use from any thread. Like the keychain
    /// it hands out and keeps its own copies: the key roots wipe the buffers they are
    /// given and read in place.
    final class MemorySecretStore: SecretStore, @unchecked Sendable {
        private var items: [String: [String: Data]] = [:]
        private let lock = NSLock()

        private static func copy(_ data: Data) -> Data { data.withUnsafeBytes { Data($0) } }

        func read(service: String, account: String) throws -> Data? {
            lock.withLock { items[service]?[account].map(Self.copy) }
        }

        func write(_ data: Data, service: String, account: String) throws {
            let copy = Self.copy(data)
            lock.withLock { items[service, default: [:]][account] = copy }
        }

        func delete(service: String, account: String) throws {
            lock.withLock { items[service]?[account] = nil }
        }

        func deleteAll(service: String) throws {
            lock.withLock { items[service] = nil }
        }
    }
}
#endif

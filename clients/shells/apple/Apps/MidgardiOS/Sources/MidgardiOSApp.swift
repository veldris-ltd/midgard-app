import MidgardCore
import MidgardKit
import SwiftUI

/// The iPhone app (Task B3): one window, the session's phases, the six consoles.
/// It ships with no data (owner decision 19 Nov 2026): Info.plist `MidgardDataSource` is
/// `none`, so the core signs in its one local user and every screen is empty.
/// [FR-C-1] [FR-C-5]
@main
struct MidgardiOSApp: App {
    @StateObject private var model = AppModel()

    init() {
        _ = MGFonts.registerAll()
    }

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
        }
    }
}

/// How the app builds its environment. Called off the main thread by
/// `AppModel.start(making:)`: it reads the keychain and the disk.
enum LaunchEnvironment {
    static let unlockReason = "Unlock Midgard"

    static func make(arguments: [String] = ProcessInfo.processInfo.arguments) throws -> AppEnvironment {
        let device = DeviceInfo(ramMb: UInt32(clamping: ProcessInfo.processInfo.physicalMemory / (1024 * 1024)),
                                hasAccelerator: true)
        #if DEBUG
        if UITesting.requested(arguments) { return try UITesting.environment(device: device) }
        #endif
        return try AppEnvironment.live(reason: unlockReason, device: device)
    }
}

#if DEBUG
/// `-MidgardUITesting` (Debug builds only; Release ignores it): a non-prompting software
/// key root over an in-memory secret store, and a fresh temporary store directory, so
/// each UI-test launch starts at enrolment without a passcode prompt or the keychain.
/// The data source still comes from Info.plist. [AC-C-11]
enum UITesting {
    static let argument = "-MidgardUITesting"

    static func requested(_ arguments: [String]) -> Bool { arguments.contains(argument) }

    static func environment(device: DeviceInfo) throws -> AppEnvironment {
        let fm = FileManager.default
        let tmp = fm.temporaryDirectory
        // Earlier runs' stores.
        for old in (try? fm.contentsOfDirectory(at: tmp, includingPropertiesForKeys: nil)) ?? []
        where old.lastPathComponent.hasPrefix("midgard-uitesting-") {
            try? fm.removeItem(at: old)
        }
        let dir = tmp.appendingPathComponent("midgard-uitesting-\(UUID().uuidString)", isDirectory: true)
        try fm.createDirectory(at: dir, withIntermediateDirectories: true)
        let secrets = MemorySecretStore()
        let root = SoftwareKeyRoot(reason: LaunchEnvironment.unlockReason, label: "Software key (UI testing)",
                                   secrets: secrets, presence: NoPrompt())
        return AppEnvironment(dataSource: try AppEnvironment.dataSourceChoice(in: .main), storeDirectory: dir,
                              secrets: secrets, keyRoot: root, device: device, replicaId: "ui-testing")
    }

    /// Unlocks without asking.
    struct NoPrompt: UserPresence {
        func confirm(reason: String) throws {}
    }

    /// Secrets in memory for one launch. Like the keychain it hands out and keeps its
    /// own copies: the key roots wipe the buffers they are given and read in place.
    final class MemorySecretStore: SecretStore {
        private var items: [String: Data] = [:]
        private let lock = NSLock()

        private func key(_ service: String, _ account: String) -> String { service + "\u{0}" + account }
        private static func copy(_ data: Data) -> Data { data.withUnsafeBytes { Data($0) } }

        func read(service: String, account: String) throws -> Data? {
            lock.lock(); defer { lock.unlock() }
            return items[key(service, account)].map(Self.copy)
        }

        func write(_ data: Data, service: String, account: String) throws {
            lock.lock(); defer { lock.unlock() }
            items[key(service, account)] = Self.copy(data)
        }

        func delete(service: String, account: String) throws {
            lock.lock(); defer { lock.unlock() }
            items[key(service, account)] = nil
        }

        func deleteAll(service: String) throws {
            lock.lock(); defer { lock.unlock() }
            items = items.filter { !$0.key.hasPrefix(service + "\u{0}") }
        }
    }
}
#endif

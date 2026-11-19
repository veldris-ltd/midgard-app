#if DEBUG
import CryptoKit
import Foundation
@preconcurrency import MidgardCore
import MidgardKit

/// The `-MidgardUITesting` launch mode (Debug builds only; Release has none of this):
/// a key root that never prompts, an in-memory secret store and a fresh temporary
/// store, so UI tests and the accessibility audit start from enrolment every launch and
/// never touch the keychain. The data source is still the one Info.plist names.
/// [RC-49] [AC-C-11]
enum UITesting {
    static let argument = "-MidgardUITesting"

    static var isActive: Bool { ProcessInfo.processInfo.arguments.contains(argument) }

    private static let base = FileManager.default.temporaryDirectory
        .appendingPathComponent("midgard-uitesting-\(UUID().uuidString)", isDirectory: true)

    static func environment(device: DeviceInfo) throws -> AppEnvironment {
        AppEnvironment(dataSource: try AppEnvironment.dataSourceChoice(in: .main),
                       storeDirectory: try StorePaths.storeDirectory(base: base),
                       secrets: MemorySecretStore(),
                       keyRoot: UITestingKeyRoot(),
                       device: device,
                       replicaId: UUID().uuidString)
    }

    /// The temporary store goes with the process.
    static func removeStore() {
        guard isActive else { return }
        try? FileManager.default.removeItem(at: base)
    }
}

/// AES-GCM under a key held in memory for this process; no prompt, no keychain.
final class UITestingKeyRoot: PlatformKeyRoot {
    private let key = SymmetricKey(size: .bits256)
    let custodyLabel = "Test key (UI testing)"
    let kind = KeyRootKind.software

    func seal(dataKey: Data) throws -> Data {
        guard let sealed = try? AES.GCM.seal(dataKey, using: key).combined else {
            throw KeyRootError.PlatformUnavailable(detail: "test seal")
        }
        return sealed
    }

    func unseal(sealed: Data) throws -> Data {
        guard let box = try? AES.GCM.SealedBox(combined: sealed) else { throw KeyRootError.Malformed(detail: "sealed box") }
        guard let opened = try? AES.GCM.open(box, using: key) else { throw KeyRootError.Unseal }
        return opened
    }

    func hardwareBacked() -> Bool { false }

    func destroyKey() throws {}
}

/// The secret store for UI testing: (service, account) → bytes, in memory.
final class MemorySecretStore: SecretStore {
    private var items: [String: [String: Data]] = [:]
    private let lock = NSLock()

    func read(service: String, account: String) throws -> Data? {
        lock.lock(); defer { lock.unlock() }
        return items[service]?[account].map { Data([UInt8]($0)) }
    }

    func write(_ data: Data, service: String, account: String) throws {
        lock.lock(); defer { lock.unlock() }
        items[service, default: [:]][account] = Data([UInt8](data))
    }

    func delete(service: String, account: String) throws {
        lock.lock(); defer { lock.unlock() }
        items[service]?[account] = nil
    }

    func deleteAll(service: String) throws {
        lock.lock(); defer { lock.unlock() }
        items[service] = nil
    }
}
#endif

import Foundation

@testable import MidgardKit

/// A keychain stand-in for tests: (service, account) → bytes, with a log of writes
/// so tests can check the order of file and keychain updates. [FR-C-9]
final class InMemorySecretStore: SecretStore {
    private var items: [String: [String: Data]] = [:]
    private(set) var log: [String] = []
    /// Called before each write with (service, account); a test may inspect the disk.
    var onWrite: ((String, String) -> Void)?
    /// When set, every write fails with this status.
    var failWrites: OSStatus?

    func read(service: String, account: String) throws -> Data? { items[service]?[account] }

    func write(_ data: Data, service: String, account: String) throws {
        if let status = failWrites { throw SecretStoreError.keychain(status) }
        onWrite?(service, account)
        items[service, default: [:]][account] = data
        log.append("write \(service)")
    }

    func delete(service: String, account: String) throws {
        items[service]?[account] = nil
        log.append("delete \(service)")
    }

    func deleteAll(service: String) throws {
        items[service] = nil
        log.append("deleteAll \(service)")
    }

    func accounts(service: String) -> [String] { (items[service] ?? [:]).keys.sorted() }
}

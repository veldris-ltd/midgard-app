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
    /// Writes to these services fail with the given status.
    var failWritesTo: [String: OSStatus] = [:]
    /// Reads of these services fail with the given status.
    var failReads: [String: OSStatus] = [:]

    func read(service: String, account: String) throws -> Data? {
        if let status = failReads[service] { throw SecretStoreError.keychain(status) }
        // A fresh buffer each time, as the keychain returns: callers wipe what they get.
        return items[service]?[account].map { Data([UInt8]($0)) }
    }

    func write(_ data: Data, service: String, account: String) throws {
        if let status = failWrites ?? failWritesTo[service] { throw SecretStoreError.keychain(status) }
        onWrite?(service, account)
        items[service, default: [:]][account] = Data([UInt8](data))
        log.append("write \(service)")
    }

    func delete(service: String, account: String) throws {
        items[service]?[account] = nil
        log.append("delete \(service)")
    }

    /// When set, `deleteAll` fails with this status.
    var failDeleteAll: OSStatus?

    func deleteAll(service: String) throws {
        if let status = failDeleteAll { throw SecretStoreError.keychain(status) }
        items[service] = nil
        log.append("deleteAll \(service)")
    }

    func accounts(service: String) -> [String] { (items[service] ?? [:]).keys.sorted() }
}

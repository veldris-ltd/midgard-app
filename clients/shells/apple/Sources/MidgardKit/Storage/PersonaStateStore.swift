import CryptoKit
import Foundation
import MidgardCore

/// What the shell keeps between launches, all sealed by the core:
///
/// - `root.sealed`, `device-audit.sealed`: the `SealedKeys` from `enrol()`;
/// - `<hmac>.state`: one sealed state file per persona, named by
///   HMAC-SHA-256(device-local key, persona id) in hex — never the persona id or a
///   plain digest of it (four known personas would be reversed by hashing each);
/// - per persona, the newest generation written, in a `WhenUnlockedThisDeviceOnly`
///   keychain item whose account is the same HMAC. Never one device-wide value: a
///   shared value would turn persona A's file into a "rollback" after persona B
///   exported a newer one.
///
/// The device-local key is 32 random bytes made on first use and kept only in the
/// keychain. Order on every save: write the file atomically, then (only after the
/// rename) move the generation, so the keychain is never ahead of the disk.
/// [FR-C-4] [FR-C-9] [AC-C-2]
public final class PersonaStateStore {
    public static let deviceKeyService = "io.veldris.midgard.state-names"
    public static let generationService = "io.veldris.midgard.state-generation"
    static let deviceKeyAccount = "state-names"
    static let rootFile = "root.sealed"
    static let deviceAuditFile = "device-audit.sealed"

    public let directory: URL
    private let secrets: SecretStore

    public init(directory: URL, secrets: SecretStore) {
        self.directory = directory
        self.secrets = secrets
    }

    private func deviceKey() throws -> SymmetricKey {
        if var raw = try secrets.read(service: Self.deviceKeyService, account: Self.deviceKeyAccount), raw.count == 32 {
            defer { Wipe.wipe(&raw) }
            return SymmetricKey(data: raw)
        }
        let key = SymmetricKey(size: .bits256)
        var raw = key.withUnsafeBytes { Data($0) }
        defer { Wipe.wipe(&raw) }
        try secrets.write(raw, service: Self.deviceKeyService, account: Self.deviceKeyAccount)
        return key
    }

    /// The persona's file name and keychain account: lowercase hex HMAC-SHA-256.
    public func fileName(for persona: String) throws -> String {
        let mac = HMAC<SHA256>.authenticationCode(for: Data(persona.utf8), using: try deviceKey())
        return mac.map { String(format: "%02x", $0) }.joined()
    }

    public func stateURL(for persona: String) throws -> URL {
        directory.appendingPathComponent(try fileName(for: persona) + ".state")
    }

    /// The newest generation saved for `persona` (0 if none): `importState`'s minimum.
    public func minGeneration(for persona: String) throws -> UInt64 {
        guard let raw = try secrets.read(service: Self.generationService, account: try fileName(for: persona)),
              raw.count == 8 else { return 0 }
        return raw.reduce(UInt64(0)) { $0 << 8 | UInt64($1) }
    }

    public func load(persona: String) throws -> Data? {
        let url = try stateURL(for: persona)
        guard FileManager.default.fileExists(atPath: url.path) else { return nil }
        return try Data(contentsOf: url)
    }

    /// Write the state file atomically, then record its generation for this persona.
    public func save(_ state: ExportedState, persona: String) throws {
        let name = try fileName(for: persona)
        try AtomicFile.write(state.file, to: directory.appendingPathComponent(name + ".state"))
        let bytes = (0..<8).reversed().map { UInt8(truncatingIfNeeded: state.generation >> (UInt64($0) * 8)) }
        try secrets.write(Data(bytes), service: Self.generationService, account: name)
    }

    /// Delete the persona's file (it did not open, or was a rollback). The generation
    /// stays, so an older copy restored later is still refused.
    public func discard(persona: String) throws {
        try AtomicFile.remove(try stateURL(for: persona))
    }

    public func saveKeys(_ keys: SealedKeys) throws {
        try AtomicFile.write(keys.root, to: directory.appendingPathComponent(Self.rootFile))
        try AtomicFile.write(keys.deviceAudit, to: directory.appendingPathComponent(Self.deviceAuditFile))
    }

    public func loadKeys() throws -> SealedKeys? {
        let root = directory.appendingPathComponent(Self.rootFile)
        let audit = directory.appendingPathComponent(Self.deviceAuditFile)
        guard FileManager.default.fileExists(atPath: root.path),
              FileManager.default.fileExists(atPath: audit.path) else { return nil }
        return SealedKeys(root: try Data(contentsOf: root), deviceAudit: try Data(contentsOf: audit))
    }

    /// Reset demo: every file here, every generation and the naming key.
    public func reset() throws {
        for name in try FileManager.default.contentsOfDirectory(atPath: directory.path) {
            try AtomicFile.remove(directory.appendingPathComponent(name))
        }
        try secrets.deleteAll(service: Self.generationService)
        try secrets.deleteAll(service: Self.deviceKeyService)
    }
}

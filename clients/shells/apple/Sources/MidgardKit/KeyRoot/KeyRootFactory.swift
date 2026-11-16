import CryptoKit
import Foundation
import Security

/// What kind of platform key seals this device's root.
public enum KeyRootKind: String, Equatable, Sendable {
    case enclave, software

    /// The kind a sealed root blob was made with, from the length of its sealed data
    /// key (`clients/keystore/src/sealed_root.rs`: `"MGSROOT1" | key_id 4+n | suite 1 |
    /// root_public 4+n | sealed_data_key 4+n | …`, lengths u32 little-endian): 113 bytes
    /// is Enclave ECIES, 60 the software root's AES-GCM. Nil for anything else.
    public init?(sealedRoot blob: Data) {
        let bytes = [UInt8](blob)
        var at = 0
        func take(_ n: Int) -> ArraySlice<UInt8>? {
            guard n >= 0, at + n <= bytes.count else { return nil }
            defer { at += n }
            return bytes[at..<(at + n)]
        }
        func field() -> ArraySlice<UInt8>? {
            guard let len = take(4) else { return nil }
            let n = len.reversed().reduce(0) { $0 << 8 | Int($1) }
            return take(n)
        }
        guard let magic = take(8), Array(magic) == Array("MGSROOT1".utf8), field() != nil, take(1) != nil,
              field() != nil, let sealed = field() else { return nil }
        switch sealed.count {
        case SecureEnclaveKeyRoot.sealedLength: self = .enclave
        case SoftwareKeyRoot.sealedLength: self = .software
        default: return nil
        }
    }
}

/// The facts about this device the choice depends on, injectable for tests.
public struct KeyRootPlatform {
    public var simulator: Bool
    public var enclaveAvailable: Bool
    /// Writes and deletes an empty item in the data-protection keychain; the status.
    public var probeProtectedKeychain: () -> OSStatus

    public init(simulator: Bool, enclaveAvailable: Bool, probeProtectedKeychain: @escaping () -> OSStatus) {
        self.simulator = simulator
        self.enclaveAvailable = enclaveAvailable
        self.probeProtectedKeychain = probeProtectedKeychain
    }

    public static var current: KeyRootPlatform {
        #if targetEnvironment(simulator)
        let simulator = true
        #else
        let simulator = false
        #endif
        return KeyRootPlatform(simulator: simulator, enclaveAvailable: SecureEnclave.isAvailable,
                               probeProtectedKeychain: KeyRootFactory.probeProtectedKeychain)
    }
}

/// Chooses the key root. At enrolment (no sealed root yet) it probes: the Enclave when
/// the device has one and the app may write to the data-protection keychain, the
/// labelled software root on the simulator or a Mac without an Enclave. An unsigned
/// build (no access group: `errSecMissingEntitlement`) cannot keep keys at all and is
/// refused; any other probe failure (a locked device) is an error to retry, never a
/// silent downgrade. Once enrolled, the root follows the choice recorded at enrolment
/// (keychain service `io.veldris.midgard.key-root.choice`), cross-checked with the
/// sealed root's shape; a contradiction is refused. Never fakes hardware.
/// [FR-C-2] [FR-C-9] [DD-6]
public enum KeyRootFactory {
    public enum Failure: Error, Equatable {
        /// No keychain access group: this build cannot keep keys.
        case unsignedBuild
        /// The keychain refused (for example the device is locked): try again.
        case keychainUnavailable(OSStatus)
        /// The recorded choice and the sealed root disagree (or one is unreadable).
        case mismatch(stored: KeyRootKind?, sealed: KeyRootKind?)
    }

    public static let choiceService = "io.veldris.midgard.key-root.choice"
    static let choiceAccount = deviceRootKeyId

    /// The key root for this device. `sealedRoot` is the stored `SealedKeys.root`, nil
    /// before enrolment.
    public static func make(reason: String, secrets: SecretStore = KeychainSecretStore(), sealedRoot: Data?,
                            platform: KeyRootPlatform = .current) throws -> PlatformKeyRoot {
        let stored = try storedChoice(secrets: secrets)
        let kind: KeyRootKind
        if let sealedRoot {
            let shape = KeyRootKind(sealedRoot: sealedRoot)
            if let stored {
                guard shape == stored else { throw Failure.mismatch(stored: stored, sealed: shape) }
                kind = stored
            } else {
                guard let shape else { throw Failure.mismatch(stored: nil, sealed: nil) }
                try record(shape, secrets: secrets)
                kind = shape
            }
        } else {
            kind = try choose(platform)
        }
        return root(kind, reason: reason, secrets: secrets, platform: platform)
    }

    /// The probe at enrolment.
    static func choose(_ platform: KeyRootPlatform) throws -> KeyRootKind {
        let status = platform.probeProtectedKeychain()
        switch status {
        case errSecSuccess, errSecDuplicateItem: break
        case errSecMissingEntitlement: throw Failure.unsignedBuild
        default: throw Failure.keychainUnavailable(status)
        }
        return platform.simulator || !platform.enclaveAvailable ? .software : .enclave
    }

    static func root(_ kind: KeyRootKind, reason: String, secrets: SecretStore,
                     platform: KeyRootPlatform) -> PlatformKeyRoot {
        switch kind {
        case .enclave:
            return SecureEnclaveKeyRoot(reason: reason)
        case .software:
            let label = platform.simulator ? "Software key (simulator)" : "Software key (no Secure Enclave)"
            return SoftwareKeyRoot(reason: reason, label: label, secrets: secrets)
        }
    }

    /// The choice recorded at enrolment, if any.
    public static func storedChoice(secrets: SecretStore) throws -> KeyRootKind? {
        let raw: Data?
        do { raw = try secrets.read(service: choiceService, account: choiceAccount) } catch { throw mapped(error) }
        guard let raw else { return nil }
        guard let kind = String(data: raw, encoding: .utf8).flatMap(KeyRootKind.init(rawValue:)) else {
            throw Failure.mismatch(stored: nil, sealed: nil)
        }
        return kind
    }

    /// Record the kind the device enrolled with.
    public static func record(_ kind: KeyRootKind, secrets: SecretStore) throws {
        do { try secrets.write(Data(kind.rawValue.utf8), service: choiceService, account: choiceAccount) } catch {
            throw mapped(error)
        }
    }

    /// Reset demo: forget the choice (the next enrolment probes again).
    public static func forgetChoice(secrets: SecretStore) throws {
        try secrets.deleteAll(service: choiceService)
    }

    private static func mapped(_ error: Error) -> Error {
        guard case let SecretStoreError.keychain(status)? = error as? SecretStoreError else { return error }
        return status == errSecMissingEntitlement ? Failure.unsignedBuild : Failure.keychainUnavailable(status)
    }

    /// Whether this process may write to the data-protection keychain. An unsigned
    /// build (no access group) reads "not found" but gets `errSecMissingEntitlement`
    /// on any write, so the probe writes an empty item and deletes it again.
    static func probeProtectedKeychain() -> OSStatus {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                    kSecAttrService as String: SecureEnclaveKeyRoot.tagPrefix + "probe",
                                    kSecAttrAccount as String: "probe",
                                    kSecUseDataProtectionKeychain as String: true]
        var add = query
        add[kSecValueData as String] = Data()
        add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(add as CFDictionary, nil)
        if status == errSecSuccess || status == errSecDuplicateItem { SecItemDelete(query as CFDictionary) }
        return status
    }
}

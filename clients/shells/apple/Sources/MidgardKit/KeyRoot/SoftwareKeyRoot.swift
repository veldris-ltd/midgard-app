import CryptoKit
import Foundation
import MidgardCore
import Security

/// The labelled software key root: for the iOS simulator (no Secure Enclave), Macs
/// without one, and builds that cannot use the data-protection keychain. A 256-bit
/// AES-GCM key in a `ThisDeviceOnly` keychain item seals the core's data key; unseal
/// first asks for user presence. `hardwareBacked()` is `false`, so the custody chip
/// says "SOFTWARE KEY". [FR-C-2] [FR-C-9] [DD-6]
public final class SoftwareKeyRoot: PlatformKeyRoot {
    /// nonce (12) | ciphertext (32) | tag (16).
    public static let sealedLength = 60
    static let service = "io.veldris.midgard.key-root.software"
    private static let aad = Data("midgard-software-key-root-v1".utf8)

    private let keyId: String
    private let reason: String
    private let secrets: SecretStore
    private let presence: UserPresence
    public let custodyLabel: String

    public init(keyId: String = deviceRootKeyId, reason: String, label: String,
                secrets: SecretStore = KeychainSecretStore(), presence: UserPresence = DeviceOwnerPresence()) {
        self.keyId = keyId
        self.reason = reason
        self.custodyLabel = label
        self.secrets = secrets
        self.presence = presence
    }

    private func storedKey() throws -> SymmetricKey? {
        guard var raw = try secrets.read(service: Self.service, account: keyId) else { return nil }
        defer { Wipe.wipe(&raw) }
        return SymmetricKey(data: raw)
    }

    public func seal(dataKey: Data) throws -> Data {
        defer { Wipe.wipeInPlace(dataKey) }
        do {
            let key: SymmetricKey
            if let stored = try storedKey() {
                key = stored
            } else {
                key = SymmetricKey(size: .bits256)
                var raw = key.withUnsafeBytes { Data($0) }
                defer { Wipe.wipe(&raw) }
                try secrets.write(raw, service: Self.service, account: keyId)
            }
            guard let sealed = try AES.GCM.seal(dataKey, using: key, authenticating: Self.aad).combined,
                  sealed.count == Self.sealedLength else {
                throw KeyRootError.PlatformUnavailable(detail: "software seal length")
            }
            return sealed
        } catch {
            throw KeyRootErrors.map(error, during: .seal)
        }
    }

    public func unseal(sealed: Data) throws -> Data {
        guard sealed.count == Self.sealedLength else {
            throw KeyRootError.Malformed(detail: "sealed length \(sealed.count), expected \(Self.sealedLength)")
        }
        do {
            try presence.confirm(reason: reason)
            guard let key = try storedKey() else { throw KeyRootError.PlatformUnavailable(detail: "no platform key") }
            let box: AES.GCM.SealedBox
            do { box = try AES.GCM.SealedBox(combined: sealed) } catch { throw KeyRootError.Malformed(detail: "sealed box") }
            var opened: Data
            do { opened = try AES.GCM.open(box, using: key, authenticating: Self.aad) } catch { throw KeyRootError.Unseal }
            defer { Wipe.wipe(&opened) }
            return opened.withUnsafeBytes { Wipe.selfWiping($0) }
        } catch {
            throw KeyRootErrors.map(error, during: .unseal)
        }
    }

    public func hardwareBacked() -> Bool { false }

    public func destroyKey() throws {
        try secrets.delete(service: Self.service, account: keyId)
    }
}

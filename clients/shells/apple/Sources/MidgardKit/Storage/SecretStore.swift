import Foundation
import Security

/// Small secrets the shell keeps outside files: the software key root's key, the
/// device-local HMAC key that names state files, and each persona's state-file
/// generation. [FR-C-9] [DD-6]
public protocol SecretStore: AnyObject {
    func read(service: String, account: String) throws -> Data?
    func write(_ data: Data, service: String, account: String) throws
    func delete(service: String, account: String) throws
    func deleteAll(service: String) throws
}

public enum SecretStoreError: Error, Equatable {
    case keychain(OSStatus)
}

/// The keychain: generic-password items, `WhenUnlockedThisDeviceOnly` (never in a
/// backup, never migrated to another device), in the data-protection keychain. On
/// macOS that keychain needs a signed app with a `keychain-access-groups`
/// entitlement; an unsigned build gets `errSecMissingEntitlement`, which is reported,
/// never worked around. Nothing read or written here is logged.
public final class KeychainSecretStore: SecretStore {
    public init() {}

    private func query(service: String, account: String?) -> [String: Any] {
        var q: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                kSecAttrService as String: service,
                                kSecUseDataProtectionKeychain as String: true]
        if let account { q[kSecAttrAccount as String] = account }
        return q
    }

    public func read(service: String, account: String) throws -> Data? {
        var q = query(service: service, account: account)
        q[kSecReturnData as String] = true
        q[kSecMatchLimit as String] = kSecMatchLimitOne
        var out: CFTypeRef?
        let status = SecItemCopyMatching(q as CFDictionary, &out)
        switch status {
        case errSecSuccess: return out as? Data
        case errSecItemNotFound: return nil
        default: throw SecretStoreError.keychain(status)
        }
    }

    public func write(_ data: Data, service: String, account: String) throws {
        let q = query(service: service, account: account)
        let update: [String: Any] = [kSecValueData as String: data]
        var status = SecItemUpdate(q as CFDictionary, update as CFDictionary)
        if status == errSecItemNotFound {
            var add = q
            add[kSecValueData as String] = data
            add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            status = SecItemAdd(add as CFDictionary, nil)
        }
        guard status == errSecSuccess else { throw SecretStoreError.keychain(status) }
    }

    public func delete(service: String, account: String) throws {
        let status = SecItemDelete(query(service: service, account: account) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw SecretStoreError.keychain(status) }
    }

    public func deleteAll(service: String) throws {
        let status = SecItemDelete(query(service: service, account: nil) as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else { throw SecretStoreError.keychain(status) }
    }
}

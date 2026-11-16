import CryptoKit
import Foundation
import Security

/// Which key root this device gets.
public enum KeyRootChoice: Equatable, Sendable {
    case secureEnclave
    case software(label: String)
}

/// Chooses the key root: the Secure Enclave only when the device has one and the app
/// can keep a key in the data-protection keychain (a signed build with
/// `keychain-access-groups` on macOS); otherwise the labelled software root, which
/// reports `hardwareBacked() == false`. Never fakes hardware. [FR-C-2] [DD-6]
public enum KeyRootFactory {
    public static func choose(enclaveAvailable: Bool, protectedKeychainUsable: Bool, simulator: Bool) -> KeyRootChoice {
        if simulator { return .software(label: "Software key (simulator)") }
        if !enclaveAvailable { return .software(label: "Software key (no Secure Enclave)") }
        if !protectedKeychainUsable { return .software(label: "Software key (unsigned build)") }
        return .secureEnclave
    }

    public static func make(reason: String, secrets: SecretStore = KeychainSecretStore()) -> PlatformKeyRoot {
        #if targetEnvironment(simulator)
        let simulator = true
        #else
        let simulator = false
        #endif
        let choice = choose(enclaveAvailable: SecureEnclave.isAvailable,
                            protectedKeychainUsable: protectedKeychainUsable(), simulator: simulator)
        switch choice {
        case .secureEnclave: return SecureEnclaveKeyRoot(reason: reason)
        case let .software(label): return SoftwareKeyRoot(reason: reason, label: label, secrets: secrets)
        }
    }

    /// Whether this process may write to the data-protection keychain. An unsigned
    /// build (no access group) reads "not found" but gets `errSecMissingEntitlement`
    /// on any write, so the probe writes an empty item and deletes it again.
    static func protectedKeychainUsable() -> Bool {
        let query: [String: Any] = [kSecClass as String: kSecClassGenericPassword,
                                    kSecAttrService as String: SecureEnclaveKeyRoot.tagPrefix + "probe",
                                    kSecAttrAccount as String: "probe",
                                    kSecUseDataProtectionKeychain as String: true]
        var add = query
        add[kSecValueData as String] = Data()
        add[kSecAttrAccessible as String] = kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        let status = SecItemAdd(add as CFDictionary, nil)
        guard status == errSecSuccess || status == errSecDuplicateItem else { return false }
        SecItemDelete(query as CFDictionary)
        return true
    }
}

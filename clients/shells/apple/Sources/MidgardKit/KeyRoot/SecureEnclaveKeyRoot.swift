import Foundation
import LocalAuthentication
import MidgardCore
import Security

/// The Secure Enclave key root. A P-256 key made inside the Enclave on the first
/// `seal`, permanent in the data-protection keychain, with access control
/// `.privateKeyUsage` + `.userPresence` (Face ID / Touch ID with passcode fallback;
/// not `.biometryCurrentSet`, which would lose the data on a biometry change).
///
/// `seal` encrypts the core's 32-byte data key to the key's public half with ECIES
/// (`eciesEncryptionCofactorVariableIVX963SHA256AESGCM`: no prompt) and returns 113
/// bytes; `unseal` decrypts inside the Enclave (one prompt). It implements the core's
/// `KeyRoot` callback and nothing else: the core's ML-KEM root never enters Swift.
///
/// On macOS the Enclave key needs the data-protection keychain and a signed build with
/// `keychain-access-groups`; `KeyRootFactory` checks that and otherwise uses the
/// software root, so `hardwareBacked()` is true only when this type is in use.
/// [FR-C-2] [FR-C-9] [DD-6]
public final class SecureEnclaveKeyRoot: PlatformKeyRoot {
    public static let keyId = deviceRootKeyId
    /// 65-byte ephemeral public point, 32-byte ciphertext, 16-byte GCM tag.
    public static let sealedLength = 113
    public static let algorithm: SecKeyAlgorithm = .eciesEncryptionCofactorVariableIVX963SHA256AESGCM
    static let tagPrefix = "io.veldris.midgard.key-root."

    private let tag: Data
    private let reason: String
    public let custodyLabel = "Secure Enclave"
    public let kind = KeyRootKind.enclave

    public init(keyId: String = SecureEnclaveKeyRoot.keyId, reason: String) {
        self.tag = Data((Self.tagPrefix + keyId).utf8)
        self.reason = reason
    }

    private var keyQuery: [String: Any] {
        [kSecClass as String: kSecClassKey,
         kSecAttrApplicationTag as String: tag,
         kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
         kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
         kSecUseDataProtectionKeychain as String: true]
    }

    /// The private key's reference. Fetching it does not prompt; using it does.
    private func privateKey(context: LAContext?, during operation: KeyRootOperation) throws -> SecKey? {
        var query = keyQuery
        query[kSecReturnRef as String] = true
        if let context { query[kSecUseAuthenticationContext as String] = context }
        var out: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &out)
        switch status {
        case errSecSuccess:
            guard let out, CFGetTypeID(out) == SecKeyGetTypeID() else {
                throw KeyRootError.PlatformUnavailable(detail: "keychain item is not a key")
            }
            return (out as! SecKey)  // checked above
        case errSecItemNotFound:
            return nil
        default:
            throw KeyRootErrors.map(status: status, during: operation)
        }
    }

    private func createKey() throws -> SecKey {
        var error: Unmanaged<CFError>?
        guard let access = SecAccessControlCreateWithFlags(nil, kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                                                           [.privateKeyUsage, .userPresence], &error) else {
            throw KeyRootErrors.map(error, during: .seal)
        }
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecUseDataProtectionKeychain as String: true,
            kSecPrivateKeyAttrs as String: [kSecAttrIsPermanent as String: true,
                                            kSecAttrApplicationTag as String: tag,
                                            kSecAttrAccessControl as String: access] as [String: Any],
        ]
        guard let key = SecKeyCreateRandomKey(attributes as CFDictionary, &error) else {
            throw KeyRootErrors.map(error, during: .seal)
        }
        return key
    }

    /// ECIES of `dataKey` to `publicKey`; refuses anything but the 113-byte shape.
    static func eciesSeal(publicKey: SecKey, dataKey: Data) throws -> Data {
        guard SecKeyIsAlgorithmSupported(publicKey, .encrypt, algorithm) else {
            throw KeyRootError.PlatformUnavailable(detail: "ECIES not supported by the key")
        }
        var error: Unmanaged<CFError>?
        guard let sealed = SecKeyCreateEncryptedData(publicKey, algorithm, dataKey as CFData, &error) as Data? else {
            throw KeyRootErrors.map(error, during: .seal)
        }
        guard sealed.count == sealedLength else {
            throw KeyRootError.PlatformUnavailable(detail: "sealed length \(sealed.count), expected \(sealedLength)")
        }
        return sealed
    }

    public func seal(dataKey: Data) throws -> Data {
        defer { Wipe.wipeInPlace(dataKey) }
        let key = try privateKey(context: nil, during: .seal) ?? createKey()
        guard let publicKey = SecKeyCopyPublicKey(key) else {
            throw KeyRootError.PlatformUnavailable(detail: "no public key")
        }
        return try Self.eciesSeal(publicKey: publicKey, dataKey: dataKey)
    }

    public func unseal(sealed: Data) throws -> Data {
        guard sealed.count == Self.sealedLength else {
            throw KeyRootError.Malformed(detail: "sealed length \(sealed.count), expected \(Self.sealedLength)")
        }
        guard sealed.first == 0x04 else { throw KeyRootError.Malformed(detail: "not an uncompressed P-256 point") }
        let context = LAContext()
        context.localizedReason = reason
        guard let key = try privateKey(context: context, during: .unseal) else {
            throw KeyRootError.PlatformUnavailable(detail: "no platform key")
        }
        guard SecKeyIsAlgorithmSupported(key, .decrypt, Self.algorithm) else {
            throw KeyRootError.PlatformUnavailable(detail: "ECIES not supported by the key")
        }
        var error: Unmanaged<CFError>?
        guard let opened = SecKeyCreateDecryptedData(key, Self.algorithm, sealed as CFData, &error) else {
            throw KeyRootErrors.map(error, during: .unseal)
        }
        let length = CFDataGetLength(opened)
        guard let bytes = CFDataGetBytePtr(opened) else { throw KeyRootError.Unseal }
        defer { _ = memset_s(UnsafeMutableRawPointer(mutating: bytes), length, 0, length) }
        return Wipe.selfWiping(UnsafeRawBufferPointer(start: bytes, count: length))
    }

    public func hardwareBacked() -> Bool { true }

    public func destroyKey() throws {
        let status = SecItemDelete(keyQuery as CFDictionary)
        guard status == errSecSuccess || status == errSecItemNotFound else {
            throw KeyRootErrors.map(status: status, during: .seal)
        }
    }
}

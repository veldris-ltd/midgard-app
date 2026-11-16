import CryptoKit
import Foundation
import LocalAuthentication
import MidgardCore
import Security
import XCTest

@testable import MidgardKit

/// The platform key roots: error mapping, the software root's seal/unseal, the
/// Enclave's ECIES shape, the factory's choice, and wiping Swift's copies of the data
/// key. Unsigned `swift test` has no keychain access group, so nothing here touches
/// the Secure Enclave. [FR-C-2] [FR-C-9] [DD-6]
final class KeyRootTests: XCTestCase {
    // MARK: error mapping

    func testCancelsMapToUserCancelled() {
        XCTAssertEqual(KeyRootErrors.map(domain: NSOSStatusErrorDomain, code: Int(errSecUserCanceled), during: .unseal),
                       .UserCancelled)
        XCTAssertEqual(KeyRootErrors.map(domain: LAError.errorDomain, code: LAError.Code.userCancel.rawValue, during: .unseal),
                       .UserCancelled)
        XCTAssertEqual(KeyRootErrors.map(domain: LAError.errorDomain, code: LAError.Code.systemCancel.rawValue, during: .unseal),
                       .UserCancelled)
        XCTAssertEqual(KeyRootErrors.map(domain: LAError.errorDomain, code: LAError.Code.appCancel.rawValue, during: .unseal),
                       .UserCancelled)
        XCTAssertEqual(errSecUserCanceled, -128)
        XCTAssertEqual(LAError.Code.userCancel.rawValue, -2)
    }

    func testPlatformProblemsMapToUnavailable() {
        for code in [LAError.Code.passcodeNotSet, .biometryNotEnrolled, .biometryLockout, .authenticationFailed] {
            guard case .PlatformUnavailable = KeyRootErrors.map(domain: LAError.errorDomain, code: code.rawValue, during: .unseal)
            else { return XCTFail("LAError \(code.rawValue) should be PlatformUnavailable") }
        }
        for status in [errSecMissingEntitlement, errSecItemNotFound, errSecInteractionNotAllowed, errSecNotAvailable] {
            guard case .PlatformUnavailable = KeyRootErrors.map(status: status, during: .unseal)
            else { return XCTFail("OSStatus \(status) should be PlatformUnavailable") }
        }
    }

    func testOtherDecryptionFailuresAreUnsealAndSealFailuresAreUnavailable() {
        XCTAssertEqual(KeyRootErrors.map(domain: NSOSStatusErrorDomain, code: Int(errSecDecode), during: .unseal), .Unseal)
        guard case .PlatformUnavailable = KeyRootErrors.map(domain: NSOSStatusErrorDomain, code: Int(errSecDecode), during: .seal)
        else { return XCTFail("a failed seal is the platform's") }
    }

    func testMappedDetailNamesOnlyTheDomainAndCode() {
        guard case let .PlatformUnavailable(detail) = KeyRootErrors.map(status: errSecMissingEntitlement, during: .seal)
        else { return XCTFail() }
        XCTAssertEqual(detail, "\(NSOSStatusErrorDomain) \(errSecMissingEntitlement)")
    }

    func testAKeyRootErrorPassesThroughUnchanged() {
        XCTAssertEqual(KeyRootErrors.map(KeyRootError.Unseal, during: .unseal), .Unseal)
        let la = NSError(domain: LAError.errorDomain, code: LAError.Code.userCancel.rawValue)
        XCTAssertEqual(KeyRootErrors.map(la, during: .unseal), .UserCancelled)
    }

    // MARK: software key root

    private func softwareRoot(presence: UserPresence = AlwaysPresent(), secrets: InMemorySecretStore = InMemorySecretStore())
        -> SoftwareKeyRoot {
        SoftwareKeyRoot(reason: "test", label: "Software key (test)", secrets: secrets, presence: presence)
    }

    func testSoftwareRootSealsAndUnseals() throws {
        let presence = CountingPresence()
        let root = softwareRoot(presence: presence)
        let key = Data((0..<32).map { UInt8($0) })
        let sealed = try root.seal(dataKey: Data([UInt8](key)))
        XCTAssertEqual(sealed.count, SoftwareKeyRoot.sealedLength)
        XCTAssertNil(sealed.range(of: key), "the data key never appears in the sealed bytes")
        XCTAssertEqual(presence.count, 0, "sealing needs no user presence")
        XCTAssertEqual(try root.unseal(sealed: sealed), key)
        XCTAssertEqual(presence.count, 1, "unsealing asks once")
        XCTAssertFalse(root.hardwareBacked())
        XCTAssertEqual(root.custodyLabel, "Software key (test)")
    }

    func testSoftwareRootKeepsOneKeyUnderTheDeviceRootId() throws {
        let secrets = InMemorySecretStore()
        let root = softwareRoot(secrets: secrets)
        _ = try root.seal(dataKey: Data(repeating: 1, count: 32))
        _ = try root.seal(dataKey: Data(repeating: 2, count: 32))
        XCTAssertEqual(secrets.accounts(service: SoftwareKeyRoot.service), ["device-root"])
    }

    func testSoftwareRootRefusesMalformedAndTamperedBytes() throws {
        let root = softwareRoot()
        XCTAssertThrowsError(try root.unseal(sealed: Data(count: 10))) { error in
            guard case KeyRootError.Malformed = error else { return XCTFail("\(error)") }
        }
        var sealed = try root.seal(dataKey: Data(repeating: 3, count: 32))
        sealed[20] ^= 1
        XCTAssertThrowsError(try root.unseal(sealed: sealed)) { error in
            XCTAssertEqual(error as? KeyRootError, .Unseal)
        }
    }

    func testSoftwareRootCancelledPresenceIsUserCancelled() throws {
        let root = softwareRoot(presence: CancellingPresence())
        let sealed = try root.seal(dataKey: Data(repeating: 4, count: 32))
        XCTAssertThrowsError(try root.unseal(sealed: sealed)) { error in
            XCTAssertEqual(error as? KeyRootError, .UserCancelled)
        }
    }

    func testSoftwareRootAfterDestroyIsUnavailable() throws {
        let root = softwareRoot()
        let sealed = try root.seal(dataKey: Data(repeating: 5, count: 32))
        try root.destroyKey()
        XCTAssertThrowsError(try root.unseal(sealed: sealed)) { error in
            guard case KeyRootError.PlatformUnavailable = error else { return XCTFail("\(error)") }
        }
    }

    func testSoftwareRootWorksThroughTheClient() throws {
        let root = softwareRoot()
        let client = try MidgardClient(config: makeConfig(), keyRoot: root)
        let keys = try client.enrol()
        let info = try client.unlock(keys: keys, personaId: "k.rao")
        XCTAssertFalse(info.chips.custody.hardwareWrapped, "a software root is never reported as hardware")
        try client.lock()
    }

    // MARK: Secure Enclave key root (shape only: no Enclave in an unsigned test run)

    func testEnclaveEciesOfADataKeyIs113Bytes() throws {
        // The same algorithm on a software P-256 key: 65-byte ephemeral point, 32 bytes
        // of ciphertext, 16-byte tag.
        let attributes: [String: Any] = [kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
                                         kSecAttrKeySizeInBits as String: 256]
        var error: Unmanaged<CFError>?
        let priv = try XCTUnwrap(SecKeyCreateRandomKey(attributes as CFDictionary, &error))
        let pub = try XCTUnwrap(SecKeyCopyPublicKey(priv))
        let sealed = try SecureEnclaveKeyRoot.eciesSeal(publicKey: pub, dataKey: Data(repeating: 6, count: 32))
        XCTAssertEqual(sealed.count, 113)
        XCTAssertEqual(SecureEnclaveKeyRoot.sealedLength, 113)
        let opened = SecKeyCreateDecryptedData(priv, SecureEnclaveKeyRoot.algorithm, sealed as CFData, &error) as Data?
        XCTAssertEqual(opened, Data(repeating: 6, count: 32))
    }

    func testEnclaveRootRefusesAWrongLengthBeforeAnyPrompt() {
        let root = SecureEnclaveKeyRoot(reason: "test")
        XCTAssertThrowsError(try root.unseal(sealed: Data(count: 60))) { error in
            guard case KeyRootError.Malformed = error else { return XCTFail("\(error)") }
        }
        XCTAssertEqual(SecureEnclaveKeyRoot.keyId, "device-root")
    }

    // MARK: factory

    /// A sealed root blob as the core writes it (`MGSROOT1`), with a sealed data key of
    /// `sealedLength` bytes.
    private func sealedRoot(sealedLength: Int) -> Data {
        func field(_ bytes: Data) -> Data {
            var n = UInt32(bytes.count).littleEndian
            return Data(bytes: &n, count: 4) + bytes
        }
        var blob = Data("MGSROOT1".utf8)
        blob += field(Data("device-root".utf8))
        blob += Data([1])
        blob += field(Data(repeating: 2, count: 40))
        blob += field(Data(repeating: 4, count: sealedLength))
        blob += Data(repeating: 0, count: 12) + field(Data(repeating: 5, count: 48))
        return blob
    }

    private func platform(simulator: Bool = false, enclave: Bool = true, probe: OSStatus = errSecSuccess,
                          probes: ProbeCounter? = nil) -> KeyRootPlatform {
        KeyRootPlatform(simulator: simulator, enclaveAvailable: enclave) {
            probes?.count += 1
            return probe
        }
    }

    func testTheSealedRootsShapeNamesItsKind() {
        XCTAssertEqual(KeyRootKind(sealedRoot: sealedRoot(sealedLength: 113)), .enclave)
        XCTAssertEqual(KeyRootKind(sealedRoot: sealedRoot(sealedLength: 60)), .software)
        XCTAssertNil(KeyRootKind(sealedRoot: sealedRoot(sealedLength: 48)))
        XCTAssertNil(KeyRootKind(sealedRoot: Data("not a root".utf8)))
    }

    func testTheRealCoreWritesASoftwareShapedRootThroughTheSoftwareKeyRoot() throws {
        let client = try MidgardClient(config: makeConfig(), keyRoot: softwareRoot())
        XCTAssertEqual(KeyRootKind(sealedRoot: try client.enrol().root), .software)
    }

    func testAtEnrolmentTheProbeChoosesTheEnclaveOnlyWhenItCanStoreAKey() throws {
        let secrets = InMemorySecretStore()
        XCTAssertTrue(try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: nil, platform: platform())
                      is SecureEnclaveKeyRoot)
        let noEnclave = try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: nil,
                                                platform: platform(enclave: false))
        XCTAssertEqual(noEnclave.kind, .software)
        XCTAssertEqual(noEnclave.custodyLabel, "Software key (no Secure Enclave)")
        let simulator = try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: nil,
                                                platform: platform(simulator: true))
        XCTAssertEqual(simulator.custodyLabel, "Software key (simulator)")
        XCTAssertFalse(simulator.hardwareBacked())
    }

    func testAnUnsignedBuildCannotKeepKeysAndSaysSo() {
        XCTAssertThrowsError(try KeyRootFactory.make(reason: "t", secrets: InMemorySecretStore(), sealedRoot: nil,
                                                     platform: platform(probe: errSecMissingEntitlement))) { error in
            XCTAssertEqual(error as? KeyRootFactory.Failure, .unsignedBuild)
        }
        XCTAssertEqual(ErrorText.describe(KeyRootFactory.Failure.unsignedBuild),
                       "This build is not signed for the keychain, so it cannot keep keys. Use a signed build.")
    }

    func testThisUnsignedTestRunIsThatCase() {
        // `swift test` has no keychain-access-groups entitlement: the real probe write
        // is refused, and enrolment is refused rather than downgraded.
        XCTAssertThrowsError(try KeyRootFactory.make(reason: "t", secrets: InMemorySecretStore(), sealedRoot: nil)) { error in
            XCTAssertEqual(error as? KeyRootFactory.Failure, .unsignedBuild)
        }
    }

    func testALockedKeychainAtEnrolmentIsAnErrorNotASoftwareDowngrade() {
        XCTAssertThrowsError(try KeyRootFactory.make(reason: "t", secrets: InMemorySecretStore(), sealedRoot: nil,
                                                     platform: platform(probe: errSecInteractionNotAllowed))) { error in
            XCTAssertEqual(error as? KeyRootFactory.Failure, .keychainUnavailable(errSecInteractionNotAllowed))
        }
    }

    func testAnEnrolledEnclaveDeviceKeepsTheEnclaveWhenAProbeWouldFail() throws {
        let secrets = InMemorySecretStore()
        try KeyRootFactory.record(.enclave, secrets: secrets)
        let probes = ProbeCounter()
        let root = try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: sealedRoot(sealedLength: 113),
                                           platform: platform(probe: errSecInteractionNotAllowed, probes: probes))
        XCTAssertTrue(root is SecureEnclaveKeyRoot)
        XCTAssertEqual(probes.count, 0, "an enrolled device is never re-probed")
    }

    func testAStoredChoiceThatContradictsTheSealedRootIsRefused() throws {
        let secrets = InMemorySecretStore()
        try KeyRootFactory.record(.enclave, secrets: secrets)
        XCTAssertThrowsError(try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: sealedRoot(sealedLength: 60),
                                                     platform: platform())) { error in
            XCTAssertEqual(error as? KeyRootFactory.Failure, .mismatch(stored: .enclave, sealed: .software))
        }
        XCTAssertThrowsError(try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: Data("junk".utf8),
                                                     platform: platform())) { error in
            XCTAssertEqual(error as? KeyRootFactory.Failure, .mismatch(stored: .enclave, sealed: nil))
        }
    }

    func testWithoutAStoredChoiceAnEnrolledDeviceFollowsTheSealedRootAndRecordsIt() throws {
        let secrets = InMemorySecretStore()
        let probes = ProbeCounter()
        let root = try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: sealedRoot(sealedLength: 113),
                                           platform: platform(probe: errSecInteractionNotAllowed, probes: probes))
        XCTAssertTrue(root is SecureEnclaveKeyRoot)
        XCTAssertEqual(probes.count, 0)
        XCTAssertEqual(try KeyRootFactory.storedChoice(secrets: secrets), .enclave)
    }

    func testAnUnreadableChoiceIsAnError() {
        let secrets = InMemorySecretStore()
        secrets.failReads[KeyRootFactory.choiceService] = errSecInteractionNotAllowed
        XCTAssertThrowsError(try KeyRootFactory.make(reason: "t", secrets: secrets, sealedRoot: sealedRoot(sealedLength: 113),
                                                     platform: platform())) { error in
            XCTAssertEqual(error as? KeyRootFactory.Failure, .keychainUnavailable(errSecInteractionNotAllowed))
        }
    }

    // MARK: wiping

    func testWipeZeroesAMutableCopy() {
        var data = Data(repeating: 0xAB, count: 32)
        Wipe.wipe(&data)
        XCTAssertEqual(data, Data(count: 32))
    }

    func testWipeInPlaceZeroesTheSharedBuffer() {
        let data = Data(repeating: 0xCD, count: 32)
        Wipe.wipeInPlace(data)
        XCTAssertEqual(data, Data(count: 32))
    }

    func testSelfWipingDataHoldsTheBytes() {
        let bytes: [UInt8] = Array(repeating: 9, count: 32)
        let data = bytes.withUnsafeBytes { Wipe.selfWiping($0) }
        XCTAssertEqual(data, Data(bytes))
    }
}

struct AlwaysPresent: UserPresence {
    func confirm(reason: String) throws {}
}

final class CountingPresence: UserPresence {
    private(set) var count = 0
    func confirm(reason: String) throws { count += 1 }
}

struct CancellingPresence: UserPresence {
    func confirm(reason: String) throws {
        throw NSError(domain: LAError.errorDomain, code: LAError.Code.userCancel.rawValue)
    }
}

final class ProbeCounter {
    var count = 0
}

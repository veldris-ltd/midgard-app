import CryptoKit
import Foundation
import MidgardCore
import XCTest

@testable import MidgardKit

/// The shell's files: sealed keys and one sealed state file per persona, named by an
/// HMAC under a device-local keychain key, written atomically before the persona's
/// keychain generation moves. [FR-C-4] [FR-C-9] [AC-C-2]
final class StateStoreTests: XCTestCase {
    private var dir: URL!
    private var secrets: InMemorySecretStore!
    private var store: PersonaStateStore!

    override func setUpWithError() throws {
        dir = try temporaryDirectory()
        secrets = InMemorySecretStore()
        store = PersonaStateStore(directory: dir, secrets: secrets)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: dir)
    }

    private func files() throws -> [String] {
        try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted()
    }

    // MARK: atomic writes

    func testAtomicWriteReplacesTheFileAndLeavesNoTemporary() throws {
        let url = dir.appendingPathComponent("x.bin")
        try AtomicFile.write(Data("one".utf8), to: url)
        try AtomicFile.write(Data("two".utf8), to: url)
        XCTAssertEqual(try Data(contentsOf: url), Data("two".utf8))
        XCTAssertEqual(try files(), ["x.bin"])
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        XCTAssertEqual(mode?.int16Value, 0o600)
    }

    func testAFailedAtomicWriteLeavesTheOldFileAndNoTemporary() throws {
        let url = dir.appendingPathComponent("x.bin")
        try AtomicFile.write(Data("old".utf8), to: url)
        XCTAssertThrowsError(try AtomicFile.write(Data("new".utf8), to: dir.appendingPathComponent("missing/x.bin")))
        XCTAssertEqual(try Data(contentsOf: url), Data("old".utf8))
        XCTAssertEqual(try files(), ["x.bin"])
    }

    // MARK: names

    func testStateFilesAreNamedByAnHmacUnderADeviceLocalKey() throws {
        let name = try store.fileName(for: "k.rao")
        XCTAssertEqual(name.count, 64)
        XCTAssertTrue(name.allSatisfy { "0123456789abcdef".contains($0) })
        XCTAssertFalse(name.contains("rao"))
        let plainDigest = SHA256.hash(data: Data("k.rao".utf8)).map { String(format: "%02x", $0) }.joined()
        XCTAssertNotEqual(name, plainDigest, "a bare digest of a known persona id is reversible")
        XCTAssertNotEqual(name, try store.fileName(for: "a.holt"))
        // Stable for this device, different on another.
        XCTAssertEqual(name, try PersonaStateStore(directory: dir, secrets: secrets).fileName(for: "k.rao"))
        XCTAssertNotEqual(name, try PersonaStateStore(directory: dir, secrets: InMemorySecretStore()).fileName(for: "k.rao"))
    }

    func testTheDeviceKeyIs32BytesInTheKeychainAndNeverInAFile() throws {
        _ = try store.fileName(for: "k.rao")
        _ = try store.fileName(for: "a.holt")
        XCTAssertEqual(secrets.accounts(service: PersonaStateStore.deviceKeyService), ["state-names"])
        let key = try XCTUnwrap(secrets.read(service: PersonaStateStore.deviceKeyService, account: "state-names"))
        XCTAssertEqual(key.count, 32)
        try store.save(ExportedState(file: Data(repeating: 1, count: 64), generation: 1), persona: "k.rao")
        for name in try files() {
            let bytes = try Data(contentsOf: dir.appendingPathComponent(name))
            XCTAssertNil(bytes.range(of: key))
        }
    }

    func testNoPersonaIdIsStoredInPlaintext() throws {
        try store.save(ExportedState(file: Data(repeating: 2, count: 64), generation: 3), persona: "j.benjamin")
        for name in try files() { XCTAssertFalse(name.contains("benjamin")) }
        for account in secrets.accounts(service: PersonaStateStore.generationService) {
            XCTAssertFalse(account.contains("benjamin"))
            XCTAssertEqual(account, try store.fileName(for: "j.benjamin"))
        }
    }

    // MARK: generations

    func testTheFileIsOnDiskBeforeTheGenerationMoves() throws {
        try store.save(ExportedState(file: Data("first".utf8), generation: 1), persona: "k.rao")
        let url = try store.stateURL(for: "k.rao")
        var sawOnDisk: Data?
        secrets.onWrite = { service, _ in
            if service == PersonaStateStore.generationService { sawOnDisk = try? Data(contentsOf: url) }
        }
        try store.save(ExportedState(file: Data("second".utf8), generation: 2), persona: "k.rao")
        XCTAssertEqual(sawOnDisk, Data("second".utf8), "the generation is written only after the rename")
        XCTAssertEqual(try store.minGeneration(for: "k.rao"), 2)
    }

    func testAFailedFileWriteLeavesTheGenerationAlone() throws {
        try store.save(ExportedState(file: Data("first".utf8), generation: 4), persona: "k.rao")
        // Make the rename fail: the state file's path becomes a non-empty directory.
        let url = try store.stateURL(for: "k.rao")
        try FileManager.default.removeItem(at: url)
        try FileManager.default.createDirectory(at: url.appendingPathComponent("blocker"), withIntermediateDirectories: true)
        XCTAssertThrowsError(try store.save(ExportedState(file: Data("second".utf8), generation: 5), persona: "k.rao"))
        XCTAssertEqual(try store.minGeneration(for: "k.rao"), 4)
    }

    func testAFailedGenerationWriteIsReported() throws {
        secrets.failWrites = errSecInteractionNotAllowed
        let local = PersonaStateStore(directory: dir, secrets: secrets)
        XCTAssertThrowsError(try local.save(ExportedState(file: Data("x".utf8), generation: 1), persona: "k.rao"))
    }

    func testGenerationsAreKeptPerPersona() throws {
        try store.save(ExportedState(file: Data("a".utf8), generation: 5), persona: "k.rao")
        try store.save(ExportedState(file: Data("b".utf8), generation: 6), persona: "a.holt")
        XCTAssertEqual(try store.minGeneration(for: "k.rao"), 5)
        XCTAssertEqual(try store.minGeneration(for: "a.holt"), 6)
        XCTAssertEqual(try store.minGeneration(for: "k.osei"), 0)
        XCTAssertEqual(try store.load(persona: "k.rao"), Data("a".utf8))
        XCTAssertNil(try store.load(persona: "k.osei"))
    }

    func testDiscardDeletesTheFileButKeepsTheGeneration() throws {
        try store.save(ExportedState(file: Data("a".utf8), generation: 7), persona: "k.rao")
        try store.discard(persona: "k.rao")
        XCTAssertNil(try store.load(persona: "k.rao"))
        XCTAssertEqual(try store.minGeneration(for: "k.rao"), 7, "a restored older file must still be refused")
    }

    // MARK: sealed keys and reset

    func testSealedKeysRoundTrip() throws {
        XCTAssertNil(try store.loadKeys())
        let keys = SealedKeys(root: Data(repeating: 1, count: 40), deviceAudit: Data(repeating: 2, count: 80))
        try store.saveKeys(keys)
        XCTAssertEqual(try store.loadKeys(), keys)
        XCTAssertEqual(try files(), ["device-audit.sealed", "root.sealed"])
    }

    func testResetRemovesFilesGenerationsAndTheNamingKey() throws {
        try store.saveKeys(SealedKeys(root: Data([1]), deviceAudit: Data([2])))
        try store.save(ExportedState(file: Data("a".utf8), generation: 1), persona: "k.rao")
        try store.reset()
        XCTAssertEqual(try files(), [])
        XCTAssertEqual(secrets.accounts(service: PersonaStateStore.generationService), [])
        XCTAssertEqual(secrets.accounts(service: PersonaStateStore.deviceKeyService), [])
    }

    // MARK: the real keychain

    func testTheKeychainStoreReportsAMissingEntitlementInsteadOfWorkingAround() throws {
        let keychain = KeychainSecretStore()
        do {
            try keychain.write(Data([1]), service: "io.veldris.midgard.test", account: "probe")
            try keychain.delete(service: "io.veldris.midgard.test", account: "probe")
            throw XCTSkip("this run can use the data-protection keychain (a signed host)")
        } catch let error as SecretStoreError {
            XCTAssertEqual(error, .keychain(errSecMissingEntitlement))
        }
    }

    // MARK: paths

    func testTheStoreDirectoryIsPrivateAndOutOfBackups() throws {
        let url = try StorePaths.storeDirectory(base: dir)
        XCTAssertTrue(url.path.hasSuffix("Midgard/\(coreVersion())"))
        let values = try url.resourceValues(forKeys: [.isExcludedFromBackupKey])
        XCTAssertEqual(values.isExcludedFromBackup, true)
        #if os(macOS)
        let mode = try FileManager.default.attributesOfItem(atPath: url.path)[.posixPermissions] as? NSNumber
        XCTAssertEqual(mode?.int16Value, 0o700)
        #endif
    }
}

import Foundation
import MidgardCore
import XCTest

@testable import MidgardKit

/// The app's session flow against the real core and the committed dataset: enrol,
/// sign in off the main thread, offline queue kept across a relaunch, a rolled-back or
/// unreadable state file discarded and refetched, unsent changes lost on import told to
/// the user, lock, reset. The key root is the labelled software root over an in-memory
/// keychain (unsigned tests cannot use the Enclave). [FR-C-1] [FR-C-4] [FR-C-9] [AC-C-13]
@MainActor
final class AppModelTests: XCTestCase {
    private var dir: URL!
    private var secrets: InMemorySecretStore!
    private var keyRoot: RecordingKeyRoot!

    override func setUp() async throws {
        dir = try temporaryDirectory()
        secrets = InMemorySecretStore()
        keyRoot = RecordingKeyRoot(SoftwareKeyRoot(reason: "test", label: "Software key (test)",
                                                   secrets: secrets, presence: AlwaysPresent()))
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: dir)
    }

    private func environment(dataSource: DataSourceChoice = .bundled(releaseDataset()),
                             makeClient: AppEnvironment.ClientFactory? = nil) -> AppEnvironment {
        var env = AppEnvironment(dataSource: dataSource, edition: .release, storeDirectory: dir, secrets: secrets, keyRoot: keyRoot,
                                 device: DeviceInfo(ramMb: 8192, hasAccelerator: true), replicaId: "app-test")
        if let makeClient { env.makeClient = makeClient }
        return env
    }

    private func started(_ env: AppEnvironment? = nil) async -> AppModel {
        let model = AppModel(idleTimeout: 3600)
        await model.start(env ?? environment())
        return model
    }

    private func signedIn(_ id: String = "k.rao", env: AppEnvironment? = nil) async throws -> AppModel {
        let model = await started(env)
        if model.phase == .needsEnrolment { await model.enrol() }
        let persona = try XCTUnwrap(model.personas.first { $0.id == id })
        await model.signIn(persona)
        guard case .unlocked = model.phase else {
            XCTFail("not signed in: \(model.phase), \(model.message ?? "")")
            throw CancellationError()
        }
        return model
    }

    func testFirstLaunchEnrolsThenSignsInOffTheMainThread() async throws {
        let model = await started()
        XCTAssertEqual(model.phase, .needsEnrolment)
        XCTAssertEqual(model.personas.count, 4)
        await model.enrol()
        XCTAssertEqual(model.phase, .locked)
        XCTAssertNotNil(try PersonaStateStore(directory: dir, secrets: secrets).loadKeys())
        XCTAssertEqual(try KeyRootFactory.storedChoice(secrets: secrets), .software, "the choice is recorded at enrolment")
        XCTAssertEqual(keyRoot.unseals, 0, "enrolment never prompts")

        let persona = try XCTUnwrap(model.personas.first { $0.id == "k.rao" })
        await model.signIn(persona)
        XCTAssertEqual(model.phase, .unlocked(persona))
        XCTAssertEqual(keyRoot.unseals, 1)
        XCTAssertEqual(keyRoot.unsealedOnMainThread, false, "unlock waits on the prompt off the main thread")
        XCTAssertEqual(model.chips?.custody.hardwareWrapped, false)
        XCTAssertEqual(model.chipsPresentation?.custody.text, "SOFTWARE KEY")
    }

    func testARelaunchFindsTheDeviceEnrolled() async throws {
        _ = try await signedIn()
        let relaunched = await started()
        XCTAssertEqual(relaunched.phase, .locked)
    }

    func testTheDemoBannerIsAlwaysShown() async throws {
        let model = await started()
        XCTAssertEqual(model.dataSourceKind, .demo)
        XCTAssertEqual(model.banner, .demo)
        XCTAssertEqual(model.banner?.text, DemoBanner.notice)
        await model.enrol()
        XCTAssertEqual(model.banner?.text, DemoBanner.notice)
    }

    func testWithNoDataSourceTheLocalUserSignsInAndEveryActionIsRefused() async throws {
        let model = await started(environment(dataSource: .none))
        XCTAssertEqual(model.phase, .needsEnrolment)
        XCTAssertEqual(model.personas.map(\.id), ["local"])
        XCTAssertEqual(model.personas.first?.display, "Local user")
        XCTAssertEqual(model.personas.first?.clearanceLabel, "OPEN")
        XCTAssertEqual(model.collections, [])
        XCTAssertEqual(model.dataSourceKind, DataSourceKind.none)
        XCTAssertEqual(model.banner, .noSource)
        XCTAssertEqual(model.banner?.text, "No data source connected")

        await model.enrol()
        let local = try XCTUnwrap(model.personas.first)
        XCTAssertFalse(model.isUnlocked)
        await model.signIn(local)
        XCTAssertEqual(model.phase, .unlocked(local))
        XCTAssertTrue(model.isUnlocked)
        XCTAssertFalse(model.isOffline)
        let id = await model.requestAction(ActionInput(verb: "demo.ping", target: "midgard.audit/AUD-1", params: []))
        XCTAssertNil(id)
        XCTAssertEqual(model.message, "Not available: no data source connected.")
        XCTAssertEqual(model.chips?.sync.pending, 0, "nothing is queued")
        await model.setOffline(true)
        XCTAssertEqual(model.chipsPresentation?.sync.text, "OFFLINE · 0 QUEUED")
        XCTAssertTrue(model.isOffline)
        await model.lock()
        XCTAssertEqual(model.phase, .locked)
        XCTAssertFalse(model.isUnlocked)
        XCTAssertFalse(model.isOffline, "no chips while locked")
        XCTAssertEqual(model.banner, .noSource)
    }

    /// The shipped apps' mode (owner decision 22 Nov 2026): the release's own sample
    /// data, under the web consoles' demonstration notice, with the mock personas and
    /// Heimdall's authorise behaving as the web mock does.
    func testWithTheSampleDataTheMockPersonasSeeTheWebPicture() async throws {
        let model = await started(environment(dataSource: .sample))
        XCTAssertEqual(model.phase, .needsEnrolment)
        XCTAssertEqual(model.dataSourceKind, DataSourceKind.sample)
        XCTAssertEqual(model.banner, .demo)
        XCTAssertEqual(model.banner?.text,
                       "Demonstration data. This console runs on built-in sample data and is not connected to an enclosure.")
        XCTAssertEqual(model.personas.map(\.id), ["k.rao", "j.benjamin", "k.osei", "a.holt"])
        XCTAssertEqual(model.collections.count, 52)

        await model.enrol()
        let osei = try XCTUnwrap(model.personas.first { $0.id == "k.osei" })
        await model.signIn(osei)
        let tracks = await model.rows("heimdall.tracks")
        XCTAssertEqual(tracks.count, 5)
        XCTAssertTrue(tracks.allSatisfy { !$0.withheld })
        _ = await model.rows("heimdall.workflows")
        let id = await model.requestAction(ActionInput(verb: "heimdall.authoriseWorkflow", target: "heimdall.workflows/W-0912",
                                                       params: [KeyValue(key: "humanGateToken", value: "HG-test")]))
        XCTAssertNotNil(id)
        await model.syncNow()
        let status = await model.actionStatus(try XCTUnwrap(id))
        XCTAssertEqual(status?.state, .applied)
        XCTAssertEqual(status?.auditId, "D-4473")

        // A lower clearance sees the OPS-N tracks withheld, by level only.
        let rao = try XCTUnwrap(model.personas.first { $0.id == "k.rao" })
        await model.switchPersona(rao)
        let lower = await model.rows("heimdall.tracks")
        XCTAssertEqual(lower.filter(\.withheld).count, 4)
    }

    func testTheDataSourceIsReadFromTheInfoPlist() throws {
        XCTAssertEqual(try AppEnvironment.dataSourceChoice(in: try fakeBundle(["MidgardDataSource": "sample"])), DataSourceChoice.sample)
        XCTAssertEqual(try AppEnvironment.dataSourceChoice(in: try fakeBundle(["MidgardDataSource": "none"])), DataSourceChoice.none)
        let withDataset = try fakeBundle(["MidgardDataSource": "bundled"], dataset: true)
        guard case let .bundled(url) = try AppEnvironment.dataSourceChoice(in: withDataset) else {
            return XCTFail("expected bundled")
        }
        XCTAssertEqual(url.lastPathComponent, "\(coreVersion()).mgdemo")
        XCTAssertThrowsError(try AppEnvironment.dataSourceChoice(in: try fakeBundle(["MidgardDataSource": "bundled"]))) {
            guard case ClientError.NotFound = $0 else { return XCTFail("expected NotFound, got \($0)") }
        }
        for info: [String: Any] in [[:], ["MidgardDataSource": ""], ["MidgardDataSource": "None"],
                                    ["MidgardDataSource": "demo"], ["MidgardDataSource": 1]] {
            XCTAssertThrowsError(try AppEnvironment.dataSourceChoice(in: try fakeBundle(info)), "\(info)") {
                XCTAssertEqual($0 as? AppEnvironment.Failure, .misconfigured(AppEnvironment.dataSourceKey))
            }
        }
    }

    func testAMissingOrUnknownDataSourceIsAFailedState() async throws {
        for info: [String: Any] in [["MidgardEdition": "release"],
                                    ["MidgardDataSource": "local", "MidgardEdition": "release"]] {
            let bundle = try fakeBundle(info)
            let model = AppModel()
            await model.start {
                try AppEnvironment.live(reason: "test", device: DeviceInfo(ramMb: 8192, hasAccelerator: true), bundle: bundle)
            }
            let failure = AppEnvironment.Failure.misconfigured(AppEnvironment.dataSourceKey)
            XCTAssertEqual(model.phase, .failed(ErrorText.describe(failure)), "\(info)")
            XCTAssertTrue(ErrorText.describe(failure).contains("MidgardDataSource"))
            XCTAssertNil(model.banner, "no source is claimed")
        }
    }

    /// A bundle on disk with `info` as its Info.plist, and the release dataset in its
    /// resources when `dataset` is set.
    private func fakeBundle(_ info: [String: Any], dataset: Bool = false) throws -> Bundle {
        let url = dir.appendingPathComponent("\(UUID().uuidString).bundle", isDirectory: true)
        let contents = url.appendingPathComponent("Contents", isDirectory: true)
        let resources = contents.appendingPathComponent("Resources", isDirectory: true)
        try FileManager.default.createDirectory(at: resources, withIntermediateDirectories: true)
        var plist = info
        plist["CFBundleIdentifier"] = "test.midgard.\(UUID().uuidString)"
        try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
            .write(to: contents.appendingPathComponent("Info.plist"))
        if dataset {
            try FileManager.default.copyItem(at: releaseDataset(),
                                             to: resources.appendingPathComponent("\(coreVersion()).mgdemo"))
        }
        return try XCTUnwrap(Bundle(url: url))
    }

    func testOfflineShowsAIOffAndQueuedChangesSurviveARelaunch() async throws {
        let model = try await signedIn()
        let rows = await model.rows("midgard.objectTypes")
        let target = try XCTUnwrap(rows.first { !$0.withheld }?.id)
        await model.setOffline(true)
        XCTAssertEqual(model.chipsPresentation?.tier.text, "AI OFF")
        XCTAssertEqual(model.chipsPresentation?.sync.text, "OFFLINE · 0 QUEUED")
        let id = await model.requestAction(ActionInput(verb: "demo.ping", target: "midgard.objectTypes/\(target)", params: []))
        XCTAssertNotNil(id)
        XCTAssertEqual(model.chips?.sync.pending, 4)
        await model.lock()
        XCTAssertEqual(model.phase, .locked)

        // Relaunch: the same keychain and files, a fresh client.
        let again = try await signedIn()
        XCTAssertEqual(again.chips?.sync.pending, 4, "the queue was restored from the persona's state file")
        XCTAssertNil(again.message)
        await again.syncNow()
        XCTAssertEqual(again.chips?.sync.pending, 0)
    }

    func testEachPersonaKeepsItsOwnGeneration() async throws {
        let model = try await signedIn("k.rao")
        _ = await model.rows("midgard.objectTypes")
        await model.lock()
        let other = try await signedIn("a.holt")
        _ = await other.rows("midgard.objectTypes")
        _ = await other.rows("midgard.objectTypes")
        await other.lock()
        let store = PersonaStateStore(directory: dir, secrets: secrets)
        XCTAssertGreaterThan(try store.minGeneration(for: "k.rao"), 0)
        XCTAssertGreaterThan(try store.minGeneration(for: "a.holt"), 0)

        // k.rao's file still opens after a.holt wrote newer generations.
        let back = try await signedIn("k.rao")
        XCTAssertNil(back.message)
        XCTAssertNotNil(try store.load(persona: "k.rao"))
    }

    func testARolledBackStateFileIsDiscardedAndRefetched() async throws {
        let store = PersonaStateStore(directory: dir, secrets: secrets)
        let model = try await signedIn()
        _ = await model.rows("midgard.objectTypes")
        await model.syncNow()  // saves a state file
        let old = try XCTUnwrap(try store.load(persona: "k.rao"))
        await model.syncNow()  // a newer one
        await model.lock()
        try AtomicFile.write(old, to: try store.stateURL(for: "k.rao"))  // roll back

        let again = try await signedIn()
        XCTAssertEqual(again.message, AppModel.Notice.staleDiscarded)
        let rows = await again.rows("midgard.objectTypes")
        XCTAssertFalse(rows.isEmpty, "refetched online")
        // The discarded file was replaced by a fresh export above the old generation.
        let saved = try XCTUnwrap(try store.load(persona: "k.rao"))
        XCTAssertNotEqual(saved, old)
    }

    func testAnUnreadableStateFileIsDiscarded() async throws {
        let store = PersonaStateStore(directory: dir, secrets: secrets)
        let model = try await signedIn()
        await model.lock()
        try AtomicFile.write(Data("garbage".utf8), to: try store.stateURL(for: "k.rao"))
        let again = try await signedIn()
        XCTAssertEqual(again.message, AppModel.Notice.unreadableDiscarded)
        XCTAssertNil(try store.load(persona: "k.rao"), "the unreadable file is deleted")
    }

    func testWithNoFileTheNextExportStillSupersedesEveryGenerationSeen() async throws {
        let store = PersonaStateStore(directory: dir, secrets: secrets)
        let model = try await signedIn()
        for _ in 0..<3 { _ = await model.rows("midgard.objectTypes") }
        await model.lock()
        let kept = try store.minGeneration(for: "k.rao")
        XCTAssertGreaterThan(kept, 1)
        try store.discard(persona: "k.rao")

        let again = try await signedIn()
        _ = await again.rows("midgard.objectTypes")
        XCTAssertGreaterThan(try store.minGeneration(for: "k.rao"), kept,
                             "a generation kept in the keychain never goes backwards")
    }

    func testAnUnreadableGenerationFailsSignInInsteadOfImportingAtZero() async throws {
        let first = try await signedIn()
        await first.lock()
        let model = await started()
        secrets.failReads[PersonaStateStore.generationService] = errSecInteractionNotAllowed
        await model.signIn(try XCTUnwrap(model.personas.first { $0.id == "k.rao" }))
        XCTAssertEqual(model.phase, .locked)
        XCTAssertEqual(model.message, ErrorText.describe(SessionController.Failure.keychainUnavailable))
        secrets.failReads = [:]
        XCTAssertNotNil(try PersonaStateStore(directory: dir, secrets: secrets).load(persona: "k.rao"),
                        "the file is kept for the next attempt")
        await model.signIn(try XCTUnwrap(model.personas.first { $0.id == "k.rao" }))
        guard case .unlocked = model.phase else { return XCTFail("\(model.phase)") }
    }

    func testEnrolmentRecordsTheKindBeforeSavingTheKeys() async throws {
        let model = await started()
        secrets.failWritesTo[KeyRootFactory.choiceService] = errSecInteractionNotAllowed
        await model.enrol()
        XCTAssertEqual(model.phase, .needsEnrolment)
        XCTAssertEqual(model.message, ErrorText.describe(KeyRootFactory.Failure.keychainUnavailable(errSecInteractionNotAllowed)))
        XCTAssertNil(try PersonaStateStore(directory: dir, secrets: secrets).loadKeys(),
                     "no keys are saved without their recorded kind")
        secrets.failWritesTo = [:]
        await model.enrol()
        XCTAssertEqual(model.phase, .locked)
        XCTAssertEqual(try KeyRootFactory.storedChoice(secrets: secrets), .software)
    }

    func testAStateFileThatCannotBeReadNowIsKeptAndSignInCanBeRetried() async throws {
        let store = PersonaStateStore(directory: dir, secrets: secrets)
        let first = try await signedIn()
        _ = await first.rows("midgard.objectTypes")
        await first.lock()
        let url = try store.stateURL(for: "k.rao")
        let saved = try Data(contentsOf: url)
        XCTAssertEqual(chmod(url.path, 0), 0)
        defer { chmod(url.path, 0o600) }

        let model = await started()
        await model.signIn(try XCTUnwrap(model.personas.first { $0.id == "k.rao" }))
        XCTAssertEqual(model.phase, .locked)
        XCTAssertEqual(model.message, ErrorText.describe(SessionController.Failure.stateFileUnavailable))
        XCTAssertTrue(FileManager.default.fileExists(atPath: url.path), "not deleted")

        XCTAssertEqual(chmod(url.path, 0o600), 0)
        XCTAssertEqual(try Data(contentsOf: url), saved)
        await model.signIn(try XCTUnwrap(model.personas.first { $0.id == "k.rao" }))
        guard case .unlocked = model.phase else { return XCTFail("\(model.phase)") }
        XCTAssertNil(model.message)
    }

    func testUnsentChangesLostOnImportAreToldToTheUser() async throws {
        let first = try await signedIn()
        await first.lock()
        let env = environment { config, root in
            DroppingPendingClient(try MidgardClient(config: config, keyRoot: root))
        }
        let model = try await signedIn(env: env)
        XCTAssertEqual(model.message, AppModel.Notice.pendingDropped)
    }

    func testACancelledPromptLeavesTheAppLockedWithAMessage() async throws {
        let model = await started()
        await model.enrol()
        keyRoot.cancelNext = true
        await model.signIn(try XCTUnwrap(model.personas.first))
        XCTAssertEqual(model.phase, .locked)
        XCTAssertEqual(model.message, ErrorText.describe(ClientError.Cancelled))
    }

    func testSwitchingPersonaWithUnsentWorkIsRefusedAndExplained() async throws {
        let model = try await signedIn()
        let rows = await model.rows("midgard.objectTypes")
        let target = try XCTUnwrap(rows.first { !$0.withheld }?.id)
        await model.setOffline(true)
        _ = await model.requestAction(ActionInput(verb: "demo.ping", target: "midgard.objectTypes/\(target)", params: []))
        let holt = try XCTUnwrap(model.personas.first { $0.id == "a.holt" })
        await model.switchPersona(holt)
        guard case let .unlocked(current) = model.phase else { return XCTFail("\(model.phase)") }
        XCTAssertEqual(current.id, "k.rao")
        XCTAssertEqual(model.message, ErrorText.describe(ClientError.PendingOps(count: 4)))
        await model.setOffline(false)
        await model.syncNow()
        await model.switchPersona(holt)
        XCTAssertEqual(model.phase, .unlocked(holt))
    }

    func testResetDemoDeletesFilesKeysAndThePlatformKey() async throws {
        let model = try await signedIn()
        _ = await model.rows("midgard.objectTypes")
        await model.resetDemo()
        XCTAssertEqual(model.phase, .needsEnrolment)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path), [])
        XCTAssertEqual(secrets.accounts(service: SoftwareKeyRoot.service), [])
        XCTAssertEqual(secrets.accounts(service: PersonaStateStore.generationService), [])
        XCTAssertNil(try KeyRootFactory.storedChoice(secrets: secrets))
    }

    func testResetDestroysThePlatformKeyEvenWhenDeletingFilesFails() async throws {
        let model = try await signedIn()
        _ = await model.rows("midgard.objectTypes")
        secrets.failDeleteAll = errSecInteractionNotAllowed
        await model.resetDemo()
        XCTAssertEqual(keyRoot.destroyed, 1)
        XCTAssertNotNil(model.message, "the failure is reported")
    }

    func testAnEnvironmentThatCannotBeMadeIsAScreenState() async {
        let model = AppModel()
        await model.start { throw KeyRootFactory.Failure.unsignedBuild }
        XCTAssertEqual(model.phase, .failed(ErrorText.describe(KeyRootFactory.Failure.unsignedBuild)))
    }

    func testAMissingDatasetIsAScreenStateNotACrash() async {
        let env = environment(dataSource: .bundled(dir.appendingPathComponent("absent.mgdemo")))
        let model = AppModel()
        await model.start(env)
        guard case .failed = model.phase else { return XCTFail("\(model.phase)") }
        XCTAssertNil(model.banner, "no source was opened, so none is claimed")
    }

    func testATamperedBundledDatasetFailsWithNoBanner() async throws {
        var bytes = try Data(contentsOf: releaseDataset())
        bytes[bytes.count / 2] ^= 1
        let tampered = dir.appendingPathComponent("tampered.mgdemo")
        try bytes.write(to: tampered)
        let model = AppModel()
        await model.start(environment(dataSource: .bundled(tampered)))
        XCTAssertEqual(model.phase, .failed(ErrorText.describe(ClientError.SignatureFailure(detail: ""))))
        XCTAssertNil(model.banner, "the demonstration banner is not shown for data that did not verify")
        XCTAssertNil(model.dataSourceKind)
    }

    func testIdleExpiryLocksAndSaves() async throws {
        let model = try await signedIn()
        _ = await model.rows("midgard.objectTypes")
        await model.idleExpired()
        XCTAssertEqual(model.phase, .locked)
        XCTAssertNotNil(try PersonaStateStore(directory: dir, secrets: secrets).load(persona: "k.rao"))
    }
}

/// Records where `unseal` runs and how often; can answer one prompt as cancelled.
final class RecordingKeyRoot: PlatformKeyRoot {
    private let inner: PlatformKeyRoot
    private let lock = NSLock()
    private var _unseals = 0
    private var _onMain: Bool?
    var cancelNext = false

    init(_ inner: PlatformKeyRoot) { self.inner = inner }

    var unseals: Int { lock.lock(); defer { lock.unlock() }; return _unseals }
    var unsealedOnMainThread: Bool? { lock.lock(); defer { lock.unlock() }; return _onMain }
    var custodyLabel: String { inner.custodyLabel }
    var kind: KeyRootKind { inner.kind }

    func seal(dataKey: Data) throws -> Data { try inner.seal(dataKey: dataKey) }

    func unseal(sealed: Data) throws -> Data {
        lock.lock()
        _unseals += 1
        _onMain = Thread.isMainThread
        let cancel = cancelNext
        cancelNext = false
        lock.unlock()
        if cancel { throw KeyRootError.UserCancelled }
        return try inner.unseal(sealed: sealed)
    }

    func hardwareBacked() -> Bool { inner.hardwareBacked() }
    private(set) var destroyed = 0
    func destroyKey() throws {
        destroyed += 1
        try inner.destroyKey()
    }
}

/// A real client whose imports report that unsent changes were dropped (the core does
/// this when the persona's clearance no longer covers the saved queue).
final class DroppingPendingClient: MidgardClientProtocol {
    private let inner: MidgardClient
    init(_ inner: MidgardClient) { self.inner = inner }

    func importState(file: Data, minGeneration: UInt64) throws -> ImportReport {
        var report = try inner.importState(file: file, minGeneration: minGeneration)
        report.pendingDropped = true
        return report
    }

    func actionStatus(actionId: String) throws -> ActionStatus { try inner.actionStatus(actionId: actionId) }
    func collectionRows(collection: String) throws -> [ConsoleRow] { try inner.collectionRows(collection: collection) }
    func collections() -> [CollectionInfo] { inner.collections() }
    func dataSourceKind() -> DataSourceKind { inner.dataSourceKind() }
    func enrol() throws -> SealedKeys { try inner.enrol() }
    func exportState() throws -> ExportedState { try inner.exportState() }
    func isLocked() -> Bool { inner.isLocked() }
    func lock() throws { try inner.lock() }
    func personas() -> [PersonaInfo] { inner.personas() }
    func requestAction(action: ActionInput) throws -> String { try inner.requestAction(action: action) }
    func resetDemo() throws { try inner.resetDemo() }
    func sessionPersona() throws -> PersonaInfo { try inner.sessionPersona() }
    func setOffline(offline: Bool) throws -> StatusChips { try inner.setOffline(offline: offline) }
    func statusChips() throws -> StatusChips { try inner.statusChips() }
    func switchPersona(personaId: String) throws -> SessionInfo { try inner.switchPersona(personaId: personaId) }
    func syncNow() throws -> SyncReport { try inner.syncNow() }
    func unlock(keys: SealedKeys, personaId: String) throws -> SessionInfo { try inner.unlock(keys: keys, personaId: personaId) }
}

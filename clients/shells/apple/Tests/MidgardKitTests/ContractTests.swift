import MidgardCore
import XCTest

@testable import MidgardKit

/// The Swift–Rust contract: every exported binding is called with real data (the
/// committed, signed demo dataset). tools/check-contract.sh fails if a binding is
/// added without a call here. [FR-C-1] [DD-6] [AC-C-13]
final class ContractTests: XCTestCase {
    func testEveryBinding() throws {
        XCTAssertFalse(coreVersion().isEmpty)
        let root = TestKeyRoot()
        let client = try MidgardClient(config: makeConfig(), keyRoot: root)

        // Locked: the sign-in screen's lists come without a session.
        XCTAssertEqual(client.dataSourceKind(), .demo)
        XCTAssertEqual(client.personas().count, 4)
        XCTAssertTrue(client.isLocked())
        XCTAssertTrue(client.collections().contains { $0.name == "midgard.objectTypes" })
        XCTAssertThrowsError(try client.statusChips()) { error in
            guard case ClientError.Locked = error else { return XCTFail("expected Locked, got \(error)") }
        }

        // Enrol never prompts; unlock prompts once.
        let keys = try client.enrol()
        XCTAssertEqual(root.prompts, 0)
        XCTAssertFalse(keys.root.isEmpty)
        XCTAssertFalse(keys.deviceAudit.isEmpty)
        let info = try client.unlock(keys: keys, personaId: "k.rao")
        XCTAssertEqual(root.prompts, 1)
        XCTAssertEqual(info.persona.id, "k.rao")
        XCTAssertEqual(info.persona.clearanceLabel, "RESTRICTED")
        XCTAssertFalse(client.isLocked())
        XCTAssertEqual(try client.sessionPersona().id, "k.rao")

        // A software key root is reported truthfully.
        XCTAssertFalse(info.chips.custody.hardwareWrapped)
        XCTAssertEqual(info.chips.custody.floor, "NonExtractable")
        XCTAssertEqual(try client.statusChips().sync.phase, .synced)

        // Withheld rows carry their level and nothing else.
        let rows = try client.collectionRows(collection: "midgard.objectTypes")
        let withheld = rows.filter(\.withheld)
        XCTAssertFalse(withheld.isEmpty)
        for row in withheld {
            XCTAssertNil(row.id)
            XCTAssertNil(row.item)
            XCTAssertNil(row.signature)
        }
        let target = try XCTUnwrap(rows.first { !$0.withheld }?.id)

        // Offline: AI off, actions queue, sync refused.
        let offline = try client.setOffline(offline: true)
        XCTAssertEqual(offline.sync.phase, .offline)
        XCTAssertEqual(offline.tier.tier, .disabled)
        let actionId = try client.requestAction(action: ActionInput(
            verb: "demo.ping", target: "midgard.objectTypes/\(target)", params: [KeyValue(key: "note", value: "contract")]))
        XCTAssertEqual(try client.actionStatus(actionId: actionId).state, .queued)
        XCTAssertThrowsError(try client.syncNow()) { error in
            guard case ClientError.Offline = error else { return XCTFail("expected Offline, got \(error)") }
        }

        // Online again: the queue syncs once; the device signs its own audit row.
        _ = try client.setOffline(offline: false)
        let sync = try client.syncNow()
        XCTAssertEqual(sync.pushed, 4)
        let status = try client.actionStatus(actionId: actionId)
        XCTAssertEqual(status.state, .applied)
        let auditId = try XCTUnwrap(status.auditId)
        let audit = try client.collectionRows(collection: "midgard.audit")
        XCTAssertEqual(audit.first { $0.id == auditId }?.signature, .device)
        XCTAssertTrue(audit.contains { $0.signature == .verified })

        // The state file round-trips; an older one is a rollback. (A failed import empties
        // the session's cache and device record, so it comes after the signed row.)
        let first = try client.exportState()
        let second = try client.exportState()
        XCTAssertGreaterThan(second.generation, first.generation)
        let report = try client.importState(file: second.file, minGeneration: second.generation)
        XCTAssertFalse(report.pendingDropped)
        XCTAssertEqual(report.generation, second.generation)
        XCTAssertThrowsError(try client.importState(file: first.file, minGeneration: second.generation)) { error in
            guard case let ClientError.Stale(found, min) = error else { return XCTFail("expected Stale, got \(error)") }
            XCTAssertEqual(found, first.generation)
            XCTAssertEqual(min, second.generation)
        }

        // Switch, lock, reset.
        XCTAssertEqual(try client.switchPersona(personaId: "a.holt").persona.id, "a.holt")
        try client.lock()
        XCTAssertTrue(client.isLocked())
        XCTAssertThrowsError(try client.collectionRows(collection: "midgard.objectTypes")) { error in
            guard case ClientError.Locked = error else { return XCTFail("expected Locked, got \(error)") }
        }
        try client.resetDemo()
        XCTAssertTrue(client.isLocked())
    }

    /// With no dataset path: one `local` persona, no collections, every action refused
    /// and nothing queued.
    func testNoDataSource() throws {
        let client = try MidgardClient(config: makeConfig(dataset: nil), keyRoot: TestKeyRoot())
        XCTAssertEqual(client.dataSourceKind(), DataSourceKind.none)
        XCTAssertEqual(client.personas().map(\.id), ["local"])
        XCTAssertEqual(client.collections(), [])
        _ = try client.unlock(keys: try client.enrol(), personaId: "local")
        XCTAssertThrowsError(try client.requestAction(action: ActionInput(verb: "demo.ping", target: "midgard.audit/AUD-1",
                                                                          params: []))) { error in
            guard case let ClientError.Refused(reason) = error else { return XCTFail("expected Refused, got \(error)") }
            XCTAssertEqual(reason, "no data source connected")
        }
        XCTAssertEqual(try client.statusChips().sync.pending, 0)
        try client.resetDemo()
        XCTAssertEqual(client.dataSourceKind(), DataSourceKind.none)
    }

    /// Over the release's sample data: the mock personas and collections, and the
    /// sample workspace's kind.
    func testSampleData() throws {
        let client = try MidgardClient(config: makeConfig(source: .sample), keyRoot: TestKeyRoot())
        XCTAssertEqual(client.dataSourceKind(), DataSourceKind.sample)
        XCTAssertEqual(client.personas().map(\.id), ["k.rao", "j.benjamin", "k.osei", "a.holt"])
        XCTAssertTrue(client.collections().contains { $0.name == "heimdall.tracks" && $0.rowType == "HeimdallTrack" })
        _ = try client.unlock(keys: try client.enrol(), personaId: "k.osei")
        XCTAssertEqual(try client.collectionRows(collection: "heimdall.tracks").count, 5)
        try client.resetDemo()
        XCTAssertEqual(client.dataSourceKind(), DataSourceKind.sample)
    }

    /// The key root is implemented (not called) here; it is exercised through the
    /// client: `seal` at enrolment, `unseal` at unlock, `hardwareBacked` for the chip.
    func testKeyRootCallbacksAreReachedThroughTheClient() throws {
        let root = TestKeyRoot()
        let sealed = try root.seal(dataKey: Data(repeating: 7, count: 32))
        XCTAssertEqual(try root.unseal(sealed: sealed), Data(repeating: 7, count: 32))
        XCTAssertFalse(root.hardwareBacked())

        let client = try MidgardClient(config: makeConfig(), keyRoot: root)
        let keys = try client.enrol()
        root.cancelNext = true
        XCTAssertThrowsError(try client.unlock(keys: keys, personaId: "k.rao")) { error in
            guard case ClientError.Cancelled = error else { return XCTFail("expected Cancelled, got \(error)") }
        }
        XCTAssertTrue(client.isLocked())
    }
}

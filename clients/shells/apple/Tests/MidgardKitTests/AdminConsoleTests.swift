import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Admin's screen models over the release's sample data, against the real core: the
/// overview and the list screens with their withheld rows, the dual-control queue (a
/// requester's own approval denied and audited, a second person's granted, a denial on
/// screen), the config lifecycle (simulate, commit, roll back; offline the simulation
/// is not run), the ledger search and its live lineage, break-glass (requested active,
/// withheld below its marking), and the chrome. The screens render in both modes.
/// [FR-C-1] [FR-C-4] [AC-C-13] [AC-AD-4] [AC-AD-6] [AC-AD-7] [FR-AD-8]
@MainActor
final class AdminConsoleTests: XCTestCase {
    private var dir: URL!

    override func setUp() async throws {
        dir = try temporaryDirectory()
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: dir)
    }

    private func signedIn(_ id: String) async throws -> AppModel {
        let secrets = InMemorySecretStore()
        let keyRoot = SoftwareKeyRoot(reason: "test", label: "Software key (test)", secrets: secrets,
                                      presence: AlwaysPresent())
        let app = AppModel(idleTimeout: 3600)
        await app.start(AppEnvironment(dataSource: .sample, edition: .release, storeDirectory: dir, secrets: secrets,
                                       keyRoot: keyRoot, device: DeviceInfo(ramMb: 8192, hasAccelerator: true),
                                       replicaId: "admin-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    private func switchTo(_ app: AppModel, _ id: String) async throws {
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == id }))
        XCTAssertEqual(app.signedInPersona?.id, id)
    }

    func testTheOverviewAndListScreensAsTheWebShowsThem() async throws {
        let app = try await signedIn("k.osei")
        let overview = MidgardListModel.overview(app)
        let roles = MidgardListModel.roles(app)
        let bindings = MidgardListModel.bindings(app)
        let registry = MidgardListModel.registry(app)
        let copilot = MidgardListModel.copilot(app)
        await overview.load()
        await roles.load()
        await bindings.load()
        await registry.load()
        await copilot.load()
        let o = try XCTUnwrap(overview.rows.first?.item)
        XCTAssertEqual(o.subsystems.map(\.name), ["Identity & Access", "Governance & Audit", "Crypto & KMS", "Object Fabric",
                                                  "Mimir", "Bifrost", "Observability"])
        XCTAssertEqual(o.subsystems[4].statusLabel, "Eval gate")
        XCTAssertEqual(Int(o.pendingApprovals), 3)
        XCTAssertEqual(Int(o.superAdminRoles), 0)
        XCTAssertEqual(roles.rows.map(\.item.name), ["IAM-Admin", "KMS-Officer", "Model-Steward", "Release-Manager", "Auditor"])
        XCTAssertEqual(AdminIamScreen.canCannot(roles.rows[4].item).map(\.1), [true, true, false])
        XCTAssertEqual(AdminIamScreen.canCannot(roles.rows[0].item).first?.0, "Administer users · groups · bindings")
        XCTAssertEqual(bindings.rows.count, 4)
        XCTAssertEqual(registry.rows.filter { $0.item.state == "pending" }.map(\.item.name), ["mistral-small-3.3-rc1"])
        XCTAssertEqual(AdminRegistryScreen.lifecycle(registry.rows[3].item).map(\.2), [false, false, false])
        XCTAssertEqual(copilot.rows.map(\.item.variant), ["message", "proposal", "message", "refusal"])
        XCTAssertEqual(copilot.rows[3].item.refusalCode, "CONTROL_BYPASS_REFUSED")

        // A. Holt (internal) reads Admin's internal rows; the restricted ledger is withheld.
        try await switchTo(app, "a.holt")
        await roles.load()
        XCTAssertEqual(roles.rows.count, 5)
        let ledger = AdminLedgerModel(app: app)
        await ledger.load()
        XCTAssertEqual(ledger.rows.count, 0)
        XCTAssertEqual(ledger.withheld, 5)
        XCTAssertEqual(ledger.liveWithheld, 3)
    }

    func testSelfApprovalIsDeniedAndASecondPersonApproves() async throws {
        // J. Benjamin requested A-9110: approving it himself is denied and audited.
        let app = try await signedIn("j.benjamin")
        let model = AdminApprovalsModel(app: app)
        await model.load()
        XCTAssertEqual(model.items.map(\.id), ["A-9110", "A-9109", "A-9108", "A-9107"])
        XCTAssertEqual(model.awaiting, 3)
        await model.approve(model.items[0])
        XCTAssertEqual(model.items[0].state, "denied")
        XCTAssertEqual(model.items[0].deniedReason, "DENIED · SELF-APPROVAL BLOCKED (AC-AD-4)")
        XCTAssertEqual(model.tail.count, 4)
        XCTAssertEqual(model.tail[1],
                       "A-9111  denied   Rotate enclosure-root-03  ·  separation-of-duties: requester cannot approve own request")
        XCTAssertEqual(model.tail[2], AdminApprovalsModel.initialTail[1])

        // The denial is in the ledger's live lineage.
        let ledger = AdminLedgerModel(app: app)
        await ledger.load()
        XCTAssertEqual(ledger.live.first?.item.id, "A-9111")
        XCTAssertEqual(ledger.live.first?.item.action, "refused · admin.approve")

        // K. Osei, a second person, approves A-9109.
        try await switchTo(app, "k.osei")
        await model.load()
        let a9109 = try XCTUnwrap(model.items.first { $0.id == "A-9109" })
        await model.approve(a9109)
        let after = try XCTUnwrap(model.items.first { $0.id == "A-9109" })
        XCTAssertEqual(after.state, "approved")
        XCTAssertEqual(after.approver, "k.osei")
        XCTAssertEqual(model.tail[1], "A-9112  approved Escalate m.silva → release-manager  ·  granted by k.osei · token 10m · single-use")
        XCTAssertNil(model.notice)

        // Deny is the second controller's own decision, on screen only.
        let a9108 = try XCTUnwrap(model.items.first { $0.id == "A-9108" })
        model.deny(a9108)
        XCTAssertEqual(model.items.first { $0.id == "A-9108" }?.deniedReason, "DENIED · SECOND CONTROLLER REJECTED")
        XCTAssertEqual(model.awaiting, 0)
        await model.load()
        XCTAssertEqual(model.items.first { $0.id == "A-9108" }?.state, "denied", "kept across a reload")
    }

    func testAConfigChangeIsSimulatedCommittedAndRolledBack() async throws {
        let app = try await signedIn("k.osei")
        let model = AdminConfigModel(app: app)
        await model.load()
        XCTAssertEqual(model.commits.map(\.item.id), ["CFG-2214", "CFG-2213", "CFG-2212", "CFG-2209"])
        await model.simulate()
        XCTAssertEqual(model.simulation?.grants, ["datasets: [flight_tracks_eu, maintenance_events]", "expiry: 2026-10-06 (90d)"])
        XCTAssertEqual(model.simulation?.revokes, ["datasets: [flight_tracks_eu]"])
        await model.commit()
        XCTAssertEqual(model.committedAt, "v482")
        XCTAssertEqual(model.current?.state, "committed")
        await model.rollback()
        XCTAssertEqual(model.rolledBackAt, "v483")
        XCTAssertNil(model.committedAt)
        XCTAssertEqual(model.current?.state, "rolled-back")
        model.selected = 1
        XCTAssertNil(model.simulation)
        XCTAssertNil(model.rolledBackAt)

        // Offline the simulation, a computed read, is not run; a commit is queued.
        await app.setOffline(true)
        await model.simulate()
        XCTAssertEqual(model.error, MidgardOutcome.offlineRead)
        await model.commit()
        XCTAssertEqual(model.error, "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE")
        XCTAssertNil(model.committedAt)
    }

    func testTheLedgerSearchesWithTheMocksTokens() async throws {
        let app = try await signedIn("k.osei")
        let model = AdminLedgerModel(app: app)
        await model.load()
        XCTAssertEqual(model.rows.count, 5)
        XCTAssertEqual(model.current?.id, "A-9107", "the web selects the second entry")
        XCTAssertEqual(model.live.map(\.item.id), ["B-0331", "A-9107", "A-9108"])
        model.draft = "actor:k.osei subsystem:kms since:24h"
        await model.search()
        XCTAssertEqual(model.rows.map(\.id), ["A-9108", "B-0331"])
        XCTAssertEqual(model.current?.id, "A-9108")
        XCTAssertEqual(model.live.first?.item.action, "applied · admin.searchLedger")
        XCTAssertEqual(model.live.first?.item.resource, "admin.ledger/*")
        model.filter = "denials"
        XCTAssertEqual(model.rows.count, 2, "the tags select, as on the web; they do not filter")
    }

    func testBreakGlassOpensActiveAndIsWithheldBelowItsMarking() async throws {
        let app = try await signedIn("j.benjamin")
        let model = AdminBreakGlassModel(app: app)
        await model.load()
        XCTAssertEqual(model.phase, "review")
        XCTAssertEqual(model.session?.holder, "k.osei")
        model.phase = "idle"
        await model.request()
        XCTAssertEqual(model.phase, "active")
        XCTAssertEqual(model.session?.holder, "j.benjamin")
        XCTAssertEqual(model.session?.remaining, "00:59:59")
        XCTAssertEqual(model.session?.justification, AdminBreakGlassModel.reason)

        try await switchTo(app, "k.rao")
        let below = AdminBreakGlassModel(app: app)
        await below.load()
        XCTAssertNil(below.session)
        XCTAssertEqual(below.withheld, 1)
        await below.request()
        XCTAssertNotNil(below.notice, "a persona below the state cannot request it")
    }

    func testTheChromeIsTheWebsAndCoversEveryScreen() {
        XCTAssertEqual(AdminChrome.screens, ["Overview", "Approvals", "IAM", "Config", "Registry", "Ledger", "Break-Glass",
                                             "Copilot"])
        XCTAssertEqual(AdminChrome.screens.sorted(), ConsoleCatalog.consoles[1].screens.sorted())
        XCTAssertEqual(AdminChrome.sections.map(\.title), ["OVERSEE", "ADMINISTER", "EMERGENCY", "ASSIST"])
        XCTAssertEqual(AdminChrome.item(for: "Config")?.asideWidth, 360)
        XCTAssertEqual(AdminChrome.item(for: "Ledger")?.context, "ADMIN / AUDIT LEDGER")
    }

    func testEveryScreenRendersInBothModes() async throws {
        let app = try await signedIn("k.osei")
        for screen in ConsoleCatalog.consoles[1].screens {
            for compact in [false, true] {
                for scheme in [ColorScheme.dark, .light] {
                    let view = AdminScreen(app: app, persona: "k.osei", screen: screen, compact: compact) { _, _ in }
                        .frame(width: compact ? 402 : 1100, height: 800)
                        .environment(\.colorScheme, scheme)
                    let renderer = ImageRenderer(content: view)
                    XCTAssertNotNil(renderer.cgImage, "\(screen) did not render")
                }
            }
        }
    }
}

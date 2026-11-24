import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Bifrost's screen models, over the release's sample data, against the real core: the
/// lists (artefacts, channels, bundles, fleet), the Release Console (Promote stepping
/// the broad ring to 100 % through the core, the gate WATCH then PASS, each step's audit
/// id; refused for a persona without the release-manager role; queued offline), the
/// Rollback centre (a plan executed through the core with its audit id in the log; the
/// drill played into the log; refused without the role), and every screen rendered in
/// both modes. [FR-C-1] [FR-C-4] [FR-B-2] [FR-B-3] [FR-B-4] [FR-B-5] [AC-C-13]
@MainActor
final class BifrostConsoleTests: XCTestCase {
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
                                       replicaId: "bifrost-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    func testTheListsShowTheSample() async throws {
        let app = try await signedIn("k.rao")
        let artefacts = MidgardListModel.artefacts(app)
        await artefacts.load()
        XCTAssertEqual(artefacts.rows.map(\.item.name),
                       ["midgard-suite", "mimir-weights-mistral", "forge-connectors", "heimdall-fusion"])
        XCTAssertEqual(artefacts.rows[3].item.state, "danger")
        XCTAssertEqual(artefacts.rows[0].marking.label, "INTERNAL")
        XCTAssertEqual(artefacts.withheld, 0)
        let channels = MidgardListModel.channels(app)
        await channels.load()
        XCTAssertEqual(channels.rows.map(\.item.name), ["stable", "beta", "canary"])
        XCTAssertEqual(channels.rows.reduce(0) { $0 + $1.item.subscribers }, 73)
        let bundles = MidgardListModel.bundles(app)
        await bundles.load()
        XCTAssertEqual(bundles.rows.filter { $0.item.state == "gated" }.map(\.id), ["B-2050-07A"])
        let fleet = MidgardListModel.fleet(app)
        await fleet.load()
        XCTAssertEqual(fleet.rows.count, 5)
        XCTAssertEqual(fleet.rows.filter { $0.item.drift == "none" }.count, 2)
        XCTAssertEqual(bifrostPercent(done: 21, total: 48), 44)
        XCTAssertEqual(bifrostPercent(done: 1, total: 8), 13)
        XCTAssertEqual(bifrostCount(73), "73")
    }

    func testPromoteStepsTheBroadRingThroughTheCore() async throws {
        let app = try await signedIn("a.holt")
        let model = BifrostReleaseModel(app: app)
        model.stepDelay = 0
        await model.load()
        XCTAssertEqual(model.release?.id, "REL-2050")
        XCTAssertEqual(model.gate, "watch")
        XCTAssertEqual(model.rings.map(\.item.stage), ["canary", "staging", "broad", "air-gap"])
        XCTAssertEqual("\(model.rolled)/\(model.capacity)", "37/73")
        XCTAssertEqual(model.fleet.count, 5)
        XCTAssertNil(model.promoteDisabled)

        await model.promote()
        XCTAssertNil(model.message)
        XCTAssertEqual(model.steps.map(\.auditId), ["AUD-B-1044", "AUD-B-1045", "AUD-B-1046", "AUD-B-1047"])
        XCTAssertEqual(model.steps.map(\.step), [
            "Promote · REL-2050 → ring broad (60%)", "Promote · REL-2050 → ring broad (77%)",
            "Promote · REL-2050 → ring broad (94%)", "Promote · REL-2050 → ring broad (100%)",
        ])
        XCTAssertEqual(model.lastAudit, "AUD-B-1047")
        XCTAssertEqual(model.gate, "pass")
        XCTAssertEqual(model.rings[2].item.done, 48)
        XCTAssertEqual("\(model.rolled)/\(model.capacity)", "64/73")
        XCTAssertFalse(model.promoting)

        // The core kept the ring rolled: a reload reads it back so.
        await model.load()
        XCTAssertEqual(model.rings[2].item.done, 48)
        // Promote again: one step, already at 100 %.
        await model.promote()
        XCTAssertEqual(model.lastAudit, "AUD-B-1048")
        XCTAssertEqual(model.gate, "pass")
    }

    func testPromoteIsRefusedWithoutTheRoleAndQueuedOffline() async throws {
        let app = try await signedIn("k.osei")
        let model = BifrostReleaseModel(app: app)
        model.stepDelay = 0
        await model.load()
        await model.promote()
        XCTAssertEqual(model.message, "Role release-manager required to promote REL-2050")
        XCTAssertNil(model.lastAudit)
        XCTAssertEqual(model.rings[2].item.done, 21, "nothing rolled")

        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "a.holt" }))
        await model.load()
        await app.setOffline(true)
        await model.promote()
        XCTAssertEqual(model.message, "Queued on this device: the step is sent when the enclosure is reachable.")
        XCTAssertNil(model.lastAudit)
    }

    func testARollbackRunsThroughTheCoreAndTheDrillPlays() async throws {
        let app = try await signedIn("a.holt")
        let model = BifrostRollbackModel(app: app)
        model.drillPace = 0
        await model.load()
        XCTAssertEqual(model.rows.map(\.id), ["RB-REL-2050", "RB-MIMIR-P7", "RB-FORGE-42"])
        XCTAssertFalse(model.gated)
        XCTAssertEqual(model.triggers, ["p99 latency > 250ms · 3 min", "error rate > 0.1% · 3 min"])
        model.select(2)
        XCTAssertEqual(model.triggers, ["manual only"])
        model.select(1)
        XCTAssertEqual(model.triggers, ["eval pass < gate"])

        await model.runDrill()
        XCTAssertEqual(model.log.count, 2 + 1 + BifrostRollbackModel.drill.count)
        XCTAssertEqual(model.log.last, "# drill complete · 41s to healthy · audit #R-2214")
        XCTAssertFalse(model.drilling)

        model.select(0)
        await model.execute()
        XCTAssertNil(model.message)
        XCTAssertEqual(model.executedAudit, "AUD-B-1044")
        XCTAssertEqual(model.log.suffix(2), ["> bifrost rollback execute --plan RB-REL-2050",
                                              "# rollback complete · health-gated · audit AUD-B-1044"])

        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "j.benjamin" }))
        await model.load()
        await model.execute()
        XCTAssertEqual(model.message, "Role release-manager required to roll back RB-REL-2050")
    }

    func testEveryScreenRendersInBothModes() async throws {
        MGFonts.registerAll()
        let app = try await signedIn("a.holt")
        for screen in BifrostChrome.screens {
            for scheme in [ColorScheme.dark, .light] {
                for compact in [false, true] {
                    let view = BifrostScreen(app: app, persona: "a.holt", screen: screen, compact: compact)
                        .environment(\.colorScheme, scheme)
                        .frame(width: compact ? 402 : 1170, height: compact ? 874 : 900)
                    XCTAssertNotNil(ImageRenderer(content: view).cgImage, "\(screen) \(scheme) \(compact)")
                }
            }
        }
        XCTAssertEqual(Set(BifrostChrome.screens), Set(ConsoleCatalog.console(number: 6).screens))
    }
}

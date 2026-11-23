import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Heimdall's screen models after the Operating Picture, over the release's sample data,
/// against the real core: Entities (the web's filters, withheld rows by count), Feeds
/// (the quarantined bridge, the user's own feeds as unverified), Picture Sync, Workflows
/// (the human gate refused without a token, authorised with one and audited, rejected on
/// screen only, queued offline), Decision Lineage (the decisions, the tail with this
/// device's signed row, "—" with nothing shown), the picture's alerts, the live-mode
/// chrome (the gateway's subject and honest seals), and every screen rendered in both
/// modes. [FR-C-1] [FR-C-4] [FR-H-3] [FR-H-5] [AC-C-13]
@MainActor
final class HeimdallConsoleTests: XCTestCase {
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
                                       replicaId: "heimdall-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    func testEntitiesFilterAsTheWebDoesAndWithholdAboveTheClearance() async throws {
        let app = try await signedIn("k.osei")
        let entities = MidgardListModel.entities(app)
        await entities.load()
        let all = entities.rows.map(\.item)
        XCTAssertEqual(all.map(\.id), ["E-0871", "E-2214", "E-2231", "E-0442", "E-1108"])
        let ids = { (f: HeimdallEntityFilter) in all.filter(f.admits).map(\.id) }
        XCTAssertEqual(ids(.air), ["E-2214", "E-2231"])
        XCTAssertEqual(ids(.maritime), ["E-0442"])
        XCTAssertEqual(ids(.ground), ["E-1108"])
        XCTAssertEqual(ids(.anomalous), ["E-0871"])
        XCTAssertEqual(ids(.lowConfidence), ["E-0871"])
        XCTAssertEqual(HeimdallEntityFilter.allCases.map(\.rawValue),
                       ["all", "air", "maritime", "ground", "anomalous", "low confidence"])
        XCTAssertEqual(entities.rows[0].marking.label, "CONFIDENTIAL // OPS-N")

        // K. Rao (restricted) sees the vessel only; the rest are counted, never shown.
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "k.rao" }))
        await entities.load()
        XCTAssertEqual(entities.rows.map(\.item.id), ["E-0442"])
        XCTAssertEqual(entities.withheld, 4)
    }

    func testFeedsShowTheQuarantinedBridgeAndSyncSitesTheirFilter() async throws {
        let app = try await signedIn("k.osei")
        let feeds = MidgardListModel.feeds(app)
        await feeds.load()
        XCTAssertEqual(feeds.rows.count, 6)
        let bridge = try XCTUnwrap(feeds.rows.last?.item)
        XCTAssertEqual(bridge.name, "LEGACY-L16")
        XCTAssertTrue(bridge.quarantined)
        XCTAssertEqual(heimdallFeedAuth(bridge), "FAILED")
        XCTAssertEqual(heimdallFeedAuth(feeds.rows[0].item), "ML-DSA · AUTH")
        XCTAssertFalse(heimdallFeedAuth(feeds.rows[0].item).contains("\u{2713}"), "no check mark the app did not check")

        let sites = MidgardListModel.syncSites(app)
        await sites.load()
        XCTAssertEqual(sites.rows.map(\.item.role), ["primary", "replica", "replica", "air-gap"])
        // A. Holt (internal) sees none of the sites (restricted and above).
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "a.holt" }))
        await sites.load()
        XCTAssertEqual(sites.rows.count, 0)
        XCTAssertEqual(sites.withheld, 4)
    }

    func testAWorkflowIsAuthorisedThroughTheHumanGateAndAudited() async throws {
        let app = try await signedIn("k.osei")
        let model = HeimdallWorkflowsModel(app: app)
        await model.load()
        XCTAssertEqual(model.rows.map(\.item.id), ["W-0912", "W-0913", "W-0911", "W-0910"])
        XCTAssertEqual(model.awaiting, 2)
        let w = try XCTUnwrap(model.current)
        XCTAssertEqual(model.stages(w).map(\.1), [true, true, true, false, false, false])

        // Without a token the core refuses, in HumanGateRequiredError's words.
        await model.authorise(token: false)
        XCTAssertEqual(model.gate, .pending)
        XCTAssertEqual(model.message, "Human gate required for action \"Re-task sensor feed NORTH-2 → E-0871\"")

        // The operator's press sends one: authorised, with the audit id the core gives.
        await model.authorise()
        XCTAssertEqual(model.gate, .authorised("D-4474"))
        XCTAssertNil(model.message)
        XCTAssertEqual(model.stages(w).map(\.1), [true, true, true, true, true, true])
        XCTAssertEqual(model.signedLabel(w), "AUTHORISED · AUDIT #D-4474")

        // Both are in the lineage, newest first; this device signed its own.
        let lineage = HeimdallLineageModel(app: app)
        await lineage.load()
        XCTAssertEqual(lineage.tail.prefix(2).map(\.item.id), ["D-4474", "D-4473"])
        XCTAssertEqual(lineage.tail[0].item.action, "applied · heimdall.authoriseWorkflow")
        XCTAssertEqual(lineage.tail[1].item.action, "refused · heimdall.authoriseWorkflow")
        XCTAssertEqual(lineage.tail[0].signature, .device)
        XCTAssertEqual(lineage.signed, 2)
        XCTAssertEqual(lineage.signedText, "2/7")

        // Selecting another workflow starts its gate afresh; Reject stays on screen.
        model.select(1)
        XCTAssertNil(model.gate)
        model.reject()
        XCTAssertEqual(model.gate, .rejected)
        await lineage.refreshTail()
        XCTAssertEqual(lineage.tail.count, 7, "a rejection is not sent")

        // A workflow needing no gate shows the web's line.
        model.select(3)
        XCTAssertEqual(model.signedLabel(try XCTUnwrap(model.current)), "COMPLETE · AUDIT #D-4468")
    }

    func testOfflineTheAuthorisationIsQueued() async throws {
        let app = try await signedIn("k.osei")
        let model = HeimdallWorkflowsModel(app: app)
        await model.load()
        await app.setOffline(true)
        await model.authorise()
        XCTAssertEqual(model.gate, .queued)
    }

    func testTheLineageShowsDecisionsAndADashWithNothingShown() async throws {
        let app = try await signedIn("k.osei")
        let lineage = HeimdallLineageModel(app: app)
        await lineage.load()
        XCTAssertEqual(lineage.decisions.map(\.item.id), ["D-4472", "D-4468", "D-4461"])
        // The sample's claim is delivered as not checked, never as a check mark.
        XCTAssertEqual(lineage.decisions[0].item.signature, "not checked (demo)")
        XCTAssertEqual(lineage.tail.map(\.item.id), ["D-4472", "D-4471", "D-4470", "D-4469", "D-4468"])
        XCTAssertEqual(lineage.signedText, "0/5")

        // A. Holt (internal) sees none of it: a dash, not a share of nothing.
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "a.holt" }))
        await lineage.load()
        XCTAssertTrue(lineage.tail.isEmpty)
        XCTAssertEqual(lineage.tailWithheld, 5)
        XCTAssertEqual(lineage.signedText, "—")
        XCTAssertEqual(lineage.withheld, 3)
    }

    func testThePictureAlertsComeFromItsOwnRows() async throws {
        let app = try await signedIn("k.osei")
        let picture = HeimdallPictureModel(app: app)
        await picture.load()
        XCTAssertEqual(picture.alerts.map(\.id), ["track.E-0871", "feed.LEGACY-L16", "feed.GND-OBS-4"])
        XCTAssertEqual(picture.alerts[0].track, "E-0871")
        XCTAssertNil(picture.alerts[1].track)
        // Below the tracks' clearance only what the persona may see raises an alert.
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "k.rao" }))
        await picture.load()
        XCTAssertEqual(picture.alerts.map(\.id), ["feed.LEGACY-L16", "feed.GND-OBS-4"])
    }

    func testLiveChromeIsTheGatewaysAndHonest() throws {
        let https = LiveSession(base: try XCTUnwrap(URL(string: "https://gw.example/api/v1")), host: "gw.example",
                                token: "t", subject: "ops.lead", clearance: "restricted", roles: [], compartments: [],
                                purpose: "operations")
        let loop = LiveSession(base: try XCTUnwrap(URL(string: "http://127.0.0.1:7711/api/v1")), host: "127.0.0.1:7711",
                               token: "t", subject: "ops.lead", clearance: "restricted", roles: [], compartments: [],
                               purpose: "operations")
        XCTAssertEqual(https.transport, "HTTPS")
        XCTAssertEqual(loop.transport, "LOOPBACK")
        XCTAssertEqual(MidgardChrome.seals(live: loop).map(\.1), ["LIVE · LOOPBACK"])
        XCTAssertEqual(MidgardChrome.seals(live: nil).map(\.1), ["PQ-SEALED", "AIR-GAP"])
        XCTAssertFalse(MidgardChrome.seals(live: https).contains { $0.1 == "PQ-SEALED" || $0.1 == "AIR-GAP" })
    }

    func testThePersonaChipNamesTheSamplePersonaWhenNotConnected() async throws {
        let app = try await signedIn("k.osei")
        let persona = try XCTUnwrap(app.signedInPersona)
        XCTAssertEqual(app.signedInName(persona), "K. Osei")
        // With no records the audit says so rather than "100 %".
        let audit = AuditModel(app: app)
        XCTAssertNil(audit.signedPercent)
    }

    func testEveryScreenRendersInBothModes() async throws {
        MGFonts.registerAll()
        let app = try await signedIn("k.osei")
        for screen in ["Entities", "Feeds", "Picture Sync", "Workflows", "Decision Lineage"] {
            for scheme in [ColorScheme.dark, .light] {
                for compact in [false, true] {
                    let view = HeimdallScreen(app: app, persona: "k.osei", screen: screen, compact: compact) { _ in }
                        .environment(\.colorScheme, scheme)
                        .frame(width: compact ? 402 : 1170, height: compact ? 874 : 900)
                    XCTAssertNotNil(ImageRenderer(content: view).cgImage, "\(screen) \(scheme) \(compact)")
                }
            }
        }
    }
}

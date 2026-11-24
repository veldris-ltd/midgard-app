import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Forge's screen models, over the release's sample data, against the real core: the
/// catalogue lists (connectors, datasets with their markings, expectations, schedules,
/// the projection's mappings; withheld above the clearance by count), the pipeline
/// builder (a build through the core's port replayed step by step and audited, refused
/// above the clearance, queued offline; the graph edited on this device only), the
/// Transform IDE (the sandboxed preview, the human gate refusing without a token and
/// authorising with one), the Mac table's widths, and every screen rendered in both
/// modes. [FR-C-1] [FR-C-4] [FR-F-1] [FR-F-3] [FR-F-7] [AC-C-13]
@MainActor
final class ForgeConsoleTests: XCTestCase {
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
                                       replicaId: "forge-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    func testTheCataloguesListTheSampleAndWithholdAboveTheClearance() async throws {
        let app = try await signedIn("j.benjamin")
        let connectors = MidgardListModel.connectors(app)
        await connectors.load()
        XCTAssertEqual(connectors.rows.map(\.item.name),
                       ["flights_jdbc", "sensor_stream", "maint_sftp", "legacy_soap", "weather_edge"])
        XCTAssertEqual(connectors.rows[3].item.action, "Client certificate expires in 6 days. Rotation is zero-downtime.")

        let datasets = MidgardListModel.datasets(app)
        await datasets.load()
        XCTAssertEqual(datasets.rows.map(\.item.id), ["flight_tracks_eu", "sensor_feed_north", "maintenance_events", "crew_roster"])
        XCTAssertEqual(datasets.rows[0].marking.label, "CONFIDENTIAL // OPS-N")
        XCTAssertEqual(datasets.rows[0].item.previousVersion, "v41")
        XCTAssertEqual(datasets.rows[0].item.lineage.map(\.label).last, "Aircraft objects")

        let checks = MidgardListModel.healthChecks(app)
        await checks.load()
        XCTAssertEqual(checks.rows.count, 5)
        XCTAssertEqual(checks.rows.filter { $0.item.projectionGate == "BLOCKS" }.map(\.item.id), ["hc-closed-by"])

        let schedules = MidgardListModel.schedules(app)
        await schedules.load()
        XCTAssertEqual(schedules.rows.compactMap(\.item.note).count, 1)

        let projections = MidgardListModel.projections(app)
        await projections.load()
        XCTAssertEqual(projections.rows.first?.item.mappings.map(\.kind), ["key", "property", "link", "struct", "governance"])

        // K. Rao (restricted) does not see the OPS-N dataset or the Aircraft projection:
        // counted, never shown.
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "k.rao" }))
        await datasets.load()
        XCTAssertEqual(datasets.rows.count, 3)
        XCTAssertEqual(datasets.withheld, 1)
        await projections.load()
        XCTAssertTrue(projections.rows.isEmpty)
        XCTAssertEqual(projections.withheld, 1)
    }

    func testABuildRunsThroughTheCoreAndReplaysItsSteps() async throws {
        let app = try await signedIn("j.benjamin")
        let model = ForgePipelineModel(app: app)
        model.stepDelay = 0
        await model.load()
        XCTAssertEqual(model.rows.map(\.item.id), ["flight_tracks_eu", "sensor_fusion_live", "maintenance_rollup"])
        XCTAssertEqual(model.selectedNode, "ds", "the web selects the dataset node first")
        XCTAssertEqual(model.context, "PIPELINES / FLIGHT_TRACKS_EU")
        XCTAssertEqual(model.log, ["> forge build flight_tracks_eu --branch main", "# last build 14:02 · OK · v41"])
        XCTAssertNil(model.runDisabled)

        await model.run()
        XCTAssertFalse(model.building)
        XCTAssertEqual(model.auditId, "FRG-2044")
        XCTAssertEqual(model.lastRun?.runId, "RUN-1601")
        XCTAssertEqual(model.progress, 100)
        XCTAssertEqual(model.phase, "Sealed")
        XCTAssertEqual(Array(model.log.suffix(8)), [
            "> forge build flight_tracks_eu --branch main",
            "resolve  sources 2/2 · PQ-mTLS OK",
            "transformclean_flights · 6/6 steps",
            "transformfuse_positions · expectations 11/12 watch",
            "seal     AES-256-GCM · ML-KEM-1024 wrap",
            "project  12,408 objects → fabric",
            "# build v42 OK · 1m38s · lineage signed (ML-DSA)",
            "# audit #FRG-2044 · signed on this device",
        ])

        // Another pipeline: its own log, and the next run number.
        model.choose(2)
        XCTAssertEqual(model.selectedNode, "mds")
        XCTAssertEqual(model.log.count, 2)
        await model.run()
        XCTAssertEqual(model.auditId, "FRG-2045")
        XCTAssertEqual(model.lastRun?.runId, "RUN-1602")
        XCTAssertEqual(model.lastRun?.version, "v9")

        // Offline, the build waits on this device.
        await app.setOffline(true)
        await model.run()
        XCTAssertEqual(model.phase, "Queued")
        XCTAssertFalse(model.building)
    }

    func testAPipelineAboveTheClearanceIsWithheld() async throws {
        let app = try await signedIn("k.rao")
        let model = ForgePipelineModel(app: app)
        model.stepDelay = 0
        await model.load()
        XCTAssertEqual(model.rows.map(\.item.id), ["sensor_fusion_live", "maintenance_rollup"])
        XCTAssertEqual(model.withheld, 1)
        await model.run()
        XCTAssertEqual(model.auditId, "FRG-2044")
    }

    func testTheGraphIsEditedOnThisDeviceOnly() async throws {
        let app = try await signedIn("j.benjamin")
        let model = ForgePipelineModel(app: app)
        await model.load()
        XCTAssertFalse(model.edited)
        XCTAssertEqual(model.graph.size, CGSize(width: 28 * 2 + 3 * 250 + 190, height: 28 * 2 + 1 * 150 + 84))

        // Settings, on both apps.
        model.setNode("ds", label: "flight_tracks_eu_v2", meta: "v41 · edited")
        XCTAssertEqual(model.node?.label, "flight_tracks_eu_v2")
        XCTAssertTrue(model.edited)
        // Moved (to the nearest quarter of a cell), added, connected, removed: the Mac's.
        model.move("ds", x: 2.13, y: -1)
        XCTAssertEqual(model.graph.nodes.first { $0.id == "ds" }?.x, 2.25)
        XCTAssertEqual(model.graph.nodes.first { $0.id == "ds" }?.y, 0)
        model.addNode(kind: "projection")
        let added = model.selectedNode
        XCTAssertEqual(model.node?.kind, "projection")
        XCTAssertEqual(model.node?.x, 3)
        XCTAssertEqual(model.node?.y, 1.5)
        model.connectFrom = "ds"
        model.tap(added)
        XCTAssertNil(model.connectFrom)
        XCTAssertTrue(model.graph.edges.contains(ForgeGraph.Edge(from: "ds", to: added)))
        model.connect("ds", "ds")
        XCTAssertFalse(model.graph.edges.contains(ForgeGraph.Edge(from: "ds", to: "ds")), "no edge to itself")
        model.remove("ds")
        XCTAssertFalse(model.graph.edges.contains { $0.from == "ds" || $0.to == "ds" })

        // A build still runs the enclosure's graph; the edits are discarded on request.
        XCTAssertNil(model.runDisabled)
        model.discard()
        XCTAssertFalse(model.edited)
        XCTAssertEqual(model.graph.nodes.count, 6)

        // A new graph is this device's alone, and is not built.
        model.drawNew()
        XCTAssertTrue(model.drawingNew)
        XCTAssertTrue(model.graph.nodes.isEmpty)
        XCTAssertEqual(model.context, "PIPELINES / NEW GRAPH")
        XCTAssertNotNil(model.runDisabled)
        model.addNode(kind: "source")
        model.addNode(kind: "dataset")
        XCTAssertEqual(model.graph.nodes.map(\.kind), ["source", "dataset"])
        model.choose(0)
        XCTAssertFalse(model.drawingNew)
        XCTAssertEqual(model.graph.nodes.count, 6, "the enclosure's graph again")
    }

    func testATransformIsPreviewedThenCommittedThroughTheHumanGate() async throws {
        let app = try await signedIn("j.benjamin")
        let model = ForgeTransformModel(app: app)
        await model.load()
        let t = try XCTUnwrap(model.transform)
        XCTAssertEqual(t.files, ["fuse_positions.py", "clean_flights.py", "expectations.yml"])
        XCTAssertEqual(model.code.components(separatedBy: "\n").count, 12)
        XCTAssertFalse(model.edited)

        await model.preview()
        let diff = try XCTUnwrap(model.diff)
        XCTAssertEqual(diff.rowsIn, 48_112)
        XCTAssertEqual(diff.rowsOut, 47_924)
        XCTAssertEqual(diff.hunks.map(\.op), ["~", "-", "+"])
        XCTAssertTrue(diff.summary.hasPrefix("Preview of: from forge import transform"))
        XCTAssertEqual(model.gate, .pending)

        // Without a token the core refuses, in ForgeHumanGateRequiredError's words.
        await model.commit(token: false)
        XCTAssertEqual(model.gate, .pending)
        XCTAssertEqual(model.message, "Human gate required to commit \"fuse_positions\"")

        // The operator's press sends one: committed as v43, with the audit id.
        await model.authorise()
        XCTAssertEqual(model.gate, .authorised("FRG-2046"))
        XCTAssertEqual(model.committed, ForgeCommitAnswer(version: "v43", applied: true))
        XCTAssertNil(model.message)

        // An edited code is previewed as edited; Reject stays on screen.
        model.code = "SELECT 1"
        XCTAssertTrue(model.edited)
        await model.preview()
        XCTAssertEqual(model.diff?.summary, "Preview of: SELECT 1")
        model.reject()
        XCTAssertEqual(model.gate, .rejected)
        model.revert()
        XCTAssertFalse(model.edited)
    }

    func testMacTableColumnsFitTheirWidthAndKeepTheirChips() {
        // Heimdall's Action workflows table, at the Mac's default window width (its
        // centre column, about 759 pt): every column shown, the State chip at its 130 pt.
        let columns: [WebTableWidths.Column] = [
            .init(width: 90, drawn: true, header: "Workflow"), .init(width: nil, drawn: false, header: "Action"),
            .init(width: 190, drawn: false, header: "Stage"), .init(width: 110, drawn: false, header: "Owner"),
            .init(width: 70, drawn: false, header: "Options"), .init(width: 60, drawn: false, header: "Age"),
            .init(width: 130, drawn: true, header: "State"),
        ]
        let fit = try? XCTUnwrap(WebTableWidths.widths(columns, in: 759))
        XCTAssertEqual(fit.map { $0.reduce(0, +) } ?? 0, 759, accuracy: 0.5)
        XCTAssertEqual(fit?[6], 130, "the state chip keeps its width")
        XCTAssertGreaterThanOrEqual(fit?[4] ?? 0, WebTableWidths.headerWidth("Options"), "OPTIONS is not cut")
        XCTAssertGreaterThanOrEqual(fit?[1] ?? 0, WebTableWidths.flexibleLeast)
        // Wider: the action takes the room.
        let wide = WebTableWidths.widths(columns, in: 1100)
        XCTAssertGreaterThan(wide?[1] ?? 0, 300)
        XCTAssertEqual(wide?[2], 190)
        // Too narrow for even the least: not a table (drawn as a list).
        XCTAssertNil(WebTableWidths.widths(columns, in: 400))
        XCTAssertLessThan(WebTableWidths.minimum(columns), 759)
    }

    func testEveryScreenRendersInBothModes() async throws {
        MGFonts.registerAll()
        let app = try await signedIn("j.benjamin")
        for screen in ForgeChrome.screens {
            for scheme in [ColorScheme.dark, .light] {
                for compact in [false, true] {
                    let view = ForgeScreen(app: app, persona: "j.benjamin", screen: screen, compact: compact) { _ in }
                        .environment(\.colorScheme, scheme)
                        .frame(width: compact ? 402 : 1170, height: compact ? 874 : 900)
                    XCTAssertNotNil(ImageRenderer(content: view).cgImage, "\(screen) \(scheme) \(compact)")
                }
            }
        }
        XCTAssertEqual(Set(ForgeChrome.screens), Set(ConsoleCatalog.console(number: 4).screens))
    }
}

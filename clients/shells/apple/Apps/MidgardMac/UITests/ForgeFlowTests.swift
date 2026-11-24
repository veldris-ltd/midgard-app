import XCTest

/// Forge's seven screens over the sample data, as the web console shows them: each
/// opened from the navigation and kept as a window screenshot attachment; then a build
/// of flight_tracks_eu through the core (audit FRG-2044, its steps replayed into the
/// log), a node drawn on the graph (a draft on this Mac) and discarded, and the
/// transform previewed and committed through the human gate (FRG-2046).
/// `MIDGARD_APPEARANCE=Light` (as `TEST_RUNNER_MIDGARD_APPEARANCE`) runs it in light
/// mode. [FR-C-1] [FR-F-1] [FR-F-3] [FR-F-7] [NFR-AI-1]
final class ForgeFlowTests: XCTestCase {
    private var app: XCUIApplication!

    /// In the web navigation's order.
    static let screens = ["pipeline-builder", "connectors", "datasets", "data-health", "schedules", "transform-ide",
                          "object-projection"]

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private var appearance: String { ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "Dark" }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "forge-mac-\(name)-\(appearance.lowercased())"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func waitText(_ element: XCUIElement, contains text: String, timeout: TimeInterval = 10) -> Bool {
        let p = NSPredicate { _, _ in Nav.text(element).contains(text) }
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: nil)], timeout: timeout) == .completed
    }

    func testEveryScreenABuildAndACommit() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 4)

        for slug in Self.screens {
            Nav.openScreen(app, slug)
            if slug != "pipeline-builder" {
                XCTAssertTrue(Nav.element(app, "mg.content.forge.\(slug)").waitForExistence(timeout: 5), "no \(slug)")
            }
            XCTAssertTrue(Nav.element(app, "mg.forge.inspector.title").waitForExistence(timeout: 10),
                          "no inspector on \(slug)")
            switch slug {
            case "pipeline-builder":
                XCTAssertTrue(Nav.element(app, "mg.forge.node.ds").waitForExistence(timeout: 5), "no graph")
                XCTAssertTrue(waitText(Nav.element(app, "mg.header.context"), contains: "PIPELINES / FLIGHT_TRACKS_EU"))
            case "connectors":
                XCTAssertTrue(Nav.element(app, "mg.forge.connectors.table.row.legacy_soap").exists)
            case "datasets":
                XCTAssertTrue(Nav.element(app, "mg.forge.datasets.lineage").exists, "no lineage")
            default:
                break
            }
            keep(slug)
        }

        // A build of flight_tracks_eu: the core's answer replayed into the log.
        Nav.openScreen(app, "pipeline-builder")
        Nav.element(app, "mg.forge.pipeline.run").click()
        let log = Nav.element(app, "mg.forge.pipeline.log")
        XCTAssertTrue(waitText(log, contains: "build v42 OK", timeout: 15), Nav.text(log))
        XCTAssertTrue(waitText(log, contains: "audit #FRG-2044"), Nav.text(log))
        XCTAssertTrue(waitText(Nav.element(app, "mg.forge.pipeline.status"), contains: "Ready"))
        keep("pipeline-built")

        // A node drawn on this Mac: a draft, discarded.
        Nav.element(app, "mg.forge.pipeline.add.source").click()
        XCTAssertTrue(Nav.element(app, "mg.forge.pipeline.draft").waitForExistence(timeout: 5), "no draft badge")
        XCTAssertTrue(Nav.element(app, "mg.forge.node.n7").exists, "the new node is not drawn")
        keep("pipeline-draft")
        Nav.element(app, "mg.forge.pipeline.discard").click()
        XCTAssertFalse(Nav.element(app, "mg.forge.node.n7").waitForExistence(timeout: 2))

        // The transform, previewed in the sandbox, then committed through the gate.
        Nav.openScreen(app, "transform-ide")
        Nav.element(app, "mg.forge.ide.preview").click()
        XCTAssertTrue(Nav.element(app, "mg.forge.ide.diff").waitForExistence(timeout: 10), "no preview")
        let authorise = Nav.element(app, "mg.forge.ide.authorise")
        XCTAssertTrue(authorise.waitForExistence(timeout: 5), "no gate")
        keep("ide-gate")
        authorise.click()
        let gate = Nav.element(app, "mg.forge.ide.gate.authorised")
        XCTAssertTrue(gate.waitForExistence(timeout: 10), "the commit was not authorised")
        XCTAssertTrue(waitText(gate, contains: "AUDIT #FRG-2046"), Nav.text(gate))
        keep("ide-committed")
    }
}

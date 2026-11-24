import XCTest

/// Heimdall's six screens over the sample data, as the web console shows them: each
/// opened from the navigation and kept as a window screenshot attachment; the picture's
/// Alerts list; then two authorisations through the human gate: the picture's ranked
/// option 01 (W-0912, audit D-4473) and, on Workflows, W-0913 (D-4474), both heading
/// Decision Lineage's live tail, signed by this device. `MIDGARD_APPEARANCE=Light` (as
/// `TEST_RUNNER_MIDGARD_APPEARANCE`) runs it in light mode. [FR-C-1] [FR-H-4] [FR-H-5]
/// [NFR-AI-1]
final class HeimdallFlowTests: XCTestCase {
    private var app: XCUIApplication!

    /// In the web navigation's order.
    static let screens = ["operating-picture", "feeds", "entities", "picture-sync", "workflows", "decision-lineage"]

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private var appearance: String { ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "Dark" }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "heimdall-mac-\(name)-\(appearance.lowercased())"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func waitText(_ element: XCUIElement, contains text: String, timeout: TimeInterval = 10) -> Bool {
        let p = NSPredicate { _, _ in Nav.text(element).contains(text) }
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: nil)], timeout: timeout) == .completed
    }

    func testEveryScreenAndTwoAuthorisations() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 3)
        // The reference layout, should the window have kept another view chosen.
        let reference = Nav.element(app, "mg.heimdall.view.2d")
        if reference.waitForExistence(timeout: 5) { reference.click() }

        for slug in Self.screens {
            Nav.openScreen(app, slug)
            if slug == "operating-picture" {
                XCTAssertTrue(Nav.element(app, "mg.heimdall.track.E-0871").waitForExistence(timeout: 10), "no tracks")
            } else {
                XCTAssertTrue(Nav.element(app, "mg.content.heimdall.\(slug)").waitForExistence(timeout: 5), "no \(slug)")
                XCTAssertTrue(Nav.element(app, "mg.heimdall.inspector.title").waitForExistence(timeout: 10),
                              "no inspector on \(slug)")
            }
            switch slug {
            case "feeds":
                XCTAssertTrue(Nav.element(app, "mg.heimdall.feeds.excluded").exists, "the bridge is not shown quarantined")
            case "picture-sync":
                XCTAssertTrue(Nav.element(app, "mg.heimdall.sync.filter").exists, "no marking filter on SITE-CHARLIE")
            case "workflows":
                XCTAssertTrue(waitText(Nav.element(app, "mg.heimdall.toolbar.awaiting"), contains: "2 AWAITING"))
            default:
                break
            }
            keep(slug)
        }

        // The picture's alerts: the anomalous track first; choosing it selects it.
        Nav.openScreen(app, "operating-picture")
        Nav.element(app, "mg.heimdall.alerts").click()
        let alert = Nav.element(app, "mg.heimdall.alert.track.E-0871")
        XCTAssertTrue(alert.waitForExistence(timeout: 5), "no alerts list")
        XCTAssertTrue(Nav.element(app, "mg.heimdall.alert.feed.LEGACY-L16").exists)
        keep("picture-alerts")
        alert.click()

        // Option 01, authorised through the human gate: W-0912, audited D-4473.
        let inspected = Nav.element(app, "mg.heimdall.inspector.track")
        XCTAssertTrue(waitText(inspected, contains: "E-0871"), Nav.text(inspected))
        Nav.element(app, "mg.heimdall.option.1").click()
        let authorise = Nav.element(app, "mg.heimdall.authorise")
        XCTAssertTrue(authorise.waitForExistence(timeout: 5))
        if !authorise.isHittable { Nav.element(app, "mg.detail.operating-picture").scroll(byDeltaX: 0, deltaY: -300) }
        authorise.click()
        let gate = Nav.element(app, "mg.heimdall.gate.authorised")
        XCTAssertTrue(gate.waitForExistence(timeout: 10), "the option was not authorised")
        XCTAssertTrue(Nav.text(gate).contains("AUDIT #D-4473"), Nav.text(gate))
        keep("picture-authorised")

        // On Workflows, W-0913 through its own gate: D-4474.
        Nav.openScreen(app, "workflows")
        let row = Nav.element(app, "mg.heimdall.workflows.table.row.W-0913")
        XCTAssertTrue(row.waitForExistence(timeout: 10))
        row.click()
        let authoriseWorkflow = Nav.element(app, "mg.heimdall.workflows.authorise")
        XCTAssertTrue(authoriseWorkflow.waitForExistence(timeout: 5))
        keep("workflows-gate")
        authoriseWorkflow.click()
        let signed = Nav.element(app, "mg.heimdall.workflows.signed")
        XCTAssertTrue(signed.waitForExistence(timeout: 10), "W-0913 was not authorised")
        XCTAssertTrue(waitText(signed, contains: "AUTHORISED · AUDIT #D-4474"), Nav.text(signed))
        keep("workflows-authorised")

        // Both head the lineage's live tail.
        Nav.openScreen(app, "decision-lineage")
        XCTAssertTrue(Nav.element(app, "mg.heimdall.lineage.tail.row.D-4474").waitForExistence(timeout: 10), "no D-4474")
        XCTAssertTrue(Nav.element(app, "mg.heimdall.lineage.tail.row.D-4473").exists, "no D-4473")
        keep("lineage-after")
    }

    /// Workflows at the 900 × 760 pt minimum window: the table does not fit, so it is
    /// drawn as the iPhone's list (`WebTableWidths`), its rows still there to choose.
    func testWorkflowsAtTheMinimumWindowAreAList() {
        app = XCUIApplication()
        var launch = Nav.freshLaunch
        launch[launch.count - 1] = "100 130 900 760 0 0 1920 1170"
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + launch
        app.launch()
        Nav.openConsole(app, 3)
        Nav.openScreen(app, "workflows")
        XCTAssertLessThanOrEqual(app.windows.firstMatch.frame.width, 901, "the window is not at its minimum")
        XCTAssertTrue(Nav.element(app, "mg.heimdall.workflows.table.row.W-0913").waitForExistence(timeout: 10))
        keep("workflows-minimum")
    }
}

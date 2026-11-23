import XCTest

/// Heimdall's six screens on the iPhone over the sample data: each opened from the
/// console's list and kept as a screenshot attachment in the simulator's appearance;
/// the picture's Alerts list; then two authorisations through the human gate: the
/// picture's ranked option 01 (W-0912, audit D-4473) and, on Workflows, W-0913 in its
/// inspector sheet (D-4474), both heading Decision Lineage's live tail.
/// [FR-C-1] [FR-H-4] [FR-H-5] [NFR-AI-1]
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

    private func keep(_ name: String) {
        let image = app.windows.firstMatch.screenshot().image
        let shot = XCTAttachment(image: image)
        shot.name = "heimdall-iphone-\(name)-\(ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "dark")"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func reveal(_ e: XCUIElement) {
        for _ in 0..<4 where !e.isHittable { app.swipeUp() }
    }

    private func waitLabel(_ e: XCUIElement, contains text: String, timeout: TimeInterval = 10) -> Bool {
        let p = NSPredicate(format: "label CONTAINS %@", text)
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: e)], timeout: timeout) == .completed
    }

    func testEveryScreenAndTwoAuthorisations() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
        AppNavigation.signIn(app)

        for slug in Self.screens {
            if slug == "operating-picture" {
                AppNavigation.openPicture(app)
            } else {
                AppNavigation.open(app, console: 3, slug)
                XCTAssertTrue(element("mg.heimdall.inspect").waitForExistence(timeout: 10), "nothing to inspect on \(slug)")
            }
            keep(slug)
            if slug == "feeds" {
                // One inspector, as the sheet the iPhone opens it in: the quarantined bridge.
                element("mg.heimdall.inspect").tap()
                XCTAssertTrue(element("mg.heimdall.feeds.excluded").waitForExistence(timeout: 10))
                keep("feeds-inspector")
                app.buttons["mg.inspector.done"].firstMatch.tap()
            }
        }

        // The picture's alerts: the anomalous track first; choosing it opens the inspector.
        AppNavigation.openPicture(app)
        element("mg.heimdall.alerts").tap()
        let alert = element("mg.heimdall.alert.track.E-0871")
        XCTAssertTrue(alert.waitForExistence(timeout: 5), "no alerts list")
        keep("picture-alerts")
        alert.tap()

        // Option 01, authorised through the human gate: W-0912, audited D-4473.
        let inspected = element("mg.heimdall.inspector.track")
        XCTAssertTrue(inspected.waitForExistence(timeout: AppNavigation.timeout), "the inspector did not open")
        XCTAssertEqual(inspected.label, "E-0871")
        let authorise = element("mg.heimdall.authorise")
        reveal(authorise)
        authorise.tap()
        let gate = element("mg.heimdall.gate.authorised")
        XCTAssertTrue(gate.waitForExistence(timeout: AppNavigation.timeout), app.debugDescription)
        XCTAssertTrue(gate.label.contains("AUDIT #D-4473"), "gate says: \(gate.label)")
        keep("picture-authorised")
        app.swipeDown(velocity: .fast)

        // On Workflows, W-0913 in its inspector sheet: D-4474.
        AppNavigation.open(app, console: 3, "workflows")
        let row = element("mg.heimdall.workflows.table.row.W-0913")
        reveal(row)
        // A list row is one element the size of the row, over its drawn text.
        XCTAssertGreaterThanOrEqual(row.frame.height, 44, "row \(row.frame)")
        XCTAssertGreaterThan(row.frame.width, 300, "row \(row.frame)")
        XCTAssertFalse(app.staticTexts["Cross-cue maritime radar sweep on E-0871"].exists, "a row's drawn text is reported")
        row.tap()
        let authoriseWorkflow = element("mg.heimdall.workflows.authorise")
        XCTAssertTrue(authoriseWorkflow.waitForExistence(timeout: AppNavigation.timeout), "no gate for W-0913")
        reveal(authoriseWorkflow)
        keep("workflows-gate")
        authoriseWorkflow.tap()
        let signed = element("mg.heimdall.workflows.signed")
        XCTAssertTrue(signed.waitForExistence(timeout: AppNavigation.timeout), "W-0913 was not authorised")
        XCTAssertTrue(waitLabel(signed, contains: "AUTHORISED · AUDIT #D-4474"), signed.label)
        keep("workflows-authorised")
        app.buttons["mg.inspector.done"].firstMatch.tap()

        // Both head the lineage's live tail.
        AppNavigation.open(app, console: 3, "decision-lineage")
        let newest = element("mg.heimdall.lineage.tail.row.D-4474")
        reveal(newest)
        XCTAssertTrue(newest.waitForExistence(timeout: 10), "no D-4474 in the tail")
        XCTAssertTrue(element("mg.heimdall.lineage.tail.row.D-4473").exists, "no D-4473 in the tail")
        keep("lineage-after")
    }
}

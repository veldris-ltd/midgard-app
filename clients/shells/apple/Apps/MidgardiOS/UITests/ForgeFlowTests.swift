import XCTest

/// Forge's seven screens on the iPhone over the sample data: each opened from the
/// console's list and kept as a screenshot attachment in the simulator's appearance;
/// one inspector (with its node settings) in its sheet; then a build of
/// flight_tracks_eu through the core (FRG-2044, its steps replayed into the log) and
/// the transform previewed and committed through the human gate in its inspector sheet
/// (FRG-2046). [FR-C-1] [FR-F-1] [FR-F-3] [FR-F-7] [NFR-AI-1]
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

    private func keep(_ name: String) {
        let image = app.windows.firstMatch.screenshot().image
        let shot = XCTAttachment(image: image)
        shot.name = "forge-iphone-\(name)-\(ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "dark")"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func reveal(_ e: XCUIElement) {
        for _ in 0..<5 where !e.isHittable { app.swipeUp() }
    }

    private func waitLabel(_ e: XCUIElement, contains text: String, timeout: TimeInterval = 10) -> Bool {
        let p = NSPredicate(format: "label CONTAINS %@", text)
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: e)], timeout: timeout) == .completed
    }

    func testEveryScreenABuildAndACommit() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
        AppNavigation.signIn(app)

        for slug in Self.screens {
            AppNavigation.open(app, console: 4, slug)
            XCTAssertTrue(element("mg.forge.inspect").waitForExistence(timeout: 10), "nothing to inspect on \(slug)")
            keep(slug)
            if slug == "pipeline-builder" {
                // The node's inspector, with its settings, in the sheet the iPhone opens.
                element("mg.forge.inspect").tap()
                XCTAssertTrue(element("mg.forge.node.label").waitForExistence(timeout: 10), "no node settings")
                keep("pipeline-node-settings")
                app.buttons["mg.inspector.done"].firstMatch.tap()
            }
        }

        // A build of flight_tracks_eu: the core's answer replayed into the log.
        AppNavigation.open(app, console: 4, "pipeline-builder")
        let run = element("mg.forge.pipeline.run")
        run.tap()
        let log = element("mg.forge.pipeline.log")
        reveal(log)
        XCTAssertTrue(waitLabel(log, contains: "build v42 OK", timeout: 15), log.label)
        XCTAssertTrue(waitLabel(log, contains: "audit #FRG-2044"), log.label)
        keep("pipeline-built")

        // The transform, previewed, then committed through the gate in its sheet.
        AppNavigation.open(app, console: 4, "transform-ide")
        element("mg.forge.ide.preview").tap()
        let authorise = element("mg.forge.ide.authorise")
        XCTAssertTrue(authorise.waitForExistence(timeout: AppNavigation.timeout), "no gate in the inspector")
        reveal(authorise)
        keep("ide-gate")
        authorise.tap()
        let gate = element("mg.forge.ide.gate.authorised")
        XCTAssertTrue(gate.waitForExistence(timeout: AppNavigation.timeout), "the commit was not authorised")
        XCTAssertTrue(waitLabel(gate, contains: "AUDIT #FRG-2046"), gate.label)
        keep("ide-committed")
        app.buttons["mg.inspector.done"].firstMatch.tap()
    }
}

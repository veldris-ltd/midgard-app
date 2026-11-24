import XCTest

/// Bifrost's six screens on the iPhone over the sample data, signed in as A. Holt (the
/// release manager): each opened from the console's list and kept as a screenshot
/// attachment in the simulator's appearance; then Promote to Ring 2 from the header,
/// stepping the broad ring to 100 % through the core (AUD-B-1044 to AUD-B-1047, the gate
/// PASS), the Rollback centre's drill played into its log, and the release plan executed
/// through its health gate from the inspector sheet (AUD-B-1048). [FR-C-1] [FR-B-2]
/// [FR-B-5]
final class BifrostFlowTests: XCTestCase {
    private var app: XCUIApplication!

    /// In the web navigation's order.
    static let screens = ["release-console", "artefacts", "channels", "bundles", "fleet", "rollback"]

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private func keep(_ name: String) {
        let image = app.windows.firstMatch.screenshot().image
        let shot = XCTAttachment(image: image)
        shot.name = "bifrost-iphone-\(name)-\(ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "dark")"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func reveal(_ e: XCUIElement) {
        for _ in 0..<6 where !e.isHittable { app.swipeUp() }
    }

    private func waitLabel(_ e: XCUIElement, contains text: String, timeout: TimeInterval = 15) -> Bool {
        let p = NSPredicate(format: "label CONTAINS %@", text)
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: e)], timeout: timeout) == .completed
    }

    func testEveryScreenAPromotionAndARollback() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
        // A. Holt holds the release-manager role the promotion and the rollback need.
        AppNavigation.toSignIn(app)
        AppNavigation.personaButton(app, "a.holt").tap()
        XCTAssertTrue(AppNavigation.consoleMarker(app).waitForExistence(timeout: AppNavigation.timeout))

        for slug in Self.screens {
            AppNavigation.open(app, console: 6, slug)
            XCTAssertTrue(element("mg.bifrost.inspect").waitForExistence(timeout: 10), "nothing to inspect on \(slug)")
            keep(slug)
        }

        // Promote to Ring 2, from the header: four steps to 100 %, the gate PASS.
        AppNavigation.open(app, console: 6, "release-console")
        let promote = element("mg.bifrost.release.promote")
        XCTAssertTrue(promote.waitForExistence(timeout: AppNavigation.timeout), "no Promote")
        promote.tap()
        let audit = element("mg.bifrost.release.audit")
        XCTAssertTrue(audit.waitForExistence(timeout: AppNavigation.timeout), "no step was audited")
        XCTAssertTrue(waitLabel(audit, contains: "AUD-B-1047"), audit.label)
        XCTAssertTrue(waitLabel(element("mg.bifrost.release.gate"), contains: "GATE PASS"))
        XCTAssertTrue(waitLabel(element("mg.bifrost.ring.ring-2"), contains: "48 of 48"))
        keep("release-promoted")

        // The drill, from the header, then the release plan executed through its gate.
        AppNavigation.open(app, console: 6, "rollback")
        element("mg.bifrost.rollback.drill").tap()
        let log = element("mg.bifrost.rollback.log")
        reveal(log)
        XCTAssertTrue(waitLabel(log, contains: "audit #R-2214"), log.label)
        keep("rollback-drill")
        app.swipeDown()
        element("mg.bifrost.inspect").tap()
        let execute = element("mg.bifrost.rollback.execute")
        XCTAssertTrue(execute.waitForExistence(timeout: AppNavigation.timeout), "no Execute rollback")
        reveal(execute)
        execute.tap()
        let executed = element("mg.bifrost.rollback.audit")
        XCTAssertTrue(executed.waitForExistence(timeout: AppNavigation.timeout), "the rollback was not audited")
        XCTAssertTrue(waitLabel(executed, contains: "AUD-B-1048"), executed.label)
        keep("rollback-executed")
        app.buttons["mg.inspector.done"].firstMatch.tap()
    }
}

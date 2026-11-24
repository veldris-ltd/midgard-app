import XCTest

/// Bifrost's six screens over the sample data, as the web console shows them, signed in
/// as A. Holt (the release manager): each opened from the navigation and kept as a
/// window screenshot attachment; then Promote to Ring 2 from the header, stepping the
/// broad ring to 100 % through the core (AUD-B-1044 to AUD-B-1047, the gate PASS), the
/// Rollback centre's drill played into its log, and the release plan executed through
/// its health gate (AUD-B-1048). `MIDGARD_APPEARANCE=Light` (as
/// `TEST_RUNNER_MIDGARD_APPEARANCE`) runs it in light mode. [FR-C-1] [FR-B-2] [FR-B-5]
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

    private var appearance: String { ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "Dark" }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "bifrost-mac-\(name)-\(appearance.lowercased())"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func waitText(_ element: XCUIElement, contains text: String, timeout: TimeInterval = 15) -> Bool {
        let p = NSPredicate { _, _ in Nav.text(element).contains(text) }
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: nil)], timeout: timeout) == .completed
    }

    func testEveryScreenAPromotionAndARollback() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + Nav.freshLaunch
        app.launch()
        // A. Holt holds the release-manager role the promotion and the rollback need.
        Nav.toSignIn(app)
        Nav.personaButton(app, "a.holt").click()
        XCTAssertTrue(Nav.element(app, "mg.console.selected").waitForExistence(timeout: 20))
        Nav.openConsole(app, 6)

        for slug in Self.screens {
            Nav.openScreen(app, slug)
            XCTAssertTrue(Nav.element(app, "mg.content.bifrost.\(slug)").waitForExistence(timeout: 5), "no \(slug)")
            keep(slug)
        }

        // Promote to Ring 2, from the header: four steps to 100 %, the gate PASS.
        Nav.openScreen(app, "release-console")
        Nav.element(app, "mg.bifrost.release.promote").click()
        let audit = Nav.element(app, "mg.bifrost.release.audit")
        XCTAssertTrue(audit.waitForExistence(timeout: 15), "no step was audited")
        XCTAssertTrue(waitText(audit, contains: "AUD-B-1047"), Nav.text(audit))
        XCTAssertTrue(waitText(Nav.element(app, "mg.bifrost.release.gate"), contains: "GATE PASS"))
        keep("release-promoted")

        // The drill, from the header, then the release plan executed through its gate.
        Nav.openScreen(app, "rollback")
        Nav.element(app, "mg.bifrost.rollback.drill").click()
        let log = Nav.element(app, "mg.bifrost.rollback.log")
        XCTAssertTrue(waitText(log, contains: "audit #R-2214"), Nav.text(log))
        Nav.element(app, "mg.bifrost.rollback.execute").click()
        let executed = Nav.element(app, "mg.bifrost.rollback.audit")
        XCTAssertTrue(executed.waitForExistence(timeout: 15), "the rollback was not audited")
        XCTAssertTrue(waitText(executed, contains: "AUD-B-1048"), Nav.text(executed))
        keep("rollback-executed")
    }
}

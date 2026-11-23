import XCTest

/// Admin's eight screens over the sample data, as the web console shows them: each
/// opened from the navigation and kept as a window screenshot attachment, then one
/// action: K. Osei, a second person, approves A-9110 (requested by J. Benjamin) on
/// Approvals, and the decision heads the audit tail with its audit id, numbered as
/// AdminMock numbers its rows. `MIDGARD_APPEARANCE=Light` (as
/// `TEST_RUNNER_MIDGARD_APPEARANCE`) runs it in light mode. [FR-C-1] [FR-C-4] [FR-AD-5]
/// [AC-AD-4] [AC-AD-6]
final class AdminFlowTests: XCTestCase {
    private var app: XCUIApplication!

    /// In the web navigation's order.
    static let screens = ["overview", "approvals", "iam", "config", "registry", "ledger", "break-glass", "copilot"]

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private var appearance: String { ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "Dark" }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "admin-mac-\(name)-\(appearance.lowercased())"
        shot.lifetime = .keepAlways
        add(shot)
    }

    func testEveryScreenAndASecondPersonsApproval() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 2)

        for slug in Self.screens {
            Nav.openScreen(app, slug)
            XCTAssertTrue(Nav.element(app, "mg.content.admin.\(slug)").waitForExistence(timeout: 5), "no \(slug) content")
            switch slug {
            case "approvals":
                XCTAssertTrue(Nav.element(app, "mg.admin.approval.A-9110").waitForExistence(timeout: 10), "no queue")
            case "break-glass":
                XCTAssertTrue(Nav.element(app, "mg.admin.breakglass.strip").waitForExistence(timeout: 10), "no state")
            case "copilot":
                XCTAssertTrue(Nav.element(app, "mg.admin.copilot.refusal").waitForExistence(timeout: 10), "no thread")
            case "ledger":
                XCTAssertTrue(Nav.element(app, "mg.admin.ledger.entry").waitForExistence(timeout: 10), "no selected entry")
            default:
                XCTAssertTrue(Nav.element(app, "mg.admin.inspector.title").waitForExistence(timeout: 10),
                              "no inspector on \(slug)")
            }
            keep(slug)
        }

        // The action: a second person's approval, routed through the core.
        Nav.openScreen(app, "approvals")
        let approve = Nav.element(app, "mg.admin.approval.A-9110.approve")
        XCTAssertTrue(approve.waitForExistence(timeout: 10))
        approve.click()
        let state = Nav.element(app, "mg.admin.approval.A-9110.state")
        XCTAssertTrue(Nav.wait(state, valuePrefix: "APPROVED · TOKEN 10m", timeout: 10) ||
                      Nav.wait(state, labelPrefix: "APPROVED · TOKEN 10m", timeout: 1), "state says: \(Nav.text(state))")
        let tail = Nav.element(app, "mg.admin.approvals.tail")
        XCTAssertTrue(Nav.text(tail).contains("A-9111  approved Rotate enclosure-root-03  ·  granted by k.osei"),
                      "tail says: \(Nav.text(tail))")
        keep("approvals-approved")
    }
}

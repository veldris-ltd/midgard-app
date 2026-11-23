import XCTest

/// Admin's eight screens on the iPhone over the sample data: each opened from the
/// console's list and kept as a screenshot attachment in the simulator's appearance,
/// then one action: K. Osei, a second person, approves A-9110 (requested by J. Benjamin)
/// on Approvals, and the decision heads the audit tail with its audit id.
/// [FR-C-1] [FR-C-4] [FR-AD-5] [AC-AD-4] [AC-AD-6]
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

    private func keep(_ name: String) {
        let image = app.windows.firstMatch.screenshot().image
        let shot = XCTAttachment(image: image)
        shot.name = "admin-iphone-\(name)-\(ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "dark")"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func open(_ slug: String) {
        AppNavigation.selectConsole(app, 2)
        let row = app.buttons.matching(NSPredicate(format: "identifier MATCHES %@", "mg\\.[0-9]+\\.admin\\.\(slug)")).firstMatch
        AppNavigation.tapClearOfConsoleBar(app, row)
        XCTAssertTrue(element("mg.content.admin.\(slug)").waitForExistence(timeout: AppNavigation.timeout),
                      "the \(slug) screen did not open")
    }

    func testEveryScreenAndASecondPersonsApproval() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
        AppNavigation.signIn(app)

        for slug in Self.screens {
            open(slug)
            XCTAssertTrue(element("mg.admin.inspect").waitForExistence(timeout: 10), "nothing to inspect on \(slug)")
            keep(slug)
            if slug == "config" {
                // One inspector, as the sheet the iPhone opens it in.
                element("mg.admin.inspect").tap()
                XCTAssertTrue(element("mg.admin.inspector.title").waitForExistence(timeout: 10))
                keep("config-inspector")
                app.buttons["mg.inspector.done"].firstMatch.tap()
            }
        }

        // The action: a second person's approval, routed through the core.
        open("approvals")
        let approve = element("mg.admin.approval.A-9110.approve")
        XCTAssertTrue(approve.waitForExistence(timeout: 10))
        for _ in 0..<3 where !approve.isHittable { app.swipeUp() }
        approve.tap()
        let state = element("mg.admin.approval.A-9110.state")
        let approved = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label BEGINSWITH %@", "APPROVED · TOKEN 10m"),
                                                 object: state)
        XCTAssertEqual(XCTWaiter.wait(for: [approved], timeout: 10), .completed, "state says: \(state.label)")
        let tail = element("mg.admin.approvals.tail")
        for _ in 0..<4 where !tail.isHittable { app.swipeUp() }
        XCTAssertTrue(tail.label.contains("A-9111  approved Rotate enclosure-root-03  ·  granted by k.osei"),
                      "tail says: \(tail.label)")
        keep("approvals-approved")
    }
}

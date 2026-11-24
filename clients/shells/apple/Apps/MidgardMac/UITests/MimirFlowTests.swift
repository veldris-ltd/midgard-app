import XCTest

/// Mimir's seven screens over the sample data, as the web console shows them: each
/// opened from the navigation and kept as a window screenshot attachment; then the Agent
/// Studio's proposal authorised through the human gate (A-77121), the selected suite run
/// from the header (A-77122, the gate blocking), the injection preset stopped at the input
/// rail (A-77123), and on the Logic Builder the gate's Delete disabled and a step added
/// and discarded. `MIDGARD_APPEARANCE=Light` (as `TEST_RUNNER_MIDGARD_APPEARANCE`) runs it
/// in light mode. [FR-C-1] [FR-MI-2] [FR-MI-3] [FR-MI-4] [FR-MI-7] [NFR-AI-1]
final class MimirFlowTests: XCTestCase {
    private var app: XCUIApplication!

    /// In the web navigation's order.
    static let screens = ["agent-studio", "logic-builder", "session-traces", "evals", "guardrails", "feedback",
                          "model-plane"]

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private var appearance: String { ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "Dark" }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "mimir-mac-\(name)-\(appearance.lowercased())"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func waitText(_ element: XCUIElement, contains text: String, timeout: TimeInterval = 10) -> Bool {
        let p = NSPredicate { _, _ in Nav.text(element).contains(text) }
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: nil)], timeout: timeout) == .completed
    }

    func testEveryScreenTheGateARunAndAnInterception() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 5)

        for slug in Self.screens {
            Nav.openScreen(app, slug)
            XCTAssertTrue(Nav.element(app, "mg.content.mimir.\(slug)").waitForExistence(timeout: 5), "no \(slug)")
            XCTAssertTrue(Nav.element(app, "mg.detail.\(slug)").waitForExistence(timeout: 10), "no inspector on \(slug)")
            keep(slug)
        }

        // The Agent Studio's proposal, authorised through the human gate.
        Nav.openScreen(app, "agent-studio")
        let authorise = Nav.element(app, "mg.mimir.studio.authorise")
        XCTAssertTrue(authorise.waitForExistence(timeout: 10), "no proposal")
        authorise.click()
        let badge = Nav.element(app, "mg.mimir.studio.authorised")
        XCTAssertTrue(badge.waitForExistence(timeout: 10), "the proposal was not authorised")
        XCTAssertTrue(waitText(badge, contains: "AUDIT #A-77121"), Nav.text(badge))
        keep("studio-authorised")

        // The selected suite run from the header.
        Nav.openScreen(app, "evals")
        Nav.element(app, "mg.mimir.evals.run-suite").click()
        let audit = Nav.element(app, "mg.mimir.evals.audit")
        XCTAssertTrue(audit.waitForExistence(timeout: 10), "no run in the inspector")
        XCTAssertTrue(waitText(audit, contains: "A-77122"), Nav.text(audit))
        XCTAssertTrue(waitText(Nav.element(app, "mg.mimir.evals.gate"), contains: "GATE · BLOCKING"))
        keep("evals-run")

        // The injection preset, stopped at the input rail.
        Nav.openScreen(app, "guardrails")
        Nav.element(app, "mg.mimir.guardrails.run").click()
        let verdict = Nav.element(app, "mg.mimir.guardrails.verdict")
        XCTAssertTrue(verdict.waitForExistence(timeout: 10), "no verdict")
        XCTAssertTrue(waitText(verdict, contains: "A-77123"), Nav.text(verdict))
        keep("guardrails-intercepted")

        // The Logic Builder: the gate cannot be deleted here; a step added is a draft.
        Nav.openScreen(app, "logic-builder")
        Nav.element(app, "mg.mimir.logic.step.6").click()
        XCTAssertTrue(Nav.element(app, "mg.mimir.logic.mandatory").waitForExistence(timeout: 5), "no MANDATORY note")
        XCTAssertFalse(Nav.element(app, "mg.mimir.logic.delete").isEnabled, "the gate's Delete is enabled")
        Nav.element(app, "mg.mimir.logic.step.3").click()
        Nav.element(app, "mg.mimir.logic.delete").click()
        XCTAssertTrue(Nav.element(app, "mg.mimir.logic.draft").waitForExistence(timeout: 5), "no draft badge")
        keep("logic-draft")
        Nav.element(app, "mg.mimir.logic.discard").click()
        XCTAssertTrue(Nav.element(app, "mg.mimir.logic.step.7").waitForExistence(timeout: 5), "the step is not back")
    }
}

import XCTest

/// Mimir's seven screens on the iPhone over the sample data: each opened from the
/// console's list and kept as a screenshot attachment in the simulator's appearance;
/// the Logic Builder's step settings in their sheet; then the Agent Studio's proposal
/// authorised through the human gate (A-77121), the selected suite run from the header
/// (A-77122, the gate blocking at 87.5 %) and the injection preset stopped at the input
/// rail (A-77123). [FR-C-1] [FR-MI-2] [FR-MI-3] [FR-MI-4] [FR-MI-7] [NFR-AI-1]
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

    private func keep(_ name: String) {
        let image = app.windows.firstMatch.screenshot().image
        let shot = XCTAttachment(image: image)
        shot.name = "mimir-iphone-\(name)-\(ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "dark")"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func reveal(_ e: XCUIElement) {
        for _ in 0..<6 where !e.isHittable { app.swipeUp() }
    }

    private func waitLabel(_ e: XCUIElement, contains text: String, timeout: TimeInterval = 10) -> Bool {
        let p = NSPredicate(format: "label CONTAINS %@", text)
        return XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: p, object: e)], timeout: timeout) == .completed
    }

    func testEveryScreenTheGateARunAndAnInterception() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
        AppNavigation.signIn(app)

        for slug in Self.screens {
            AppNavigation.open(app, console: 5, slug)
            XCTAssertTrue(element("mg.mimir.inspect").waitForExistence(timeout: 10), "nothing to inspect on \(slug)")
            keep(slug)
            if slug == "logic-builder" {
                // The selected step's inspector, with its settings, in the sheet.
                element("mg.mimir.inspect").tap()
                XCTAssertTrue(element("mg.mimir.logic.title").waitForExistence(timeout: 10), "no step settings")
                keep("logic-step-settings")
                app.buttons["mg.inspector.done"].firstMatch.tap()
            }
        }

        // The Agent Studio's proposal, authorised through the human gate.
        AppNavigation.open(app, console: 5, "agent-studio")
        let authorise = element("mg.mimir.studio.authorise")
        XCTAssertTrue(authorise.waitForExistence(timeout: AppNavigation.timeout), "no proposal")
        reveal(authorise)
        keep("studio-gate")
        authorise.tap()
        let badge = element("mg.mimir.studio.authorised")
        XCTAssertTrue(badge.waitForExistence(timeout: AppNavigation.timeout), "the proposal was not authorised")
        XCTAssertTrue(waitLabel(badge, contains: "AUDIT #A-77121"), badge.label)
        XCTAssertFalse(element("mg.mimir.studio.proposal").exists, "the proposal is still offered")
        keep("studio-authorised")

        // The selected suite (risk-calibration) run from the header: its cases in the sheet.
        AppNavigation.open(app, console: 5, "evals")
        element("mg.mimir.evals.run-suite").tap()
        let audit = element("mg.mimir.evals.audit")
        XCTAssertTrue(audit.waitForExistence(timeout: AppNavigation.timeout), "no run in the inspector")
        XCTAssertTrue(waitLabel(audit, contains: "A-77122"), audit.label)
        XCTAssertTrue(waitLabel(element("mg.mimir.evals.gate"), contains: "GATE · BLOCKING"))
        keep("evals-run")
        app.buttons["mg.inspector.done"].firstMatch.tap()

        // The injection preset, stopped at the input rail.
        AppNavigation.open(app, console: 5, "guardrails")
        let run = element("mg.mimir.guardrails.run")
        reveal(run)
        run.tap()
        let verdict = element("mg.mimir.guardrails.verdict")
        XCTAssertTrue(verdict.waitForExistence(timeout: AppNavigation.timeout), "no verdict")
        reveal(verdict)
        XCTAssertTrue(waitLabel(verdict, contains: "REFUSED · PROMPT-INJECTION"), verdict.label)
        XCTAssertTrue(waitLabel(verdict, contains: "A-77123"), verdict.label)
        keep("guardrails-intercepted")
    }
}

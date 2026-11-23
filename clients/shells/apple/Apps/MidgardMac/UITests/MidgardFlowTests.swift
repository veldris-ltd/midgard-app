import XCTest

/// Midgard's eleven screens over the sample data, as the web console shows them: each
/// opened from the navigation (a search run on Data search, a dry run on Policies), then
/// the dual-control request on Object Fabric: an object chosen, "Ground aircraft"
/// requested, and the enclosure's answer shown with its approval id, then found in the
/// audit lineage. Each screen is kept as a window screenshot attachment.
/// `MIDGARD_APPEARANCE=Light` (as `TEST_RUNNER_MIDGARD_APPEARANCE`) runs it in light mode
/// for this app only. [FR-C-1] [FR-C-4] [NFR-AI-1] [DD-1]
final class MidgardFlowTests: XCTestCase {
    private var app: XCUIApplication!

    static let screens = ["object-fabric", "data-search", "search-around", "policies", "audit", "identity", "keys",
                          "compute", "sdk", "observability", "resilience"]

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private var appearance: String { ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "Dark" }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = "midgard-mac-\(name)-\(appearance.lowercased())"
        shot.lifetime = .keepAlways
        add(shot)
    }

    func testEveryScreenAndTheDualControlRequest() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", appearance] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 1)

        for slug in Self.screens {
            Nav.openScreen(app, slug)
            XCTAssertTrue(Nav.element(app, "mg.content.midgard.\(slug)").waitForExistence(timeout: 5), "no \(slug) content")
            switch slug {
            case "data-search":
                Nav.element(app, "mg.midgard.data.search").click()
                XCTAssertTrue(Nav.element(app, "mg.midgard.data.count").waitForExistence(timeout: 10), "no search answer")
            case "policies":
                Nav.element(app, "mg.midgard.policies.evaluate").click()
                XCTAssertTrue(Nav.element(app, "mg.midgard.policies.decision").waitForExistence(timeout: 10), "no decision")
            case "search-around":
                XCTAssertTrue(Nav.element(app, "mg.midgard.search.audited").waitForExistence(timeout: 10), "no traversal")
            default:
                XCTAssertTrue(Nav.element(app, "mg.midgard.inspector.title").waitForExistence(timeout: 10),
                              "no inspector on \(slug)")
            }
            keep(slug)
        }

        // The action: request a second person's approval of a human-gated action.
        Nav.openScreen(app, "object-fabric")
        let target = Nav.element(app, "mg.midgard.fabric.target")
        XCTAssertTrue(target.waitForExistence(timeout: 10))
        target.click()
        let object = app.menuItems["aircraft/1000"]
        XCTAssertTrue(object.waitForExistence(timeout: 5), "no aircraft/1000 to act on")
        object.click()
        let ground = Nav.element(app, "mg.midgard.fabric.action.ground-aircraft")
        XCTAssertTrue(ground.waitForExistence(timeout: 5))
        ground.click()
        let notice = Nav.element(app, "mg.midgard.fabric.notice")
        XCTAssertTrue(notice.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(Nav.text(notice).hasPrefix("APR-1 REQUESTED · Ground aircraft · aircraft/1000"),
                      "notice says: \(Nav.text(notice))")
        XCTAssertTrue(Nav.element(app, "mg.midgard.approval.APR-1").waitForExistence(timeout: 5))
        keep("object-fabric-requested")

        // Audited in the mock's numbering, signed by this device.
        Nav.openScreen(app, "audit")
        XCTAssertTrue(Nav.element(app, "mg.midgard.audit.table.row.AUD-1043").waitForExistence(timeout: 10))
        keep("audit-after-request")
    }
}

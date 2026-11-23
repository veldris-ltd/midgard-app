import XCTest

/// Midgard's eleven screens on the iPhone over the sample data: each opened from the
/// console's list (a search run on Data search, a dry run on Policies), its inspector
/// opened as a sheet on one, then the dual-control request on Object Fabric: an object
/// chosen in the inspector, "Ground aircraft" requested, and the enclosure's answer
/// shown with its approval id. Each screen is kept as a screenshot attachment, in the
/// simulator's appearance. [FR-C-1] [FR-C-4] [NFR-AI-1] [DD-1]
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

    private func keep(_ name: String) {
        let image = app.windows.firstMatch.screenshot().image
        let shot = XCTAttachment(image: image)
        shot.name = "midgard-iphone-\(name)-\(ProcessInfo.processInfo.environment["MIDGARD_APPEARANCE"] ?? "dark")"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func open(_ slug: String) {
        AppNavigation.selectConsole(app, 1)
        let row = app.buttons.matching(NSPredicate(format: "identifier MATCHES %@", "mg\\.[0-9]+\\.midgard\\.\(slug)")).firstMatch
        AppNavigation.tapClearOfConsoleBar(app, row)
        XCTAssertTrue(element("mg.content.midgard.\(slug)").waitForExistence(timeout: AppNavigation.timeout),
                      "the \(slug) screen did not open")
    }

    func testEveryScreenAndTheDualControlRequest() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
        AppNavigation.signIn(app)

        for slug in Self.screens {
            open(slug)
            switch slug {
            case "data-search":
                element("mg.midgard.data.search").tap()
                XCTAssertTrue(element("mg.midgard.data.count").waitForExistence(timeout: 10), "no search answer")
            case "policies":
                let evaluate = element("mg.midgard.policies.evaluate")
                for _ in 0..<3 where !evaluate.isHittable { app.swipeUp() }
                evaluate.tap()
                XCTAssertTrue(element("mg.midgard.policies.decision").waitForExistence(timeout: 10), "no decision")
            case "search-around":
                XCTAssertTrue(element("mg.midgard.search.audited").waitForExistence(timeout: 10) ||
                              element("mg.midgard.inspect").exists, "no traversal")
            default:
                XCTAssertTrue(element("mg.midgard.inspect").waitForExistence(timeout: 10), "nothing to inspect on \(slug)")
            }
            keep(slug)
            if slug == "keys" {
                // One inspector, as the sheet the iPhone opens it in.
                element("mg.midgard.inspect").tap()
                XCTAssertTrue(element("mg.midgard.inspector.title").waitForExistence(timeout: 10))
                keep("keys-inspector")
                app.buttons["mg.inspector.done"].firstMatch.tap()
            }
        }

        // The action: request a second person's approval of a human-gated action.
        open("object-fabric")
        element("mg.midgard.inspect").tap()
        let target = element("mg.midgard.fabric.target")
        XCTAssertTrue(target.waitForExistence(timeout: 10))
        target.tap()
        let object = app.buttons["aircraft/1000"]
        XCTAssertTrue(object.waitForExistence(timeout: 5), "no aircraft/1000 to act on")
        object.tap()
        let ground = element("mg.midgard.fabric.action.ground-aircraft")
        XCTAssertTrue(ground.waitForExistence(timeout: 5))
        ground.tap()
        let notice = element("mg.midgard.fabric.notice")
        XCTAssertTrue(notice.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(notice.label.hasPrefix("APR-1 REQUESTED · Ground aircraft · aircraft/1000"), "notice says: \(notice.label)")
        keep("object-fabric-requested")
    }
}

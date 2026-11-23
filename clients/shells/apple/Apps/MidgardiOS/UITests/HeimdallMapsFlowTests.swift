import XCTest

/// Heimdall's view switch on the iPhone (Phase I), as the web console's: 2D, then MAP (the
/// tracks on the vector map), then 3D (the globe), a REGION preset from each view's
/// REGION menu, and a track chosen from the globe's TRACKS menu, which opens the
/// inspector on it. Each view is kept as a screenshot, the globe also in light mode.
/// Nothing here depends on the map tiles arriving: offline, the views fall back to the
/// bundled outlines. [FR-H-1] [AC-H-1]
final class HeimdallMapsFlowTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        XCUIDevice.shared.appearance = .dark
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
    }

    override func tearDown() {
        XCUIDevice.shared.appearance = .dark
        app.terminate()
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    /// Lets the view draw (and the map tiles arrive, when there is a network).
    private func settle(_ seconds: TimeInterval) {
        _ = XCTWaiter.wait(for: [XCTestExpectation(description: "settle")], timeout: seconds)
    }

    /// Chooses `item` from the menu `menu`.
    private func choose(_ menu: String, _ item: String) {
        let button = element(menu)
        XCTAssertTrue(button.waitForExistence(timeout: AppNavigation.timeout), "no \(menu)")
        button.tap()
        let entry = app.buttons.matching(NSPredicate(format: "label BEGINSWITH %@", item)).firstMatch
        XCTAssertTrue(entry.waitForExistence(timeout: AppNavigation.timeout), "no \(item) in \(menu)")
        entry.tap()
    }

    private func waitFor(_ id: String, labelContaining text: String) {
        let e = element(id)
        XCTAssertTrue(e.waitForExistence(timeout: AppNavigation.timeout), "no \(id)")
        let has = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS %@", text), object: e)
        XCTAssertEqual(XCTWaiter.wait(for: [has], timeout: 10), .completed, "\(id) says: \(e.label)")
    }

    func testTheViewSwitchMapAndGlobe() {
        // 2D: the reference layout, with the switch in the navigation bar.
        AppNavigation.openPicture(app)
        for id in ["2d", "map", "3d"] {
            XCTAssertTrue(app.buttons["mg.heimdall.view.\(id)"].exists, "no \(id) in the view switch")
        }
        keep("iphone-2d")

        // MAP: a preset from the REGION menu.
        AppNavigation.openMap(app)
        choose("mg.heimdall.region", "UK & IRELAND")
        waitFor("mg.heimdall.map.summary", labelContaining: "UK & IRELAND")
        settle(10)
        keep("iphone-map")

        // 3D: a preset and a style; then a track from the TRACKS menu opens its inspector.
        AppNavigation.openThreeD(app)
        choose("mg.heimdall.region", "NORTH SEA")
        waitFor("mg.heimdall.3d.summary", labelContaining: "NORTH SEA")
        choose("mg.heimdall.3d.style", "SATELLITE")
        settle(12)
        keep("iphone-3d")
        XCUIDevice.shared.appearance = .light
        settle(6)
        keep("iphone-3d-light")
        XCUIDevice.shared.appearance = .dark
        choose("mg.heimdall.tracks", "E-2214")
        let inspected = element("mg.heimdall.inspector.track")
        XCTAssertTrue(inspected.waitForExistence(timeout: AppNavigation.timeout), "the inspector did not open")
        XCTAssertEqual(inspected.label, "E-2214")
    }
}

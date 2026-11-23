import XCTest

/// Heimdall's view switch on the Mac (Phase I), as the web console's: 2D, then MAP (the
/// tracks on the vector map), then 3D (the globe), a REGION preset in each, and a track
/// chosen from the globe's track list, which the inspector then shows. Each view is kept
/// as a window screenshot. Nothing here depends on the map tiles arriving: offline, the
/// views fall back to the bundled outlines. [FR-H-1] [AC-H-1]
final class HeimdallMapsFlowTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    /// Lets the view draw (and the map tiles arrive, when there is a network).
    private func settle(_ seconds: TimeInterval) {
        _ = XCTWaiter.wait(for: [XCTestExpectation(description: "settle")], timeout: seconds)
    }

    func testTheViewSwitchMapAndGlobe() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", "Dark"] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 3)

        // 2D: the reference layout, with its tracks.
        XCTAssertTrue(Nav.element(app, "mg.heimdall.track.E-0871").waitForExistence(timeout: 10))
        for id in ["2d", "map", "3d"] {
            XCTAssertTrue(Nav.element(app, "mg.heimdall.view.\(id)").exists, "no \(id) in the view switch")
        }
        keep("mac-2d")

        // MAP: a preset, and the tracks as markers on it.
        Nav.openMap(app)
        let uk = Nav.element(app, "mg.heimdall.region.uk-ireland")
        XCTAssertTrue(uk.waitForExistence(timeout: 5))
        uk.click()
        let mapSummary = Nav.element(app, "mg.heimdall.map.summary")
        XCTAssertTrue(mapSummary.waitForExistence(timeout: 5))
        XCTAssertTrue(Nav.wait(mapSummary, labelPrefix: "Map at zoom", timeout: 10), mapSummary.label)
        XCTAssertTrue(Nav.element(app, "mg.heimdall.map.track.E-0871").exists)
        XCTAssertTrue(Nav.element(app, "mg.heimdall.honesty").exists)
        settle(10)
        keep("mac-map")

        // 3D: the globe, a preset, then a track from the list: the inspector follows.
        Nav.openThreeD(app)
        let northSea = Nav.element(app, "mg.heimdall.region.north-sea")
        XCTAssertTrue(northSea.waitForExistence(timeout: 5))
        northSea.click()
        let summary = Nav.element(app, "mg.heimdall.3d.summary")
        XCTAssertTrue(summary.waitForExistence(timeout: 5))
        let towards = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label CONTAINS %@", "NORTH SEA"), object: summary)
        XCTAssertEqual(XCTWaiter.wait(for: [towards], timeout: 10), .completed, summary.label)
        Nav.element(app, "mg.heimdall.3d.style.satellite").click()
        settle(12)
        keep("mac-3d")
        // With the terrain tiles online the close-up relief presets are listed too.
        let snowdonia = Nav.element(app, "mg.heimdall.region.snowdonia")
        if snowdonia.exists {
            Nav.element(app, "mg.heimdall.3d.style.topographic").click()
            snowdonia.click()
            settle(15)
            keep("mac-3d-topographic")
        }
        // Flying to a track is a move the tracks decide: map tiles pause, as on the web.
        Nav.element(app, "mg.heimdall.3d.track.E-2214").click()
        let inspected = Nav.element(app, "mg.heimdall.inspector.track")
        XCTAssertTrue(Nav.wait(inspected, value: "E-2214", timeout: 10) || Nav.text(inspected) == "E-2214",
                      "the inspector shows \(Nav.text(inspected))")
    }
}

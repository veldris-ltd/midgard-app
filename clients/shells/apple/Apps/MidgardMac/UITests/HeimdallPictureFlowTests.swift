import XCTest

/// Heimdall's Operating Picture over the sample data, as the web console behaves: the
/// five tracks, E-0871 selected with its three ranked options, another track selected
/// from the picture, Reject recorded on screen, and Authorise applied through the core
/// with the audit id the web mock gives (D-4473). Each state is kept as a window
/// screenshot attachment.
/// [FR-H-4] [FR-H-5] [AC-C-13]
final class HeimdallPictureFlowTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private func launch() {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"] + Nav.freshLaunch
        app.launch()
        Nav.openConsole(app, 3)
    }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        shot.name = name
        shot.lifetime = .keepAlways
        add(shot)
    }

    func testThePictureAndItsGateBehaveAsTheWebConsole() {
        launch()
        for id in ["E-2214", "E-2231", "E-0442", "E-0871", "E-1108"] {
            XCTAssertTrue(Nav.element(app, "mg.heimdall.track.\(id)").waitForExistence(timeout: 10), "no track \(id)")
        }
        let inspected = Nav.element(app, "mg.heimdall.inspector.track")
        XCTAssertTrue(inspected.waitForExistence(timeout: 5))
        XCTAssertEqual(Nav.text(inspected), "E-0871")
        for rank in 1...3 { XCTAssertTrue(Nav.element(app, "mg.heimdall.option.\(rank)").exists, "no option \(rank)") }
        XCTAssertTrue(Nav.element(app, "mg.heimdall.honesty").exists)
        keep("heimdall-mac")

        // Another track: its own inspector, with no options.
        Nav.element(app, "mg.heimdall.track.E-2214").click()
        XCTAssertTrue(Nav.wait(inspected, value: "E-2214") || Nav.text(inspected) == "E-2214")
        XCTAssertFalse(Nav.element(app, "mg.heimdall.authorise").exists)

        // Back to E-0871: Reject is recorded on screen; a new option resets the gate.
        Nav.element(app, "mg.heimdall.track.E-0871").click()
        let reject = Nav.element(app, "mg.heimdall.reject")
        // In a shorter window (an edition banner above) the gate is below the fold.
        for _ in 0..<3 where !reject.isHittable {
            Nav.element(app, "mg.detail.operating-picture").scroll(byDeltaX: 0, deltaY: -300)
        }
        reject.click()
        XCTAssertTrue(Nav.element(app, "mg.heimdall.gate.rejected").waitForExistence(timeout: 5))
        Nav.element(app, "mg.heimdall.option.3").click()
        XCTAssertTrue(Nav.element(app, "mg.heimdall.hold").waitForExistence(timeout: 5))
        Nav.element(app, "mg.heimdall.option.1").click()

        // Authorise: applied by the core, with the audit id the mock gives.
        let authorise = Nav.element(app, "mg.heimdall.authorise")
        for _ in 0..<3 where !authorise.isHittable {
            Nav.element(app, "mg.detail.operating-picture").scroll(byDeltaX: 0, deltaY: -300)
        }
        authorise.click()
        let done = Nav.element(app, "mg.heimdall.gate.authorised")
        XCTAssertTrue(done.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(Nav.text(done).contains("AUDIT #D-4473"), "gate says: \(Nav.text(done))")
        keep("heimdall-mac-authorised")
    }
}

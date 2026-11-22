import XCTest

/// Heimdall's Operating Picture over the sample data on the iPhone: the picture first,
/// with the five tracks; a track opens the inspector as a sheet; Authorise is applied
/// through the core with the audit id the web mock gives (D-4473). [FR-H-4] [FR-H-5]
/// [AC-C-13]
final class HeimdallPictureFlowTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
    }

    override func tearDown() {
        app.terminate()
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    func testThePictureAndItsInspectorBehaveAsTheWebConsole() {
        AppNavigation.openPicture(app)
        for id in ["E-2214", "E-2231", "E-0442", "E-0871", "E-1108"] {
            XCTAssertTrue(element("mg.heimdall.track.\(id)").exists, "no track \(id)")
        }
        XCTAssertTrue(element("mg.classification").exists)

        // A track opens its inspector as a sheet.
        element("mg.heimdall.track.E-0871").tap()
        let inspected = element("mg.heimdall.inspector.track")
        XCTAssertTrue(inspected.waitForExistence(timeout: AppNavigation.timeout))
        XCTAssertEqual(inspected.label, "E-0871")
        XCTAssertTrue(element("mg.heimdall.option.1").exists)

        // Authorise: applied by the core, with the audit id the mock gives.
        let authorise = element("mg.heimdall.authorise")
        for _ in 0..<3 where !authorise.isHittable { app.swipeUp() }
        authorise.tap()
        let done = element("mg.heimdall.gate.authorised")
        XCTAssertTrue(done.waitForExistence(timeout: AppNavigation.timeout), app.debugDescription)
        XCTAssertTrue(done.label.contains("AUDIT #D-4473"), "gate says: \(done.label)")
    }

    /// Holds the picture, then its inspector, on screen for `xcrun simctl io` screenshots
    /// (run with TEST_RUNNER_MIDGARD_SCREENSHOT_HOLD=<seconds>); skipped otherwise.
    func testHoldForScreenshots() throws {
        guard let hold = ProcessInfo.processInfo.environment["MIDGARD_SCREENSHOT_HOLD"].flatMap(Double.init) else {
            throw XCTSkip("screenshot hold not requested")
        }
        AppNavigation.openPicture(app)
        Thread.sleep(forTimeInterval: hold)
        element("mg.heimdall.inspect").tap()
        XCTAssertTrue(element("mg.heimdall.inspector.track").waitForExistence(timeout: AppNavigation.timeout))
        Thread.sleep(forTimeInterval: hold)
    }
}

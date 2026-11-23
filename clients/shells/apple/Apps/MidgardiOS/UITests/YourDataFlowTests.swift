import XCTest

/// Your Data on the iPhone (Phase J), kept as screenshots: Your Data from the session
/// menu with files imported over the sample data (from `MIDGARD_UITEST_IMPORT`, as the
/// Files picker would give them: the system picker is not driven), their layer on
/// Heimdall's picture in 2D and MAP, and a refused file named with its line. Skipped
/// when the files are not given (as `TEST_RUNNER_MIDGARD_UITEST_IMPORT_OK` / `_BAD`).
/// [DD-1] [RC-10] [FR-H-1]
final class YourDataFlowTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
        XCUIDevice.shared.appearance = .dark
    }

    override func tearDown() {
        app?.terminate()
    }

    private func keep(_ name: String) {
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "yourdata-ios-\(name)"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func launch(_ imports: String, feed: String? = nil) {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"]
        app.launchEnvironment["MIDGARD_UITEST_IMPORT"] = imports
        if let feed { app.launchEnvironment["MIDGARD_UITEST_FEED"] = feed }
        app.launch()
        AppNavigation.signIn(app)
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    private func text(_ e: XCUIElement) -> String {
        if let v = e.value as? String, !v.isEmpty { return v }
        return e.label
    }

    private func settle(_ seconds: TimeInterval) {
        _ = XCTWaiter.wait(for: [XCTestExpectation(description: "settle")], timeout: seconds)
    }

    func testImportedFilesAndTheirLayerOnThePicture() throws {
        guard let files = env["MIDGARD_UITEST_IMPORT_OK"] else { throw XCTSkip("MIDGARD_UITEST_IMPORT_OK not set") }
        launch(files, feed: env["MIDGARD_UITEST_FEED"])
        AppNavigation.sessionMenu(app, "mg.session.yourdata")
        let notice = element("mg.yourdata.notice")
        XCTAssertTrue(notice.waitForExistence(timeout: 20), "no import result")
        XCTAssertTrue(text(notice).hasPrefix("Imported"), text(notice))
        keep("import")
        app.swipeUp()
        keep("gateway-and-feeds")
        app.buttons["mg.yourdata.done"].tap()

        AppNavigation.openPicture(app)
        settle(3)
        keep("picture-2d")
        app.swipeUp()
        keep("picture-layers")
        AppNavigation.openMap(app)
        settle(6)
        keep("picture-map")
    }

    func testARefusedFileIsNamedWithItsLine() throws {
        guard let ok = env["MIDGARD_UITEST_IMPORT_OK"], let bad = env["MIDGARD_UITEST_IMPORT_BAD"] else {
            throw XCTSkip("MIDGARD_UITEST_IMPORT_OK and _BAD not set")
        }
        launch(ok + "||" + bad)
        AppNavigation.sessionMenu(app, "mg.session.yourdata")
        let notice = element("mg.yourdata.notice")
        let refused = NSPredicate { [unowned self] _, _ in text(notice).hasPrefix("Import refused") }
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: refused, object: nil)], timeout: 20), .completed,
                       text(notice))
        XCTAssertTrue(text(notice).contains("line 1"), text(notice))
        keep("refusal")
    }
}

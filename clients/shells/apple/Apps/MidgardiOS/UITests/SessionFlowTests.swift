import XCTest

/// The session on the shipped app, over the release's sample data (owner decision 22
/// Nov 2026): enrol, the mock personas, the web consoles' demonstration notice, all six
/// consoles, offline, lock and unlock. [FR-C-1] [FR-C-5] [FR-C-9] [AC-C-13]
final class SessionFlowTests: XCTestCase {
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

    func testSampleDataSessionFlow() {
        // Enrol, then the personas the mocks name.
        XCTAssertTrue(AppNavigation.enrolButton(app).waitForExistence(timeout: AppNavigation.timeout))
        AppNavigation.enrolButton(app).tap()
        XCTAssertTrue(AppNavigation.personaButton(app).waitForExistence(timeout: AppNavigation.timeout))
        for (n, id) in ["k.rao", "j.benjamin", "k.osei", "a.holt"].enumerated() {
            XCTAssertTrue(app.buttons["mg.\(n).persona.\(id)"].exists, "no persona \(id)")
        }
        AppNavigation.personaButton(app).tap()
        XCTAssertTrue(AppNavigation.consoleMarker(app).waitForExistence(timeout: AppNavigation.timeout))

        // The banner is the web consoles' own demonstration notice.
        let banner = element("mg.banner.demo")
        XCTAssertTrue(banner.exists, "no demonstration banner")
        XCTAssertEqual(banner.label,
                       "Demonstration data. This console runs on built-in sample data and is not connected to an enclosure.")
        XCTAssertFalse(element("mg.banner.nosource").exists)

        // Each console opens at its list of screens.
        let firstScreens = ["midgard.object-fabric", "admin.overview", "heimdall.operating-picture",
                            "forge.connectors", "mimir.model-plane", "bifrost.release-console"]
        for (index, first) in firstScreens.enumerated() {
            AppNavigation.selectConsole(app, index + 1)
            XCTAssertTrue(app.buttons["mg.1.\(first)"].waitForExistence(timeout: AppNavigation.timeout),
                          "console \(index + 1) does not list \(first)")
        }

        // Offline: the sync chip says so.
        AppNavigation.sessionMenu(app, "mg.session.offline")
        let offline = NSPredicate(format: "label BEGINSWITH 'Sync: offline'")
        expectation(for: offline, evaluatedWith: element("mg.chip.sync"))
        waitForExpectations(timeout: AppNavigation.timeout)

        // Lock, then unlock.
        AppNavigation.sessionMenu(app, "mg.session.lock")
        XCTAssertTrue(AppNavigation.unlockButton(app).waitForExistence(timeout: AppNavigation.timeout))
        XCTAssertFalse(AppNavigation.consoleMarker(app).exists, "the consoles are still shown while locked")
        AppNavigation.unlockButton(app).tap()
        XCTAssertTrue(AppNavigation.consoleMarker(app).waitForExistence(timeout: AppNavigation.timeout))
        XCTAssertTrue(element("mg.banner.demo").exists)
    }

    func testEveryScreenOpensItsPlaceholder() {
        AppNavigation.signIn(app)
        let counts = [11, 8, 6, 7, 7, 6]
        XCTAssertEqual(counts.reduce(0, +), 45)
        for (index, count) in counts.enumerated() {
            AppNavigation.selectConsole(app, index + 1)
            let rows = app.buttons.matching(NSPredicate(format: "identifier MATCHES %@",
                                                        "mg\\.[0-9]+\\.\(AppNavigation.consoles[index].lowercased())\\..*"))
            XCTAssertEqual(rows.count, count, "\(AppNavigation.consoles[index]) lists \(rows.count) screens")
        }
        AppNavigation.openDetail(app)
        XCTAssertTrue(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'Phase C'")).firstMatch.exists)
    }

    func testSearchFiltersTheScreens() {
        AppNavigation.signIn(app)
        AppNavigation.selectConsole(app, 1)
        let field = element("mg.search.midgard")
        XCTAssertTrue(field.waitForExistence(timeout: AppNavigation.timeout))
        field.tap()
        field.typeText("audit")
        XCTAssertTrue(app.buttons["mg.1.midgard.audit"].waitForExistence(timeout: AppNavigation.timeout))
        XCTAssertFalse(app.buttons["mg.1.midgard.object-fabric"].exists)
    }

    func testBackgroundLocks() {
        AppNavigation.signIn(app)
        XCUIDevice.shared.press(.home)
        XCTAssertTrue(app.wait(for: .runningBackground, timeout: AppNavigation.timeout)
            || app.wait(for: .runningBackgroundSuspended, timeout: AppNavigation.timeout))
        app.activate()
        XCTAssertTrue(AppNavigation.unlockButton(app).waitForExistence(timeout: AppNavigation.timeout),
                      "the app did not lock in the background")
    }

    /// The key commands are the app's, at the end of the responder chain: they fire
    /// while the search field is being edited, which holds first responder.
    func testCommandDigitsWorkWhileSearching() {
        AppNavigation.signIn(app)
        AppNavigation.selectConsole(app, 1)
        // The simulator can drop a launch's first hardware key event (see below).
        app.typeKey("x", modifierFlags: [])
        let field = element("mg.search.midgard")
        XCTAssertTrue(field.waitForExistence(timeout: AppNavigation.timeout))
        field.tap()
        field.typeText("audit")
        XCTAssertTrue(app.buttons["mg.1.midgard.audit"].waitForExistence(timeout: AppNavigation.timeout))
        XCTAssertEqual(field.value(forKey: "hasKeyboardFocus") as? Bool, true, "the search field is being edited")
        app.typeKey("2", modifierFlags: .command)
        let marker = AppNavigation.consoleMarker(app)
        expectation(for: NSPredicate(format: "value == '2'"), evaluatedWith: marker)
        waitForExpectations(timeout: 5)
        XCTAssertEqual(marker.label, "Console 2 of 6: Admin")
    }

    func testCommandDigitsSelectConsoles() {
        AppNavigation.signIn(app)
        let marker = AppNavigation.consoleMarker(app)
        // The simulator can drop the first hardware key event of a launch (seen in a key
        // log in the app), so one plain key first; with no text field focused it does
        // nothing. With Full Keyboard Access on, ⌘-digit presses often never reached the
        // app at all: run this with it off (the simulator's default).
        app.typeKey("x", modifierFlags: [])
        for n in [3, 6, 1, 5, 2, 4] {
            app.typeKey("\(n)", modifierFlags: .command)
            XCTAssertEqual(marker.value as? String, "\(n)", "⌘\(n) must select console \(n)")
            // The status line says it in words.
            XCTAssertEqual(marker.label, "Console \(n) of 6: \(AppNavigation.consoles[n - 1])")
        }
    }
}

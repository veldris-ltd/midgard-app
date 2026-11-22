import XCTest

/// The session end to end over the release's sample data (owner decision 22 Nov 2026):
/// enrol, see the mock personas, sign in, see the web consoles' demonstration notice,
/// open each console, go offline and back, lock and unlock. [FR-C-1] [FR-C-5] [FR-C-9]
/// [AC-C-13]
final class SessionFlowTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUp() {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting"] + Nav.freshLaunch
        app.launch()
    }

    override func tearDown() {
        app.terminate()
    }

    func testSampleDataSessionFlow() {
        // Enrol, then the sign-in screen offers the personas the mocks name.
        Nav.toSignIn(app)
        for (n, id) in ["k.rao", "j.benjamin", "k.osei", "a.holt"].enumerated() {
            XCTAssertTrue(Nav.element(app, "mg.\(n).persona.\(id)").waitForExistence(timeout: 10), "no persona \(id)")
        }
        let osei = Nav.personaButton(app)
        XCTAssertTrue(osei.label.hasPrefix("K. Osei"), "persona label: \(osei.label)")
        osei.click()

        // The banner is the web consoles' own demonstration notice.
        let banner = Nav.element(app, "mg.banner.demo")
        XCTAssertTrue(banner.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertEqual(Nav.text(banner),
                       "Demonstration data. This console runs on built-in sample data and is not connected to an enclosure.")
        XCTAssertFalse(Nav.element(app, "mg.banner.nosource").exists)

        // Each console, from the sidebar, shows its first screen's placeholder.
        let firsts = ["object-fabric", "overview", "operating-picture", "connectors", "model-plane", "release-console"]
        for n in 1...6 {
            Nav.openConsole(app, n)
            XCTAssertTrue(Nav.element(app, "mg.detail.\(firsts[n - 1])").waitForExistence(timeout: 5),
                          "console \(n) does not show its first screen")
        }
        // ⌘1 goes back to Midgard.
        app.typeKey("1", modifierFlags: .command)
        XCTAssertTrue(Nav.wait(Nav.element(app, "mg.console.selected"), value: "1"))

        // Offline from the persona menu: the sync chip shows OFFLINE (its visible words,
        // the static text's value) and speaks "Sync: offline" (its label).
        let sync = Nav.element(app, "mg.chip.sync")
        XCTAssertTrue(Nav.wait(sync, labelPrefix: "Sync: synced"), "sync chip: \(sync.label)")
        XCTAssertTrue(Nav.wait(sync, valuePrefix: "SYNCED"), "sync chip shows: \(String(describing: sync.value))")
        Nav.element(app, "mg.0.persona-menu").click()
        app.menuItems["Go offline"].click()
        XCTAssertTrue(Nav.wait(sync, labelPrefix: "Sync: offline"), "sync chip: \(sync.label)")
        XCTAssertTrue(Nav.wait(sync, valuePrefix: "OFFLINE"), "sync chip shows: \(String(describing: sync.value))")
        // And back online with ⇧⌘O.
        app.typeKey("o", modifierFlags: [.command, .shift])
        XCTAssertTrue(Nav.wait(sync, labelPrefix: "Sync: synced"), "sync chip: \(sync.label)")
        XCTAssertTrue(Nav.wait(sync, valuePrefix: "SYNCED"), "sync chip shows: \(String(describing: sync.value))")

        // ⌘L locks; Unlock returns to the shell as the same persona.
        app.typeKey("l", modifierFlags: .command)
        let unlock = Nav.element(app, "mg.0.unlock")
        XCTAssertTrue(unlock.waitForExistence(timeout: 10))
        XCTAssertFalse(Nav.element(app, "mg.console.selected").exists)
        unlock.click()
        XCTAssertTrue(Nav.element(app, "mg.console.selected").waitForExistence(timeout: 20))
        XCTAssertTrue(Nav.element(app, "mg.banner.demo").exists)
    }
}

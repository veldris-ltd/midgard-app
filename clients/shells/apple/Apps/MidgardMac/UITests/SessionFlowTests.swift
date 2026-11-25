import XCTest

/// The session end to end over the release's sample data (owner decision 22 Nov 2026):
/// enrol, see the mock personas, sign in, see the data chip name the sample workspace,
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

        // No banner: the data chip names the sample workspace (owner decision 25 Nov 2026).
        let chip = Nav.element(app, "mg.chip.source")
        XCTAssertTrue(chip.waitForExistence(timeout: 20), app.debugDescription)
        XCTAssertTrue(Nav.wait(chip, value: "SAMPLE WORKSPACE"), "data chip shows: \(String(describing: chip.value))")
        XCTAssertFalse(Nav.element(app, "mg.banner.demo").exists)
        XCTAssertFalse(Nav.element(app, "mg.banner.nosource").exists)

        // Each console, from the sidebar, shows its first screen, built (Phases C to H):
        // its content and inspector in the web's frame, no placeholder anywhere.
        let firsts = ["object-fabric", "overview", "operating-picture", "connectors", "model-plane", "release-console"]
        for n in 1...6 {
            Nav.openConsole(app, n)
            XCTAssertTrue(Nav.element(app, "mg.detail.\(firsts[n - 1])").waitForExistence(timeout: 5),
                          "console \(n) does not show its first screen")
            XCTAssertFalse(app.staticTexts.containing(NSPredicate(format: "label CONTAINS 'arrives in Phase'"))
                .firstMatch.exists, "console \(n) shows a placeholder")
        }
        XCTAssertTrue(Nav.element(app, "mg.content.bifrost.release-console").exists, "Bifrost's screen is not built")
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
        XCTAssertTrue(Nav.element(app, "mg.chip.source").exists)
    }
}

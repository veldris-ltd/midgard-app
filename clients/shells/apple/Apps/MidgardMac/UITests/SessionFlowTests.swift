import XCTest

/// The session end to end with no data source (owner decision 19 Nov 2026): enrol,
/// sign in as the local user, see the "No data source connected" banner, open each
/// console, go offline and back, lock and unlock. [FR-C-1] [FR-C-5] [FR-C-9] [AC-C-13]
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

    func testNoDataSessionFlow() {
        // Enrol, then the sign-in screen offers the one local user.
        Nav.toSignIn(app)
        let local = Nav.element(app, "mg.0.persona.local")
        XCTAssertTrue(local.waitForExistence(timeout: 10))
        XCTAssertTrue(local.label.hasPrefix("Local user"), "persona label: \(local.label)")
        XCTAssertFalse(Nav.element(app, matching: "mg\\.1\\.persona\\..*").exists, "only one persona with no data")
        local.click()

        // The banner says there is no source.
        let banner = Nav.element(app, "mg.banner.nosource")
        XCTAssertTrue(banner.waitForExistence(timeout: 20))
        XCTAssertEqual(banner.label, "No data source connected")
        XCTAssertFalse(Nav.element(app, "mg.banner.demo").exists)

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

        // Offline from the persona menu: the sync chip says OFFLINE (spoken "Sync: offline").
        let sync = Nav.element(app, "mg.chip.sync")
        XCTAssertTrue(Nav.wait(sync, labelPrefix: "Sync: synced"), "sync chip: \(sync.label)")
        Nav.element(app, "mg.0.persona-menu").click()
        app.menuItems["Go offline"].click()
        XCTAssertTrue(Nav.wait(sync, labelPrefix: "Sync: offline"), "sync chip: \(sync.label)")
        // And back online with ⇧⌘O.
        app.typeKey("o", modifierFlags: [.command, .shift])
        XCTAssertTrue(Nav.wait(sync, labelPrefix: "Sync: synced"), "sync chip: \(sync.label)")

        // ⌘L locks; Unlock returns to the shell as the same persona.
        app.typeKey("l", modifierFlags: .command)
        let unlock = Nav.element(app, "mg.0.unlock")
        XCTAssertTrue(unlock.waitForExistence(timeout: 10))
        XCTAssertFalse(Nav.element(app, "mg.console.selected").exists)
        unlock.click()
        XCTAssertTrue(Nav.element(app, "mg.console.selected").waitForExistence(timeout: 20))
        XCTAssertTrue(Nav.element(app, "mg.banner.nosource").exists)
    }
}

import XCTest

/// The edition the app under test says it is (Task B7), against `Edition`, which the
/// UI-test target defines in UITests-Maps (the one edition since 23 Nov 2026): no banner
/// over the sample workspace but its data chip and invitation (owner decision 25 Nov
/// 2026), Heimdall's view switch (2D, MAP, 3D), and About. The branches for an app without
/// them stay, so the tests still hold if an edition without them returns. [FR-C-1] [AC-C-13]
final class EditionTests: XCTestCase {
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

    func testTheSampleWorkspaceHasADataChipAndAnInvitationNoBanner() {
        Nav.signedIn(app)
        let chip = Nav.element(app, "mg.chip.source")
        XCTAssertTrue(chip.waitForExistence(timeout: 10), app.debugDescription)
        XCTAssertTrue(Nav.wait(chip, value: "SAMPLE WORKSPACE"), "data chip shows: \(String(describing: chip.value))")
        XCTAssertEqual(chip.label, "Data: the sample workspace, built into the app")
        for id in ["mg.banner.demo", "mg.banner.edition3d", "mg.banner.editionmaps", "mg.banner.live", "mg.banner.nosource"] {
            XCTAssertFalse(Nav.element(app, id).exists, "\(id) is shown")
        }
        // The invitation: its words, and Connect your data opens Your Data (⇧⌘D's window).
        let text = Nav.element(app, "mg.invite.text")
        XCTAssertTrue(text.waitForExistence(timeout: 5), app.debugDescription)
        XCTAssertEqual(Nav.text(text),
                       "Exploring the sample workspace. Connect your own data server or import JSON to see your data.")
        Nav.element(app, "mg.invite.connect").click()
        XCTAssertTrue(Nav.element(app, "mg.yourdata").waitForExistence(timeout: 10), "Your Data did not open")
        app.windows["Your Data"].firstMatch.buttons[XCUIIdentifierCloseWindow].firstMatch.click()
        // Dismissed, it goes; the chip stays.
        let dismiss = Nav.element(app, "mg.invite.dismiss")
        XCTAssertTrue(dismiss.waitForExistence(timeout: 10))
        dismiss.click()
        XCTAssertFalse(Nav.element(app, "mg.invite").waitForExistence(timeout: 2), "the invitation is still shown")
        XCTAssertTrue(chip.exists)
    }

    func testHeimdallOffersTheViewSwitchOnlyIn3D() {
        Nav.openConsole(app, 3)
        XCTAssertTrue(Nav.element(app, "mg.detail.operating-picture").waitForExistence(timeout: 5))
        let globe = Nav.element(app, "mg.heimdall.3d.view")
        if Edition.isThreeD {
            Nav.openThreeD(app)
            // Another screen has no switch; back on the Operating Picture the view chosen
            // is still the 3D one, as the web keeps it.
            Nav.openScreen(app, "entities")
            XCTAssertFalse(Nav.element(app, "mg.heimdall.view.3d").exists, "the switch is on the Operating Picture only")
            Nav.openScreen(app, "operating-picture")
            XCTAssertTrue(globe.waitForExistence(timeout: 5))
            Nav.openView(app, "2d")
            XCTAssertFalse(globe.exists)
        } else {
            XCTAssertFalse(Nav.element(app, "mg.heimdall.view.3d").exists, "the release edition has no view switch")
            XCTAssertFalse(globe.exists)
        }
    }

    func testOnlyThe3DAppsHaveAbout() {
        Nav.signedIn(app)
        app.typeKey(",", modifierFlags: .command)
        XCTAssertTrue(app.toolbars.buttons["General"].firstMatch.waitForExistence(timeout: 10),
                      "Settings did not open:\n\(app.debugDescription)")
        let tab = app.toolbars.buttons["About"].firstMatch
        guard let aboutName = Edition.aboutName, let description = Edition.aboutRelease else {
            XCTAssertFalse(tab.exists, "the release app's Settings are unchanged: no About tab")
            XCTAssertFalse(Nav.element(app, "mg.about.name").exists)
            return
        }
        XCTAssertTrue(tab.exists, "no About tab in Settings:\n\(app.debugDescription)")
        tab.click()
        let name = Nav.element(app, "mg.about.name")
        XCTAssertTrue(name.waitForExistence(timeout: 5), app.debugDescription)
        XCTAssertEqual(Nav.text(name), aboutName)
        XCTAssertEqual(Nav.text(Nav.element(app, "mg.about.edition")), "Edition: \(aboutName)")
        let release = Nav.text(Nav.element(app, "mg.about.release"))
        XCTAssertTrue(release.hasPrefix(description), "About says: \(release)")
        XCTAssertTrue(Nav.text(Nav.element(app, "mg.about.version")).hasPrefix("Version 0.1.0 ("))
    }
}

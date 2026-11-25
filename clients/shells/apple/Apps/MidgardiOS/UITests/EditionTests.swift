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
        app.launchArguments = ["-MidgardUITesting"]
        app.launch()
    }

    override func tearDown() {
        app.terminate()
    }

    private func element(_ id: String) -> XCUIElement { app.descendants(matching: .any)[id] }

    func testTheSampleWorkspaceHasADataChipAndAnInvitationNoBanner() {
        AppNavigation.signIn(app)
        let chip = element("mg.chip.source")
        XCTAssertTrue(chip.waitForExistence(timeout: AppNavigation.timeout), app.debugDescription)
        XCTAssertEqual(chip.label, "Data: the sample workspace, built into the app")
        for id in ["mg.banner.demo", "mg.banner.edition3d", "mg.banner.editionmaps", "mg.banner.live", "mg.banner.nosource"] {
            XCTAssertFalse(element(id).exists, "\(id) is shown")
        }
        // The invitation: its words, and Connect your data opens Your Data (the sheet the
        // session menu opens).
        let text = element("mg.invite.text")
        XCTAssertTrue(text.waitForExistence(timeout: AppNavigation.timeout), app.debugDescription)
        XCTAssertEqual(text.label,
                       "Exploring the sample workspace. Connect your own data server or import JSON to see your data.")
        app.buttons["mg.invite.connect"].tap()
        let done = app.buttons["mg.yourdata.done"]
        XCTAssertTrue(done.waitForExistence(timeout: AppNavigation.timeout), "Your Data did not open")
        done.tap()
        // Dismissed, it goes; the chip stays.
        let dismiss = app.buttons["mg.invite.dismiss"]
        XCTAssertTrue(dismiss.waitForExistence(timeout: AppNavigation.timeout))
        dismiss.tap()
        XCTAssertFalse(element("mg.invite").waitForExistence(timeout: 2), "the invitation is still shown")
        XCTAssertTrue(chip.exists)
    }

    func testHeimdallOffersTheViewSwitchOnlyIn3D() {
        AppNavigation.signIn(app)
        AppNavigation.selectConsole(app, 3)
        if Edition.isThreeD {
            AppNavigation.openThreeD(app)
            // Back on Heimdall's other screens there is no switch.
            AppNavigation.open(app, console: 3, "entities")
            XCTAssertFalse(app.buttons["mg.heimdall.view.3d"].exists, "the switch is on the Operating Picture only")
        } else {
            app.buttons["mg.1.heimdall.operating-picture"].tap()
            XCTAssertTrue(element("mg.content.heimdall.operating-picture").waitForExistence(timeout: AppNavigation.timeout))
            XCTAssertFalse(app.buttons["mg.heimdall.view.3d"].exists, "the release edition has no view switch")
            XCTAssertFalse(element("mg.heimdall.3d.view").exists)
        }
    }

    func testOnlyThe3DAppsHaveAbout() {
        AppNavigation.signIn(app)
        guard let aboutName = Edition.aboutName, let description = Edition.aboutRelease else {
            app.buttons["mg.0.session-menu"].firstMatch.tap()
            XCTAssertTrue(app.buttons["mg.session.lock"].waitForExistence(timeout: AppNavigation.timeout))
            XCTAssertFalse(app.buttons["mg.session.about"].exists, "the release app's menu is unchanged: no About")
            return
        }
        AppNavigation.sessionMenu(app, "mg.session.about")
        let name = element("mg.about.name")
        XCTAssertTrue(name.waitForExistence(timeout: AppNavigation.timeout), app.debugDescription)
        XCTAssertEqual(name.label, aboutName)
        XCTAssertEqual(element("mg.about.edition").label, "Edition: \(aboutName)")
        let release = element("mg.about.release").label
        XCTAssertTrue(release.hasPrefix(description), "About says: \(release)")
        XCTAssertTrue(element("mg.about.version").label.hasPrefix("Version 0.1.0 ("))
        app.buttons["mg.about.done"].tap()
        XCTAssertTrue(AppNavigation.consoleMarker(app).waitForExistence(timeout: AppNavigation.timeout))
    }
}

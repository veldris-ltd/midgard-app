import XCTest

/// The edition the app under test says it is (Task B7), against `Edition`, which each
/// UI-test target defines for its app (UITests-Release, UITests-3D, UITests-Maps): the
/// edition banner, Heimdall's "3D view" entry, and About, all of which only the 3D and
/// maps apps have: the release app shows no visible change. [FR-C-1] [AC-C-13]
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

    func testTheBannersSayWhichEditionThisIs() {
        AppNavigation.signIn(app)
        XCTAssertTrue(element("mg.banner.demo").waitForExistence(timeout: AppNavigation.timeout))
        for id in ["mg.banner.edition3d", "mg.banner.editionmaps"] where id != Edition.banner?.id {
            XCTAssertFalse(element(id).exists, "\(id) is not this edition's banner")
        }
        if let expected = Edition.banner {
            let banner = element(expected.id)
            XCTAssertTrue(banner.waitForExistence(timeout: 5), app.debugDescription)
            XCTAssertEqual(banner.label, expected.text)
        }
    }

    func testHeimdallOffersThe3DViewOnlyIn3D() {
        AppNavigation.signIn(app)
        AppNavigation.selectConsole(app, 3)
        if Edition.isThreeD {
            AppNavigation.openThreeD(app)
            XCTAssertTrue(element("mg.heimdall.3d.view").label.contains("3D view"))
            // Back on Heimdall's other screens there is no entry.
            AppNavigation.selectConsole(app, 3)
            app.buttons["mg.2.heimdall.entities"].tap()
            XCTAssertTrue(element("mg.placeholder.heimdall.entities").waitForExistence(timeout: AppNavigation.timeout))
            XCTAssertFalse(app.buttons["mg.heimdall.3d"].exists, "the entry is on the Operating Picture only")
        } else {
            app.buttons["mg.1.heimdall.operating-picture"].tap()
            XCTAssertTrue(element("mg.content.heimdall.operating-picture").waitForExistence(timeout: AppNavigation.timeout))
            XCTAssertFalse(app.buttons["mg.heimdall.3d"].exists, "the release edition has no 3D entry")
            XCTAssertFalse(element("mg.heimdall.3d.view").exists)
        }
    }

    func testOnlyThe3DAppsHaveAbout() {
        AppNavigation.signIn(app)
        guard let aboutName = Edition.aboutName, let banner = Edition.banner else {
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
        XCTAssertTrue(release.hasPrefix(banner.text), "About says: \(release)")
        app.buttons["mg.about.done"].tap()
        XCTAssertTrue(AppNavigation.consoleMarker(app).waitForExistence(timeout: AppNavigation.timeout))
    }
}

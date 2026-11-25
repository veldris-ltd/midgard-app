import XCTest

/// App Store screenshots and the invitation's review shots, taken only when asked
/// (`MIDGARD_STORE_SHOTS=1` in the test runner's environment, so
/// `TEST_RUNNER_MIDGARD_STORE_SHOTS=1` on the xcodebuild command line): light mode, the
/// sample workspace, each a full-screen attachment kept in the result bundle. Skipped in
/// every ordinary run.
final class StoreScreenshotTests: XCTestCase {
    private var app: XCUIApplication!

    override func setUpWithError() throws {
        guard ProcessInfo.processInfo.environment["MIDGARD_STORE_SHOTS"] == "1" else {
            throw XCTSkip("store screenshots not requested")
        }
        continueAfterFailure = true
    }

    override func tearDown() {
        app?.terminate()
    }

    private func launch(inviteDismissed: Bool) {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", "Light",
                               "-MidgardSampleInviteDismissed", inviteDismissed ? "YES" : "NO"]
        app.launch()
        AppNavigation.signIn(app)
    }

    private func shot(_ name: String) {
        Thread.sleep(forTimeInterval: 2.5)
        let attachment = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    /// The invitation and the data chip, as a customer first sees them, then dismissed.
    func testInvitationShots() {
        launch(inviteDismissed: false)
        AppNavigation.selectConsole(app, 1)
        XCTAssertTrue(app.descendants(matching: .any)["mg.invite"].waitForExistence(timeout: AppNavigation.timeout))
        shot("after-invite-iphone")
        app.buttons["mg.invite.dismiss"].tap()
        shot("after-dismissed-iphone")
    }

    func testStoreShots() {
        launch(inviteDismissed: true)
        AppNavigation.openMap(app)
        shot("01-heimdall-map")
        AppNavigation.openThreeD(app)
        shot("02-heimdall-3d")
        AppNavigation.open(app, console: 1, "object-fabric")
        shot("03-midgard-object-fabric")
        AppNavigation.open(app, console: 2, "overview")
        shot("04-admin-overview")
        AppNavigation.open(app, console: 4, "pipeline-builder")
        shot("05-forge-pipeline-builder")
        AppNavigation.open(app, console: 5, "agent-studio")
        shot("06-mimir-agent-studio")
        AppNavigation.open(app, console: 6, "release-console")
        shot("07-bifrost-release-console")
        AppNavigation.sessionMenu(app, "mg.session.yourdata")
        XCTAssertTrue(app.buttons["mg.yourdata.done"].waitForExistence(timeout: AppNavigation.timeout))
        shot("08-your-data")
    }
}

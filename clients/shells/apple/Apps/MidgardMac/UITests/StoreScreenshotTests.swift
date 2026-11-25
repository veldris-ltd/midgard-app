import XCTest

/// Mac App Store screenshots and the invitation's review shots, taken only when asked
/// (`MIDGARD_STORE_SHOTS=1` in the test runner's environment, so
/// `TEST_RUNNER_MIDGARD_STORE_SHOTS=1` on the xcodebuild command line): light mode, the
/// sample workspace, a 1440 × 900 window (16:10), each window kept as an attachment in
/// the result bundle. Skipped in every ordinary run.
final class StoreScreenshotTests: XCTestCase {
    private var app: XCUIApplication!

    /// The window at 1440 × 900 points: one of the Mac App Store's 16:10 sizes.
    static let storeWindow = ["-ApplePersistenceIgnoreState", "YES",
        "-NSWindow Frame SwiftUI.PresentedWindowContent<MidgardKit.ConsoleRoute, "
            + "SwiftUI.ModifiedContent<Midgard.RootView, SwiftUI._FlexFrameLayout>>-1-AppWindow-1",
        "100 100 1440 900 0 0 1920 1170"]

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
                               "-MidgardSampleInviteDismissed", inviteDismissed ? "YES" : "NO"] + Self.storeWindow
        app.launch()
        Nav.signedIn(app)
    }

    private func shot(_ name: String, window: XCUIElement? = nil) {
        Thread.sleep(forTimeInterval: 2.5)
        let target = window ?? app.windows.element(boundBy: 0)
        let attachment = XCTAttachment(screenshot: target.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }

    func testInvitationShots() {
        launch(inviteDismissed: false)
        Nav.openConsole(app, 1)
        XCTAssertTrue(Nav.element(app, "mg.invite").waitForExistence(timeout: 10))
        shot("after-invite-mac")
        Nav.element(app, "mg.invite.dismiss").click()
        shot("after-dismissed-mac")
    }

    func testStoreShots() {
        launch(inviteDismissed: true)
        Nav.openMap(app)
        shot("01-heimdall-map")
        Nav.openThreeD(app)
        shot("02-heimdall-3d")
        Nav.openConsole(app, 1)
        shot("03-midgard-object-fabric")
        Nav.openConsole(app, 2)
        shot("04-admin-overview")
        Nav.openConsole(app, 4)
        Nav.openScreen(app, "pipeline-builder")
        shot("05-forge-pipeline-builder")
        Nav.openConsole(app, 5)
        Nav.openScreen(app, "agent-studio")
        shot("06-mimir-agent-studio")
        Nav.openConsole(app, 6)
        shot("07-bifrost-release-console")
        app.typeKey("d", modifierFlags: [.command, .shift])
        let data = app.windows["Your Data"].firstMatch
        XCTAssertTrue(data.waitForExistence(timeout: 10))
        shot("08-your-data", window: data)
    }
}

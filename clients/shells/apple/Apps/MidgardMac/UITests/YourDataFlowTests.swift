import AppKit
import XCTest

/// Your Data on the Mac (Phase J), each step kept as a window screenshot: files imported
/// over the sample data (the picker is the system's and is not driven: the files come
/// from `MIDGARD_UITEST_IMPORT`, as the picker would give them) and their layer on
/// Heimdall's picture in 2D, MAP and 3D; a refused file named with its line; and, when
/// `MIDGARD_GW_ADDRESS`, `MIDGARD_GW_USER` and `MIDGARD_GW_PASSWORD` name a running
/// gateway (as `TEST_RUNNER_…`), a sign-in to it typed into Your Data and Midgard's
/// Object Fabric read live. Each test is skipped when its inputs are not given.
/// [DD-1] [RC-10] [RC-24] [FR-H-1]
final class YourDataFlowTests: XCTestCase {
    private var app: XCUIApplication!
    private let env = ProcessInfo.processInfo.environment

    override func setUp() {
        continueAfterFailure = false
    }

    override func tearDown() {
        app?.terminate()
    }

    private func keep(_ name: String, _ window: XCUIElement? = nil) {
        let shot = XCTAttachment(screenshot: (window ?? app.windows.firstMatch).screenshot())
        shot.name = "yourdata-mac-\(name)"
        shot.lifetime = .keepAlways
        add(shot)
    }

    private func launch(_ extra: [String: String]) {
        app = XCUIApplication()
        app.launchArguments = ["-MidgardUITesting", "-AppleInterfaceStyle", "Dark"] + Nav.freshLaunch
        for (k, v) in extra { app.launchEnvironment[k] = v }
        app.launch()
        Nav.signedIn(app)
    }

    /// Session ▸ Your Data… (⇧⌘D): its window.
    private func openYourData() -> XCUIElement {
        app.typeKey("d", modifierFlags: [.command, .shift])
        let window = app.windows.containing(.any, identifier: "mg.yourdata").firstMatch
        XCTAssertTrue(window.waitForExistence(timeout: 10), "Your Data did not open")
        return window
    }

    private func settle(_ seconds: TimeInterval) {
        _ = XCTWaiter.wait(for: [XCTestExpectation(description: "settle")], timeout: seconds)
    }

    func testImportedFilesAndTheirLayerOnThePicture() throws {
        guard let files = env["MIDGARD_UITEST_IMPORT_OK"] else { throw XCTSkip("MIDGARD_UITEST_IMPORT_OK not set") }
        var extra = ["MIDGARD_UITEST_IMPORT": files]
        if let feed = env["MIDGARD_UITEST_FEED"] { extra["MIDGARD_UITEST_FEED"] = feed }
        launch(extra)
        let window = openYourData()
        let notice = Nav.element(app, "mg.yourdata.notice")
        XCTAssertTrue(notice.waitForExistence(timeout: 20), "no import result")
        XCTAssertTrue(Nav.text(notice).hasPrefix("Imported"), Nav.text(notice))
        keep("import", window)
        window.typeKey("w", modifierFlags: .command)

        Nav.openView(app, "2d")
        settle(4)
        keep("picture-2d")
        Nav.openView(app, "map")
        settle(6)
        keep("picture-map")
        Nav.openView(app, "3d")
        settle(6)
        keep("picture-3d")
    }

    func testARefusedFileIsNamedWithItsLine() throws {
        guard let ok = env["MIDGARD_UITEST_IMPORT_OK"], let bad = env["MIDGARD_UITEST_IMPORT_BAD"] else {
            throw XCTSkip("MIDGARD_UITEST_IMPORT_OK and _BAD not set")
        }
        launch(["MIDGARD_UITEST_IMPORT": ok + "||" + bad])
        let window = openYourData()
        let notice = Nav.element(app, "mg.yourdata.notice")
        let refused = NSPredicate { _, _ in Nav.text(notice).hasPrefix("Import refused") }
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: refused, object: nil)], timeout: 20), .completed,
                       Nav.text(notice))
        XCTAssertTrue(Nav.text(notice).contains("line 1"), Nav.text(notice))
        keep("refusal", window)
    }

    func testAGatewaySignInReadsMidgardLive() throws {
        guard let address = env["MIDGARD_GW_ADDRESS"], let user = env["MIDGARD_GW_USER"],
              let password = env["MIDGARD_GW_PASSWORD"] else { throw XCTSkip("no gateway given") }
        launch([:])
        let window = openYourData()
        // Pasted: typing drops the shifted ":" on some keyboard layouts.
        for (id, text) in [("mg.yourdata.address", address), ("mg.yourdata.username", user)] {
            let field = Nav.element(app, id)
            XCTAssertTrue(field.waitForExistence(timeout: 5))
            field.click()
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(text, forType: .string)
            field.typeKey("v", modifierFlags: .command)
        }
        NSPasteboard.general.clearContents()
        let secret = Nav.element(app, "mg.yourdata.password")
        secret.click()
        secret.typeText(password)
        Nav.element(app, "mg.yourdata.connect").click()
        let state = Nav.element(app, "mg.yourdata.gateway.state")
        let live = NSPredicate { _, _ in Nav.text(state).hasPrefix("LIVE") }
        XCTAssertEqual(XCTWaiter.wait(for: [XCTNSPredicateExpectation(predicate: live, object: nil)], timeout: 20), .completed,
                       Nav.text(Nav.element(app, "mg.yourdata.gateway.error")))
        keep("gateway", window)
        window.typeKey("w", modifierFlags: .command)

        Nav.openConsole(app, 1)
        Nav.openScreen(app, "object-fabric")
        XCTAssertTrue(Nav.element(app, "mg.banner.live").waitForExistence(timeout: 10), "no LIVE banner")
        settle(3)
        keep("live-object-fabric")
        Nav.openScreen(app, "audit")
        settle(2)
        keep("live-audit")
        Nav.openConsole(app, 2)
        XCTAssertTrue(Nav.element(app, "mg.admin.notserved").waitForExistence(timeout: 10), "Admin does not say it is not served")
        keep("live-admin-not-served")
    }
}

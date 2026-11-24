import MidgardAuditKit
import XCTest

/// The seven affordances (clients/shells/apple/accessibility.toml), measured on the
/// iPhone apps, both editions, by clients/shells/apple/audit.sh: the sign-in screen, each
/// console's list of screens, two of Midgard's screens (Object Fabric, Policies), Admin's
/// Overview, Heimdall's Operating Picture, Feeds and Workflows, Forge's Pipelines and
/// Transform IDE, the Your Data sheet, and
/// the screens only this edition has (`Edition.auditedScreens`: Heimdall's MAP and 3D
/// views). [RC-49] [AC-C-11]
final class AccessibilityAudit: AccessibilityAuditBase {
    override var screens: [(XCUIApplication) -> Void] {
        var list: [(XCUIApplication) -> Void] = [AppNavigation.toSignIn]
        for n in 1...AppNavigation.consoles.count {
            list.append { app in
                AppNavigation.signIn(app)
                AppNavigation.selectConsole(app, n)
            }
        }
        list.append(AppNavigation.openDetail)
        list.append { AppNavigation.openMidgard($0, "policies") }
        list.append { AppNavigation.openAdmin($0, "overview") }
        list.append(AppNavigation.openPicture)
        list.append { AppNavigation.open($0, console: 3, "feeds") }
        list.append { AppNavigation.open($0, console: 3, "workflows") }
        list.append { AppNavigation.open($0, console: 4, "pipeline-builder") }
        list.append { AppNavigation.open($0, console: 4, "transform-ide") }
        // Last: on the iPhone it is a sheet over the console.
        let yourData: (XCUIApplication) -> Void = { app in
            AppNavigation.signIn(app)
            if !app.descendants(matching: .any)["mg.yourdata"].exists {
                // At accessibility text sizes the menu scrolls: Your data is further down.
                app.buttons["mg.0.session-menu"].firstMatch.tap()
                let item = app.buttons["mg.session.yourdata"]
                for _ in 0..<4 where !item.waitForExistence(timeout: 2) {
                    app.collectionViews.firstMatch.swipeUp()
                }
                item.tap()
            }
            XCTAssertTrue(app.descendants(matching: .any)["mg.yourdata"].waitForExistence(timeout: AppNavigation.timeout),
                          "Your Data did not open")
        }
        return list + Edition.auditedScreens + [yourData]
    }
}

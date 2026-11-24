import MidgardAuditKit
import XCTest

/// The accessibility audit of the macOS apps (clients/shells/apple/audit.sh), both
/// editions: the sign-in screen, each console's first screen (Admin's Overview among
/// them), two more of Midgard's (Data Search, Policies), Admin's Approvals, Heimdall's
/// Workflows and Decision Lineage, Forge's Pipelines and Transform IDE (and its
/// Connectors, the console's first), Mimir's Agent Studio and Guardrails (and its Model
/// Plane, the console's first), Bifrost's Fleet & drift and Rollback centre (and its
/// Release Console, the console's first), the Your Data window, and the screens only this
/// edition has (`Edition.auditedScreens`: Heimdall's MAP and 3D
/// views). Dynamic Type is measured on iOS only; its macOS test fails by design and the
/// scheme skips it, so `xcodebuild test` stays green while audit.sh still reads it as
/// not measured here. [RC-49] [AC-C-11]
final class AccessibilityAudit: AccessibilityAuditBase {
    override var launchArguments: [String] { super.launchArguments + Nav.freshLaunch }

    override var screens: [(XCUIApplication) -> Void] {
        var list: [(XCUIApplication) -> Void] = [{ Nav.toSignIn($0) }]
        for n in 1...6 { list.append { Nav.openConsole($0, n) } }
        for slug in ["data-search", "policies"] {
            list.append { app in
                Nav.openConsole(app, 1)
                Nav.openScreen(app, slug)
            }
        }
        list.append { app in
            Nav.openConsole(app, 2)
            Nav.openScreen(app, "approvals")
        }
        for slug in ["workflows", "decision-lineage"] {
            list.append { app in
                Nav.openConsole(app, 3)
                Nav.openScreen(app, slug)
            }
        }
        for slug in ["pipeline-builder", "transform-ide"] {
            list.append { app in
                Nav.openConsole(app, 4)
                Nav.openScreen(app, slug)
            }
        }
        for slug in ["agent-studio", "guardrails"] {
            list.append { app in
                Nav.openConsole(app, 5)
                Nav.openScreen(app, slug)
            }
        }
        for slug in ["fleet", "rollback"] {
            list.append { app in
                Nav.openConsole(app, 6)
                Nav.openScreen(app, slug)
            }
        }
        // Last, as on the iPhone (here it is a window of its own).
        let yourData: (XCUIApplication) -> Void = { app in
            Nav.signedIn(app)
            if !app.windows.containing(.any, identifier: "mg.yourdata").firstMatch.exists {
                app.typeKey("d", modifierFlags: [.command, .shift])
            }
            XCTAssertTrue(app.windows.containing(.any, identifier: "mg.yourdata").firstMatch.waitForExistence(timeout: 10),
                          "Your Data did not open")
            // The console window under it is closed: its screens were audited on their
            // own, and the audit measured every element the Your Data window covered
            // (eleven contrast findings were the console's covered texts).
            let console = app.windows.containing(.any, identifier: "mg.console.selected").firstMatch
            if console.exists {
                console.buttons[XCUIIdentifierCloseWindow].click()
                XCTAssertTrue(console.waitForNonExistence(timeout: 5), "the console window did not close")
            }
        }
        return list + Edition.auditedScreens + [yourData]
    }
}

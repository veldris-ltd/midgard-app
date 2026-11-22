import MidgardAuditKit
import XCTest

/// The accessibility audit of the macOS apps (clients/shells/apple/audit.sh), both
/// editions: the sign-in screen, each console's first screen, one screen's detail, and
/// the screens only this edition has (`Edition.auditedScreens`: the 3D view in Midgard 3D). Dynamic Type
/// is measured on iOS only; its macOS test fails by design and the scheme skips it, so
/// `xcodebuild test` stays green while audit.sh still reads it as not measured here.
/// [RC-49] [AC-C-11]
final class AccessibilityAudit: AccessibilityAuditBase {
    override var launchArguments: [String] { super.launchArguments + Nav.freshLaunch }

    override var screens: [(XCUIApplication) -> Void] {
        var list: [(XCUIApplication) -> Void] = [{ Nav.toSignIn($0) }]
        for n in 1...6 { list.append { Nav.openConsole($0, n) } }
        list.append { app in
            Nav.openConsole(app, 1)
            Nav.openScreen(app, "data-search")
        }
        return list + Edition.auditedScreens
    }
}

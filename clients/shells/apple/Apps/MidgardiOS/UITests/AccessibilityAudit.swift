import MidgardAuditKit
import XCTest

/// The seven affordances (clients/shells/apple/accessibility.toml), measured on the
/// iPhone app by clients/shells/apple/audit.sh: the sign-in screen, each console's
/// list of screens, and one screen. [RC-49] [AC-C-11]
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
        return list
    }
}

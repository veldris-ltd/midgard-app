import XCTest

/// Steps the UI tests share. Each one starts from wherever the app is (a fresh store
/// at launch: enrolment first), so the audit's navigations can run in any order.
enum Nav {
    /// Every test launch starts with no restored windows: a run that terminated the app
    /// can leave saved state with none, and the app would then open no window.
    static let freshLaunch = ["-ApplePersistenceIgnoreState", "YES"]

    static let consoleIds = ["midgard", "admin", "heimdall", "forge", "mimir", "bifrost"]

    static func element(_ app: XCUIApplication, _ id: String) -> XCUIElement {
        app.descendants(matching: .any)[id]
    }

    /// The first element whose identifier matches `pattern` (an NSPredicate MATCHES).
    static func element(_ app: XCUIApplication, matching pattern: String) -> XCUIElement {
        app.descendants(matching: .any).matching(NSPredicate(format: "identifier MATCHES %@", pattern)).firstMatch
    }

    /// Enrol if the app asks to, ending on the sign-in screen (or past it).
    static func toSignIn(_ app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) {
        let enrol = element(app, "mg.0.enrol")
        let persona = element(app, "mg.0.persona.local")
        let console = element(app, "mg.console.selected")
        let deadline = Date().addingTimeInterval(45)
        while Date() < deadline {
            if persona.exists || console.exists { return }
            if enrol.exists && enrol.isHittable { enrol.click() }
            _ = persona.waitForExistence(timeout: 0.5)
        }
        XCTFail("the sign-in screen did not appear:\n\(app.debugDescription)", file: file, line: line)
    }

    /// Signed in as the local user, inside the console shell.
    static func signedIn(_ app: XCUIApplication, file: StaticString = #filePath, line: UInt = #line) {
        toSignIn(app, file: file, line: line)
        let console = element(app, "mg.console.selected")
        if console.exists { return }
        let unlock = element(app, "mg.0.unlock")
        if unlock.exists { unlock.click() } else { element(app, "mg.0.persona.local").click() }
        XCTAssertTrue(console.waitForExistence(timeout: 20), "the console shell did not appear", file: file, line: line)
    }

    /// Select console `n` (1…6) from the sidebar.
    static func openConsole(_ app: XCUIApplication, _ n: Int, file: StaticString = #filePath, line: UInt = #line) {
        signedIn(app, file: file, line: line)
        let row = element(app, matching: "mg\\.[0-9]+\\.console\\.\(consoleIds[n - 1])")
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no sidebar entry for console \(n)", file: file, line: line)
        row.click()
        let selected = element(app, "mg.console.selected")
        XCTAssertTrue(wait(selected, value: "\(n)"), "console \(n) is not selected", file: file, line: line)
    }

    /// Select screen `slug` of the selected console from the sidebar.
    static func openScreen(_ app: XCUIApplication, _ slug: String, file: StaticString = #filePath, line: UInt = #line) {
        let row = element(app, matching: "mg\\.[0-9]+\\.screen\\.\(slug)")
        XCTAssertTrue(row.waitForExistence(timeout: 5), "no sidebar entry for screen \(slug)", file: file, line: line)
        row.click()
        XCTAssertTrue(element(app, "mg.detail.\(slug)").waitForExistence(timeout: 5),
                      "the \(slug) detail did not appear", file: file, line: line)
    }

    /// What a text element says: on macOS static text carries it as its value, and an
    /// element given an accessibility label carries it as the label.
    static func text(_ element: XCUIElement) -> String {
        if let value = element.value as? String, !value.isEmpty { return value }
        return element.label
    }

    /// Wait until `element`'s value is `value`.
    static func wait(_ element: XCUIElement, value: String, timeout: TimeInterval = 5) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "value == %@", value), object: element)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    /// Wait until `element`'s label starts with `prefix`.
    static func wait(_ element: XCUIElement, labelPrefix prefix: String, timeout: TimeInterval = 5) -> Bool {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate(format: "label BEGINSWITH %@", prefix),
                                                    object: element)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }
}

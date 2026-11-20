import XCTest

/// Getting around the iPhone app in UI tests. Every step starts from wherever the app
/// is, so the audit's screens can run in any order (its keyboard test jumps from the
/// sign-in screen straight to the last one).
enum AppNavigation {
    static let timeout: TimeInterval = 15
    /// The consoles in tab order (⌘1…⌘6).
    static let consoles = ["Midgard", "Admin", "Heimdall", "Forge", "Mimir", "Bifrost"]

    static func enrolButton(_ app: XCUIApplication) -> XCUIElement { app.buttons["mg.0.enrol"] }
    static func localPersona(_ app: XCUIApplication) -> XCUIElement { app.buttons["mg.0.persona.local"] }
    static func unlockButton(_ app: XCUIApplication) -> XCUIElement { app.buttons["mg.0.unlock"] }
    static func consoleMarker(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["mg.console.selected"]
    }

    /// Waits until one of `elements` exists; returns it.
    @discardableResult
    static func first(of elements: [XCUIElement], timeout: TimeInterval = timeout,
                      file: StaticString = #filePath, line: UInt = #line) -> XCUIElement? {
        let deadline = Date().addingTimeInterval(timeout)
        repeat {
            if let found = elements.first(where: { $0.exists }) { return found }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        } while Date() < deadline
        XCTFail("none of \(elements.map(\.identifier)) appeared", file: file, line: line)
        return nil
    }

    /// The sign-in screen: enrols this (fresh) device first if it asks.
    static func toSignIn(_ app: XCUIApplication) {
        if first(of: [enrolButton(app), localPersona(app)])?.identifier == "mg.0.enrol" {
            enrolButton(app).tap()
        }
        XCTAssertTrue(localPersona(app).waitForExistence(timeout: timeout), "the sign-in screen did not appear")
    }

    /// Signed in as `local` (enrolling or unlocking as needed), inside a console.
    static func signIn(_ app: XCUIApplication) {
        for _ in 0..<3 {
            guard let found = first(of: [consoleMarker(app), enrolButton(app), localPersona(app), unlockButton(app)])
            else { return }
            if found.identifier == "mg.console.selected" { return }
            found.tap()
            if consoleMarker(app).waitForExistence(timeout: 2) { return }
        }
        XCTAssertTrue(consoleMarker(app).waitForExistence(timeout: timeout), "did not reach the consoles")
    }

    /// Console `n` (1…6) selected from the console bar, at its list of screens.
    static func selectConsole(_ app: XCUIApplication, _ n: Int, file: StaticString = #filePath, line: UInt = #line) {
        let name = consoles[n - 1]
        app.buttons["mg.console.tab.\(name.lowercased())"].tap()
        // Back to the console's list if a screen was open.
        let list = app.navigationBars[name]
        if !list.waitForExistence(timeout: 3) {
            let back = app.navigationBars.buttons[name]
            if back.exists { back.tap() }
        }
        XCTAssertTrue(list.waitForExistence(timeout: timeout), "\(name)'s screen list did not appear",
                      file: file, line: line)
        XCTAssertEqual(consoleMarker(app).value as? String, "\(n)", "console \(n) is not selected",
                       file: file, line: line)
    }

    /// Midgard's first screen, opened.
    static func openDetail(_ app: XCUIApplication) {
        signIn(app)
        selectConsole(app, 1)
        app.buttons["mg.1.midgard.object-fabric"].tap()
        XCTAssertTrue(app.descendants(matching: .any)["mg.placeholder.midgard.object-fabric"]
            .waitForExistence(timeout: timeout), "the Object Fabric screen did not open")
    }

    /// Runs one of the session menu's commands.
    static func sessionMenu(_ app: XCUIApplication, _ item: String) {
        app.buttons["mg.0.session-menu"].firstMatch.tap()
        let button = app.buttons[item]
        XCTAssertTrue(button.waitForExistence(timeout: timeout), "no \(item) in the session menu")
        button.tap()
    }
}

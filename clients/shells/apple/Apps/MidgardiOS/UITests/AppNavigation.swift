import XCTest

/// Getting around the iPhone app in UI tests. Every step starts from wherever the app
/// is, so the audit's screens can run in any order (its keyboard test jumps from the
/// sign-in screen straight to the last one).
enum AppNavigation {
    static let timeout: TimeInterval = 15
    /// The consoles in tab order (⌘1…⌘6).
    static let consoles = ["Midgard", "Admin", "Heimdall", "Forge", "Mimir", "Bifrost"]

    static func enrolButton(_ app: XCUIApplication) -> XCUIElement { app.buttons["mg.0.enrol"] }
    /// The persona the tests sign in as: K. Osei, the web Heimdall console's own subject
    /// (confidential, OPS-N), who sees the whole sample picture.
    static let persona = "k.osei"
    static func personaButton(_ app: XCUIApplication, _ id: String = persona) -> XCUIElement {
        app.buttons.matching(NSPredicate(format: "identifier MATCHES %@",
                                         "mg\\.[0-9]+\\.persona\\.\(id.replacingOccurrences(of: ".", with: "\\."))")).firstMatch
    }
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
        if first(of: [enrolButton(app), personaButton(app)])?.identifier == "mg.0.enrol" {
            enrolButton(app).tap()
        }
        XCTAssertTrue(personaButton(app).waitForExistence(timeout: timeout), "the sign-in screen did not appear")
    }

    /// Signed in as `persona` (enrolling or unlocking as needed), inside a console.
    static func signIn(_ app: XCUIApplication) {
        for _ in 0..<3 {
            guard let found = first(of: [consoleMarker(app), enrolButton(app), personaButton(app), unlockButton(app)])
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
        for _ in 0..<3 where !list.waitForExistence(timeout: 3) {
            let back = app.navigationBars.buttons["BackButton"]
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
        tapClearOfConsoleBar(app, app.buttons["mg.1.midgard.object-fabric"])
        XCTAssertTrue(app.descendants(matching: .any)["mg.content.midgard.object-fabric"]
            .waitForExistence(timeout: timeout), "the Object Fabric screen did not open")
    }

    /// One of Midgard's screens (`slug`, e.g. `policies`), opened from its list.
    static func openMidgard(_ app: XCUIApplication, _ slug: String) {
        signIn(app)
        selectConsole(app, 1)
        let row = app.buttons.matching(NSPredicate(format: "identifier MATCHES %@", "mg\\.[0-9]+\\.midgard\\.\(slug)")).firstMatch
        // At accessibility text sizes the row can be screens down the list: the list's
        // search brings it to the top.
        if !row.exists {
            let field = app.descendants(matching: .any)["mg.search.midgard"]
            field.tap()
            field.typeText(String(slug.prefix(5)))
        }
        tapClearOfConsoleBar(app, row)
        XCTAssertTrue(app.descendants(matching: .any)["mg.content.midgard.\(slug)"]
            .waitForExistence(timeout: timeout), "Midgard's \(slug) screen did not open")
    }

    /// One of Admin's screens (`slug`, e.g. `overview`), opened from its list.
    static func openAdmin(_ app: XCUIApplication, _ slug: String) {
        signIn(app)
        selectConsole(app, 2)
        let row = app.buttons.matching(NSPredicate(format: "identifier MATCHES %@", "mg\\.[0-9]+\\.admin\\.\(slug)")).firstMatch
        tapClearOfConsoleBar(app, row)
        XCTAssertTrue(app.descendants(matching: .any)["mg.content.admin.\(slug)"]
            .waitForExistence(timeout: timeout), "Admin's \(slug) screen did not open")
    }

    /// Heimdall's Operating Picture with view `id` chosen in the navigation bar's view
    /// switch (2D, MAP or 3D; MAP and 3D in the Maps edition only).
    static func openView(_ app: XCUIApplication, _ id: String) {
        let button = app.buttons["mg.heimdall.view.\(id)"]
        if !button.exists {
            signIn(app)
            selectConsole(app, 3)
            tapClearOfConsoleBar(app, app.buttons["mg.1.heimdall.operating-picture"])
            XCTAssertTrue(app.descendants(matching: .any)["mg.content.heimdall.operating-picture"]
                .waitForExistence(timeout: timeout), "the Operating Picture did not open")
        }
        XCTAssertTrue(button.waitForExistence(timeout: timeout), "no \(id) in Heimdall's view switch")
        button.tap()
        let shown = id == "2d" ? app.descendants(matching: .any)["mg.heimdall.track.E-0871"]
                               : app.descendants(matching: .any)["mg.heimdall.\(id).view"]
        XCTAssertTrue(shown.waitForExistence(timeout: timeout), "the \(id) view did not open")
    }

    /// Heimdall's Operating Picture with its 3D view open (the Maps edition).
    static func openThreeD(_ app: XCUIApplication) { openView(app, "3d") }

    /// Heimdall's Operating Picture with its MAP view open (the Maps edition).
    static func openMap(_ app: XCUIApplication) { openView(app, "map") }

    /// Taps a screen row where it shows. At accessibility text sizes, and more so under
    /// an edition banner, only a strip of the list shows between the navigation bar and
    /// the console bar: the list is swiped up until the row exists (at most four times),
    /// and the tap goes to the middle of the row's visible part, because a tap on its
    /// centre can land on the console bar although XCUITest calls the row hittable.
    static func tapClearOfConsoleBar(_ app: XCUIApplication, _ row: XCUIElement) {
        for _ in 0..<4 where !row.waitForExistence(timeout: 2) {
            app.scrollViews.firstMatch.swipeUp()
        }
        let bar = app.buttons["mg.console.tab.midgard"]
        let nav = app.navigationBars.firstMatch
        guard row.exists, bar.exists else { return row.tap() }
        let top = max(row.frame.minY, nav.exists ? nav.frame.maxY : 0)
        let bottom = min(row.frame.maxY, bar.frame.minY)
        guard bottom - top >= 8 else { return row.tap() }
        app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: row.frame.midX, dy: (top + bottom) / 2)).tap()
    }

    /// Heimdall's Operating Picture, opened.
    static func openPicture(_ app: XCUIApplication) {
        signIn(app)
        selectConsole(app, 3)
        tapClearOfConsoleBar(app, app.buttons["mg.1.heimdall.operating-picture"])
        // The reference layout, should the scene have kept another view chosen.
        let track = app.descendants(matching: .any)["mg.heimdall.track.E-0871"]
        let reference = app.buttons["mg.heimdall.view.2d"]
        if !track.waitForExistence(timeout: 5), reference.exists { reference.tap() }
        XCTAssertTrue(track.waitForExistence(timeout: timeout), "the Operating Picture did not open")
    }

    /// Runs one of the session menu's commands.
    static func sessionMenu(_ app: XCUIApplication, _ item: String) {
        app.buttons["mg.0.session-menu"].firstMatch.tap()
        let button = app.buttons[item]
        XCTAssertTrue(button.waitForExistence(timeout: timeout), "no \(item) in the session menu")
        button.tap()
    }
}

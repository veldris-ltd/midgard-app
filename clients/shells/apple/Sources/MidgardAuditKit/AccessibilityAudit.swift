import XCTest

/// The seven affordances of clients/shells/apple/accessibility.toml, measured on the
/// running app. Each app's UI-test target subclasses this as `AccessibilityAudit` and
/// supplies `screens`: closures that navigate to each screen to audit. One test per
/// affordance, named `test_affordance_<name>`: clients/shells/apple/tools/audit-failures.js
/// reads the result bundle by those names, and a test that fails, is skipped or does not
/// run is recorded as failed. A test that finds nothing to measure fails. [RC-49] [AC-C-11]
open class AccessibilityAuditBase: XCTestCase {
    /// The base class is abstract: run its tests only through a subclass.
    override open class var defaultTestSuite: XCTestSuite {
        ObjectIdentifier(self) == ObjectIdentifier(AccessibilityAuditBase.self)
            ? XCTestSuite(name: "AccessibilityAuditBase (abstract)") : super.defaultTestSuite
    }

    /// Navigations, each ending on a screen to audit. The first is the sign-in screen.
    open var screens: [(XCUIApplication) -> Void] { [] }
    /// `-MidgardUITesting` (Debug builds only) selects a non-prompting key root and a
    /// fresh store (Phase B root views).
    open var launchArguments: [String] { ["-MidgardUITesting"] }

    public func launch(_ extra: [String] = [], environment: [String: String] = [:]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = launchArguments + extra
        app.launchEnvironment = environment
        app.launch()
        return app
    }

    /// The screens to audit; fails the test when a subclass supplies none.
    func auditedScreens(file: StaticString = #filePath, line: UInt = #line) -> [(XCUIApplication) -> Void] {
        let list = screens
        if list.isEmpty { XCTFail("no screens to audit: nothing was measured", file: file, line: line) }
        return list
    }

    func audit(_ types: XCUIAccessibilityAuditType, extra: [String] = []) throws {
        let list = auditedScreens()
        guard !list.isEmpty else { return }
        let app = launch(extra)
        for go in list {
            go(app)
            try app.performAccessibilityAudit(for: types)
        }
        app.terminate()
    }

    /// The elements whose identifier is `mg.<n>.…`, in accessibility order.
    func numbered(_ app: XCUIApplication) -> [XCUIElement] {
        app.descendants(matching: .any)
            .matching(NSPredicate(format: "identifier MATCHES 'mg\\\\.[0-9]+\\\\..*'"))
            .allElementsBoundByAccessibilityElement
    }

    public func test_affordance_screen_reader_labels() throws {
        try audit([.sufficientElementDescription, .elementDetection])
    }

    public func test_affordance_contrast_aa() throws {
        #if os(iOS)
        let before = XCUIDevice.shared.appearance
        defer { XCUIDevice.shared.appearance = before }
        for style in [XCUIDevice.Appearance.light, .dark] {
            XCUIDevice.shared.appearance = style
            try audit(.contrast)
        }
        #else
        try audit(.contrast, extra: ["-AppleInterfaceStyle", "Light"])
        try audit(.contrast, extra: ["-AppleInterfaceStyle", "Dark"])
        #endif
    }

    public func test_affordance_hit_target_size() throws { try audit(.hitRegion) }

    public func test_affordance_dynamic_type() throws {
        #if os(iOS)
        try audit([.dynamicType, .textClipped],
                  extra: ["-UIPreferredContentSizeCategoryName", "UICTContentSizeCategoryAccessibilityXXXL"])
        #else
        XCTFail("Dynamic Type is measured on iOS; the macOS run does not claim it")
        #endif
    }

    public func test_affordance_focus_order() throws {
        let list = auditedScreens()
        guard !list.isEmpty else { return }
        let app = launch()
        var measured = 0
        for go in list {
            go(app)
            let ordered = numbered(app)
            let indices = ordered.compactMap { Int($0.identifier.split(separator: ".")[1]) }
            XCTAssertEqual(indices, indices.sorted(), "accessibility order must follow mg.<n>. order")
            let frames = ordered.map(\.frame)
            for (a, b) in zip(frames, frames.dropFirst()) {
                // Reading order: a clearly higher row first, or the same row left to right.
                XCTAssertTrue(a.minY < b.minY - 1 || (abs(a.minY - b.minY) < 1 && a.minX <= b.minX),
                              "elements must be in reading order: \(a) before \(b)")
            }
            if ordered.count > 1 { measured += 1 }
        }
        XCTAssertGreaterThan(measured, 0, "no audited screen has two mg.<n>. elements: focus order was not measured")
    }

    public func test_affordance_reduced_motion() throws {
        let list = auditedScreens()
        guard !list.isEmpty else { return }
        let app = launch(environment: ["MIDGARD_REDUCE_MOTION": "1"])
        let motion = app.descendants(matching: .any)["mg.motion"]
        XCTAssertTrue(motion.waitForExistence(timeout: 5), "no mg.motion element")
        XCTAssertEqual(motion.value as? String, "reduced")
        for go in list {
            go(app)
            let animated = app.descendants(matching: .any).matching(NSPredicate(format: "identifier BEGINSWITH 'mg.anim.'"))
            XCTAssertEqual(animated.count, 0, "an animation runs with Reduce Motion on")
            #if os(iOS)
            XCTAssertEqual(app.activityIndicators.count, 0, "a spinner runs with Reduce Motion on")
            #else
            XCTAssertEqual(app.progressIndicators.count, 0, "a spinner runs with Reduce Motion on")
            #endif
        }
    }

    public func test_affordance_keyboard_navigation() throws {
        let list = auditedScreens()
        guard let signIn = list.first else { return }
        #if os(iOS)
        // Needs a hardware keyboard and Full Keyboard Access on the simulator (Phase B).
        let app = launch()
        #else
        // Tab reaches every control only with keyboard navigation on (AppleKeyboardUIMode 2).
        let app = launch(["-AppleKeyboardUIMode", "2"])
        #endif
        signIn(app)
        // Tab reaches the first numbered control of the sign-in screen.
        let first = numbered(app).first
        XCTAssertNotNil(first, "no mg.<n>. element on the sign-in screen")
        app.typeKey(XCUIKeyboardKey.tab.rawValue, modifierFlags: [])
        XCTAssertTrue(hasKeyboardFocus(app), "Tab must move focus to an mg.<n>. element")
        // ⌘1…⌘6 select the consoles, once signed in (the last screen is inside one).
        list.last.map { $0(app) }
        let selected = app.descendants(matching: .any)["mg.console.selected"]
        guard selected.waitForExistence(timeout: 5) else { return XCTFail("no console selector") }
        for n in 1...6 {
            app.typeKey("\(n)", modifierFlags: .command)
            XCTAssertEqual(selected.value as? String, "\(n)", "⌘\(n) must select console \(n)")
        }
    }

    /// Whether an `mg.<n>.` element holds keyboard focus.
    func hasKeyboardFocus(_ app: XCUIApplication) -> Bool {
        #if os(iOS)
        return numbered(app).contains { $0.hasFocus }
        #else
        return app.descendants(matching: .any)
            .matching(NSPredicate(format: "hasKeyboardFocus == true AND identifier MATCHES 'mg\\\\.[0-9]+\\\\..*'"))
            .count > 0
        #endif
    }
}

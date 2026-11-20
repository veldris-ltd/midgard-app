import Foundation
import MidgardAuditKit
import XCTest

/// The audit base class runs nothing by itself, and a subclass runs exactly one test
/// per affordance the shell declares, under the names audit-failures.js reads. The UI
/// tests themselves need a running app (Phase B); here only their discovery is checked.
/// [RC-49] [AC-C-11]
final class AuditKitTests: XCTestCase {
    /// A subclass as the apps declare one, kept out of this run: its tests launch an app.
    final class Probe: AccessibilityAuditBase {
        override class var defaultTestSuite: XCTestSuite { XCTestSuite(name: "Probe (not run)") }
    }

    private func declared() throws -> [String] {
        let toml = try String(contentsOf: repositoryRoot().appendingPathComponent("clients/shells/apple/accessibility.toml"),
                              encoding: .utf8)
        let pattern = try NSRegularExpression(pattern: #"(?m)^([a-z_]+)\s*=\s*"conformant""#)
        return pattern.matches(in: toml, range: NSRange(toml.startIndex..., in: toml))
            .map { String(toml[Range($0.range(at: 1), in: toml)!]) }.sorted()
    }

    private func testNames(_ suite: XCTestSuite) -> [String] {
        suite.tests.flatMap { test -> [String] in
            if let inner = test as? XCTestSuite { return testNames(inner) }
            return [test.name]
        }
    }

    func testTheBaseClassRunsNothing() {
        XCTAssertEqual(AccessibilityAuditBase.defaultTestSuite.testCaseCount, 0)
    }

    func testASubclassHasOneTestPerDeclaredAffordance() throws {
        let names = testNames(XCTestSuite(forTestCaseClass: Probe.self))
        let affordances = names.compactMap { name -> String? in
            guard let range = name.range(of: "test_affordance_") else { return nil }
            return String(name[range.upperBound...].prefix { $0 != "]" && $0 != "(" })
        }.sorted()
        XCTAssertEqual(affordances.count, names.count, "only affordance tests: \(names)")
        XCTAssertEqual(affordances, try declared())
    }

    func testTheResultParserKnowsTheSameAffordances() throws {
        let js = try String(contentsOf: repositoryRoot().appendingPathComponent("clients/shells/apple/tools/audit-failures.js"),
                            encoding: .utf8)
        let list = try XCTUnwrap(js.range(of: #"const all = \[[^\]]*\]"#, options: .regularExpression))
        let names = try NSRegularExpression(pattern: #"'([a-z_]+)'"#)
        let text = String(js[list])
        let parsed = names.matches(in: text, range: NSRange(text.startIndex..., in: text))
            .map { String(text[Range($0.range(at: 1), in: text)!]) }.sorted()
        XCTAssertEqual(parsed, try declared())
    }

    /// The one issue the audit lets pass: on macOS, an issue on the app's Touch Bar
    /// element, which every Mac app has and none can label. Anything else, and the
    /// Touch Bar anywhere but macOS, still fails the affordance.
    func testOnlyTheMacTouchBarIsIgnored() {
        XCTAssertTrue(AuditIssueFilter.ignores(onMac: true, elementType: .touchBar))
        XCTAssertFalse(AuditIssueFilter.ignores(onMac: false, elementType: .touchBar))
        for type: XCUIElement.ElementType in [.any, .other, .staticText, .button, .window, .application, .group] {
            XCTAssertFalse(AuditIssueFilter.ignores(onMac: true, elementType: type), "\(type.rawValue)")
        }
        XCTAssertFalse(AuditIssueFilter.ignores(onMac: true, elementType: nil), "an issue with no element")
    }

    /// The filter is fixed in source: nothing from the environment, launch arguments or
    /// defaults can widen it, and the audit applies it on every screen.
    func testTheIssueFilterCannotBeWidenedFromOutside() throws {
        let source = try String(contentsOf: repositoryRoot()
            .appendingPathComponent("clients/shells/apple/Sources/MidgardAuditKit/AccessibilityAudit.swift"), encoding: .utf8)
        for reader in ["ProcessInfo", "UserDefaults", "getenv", "CommandLine", "environment[", ".arguments"] {
            XCTAssertFalse(source.contains(reader), "AccessibilityAudit.swift reads \(reader)")
        }
        XCTAssertEqual(source.components(separatedBy: "performAccessibilityAudit(").count - 1, 1,
                       "one audit call, through the filter")
        XCTAssertTrue(source.contains("performAccessibilityAudit(for: types) { issue in"))
    }
}

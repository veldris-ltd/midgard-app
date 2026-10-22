import XCTest
@testable import MidgardShellCore

final class PathsTests: XCTestCase {
    let bundle = URL(fileURLWithPath: "/Applications/Midgard.app/Contents/Resources")
    let user = URL(fileURLWithPath: "/Users/presenter")

    func testDefaults() {
        let p = Paths(environment: [:], bundleResources: bundle, userHome: user)
        XCTAssertEqual(p.resources.path, "/Applications/Midgard.app/Contents/Resources")
        XCTAssertEqual(p.home.path, "/Users/presenter/Library/Application Support/Midgard")
        XCTAssertEqual(p.logs.path, "/Users/presenter/Library/Logs/Midgard")
    }

    func testOverrides() {
        let env = [
            "MIDGARD_APP_RESOURCES": "/tmp/res",
            "MIDGARD_APP_HOME": "/tmp/home",
            "MIDGARD_APP_LOGS": "/tmp/logs",
        ]
        let p = Paths(environment: env, bundleResources: bundle, userHome: user)
        XCTAssertEqual(p.resources.path, "/tmp/res")
        XCTAssertEqual(p.home.path, "/tmp/home")
        XCTAssertEqual(p.logs.path, "/tmp/logs")
    }

    func testHelpersOutsideABundle() {
        let p = Paths(environment: [:], bundleResources: bundle, userHome: user)
        XCTAssertEqual(p.helpers.path, "/Applications/Midgard.app/Contents/Resources/bin")
        XCTAssertTrue(p.devLayout)
    }

    func testHelpersInABundle() {
        let helpers = URL(fileURLWithPath: "/Applications/Midgard.app/Contents/Helpers")
        let p = Paths(environment: [:], bundleResources: bundle, userHome: user, bundleHelpers: helpers)
        XCTAssertEqual(p.helpers.path, "/Applications/Midgard.app/Contents/Helpers")
        XCTAssertFalse(p.devLayout)
    }

    func testDevelopmentResourcesKeepBin() {
        let helpers = URL(fileURLWithPath: "/Applications/Midgard.app/Contents/Helpers")
        let p = Paths(environment: ["MIDGARD_APP_RESOURCES": "/tmp/res"], bundleResources: bundle,
                      userHome: user, bundleHelpers: helpers)
        XCTAssertEqual(p.helpers.path, "/tmp/res/bin")
        XCTAssertTrue(p.devLayout)
    }

    func testEmptyOverrideIsIgnored() {
        let p = Paths(environment: ["MIDGARD_APP_HOME": ""], bundleResources: bundle, userHome: user)
        XCTAssertEqual(p.home.path, "/Users/presenter/Library/Application Support/Midgard")
    }

    func testCurrentUsesRealHome() {
        let p = Paths.current(environment: [:])
        XCTAssertTrue(p.home.path.hasSuffix("/Library/Application Support/Midgard"))
    }
}

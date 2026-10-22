import XCTest
@testable import MidgardShellCore

final class SystemCheckTests: XCTestCase {
    let gib: UInt64 = 1024 * 1024 * 1024

    func v(_ major: Int, _ minor: Int, _ patch: Int = 0) -> OperatingSystemVersion {
        OperatingSystemVersion(majorVersion: major, minorVersion: minor, patchVersion: patch)
    }

    func testSupportedMac() {
        XCTAssertNil(SystemCheck.problem(osVersion: v(13, 0), memoryBytes: 8 * gib))
        XCTAssertNil(SystemCheck.problem(osVersion: v(26, 1), memoryBytes: 16 * gib))
    }

    func testOldMacOS() {
        XCTAssertEqual(
            SystemCheck.problem(osVersion: v(12, 6), memoryBytes: 16 * gib),
            "Midgard needs macOS 13 or later; this Mac runs 12.6.")
        XCTAssertEqual(
            SystemCheck.problem(osVersion: v(12, 7, 4), memoryBytes: 16 * gib),
            "Midgard needs macOS 13 or later; this Mac runs 12.7.4.")
    }

    func testTooLittleMemory() {
        let bytes = UInt64(7.9 * Double(gib))
        XCTAssertEqual(
            SystemCheck.problem(osVersion: v(14, 0), memoryBytes: bytes),
            "Midgard needs at least 8 GB of memory; this Mac has 7.9 GB.")
    }

    func testOsReportedFirstWhenBothFail() {
        let msg = SystemCheck.problem(osVersion: v(11, 0), memoryBytes: 4 * gib)
        XCTAssertEqual(msg, "Midgard needs macOS 13 or later; this Mac runs 11.0.")
    }

    func testThisMacPasses() {
        // The development Macs meet the requirement; this exercises the real readings.
        XCTAssertNil(SystemCheck.problemOnThisMac())
    }
}

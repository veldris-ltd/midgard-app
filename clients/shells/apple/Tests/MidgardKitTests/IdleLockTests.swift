import XCTest

@testable import MidgardKit

/// The idle lock fires once after the timeout, not while activity continues, and not
/// after it is stopped. [FR-C-9]
@MainActor
final class IdleLockTests: XCTestCase {
    func testExpiresAfterTheTimeout() async {
        let fired = expectation(description: "expired")
        let idle = IdleLock(timeout: 0.05) { fired.fulfill() }
        idle.touch()
        await fulfillment(of: [fired], timeout: 2)
    }

    func testStopCancels() async throws {
        var count = 0
        let idle = IdleLock(timeout: 0.05) { count += 1 }
        idle.touch()
        idle.stop()
        try await Task.sleep(nanoseconds: 200_000_000)
        XCTAssertEqual(count, 0)
    }

    func testActivityPostpones() async throws {
        var count = 0
        let idle = IdleLock(timeout: 0.3) { count += 1 }
        idle.touch()
        try await Task.sleep(nanoseconds: 150_000_000)
        idle.touch()
        try await Task.sleep(nanoseconds: 150_000_000)
        XCTAssertEqual(count, 0)
        idle.stop()
    }
}

import Darwin
import XCTest
@testable import MidgardShellCore

final class PortsTests: XCTestCase {
    func testPortsAreDistinctAndNonZero() throws {
        let ports = try Ports.free(count: 8)
        XCTAssertEqual(ports.count, 8)
        XCTAssertEqual(Set(ports).count, 8)
        XCTAssertFalse(ports.contains(0))
    }

    func testZeroCountIsEmpty() throws {
        XCTAssertEqual(try Ports.free(count: 0), [])
    }

    func testEachPortCanBeBoundAfterwards() throws {
        for port in try Ports.free(count: 4) {
            let fd = socket(AF_INET, SOCK_STREAM, 0)
            XCTAssertGreaterThanOrEqual(fd, 0)
            defer { close(fd) }
            var addr = sockaddr_in()
            addr.sin_family = sa_family_t(AF_INET)
            addr.sin_port = port.bigEndian
            addr.sin_addr.s_addr = inet_addr("127.0.0.1")
            let rc = withUnsafePointer(to: &addr) {
                $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                    Darwin.bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                }
            }
            XCTAssertEqual(rc, 0, "port \(port) could not be bound: errno \(errno)")
        }
    }
}

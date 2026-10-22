import XCTest
@testable import MidgardShellCore

final class HomeTests: XCTestCase {
    var root: URL!
    var starting: URL!
    var home: URL!
    let fm = FileManager.default

    override func setUpWithError() throws {
        root = fm.temporaryDirectory.appendingPathComponent("home-tests-\(UUID().uuidString)", isDirectory: true)
        starting = root.appendingPathComponent("starting-state", isDirectory: true)
        home = root.appendingPathComponent("Support/Midgard", isDirectory: true)
        try fm.createDirectory(at: starting.appendingPathComponent("gateway/keys"), withIntermediateDirectories: true)
        try "v1\n".write(to: starting.appendingPathComponent("VERSION"), atomically: true, encoding: .utf8)
        try "listen = 1".write(to: starting.appendingPathComponent("gateway/config.toml"), atomically: true, encoding: .utf8)
        let key = starting.appendingPathComponent("gateway/keys/issuer.key")
        try "secret".write(to: key, atomically: true, encoding: .utf8)
        try fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: key.path)
    }

    override func tearDownWithError() throws {
        try? fm.removeItem(at: root)
    }

    private func read(_ relative: String) throws -> String {
        try String(contentsOf: home.appendingPathComponent(relative), encoding: .utf8)
    }

    private func siblings(containing fragment: String) throws -> [String] {
        try fm.contentsOfDirectory(atPath: home.deletingLastPathComponent().path)
            .filter { $0.contains(fragment) }
    }

    func testFirstPrepareCopiesAndMarks() throws {
        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertEqual(try read("gateway/keys/issuer.key"), "secret")
        let mode = try fm.attributesOfItem(atPath: home.appendingPathComponent("gateway/keys/issuer.key").path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o600)
        XCTAssertEqual(try read(".midgard-home"), "v1")
        XCTAssertTrue(Home.isPrepared(starting: starting, home: home))
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testVersionDefaultsToDev() throws {
        try fm.removeItem(at: starting.appendingPathComponent("VERSION"))
        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read(".midgard-home"), "dev")
    }

    func testSecondPrepareIsANoOp() throws {
        try Home.prepare(starting: starting, home: home)
        try "edited".write(to: home.appendingPathComponent("gateway/config.toml"), atomically: true, encoding: .utf8)
        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "edited")
    }

    func testInterruptedCopyIsCleanedUpAndRedone() throws {
        // A crash mid-copy: a leftover temp sibling, and a home with no marker.
        let parent = home.deletingLastPathComponent()
        try fm.createDirectory(at: parent, withIntermediateDirectories: true)
        let temp = parent.appendingPathComponent("Midgard.midgard-copy-DEAD")
        try fm.createDirectory(at: temp, withIntermediateDirectories: true)
        try "half".write(to: temp.appendingPathComponent("partial"), atomically: true, encoding: .utf8)
        try fm.createDirectory(at: home, withIntermediateDirectories: true)
        try "stale".write(to: home.appendingPathComponent("leftover"), atomically: true, encoding: .utf8)
        XCTAssertFalse(Home.isPrepared(starting: starting, home: home))

        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertFalse(fm.fileExists(atPath: home.appendingPathComponent("leftover").path))
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testResetRestoresStartingState() throws {
        try Home.prepare(starting: starting, home: home)
        try "edited".write(to: home.appendingPathComponent("gateway/config.toml"), atomically: true, encoding: .utf8)
        try "new".write(to: home.appendingPathComponent("added.txt"), atomically: true, encoding: .utf8)

        let report = try Home.reset(starting: starting, home: home)
        XCTAssertNil(report.leftover)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertFalse(fm.fileExists(atPath: home.appendingPathComponent("added.txt").path))
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testResetWithoutAnyHomeStillPrepares() throws {
        try Home.reset(starting: starting, home: home)
        XCTAssertTrue(Home.isPrepared(starting: starting, home: home))
    }

    func testVersionChangeTriggersFreshCopy() throws {
        try Home.prepare(starting: starting, home: home)
        try "edited".write(to: home.appendingPathComponent("gateway/config.toml"), atomically: true, encoding: .utf8)
        try "v2".write(to: starting.appendingPathComponent("VERSION"), atomically: true, encoding: .utf8)
        XCTAssertFalse(Home.isPrepared(starting: starting, home: home))

        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertEqual(try read(".midgard-home"), "v2")
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testMissingStartingStateFailsWithoutMarking() throws {
        let missing = root.appendingPathComponent("nope")
        XCTAssertThrowsError(try Home.prepare(starting: missing, home: home))
        XCTAssertFalse(Home.isPrepared(starting: missing, home: home))
    }
}

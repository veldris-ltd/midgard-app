import Darwin
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
        // Some tests lock folders; unlock everything so it can be deleted.
        if let walk = fm.enumerator(atPath: root.path) {
            for case let relative as String in walk {
                let path = root.appendingPathComponent(relative).path
                var isDir: ObjCBool = false
                if fm.fileExists(atPath: path, isDirectory: &isDir), isDir.boolValue {
                    chmod(path, 0o755)
                }
            }
        }
        try? fm.removeItem(at: root)
    }

    private func read(_ relative: String) throws -> String {
        try String(contentsOf: home.appendingPathComponent(relative), encoding: .utf8)
    }

    private func write(_ text: String, _ relative: String) throws {
        try text.write(to: home.appendingPathComponent(relative), atomically: true, encoding: .utf8)
    }

    private func siblings(containing fragment: String) throws -> [String] {
        let parent = home.deletingLastPathComponent().path
        guard fm.fileExists(atPath: parent) else { return [] }
        return try fm.contentsOfDirectory(atPath: parent)
            .filter { $0.contains(fragment) }
    }

    private func mode(_ url: URL) throws -> Int? {
        try fm.attributesOfItem(atPath: url.path)[.posixPermissions] as? Int
    }

    func testFirstPrepareCopiesAndMarks() throws {
        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertEqual(try read("gateway/keys/issuer.key"), "secret")
        XCTAssertEqual(try mode(home.appendingPathComponent("gateway/keys/issuer.key")), 0o600)
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
        try write("edited", "gateway/config.toml")
        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "edited")
    }

    func testInterruptedCopyIsCleanedUpAndRedone() throws {
        // A crash mid-copy leaves only a temporary sibling; home was never moved into place.
        let parent = home.deletingLastPathComponent()
        let temp = parent.appendingPathComponent("Midgard.midgard-copy-DEAD")
        try fm.createDirectory(at: temp, withIntermediateDirectories: true)
        try "half".write(to: temp.appendingPathComponent("partial"), atomically: true, encoding: .utf8)
        XCTAssertFalse(Home.isPrepared(starting: starting, home: home))

        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertFalse(fm.fileExists(atPath: home.appendingPathComponent("partial").path))
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testEmptyHomeIsReplaced() throws {
        try fm.createDirectory(at: home, withIntermediateDirectories: true)
        try Home.prepare(starting: starting, home: home)
        XCTAssertTrue(Home.isPrepared(starting: starting, home: home))
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testForeignFolderIsNeverReplaced() throws {
        try fm.createDirectory(at: home, withIntermediateDirectories: true)
        try write("mine", "notes.txt")
        for call in [{ try Home.prepare(starting: self.starting, home: self.home) },
                     { _ = try Home.reset(starting: self.starting, home: self.home) }] {
            XCTAssertThrowsError(try call()) { error in
                XCTAssertEqual(error as? HomeError, .notAWorkingCopy(self.home))
                XCTAssertTrue(String(describing: error).contains(self.home.path))
            }
            XCTAssertEqual(try fm.contentsOfDirectory(atPath: home.path), ["notes.txt"])
            XCTAssertEqual(try read("notes.txt"), "mine")
            XCTAssertEqual(try siblings(containing: ".midgard-"), [])
        }
    }

    func testSymlinkedStartingStateIsCopiedAsARealFolder() throws {
        let link = root.appendingPathComponent("starting-link")
        try fm.createSymbolicLink(at: link, withDestinationURL: starting)
        try Home.prepare(starting: link, home: home)
        XCTAssertNil(try? fm.destinationOfSymbolicLink(atPath: home.path))
        var isDir: ObjCBool = false
        XCTAssertTrue(fm.fileExists(atPath: home.path, isDirectory: &isDir) && isDir.boolValue)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertFalse(fm.fileExists(atPath: starting.appendingPathComponent(".midgard-home").path))
        XCTAssertFalse(fm.fileExists(atPath: link.appendingPathComponent(".midgard-home").path))
    }

    func testSymlinkInsideStartingStateIsRejected() throws {
        let inner = starting.appendingPathComponent("gateway/keys/link.key")
        try fm.createSymbolicLink(atPath: inner.path, withDestinationPath: "/etc/hosts")
        XCTAssertThrowsError(try Home.prepare(starting: starting, home: home)) { error in
            guard case .symlinkInStartingState(let url) = error as? HomeError else {
                return XCTFail("expected symlinkInStartingState, got \(error)")
            }
            XCTAssertEqual(url.lastPathComponent, "link.key")
            XCTAssertTrue(String(describing: error).contains("link.key"))
        }
        XCTAssertFalse(fm.fileExists(atPath: home.path))
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testCopyIsWritableAndUnquarantined() throws {
        let locked = starting.appendingPathComponent("locked")
        try fm.createDirectory(at: locked, withIntermediateDirectories: true)
        let file = locked.appendingPathComponent("data.json")
        try "{}".write(to: file, atomically: true, encoding: .utf8)
        let value = "0081;00000000;Safari;"
        XCTAssertEqual(setxattr(file.path, "com.apple.quarantine", value, value.utf8.count, 0, 0), 0)
        XCTAssertEqual(chmod(locked.path, 0o555), 0)

        try Home.prepare(starting: starting, home: home)
        let copied = home.appendingPathComponent("locked")
        XCTAssertEqual(try XCTUnwrap(mode(copied)) & 0o700, 0o700)
        XCTAssertEqual(getxattr(copied.appendingPathComponent("data.json").path,
                                "com.apple.quarantine", nil, 0, 0, 0), -1)
        // The demo can write there.
        try write("{\"x\":1}", "locked/data.json")
    }

    func testRoomCheck() {
        XCTAssertNoThrow(try Home.checkRoom(needed: 10, available: 10))
        XCTAssertThrowsError(try Home.checkRoom(needed: 11, available: 10)) { error in
            XCTAssertEqual(error as? HomeError, .insufficientSpace(needed: 11, available: 10))
        }
    }

    func testResetRestoresStartingState() throws {
        try Home.prepare(starting: starting, home: home)
        try write("edited", "gateway/config.toml")
        try write("new", "added.txt")

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

    func testResetWhoseFreshCopyFailsPutsThePreviousCopyBack() throws {
        try Home.prepare(starting: starting, home: home)
        try write("edited", "gateway/config.toml")
        try fm.removeItem(at: starting)

        XCTAssertThrowsError(try Home.reset(starting: starting, home: home))
        XCTAssertEqual(try read("gateway/config.toml"), "edited")
        XCTAssertEqual(try read(".midgard-home"), "v1")
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testResetReportsAnUndeletablePreviousCopy() throws {
        try Home.prepare(starting: starting, home: home)
        let locked = home.appendingPathComponent("locked")
        try fm.createDirectory(at: locked, withIntermediateDirectories: true)
        try "x".write(to: locked.appendingPathComponent("stuck"), atomically: true, encoding: .utf8)
        XCTAssertEqual(chmod(locked.path, 0o555), 0)

        let report = try Home.reset(starting: starting, home: home)
        let leftover = try XCTUnwrap(report.leftover)
        XCTAssertTrue(leftover.lastPathComponent.hasPrefix("Midgard.midgard-old-"))
        XCTAssertNotNil(report.reason)
        XCTAssertTrue(fm.fileExists(atPath: leftover.path))
        XCTAssertTrue(Home.isPrepared(starting: starting, home: home))
        XCTAssertFalse(fm.fileExists(atPath: locked.path))
    }

    func testSuccessfulPrepareSweepsOldCopies() throws {
        let parent = home.deletingLastPathComponent()
        let old = parent.appendingPathComponent("Midgard.midgard-old-BEEF")
        try fm.createDirectory(at: old, withIntermediateDirectories: true)
        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testVersionChangeTriggersFreshCopy() throws {
        try Home.prepare(starting: starting, home: home)
        try write("edited", "gateway/config.toml")
        try "v2".write(to: starting.appendingPathComponent("VERSION"), atomically: true, encoding: .utf8)
        XCTAssertFalse(Home.isPrepared(starting: starting, home: home))

        try Home.prepare(starting: starting, home: home)
        XCTAssertEqual(try read("gateway/config.toml"), "listen = 1")
        XCTAssertEqual(try read(".midgard-home"), "v2")
        XCTAssertEqual(try siblings(containing: ".midgard-"), [])
    }

    func testMissingStartingStateFailsWithoutMarking() throws {
        let missing = root.appendingPathComponent("nope")
        XCTAssertThrowsError(try Home.prepare(starting: missing, home: home)) { error in
            XCTAssertEqual(error as? HomeError, .startingStateMissing(missing))
        }
        XCTAssertFalse(Home.isPrepared(starting: missing, home: home))
        XCTAssertFalse(fm.fileExists(atPath: home.path))
    }
}

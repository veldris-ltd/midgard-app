import Foundation
import MidgardCore
import XCTest

@testable import MidgardKit

/// The `-MidgardUITesting` launch mode both apps share (Debug builds only): a fresh
/// store each launch with earlier launches' removed, the software key root that never
/// prompts, secrets in memory, the data source from Info.plist. [RC-49] [AC-C-11]
final class UITestingEnvironmentTests: XCTestCase {
    private var tmp: URL!

    override func setUpWithError() throws {
        tmp = try temporaryDirectory()
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: tmp)
    }

    private func bundle(_ dataSource: String) throws -> Bundle {
        let url = tmp.appendingPathComponent("App-\(UUID().uuidString).bundle", isDirectory: true)
        let contents = url.appendingPathComponent("Contents", isDirectory: true)
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        try PropertyListSerialization.data(fromPropertyList: ["MidgardDataSource": dataSource, "MidgardEdition": "release",
                                                              "CFBundleIdentifier": "test.midgard.\(UUID().uuidString)"],
                                           format: .xml, options: 0)
            .write(to: contents.appendingPathComponent("Info.plist"))
        return try XCTUnwrap(Bundle(url: url))
    }

    func testRequestedOnlyByTheArgument() {
        XCTAssertTrue(UITestingEnvironment.requested(["app", "-MidgardUITesting"]))
        XCTAssertFalse(UITestingEnvironment.requested(["app"]))
    }

    func testEachLaunchStartsFreshAndRemovesEarlierStores() throws {
        let old = tmp.appendingPathComponent("midgard-uitesting-earlier", isDirectory: true)
        let unrelated = tmp.appendingPathComponent("something-else", isDirectory: true)
        try FileManager.default.createDirectory(at: old, withIntermediateDirectories: true)
        try FileManager.default.createDirectory(at: unrelated, withIntermediateDirectories: true)
        let device = DeviceInfo(ramMb: 8192, hasAccelerator: true)

        let env = try UITestingEnvironment.make(reason: "test", device: device, bundle: try bundle("none"),
                                                temporaryDirectory: tmp)
        XCTAssertFalse(FileManager.default.fileExists(atPath: old.path), "an earlier launch's store is removed")
        XCTAssertTrue(FileManager.default.fileExists(atPath: unrelated.path), "nothing else is")
        XCTAssertEqual(env.dataSource, DataSourceChoice.none)
        XCTAssertTrue(env.storeDirectory.path.hasPrefix(tmp.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: env.storeDirectory.path))
        XCTAssertEqual(env.keyRoot.custodyLabel, UITestingEnvironment.custodyLabel)
        XCTAssertFalse(env.keyRoot.hardwareBacked())
        XCTAssertTrue(env.secrets is UITestingEnvironment.MemorySecretStore, "never the keychain")

        // The key root seals and unseals without a prompt.
        let key = Data((0..<32).map { UInt8($0) })
        let sealed = try env.keyRoot.seal(dataKey: key.withUnsafeBytes { Data($0) })  // its own buffer: seal wipes it
        XCTAssertEqual(try env.keyRoot.unseal(sealed: sealed), key)

        // The next launch removes this one's store; quitting removes its own.
        let next = try UITestingEnvironment.make(reason: "test", device: device, bundle: try bundle("none"),
                                                 temporaryDirectory: tmp)
        XCTAssertFalse(FileManager.default.fileExists(atPath: env.storeDirectory.path))
        UITestingEnvironment.removeCurrentStore()
        XCTAssertFalse(FileManager.default.fileExists(atPath: next.storeDirectory.path))
    }

    func testAMisconfiguredBundleFailsBeforeTouchingAnything() throws {
        let old = tmp.appendingPathComponent("midgard-uitesting-earlier", isDirectory: true)
        try FileManager.default.createDirectory(at: old, withIntermediateDirectories: true)
        XCTAssertThrowsError(try UITestingEnvironment.make(reason: "test", device: DeviceInfo(ramMb: 1, hasAccelerator: false),
                                                           bundle: try bundle("demo"), temporaryDirectory: tmp)) {
            XCTAssertEqual($0 as? AppEnvironment.Failure, .misconfigured(AppEnvironment.dataSourceKey))
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: old.path))
    }

    func testTheMemoryStoreKeepsItsOwnCopies() throws {
        let store = UITestingEnvironment.MemorySecretStore()
        var data = Data([1, 2, 3])
        try store.write(data, service: "s", account: "a")
        data[0] = 9
        XCTAssertEqual(try store.read(service: "s", account: "a"), Data([1, 2, 3]))
        try store.write(Data([4]), service: "s", account: "b")
        try store.delete(service: "s", account: "a")
        XCTAssertNil(try store.read(service: "s", account: "a"))
        XCTAssertEqual(try store.read(service: "s", account: "b"), Data([4]))
        try store.deleteAll(service: "s")
        XCTAssertNil(try store.read(service: "s", account: "b"))
    }
}

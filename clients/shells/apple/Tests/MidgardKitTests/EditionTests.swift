import Foundation
import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// The three editions (owner decision 22 Nov 2026): Midgard, the signed release
/// `v0.1.0-rc.3`; Midgard 3D, the same plus a Heimdall globe; and Midgard Maps, the 3D
/// edition with the network entitlement for map tiles. The edition comes from
/// Info.plist `MidgardEdition`; a missing or unknown value is the "app misconfigured"
/// failed state, as with `MidgardDataSource`. The release apps show nothing new; only the
/// 3D editions have an About screen and a second banner. The "3D" entry needs both a 3D
/// edition and the globe library's link-time registration. [FR-C-1] [AC-C-13]
@MainActor
final class EditionTests: XCTestCase {
    private var dir: URL!

    override func setUp() async throws {
        dir = try temporaryDirectory()
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: dir)
    }

    private func fakeBundle(_ info: [String: Any]) throws -> Bundle {
        let url = dir.appendingPathComponent("\(UUID().uuidString).bundle", isDirectory: true)
        let contents = url.appendingPathComponent("Contents", isDirectory: true)
        try FileManager.default.createDirectory(at: contents, withIntermediateDirectories: true)
        var plist = info
        plist["CFBundleIdentifier"] = "test.midgard.\(UUID().uuidString)"
        try PropertyListSerialization.data(fromPropertyList: plist, format: .xml, options: 0)
            .write(to: contents.appendingPathComponent("Info.plist"))
        return try XCTUnwrap(Bundle(url: url))
    }

    private func environment(edition: AppEdition) -> AppEnvironment {
        let secrets = InMemorySecretStore()
        let root = SoftwareKeyRoot(reason: "test", label: "Software key (test)", secrets: secrets,
                                   presence: AlwaysPresent())
        return AppEnvironment(dataSource: .none, edition: edition, storeDirectory: dir, secrets: secrets,
                              keyRoot: root, device: DeviceInfo(ramMb: 8192, hasAccelerator: true),
                              replicaId: "edition-test")
    }

    // MARK: reading MidgardEdition

    func testTheEditionIsReadFromTheInfoPlist() throws {
        XCTAssertEqual(AppEnvironment.editionKey, "MidgardEdition")
        XCTAssertEqual(try AppEnvironment.edition(in: try fakeBundle(["MidgardEdition": "release"])), .release)
        XCTAssertEqual(try AppEnvironment.edition(in: try fakeBundle(["MidgardEdition": "3d"])), .threeD)
        XCTAssertEqual(try AppEnvironment.edition(in: try fakeBundle(["MidgardEdition": "maps"])), .maps)
        for info: [String: Any] in [[:], ["MidgardEdition": ""], ["MidgardEdition": "Release"],
                                    ["MidgardEdition": "3D"], ["MidgardEdition": "globe"], ["MidgardEdition": "Maps"],
                                    ["MidgardEdition": "3d-maps"], ["MidgardEdition": 3]] {
            XCTAssertThrowsError(try AppEnvironment.edition(in: try fakeBundle(info)), "\(info)") {
                XCTAssertEqual($0 as? AppEnvironment.Failure, .misconfigured(AppEnvironment.editionKey))
            }
        }
    }

    // A missing or unknown value is a failed state: the one release-app path the edition
    // can reach, and only for a misbuilt app.
    func testAMissingOrUnknownEditionIsAFailedState() async throws {
        for info: [String: Any] in [["MidgardDataSource": "none"],
                                    ["MidgardDataSource": "none", "MidgardEdition": "beta"]] {
            let bundle = try fakeBundle(info)
            let model = AppModel()
            await model.start {
                try AppEnvironment.live(reason: "test", device: DeviceInfo(ramMb: 8192, hasAccelerator: true), bundle: bundle)
            }
            let failure = AppEnvironment.Failure.misconfigured(AppEnvironment.editionKey)
            XCTAssertEqual(model.phase, .failed(ErrorText.describe(failure)), "\(info)")
            XCTAssertTrue(ErrorText.describe(failure).contains("MidgardEdition"))
            XCTAssertTrue(ErrorText.describe(failure).hasPrefix("This app is misconfigured"))
            XCTAssertNil(model.edition, "no edition is claimed")
            XCTAssertNil(model.editionBanner)
            XCTAssertNil(model.about)
            XCTAssertNil(model.threeDEntry)
        }
    }

    func testTheUITestingLaunchReadsTheEditionToo() throws {
        let device = DeviceInfo(ramMb: 8192, hasAccelerator: true)
        let env = try UITestingEnvironment.make(reason: "test", device: device,
                                                bundle: try fakeBundle(["MidgardDataSource": "none", "MidgardEdition": "3d"]),
                                                temporaryDirectory: dir)
        XCTAssertEqual(env.edition, .threeD)
        XCTAssertThrowsError(try UITestingEnvironment.make(reason: "test", device: device,
                                                           bundle: try fakeBundle(["MidgardDataSource": "none"]),
                                                           temporaryDirectory: dir)) {
            XCTAssertEqual($0 as? AppEnvironment.Failure, .misconfigured(AppEnvironment.editionKey))
        }
        UITestingEnvironment.removeCurrentStore()
    }

    // MARK: what the model shows comes from the linked provider

    /// SwiftPM links every test target into one bundle, MidgardGlobeKitTests included, so
    /// the edition provider is registered here; a release app links none, and
    /// tools/check-edition.sh checks that. Through the provider, the release edition still
    /// gets nothing.
    func testTheModelShowsOnlyWhatTheProviderGivesTheEdition() async {
        let release = AppModel()
        await release.start(environment(edition: .release))
        XCTAssertEqual(release.phase, .needsEnrolment)
        XCTAssertEqual(release.edition, .release)
        XCTAssertNil(release.editionBanner)
        XCTAssertNil(release.about)
        XCTAssertNil(release.threeDEntry)
        XCTAssertEqual(release.banner, .noSource, "the source banner is unchanged")

        for edition in [AppEdition.threeD, .maps] {
            let model = AppModel()
            await model.start(environment(edition: edition))
            XCTAssertEqual(model.edition, edition)
            XCTAssertNotNil(model.editionBanner, "\(edition)")
            XCTAssertNotNil(model.about, "\(edition)")
            XCTAssertNotNil(model.threeDEntry, "\(edition)")
            XCTAssertEqual(model.banner, .noSource)
        }
    }

    // MARK: MidgardKit, which the release apps link, holds nothing of the editions

    private func sources(_ modules: [String]) throws -> [(String, String)] {
        let base = repositoryRoot().appendingPathComponent("clients/shells/apple/Sources")
        var out: [(String, String)] = []
        for module in modules {
            let files = try XCTUnwrap(FileManager.default.enumerator(at: base.appendingPathComponent(module),
                                                                    includingPropertiesForKeys: nil))
                .compactMap { $0 as? URL }.filter { ["swift", "c", "h", "modulemap"].contains($0.pathExtension) }
            XCTAssertFalse(files.isEmpty, module)
            for file in files {
                out.append(("\(module)/\(file.lastPathComponent)", try String(contentsOf: file, encoding: .utf8)))
            }
        }
        return out
    }

    func testMidgardKitHoldsNoEditionWordingGlobeOrRendererImport() throws {
        // The release apps link these modules: tools/check-edition.sh refuses each word
        // below in their binaries; here the sources are held to it.
        let words = ["Globe", "not in the signed release", "not the signed release", "Midgard Maps", "Midgard 3D",
                     "\"Edition: ", "ea5899ab", "v0.1.0-rc.3", "EditionBanner", "EditionAbout", "AboutView",
                     "SignedRelease", "EditionUIProvider"]
        for (name, text) in try sources(["MidgardKit", "MidgardHooks", "MidgardAuditKit"]) {
            for word in words {
                XCTAssertFalse(text.contains(word), "\(name) holds \"\(word)\"")
            }
        }
        let renderer = try NSRegularExpression(pattern: #"(import\s+(Metal|MetalKit)\b)|(#\s*(include|import)\s*<Metal)"#)
        for (name, text) in try sources(["MidgardKit", "MidgardHooks"]) {
            XCTAssertEqual(renderer.numberOfMatches(in: text, range: NSRange(text.startIndex..., in: text)), 0,
                           "\(name) imports Metal or MetalKit")
        }
    }
}

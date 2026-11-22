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

    // MARK: banner

    func testOnlyThe3DEditionsShowAnEditionBanner() async {
        let release = AppModel()
        await release.start(environment(edition: .release))
        XCTAssertEqual(release.phase, .needsEnrolment)
        XCTAssertEqual(release.edition, .release)
        XCTAssertNil(release.editionBanner)
        XCTAssertEqual(release.banner, .noSource, "the source banner is unchanged")

        let threeD = AppModel()
        await threeD.start(environment(edition: .threeD))
        XCTAssertEqual(threeD.edition, .threeD)
        XCTAssertEqual(threeD.editionBanner, .threeD)
        XCTAssertEqual(threeD.banner, .noSource, "the 3D edition keeps the source banner")
        XCTAssertEqual(EditionBanner.threeD.text, "3D edition, not in the signed release")
        XCTAssertEqual(EditionBanner.threeD.identifier, "mg.banner.edition3d")

        let maps = AppModel()
        await maps.start(environment(edition: .maps))
        XCTAssertEqual(maps.edition, .maps)
        XCTAssertEqual(maps.editionBanner, .maps)
        XCTAssertEqual(maps.banner, .noSource)
        XCTAssertEqual(EditionBanner.maps.text, "3D maps edition, not in the signed release")
        XCTAssertEqual(EditionBanner.maps.identifier, "mg.banner.editionmaps")
    }

    // MARK: About

    func testTheReleaseEditionHasNoAbout() async {
        XCTAssertNil(EditionAbout(.release), "the release apps show nothing new")
        let release = AppModel()
        await release.start(environment(edition: .release))
        XCTAssertNil(release.about)
        let threeD = AppModel()
        await threeD.start(environment(edition: .threeD))
        XCTAssertNotNil(threeD.about)
        let maps = AppModel()
        await maps.start(environment(edition: .maps))
        XCTAssertEqual(maps.about?.name, "Midgard Maps")
    }

    func testAboutNamesThe3DEditionAndNeverCallsItTheSignedRelease() throws {
        XCTAssertEqual(SignedRelease.tag, "v0.1.0-rc.3")
        XCTAssertEqual(SignedRelease.commit, "ea5899ab")
        let about = try XCTUnwrap(EditionAbout(.threeD))
        XCTAssertEqual(about.name, "Midgard 3D")
        XCTAssertEqual(about.editionLine, "Edition: Midgard 3D")
        XCTAssertEqual(about.releaseLine,
                       "3D edition, not in the signed release. It is v0.1.0-rc.3 (ea5899ab) with a 3D view added to Heimdall.")
        let maps = try XCTUnwrap(EditionAbout(.maps))
        XCTAssertEqual(maps.editionLine, "Edition: Midgard Maps")
        XCTAssertTrue(maps.releaseLine.hasPrefix("3D maps edition, not in the signed release. It is v0.1.0-rc.3"))
    }

    func testEditionViewsRender() throws {
        MGFonts.registerAll()
        let renderer = ImageRenderer(content: AboutView(try XCTUnwrap(EditionAbout(.threeD))).frame(width: 390, height: 600))
        XCTAssertNotNil(renderer.cgImage)
        XCTAssertNotNil(ImageRenderer(content: EditionBannerView(.threeD).frame(width: 390)).cgImage)
        XCTAssertNotNil(ImageRenderer(content: EditionBannerView(.maps).frame(width: 390)).cgImage)
    }

    // MARK: the "3D" entry: edition and link-time registration

    func testThe3DEntryNeedsThe3DEditionAndARegisteredLibrary() {
        let registered = StubThreeD()
        XCTAssertNil(HeimdallThreeD.entry(edition: .release, registered: registered), "never in the release edition")
        XCTAssertNil(HeimdallThreeD.entry(edition: nil, registered: registered))
        XCTAssertNil(HeimdallThreeD.entry(edition: .threeD, registered: nil), "not without the library")
        XCTAssertTrue(HeimdallThreeD.entry(edition: .threeD, registered: registered) === registered)
        XCTAssertTrue(HeimdallThreeD.entry(edition: .maps, registered: registered) === registered)
        XCTAssertNil(HeimdallThreeD.entry(edition: .maps, registered: nil))
    }

    func testMidgardKitNamesNoGlobeAndNeverImportsTheGlobeLibrary() throws {
        // The release apps link MidgardKit: a "Globe" name in it would put a Globe type
        // in their binaries, which tools/check-edition.sh refuses.
        let sources = repositoryRoot().appendingPathComponent("clients/shells/apple/Sources")
        for module in ["MidgardKit", "MidgardHooks", "MidgardAuditKit"] {
            let base = sources.appendingPathComponent(module)
            let files = try XCTUnwrap(FileManager.default.enumerator(at: base, includingPropertiesForKeys: nil))
                .compactMap { $0 as? URL }.filter { ["swift", "c", "h"].contains($0.pathExtension) }
            XCTAssertFalse(files.isEmpty, module)
            for file in files {
                let text = try String(contentsOf: file, encoding: .utf8)
                XCTAssertFalse(text.contains("Globe"), "\(module)/\(file.lastPathComponent) names a Globe")
            }
        }
    }
}

private final class StubThreeD: HeimdallThreeDView {
    let title = "stub"
    func makeView() -> AnyView { AnyView(Text("stub")) }
}

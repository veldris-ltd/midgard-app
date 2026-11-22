import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Heimdall's Operating Picture over the release's sample data: the five tracks and the
/// four layers as the web console loads them, E-0871 selected first, the options'
/// human gate authorising through the core (the audit id the mock would give), Reject
/// and Hold as the web console has them, and a lower clearance seeing withheld tracks
/// by count only. The views render in both modes. [FR-H-4] [FR-H-5] [AC-C-13] [DD-7]
@MainActor
final class HeimdallPictureTests: XCTestCase {
    private var dir: URL!

    override func setUp() async throws {
        dir = try temporaryDirectory()
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: dir)
    }

    private func signedIn(_ id: String) async throws -> AppModel {
        let secrets = InMemorySecretStore()
        let keyRoot = SoftwareKeyRoot(reason: "test", label: "Software key (test)", secrets: secrets,
                                      presence: AlwaysPresent())
        let app = AppModel(idleTimeout: 3600)
        await app.start(AppEnvironment(dataSource: .sample, edition: .release, storeDirectory: dir, secrets: secrets,
                                       keyRoot: keyRoot, device: DeviceInfo(ramMb: 8192, hasAccelerator: true),
                                       replicaId: "heimdall-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    func testThePictureLoadsAsTheWebConsoleShowsIt() async throws {
        let model = HeimdallPictureModel(app: try await signedIn("k.osei"))
        await model.load()
        XCTAssertEqual(model.tracks.map(\.id), ["E-2214", "E-2231", "E-0442", "E-0871", "E-1108"])
        XCTAssertEqual(model.layers.map(\.id), ["air", "maritime", "ground", "feeds"])
        XCTAssertEqual(model.layerOn, ["air": true, "maritime": true, "ground": true, "feeds": false])
        XCTAssertEqual(model.selected, "E-0871")
        XCTAssertEqual(model.options.map(\.rank), [1, 2, 3])
        XCTAssertEqual(model.withheld, 0)
        XCTAssertEqual(model.markings["E-0871"]?.label, "CONFIDENTIAL // OPS-N")
        XCTAssertEqual(HeimdallPictureModel.range(try XCTUnwrap(model.selectedTrack)), 37)

        model.setLayer("maritime", false)
        XCTAssertFalse(model.visibleTracks.contains { $0.id == "E-0442" })
        model.setLayer("maritime", true)
        XCTAssertEqual(model.visibleTracks.count, 5)
    }

    func testAuthoriseGoesThroughTheCoreAndShowsTheAuditId() async throws {
        let model = HeimdallPictureModel(app: try await signedIn("k.osei"))
        await model.load()
        XCTAssertEqual(model.gate, .pending)
        await model.authorise()
        XCTAssertEqual(model.gate, .authorised("D-4473"))
        XCTAssertNil(model.message)

        // A new option resets the gate; Reject is the operator's own decision.
        model.choose(2)
        XCTAssertEqual(model.gate, .pending)
        model.reject()
        XCTAssertEqual(model.gate, .rejected)

        // Hold opens no workflow: Authorise does nothing.
        model.choose(3)
        await model.authorise()
        XCTAssertEqual(model.gate, .pending)
    }

    func testOfflineTheAuthorisationIsQueued() async throws {
        let app = try await signedIn("k.osei")
        let model = HeimdallPictureModel(app: app)
        await model.load()
        await app.setOffline(true)
        await model.authorise()
        XCTAssertEqual(model.gate, .queued)
        XCTAssertEqual(app.chips?.sync.pending, 4, "the action's four fields wait to be sent")
    }

    func testALowerClearanceSeesWithheldTracksByCountOnly() async throws {
        let model = HeimdallPictureModel(app: try await signedIn("k.rao"))
        await model.load()
        XCTAssertEqual(model.tracks.map(\.id), ["E-0442"])
        XCTAssertEqual(model.withheld, 4)
        XCTAssertEqual(model.selected, "E-0442")
        XCTAssertEqual(model.options, [])
    }

    func testTheViewsRenderInBothModes() async throws {
        MGFonts.registerAll()
        let app = try await signedIn("k.osei")
        let model = HeimdallPictureModel(app: app)
        await model.load()
        for scheme in [ColorScheme.dark, .light] {
            let frame = HeimdallConsoleFrame(nav: { HeimdallLayersList(model: model) },
                                             main: { HeimdallPictureCanvas(model: model) },
                                             aside: { HeimdallInspector(model: model) },
                                             trailing: { EmptyView() })
                .environment(\.colorScheme, scheme)
                .frame(width: 1400, height: 900)
            XCTAssertNotNil(ImageRenderer(content: frame).cgImage)
            let compact = HeimdallOperatingPictureCompact(app: app, personaId: "k.osei")
                .environment(\.colorScheme, scheme)
                .frame(width: 402, height: 874)
            XCTAssertNotNil(ImageRenderer(content: compact).cgImage)
        }
    }
}

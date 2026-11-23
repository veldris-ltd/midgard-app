import CoreText
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

    /// The picture's readouts sit side by side where both fit (the Mac's picture) and
    /// the focus readout starts under the left one where they would overlap (an iPhone's
    /// width): never overlapping.
    func testThePictureReadoutsNeverOverlap() {
        let layout = PictureReadoutsLayout(spacing: 6)
        let left = CGSize(width: 200, height: 60)
        let right = CGSize(width: 180, height: 50)
        let wide = layout.frames(width: 900, sizes: [left, right])
        XCTAssertEqual(wide[1].minY, 0)
        XCTAssertEqual(wide[1].maxX, 900)
        XCTAssertFalse(wide[0].intersects(wide[1]))
        let narrow = layout.frames(width: 380, sizes: [left, right])
        XCTAssertEqual(narrow[1].minY, 66)
        XCTAssertEqual(narrow[1].maxX, 380)
        XCTAssertFalse(narrow[0].intersects(narrow[1]))
        XCTAssertEqual(layout.frames(width: 380, sizes: [left]).count, 1)
    }
    /// The size a one-line mono label draws at, as iOS draws it: JetBrains Mono at
    /// `points`, `tracking` pt between letters, `padding` pt each side.
    private func monoLabel(_ text: String, points: CGFloat, tracking: CGFloat, padding: CGFloat) -> CGSize {
        let descriptor = CTFontDescriptorCreateWithAttributes([kCTFontFamilyNameAttribute: MGFonts.monoFamily] as CFDictionary)
        let font = CTFontCreateWithFontDescriptor(descriptor, points, nil)
        XCTAssertEqual(CTFontCopyFamilyName(font) as String, MGFonts.monoFamily, "measured in another font")
        let string = NSAttributedString(string: text, attributes: [
            NSAttributedString.Key(kCTFontAttributeName as String): font,
            NSAttributedString.Key(kCTKernAttributeName as String): tracking,
        ])
        let line = CTLineCreateWithAttributedString(string)
        let width = CTLineGetTypographicBounds(line, nil, nil, nil)
        let height = CTFontGetAscent(font) + CTFontGetDescent(font) + CTFontGetLeading(font)
        return CGSize(width: ceil(width) + 2 * padding, height: ceil(height))
    }

    /// The iPhone layout (I1): the readouts are a strip above the picture, so none lies
    /// over a track's symbol or label; every track's 44 pt symbol frame and label lie
    /// wholly inside the picture, and no two labels overlap, at an iPhone SE's and an
    /// iPhone 17's width, at the default text size (11 pt) and at XXXL (caption 2 at
    /// 17 pt), at the tallest and the shortest picture the strip leaves. [FR-H-4]
    /// [DD-7] [RC-49] [AC-C-11]
    func testTheCompactPictureKeepsTracksClearOfTheReadoutsAndInside() async throws {
        MGFonts.registerAll()
        let model = HeimdallPictureModel(app: try await signedIn("k.osei"))
        await model.load()
        XCTAssertEqual(model.visibleTracks.count, 5)
        let stack = PictureStripLayout()
        XCTAssertEqual(stack.pictureHeight(strip: 0), 520)
        XCTAssertEqual(stack.pictureHeight(strip: 60), 460)
        XCTAssertEqual(stack.pictureHeight(strip: 200), 440)
        for width in [375.0, 402.0] {
            for points in [11.0, 17.0] {
                for strip in [0.0, 84.0, 160.0] {
                    let size = CGSize(width: width, height: stack.pictureHeight(strip: strip))
                    // The strip's frame, and the picture's frame under it, in the stack.
                    let readouts = CGRect(x: 0, y: 0, width: width, height: strip)
                    var labels: [(String, CGRect)] = []
                    for t in model.visibleTracks {
                        let at = HeimdallPictureGeometry.point(x: t.x, y: t.y, in: size)
                        let text = "\(t.id)\(t.track == "anomalous" ? " ⚠\u{FE0E}" : "") · \(Int(t.hdg))°"
                        let label = HeimdallPictureGeometry.labelFrame(
                            at: at, label: monoLabel(text, points: points, tracking: 0.08 * 11, padding: 2), in: size)
                        let symbol = HeimdallPictureGeometry.symbolFrame(at: at)
                        // The track's button reaches over its symbol and its label.
                        let button = HeimdallPictureGeometry.trackFrame(at: at, label: label.size, in: size)
                        XCTAssertTrue(button.contains(symbol) && button.contains(label), "\(t.id): \(button)")
                        let picture = CGRect(origin: .zero, size: size)
                        let context = "\(t.id) at \(width)×\(size.height), \(points) pt"
                        XCTAssertTrue(picture.contains(symbol), "symbol outside the picture: \(context) \(symbol)")
                        XCTAssertTrue(picture.insetBy(dx: 1, dy: 1).contains(label),
                                      "label clipped by the picture: \(context) \(label)")
                        for (name, frame) in [("symbol", symbol), ("label", label), ("button", button)] {
                            let inStack = frame.offsetBy(dx: 0, dy: strip)
                            XCTAssertFalse(inStack.intersects(readouts),
                                           "\(name) under the readouts: \(context) \(inStack) \(readouts)")
                        }
                        labels.append((t.id, label))
                    }
                    for (i, a) in labels.enumerated() {
                        for b in labels[(i + 1)...] {
                            XCTAssertFalse(a.1.intersects(b.1), "labels \(a.0) and \(b.0) overlap at \(width) pt, \(points) pt")
                        }
                    }
                }
            }
        }
        // A label wider than the room left of the right edge is moved in, not cut.
        let pinned = HeimdallPictureGeometry.pinned(CGPoint(x: 360, y: 100), size: CGSize(width: 80, height: 16),
                                                    centredVertically: true, in: CGSize(width: 375, height: 440))
        XCTAssertEqual(pinned.maxX, 373)
        XCTAssertEqual(pinned.midY, 100)
    }
}

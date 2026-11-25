import MidgardKit
import SwiftUI
import XCTest

@testable import MidgardGlobeKit

/// The 3D editions' library (Task B7): linked in, it registers its edition provider at
/// link time (its strong `midgard_edition_hook` replaces MidgardHooks' weak default), with
/// no call at launch and no runtime switch. The provider gives the 3D and maps editions
/// About and Heimdall's MAP and 3D views (Phase I), and the release edition nothing (no
/// edition banner since 25 Nov 2026). [FR-C-1] [FR-H-1] [AC-C-13]
@MainActor
final class GlobeRegistrationTests: XCTestCase {
    func testLinkingTheLibraryRegistersItsProvider() throws {
        let registered = try XCTUnwrap(EditionHook.registered, "linked, the library is registered")
        XCTAssertTrue(registered === EditionUIProvider.shared)
    }

    func testTheReleaseEditionGetsNothing() {
        let provider = EditionUIProvider.shared
        XCTAssertNil(provider.about(for: .release))
        XCTAssertNil(provider.pictureViews(for: .release))
        XCTAssertNil(EditionBanner(.release))
        XCTAssertNil(EditionAbout(.release))
    }

    func testThe3DEditionsGetTheirAboutAndPictureViews() throws {
        let provider = EditionUIProvider.shared
        for edition in [AppEdition.threeD, .maps] {
            XCTAssertNotNil(provider.about(for: edition))
            XCTAssertTrue(provider.pictureViews(for: edition) === GlobeRegistration.shared)
        }
        // The web's switch: 2D (the app's own), then MAP and 3D.
        XCTAssertEqual(GlobeRegistration.shared.views.map(\.id), ["map", "3d"])
        XCTAssertEqual(GlobeRegistration.shared.views.map(\.label), ["MAP", "3D"])
        XCTAssertEqual(HeimdallPictureView.reference, HeimdallPictureView(id: "2d", label: "2D"))
    }

    func testAboutNamesTheEditionAndNeverCallsItADemo() throws {
        let about = try XCTUnwrap(EditionAbout(.threeD))
        XCTAssertEqual(about.name, "Midgard 3D")
        XCTAssertEqual(about.editionLine, "Edition: Midgard 3D")
        let maps = try XCTUnwrap(EditionAbout(.maps))
        XCTAssertEqual(maps.name, "Midgard Maps")
        XCTAssertEqual(maps.editionLine, "Edition: Midgard Maps")
        XCTAssertEqual(maps.releaseLine,
                       "Operations consoles with map and 3D views in Heimdall. The app starts on a built-in sample "
                           + "workspace; connect your own data server or import JSON files from Your Data.")
        for word in ["demo", "trial", "beta", "signed release", "rc."] {
            XCTAssertFalse(maps.releaseLine.lowercased().contains(word), word)
        }
    }

    func testTheViewsRenderIncludingAtTheLargestAccessibilitySize() throws {
        MGFonts.registerAll()
        XCTAssertNotNil(ImageRenderer(content: AboutView(try XCTUnwrap(EditionAbout(.maps))).frame(width: 390, height: 600)).cgImage)
        for size in [DynamicTypeSize.large, .accessibility5] {
            let view = AboutView(try XCTUnwrap(EditionAbout(.maps))).environment(\.dynamicTypeSize, size).frame(width: 390)
            XCTAssertNotNil(ImageRenderer(content: view).cgImage, "About at \(size)")
        }
        XCTAssertNotNil(ImageRenderer(content: HeimdallViewSwitch(views: GlobeRegistration.shared.views,
                                                                  selection: .constant("3d"))).cgImage)
        XCTAssertNotNil(GlobeOutlines.shared(), "the bundled Natural Earth outlines load")
    }
}

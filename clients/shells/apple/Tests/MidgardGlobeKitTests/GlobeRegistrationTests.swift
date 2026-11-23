import MidgardKit
import SwiftUI
import XCTest

@testable import MidgardGlobeKit

/// The 3D editions' library (Task B7): linked in, it registers its edition provider at
/// link time (its strong `midgard_edition_hook` replaces MidgardHooks' weak default), with
/// no call at launch and no runtime switch. The provider gives the 3D and maps editions
/// their banner, About and Heimdall's MAP and 3D views (Phase I), and the release edition
/// nothing. [FR-C-1] [FR-H-1] [AC-C-13]
@MainActor
final class GlobeRegistrationTests: XCTestCase {
    func testLinkingTheLibraryRegistersItsProvider() throws {
        let registered = try XCTUnwrap(EditionHook.registered, "linked, the library is registered")
        XCTAssertTrue(registered === EditionUIProvider.shared)
    }

    func testTheReleaseEditionGetsNothing() {
        let provider = EditionUIProvider.shared
        XCTAssertNil(provider.banner(for: .release))
        XCTAssertNil(provider.about(for: .release))
        XCTAssertNil(provider.pictureViews(for: .release))
        XCTAssertNil(EditionBanner(.release))
        XCTAssertNil(EditionAbout(.release))
    }

    func testThe3DEditionsGetTheirBannerAboutAndPictureViews() throws {
        let provider = EditionUIProvider.shared
        for edition in [AppEdition.threeD, .maps] {
            XCTAssertNotNil(provider.banner(for: edition))
            XCTAssertNotNil(provider.about(for: edition))
            XCTAssertTrue(provider.pictureViews(for: edition) === GlobeRegistration.shared)
        }
        // The web's switch: 2D (the app's own), then MAP and 3D.
        XCTAssertEqual(GlobeRegistration.shared.views.map(\.id), ["map", "3d"])
        XCTAssertEqual(GlobeRegistration.shared.views.map(\.label), ["MAP", "3D"])
        XCTAssertEqual(HeimdallPictureView.reference, HeimdallPictureView(id: "2d", label: "2D"))
    }

    func testBannerTexts() {
        XCTAssertEqual(EditionBanner.threeD.text, "3D edition, not in the signed release")
        XCTAssertEqual(EditionBanner.maps.text, "Maps edition, not in the signed release")
        XCTAssertEqual(EditionBanner.threeD.compactText, "3D edition · not the signed release")
        XCTAssertEqual(EditionBanner.maps.compactText, "Maps edition · not the signed release")
        XCTAssertEqual(EditionBanner.threeD.identifier, "mg.banner.edition3d")
        XCTAssertEqual(EditionBanner.maps.identifier, "mg.banner.editionmaps")
    }

    func testTheCompactTextsAreShort() {
        for banner in [EditionBanner.threeD, .maps] {
            XCTAssertLessThan(banner.compactText.count, banner.text.count)
            XCTAssertLessThanOrEqual(banner.compactText.count, 37, banner.compactText)
        }
    }

    func testAboutNamesTheEditionAndNeverCallsItTheSignedRelease() throws {
        XCTAssertEqual(SignedRelease.tag, "v0.1.0-rc.3")
        XCTAssertEqual(SignedRelease.commit, "ea5899ab")
        let about = try XCTUnwrap(EditionAbout(.threeD))
        XCTAssertEqual(about.name, "Midgard 3D")
        XCTAssertEqual(about.editionLine, "Edition: Midgard 3D")
        XCTAssertEqual(about.releaseLine,
                       "3D edition, not in the signed release. It is v0.1.0-rc.3 (ea5899ab) with map and 3D views added to Heimdall.")
        let maps = try XCTUnwrap(EditionAbout(.maps))
        XCTAssertEqual(maps.name, "Midgard Maps")
        XCTAssertEqual(maps.editionLine, "Edition: Midgard Maps")
        XCTAssertTrue(maps.releaseLine.hasPrefix("Maps edition, not in the signed release. It is v0.1.0-rc.3"))
    }

    func testTheViewsRenderIncludingAtTheLargestAccessibilitySize() throws {
        MGFonts.registerAll()
        XCTAssertNotNil(ImageRenderer(content: AboutView(try XCTUnwrap(EditionAbout(.maps))).frame(width: 390, height: 600)).cgImage)
        for banner in [EditionBanner.threeD, .maps] {
            for size in [DynamicTypeSize.large, .accessibility5] {
                let view = EditionBannerView(banner).environment(\.dynamicTypeSize, size).frame(width: 390)
                XCTAssertNotNil(ImageRenderer(content: view).cgImage, "\(banner) at \(size)")
            }
        }
        XCTAssertNotNil(ImageRenderer(content: HeimdallViewSwitch(views: GlobeRegistration.shared.views,
                                                                  selection: .constant("3d"))).cgImage)
        XCTAssertNotNil(GlobeOutlines.shared(), "the bundled Natural Earth outlines load")
    }
}

import MidgardKit
import SwiftUI
import XCTest

@testable import MidgardGlobeKit

/// The 3D editions' library (Task B7): linked in, it registers its edition provider at
/// link time (its strong `midgard_edition_hook` replaces MidgardHooks' weak default), with
/// no call at launch and no runtime switch. The provider gives the 3D and maps editions
/// their banner, About and Heimdall's 3D view, and the release edition nothing. Until
/// Phase I the 3D view is a placeholder that says "3D view". [FR-C-1] [AC-C-13]
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
        XCTAssertNil(provider.threeD(for: .release))
        XCTAssertNil(EditionBanner(.release))
        XCTAssertNil(EditionAbout(.release))
    }

    func testThe3DEditionsGetTheirBannerAboutAnd3DView() throws {
        let provider = EditionUIProvider.shared
        for edition in [AppEdition.threeD, .maps] {
            XCTAssertNotNil(provider.banner(for: edition))
            XCTAssertNotNil(provider.about(for: edition))
            XCTAssertTrue(provider.threeD(for: edition) === GlobeRegistration.shared)
        }
        XCTAssertEqual(GlobeRegistration.shared.title, "3D view")
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
                       "3D edition, not in the signed release. It is v0.1.0-rc.3 (ea5899ab) with a 3D view added to Heimdall.")
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
        XCTAssertNotNil(ImageRenderer(content: GlobeRegistration.shared.makeView().frame(width: 390, height: 600)).cgImage)
        XCTAssertNotNil(ImageRenderer(content: GlobeRegistration.shared.makeEntry(showing: .constant(false))
            .frame(width: 390)).cgImage)
        XCTAssertEqual(GlobePlaceholderView.text, "3D view")
        XCTAssertEqual(GlobePlaceholderView.identifier, "mg.heimdall.3d.view")
    }
}

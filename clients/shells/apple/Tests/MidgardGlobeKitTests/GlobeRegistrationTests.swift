import MidgardKit
import SwiftUI
import XCTest

@testable import MidgardGlobeKit

/// The 3D edition's library (Task B7): linked in, it registers itself with Heimdall at
/// link time (its strong `midgard_heimdall_3d_hook` replaces MidgardHooks' weak
/// default), with no call at launch and no runtime switch. Until Phase I its view is a
/// placeholder that says "3D view". [FR-C-1]
@MainActor
final class GlobeRegistrationTests: XCTestCase {
    func testLinkingTheLibraryRegistersIt() throws {
        let registered = try XCTUnwrap(HeimdallThreeD.registered, "linked, the library is registered")
        XCTAssertTrue(registered === GlobeRegistration.shared)
        XCTAssertEqual(registered.title, "3D view")
    }

    func testThe3DEditionsGetTheEntryAndTheReleaseEditionDoesNot() {
        XCTAssertNotNil(HeimdallThreeD.entry(edition: .threeD, registered: HeimdallThreeD.registered))
        XCTAssertNotNil(HeimdallThreeD.entry(edition: .maps, registered: HeimdallThreeD.registered))
        XCTAssertNil(HeimdallThreeD.entry(edition: .release, registered: HeimdallThreeD.registered))
    }

    func testThePlaceholderSaysWhatItIs() {
        XCTAssertEqual(GlobePlaceholderView.text, "3D view")
        XCTAssertEqual(GlobePlaceholderView.identifier, "mg.heimdall.3d.view")
        let renderer = ImageRenderer(content: GlobeRegistration.shared.makeView().frame(width: 390, height: 600))
        XCTAssertNotNil(renderer.cgImage)
    }
}

import CoreText
import SwiftUI
import XCTest

@testable import MidgardKit

/// The generated tokens are the design system's values, and the bundled fonts are the
/// families the views ask for. [DD-7]
final class TokensTests: XCTestCase {
    func testMarkingColoursAreFixed() {
        XCTAssertEqual(MGToken.markingConfidential, 0xCA1D25)
        XCTAssertEqual(MGToken.markingRestricted, 0xF0A824)
        XCTAssertEqual(MGToken.markingInternal, 0x3D8BFF)
        XCTAssertEqual(MGToken.markingOpen, 0x21C07A)
        XCTAssertEqual(MGToken.markingHex(.confidential), MGToken.markingConfidential)
        XCTAssertEqual(MGToken.markingHex(.open), MGToken.markingOpen)
    }

    func testStatusColoursAliasesAndSpacing() {
        XCTAssertEqual(MGToken.secure, 0x21C07A)
        XCTAssertEqual(MGToken.warn, 0xF0A824)
        XCTAssertEqual(MGToken.danger, 0xCA1D25)
        XCTAssertEqual(MGToken.info, 0x3D8BFF)
        XCTAssertEqual(MGToken.bgPage, MGToken.ink900, "aliases are resolved")
        XCTAssertEqual(MGSpace.s4, 16)
        XCTAssertEqual(MGSpace.s2, 8)
        XCTAssertEqual(MGRadius.md, 10)
        XCTAssertEqual(MGAccent.heimdall.hex, MGToken.heimdall500)
    }

    func testFontsAreBundledAndRegister() {
        XCTAssertTrue(MGFonts.registerAll(), "Oxanium, Space Grotesk and JetBrains Mono must register")
        XCTAssertTrue(MGFonts.registerAll(), "a second registration is not an error")
    }

    func testTheFamilyNamesTheViewsUseResolveToTheBundledFonts() {
        MGFonts.registerAll()
        for family in [MGFonts.displayFamily, MGFonts.sansFamily, MGFonts.monoFamily] {
            let font = CTFontCreateWithName(family as CFString, 14, nil)
            XCTAssertEqual(CTFontCopyFamilyName(font) as String, family, "\(family) falls back to the system font")
        }
    }
}

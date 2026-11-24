import CoreText
import SwiftUI

/// The house type (Oxanium display, Space Grotesk UI, JetBrains Mono labels), bundled
/// (OFL-1.1) and registered for the process; sizes follow Dynamic Type through
/// `relativeTo:`. The family names are the ones the font files declare. [DD-7]
public enum MGFonts {
    public static let displayFamily = "Oxanium"
    public static let sansFamily = "Space Grotesk"
    public static let monoFamily = "JetBrains Mono"
    static let files = ["Oxanium", "SpaceGrotesk", "JetBrainsMono"]
    /// `kCTFontManagerErrorAlreadyRegistered`.
    static let alreadyRegistered = 105

    @discardableResult
    public static func registerAll() -> Bool {
        files.allSatisfy { name in
            guard let url = Bundle.module.url(forResource: name, withExtension: "ttf", subdirectory: "Fonts") else {
                return false
            }
            var error: Unmanaged<CFError>?
            if CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) { return true }
            guard let error = error?.takeRetainedValue() else { return false }
            return CFErrorGetCode(error) == alreadyRegistered
        }
    }

    public static func display(_ style: Font.TextStyle) -> Font { .custom(displayFamily, size: size(style), relativeTo: scaling(style)) }
    public static func sans(_ style: Font.TextStyle) -> Font { .custom(sansFamily, size: size(style), relativeTo: scaling(style)) }
    public static func mono(_ style: Font.TextStyle) -> Font { .custom(monoFamily, size: size(style), relativeTo: scaling(style)) }

    /// The text style a size scales with. On iOS caption2 scales as caption: the iOS
    /// Dynamic Type audit reports text relative to caption2 as "partially unsupported"
    /// (the sign-in screen's persona rows, the web's small mono labels). The base size is
    /// the style's own; only the scaling changes.
    public static func scaling(_ style: Font.TextStyle) -> Font.TextStyle {
        #if os(iOS)
        return style == .caption2 ? .caption : style
        #else
        return style
        #endif
    }

    /// Base sizes from typography.css (console default body 14, chips 12, eyebrow 11).
    static func size(_ style: Font.TextStyle) -> CGFloat {
        switch style {
        case .largeTitle: return 32
        case .title: return 24
        case .title2: return 20
        case .headline, .body: return 14
        case .callout, .subheadline, .footnote: return 13
        case .caption: return 12
        default: return 11
        }
    }
}

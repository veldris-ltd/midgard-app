import SwiftUI

/// The web consoles' semantic colours (`ui/design-system/src/tokens/colors.css`), dark
/// and light (`.mode-light`), for the screens matched to the web suite (owner decision
/// 22 Nov 2026). Text colours are the web's except where the web's fails WCAG AA on its
/// own surfaces: `faint` text is `--text-muted` in both modes, and accent and warning
/// text in light mode use the darker amber the tone-text tokens derive. [DD-7] [RC-49]
/// [AC-C-11]
public enum MGWeb {
    public static let bgPage = Color(mgLight: MGToken.Light.bgPage, dark: (MGToken.bgPage, 1))
    public static let bgSunken = Color(mgLight: MGToken.Light.bgSunken, dark: (MGToken.bgSunken, 1))
    public static let surfaceCard = Color(mgLight: MGToken.Light.surfaceCard, dark: (MGToken.surfaceCard, 1))
    public static let surfaceRaised = Color(mgLight: MGToken.Light.surfaceRaised, dark: (MGToken.surfaceRaised, 1))
    public static let surfaceInset = Color(mgLight: MGToken.Light.surfaceInset, dark: (MGToken.surfaceInset, 1))
    public static let border = Color(mgLight: MGToken.Light.border, dark: (MGToken.border, 1))
    public static let borderStrong = Color(mgLight: MGToken.Light.borderStrong, dark: (MGToken.borderStrong, 1))
    public static let divider = Color(mgLight: MGToken.Light.divider, dark: MGToken.divider)
    public static let gridLine = Color(mgLight: MGToken.Light.gridLine, dark: (0xFFFFFF, 0.035))

    public static let textHeading = Color(mgLight: MGToken.Light.textHeading, dark: (MGToken.textHeading, 1))
    /// `--grey-100`: option titles.
    public static let grey100 = Color(mgLight: MGToken.Light.grey100, dark: (MGToken.grey100, 1))
    public static let textBody = Color(mgLight: MGToken.Light.textBody, dark: (MGToken.textBody, 1))
    public static let textMuted = Color(mgLight: MGToken.Light.textMuted, dark: (MGToken.textMuted, 1))
    /// Where the web writes `--text-faint`: `--text-muted`, the nearest token that the
    /// on-device contrast audit passes at the web's small mono sizes (a mid grey at
    /// 5:1 still failed it at 8–11 pt).
    public static let textFaint = Color(mgLight: MGToken.Light.textMuted, dark: (MGToken.textMuted, 1))
    /// Decoration only (rules, ticks, rings): the web's own faint grey.
    public static let decorFaint = Color(mgLight: MGToken.Light.textFaint, dark: (MGToken.textFaint, 1))

    public static let secure = Color(mgHex: MGToken.secure)
    public static let warn = Color(mgHex: MGToken.warn)
    public static let warnTint = Color(mgHex: MGToken.warnTint.0, opacity: MGToken.warnTint.1)
    /// Warning-coloured text: the warn tone, or its tone-text shade on paper.
    public static let warnText = Color(mgLight: (MGToken.ToneText.warnLight, 1), dark: (MGToken.warn, 1))
    public static let secureText = Color(mgLight: (MGToken.ToneText.secureLight, 1), dark: (MGToken.secure, 1))
    public static let infoText = Color(mgLight: (MGToken.ToneText.infoLight, 1), dark: (MGToken.ToneText.infoDark, 1))
}

/// One product's accent scope (`.theme-<product>`). [DD-7]
public struct MGWebAccent {
    /// `--accent`: fills, the selected rule, track symbols.
    public let fill: Color
    /// `--accent-hover`: accent text (and its darker shade on paper, for AA).
    public let text: Color
    /// The base accent's own hex, for the ink on an accent fill.
    public let hex: UInt32
    public let tint: Color
    public let glow: Color

    public static let heimdall = MGWebAccent(
        fill: Color(mgHex: MGToken.heimdall500),
        text: Color(mgLight: (MGToken.ToneText.warnLight, 1), dark: (MGToken.heimdall300, 1)),
        hex: MGToken.heimdall500,
        tint: Color(mgHex: MGToken.heimdallTint.0, opacity: MGToken.heimdallTint.1),
        glow: Color(mgHex: MGToken.heimdallGlow.0, opacity: MGToken.heimdallGlow.1))

    /// `--accent` used as text (section numerals): the base accent in dark mode.
    public var baseText: Color { Color(mgLight: (MGToken.ToneText.warnLight, 1), dark: (hex, 1)) }
}

/// The web's type at its pixel sizes (`typography.css`): on macOS a CSS pixel is a point;
/// on iOS each size scales with its text style (Dynamic Type). [DD-7]
public enum MGWebFont {
    public static func display(_ size: CGFloat, _ style: Font.TextStyle = .body) -> Font {
        .custom(MGFonts.displayFamily, size: size, relativeTo: style)
    }

    public static func sans(_ size: CGFloat, _ style: Font.TextStyle = .body) -> Font {
        .custom(MGFonts.sansFamily, size: size, relativeTo: style)
    }

    public static func mono(_ size: CGFloat, _ style: Font.TextStyle = .caption2) -> Font {
        .custom(MGFonts.monoFamily, size: size, relativeTo: style)
    }
}

extension View {
    /// Letter-spacing in ems, as the web writes it (`letter-spacing: 0.14em`).
    func mgTracking(_ em: CGFloat, size: CGFloat) -> some View {
        tracking(em * size)
    }
}

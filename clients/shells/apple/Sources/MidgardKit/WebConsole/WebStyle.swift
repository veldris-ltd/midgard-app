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
    /// Danger-coloured text (`--midgard-300` on its tint in dark mode; its tone-text
    /// shade on paper).
    public static let dangerText = Color(mgLight: (MGToken.ToneText.dangerLight, 1), dark: (MGToken.ToneText.dangerDark, 1))
    public static let danger = Color(mgHex: MGToken.danger)
    public static let secureTint = Color(mgHex: MGToken.secureTint.0, opacity: MGToken.secureTint.1)
    public static let dangerTint = Color(mgHex: MGToken.dangerTint.0, opacity: MGToken.dangerTint.1)
    public static let infoTint = Color(mgHex: MGToken.infoTint.0, opacity: MGToken.infoTint.1)
    /// `--grey-300`: badge, pill and terminal text.
    public static let grey300 = Color(mgLight: MGToken.Light.grey300, dark: (MGToken.grey300, 1))

    /// Whether the picture's smallest readouts (scale, ring and focus labels) are
    /// decoration, spoken through the picture's own label. On the Mac they stay at the
    /// web's 7–9 pt, below what the contrast audit reads; on the iPhone they are 11 pt
    /// and readable, and the audit expects visible text to be accessible.
    public static let readoutsDecorative: Bool = {
        #if os(macOS)
        return true
        #else
        return false
        #endif
    }()
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
    /// The numeral colour on paper: the darker shade that reads AA there.
    var baseTextLight: UInt32 = MGToken.ToneText.warnLight
    /// The numeral colour in dark mode, when the base accent does not read AA there.
    var baseTextDark: UInt32?

    public static let heimdall = MGWebAccent(
        fill: Color(mgHex: MGToken.heimdall500),
        text: Color(mgLight: (MGToken.ToneText.warnLight, 1), dark: (MGToken.heimdall300, 1)),
        hex: MGToken.heimdall500,
        tint: Color(mgHex: MGToken.heimdallTint.0, opacity: MGToken.heimdallTint.1),
        glow: Color(mgHex: MGToken.heimdallGlow.0, opacity: MGToken.heimdallGlow.1))

    /// Forge's scope (`.theme-forge`). Accent text is `--accent-hover`: `--forge-300` in
    /// dark mode; on paper the web's `--forge-600` reads 4.3:1 on the page, under AA, so
    /// a darker shade of it (0x8A400C, 6:1 on the page, 5.6:1 on the accent tint) is used
    /// for text there, as Heimdall's is.
    public static let forge = MGWebAccent(
        fill: Color(mgHex: MGToken.forge500),
        text: Color(mgLight: (forgeTextLight, 1), dark: (MGToken.forge300, 1)),
        hex: MGToken.forge500,
        tint: Color(mgHex: MGToken.forgeTint.0, opacity: MGToken.forgeTint.1),
        glow: Color(mgHex: MGToken.forgeGlow.0, opacity: MGToken.forgeGlow.1),
        baseTextLight: forgeTextLight)

    /// Forge's accent text on paper.
    static let forgeTextLight: UInt32 = 0x8A400C

    /// Mimir's scope (`.theme-mimir`). Accent text is `--accent-hover`: `--mimir-300` in
    /// dark mode; on paper the web's `--mimir-600` reads 4.0:1 on the page (3.7:1 on the
    /// accent tint), under AA, so a darker shade of it (0x12666F, 5.8:1 on the page, 5.2:1
    /// on the tint) is used for text there, as Forge's is.
    public static let mimir = MGWebAccent(
        fill: Color(mgHex: MGToken.mimir500),
        text: Color(mgLight: (mimirTextLight, 1), dark: (MGToken.mimir300, 1)),
        hex: MGToken.mimir500,
        tint: Color(mgHex: MGToken.mimirTint.0, opacity: MGToken.mimirTint.1),
        glow: Color(mgHex: MGToken.mimirGlow.0, opacity: MGToken.mimirGlow.1),
        baseTextLight: mimirTextLight)

    /// Mimir's accent text on paper.
    static let mimirTextLight: UInt32 = 0x12666F

    /// Midgard's scope (`.theme-midgard`), also Admin's. Accent text is `--accent-hover`
    /// (`--midgard-300` in dark mode, `--midgard-600` on paper).
    public static let midgard = MGWebAccent(
        fill: Color(mgHex: MGToken.midgard500),
        text: Color(mgLight: MGToken.Light.accentHover, dark: (MGToken.midgard300, 1)),
        hex: MGToken.midgard500,
        tint: Color(mgHex: MGToken.midgardTint.0, opacity: MGToken.midgardTint.1),
        glow: Color(mgHex: MGToken.midgardGlow.0, opacity: MGToken.midgardGlow.1),
        baseTextLight: MGToken.Light.accentHover.0,
        // The base red is 3.6:1 on the dark page, under AA for the numerals' 11 pt:
        // lifted to `--midgard-300`, as the web's accent text is.
        baseTextDark: MGToken.midgard300)

    /// `--accent` used as text (section numerals): the base accent in dark mode.
    public var baseText: Color { Color(mgLight: (baseTextLight, 1), dark: (baseTextDark ?? hex, 1)) }
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

    /// On iOS no smaller than 11 pt at the default text size: the web's 8–9 px mono labels
    /// are below what the iPhone's contrast audit reads.
    public static func mono(_ size: CGFloat, _ style: Font.TextStyle = .caption2) -> Font {
        .custom(MGFonts.monoFamily, size: monoSize(size), relativeTo: style)
    }

    /// The size `mono(_:)` draws the web's `size` at (before Dynamic Type): the web's
    /// own on the Mac, the 11 pt floor on iOS. Letter-spacing in ems is taken of this,
    /// not of the web's size.
    public static func monoSize(_ size: CGFloat) -> CGFloat {
        #if os(iOS)
        return max(size, 11)
        #else
        return size
        #endif
    }
}

extension View {
    /// Letter-spacing in ems, as the web writes it (`letter-spacing: 0.14em`).
    func mgTracking(_ em: CGFloat, size: CGFloat) -> some View {
        tracking(em * size)
    }
}

import SwiftUI

/// Colours the shared screens set over the system's, from the design tokens, so their
/// contrast does not depend on an app's accent colour or on the system's secondary
/// grey (which failed AA on the sign-in screen). The text colours are drawn on the
/// window's own background, which the host chooses: each meets AA on every background
/// the apps use, in its mode — the system backgrounds of iOS (white, black) and macOS
/// (window background, light and dark) and the theme's surfaces — checked by
/// `AccessibilityPrecheckTests.testStyleTextMeetsAAOnTheSystemBackgrounds`. [RC-49]
/// [AC-C-11] [DD-7]
public enum MGStyle {
    /// Buttons and links: the accent's pressed shade in light mode, its hover shade in
    /// dark mode (the base accent red is below 4.5:1 on black).
    public static let accentTokens: (light: UInt32, dark: UInt32) = (MGToken.accentPress, MGToken.accentHover)
    /// Primary text: the heading token of each mode.
    public static let textTokens: (light: UInt32, dark: UInt32) = (MGToken.Light.textHeading.0, MGToken.textHeading)
    /// Secondary text (captions, roles): the body token of each mode, not the muted one,
    /// which is 4.4:1 on the macOS dark window background.
    public static let secondaryTextTokens: (light: UInt32, dark: UInt32) = (MGToken.Light.textBody.0, MGToken.textBody)

    public static let accent = color(accentTokens)
    public static let text = color(textTokens)
    public static let secondaryText = color(secondaryTextTokens)

    static func color(_ tokens: (light: UInt32, dark: UInt32)) -> Color {
        Color(mgLight: (tokens.light, 1), dark: (tokens.dark, 1))
    }
}

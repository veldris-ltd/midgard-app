import SwiftUI

/// A signature status. The check-mark symbol appears only when the badge says so
/// (`verified` or `device`, both checked by the core). Outlined in its tone over the
/// tone's tint, lettered in the tone's text token for the mode. [AC-C-13] [RC-49]
public struct SignatureBadgeView: View {
    let badge: SignatureBadge
    public init(_ badge: SignatureBadge) { self.badge = badge }

    public var body: some View {
        HStack(spacing: MGSpace.s1) {
            if badge.showsCheck { Image(systemName: "checkmark.seal.fill").accessibilityHidden(true) }
            Text(badge.text).font(MGFonts.mono(.caption2))
        }
        .padding(.horizontal, MGSpace.s2)
        .padding(.vertical, 2)
        .foregroundStyle(Color(mgLight: (badge.tone.text.light, 1), dark: (badge.tone.text.dark, 1)))
        .background(Capsule().fill(Color(mgHex: badge.tone.tint.0, opacity: badge.tone.tint.1)))
        .overlay(Capsule().strokeBorder(Color(mgHex: badge.tone.hex), lineWidth: 1))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(badge.accessibilityLabel))
        .accessibilityAddTraits(.isStaticText)
        .accessibilityIdentifier("mg.signature")
    }
}

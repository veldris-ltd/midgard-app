import SwiftUI

/// A signature status. The check-mark symbol appears only when the badge says so
/// (`verified` or `device`, both checked by the core). Filled with its tone, lettered in
/// the ink that meets AA on it. [AC-C-13] [RC-49]
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
        .foregroundStyle(Color(mgHex: MGContrast.ink(on: badge.tone.hex)))
        .background(Capsule().fill(Color(mgHex: badge.tone.hex)))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(badge.accessibilityLabel))
        .accessibilityIdentifier("mg.signature")
    }
}

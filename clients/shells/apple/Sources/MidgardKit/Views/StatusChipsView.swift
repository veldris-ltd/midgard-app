import SwiftUI

/// The header chips — sync, AI tier, key custody. Each chip is outlined in its tone over
/// the tone's tint, lettered in the tone's text token for the mode, which meets AA on the
/// tint over every surface. [FR-C-9] [AC-C-13] [RC-49]
///
/// Accessibility: each chip is one text element (static text, like the source banner),
/// not a combined element, so its visible words stay its value ("OFFLINE · 0 QUEUED",
/// what macOS reports and the UI tests read) while VoiceOver speaks the spelled-out
/// label ("Sync: offline, 0 changes queued"). The chips are not controls: nothing on
/// them can be activated, so they have no button trait and need no hit target. (The
/// session's idle touch is watched at the window, `IdleTouchWatcher`, not by a tap
/// gesture over the screen, which made the hit-region audit treat them as tappable.)
///
/// Dynamic Type: one row while it fits; at accessibility text sizes, or wherever the row
/// is too wide, one chip under the other, each free to wrap.
public struct StatusChipsView: View {
    let chips: StatusChipsPresentation
    @Environment(\.dynamicTypeSize) private var typeSize
    public init(_ chips: StatusChipsPresentation) { self.chips = chips }

    public var body: some View {
        if typeSize.isAccessibilitySize {
            column
        } else {
            ViewThatFits(in: .horizontal) {
                HStack(spacing: MGSpace.s2) { all }
                column
            }
        }
    }

    private var column: some View {
        VStack(alignment: .leading, spacing: MGSpace.s1) { all }
            .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private var all: some View {
        chip(chips.sync, id: "mg.chip.sync")
        chip(chips.tier, id: "mg.chip.tier")
        chip(chips.custody, id: "mg.chip.custody")
    }

    private func chip(_ chip: ChipText, id: String) -> some View {
        Text(chip.text)
            .font(MGFonts.mono(.caption2))
            .multilineTextAlignment(.leading)
            .padding(.horizontal, MGSpace.s2)
            .padding(.vertical, 2)
            .foregroundStyle(Color(mgLight: (chip.tone.text.light, 1), dark: (chip.tone.text.dark, 1)))
            // A rounded rectangle rather than a capsule: a wrapped chip keeps its corners.
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(Color(mgHex: chip.tone.tint.0, opacity: chip.tone.tint.1)))
            .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous)
                .strokeBorder(Color(mgHex: chip.tone.hex), lineWidth: 1))
            .accessibilityLabel(Text(chip.accessibilityLabel))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier(id)
    }
}

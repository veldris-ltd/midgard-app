import SwiftUI

/// The header chips — sync, AI tier, key custody — each spelled out for VoiceOver. Each
/// chip is outlined in its tone over the tone's tint, lettered in the tone's text token
/// for the mode, which meets AA on the tint over every surface. [FR-C-9] [AC-C-13] [RC-49]
public struct StatusChipsView: View {
    let chips: StatusChipsPresentation
    public init(_ chips: StatusChipsPresentation) { self.chips = chips }

    public var body: some View {
        HStack(spacing: MGSpace.s2) {
            chip(chips.sync, id: "mg.chip.sync")
            chip(chips.tier, id: "mg.chip.tier")
            chip(chips.custody, id: "mg.chip.custody")
        }
    }

    private func chip(_ chip: ChipText, id: String) -> some View {
        Text(chip.text)
            .font(MGFonts.mono(.caption2))
            .padding(.horizontal, MGSpace.s2)
            .padding(.vertical, 2)
            .foregroundStyle(Color(mgLight: (chip.tone.text.light, 1), dark: (chip.tone.text.dark, 1)))
            .background(Capsule().fill(Color(mgHex: chip.tone.tint.0, opacity: chip.tone.tint.1)))
            .overlay(Capsule().strokeBorder(Color(mgHex: chip.tone.hex), lineWidth: 1))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(chip.accessibilityLabel))
            .accessibilityIdentifier(id)
    }
}

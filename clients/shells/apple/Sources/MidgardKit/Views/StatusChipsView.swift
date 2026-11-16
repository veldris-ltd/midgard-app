import SwiftUI

/// The header chips — sync, AI tier, key custody — each spelled out for VoiceOver.
/// [FR-C-9] [AC-C-13]
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
            .foregroundStyle(Color(mgHex: chip.tone.hex))
            .background(Capsule().strokeBorder(Color(mgHex: chip.tone.hex), lineWidth: 1))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(chip.accessibilityLabel))
            .accessibilityIdentifier(id)
    }
}

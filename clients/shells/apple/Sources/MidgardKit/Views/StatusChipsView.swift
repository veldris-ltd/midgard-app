import SwiftUI

/// The header chips — the data chip (which data the consoles show: `DataSourceChip`),
/// then sync, AI tier, key custody. Each chip is outlined in its tone over
/// the tone's tint, lettered in the tone's text token for the mode, which meets AA on the
/// tint over every surface. [FR-C-9] [AC-C-13] [RC-49]
///
/// Accessibility: each chip is one static-text element whose label is spelled out for
/// VoiceOver ("Sync: offline, 0 changes queued"). On macOS it also carries its visible
/// words as its value ("OFFLINE · 0 QUEUED"): there a label set on a text replaces the
/// text's value, and the visible words were then nowhere in the accessibility tree
/// (seen in the B2 UI tests). On iOS the label alone is spoken, as before. A combined
/// element (`children: .ignore`) keeps the label and value apart; it needs the
/// static-text trait for a role. The chips are not controls: nothing on
/// them can be activated, so they have no button trait and need no hit target. (The
/// session's idle touch is watched at the window, `IdleTouchWatcher`, not by a tap
/// gesture over the screen, which made the hit-region audit treat them as tappable.)
///
/// Dynamic Type: one row while it fits; at accessibility text sizes, or wherever the row
/// is too wide, one chip under the other, each free to wrap.
public struct StatusChipsView: View {
    let chips: StatusChipsPresentation
    let source: ChipText?
    @Environment(\.dynamicTypeSize) private var typeSize
    public init(_ chips: StatusChipsPresentation, source: ChipText? = nil) {
        self.chips = chips
        self.source = source
    }

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
        if let source { chip(source, id: "mg.chip.source") }
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
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(chip.accessibilityLabel))
            .accessibilityAddTraits(.isStaticText)
            .modifier(VisibleWordsOnMac(text: chip.text))
            .accessibilityIdentifier(id)
    }
}

/// On macOS, a chip's visible words as its accessibility value (see `StatusChipsView`).
struct VisibleWordsOnMac: ViewModifier {
    let text: String

    func body(content: Content) -> some View {
        #if os(macOS)
        content.accessibilityValue(Text(text))
        #else
        content
        #endif
    }
}

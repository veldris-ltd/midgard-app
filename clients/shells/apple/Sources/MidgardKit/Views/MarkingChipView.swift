import MidgardCore
import SwiftUI

/// A marking chip: the fixed governance colour of its level (never themed). [AC-C-13] [DD-7]
public struct MarkingChipView: View {
    let chip: MarkingChip
    public init(_ chip: MarkingChip) { self.chip = chip }

    public var body: some View {
        Text(chip.label)
            .font(MGFonts.mono(.caption2))
            .padding(.horizontal, MGSpace.s2)
            .padding(.vertical, 2)
            .foregroundStyle(Color(mgHex: MGToken.ink950))
            .background(Capsule().fill(Color(mgHex: MGToken.markingHex(chip.level))))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text("Marking \(chip.label)"))
            .accessibilityIdentifier("mg.marking")
    }
}

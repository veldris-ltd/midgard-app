import MidgardCore
import SwiftUI

/// A marking chip: the fixed governance colour of its level (never themed), its text in
/// the ink that meets AA on that colour (white on confidential red). [AC-C-13] [DD-7] [RC-49]
public struct MarkingChipView: View {
    let chip: MarkingChip
    public init(_ chip: MarkingChip) { self.chip = chip }

    public var body: some View {
        Text(chip.label)
            .font(MGFonts.mono(.caption2))
            .padding(.horizontal, MGSpace.s2)
            .padding(.vertical, 2)
            .foregroundStyle(Color(mgHex: MGContrast.ink(on: MGToken.markingHex(chip.level))))
            .background(Capsule().fill(Color(mgHex: MGToken.markingHex(chip.level))))
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text("Marking \(chip.label)"))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.marking")
    }
}

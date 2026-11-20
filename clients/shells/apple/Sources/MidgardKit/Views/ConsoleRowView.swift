import MidgardCore
import SwiftUI

/// A console row in a list: withheld rows by level only; shown rows with their id,
/// marking and (audit rows) signature status. Console screens (Phases C–H) build their
/// own row content on top of this. [AC-C-13]
public struct ConsoleRowView: View {
    let row: RowPresentation
    public init(_ row: ConsoleRow) { self.row = RowPresentation(row) }

    public var body: some View {
        if row.isWithheld {
            WithheldRowView(row)
        } else {
            HStack(spacing: MGSpace.s3) {
                Text(row.title).font(MGFonts.mono(.body))
                Spacer(minLength: MGSpace.s2)
                if let signature = row.signature { SignatureBadgeView(signature) }
                MarkingChipView(row.marking)
            }
            .frame(minHeight: 44)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(row.accessibilityLabel))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.row")
        }
    }
}

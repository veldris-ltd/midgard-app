import MidgardCore
import SwiftUI

/// A row above the persona's clearance: the word "Withheld", the reason and the level's
/// chip, nothing else. [AC-C-13]
public struct WithheldRowView: View {
    let row: RowPresentation
    public init(_ row: RowPresentation) { self.row = row }

    public var body: some View {
        HStack(spacing: MGSpace.s3) {
            Text("\(row.title) — \(row.detail ?? "")").font(MGFonts.sans(.body)).italic()
            Spacer(minLength: MGSpace.s2)
            MarkingChipView(row.marking)
        }
        .frame(minHeight: 44)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(row.accessibilityLabel))
        .accessibilityIdentifier("mg.row.withheld")
    }
}

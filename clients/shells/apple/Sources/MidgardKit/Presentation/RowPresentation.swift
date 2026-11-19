import MidgardCore

/// A console row as a list shows it. A withheld row says "Withheld — above your
/// clearance" with its marking level and nothing else (the core gives it no id, item or
/// signature). [AC-C-13] [FR-C-9]
public struct RowPresentation: Equatable {
    public let isWithheld: Bool
    public let title: String
    public let detail: String?
    public let marking: MarkingChip
    public let signature: SignatureBadge?
    public let accessibilityLabel: String

    public init(_ row: ConsoleRow) {
        marking = row.marking
        if row.withheld {
            isWithheld = true
            title = "Withheld"
            detail = "above your clearance"
            signature = nil
            accessibilityLabel = "Withheld row, \(row.marking.label)"
        } else {
            isWithheld = false
            title = row.id ?? "Row \(row.position + 1)"
            detail = nil
            signature = row.signature.map(SignatureBadge.init)
            let signed = signature.map { ", \($0.accessibilityLabel)" } ?? ""
            accessibilityLabel = "\(title), marking \(row.marking.label)\(signed)"
        }
    }
}

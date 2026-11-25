import MidgardCore

/// How a signature or verification claim is shown. Only a status the core checked —
/// `verified` (the pinned sample-data key) or `device` (this device's audit key) — gets
/// a check mark. A claim the sample data does not check is delivered by the core as
/// "not checked (sample)" and shown that way; any check mark left in other free text is
/// removed; Mimir's provenance label "verified" carries a sample qualifier and is never a
/// check. [AC-C-13] [DD-7]
public struct SignatureBadge: Equatable {
    /// The core's status when the badge shows one; nil for claims and labels.
    public let status: SignatureStatus?
    public let text: String
    public let accessibilityLabel: String
    public let showsCheck: Bool
    public let tone: ChipTone

    /// The text the core delivers for an unchecked claim (`demo::audit::NOT_CHECKED`).
    public static let notChecked = "not checked (sample)"
    /// Check-mark scalars, and the emoji presentation selector that can follow them.
    /// Filtered per scalar: a check followed by U+FE0F is one `Character`.
    private static let checkMarks: Set<UInt32> = [0x2713, 0x2714, 0x2705, 0x2611, 0x1F5F8, 0x1F5F9, 0xFE0F]

    static func withoutCheckMarks(_ text: String) -> String {
        var scalars = String.UnicodeScalarView()
        scalars.append(contentsOf: text.unicodeScalars.filter { !checkMarks.contains($0.value) })
        return String(scalars).trimmingCharacters(in: .whitespaces)
    }

    public init(_ status: SignatureStatus) {
        self.status = status
        switch status {
        case .verified:
            text = "VERIFIED"
            accessibilityLabel = "Signature verified against the pinned sample-data key"
            showsCheck = true
            tone = .secure
        case .device:
            text = "DEVICE-SIGNED"
            accessibilityLabel = "Signed by this device's audit key"
            showsCheck = true
            tone = .secure
        case .unsigned:
            text = "UNSIGNED"
            accessibilityLabel = "Not signed"
            showsCheck = false
            tone = .warn
        }
    }

    /// A claim in a row's text (for example a release's `signature` field).
    public init(claim: String) {
        status = nil
        showsCheck = false
        if claim == Self.notChecked {
            text = "NOT CHECKED (SAMPLE)"
            accessibilityLabel = "Signature claim not checked: sample data"
            tone = .warn
        } else {
            let stripped = Self.withoutCheckMarks(claim)
            text = stripped
            accessibilityLabel = "Claim: \(stripped), not checked"
            tone = .info
        }
    }

    /// A provenance label (Mimir's `feedback.prov`): a label, not a signature.
    public init(provenance: String) {
        status = nil
        showsCheck = false
        tone = .info
        let label = Self.withoutCheckMarks(provenance)
        if label.lowercased() == "verified" {
            text = "verified (sample label)"
            accessibilityLabel = "Provenance: verified, a sample label, not a checked signature"
        } else {
            text = label
            accessibilityLabel = "Provenance: \(label)"
        }
    }
}

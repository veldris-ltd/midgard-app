import MidgardCore

/// How a signature or verification claim is shown. Only a status the core checked —
/// `verified` (the pinned demo-audit key) or `device` (this device's audit key) — gets
/// a check mark. A claim the demo does not check is delivered by the core as
/// "not checked (demo)" and shown that way; any check mark left in other free text is
/// removed; Mimir's provenance label "verified" carries a demo qualifier and is never a
/// check. [AC-C-13] [DD-7]
public struct SignatureBadge: Equatable {
    /// The core's status when the badge shows one; nil for claims and labels.
    public let status: SignatureStatus?
    public let text: String
    public let accessibilityLabel: String
    public let showsCheck: Bool
    public let tone: ChipTone

    /// The text the core delivers for an unchecked claim (`demo::audit::NOT_CHECKED`).
    public static let notChecked = "not checked (demo)"
    private static let checkMarks: Set<Character> = ["\u{2713}", "\u{2714}", "\u{2705}", "\u{2611}"]

    public init(_ status: SignatureStatus) {
        self.status = status
        switch status {
        case .verified:
            text = "VERIFIED"
            accessibilityLabel = "Signature verified against the pinned demo key"
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
            text = "NOT CHECKED (DEMO)"
            accessibilityLabel = "Signature claim not checked: demo data"
            tone = .warn
        } else {
            let stripped = String(claim.filter { !Self.checkMarks.contains($0) })
                .trimmingCharacters(in: .whitespaces)
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
        let label = String(provenance.filter { !Self.checkMarks.contains($0) }).trimmingCharacters(in: .whitespaces)
        if label.lowercased() == "verified" {
            text = "verified (demo label)"
            accessibilityLabel = "Provenance: verified, a demo label, not a checked signature"
        } else {
            text = label
            accessibilityLabel = "Provenance: \(label)"
        }
    }
}

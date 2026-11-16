import MidgardCore

/// The colour a chip takes (`--secure`, `--warn`, `--danger`, `--info`).
public enum ChipTone: Equatable, Sendable {
    case secure, warn, danger, info

    public var hex: UInt32 {
        switch self {
        case .secure: return MGToken.secure
        case .warn: return MGToken.warn
        case .danger: return MGToken.danger
        case .info: return MGToken.info
        }
    }
}

/// One header chip: its text, a spelled-out accessibility label, and its tone.
public struct ChipText: Equatable, Sendable {
    public let text: String
    public let accessibilityLabel: String
    public let tone: ChipTone
}

/// The three header chips — sync, inference tier, key custody — from the core's
/// `StatusChips`. Offline the tier is "AI OFF" (owner decision 6: no simulated
/// on-device tier). Custody says "SECURE ENCLAVE" only when the core reports the root
/// hardware-wrapped. [FR-C-9] [AC-C-13]
public struct StatusChipsPresentation: Equatable, Sendable {
    public let sync: ChipText
    public let tier: ChipText
    public let custody: ChipText

    public init(_ chips: StatusChips) {
        let n = chips.sync.pending
        let changes = n == 1 ? "1 change" : "\(n) changes"
        switch chips.sync.phase {
        case .synced:
            sync = ChipText(text: "SYNCED", accessibilityLabel: "Sync: synced", tone: .secure)
        case .syncing:
            sync = ChipText(text: "SYNCING · \(n)", accessibilityLabel: "Sync: syncing, \(changes)", tone: .info)
        case .offline:
            sync = ChipText(text: "OFFLINE · \(n) QUEUED", accessibilityLabel: "Sync: offline, \(changes) queued", tone: .warn)
        case .conflict:
            sync = ChipText(text: "CONFLICT", accessibilityLabel: "Sync: conflict, review needed", tone: .danger)
        }
        let tierText = Self.tierText(chips.tier.tier)
        let spoken: String
        switch chips.tier.tier {
        case .onDevice: spoken = "on device"
        case .remote: spoken = "remote"
        case .hybrid: spoken = "hybrid"
        case .disabled: spoken = "off"
        }
        let reason = chips.tier.reason.isEmpty ? "" : " (\(chips.tier.reason))"
        tier = ChipText(text: tierText, accessibilityLabel: "AI: \(spoken)\(reason)",
                        tone: chips.tier.tier == .disabled ? .warn : .info)
        if chips.custody.hardwareWrapped {
            custody = ChipText(text: "SECURE ENCLAVE",
                               accessibilityLabel: "Key custody: Secure Enclave, \(chips.custody.floor)", tone: .secure)
        } else {
            custody = ChipText(text: "SOFTWARE KEY",
                               accessibilityLabel: "Key custody: software key, \(chips.custody.floor)", tone: .warn)
        }
    }

    public static func tierText(_ tier: InferenceTier) -> String {
        switch tier {
        case .onDevice: return "ON-DEVICE"
        case .remote: return "REMOTE"
        case .hybrid: return "HYBRID"
        case .disabled: return "AI OFF"
        }
    }
}

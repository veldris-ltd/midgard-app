import MidgardCore

/// The colour a chip takes (`--secure`, `--warn`, `--danger`, `--info`): outlined in the
/// tone, filled with its tint, lettered in its per-mode text token. [RC-49]
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

    /// The tone's tint (`--<tone>-tint`), the chip's fill.
    public var tint: (UInt32, Double) {
        switch self {
        case .secure: return MGToken.secureTint
        case .warn: return MGToken.warnTint
        case .danger: return MGToken.dangerTint
        case .info: return MGToken.infoTint
        }
    }

    /// Text on the tint, per mode: meets AA on the tint over every surface of the mode.
    public var text: (dark: UInt32, light: UInt32) {
        switch self {
        case .secure: return (MGToken.ToneText.secureDark, MGToken.ToneText.secureLight)
        case .warn: return (MGToken.ToneText.warnDark, MGToken.ToneText.warnLight)
        case .danger: return (MGToken.ToneText.dangerDark, MGToken.ToneText.dangerLight)
        case .info: return (MGToken.ToneText.infoDark, MGToken.ToneText.infoLight)
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

/// The data chip, first in the status row: which data the consoles show. The sample
/// workspace is named as such and never called live; "LIVE · <host>" only while a gateway
/// the user connected is serving Midgard's screens. [AC-C-13]
public enum DataSourceChip {
    public static func make(kind: DataSourceKind?, liveHost: String?, imported: Bool) -> ChipText? {
        switch kind {
        case DataSourceKind.sample?, DataSourceKind.demo?:
            if let host = liveHost {
                return ChipText(text: "LIVE · \(host)",
                                accessibilityLabel: "Data: live from \(host). Midgard's screens read this gateway; the other consoles show the sample workspace.",
                                tone: .secure)
            }
            if imported {
                return ChipText(text: "SAMPLE WORKSPACE + IMPORTED",
                                accessibilityLabel: "Data: the sample workspace and your imported files", tone: .info)
            }
            return ChipText(text: "SAMPLE WORKSPACE",
                            accessibilityLabel: "Data: the sample workspace, built into the app", tone: .info)
        case DataSourceKind.none?:
            return ChipText(text: "NO DATA SOURCE", accessibilityLabel: "Data: no data source connected", tone: .warn)
        case nil:
            return nil
        }
    }
}

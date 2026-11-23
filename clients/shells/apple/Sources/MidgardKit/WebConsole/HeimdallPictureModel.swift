import Foundation
@preconcurrency import MidgardCore

/// Heimdall's console frame as the web suite draws it (`ui/shared/src/nav.ts`, product
/// `heimdall`, at the signed tag): the classification banner, the header's seals and
/// the navigation's sections with their badges. These are the web console's fixed
/// chrome, not data. [DD-7]
public enum HeimdallChrome {
    public static let banner = (level: MarkingLevelChip.confidential,
                                text: "CONFIDENTIAL · OPERATIONAL PICTURE · RECIPIENT EYES ONLY")
    public static let context = "COMMON OPERATING PICTURE / SECTOR NORTH"
    public static let seals: [(WebStatusPill.Status, String)] = [
        (.online, "12 FEEDS · FUSED"), (.secure, "FEEDS PQ-AUTH ✓"), (.secure, "SYNC · 3 SITES"),
    ]

    /// One navigation item: the web's id, label, icon (as an SF Symbol) and badge, and
    /// the app's screen it opens (`ConsoleCatalog`'s Heimdall screens).
    public struct Item {
        public let id: String
        public let label: String
        public let symbol: String
        public let badge: String?
        public let screen: String
    }

    public static let sections: [(title: String, items: [Item])] = [
        ("PICTURE", [
            Item(id: "cop", label: "Operating picture", symbol: "map", badge: nil, screen: "Operating Picture"),
            Item(id: "feeds", label: "Feeds", symbol: "antenna.radiowaves.left.and.right", badge: "12", screen: "Feeds"),
            Item(id: "entities", label: "Entities", symbol: "scope", badge: "1,204", screen: "Entities"),
            Item(id: "sync", label: "Picture sync", symbol: "point.3.connected.trianglepath.dotted", badge: nil,
                 screen: "Picture Sync"),
        ]),
        ("DECIDE", [
            Item(id: "workflows", label: "Action workflows", symbol: "arrow.triangle.branch", badge: "2", screen: "Workflows"),
            Item(id: "lineage", label: "Decision lineage", symbol: "scroll", badge: nil, screen: "Decision Lineage"),
        ]),
    ]
}

/// The Operating Picture's state, as `OperatingPictureScreen.tsx` keeps it: the
/// picture's layers and tracks (as the signed-in persona may see them), the layer
/// switches, the selected track, the chosen option and the human gate. Authorise goes
/// through the core as the action `heimdall.authoriseWorkflow` on the option's workflow
/// (with a human-gate token, as the web console sends one); Reject is the operator's
/// own decision, recorded on screen only, as in the web console. [FR-H-4] [FR-H-5]
/// [FR-C-4] [AC-C-13]
@MainActor
public final class HeimdallPictureModel: ObservableObject {
    public enum Gate: Equatable {
        case pending, queued, authorised(String?), rejected
    }

    @Published public private(set) var tracks: [HeimdallTrack] = []
    @Published public private(set) var markings: [String: MarkingChip] = [:]
    /// Tracks above the persona's clearance: counted, never drawn.
    @Published public private(set) var withheld = 0
    @Published public private(set) var layers: [HeimdallLayer] = []
    @Published public private(set) var loaded = false
    @Published public var layerOn: [String: Bool] = [:]
    @Published public private(set) var selected: String?
    @Published public private(set) var chosen = 1
    @Published public private(set) var gate: Gate = .pending
    @Published public private(set) var busy = false
    @Published public var hovered: String?
    @Published public var message: String?

    /// Option rank → the human-gated workflow it opens; rank 3 (hold) opens none.
    public static let workflowForRank: [Int: String] = [1: "W-0912", 2: "W-0913"]

    /// What each class of track is seen by (the web's `CAPABILITY`).
    public static let capability: [String: String] = [
        "AIRCRAFT": "IFF · ADS-B · RADAR", "VESSEL": "AIS · RADAR", "GROUND UNIT": "OBSERVER NET",
        "UNKNOWN": "RADAR ONLY · UNCORROBORATED",
    ]

    private let app: AppModel
    private var token = 0

    public init(app: AppModel) {
        self.app = app
    }

    public var selectedTrack: HeimdallTrack? { tracks.first { $0.id == selected } }
    public var visibleTracks: [HeimdallTrack] { tracks.filter { isOn($0.layer) } }
    public var options: [HeimdallOption] { selectedTrack?.options ?? [] }

    public func isOn(_ layer: String) -> Bool { layerOn[layer] != false }

    /// Read the picture, its tracks and the workflows the options act on.
    public func load() async {
        let picture = await app.rows("heimdall.picture")
        let rows = await app.rows("heimdall.tracks")
        // Cached so an authorise can name its workflow (the core acts only on rows read).
        _ = await app.rows("heimdall.workflows")
        layers = picture.compactMap {
            if case let .heimdallPicture(item)? = $0.item { return item.layers }
            return nil
        }.first ?? []
        if layerOn.isEmpty {
            layerOn = Dictionary(uniqueKeysWithValues: layers.map { ($0.id, $0.active) })
        }
        var shown: [HeimdallTrack] = []
        var marks: [String: MarkingChip] = [:]
        for row in rows {
            guard case let .heimdallTrack(track)? = row.item else { continue }
            shown.append(track)
            marks[track.id] = row.marking
        }
        tracks = shown
        markings = marks
        withheld = rows.filter(\.withheld).count
        if selected == nil || !shown.contains(where: { $0.id == selected }) {
            selected = shown.first { $0.id == "E-0871" }?.id ?? shown.first?.id
        }
        loaded = true
    }

    public func select(_ id: String) {
        selected = id
        gate = .pending
    }

    public func choose(_ rank: Int) {
        chosen = rank
        gate = .pending
    }

    public func setLayer(_ id: String, _ on: Bool) {
        layerOn[id] = on
    }

    /// Authorise the chosen option: its workflow, with a human-gate token, through the
    /// core; then sync, and show the audit id the enclosure gives.
    public func authorise() async {
        guard let workflow = Self.workflowForRank[chosen], !busy else { return }
        busy = true
        defer { busy = false }
        token += 1
        // In sample mode the human gate is the Authorise press itself: only this call,
        // from that press, sends a token; no handler may send one automatically.
        let gateToken = "HG-\(String(Int(Date().timeIntervalSince1970 * 1000), radix: 36))-\(token)"
        guard let id = await app.requestAction(ActionInput(verb: "heimdall.authoriseWorkflow",
                                                           target: "heimdall.workflows/\(workflow)",
                                                           params: [KeyValue(key: "humanGateToken", value: gateToken)]))
        else {
            message = app.message
            return
        }
        await app.syncNow()
        switch await app.actionStatus(id) {
        case let status? where status.state == .applied:
            gate = .authorised(status.auditId)
        case let status? where status.state == .refused || status.state == .conflict:
            gate = .pending
            message = status.reason
        default:
            gate = .queued
        }
    }

    /// The operator rejects the option: recorded on screen, as the web console does.
    public func reject() {
        gate = .rejected
    }

    /// Range from the picture's centre, as the web computes it.
    public static func range(_ t: HeimdallTrack) -> Int {
        Int((hypot(t.x - 50, t.y - 50) * 1.3).rounded())
    }

    public static func heading(_ value: Double) -> String {
        let h = Int(value.rounded())
        return h < 10 ? "00\(h)" : h < 100 ? "0\(h)" : "\(h)"
    }
}

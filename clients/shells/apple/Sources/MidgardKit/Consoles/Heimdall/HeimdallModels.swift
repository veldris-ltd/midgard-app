import Foundation
@preconcurrency import MidgardCore

// Heimdall's screen models, over the release's sample data and through the core only
// (`AppModel`, as Midgard's and Admin's are): every row keeps its marking, rows above
// the persona are withheld by count, and the one action is the core's port of
// `HeimdallMock.authoriseWorkflow` (`clients/core/src/demo/actions/heimdall.rs`), decided
// once and audited in `heimdall.audit`. Heimdall is not served by a gateway: connected
// to one, these screens keep the sample data. [FR-C-1] [FR-C-4] [FR-H-1] [FR-H-5]
// [AC-C-13]

public extension MidgardListModel where Item == HeimdallEntity {
    static func entities(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "heimdall.entities") {
            if case let .heimdallEntity(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == HeimdallFeed {
    static func feeds(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "heimdall.feeds") {
            if case let .heimdallFeed(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == HeimdallSyncSite {
    static func syncSites(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "heimdall.syncSites") {
            if case let .heimdallSyncSite(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == HeimdallDecision {
    static func decisions(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "heimdall.decisions") {
            if case let .heimdallDecision(item) = $0 { return item }
            return nil
        }
    }
}

/// The Entities screen's filter tags (`FILTERS` in `EntitiesScreen.tsx`), in the web's
/// order and words.
public enum HeimdallEntityFilter: String, CaseIterable, Sendable {
    case all, air, maritime, ground, anomalous
    case lowConfidence = "low confidence"

    /// Whether `e` passes the filter, as the web screen filters.
    public func admits(_ e: HeimdallEntity) -> Bool {
        switch self {
        case .all: return true
        case .anomalous: return e.track == "anomalous"
        case .lowConfidence: return e.conf < 70
        case .air: return e.cls == "AIRCRAFT"
        case .maritime: return e.cls == "VESSEL"
        case .ground: return e.cls == "GROUND UNIT"
        }
    }
}

public extension HeimdallFeed {
    /// Unverified (no PQ authentication), so quarantined out of fusion: the sample's
    /// failed bridge and every feed the user added.
    var quarantined: Bool { !auth }
}

/// What a sample or user feed row's PQ authentication reads as. The sample states it
/// (`auth`, `scheme`); the app checked nothing, so no check mark is drawn.
public func heimdallFeedAuth(_ f: HeimdallFeed) -> String {
    if f.auth { return "\(f.scheme) · AUTH" }
    return f.scheme == "none" ? "UNVERIFIED" : "FAILED"
}

/// The status a Heimdall row carries (`status`) as a pill.
func heimdallPill(_ status: String, _ label: String) -> WebStatusPill {
    switch status {
    case "secure": return WebStatusPill(.secure, label, live: MGWebAccent.heimdall.text)
    case "online": return WebStatusPill(.online, label, live: MGWebAccent.heimdall.text)
    case "healthy": return WebStatusPill(.healthy, label, live: MGWebAccent.heimdall.text)
    case "warn": return WebStatusPill(.warn, label, live: MGWebAccent.heimdall.text)
    case "gated": return WebStatusPill(.gated, label, live: MGWebAccent.heimdall.text)
    case "danger": return WebStatusPill(.danger, label, live: MGWebAccent.heimdall.text)
    default: return WebStatusPill(.offline, label, live: MGWebAccent.heimdall.text)
    }
}

// MARK: - Workflows

/// Workflows (`WorkflowsScreen.tsx`): the human-gated action workflows. Authorise sends
/// the selected workflow to the core's port of `authoriseWorkflow` with a human-gate
/// token made from the operator's press (as the web screen makes one); the core refuses
/// a gated workflow without one ("Human gate required…") and audits every outcome, and
/// the inspector shows the audit id it gives. Reject is the operator's own decision,
/// shown on screen and not sent, as on the web. [FR-H-5] [NFR-AI-1]
@MainActor
public final class HeimdallWorkflowsModel: ObservableObject {
    public enum Gate: Equatable {
        case pending, queued, authorised(String?), rejected
    }

    @Published public private(set) var rows: [MidgardRow<HeimdallWorkflow>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public private(set) var selected = 0
    @Published public private(set) var gate: Gate?
    @Published public private(set) var busy = false
    @Published public private(set) var message: String?

    private let app: AppModel
    private var presses = 0

    public init(app: AppModel) {
        self.app = app
    }

    public var currentIndex: Int? { rows.isEmpty ? nil : min(selected, rows.count - 1) }
    public var current: HeimdallWorkflow? { currentIndex.map { rows[$0].item } }
    /// The toolbar's "N AWAITING AUTHORISATION": the human-gated workflows shown.
    public var awaiting: Int { rows.filter(\.item.humanGated).count }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("heimdall.workflows") {
            if case let .heimdallWorkflow(item) = $0 { return item }
            return nil
        }
        self.rows = rows
        self.withheld = withheld
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    public func select(_ index: Int) {
        selected = index
        gate = nil
        message = nil
    }

    /// Authorise the selected workflow, with a token from this press.
    public func authorise() async {
        await authorise(token: true)
    }

    /// Send the authorisation; `token` false sends none (the gate's refusal, for tests).
    func authorise(token: Bool) async {
        guard let w = current, !busy else { return }
        busy = true
        defer { busy = false }
        message = nil
        presses += 1
        // The human gate is this press: only this call, from it, sends a token.
        let gateToken = "HG-\(String(Int(Date().timeIntervalSince1970 * 1000), radix: 36))-\(presses)"
        let params = token ? [("humanGateToken", gateToken)] : []
        switch await app.midgardRun("heimdall.authoriseWorkflow", "heimdall.workflows/\(w.id)", params) {
        case let .applied(auditId, _):
            gate = .authorised(auditId)
        case let .refused(reason), let .conflict(reason), let .failed(reason):
            gate = .pending
            message = reason
        case .queued:
            gate = .queued
        }
    }

    public func reject() {
        gate = .rejected
    }

    /// Whether the selected workflow's gate was authorised on this screen.
    public var authorised: Bool {
        if case .authorised = gate { return true }
        return false
    }

    /// The inspector's STAGES, as the web screen derives them.
    public func stages(_ w: HeimdallWorkflow) -> [(String, Bool)] {
        let done = w.stage == "complete"
        return [
            ("Anomaly detected", true),
            ("Options generated · cited", true),
            ("Operator review", true),
            ("Human authorisation", !w.humanGated || authorised),
            ("Execution", w.stage == "executing" || done || authorised),
            ("Signed lineage", done || authorised),
        ]
    }

    /// The line under a workflow that needs no gate now, as the web words it.
    public func signedLabel(_ w: HeimdallWorkflow) -> String {
        if case let .authorised(id) = gate { return "AUTHORISED · AUDIT #\(id ?? "—")" }
        return w.stage == "complete" ? "COMPLETE · AUDIT #D-4468" : "AUTHORISED · EXECUTING"
    }
}

// MARK: - Decision lineage

/// Decision Lineage (`DecisionLineageScreen.tsx`): the signed decision records and the
/// live lineage tail. The tail is `heimdall.audit` as the core delivers it (the mock's
/// seed and what this device's actions add), newest first, at most 40, re-read while the
/// screen is open; the mock's synthetic stream is not invented. A record's signature is
/// shown as the core checked it: the seeded rows carry none, this device's own are
/// signed by its audit key. [FR-H-5] [NFR-SEC-2] [AC-C-13]
@MainActor
public final class HeimdallLineageModel: ObservableObject {
    @Published public private(set) var decisions: [MidgardRow<HeimdallDecision>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var tail: [MidgardRow<AuditRecord>] = []
    @Published public private(set) var tailWithheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var selected = 0

    /// How often the tail is re-read while the screen is open (the mock streams a record
    /// every 4.2 s).
    public static let tailInterval: UInt64 = 4_200_000_000
    /// The most records the tail keeps (`useAuditStream(…, 40)`).
    public static let tailLimit = 40

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public var currentIndex: Int? { decisions.isEmpty ? nil : min(selected, decisions.count - 1) }
    public var current: HeimdallDecision? { currentIndex.map { decisions[$0].item } }

    /// The tail's records whose signature the core checked.
    public var signed: Int { tail.filter { $0.signature == .verified || $0.signature == .device }.count }

    /// "signed/total", or "—" with no records shown.
    public var signedText: String { tail.isEmpty ? "—" : "\(signed)/\(tail.count)" }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("heimdall.decisions") {
            if case let .heimdallDecision(item) = $0 { return item }
            return nil
        }
        decisions = rows
        self.withheld = withheld
        await refreshTail()
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    public func refreshTail() async {
        let (rows, withheld) = await app.midgardRows("heimdall.audit") {
            if case let .auditRecord(item) = $0 { return item }
            return nil
        }
        tail = Array(rows.prefix(Self.tailLimit))
        tailWithheld = withheld
    }

    /// Re-read the tail until the task is cancelled (the screen closes).
    public func follow() async {
        while !Task.isCancelled {
            try? await Task.sleep(nanoseconds: Self.tailInterval)
            if Task.isCancelled { return }
            await refreshTail()
        }
    }
}

// MARK: - Alerts

/// One alert on the Operating Picture's Alerts list: what the picture's own data says
/// needs attention (an anomalous track, a feed quarantined out of fusion or stale). The
/// web console's Alerts button has no list behind it; this one is built from the same
/// sample rows the screens show, nothing invented. [FR-H-1] [FR-H-3]
public struct HeimdallAlert: Identifiable, Equatable, Sendable {
    public enum Tone: Sendable { case warn, danger }
    public let id: String
    public let tone: Tone
    public let title: String
    public let detail: String
    /// The track the alert is about, which choosing it selects.
    public let track: String?

    /// The alerts for `tracks` and `feeds` (the persona's own rows): anomalous tracks
    /// first, then feeds failing authentication, then stale ones.
    public static func of(tracks: [HeimdallTrack], feeds: [HeimdallFeed]) -> [HeimdallAlert] {
        var out: [HeimdallAlert] = []
        for t in tracks where t.track == "anomalous" {
            out.append(HeimdallAlert(id: "track.\(t.id)", tone: .warn, title: "\(t.id) · \(t.cls) · ANOMALOUS",
                                     detail: "Confidence \(Int((t.conf * 100).rounded()))% · \(Int(t.feeds)) feeds · ranked options in the inspector",
                                     track: t.id))
        }
        for f in feeds where !f.auth {
            out.append(HeimdallAlert(id: "feed.\(f.name)", tone: .danger, title: "\(f.name) · \(f.statusLabel.uppercased())",
                                     detail: "Quarantined: excluded from fusion until it authenticates", track: nil))
        }
        for f in feeds where f.auth && f.status == "warn" {
            out.append(HeimdallAlert(id: "feed.\(f.name)", tone: .warn, title: "\(f.name) · \(f.statusLabel.uppercased())",
                                     detail: "\(f.kind) · latency \(f.latency)", track: nil))
        }
        return out
    }
}

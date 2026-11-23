import Foundation
@preconcurrency import MidgardCore

// Admin's screen models, over the release's sample data and through the core only
// (`AppModel`, as Midgard's are): every row keeps its marking, rows above the persona
// are withheld by count, and every action is the core's port of `AdminMock`
// (`clients/core/src/demo/actions/admin.rs`), decided once and audited in
// `admin.audit`. [FR-C-1] [FR-C-4] [FR-AD-1] [AC-C-13]

public extension MidgardListModel where Item == AdminOverview {
    static func overview(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "admin.overview") {
            if case let .adminOverview(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == AdminRole {
    static func roles(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "admin.roles") {
            if case let .adminRole(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == AdminBinding {
    static func bindings(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "admin.bindings") {
            if case let .adminBinding(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == AdminRegistryItem {
    static func registry(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "admin.registry") {
            if case let .adminRegistryItem(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == AdminCopilotTurn {
    static func copilot(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "admin.copilot") {
            if case let .adminCopilotTurn(item) = $0 { return item }
            return nil
        }
    }
}

extension AppModel {
    /// The id of the newest `admin.audit` row this persona made on `target`: a refused
    /// action's audit id (the core returns one only with an applied action).
    func adminAuditId(for target: String) async -> String? {
        let me = signedInPersona?.id
        let (rows, _) = await midgardRows("admin.audit") {
            if case let .auditRecord(item) = $0 { return item }
            return nil
        }
        return rows.first { $0.item.resource == target && $0.item.actor == me }?.id
    }

    /// Admin's live ledger (`admin.audit`), as the web's `useAuditStream` lists it.
    func adminLiveLedger() async -> (rows: [MidgardRow<AuditRecord>], withheld: Int) {
        await midgardRows("admin.audit") {
            if case let .auditRecord(item) = $0 { return item }
            return nil
        }
    }
}

/// `admin.approve`'s answer (`AdminApprovalDecision`, the audit id coming with the
/// action).
public struct AdminDecisionAnswer: Decodable, Equatable {
    public let decision: String
    public let reason: String
}

/// `admin.simulateConfig`'s answer (`AdminConfigSimulation`).
public struct AdminSimulationAnswer: Decodable, Equatable {
    public let grants: [String]
    public let revokes: [String]
    public let summary: String
}

/// `admin.commitConfig` and `admin.rollbackConfig`'s answer (the version).
public struct AdminVersionAnswer: Decodable, Equatable {
    public let version: String
}

/// One entry of `admin.searchLedger`'s answer (`AdminLedgerEntry`).
public struct AdminLedgerAnswer: Decodable, Equatable {
    public let id: String
    public let actor: String
    public let action: String
    public let subsystem: String
    public let justification: String
    public let approver: String
    public let ts: String

    var entry: AdminLedgerEntry {
        AdminLedgerEntry(id: id, actor: actor, action: action, subsystem: subsystem, justification: justification,
                         approver: approver, ts: ts)
    }
}

/// `admin.requestBreakGlass`'s answer (`AdminBreakGlass`).
public struct AdminBreakGlassAnswer: Decodable, Equatable {
    public let state: String
    public let holder: String
    public let scope: String
    public let remaining: String
    public let justification: String
}

// MARK: - Approvals

/// Approvals (`ApprovalsScreen.tsx`): the dual-control queue. Approving routes the
/// decision through the core as the persona signed in (`admin.approve`), which denies a
/// requester's own approval (AC-AD-4) and audits both outcomes (AC-AD-6); each decision
/// heads the "AUDIT · INCLUDING DENIALS" tail with its audit id. Deny is the second
/// controller's own decision on screen, as on the web: it is not sent. [FR-AD-5]
@MainActor
public final class AdminApprovalsModel: ObservableObject {
    /// The tail as the web screen starts it.
    public static let initialTail = [
        "> admin audit tail --class approvals",
        "#A-9107  denied   fleet-release   m.silva  self-approval · separation check",
        "#A-9104  approved model-revoke    p.iwu → j.benjamin · token 10m",
        "#A-9101  approved kms-rotation    drill · k.osei → a.novak",
    ]

    @Published public private(set) var items: [AdminApproval] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public private(set) var busy: String?
    @Published public private(set) var tail = AdminApprovalsModel.initialTail
    @Published public private(set) var notice: MidgardNotice?
    /// Denied on this screen by its second controller (id → reason), not sent.
    private var denied: [String: String] = [:]

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("admin.approvals") {
            if case let .adminApproval(item) = $0 { return item }
            return nil
        }
        items = rows.map { row in
            var a = row.item
            if a.state == "awaiting", let reason = denied[a.id] {
                a.state = "denied"
                a.deniedReason = reason
            }
            return a
        }
        self.withheld = withheld
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    public var awaiting: Int { items.filter { $0.state == "awaiting" }.count }
    public var approved: Int { items.filter { $0.state == "approved" }.count }
    public var deniedCount: Int { items.filter { $0.state == "denied" }.count }

    /// The second controller's approval, as the persona signed in.
    public func approve(_ item: AdminApproval) async {
        guard busy == nil else { return }
        busy = item.id
        notice = nil
        defer { busy = nil }
        let target = "admin.approvals/\(item.id)"
        switch await app.midgardRun("admin.approve", target) {
        case let .applied(audit, result):
            let reason = midgardDecode(AdminDecisionAnswer.self, result)?.reason ?? "granted"
            record("\(audit ?? "—")  approved \(item.action)  ·  \(reason)")
        case let .refused(why):
            let audit = await app.adminAuditId(for: target)
            record("\(audit ?? "—")  denied   \(item.action)  ·  \(why)")
        case let .conflict(why), let .failed(why):
            notice = MidgardNotice(.danger, why)
        case .queued:
            notice = MidgardNotice(.warn, "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE")
        }
        await load()
    }

    /// The second controller's denial: shown here, not sent (the web screen's own).
    public func deny(_ item: AdminApproval) {
        denied[item.id] = "DENIED · SECOND CONTROLLER REJECTED"
        items = items.map {
            guard $0.id == item.id, $0.state == "awaiting" else { return $0 }
            var a = $0
            a.state = "denied"
            a.deniedReason = "DENIED · SECOND CONTROLLER REJECTED"
            return a
        }
    }

    private func record(_ line: String) {
        tail = [tail[0], line] + tail.dropFirst().prefix(2)
    }
}

// MARK: - Config

/// Config (`ConfigScreen.tsx`): versioned config. The selected change's diff is
/// simulated (`admin.simulateConfig`, a computed read: not run offline), committed
/// (`admin.commitConfig`, the next version, signed) and rolled back
/// (`admin.rollbackConfig`); each step is audited. [FR-AD-6] [FR-AD-7] [AC-AD-7]
@MainActor
public final class AdminConfigModel: ObservableObject {
    @Published public private(set) var commits: [MidgardRow<AdminConfigCommit>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var selected = 0 {
        didSet {
            guard selected != oldValue else { return }
            simulation = nil
            committedAt = nil
            rolledBackAt = nil
            error = nil
        }
    }
    @Published public private(set) var simulation: AdminSimulationAnswer?
    @Published public private(set) var committedAt: String?
    @Published public private(set) var rolledBackAt: String?
    @Published public private(set) var error: String?
    @Published public private(set) var busy = false

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public var current: AdminConfigCommit? { commits.indices.contains(selected) ? commits[selected].item : nil }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("admin.configCommits") {
            if case let .adminConfigCommit(item) = $0 { return item }
            return nil
        }
        commits = rows
        self.withheld = withheld
        if selected >= rows.count { selected = 0 }
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    public func simulate() async {
        guard let c = current, !busy else { return }
        busy = true
        error = nil
        defer { busy = false }
        guard !app.isOffline else {
            error = MidgardOutcome.offlineRead
            return
        }
        switch await app.midgardRun("admin.simulateConfig", "admin.configCommits/\(c.id)") {
        case let .applied(_, result):
            simulation = midgardDecode(AdminSimulationAnswer.self, result)
            if simulation == nil { error = "The enclosure's answer did not decode." }
        case let .refused(why), let .conflict(why), let .failed(why):
            error = why
        case .queued:
            error = MidgardOutcome.offlineRead
        }
    }

    public func commit() async {
        guard let c = current else { return }
        if let version = await change("admin.commitConfig", c) { committedAt = version }
    }

    public func rollback() async {
        guard let c = current else { return }
        if let version = await change("admin.rollbackConfig", c) {
            rolledBackAt = version
            committedAt = nil
        }
    }

    private func change(_ verb: String, _ c: AdminConfigCommit) async -> String? {
        guard !busy else { return nil }
        busy = true
        error = nil
        defer { busy = false }
        let outcome = await app.midgardRun(verb, "admin.configCommits/\(c.id)")
        await load()
        switch outcome {
        case let .applied(_, result):
            return midgardDecode(AdminVersionAnswer.self, result)?.version
        case let .refused(why), let .conflict(why), let .failed(why):
            error = why
        case .queued:
            error = "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE"
        }
        return nil
    }
}

// MARK: - Ledger

/// Ledger (`LedgerScreen.tsx`): one ledger across the subsystems (`admin.ledger`), its
/// search (`admin.searchLedger`, the mock's `actor:`, `subsystem:` and `since:` tokens;
/// a computed read: not run offline), the filter tags as the web draws them, the
/// selected entry, and the live lineage (`admin.audit`). [FR-AD-9] [AC-AD-6] [NFR-PQ-3]
@MainActor
public final class AdminLedgerModel: ObservableObject {
    public static let filters = ["all", "approvals", "denials", "break-glass", "simulations"]

    @Published public private(set) var ledger: [AdminLedgerEntry] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var results: [AdminLedgerEntry]?
    @Published public private(set) var live: [MidgardRow<AuditRecord>] = []
    @Published public private(set) var liveWithheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var draft = ""
    @Published public var filter = "all"
    @Published public var selected = 1
    @Published public private(set) var error: String?
    @Published public private(set) var busy = false

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    /// The entries shown: the last search's answer, or the whole ledger.
    public var rows: [AdminLedgerEntry] { results ?? ledger }

    public var current: AdminLedgerEntry? {
        let r = rows
        return r.indices.contains(selected) ? r[selected] : r.first
    }

    public var currentIndex: Int? {
        let r = rows
        return r.isEmpty ? nil : (r.indices.contains(selected) ? selected : 0)
    }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("admin.ledger") {
            if case let .adminLedgerEntry(item) = $0 { return item }
            return nil
        }
        ledger = rows.map(\.item)
        self.withheld = withheld
        await reloadLive()
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    func reloadLive() async {
        (live, liveWithheld) = await app.adminLiveLedger()
    }

    /// Return in the search box: the ledger searched for the draft, audited.
    public func search() async {
        guard !busy else { return }
        busy = true
        error = nil
        defer { busy = false }
        guard !app.isOffline else {
            error = MidgardOutcome.offlineRead
            return
        }
        switch await app.midgardRun("admin.searchLedger", "admin.ledger/*", [("query", draft)]) {
        case let .applied(_, result):
            if let found = midgardDecode([AdminLedgerAnswer].self, result) {
                results = found.map(\.entry)
                selected = 0
            } else {
                error = "The enclosure's answer did not decode."
            }
        case let .refused(why), let .conflict(why), let .failed(why):
            error = why
        case .queued:
            error = MidgardOutcome.offlineRead
        }
        await reloadLive()
    }
}

// MARK: - Break-glass

/// Break-glass (`BreakGlassScreen.tsx`): the emergency elevation's state
/// (`admin.breakGlass`, confidential: withheld below), the phase tags the web shows it
/// by, and the request (`admin.requestBreakGlass`, with the web screen's written
/// justification), which opens it active, time-boxed and held by the requester.
/// [FR-AD-8] [AC-AD-8]
@MainActor
public final class AdminBreakGlassModel: ObservableObject {
    /// The justification the web screen's request sends.
    public static let reason = "HSM failover mid-rotation: primary HSM unreachable, rotation window closes 07:00. "
        + "Need unwrap capability to complete re-wrap on secondary."

    @Published public private(set) var session: AdminBreakGlass?
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    /// The phase shown (the web's tags switch it on screen): idle, active or review.
    @Published public var phase = "review"
    @Published public private(set) var notice: MidgardNotice?
    @Published public private(set) var busy = false

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("admin.breakGlass") {
            if case let .adminBreakGlass(item) = $0 { return item }
            return nil
        }
        session = rows.first?.item
        self.withheld = withheld
        if let s = session { phase = s.state }
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    public func request() async {
        guard !busy else { return }
        busy = true
        notice = nil
        defer { busy = false }
        switch await app.midgardRun("admin.requestBreakGlass", "admin.breakGlass/breakglass", [("reason", Self.reason)]) {
        case let .applied(_, result):
            if let s = midgardDecode(AdminBreakGlassAnswer.self, result) {
                session = AdminBreakGlass(state: s.state, holder: s.holder, scope: s.scope, remaining: s.remaining,
                                          justification: s.justification)
                phase = s.state
            }
        case let .refused(why), let .conflict(why), let .failed(why):
            notice = MidgardNotice(.danger, why)
        case .queued:
            notice = MidgardNotice(.warn, "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE")
        }
    }
}

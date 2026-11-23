import Foundation
@preconcurrency import MidgardCore

/// Policies (`PoliciesScreen.tsx`): the rules this persona may read, the markings and
/// purposes they reference, and the policy dry run: a purpose and a marking evaluated
/// against the session's policy context (the signed-in persona's subject, clearance and
/// roles) by `midgard.simulatePolicy`, for a signed grant or deny with the matched
/// rule. Rules come from the policy file at boot, so New policy and Edit clearances are
/// disabled and say so. [NFR-SEC-2] [FR-C-4]
@MainActor
public final class PoliciesModel: ObservableObject {
    public static let markings = ["confidential", "restricted", "internal", "open"]

    @Published public private(set) var rules: [MidgardRow<MidgardPolicyRule>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var tab = 0
    @Published public var selected = 0
    @Published public var purpose = "operations"
    @Published public var marking = "confidential"
    @Published public private(set) var decision: MidgardPolicyAnswer?
    /// The purpose and marking the decision shown was for, and its audit id.
    @Published public private(set) var decided: (purpose: String, marking: String, auditId: String?)?
    @Published public private(set) var error: String?
    @Published public private(set) var busy = false

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    /// The session's policy context (`gateway.policy()`).
    public var subject: String { app.signedInPersona?.id ?? "" }
    public var clearance: String {
        (app.signedInPersona?.clearanceLabel ?? "").components(separatedBy: " // ").first ?? ""
    }
    public var roles: [String] { app.signedInPersona?.roles ?? [] }

    /// The context's purpose first, then each rule's (`Array.from(new Set(…))`).
    public var purposes: [String] {
        var seen: [String] = []
        for p in ["operations"] + rules.map(\.item.purpose) where !seen.contains(p) { seen.append(p) }
        return seen
    }

    public var current: MidgardPolicyRule? { rules.indices.contains(selected) ? rules[selected].item : nil }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("midgard.policies") {
            if case let .midgardPolicyRule(item) = $0 { return item }
            return nil
        }
        rules = rows
        self.withheld = withheld
        if selected >= rows.count { selected = 0 }
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    public func evaluate() async {
        guard !busy else { return }
        busy = true
        error = nil
        defer { busy = false }
        guard !app.isOffline else {
            error = MidgardOutcome.offlineRead
            return
        }
        let (purpose, marking) = (self.purpose, self.marking)
        switch await app.midgardRun("midgard.simulatePolicy", "midgard.policies/*", [("purpose", purpose), ("marking", marking)]) {
        case let .applied(audit, result):
            decision = midgardDecode(MidgardPolicyAnswer.self, result)
            decided = (purpose, marking, audit)
            if decision == nil { error = "The enclosure's answer did not decode." }
        case let .refused(why), let .conflict(why), let .failed(why):
            error = why
        case .queued:
            error = MidgardOutcome.offlineRead
        }
    }
}

/// Audit (`AuditScreen.tsx`): the ledger (`midgard.audit`, newest first) with a
/// substring filter on actor and resource and the category tags, the selected record's
/// lineage trace, and Export chain, which saves the records shown. Every figure is
/// counted from the records this persona may read. A record shows a check only when the
/// core checked its signature. [NFR-SEC-2] [AC-M-4] [AC-C-13]
@MainActor
public final class AuditModel: ObservableObject {
    public enum Category: String, CaseIterable {
        case all, actions, queries, writes, sessions, admin, agent

        public var label: String {
            switch self {
            case .all: return "all events"
            case .actions: return "actions"
            case .queries: return "queries"
            case .writes: return "writes"
            case .sessions: return "sign-ins"
            case .admin: return "administration"
            case .agent: return "agent activity"
            }
        }
    }

    @Published public private(set) var records: [MidgardRow<AuditRecord>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var query = ""
    @Published public var category: Category = .all
    @Published public var selected = 0

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("midgard.audit") {
            if case let .auditRecord(item) = $0 { return item }
            return nil
        }
        records = rows
        self.withheld = withheld
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }

    /// Edge decisions on these verbs bring data in, rather than act on it.
    static let writeVerbs: Set<String> = ["SyncRecords", "OpenChannel"]

    /// The category of one ledger entry, as the web reads the gateway's and the mock's
    /// wording; the core's own (`applied · midgard.query`) reads by its verb.
    public static func category(of r: AuditRecord) -> Category? {
        let words = r.action.split(whereSeparator: \.isWhitespace).map(String.init)
        let head = words.first ?? ""
        let verb = words.count > 1 ? words[1] : ""
        if head == "PERMIT" || head == "DENY" {
            if verb == "Read" { return .queries }
            return writeVerbs.contains(verb) ? .writes : .actions
        }
        if head == "LOGIN" { return .sessions }
        if head == "SYNC" || head == "IMPORT" { return .writes }
        if ["ENROLL", "GRANT", "ISSUE"].contains(head) { return .admin }
        if r.action.hasPrefix("Query") { return .queries }
        if r.action.hasPrefix("Action") { return .actions }
        if r.action.hasPrefix("Write") { return .writes }
        // The core's wording for an action it decided: `<status> · <console>.<verb>`.
        if ["applied", "refused", "conflict"].contains(head) {
            let name = r.action.components(separatedBy: " · ").last ?? ""
            let queries = ["midgard.query", "midgard.searchAround", "midgard.simulatePolicy"]
            return queries.contains(name) ? .queries : .actions
        }
        return nil
    }

    public static func matches(_ r: AuditRecord, _ c: Category) -> Bool {
        switch c {
        case .all: return true
        case .agent: return r.actor.hasPrefix("agent:")
        default: return category(of: r) == c
        }
    }

    public var rows: [MidgardRow<AuditRecord>] {
        let q = query.trimmingCharacters(in: .whitespaces).lowercased()
        return records.filter {
            Self.matches($0.item, category)
                && (q.isEmpty || $0.item.actor.lowercased().contains(q) || $0.item.resource.lowercased().contains(q))
        }
    }

    public var total: Int { records.count }
    public var signed: Int { records.filter { $0.signature == .verified || $0.signature == .device }.count }
    public var signedPercent: Int { total == 0 ? 100 : Int((100 * Double(signed) / Double(total)).rounded()) }

    public var currentIndex: Int? {
        let r = rows
        return r.isEmpty ? nil : min(selected, r.count - 1)
    }

    public var current: MidgardRow<AuditRecord>? { currentIndex.map { rows[$0] } }

    public func setCategory(_ c: Category) {
        category = c
        selected = 0
    }

    /// The lineage trace of the selected record: the one before it (what caused it),
    /// itself, and the one after it (what it led to), among the rows shown.
    public var trace: [(String, String)] {
        guard let i = currentIndex else { return [] }
        let r = rows
        let e = r[i].item
        return [
            ("↑ caused by", i < r.count - 1 ? "#\(r[i + 1].item.id) · \(r[i + 1].item.action)" : "origin"),
            ("this event", "\(e.action) → \(e.resource)"),
            ("↓ leads to", i > 0 ? "#\(r[i - 1].item.id) · \(r[i - 1].item.action)" : "chain head"),
        ]
    }

    /// The records shown, as the JSON Export chain saves (the web's `AuditRecord` list),
    /// each with the signature status the core decided.
    public func exportJSON() -> Data {
        let list: [[String: String]] = rows.map {
            ["id": $0.item.id, "actor": $0.item.actor, "action": $0.item.action, "resource": $0.item.resource,
             "ts": $0.item.ts, "pqSignature": Self.signatureText($0.signature), "marking": $0.marking.label]
        }
        return (try? JSONSerialization.data(withJSONObject: list, options: [.prettyPrinted, .sortedKeys])) ?? Data()
    }

    /// `audit-chain-<time>.json`, as the web names the download.
    public static func exportName(_ date: Date = Date()) -> String {
        let f = ISO8601DateFormatter()
        let stamp = f.string(from: date).replacingOccurrences(of: ":", with: "-")
        return "audit-chain-\(stamp).json"
    }

    /// A record's signature as the screens say it: the web's `ML-DSA ✓` only for a
    /// signature the core checked.
    public static func signatureText(_ s: SignatureStatus?) -> String {
        switch s {
        case .verified?, .device?: return "ML-DSA ✓"
        default: return "UNSIGNED"
        }
    }
}

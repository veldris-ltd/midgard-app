import Foundation
@preconcurrency import MidgardCore

/// Object Fabric (`ObjectFabricScreen.tsx`): the registry's object types, and actions on
/// one object of the selected type at a time. A human-gated action is requested here
/// (`midgard.requestApproval`), approved by a second persona (`midgard.decideApproval`),
/// and only then run by the requester with that approval's id as the gate token
/// (`midgard.applyAction`); every action, request and decision shows its outcome or the
/// enclosure's refusal, as the web console does. The audit lineage under the table is
/// `midgard.audit`, newest first. [DD-1] [NFR-AI-1] [NFR-SEC-2] [FR-C-4]
@MainActor
public final class ObjectFabricModel: ObservableObject {
    /// The registry views the gateway does not serve yet, each with what it would show.
    public static let unservedTabs: [Int: String] = [
        1: "Interfaces are not served by this gateway yet. Object types are declared in the ontology file, which has no interfaces section.",
        2: "Shared properties are not served by this gateway yet. Each object type lists its own properties.",
    ]

    @Published public private(set) var types: [MidgardRow<MidgardObjectType>] = []
    @Published public private(set) var typesWithheld = 0
    @Published public private(set) var objects: [MidgardRow<MidgardObjectInstance>] = []
    @Published public private(set) var approvals: [MidgardRow<MidgardApproval>] = []
    @Published public private(set) var audit: [MidgardRow<AuditRecord>] = []
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var tab = 0
    @Published public private(set) var selected = 0
    @Published public private(set) var target = ""
    @Published public private(set) var notice: MidgardNotice?
    @Published public private(set) var busy = false

    private let app: AppModel
    private var token = 0

    public init(app: AppModel) {
        self.app = app
    }

    private var me: String { app.signedInPersona?.id ?? "" }

    public var selectedType: MidgardObjectType? { types.indices.contains(selected) ? types[selected].item : nil }
    public var selectedMarking: MarkingChip? { types.indices.contains(selected) ? types[selected].marking : nil }

    /// The selected type's objects this persona may act on.
    public var instances: [MidgardObjectInstance] {
        guard let t = selectedType else { return [] }
        return objects.map(\.item).filter { $0.typeId == t.apiName }
    }

    public var picked: MidgardObjectInstance? { instances.first { $0.id == target } }

    /// The approvals on objects of the selected type.
    public var typeApprovals: [MidgardApproval] {
        guard let t = selectedType else { return [] }
        return approvals.map(\.item).filter { $0.target.hasPrefix("\(t.apiName)/") }
    }

    public func isMine(_ a: MidgardApproval) -> Bool { a.requester == me }

    /// `1,445` → 1445, summed over the types shown.
    public var instanceTotal: Int {
        types.reduce(0) { $0 + (Int($1.item.instanceCount.filter(\.isNumber)) ?? 0) }
    }

    /// The share of the lineage rows whose signature the core checked.
    public var signedShare: String {
        guard !audit.isEmpty else { return "—" }
        let signed = audit.filter { $0.signature == .verified || $0.signature == .device }.count
        return "\(Int((100 * Double(signed) / Double(audit.count)).rounded()))%"
    }

    public func load() async {
        let (types, withheld) = await app.midgardRows("midgard.objectTypes") {
            if case let .midgardObjectType(item) = $0 { return item }
            return nil
        }
        self.types = types
        typesWithheld = withheld
        if selected >= types.count { selected = 0 }
        await reloadObjects()
        await reloadApprovals()
        await reloadAudit()
        state = types.isEmpty && withheld == 0 ? .empty : .loaded
    }

    private func reloadObjects() async {
        objects = await app.midgardRows("midgard.objects") {
            if case let .midgardObjectInstance(item) = $0 { return item }
            return nil
        }.rows
        if !target.isEmpty, picked == nil { target = "" }
    }

    private func reloadApprovals() async {
        approvals = await app.midgardRows("midgard.approvals") {
            if case let .midgardApproval(item) = $0 { return item }
            return nil
        }.rows
    }

    private func reloadAudit() async {
        audit = Array(await app.midgardRows("midgard.audit") {
            if case let .auditRecord(item) = $0 { return item }
            return nil
        }.rows.prefix(8))
    }

    public func select(_ index: Int) {
        guard types.indices.contains(index) else { return }
        selected = index
        target = ""
        notice = nil
    }

    public func setTarget(_ id: String) {
        target = id
        notice = nil
    }

    /// An action of the selected type pressed: requested for approval when it is
    /// human-gated, run at once otherwise.
    public func trigger(_ action: MidgardActionType) async {
        guard !target.isEmpty, !busy else { return }
        if action.humanGated {
            await request(action.name)
        } else {
            await run(action.name, on: target, token: nil)
        }
    }

    private func attempt(_ work: () async -> MidgardNotice) async {
        busy = true
        notice = nil
        notice = await work()
        busy = false
    }

    public func request(_ action: String) async {
        let on = target
        await attempt {
            let outcome = await app.midgardRun("midgard.requestApproval", "midgard.objects/\(on)", [("actionType", action)])
            await reloadApprovals()
            await reloadAudit()
            if let refused = MidgardNotice.unless(outcome) { return refused }
            guard case let .applied(_, result) = outcome, let a = midgardDecode(MidgardApprovalAnswer.self, result) else {
                return MidgardNotice(.danger, "The enclosure's answer did not decode.")
            }
            return MidgardNotice(.secure, "\(a.id) REQUESTED · \(action) · \(a.target) · WAITING FOR A SECOND PERSON")
        }
    }

    public func decide(_ approval: MidgardApproval, approve: Bool) async {
        await attempt {
            let outcome = await app.midgardRun("midgard.decideApproval", "midgard.approvals/\(approval.id)",
                                               [("approve", approve ? "true" : "false")])
            await reloadApprovals()
            await reloadAudit()
            if let refused = MidgardNotice.unless(outcome) { return refused }
            guard case let .applied(_, result) = outcome, let a = midgardDecode(MidgardApprovalAnswer.self, result) else {
                return MidgardNotice(.danger, "The enclosure's answer did not decode.")
            }
            return MidgardNotice(.secure, "\(a.id) \(a.status.uppercased()) · \(a.actionType) · \(a.target)")
        }
    }

    /// Run `action` on `on`, with an approval's id as the human-gate token when it is
    /// gated. Only this call, from the operator's press, ever sends a token.
    public func run(_ action: String, on: String, token: String?) async {
        var params = [("actionType", action)]
        if let token { params.append(("humanGateToken", token)) }
        await attempt {
            let outcome = await app.midgardRun("midgard.applyAction", "midgard.objects/\(on)", params)
            await reloadObjects()
            await reloadApprovals()
            await reloadAudit()
            if let refused = MidgardNotice.unless(outcome) { return refused }
            guard case let .applied(auditId, _) = outcome else { return MidgardNotice(.danger, "not applied") }
            return MidgardNotice(.secure, "\(action) · \(on) · APPLIED · AUDIT #\(auditId ?? "—")")
        }
    }
}

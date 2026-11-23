import Foundation
@preconcurrency import MidgardCore

// The shared parts of the Midgard console's screen models: a collection's rows as a
// screen reads them (withheld rows counted, never shown), a list screen's model, and
// running one of the console's actions through the core (request, sync, status), as
// the web console calls its gateway. Every call goes through `AppModel`, so the core
// decides clearance, markings and auditing. [FR-C-1] [FR-C-4] [AC-C-13]

/// One row a screen shows: its id, its marking and its typed read model.
public struct MidgardRow<Item> {
    public let id: String
    public let marking: MarkingChip
    public let item: Item
    /// For an audit row, what the core's signature check found.
    public let signature: SignatureStatus?
}

/// A screen's loading state.
public enum MidgardLoad: Equatable {
    case loading
    case loaded
    /// Nothing to show (and nothing withheld).
    case empty
}

extension AppModel {
    /// The rows of `collection` this persona may see, as `pick` reads them, and how many
    /// are withheld above the persona's clearance.
    func midgardRows<Item>(_ collection: String, _ pick: (RowItem) -> Item?) async -> (rows: [MidgardRow<Item>], withheld: Int) {
        let rows = await rows(collection)
        let shown: [MidgardRow<Item>] = rows.compactMap { row in
            guard let id = row.id, let item = row.item.flatMap(pick) else { return nil }
            return MidgardRow(id: id, marking: row.marking, item: item, signature: row.signature)
        }
        return (shown, rows.filter(\.withheld).count)
    }

    /// The signed-in persona, if any.
    var signedInPersona: PersonaInfo? {
        if case let .unlocked(persona) = phase { return persona }
        return nil
    }
}

/// What one of the console's actions came to.
public enum MidgardOutcome: Equatable {
    /// Applied by the enclosure: its audit id and its result (canonical JSON).
    case applied(auditId: String?, result: String)
    /// Refused, with the enclosure's reason.
    case refused(String)
    /// The item changed under it (decided twice).
    case conflict(String)
    /// Queued on this device until the enclosure is reachable.
    case queued
    /// The core would not make the action (with its message).
    case failed(String)

    /// Why a computed read is not run offline: it runs in the enclosure, and a read is
    /// never queued.
    public static let offlineRead = "Offline: this runs in the enclosure and is not queued. Go online and run it again."
}

extension AppModel {
    /// Make an action, sync, and say what became of it. [FR-C-4]
    func midgardRun(_ verb: String, _ target: String, _ params: [(String, String)] = []) async -> MidgardOutcome {
        // Connected: the gateway runs Midgard's and Admin's, and audits them; the other
        // consoles are not served by it and keep acting on the sample data.
        if userData.live != nil, verb.hasPrefix("midgard.") || verb.hasPrefix("admin.") {
            return await userData.liveRun(verb, target, params)
        }
        let input = ActionInput(verb: verb, target: target, params: params.map { KeyValue(key: $0.0, value: $0.1) })
        guard let id = await requestAction(input) else {
            return .failed(message ?? "The action could not be made.")
        }
        if !isOffline { await syncNow() }
        guard let status = await actionStatus(id) else { return .failed(message ?? "The action's state is unknown.") }
        switch status.state {
        case .applied: return .applied(auditId: status.auditId, result: status.result ?? "{}")
        case .refused: return .refused(status.reason ?? "refused")
        case .conflict: return .conflict(status.reason ?? "conflict")
        default: return .queued
        }
    }
}

/// Decode an action's result.
func midgardDecode<T: Decodable>(_ type: T.Type, _ json: String) -> T? {
    try? JSONDecoder().decode(T.self, from: Data(json.utf8))
}

/// The web's wording for an approval's status now: an approved gate past its expiry
/// has expired (`status` in `MockGateway`).
public func midgardApprovalStatus(_ a: MidgardApproval, now: Date = Date()) -> String {
    if a.status == "approved", (a.expiresAt ?? 0) <= now.timeIntervalSince1970 { return "expired" }
    return a.status
}

/// A list screen's model (Identity, Keys, Compute, Observability, Resilience, SDK): the
/// collection's rows and the row selected for the inspector. [FR-C-1]
@MainActor
public final class MidgardListModel<Item>: ObservableObject {
    @Published public private(set) var rows: [MidgardRow<Item>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var selected = 0

    private let app: AppModel
    public let collection: String
    private let pick: (RowItem) -> Item?

    public init(app: AppModel, collection: String, pick: @escaping (RowItem) -> Item?) {
        self.app = app
        self.collection = collection
        self.pick = pick
    }

    public var current: Item? { rows.indices.contains(selected) ? rows[selected].item : nil }

    public func load() async {
        let (rows, withheld) = await app.midgardRows(collection, pick)
        self.rows = rows
        self.withheld = withheld
        if selected >= rows.count { selected = 0 }
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
    }
}

public extension MidgardListModel where Item == MidgardIdentity {
    static func identities(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "midgard.identities") {
            if case let .midgardIdentity(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == MidgardKeyRecord {
    static func keys(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "midgard.keys") {
            if case let .midgardKeyRecord(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == MidgardComputeJob {
    static func jobs(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "midgard.computeJobs") {
            if case let .midgardComputeJob(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == MidgardSloMetric {
    static func slos(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "midgard.observability") {
            if case let .midgardSloMetric(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == MidgardRolloutState {
    static func rollouts(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "midgard.resilience") {
            if case let .midgardRolloutState(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == MidgardRpcSurface {
    static func surfaces(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "midgard.contractSurface") {
            if case let .midgardRpcSurface(item) = $0 { return item }
            return nil
        }
    }
}

/// An object of a query's answer (`ObjectInstance`).
public struct MidgardFoundObject: Decodable, Equatable {
    public struct Marking: Decodable, Equatable {
        public let level: String
        public let compartments: [String]
    }

    public let id: String
    public let typeId: String
    public let propertyValues: [String: String]
    public let markings: [Marking]

    /// The properties in the web's order (the canonical JSON's: by name).
    public var properties: [(String, String)] { propertyValues.sorted { $0.key < $1.key }.map { ($0.key, $0.value) } }
}

/// `midgard.query`'s answer (`QueryResponse`, the audit id coming with the action).
public struct MidgardQueryAnswer: Decodable, Equatable {
    public let results: [MidgardFoundObject]
    public let total: Int
    public let offset: Int
    public let limit: Int
}

/// `midgard.searchAround`'s answer (`SearchGraph`, with the nodes withheld above the
/// persona counted).
public struct MidgardGraphAnswer: Decodable, Equatable {
    public struct Node: Decodable, Equatable {
        public let id: String
        public let label: String
        public let typeName: String
        public let marking: String
        public let hop: Int
        public let propertyValues: [String: String]
    }

    public struct Edge: Decodable, Equatable {
        public let from: String
        public let to: String
        public let label: String
    }

    public let seed: String
    public let nodes: [Node]
    public let edges: [Edge]
    public let truncated: Bool
    public let nodeLimit: Int
    public let seedObjects: Int
    public let withheld: Int
}

/// `midgard.simulatePolicy`'s answer (`PolicyDecision`).
public struct MidgardPolicyAnswer: Decodable, Equatable {
    public let decision: String
    public let matchedRule: String?
    public let reason: String
}

/// `midgard.requestApproval` and `midgard.decideApproval`'s answer (`Approval`).
public struct MidgardApprovalAnswer: Decodable, Equatable {
    public let id: String
    public let actionType: String
    public let target: String
    public let status: String
}

/// A marking word (`confidential`) as a chip.
public func midgardChip(_ level: String, compartments: [String] = []) -> MarkingChip {
    let chip: MarkingLevelChip
    switch level.lowercased() {
    case "confidential": chip = .confidential
    case "restricted": chip = .restricted
    case "internal": chip = .internal
    default: chip = .open
    }
    let label = level.uppercased() + (compartments.isEmpty ? "" : " // " + compartments.joined(separator: ", "))
    return MarkingChip(level: chip, label: label)
}

/// The notice an action leaves on screen (`Notice` in the web screens).
public struct MidgardNotice: Equatable {
    public enum Tone: Equatable { case secure, danger, warn }
    public let tone: Tone
    public let text: String

    public init(_ tone: Tone, _ text: String) {
        self.tone = tone
        self.text = text
    }

    /// The notice for an outcome that is not applied (nil when it is).
    static func unless(_ outcome: MidgardOutcome) -> MidgardNotice? {
        switch outcome {
        case .applied: return nil
        case let .refused(why), let .conflict(why), let .failed(why): return MidgardNotice(.danger, why)
        case .queued: return MidgardNotice(.warn, "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE")
        }
    }
}

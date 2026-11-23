import Foundation
@preconcurrency import MidgardCore

/// Search Around (`SearchAroundScreen.tsx`): pick a seed object and a hop depth, run the
/// traversal (`midgard.searchAround` through the core) and show the graph grouped by
/// hop with each node's own properties. What is shown is always the traversal that was
/// run: the heading names its seed, and a failed run shows the refusal instead of the
/// previous results. Nodes above the persona's level are withheld by the enclosure and
/// counted. [DD-1] [NFR-SEC-2] [FR-C-4]
@MainActor
public final class SearchAroundModel: ObservableObject {
    /// Node limits on offer; the gateway allows up to 500.
    public static let limits = [60, 200, 500]

    /// One traversal as it was run, so typing a new seed changes nothing until Run.
    public struct Traversal: Equatable {
        public var seedType: String
        public var seedId: String
        public var hops: Int
        public var limit: Int

        public var seed: String { "\(seedType)/\(seedId.isEmpty ? "*" : seedId)" }
    }

    @Published public private(set) var types: [MidgardObjectType] = []
    @Published public var seedType: String
    @Published public var seedId: String
    @Published public var hops: Int
    @Published public var limit: Int
    @Published public private(set) var run: Traversal
    @Published public private(set) var graph: MidgardGraphAnswer?
    @Published public private(set) var auditId: String?
    @Published public private(set) var error: String?
    @Published public private(set) var running = false
    @Published public var selectedId: String?

    private let app: AppModel

    /// `link` is how the screen was opened: "Explore instances" on a type (every object
    /// of it, one hop out) or "Open in Search-around" on one object; otherwise the web's
    /// own first traversal, `aircraft/AC-2214` at two hops.
    public init(app: AppModel, link: MidgardLink? = nil) {
        self.app = app
        var first = Traversal(seedType: "aircraft", seedId: "AC-2214", hops: 2, limit: Self.limits[0])
        switch link {
        case let .seed(seed)?:
            let parts = seed.split(separator: "/", maxSplits: 1).map(String.init)
            if parts.count == 2 { first = Traversal(seedType: parts[0], seedId: parts[1], hops: 2, limit: Self.limits[0]) }
        case let .exploreType(type)?:
            first = Traversal(seedType: type, seedId: "", hops: 1, limit: Self.limits[0])
        default: break
        }
        seedType = first.seedType
        seedId = first.seedId
        hops = first.hops
        limit = first.limit
        run = first
    }

    public var nodes: [MidgardGraphAnswer.Node] { graph?.nodes ?? [] }
    public var edges: [MidgardGraphAnswer.Edge] { graph?.edges ?? [] }
    public var hopLevels: [Int] { Array(Set(nodes.map(\.hop))).sorted() }
    public var node: MidgardGraphAnswer.Node? { nodes.first { $0.id == selectedId } ?? nodes.first }

    public func load() async {
        types = await app.midgardRows("midgard.objectTypes") {
            if case let .midgardObjectType(item) = $0 { return item }
            return nil
        }.rows.map(\.item)
        await traverse(run)
    }

    /// Run the traversal the form now describes.
    public func runTraversal() async {
        let t = Traversal(seedType: seedType, seedId: seedId.trimmingCharacters(in: .whitespaces), hops: hops, limit: limit)
        selectedId = nil
        await traverse(t)
    }

    private func traverse(_ t: Traversal) async {
        run = t
        running = true
        error = nil
        graph = nil
        defer { running = false }
        guard !app.isOffline else {
            error = MidgardOutcome.offlineRead
            return
        }
        var params = [("hops", String(t.hops)), ("limit", String(t.limit))]
        if !t.seedId.isEmpty { params.append(("seedId", t.seedId)) }
        switch await app.midgardRun("midgard.searchAround", "midgard.objectTypes/\(t.seedType)", params) {
        case let .applied(audit, result):
            graph = midgardDecode(MidgardGraphAnswer.self, result)
            auditId = audit
            if graph == nil { error = "The enclosure's answer did not decode." }
        case let .refused(why), let .conflict(why), let .failed(why):
            error = why == "no such target" ? "unknown seed object type: \(t.seedType)" : why
        case .queued:
            error = MidgardOutcome.offlineRead
        }
    }
}

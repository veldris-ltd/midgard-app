import Foundation
@preconcurrency import MidgardCore

// Forge's screen models, over the release's sample data and through the core only
// (`AppModel`, as the other consoles' are): every row keeps its marking, rows above the
// persona are withheld by count, and the actions are the core's ports of `ForgeMock`'s
// (`clients/core/src/demo/actions/forge.rs`): a pipeline build, a transform preview and
// its human-gated commit, each decided once and audited in `forge.audit`. Forge is not
// served by a gateway: connected to one, these screens keep the sample data. [FR-C-1]
// [FR-C-4] [FR-F-1] [FR-F-3] [FR-F-7] [AC-C-13]

public extension MidgardListModel where Item == ForgeConnector {
    static func connectors(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "forge.connectors") {
            if case let .forgeConnector(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == ForgeDataset {
    static func datasets(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "forge.datasets") {
            if case let .forgeDataset(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == ForgeHealthCheck {
    static func healthChecks(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "forge.healthChecks") {
            if case let .forgeHealthCheck(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == ForgeSchedule {
    static func schedules(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "forge.schedules") {
            if case let .forgeSchedule(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == ForgeProjection {
    static func projections(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "forge.projections") {
            if case let .forgeProjection(item) = $0 { return item }
            return nil
        }
    }
}

public extension ForgeDataset {
    /// The version before this one ("v41" for "v42"), as the inspector's diff heading
    /// names it.
    var previousVersion: String {
        guard let n = Int(version.dropFirst()) else { return "" }
        return "v\(n - 1)"
    }
}

/// A human-gate token made from one press: only an operator's press makes one, never a
/// handler on its own.
func forgeGateToken(_ press: Int) -> String {
    "HG-\(String(Int(Date().timeIntervalSince1970 * 1000), radix: 36))-\(press)"
}

// MARK: - Answers

/// `ForgeBuildResult`, the core's answer to `forge.runPipeline`.
public struct ForgeBuildAnswer: Decodable, Equatable {
    public struct Step: Decodable, Equatable {
        public let stage: String
        public let detail: String
        public let status: String
    }

    public let runId: String
    public let version: String
    public let durationLabel: String
    public let steps: [Step]
}

/// `ForgePreviewResult`, the core's answer to `forge.previewTransform`.
public struct ForgePreviewAnswer: Decodable, Equatable {
    public struct Diff: Decodable, Equatable {
        public struct Hunk: Decodable, Equatable {
            public let op: String
            public let text: String
        }

        public let summary: String
        public let rowsIn: Int
        public let rowsOut: Int
        public let expectations: String
        public let durationLabel: String
        public let hunks: [Hunk]
    }

    public let diff: Diff
}

/// `forge.applyTransform`'s answer (the mock's `ForgeApplyResult` but its audit id,
/// which comes with the action).
public struct ForgeCommitAnswer: Decodable, Equatable {
    public let version: String
    public let applied: Bool
}

// MARK: - Pipeline builder

/// A pipeline's graph as this device draws and edits it: the enclosure's nodes and
/// edges to start with; moved, renamed, added or connected here only. The enclosure has
/// no graph write in this release (nor has the web mock), so an edit is a draft on this
/// device, never sent, and a build runs the enclosure's own graph. [FR-F-1]
public struct ForgeGraph: Equatable, Sendable {
    public struct Node: Equatable, Identifiable, Sendable {
        public let id: String
        public var kind: String
        public var label: String
        public var icon: String
        public var x: Double
        public var y: Double
        public var status: String
        public var meta: String
    }

    public struct Edge: Equatable, Hashable, Sendable {
        public let from: String
        public let to: String
    }

    public var nodes: [Node]
    public var edges: [Edge]

    public init(nodes: [Node], edges: [Edge]) {
        self.nodes = nodes
        self.edges = edges
    }

    init(_ p: ForgePipeline) {
        nodes = p.nodes.map {
            Node(id: $0.id, kind: $0.kind, label: $0.label, icon: $0.icon, x: $0.x, y: $0.y, status: $0.status, meta: $0.meta)
        }
        edges = p.edges.map { Edge(from: $0.from, to: $0.to) }
    }

    /// The web's grid: 250 × 150 per column and row, nodes 190 × 84, 28 pt padding.
    public static let column: Double = 250
    public static let row: Double = 150
    public static let nodeWidth: Double = 190
    public static let nodeHeight: Double = 84
    public static let pad: Double = 28

    /// The content box the nodes need (the web's `graphW` × `graphH`).
    public var size: CGSize {
        let maxX = nodes.map(\.x).max() ?? 0
        let maxY = nodes.map(\.y).max() ?? 0
        return CGSize(width: Self.pad * 2 + max(0, maxX) * Self.column + Self.nodeWidth,
                      height: Self.pad * 2 + max(0, maxY) * Self.row + Self.nodeHeight)
    }

    /// A node's top-left corner in the content box.
    public static func origin(_ n: Node) -> CGPoint {
        CGPoint(x: pad + n.x * column, y: pad + n.y * row)
    }

    /// The node kinds in the web's order, with their default icons.
    public static let kinds: [(kind: String, icon: String)] = [
        ("source", "database"), ("transform", "workflow"), ("dataset", "table"), ("projection", "boxes"),
    ]
}

/// Pipeline Builder (`PipelineBuilderScreen.tsx`): the pipelines as tags, the selected
/// one's graph and the selected node's inspector, and "Run build", which sends the
/// pipeline to the core's port of `runPipeline` and replays the staged steps it returns
/// into the build log and meter, 560 ms apart, as the web screen does. The graph can be
/// viewed and a node's settings edited on both apps; on the Mac nodes can also be moved,
/// added, connected and removed, and a new graph drawn. Those edits stay on this device
/// (`ForgeGraph`). [FR-F-1] [FR-F-3] [FR-F-7]
@MainActor
public final class ForgePipelineModel: ObservableObject {
    @Published public private(set) var rows: [MidgardRow<ForgePipeline>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    /// The pipeline tag chosen; `nil` with a new graph drawn on this device open.
    @Published public private(set) var pipelineIndex: Int? = 0
    @Published public var selectedNode = ""
    @Published public private(set) var log: [String] = ["> forge build — awaiting selection"]
    @Published public private(set) var building = false
    @Published public private(set) var progress = 0
    @Published public private(set) var phase = "Idle"
    @Published public private(set) var lastRun: ForgeBuildAnswer?
    @Published public private(set) var auditId: String?
    @Published public private(set) var message: String?
    /// The graphs as drawn on this device, by pipeline id (and `ForgePipelineModel.newId`).
    @Published public private(set) var drafts: [String: ForgeGraph] = [:]
    /// A node chosen to connect from (the Mac's Connect): the next node chosen is its
    /// target.
    @Published public var connectFrom: String?

    /// The pause between two replayed steps (the web's 560 ms); 0 in tests.
    public var stepDelay: UInt64 = 560_000_000
    /// The id under which a new graph drawn on this device is kept.
    public static let newId = "draft:new"

    private let app: AppModel
    private var replay: Task<Void, Never>?

    public init(app: AppModel) {
        self.app = app
    }

    public var pipeline: ForgePipeline? {
        guard let i = pipelineIndex, rows.indices.contains(i) else { return nil }
        return rows[i].item
    }

    public var pipelineRow: MidgardRow<ForgePipeline>? {
        guard let i = pipelineIndex, rows.indices.contains(i) else { return nil }
        return rows[i]
    }

    /// Whether the new graph drawn on this device is open.
    public var drawingNew: Bool { pipelineIndex == nil }

    private var key: String? { drawingNew ? Self.newId : pipeline?.id }

    /// The graph shown: this device's draft of it, or the enclosure's.
    public var graph: ForgeGraph {
        if let key, let draft = drafts[key] { return draft }
        return pipeline.map(ForgeGraph.init) ?? ForgeGraph(nodes: [], edges: [])
    }

    /// Whether the graph shown differs from the enclosure's.
    public var edited: Bool {
        guard let key, let draft = drafts[key] else { return false }
        return pipeline.map { draft != ForgeGraph($0) } ?? true
    }

    public var node: ForgeGraph.Node? { graph.nodes.first { $0.id == selectedNode } }

    /// The header's context line (`PIPELINES / <context>`).
    public var context: String { "PIPELINES / \(drawingNew ? "NEW GRAPH" : pipeline?.context ?? "BUILDER")" }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("forge.pipelines") {
            if case let .forgePipeline(item) = $0 { return item }
            return nil
        }
        self.rows = rows
        self.withheld = withheld
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
        if let i = pipelineIndex, i >= rows.count { pipelineIndex = rows.isEmpty ? nil : 0 }
        reset()
    }

    /// The node the web selects first: the dataset, else the first.
    private func defaultNode() -> String {
        graph.nodes.first { $0.kind == "dataset" }?.id ?? graph.nodes.first?.id ?? ""
    }

    private func reset() {
        replay?.cancel()
        building = false
        selectedNode = defaultNode()
        connectFrom = nil
        message = nil
        auditId = nil
        lastRun = nil
        progress = 0
        phase = "Idle"
        if let p = pipeline, !drawingNew {
            let last = p.versions.first
            log = ["> forge build \(p.name) --branch \(p.branch)",
                   "# last build \(last?.ts ?? "—") · \(last?.status ?? "—") · \(p.currentVersion)"]
        } else {
            log = ["> forge build — awaiting selection"]
        }
    }

    public func choose(_ index: Int) {
        guard rows.indices.contains(index) else { return }
        pipelineIndex = index
        reset()
    }

    // MARK: Build

    /// Why "Run build" cannot be pressed now, if it cannot.
    public var runDisabled: String? {
        if building { return "A build is running" }
        if drawingNew { return "A graph drawn on this device is not in the enclosure: only the enclosure's pipelines are built" }
        if pipeline == nil { return "No pipeline is selected" }
        return nil
    }

    /// Build the selected pipeline: the core's `forge.runPipeline`, then its steps
    /// replayed into the log and meter.
    public func run() async {
        guard runDisabled == nil, let p = pipeline else { return }
        building = true
        progress = 0
        phase = "Resolving"
        message = nil
        log.append("> forge build \(p.name) --branch \(p.branch)")
        switch await app.midgardRun("forge.runPipeline", "forge.pipelines/\(p.id)") {
        case let .applied(audit, result):
            guard let answer = midgardDecode(ForgeBuildAnswer.self, result) else {
                building = false
                phase = "Idle"
                message = "The build's answer could not be read."
                return
            }
            auditId = audit
            lastRun = answer
            await replaySteps(answer)
        case let .refused(reason), let .conflict(reason), let .failed(reason):
            building = false
            phase = "Idle"
            message = reason
            log.append("# refused · \(reason)")
        case .queued:
            building = false
            phase = "Queued"
            message = "Queued on this device: the build runs when the enclosure is reachable."
            log.append("# queued · offline")
        }
    }

    private func replaySteps(_ answer: ForgeBuildAnswer) async {
        let delay = stepDelay
        let task = Task { @MainActor [weak self] in
            for (i, step) in answer.steps.enumerated() {
                if delay > 0 { try? await Task.sleep(nanoseconds: delay) }
                guard let self, !Task.isCancelled else { return }
                let pad = (step.stage + "        ").prefix(9)
                self.log.append(step.stage == "done" ? "# \(step.detail)" : "\(pad)\(step.detail)")
                self.progress = Int((Double(i + 1) / Double(answer.steps.count) * 100).rounded())
                self.phase = step.stage == "done" ? "Sealed" : step.stage.prefix(1).uppercased() + step.stage.dropFirst()
            }
            self?.building = false
            if let audit = self?.auditId { self?.log.append("# audit #\(audit) · signed on this device") }
        }
        replay = task
        await task.value
    }

    // MARK: Graph edits (this device only)

    private func edit(_ change: (inout ForgeGraph) -> Void) {
        guard let key else { return }
        var g = graph
        change(&g)
        drafts[key] = g
    }

    /// A node's settings: its label and the line under it.
    public func setNode(_ id: String, label: String? = nil, meta: String? = nil) {
        edit { g in
            guard let i = g.nodes.firstIndex(where: { $0.id == id }) else { return }
            if let label { g.nodes[i].label = label }
            if let meta { g.nodes[i].meta = meta }
        }
    }

    /// Move a node to a grid position (columns and rows, not below 0).
    public func move(_ id: String, x: Double, y: Double) {
        edit { g in
            guard let i = g.nodes.firstIndex(where: { $0.id == id }) else { return }
            g.nodes[i].x = max(0, (x * 4).rounded() / 4)
            g.nodes[i].y = max(0, (y * 4).rounded() / 4)
        }
    }

    /// Add a node of `kind` in the next free column (the Mac's Add).
    public func addNode(kind: String) {
        let icon = ForgeGraph.kinds.first { $0.kind == kind }?.icon ?? "workflow"
        var newId = ""
        edit { g in
            var n = g.nodes.count + 1
            while g.nodes.contains(where: { $0.id == "n\(n)" }) { n += 1 }
            newId = "n\(n)"
            let column = Double(ForgeGraph.kinds.firstIndex { $0.kind == kind } ?? 0)
            let taken = g.nodes.filter { abs($0.x - column) < 0.5 }.map(\.y)
            let y = (taken.max() ?? -1) + 1
            g.nodes.append(ForgeGraph.Node(id: newId, kind: kind, label: "new_\(kind)", icon: icon, x: column, y: y,
                                           status: "healthy", meta: "draft · this device"))
        }
        if !newId.isEmpty { selectedNode = newId }
    }

    /// Connect `from` to `to` (once; never a node to itself).
    public func connect(_ from: String, _ to: String) {
        guard from != to else { return }
        edit { g in
            let e = ForgeGraph.Edge(from: from, to: to)
            if !g.edges.contains(e) { g.edges.append(e) }
        }
        connectFrom = nil
    }

    /// Choose a node: in Connect, the target of the edge; otherwise the inspected one.
    public func tap(_ id: String) {
        if let from = connectFrom {
            connect(from, id)
        } else {
            selectedNode = id
        }
    }

    /// Remove a node and its edges.
    public func remove(_ id: String) {
        edit { g in
            g.nodes.removeAll { $0.id == id }
            g.edges.removeAll { $0.from == id || $0.to == id }
        }
        if selectedNode == id { selectedNode = defaultNode() }
    }

    /// Put the graph back as the enclosure has it.
    public func discard() {
        guard let key else { return }
        drafts[key] = nil
        if drawingNew { pipelineIndex = rows.isEmpty ? nil : 0 }
        reset()
    }

    /// Open a new, empty graph on this device (the Mac's New graph).
    public func drawNew() {
        pipelineIndex = nil
        if drafts[Self.newId] == nil { drafts[Self.newId] = ForgeGraph(nodes: [], edges: []) }
        reset()
    }
}

// MARK: - Transform IDE

/// Transform IDE (`TransformIdeScreen.tsx`): the transform's files and code; "Preview
/// run" and "Commit v43" both send the code to the core's port of `previewTransform`
/// (the web's pair do the same), which renders the sandboxed diff and opens the human
/// gate; Authorise sends `forge.applyTransform` with a token made from that press, and
/// the gate shows the audit id the core gives. Without a token the core refuses
/// ("Human gate required to commit \"fuse_positions\""). Reject is the operator's own
/// decision, shown on screen and not sent, as on the web. The code can be edited here
/// before a preview (a draft on this device): the preview and the commit carry it.
/// [FR-F-1] [FR-F-3] [FR-F-7] [NFR-AI-1]
@MainActor
public final class ForgeTransformModel: ObservableObject {
    public enum Gate: Equatable {
        case pending, queued, authorised(String?), rejected
    }

    @Published public private(set) var row: MidgardRow<ForgeTransform>?
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var fileIndex = 0
    /// The code as edited on this device (the transform's own until edited).
    @Published public var code = ""
    @Published public private(set) var diff: ForgePreviewAnswer.Diff?
    @Published public private(set) var gate: Gate?
    @Published public private(set) var busy = false
    @Published public private(set) var message: String?
    @Published public private(set) var committed: ForgeCommitAnswer?

    private let app: AppModel
    private var presses = 0

    public init(app: AppModel) {
        self.app = app
    }

    public var transform: ForgeTransform? { row?.item }

    /// Whether the code differs from the transform's.
    public var edited: Bool { transform.map { code != $0.code.joined(separator: "\n") } ?? false }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("forge.transform") {
            if case let .forgeTransform(item) = $0 { return item }
            return nil
        }
        row = rows.first
        self.withheld = withheld
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
        code = transform?.code.joined(separator: "\n") ?? ""
        fileIndex = 0
        diff = nil
        gate = nil
        message = nil
    }

    /// Put the code back as the transform has it.
    public func revert() {
        code = transform?.code.joined(separator: "\n") ?? ""
    }

    /// Preview the code in the sandbox: the diff, then the gate.
    public func preview() async {
        guard transform != nil, !busy else { return }
        busy = true
        defer { busy = false }
        message = nil
        switch await app.midgardRun("forge.previewTransform", "forge.transform/transform", [("sql", code)]) {
        case let .applied(_, result):
            guard let answer = midgardDecode(ForgePreviewAnswer.self, result) else {
                message = "The preview's answer could not be read."
                return
            }
            diff = answer.diff
            gate = .pending
        case let .refused(reason), let .conflict(reason), let .failed(reason):
            message = reason
        case .queued:
            message = "Queued on this device: the preview runs when the enclosure is reachable."
        }
    }

    /// Authorise the commit, with a token from this press.
    public func authorise() async {
        await commit(token: true)
    }

    /// Send the commit; `token` false sends none (the gate's refusal, for tests).
    func commit(token: Bool) async {
        guard transform != nil, !busy else { return }
        busy = true
        defer { busy = false }
        message = nil
        presses += 1
        // The human gate is this press: only this call, from it, sends a token.
        var params = [("sql", code)]
        if token { params.append(("humanGateToken", forgeGateToken(presses))) }
        switch await app.midgardRun("forge.applyTransform", "forge.transform/transform", params) {
        case let .applied(auditId, result):
            committed = midgardDecode(ForgeCommitAnswer.self, result)
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
}

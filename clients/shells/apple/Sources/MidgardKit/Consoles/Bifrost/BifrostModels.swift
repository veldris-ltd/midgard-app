import Foundation
@preconcurrency import MidgardCore

// Bifrost's screen models, over the release's sample data and through the core only
// (`AppModel`, as the other consoles' are): every row keeps its marking, rows above the
// persona are withheld by count, and the actions are the core's ports of
// `BifrostMock`'s (`clients/core/src/demo/actions/bifrost.rs`): a promotion step and a
// rollback, each for the release-manager role only, decided once and audited in
// `bifrost.audit`. Bifrost is not served by a gateway: connected to one, these screens
// keep the sample data. [FR-C-1] [FR-C-4] [FR-B-2] [FR-B-3] [FR-B-4] [FR-B-5] [AC-C-13]

public extension MidgardListModel where Item == BifrostArtefact {
    static func artefacts(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "bifrost.artefacts") {
            if case let .bifrostArtefact(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == BifrostChannel {
    static func channels(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "bifrost.channels") {
            if case let .bifrostChannel(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == BifrostBundle {
    static func bundles(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "bifrost.bundles") {
            if case let .bifrostBundle(item) = $0 { return item }
            return nil
        }
    }
}

public extension MidgardListModel where Item == BifrostFleetNode {
    static func fleet(_ app: AppModel) -> MidgardListModel {
        MidgardListModel(app: app, collection: "bifrost.fleet") {
            if case let .bifrostFleetNode(item) = $0 { return item }
            return nil
        }
    }
}

/// A number as the web prints it (`String(n)`: no decimals when whole).
func bifrostCount(_ v: Double) -> String {
    v.truncatingRemainder(dividingBy: 1) == 0 ? String(format: "%.0f", v) : String(v)
}

/// A ring's rolled share, as the web's `Math.round((done / total) * 100)`.
public func bifrostPercent(done: Double, total: Double) -> Int {
    total > 0 ? Int((done / total * 100).rounded(.toNearestOrAwayFromZero)) : 0
}

/// The Artefacts filter tags (`FILTERS`). The web's tags only highlight: they filter
/// nothing, and neither do these.
public let bifrostArtefactFilters = ["all", "images", "bundles", "weights", "blocked"]

// MARK: - Release Console

/// `BifrostPromotion`, the core's answer to `bifrost.promoteRelease` (its audit id comes
/// with the action); `step` is the mock's own record of it.
public struct BifrostPromotionAnswer: Decodable, Equatable {
    public let ring: String
    public let progress: Int
    public let healthGate: String
    public let step: String
}

/// Release Console (`ReleaseConsoleScreen.tsx`): the first release, the rings with their
/// meters, the fleet, and Promote, which advances the broad ring (PROD-EU) one
/// health-gated step at a time through the core's port of `promoteRelease`, until it is
/// fully rolled or the gate fails, as the web's loop does: each step's audit id is the
/// LAST STEP, its progress the ring's meter, its gate the pill. A persona without the
/// release-manager role is refused by the core, and the refusal is shown; offline the
/// step is queued and the loop stops. [FR-B-2] [FR-B-6]
@MainActor
public final class BifrostReleaseModel: ObservableObject {
    @Published public private(set) var releases: [MidgardRow<BifrostRelease>] = []
    @Published public private(set) var rings: [MidgardRow<BifrostRing>] = []
    @Published public private(set) var fleet: [MidgardRow<BifrostFleetNode>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public private(set) var gate = "watch"
    @Published public private(set) var promoting = false
    @Published public private(set) var lastAudit: String?
    /// Every step this screen made, oldest first: the mock's record of it and its audit id.
    @Published public private(set) var steps: [(step: String, auditId: String?)] = []
    @Published public private(set) var message: String?

    /// The ring Promote advances (`PROMOTE_STAGE`).
    public static let promoteStage = "broad"
    /// The pause between two steps (the mock's 180 ms latency, so the meter is seen to
    /// move); 0 in tests.
    public var stepDelay: UInt64 = 180_000_000

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public var release: BifrostRelease? { releases.first?.item }
    public var rolled: Int { Int(rings.reduce(0) { $0 + $1.item.done }) }
    public var capacity: Int { Int(rings.reduce(0) { $0 + $1.item.total }) }

    public func load() async {
        let (releases, hiddenReleases) = await app.midgardRows("bifrost.releases") {
            if case let .bifrostRelease(item) = $0 { return item }
            return nil
        }
        let (rings, hiddenRings) = await app.midgardRows("bifrost.rings") {
            if case let .bifrostRing(item) = $0 { return item }
            return nil
        }
        let (fleet, hiddenFleet) = await app.midgardRows("bifrost.fleet") {
            if case let .bifrostFleetNode(item) = $0 { return item }
            return nil
        }
        self.releases = releases
        self.rings = rings
        self.fleet = fleet
        withheld = hiddenReleases + hiddenRings + hiddenFleet
        state = releases.isEmpty && rings.isEmpty && withheld == 0 ? .empty : .loaded
        if !promoting, let r = releases.first?.item { gate = r.healthGate }
    }

    /// Why Promote cannot be pressed now, if it cannot.
    public var promoteDisabled: String? {
        if promoting { return "A promotion is in flight" }
        if release == nil { return releases.isEmpty && withheld > 0 ? "The release is above your clearance" : "No release" }
        return nil
    }

    /// Promote: one step after another until the ring is rolled or the gate fails.
    public func promote() async {
        guard let r = releases.first, !promoting else { return }
        promoting = true
        message = nil
        defer { promoting = false }
        for _ in 0..<100 {
            let outcome = await app.midgardRun("bifrost.promoteRelease", "bifrost.releases/\(r.id)",
                                               [("ring", Self.promoteStage)])
            switch outcome {
            case let .applied(auditId, result):
                guard let answer = midgardDecode(BifrostPromotionAnswer.self, result) else {
                    message = "The promotion's answer could not be read."
                    return
                }
                gate = answer.healthGate
                lastAudit = auditId
                steps.append((answer.step, auditId))
                if let i = rings.firstIndex(where: { $0.item.stage == answer.ring }) {
                    var ring = rings[i].item
                    ring.done = (Double(answer.progress) / 100 * ring.total).rounded(.toNearestOrAwayFromZero)
                    rings[i] = MidgardRow(id: rings[i].id, marking: rings[i].marking, item: ring, signature: nil)
                }
                if answer.progress >= 100 || answer.healthGate == "fail" { return }
                if stepDelay > 0 { try? await Task.sleep(nanoseconds: stepDelay) }
            case let .refused(reason), let .conflict(reason), let .failed(reason):
                message = reason
                return
            case .queued:
                message = "Queued on this device: the step is sent when the enclosure is reachable."
                return
            }
        }
    }
}

// MARK: - Rollback

/// `{ plan, scope, step }`, the core's answer to `bifrost.rollback`.
public struct BifrostRollbackAnswer: Decodable, Equatable {
    public let plan: String
    public let scope: String
    public let step: String
}

/// Rollback centre (`RollbackScreen.tsx`): the standing plans, the selected plan's
/// automatic triggers and health gate, Execute (the core's port of `rollback`, for the
/// release-manager role and an armed or available plan only; the audit id joins the
/// log) and Run drill, the web's canned rehearsal, played into the log at the web's
/// times (no gateway call, on the web or here). [FR-B-2]
@MainActor
public final class BifrostRollbackModel: ObservableObject {
    @Published public private(set) var rows: [MidgardRow<BifrostRollback>] = []
    @Published public private(set) var withheld = 0
    @Published public private(set) var state: MidgardLoad = .loading
    @Published public var selected = 0
    @Published public private(set) var drilling = false
    @Published public private(set) var executing = false
    @Published public private(set) var executedAudit: String?
    @Published public private(set) var message: String?
    @Published public private(set) var log = [
        "> bifrost rollback drill --plan release-2.5.0",
        "# last drill 2d ago · 41s to healthy",
    ]

    /// The drill's lines and when the web shows them (ms after the press).
    public static let drill: [(line: String, at: UInt64)] = [
        ("freeze     promotions paused", 400),
        ("revert     2.5.0 → 2.4.1 · ring order reversed", 1200),
        ("health     p99 OK · errors 0 · 38s", 2200),
        ("restore    promotions resumed · state intact", 3000),
        ("# drill complete · 41s to healthy · audit #R-2214", 3400),
    ]

    /// 1 plays the drill at the web's pace; 0 at once (tests).
    public var drillPace: UInt64 = 1

    private let app: AppModel

    public init(app: AppModel) {
        self.app = app
    }

    public var current: MidgardRow<BifrostRollback>? { rows.indices.contains(selected) ? rows[selected] : nil }

    /// The web's health gate: a plan runs only while armed or available.
    public var gated: Bool {
        guard let p = current?.item else { return true }
        return p.state != "secure" && p.state != "healthy"
    }

    /// The automatic triggers the selected plan shows (the web's filtered list).
    public var triggers: [String] {
        guard let p = current?.item else { return [] }
        return [
            ("p99 latency > 250ms · 3 min", p.kind == "release"),
            ("error rate > 0.1% · 3 min", p.kind == "release"),
            ("eval pass < gate", p.kind == "agent policy"),
            ("manual only", p.trigger == "manual only"),
        ].filter(\.1).map(\.0)
    }

    public func load() async {
        let (rows, withheld) = await app.midgardRows("bifrost.rollbacks") {
            if case let .bifrostRollback(item) = $0 { return item }
            return nil
        }
        self.rows = rows
        self.withheld = withheld
        state = rows.isEmpty && withheld == 0 ? .empty : .loaded
        if selected >= rows.count { selected = 0 }
    }

    public func select(_ index: Int) {
        guard rows.indices.contains(index) else { return }
        selected = index
        message = nil
    }

    /// Run drill: the web's canned rehearsal, line by line.
    public func runDrill() async {
        guard !drilling else { return }
        drilling = true
        defer { drilling = false }
        log.append("> bifrost rollback drill --plan release-2.5.0")
        var elapsed: UInt64 = 0
        for (line, at) in Self.drill {
            if drillPace > 0, at > elapsed {
                try? await Task.sleep(nanoseconds: (at - elapsed) * 1_000_000 * drillPace)
                elapsed = at
            }
            log.append(line)
        }
        if drillPace > 0 { try? await Task.sleep(nanoseconds: 200_000_000 * drillPace) }
    }

    /// Execute the selected plan through the core.
    public func execute() async {
        guard let p = current, !gated, !executing else { return }
        executing = true
        message = nil
        defer { executing = false }
        switch await app.midgardRun("bifrost.rollback", "bifrost.rollbacks/\(p.id)") {
        case let .applied(auditId, _):
            executedAudit = auditId
            log.append("> bifrost rollback execute --plan \(p.id)")
            log.append("# rollback complete · health-gated · audit \(auditId ?? "—")")
        case let .refused(reason), let .conflict(reason), let .failed(reason):
            message = reason
        case .queued:
            message = "Queued on this device: the rollback is sent when the enclosure is reachable."
        }
    }
}

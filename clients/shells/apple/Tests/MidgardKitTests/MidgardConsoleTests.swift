import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Midgard's screen models over the release's sample data, against the real core: the
/// registry and its withheld types, the dual-control object action across two personas
/// (request, self-approval refused, a second person's approval, run with the gate
/// token), Data search with its conditions and its offline last answer, Search-around's
/// graph and withheld nodes, the policy dry run, the audit ledger's filters and export,
/// and the list screens. The screens render in both modes. [FR-C-1] [FR-C-4] [AC-C-13]
/// [NFR-AI-1] [NFR-SEC-2] [DD-1]
@MainActor
final class MidgardConsoleTests: XCTestCase {
    private var dir: URL!

    override func setUp() async throws {
        dir = try temporaryDirectory()
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: dir)
    }

    private func signedIn(_ id: String) async throws -> AppModel {
        let secrets = InMemorySecretStore()
        let keyRoot = SoftwareKeyRoot(reason: "test", label: "Software key (test)", secrets: secrets,
                                      presence: AlwaysPresent())
        let app = AppModel(idleTimeout: 3600)
        await app.start(AppEnvironment(dataSource: .sample, edition: .release, storeDirectory: dir, secrets: secrets,
                                       keyRoot: keyRoot, device: DeviceInfo(ramMb: 8192, hasAccelerator: true),
                                       replicaId: "midgard-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    private func switchTo(_ app: AppModel, _ id: String) async throws {
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == id }))
        XCTAssertEqual(app.signedInPersona?.id, id)
    }

    func testTheRegistryAsTheWebShowsItAndWithheldByClearance() async throws {
        let app = try await signedIn("k.osei")
        let model = ObjectFabricModel(app: app)
        XCTAssertEqual(model.state, .loading)
        await model.load()
        XCTAssertEqual(model.state, .loaded)
        XCTAssertEqual(model.types.map(\.item.name), ["Aircraft", "Facility", "Sensor Feed", "Crew Member", "Maintenance Event"])
        XCTAssertEqual(model.instanceTotal, 66_093)
        XCTAssertEqual(model.types[0].marking.label, "CONFIDENTIAL // OPS-N")
        XCTAssertEqual(model.instances.map(\.id), (1000...1004).map { "aircraft/\($0)" })
        XCTAssertEqual(model.audit.count, 5)
        XCTAssertEqual(model.signedShare, "0%", "the sample's seeded rows carry no signature")
        model.tab = 1
        XCTAssertNotNil(ObjectFabricModel.unservedTabs[model.tab])

        // An analyst (restricted) sees Sensor Feed (confidential // SIG) withheld.
        try await switchTo(app, "k.rao")
        await model.load()
        XCTAssertFalse(model.types.contains { $0.item.name == "Sensor Feed" })
        XCTAssertFalse(model.types.contains { $0.item.name == "Aircraft" })
        XCTAssertEqual(model.typesWithheld, 2)
    }

    func testTheDualControlActionRunsWithASecondPersonsApproval() async throws {
        let app = try await signedIn("j.benjamin")
        let model = ObjectFabricModel(app: app)
        await model.load()
        model.setTarget("aircraft/1000")
        let ground = try XCTUnwrap(model.selectedType?.actions.first { $0.name == "Ground aircraft" })
        await model.trigger(ground)
        XCTAssertEqual(model.notice, MidgardNotice(.secure,
            "APR-1 REQUESTED · Ground aircraft · aircraft/1000 · WAITING FOR A SECOND PERSON"))
        let pending = try XCTUnwrap(model.typeApprovals.first)
        XCTAssertTrue(model.isMine(pending))
        XCTAssertEqual(midgardApprovalStatus(pending), "pending")
        XCTAssertEqual(model.audit.first?.item.id, "AUD-1043")
        XCTAssertEqual(model.audit.first?.signature, .device)

        // The requester cannot approve it.
        await model.decide(pending, approve: true)
        XCTAssertEqual(model.notice, MidgardNotice(.danger, "separation-of-duties: requester cannot approve own request"))

        // A second person approves it.
        try await switchTo(app, "k.osei")
        await model.load()
        let theirs = try XCTUnwrap(model.typeApprovals.first)
        XCTAssertFalse(model.isMine(theirs))
        await model.decide(theirs, approve: true)
        XCTAssertEqual(model.notice, MidgardNotice(.secure, "APR-1 APPROVED · Ground aircraft · aircraft/1000"))

        // Back to the requester, who runs it with the approval as the gate token.
        try await switchTo(app, "j.benjamin")
        await model.load()
        let approved = try XCTUnwrap(model.typeApprovals.first)
        XCTAssertEqual(midgardApprovalStatus(approved), "approved")
        await model.run(approved.actionType, on: approved.target, token: approved.id)
        let notice = try XCTUnwrap(model.notice)
        XCTAssertEqual(notice.tone, .secure)
        XCTAssertTrue(notice.text.hasPrefix("Ground aircraft · aircraft/1000 · APPLIED · AUDIT #AUD-"), notice.text)
        XCTAssertEqual(model.typeApprovals.first?.status, "used")

        // Without a gate the action is refused, with the mock's words.
        await model.run("Ground aircraft", on: "aircraft/1000", token: nil)
        XCTAssertEqual(model.notice, MidgardNotice(.danger, "Human gate required for action \"Ground aircraft\""))
    }

    func testOfflineAnApprovalRequestIsQueued() async throws {
        let app = try await signedIn("j.benjamin")
        let model = ObjectFabricModel(app: app)
        await model.load()
        await app.setOffline(true)
        model.setTarget("aircraft/1001")
        await model.request("Schedule maintenance")
        XCTAssertEqual(model.notice, MidgardNotice(.warn, "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE"))
    }

    func testDataSearchAppliesConditionsAndOfflineShowsTheLastAnswer() async throws {
        let app = try await signedIn("k.osei")
        let model = DataSearchModel(app: app)
        await model.load()
        XCTAssertEqual(model.type?.apiName, "aircraft")
        await model.run()
        XCTAssertEqual(model.total, 5)
        XCTAssertEqual(model.from, 1)
        XCTAssertEqual(model.to, 5)
        XCTAssertNotNil(model.auditId)

        // The sample's types declare no properties: a condition is on any text.
        model.addCondition()
        XCTAssertEqual(model.conditions, [.init(property: "*", op: "contains", value: "")])
        model.setCondition(0, value: "Standby")
        await model.run()
        XCTAssertEqual(model.total, 3)
        XCTAssertEqual(model.results.map(\.id), ["aircraft/1000", "aircraft/1002", "aircraft/1004"])
        XCTAssertEqual(DataSearchModel.describe(model.conditions[0]), "any text contains Standby")
        XCTAssertNil(model.offlineNote)

        // Offline: the same search shows its last answer and says so; a new one cannot run.
        await app.setOffline(true)
        await model.run()
        XCTAssertEqual(model.total, 3)
        XCTAssertNotNil(model.offlineNote)
        model.setCondition(0, value: "active")
        await model.run()
        XCTAssertNil(model.answer)
        XCTAssertNotNil(model.error)
        await app.setOffline(false)

        // Every type for text (the header's search).
        await model.search(text: "active")
        XCTAssertEqual(model.textHits?.count, 5)
        XCTAssertTrue(model.textHits?.allSatisfy { $0.total == 2 } ?? false)
        await model.search(text: "no such text")
        XCTAssertTrue(model.textHits?.allSatisfy { $0.total == 0 } ?? false)
    }

    func testSearchAroundRunsTheTraversalAndWithholdsAboveClearance() async throws {
        let app = try await signedIn("k.osei")
        let model = SearchAroundModel(app: app)
        await model.load()
        XCTAssertEqual(model.run.seed, "aircraft/AC-2214")
        XCTAssertEqual(model.nodes.count, 6)
        XCTAssertEqual(model.edges.count, 5)
        XCTAssertEqual(model.hopLevels, [0, 1, 2])
        XCTAssertEqual(model.node?.id, "aircraft/AC-2214")
        XCTAssertNil(model.error)

        // Explore instances: every object of a type, one hop out; k.rao sees the
        // confidential sensor feed withheld.
        try await switchTo(app, "k.rao")
        let explore = SearchAroundModel(app: app, link: .exploreType("crew_member"))
        await explore.load()
        XCTAssertEqual(explore.run.seed, "crew_member/*")
        XCTAssertEqual(explore.graph?.withheld, 1)
        XCTAssertFalse(explore.nodes.contains { $0.typeName == "Sensor Feed" })

        // A seed type above the persona is refused, and nothing is shown.
        explore.seedType = "aircraft"
        await explore.runTraversal()
        XCTAssertNil(explore.graph)
        XCTAssertNotNil(explore.error)
    }

    func testThePolicyDryRunIsSignedAndDenyByDefault() async throws {
        let app = try await signedIn("j.benjamin")
        let model = PoliciesModel(app: app)
        await model.load()
        XCTAssertEqual(model.rules.count, 4)
        XCTAssertEqual(model.purposes, ["operations", "analysis", "export"])
        XCTAssertEqual(model.subject, "j.benjamin")
        XCTAssertEqual(model.clearance, "CONFIDENTIAL")
        await model.evaluate()
        XCTAssertEqual(model.decision, MidgardPolicyAnswer(decision: "grant", matchedRule: "POL-CONF-OPS",
                                                           reason: "Granted by Confidential OPS-N read."))
        XCTAssertEqual(model.decided?.auditId, "AUD-1043")
        model.purpose = "export"
        await model.evaluate()
        XCTAssertEqual(model.decision?.decision, "deny")
        XCTAssertEqual(model.decision?.matchedRule, "POL-DENY-EXPORT")

        // k.rao reads one rule (restricted): the others are withheld.
        try await switchTo(app, "k.rao")
        await model.load()
        XCTAssertEqual(model.rules.map(\.item.id), ["POL-RESTRICT"])
        XCTAssertEqual(model.withheld, 3)
    }

    func testTheAuditLedgerFiltersAndExportsWhatIsShown() async throws {
        let app = try await signedIn("k.osei")
        let policies = PoliciesModel(app: app)
        await policies.load()
        await policies.evaluate()
        let model = AuditModel(app: app)
        await model.load()
        XCTAssertEqual(model.total, 6)
        XCTAssertEqual(model.signed, 1)
        XCTAssertEqual(model.signedPercent, 17)
        XCTAssertEqual(model.current?.item.id, "AUD-1043")
        XCTAssertEqual(AuditModel.signatureText(model.current?.signature), "ML-DSA ✓")
        XCTAssertEqual(model.trace.first?.0, "↑ caused by")
        XCTAssertEqual(model.trace.last?.1, "chain head")

        model.setCategory(.queries)
        XCTAssertEqual(model.rows.map(\.item.id), ["AUD-1043", "AUD-1041", "AUD-1039"])
        model.setCategory(.agent)
        XCTAssertEqual(model.rows.map(\.item.actor), ["agent:mimir/dispatch-01"])
        model.setCategory(.all)
        model.query = "FABRIC"
        XCTAssertEqual(model.rows.map(\.item.id), ["AUD-1039"])
        let exported = try JSONSerialization.jsonObject(with: model.exportJSON()) as? [[String: String]]
        XCTAssertEqual(exported?.map { $0["id"] }, ["AUD-1039"])
        XCTAssertEqual(exported?.first?["pqSignature"], "UNSIGNED")
        XCTAssertTrue(AuditModel.exportName().hasPrefix("audit-chain-"))
        model.query = "nothing like this"
        XCTAssertNil(model.current)
    }

    func testTheListScreensLoadAndCountWithheldRows() async throws {
        let app = try await signedIn("k.osei")
        let identities = MidgardListModel.identities(app)
        let keys = MidgardListModel.keys(app)
        let jobs = MidgardListModel.jobs(app)
        let slos = MidgardListModel.slos(app)
        let rollouts = MidgardListModel.rollouts(app)
        let surfaces = MidgardListModel.surfaces(app)
        await identities.load()
        await keys.load()
        await jobs.load()
        await slos.load()
        await rollouts.load()
        await surfaces.load()
        XCTAssertEqual(identities.rows.map(\.item.subject).first, "j.benjamin")
        XCTAssertEqual(keys.rows.count, 4)
        XCTAssertEqual(jobs.rows.map(\.item.state), ["running", "done", "queued", "failed"])
        XCTAssertEqual(slos.rows.count, 5)
        XCTAssertEqual(ObservabilityScreen.percent("99.97%"), 99.97)
        XCTAssertNil(ObservabilityScreen.percent("38ms"))
        XCTAssertEqual(rollouts.rows.filter { $0.item.healthGate == "watch" }.count, 1)
        XCTAssertEqual(surfaces.rows.reduce(0) { $0 + $1.item.rpcs.count }, 5)
        surfaces.selected = 1
        XCTAssertEqual(surfaces.current?.service, "midgard.v1.common")

        // A lower clearance: the keys above it are withheld, by count only.
        try await switchTo(app, "a.holt")
        await keys.load()
        XCTAssertEqual(keys.rows.count, 0)
        XCTAssertEqual(keys.withheld, 4)
        XCTAssertEqual(keys.state, .loaded)
        XCTAssertNil(keys.current)
    }

    func testLinksRoundTripAsNavigationPathElements() {
        for link in [MidgardLink.exploreType("aircraft"), .seed("aircraft/1000"), .text("stand by")] {
            let (screen, back) = MidgardLink.decode(link.encoded(for: "Search Around"))
            XCTAssertEqual(screen, "Search Around")
            XCTAssertEqual(back, link)
        }
        XCTAssertNil(MidgardLink.decode("Keys").link)
        XCTAssertEqual(MidgardChrome.sections.flatMap(\.items).map(\.screen).sorted(),
                       ConsoleCatalog.consoles[0].screens.sorted())
    }

    func testEveryScreenRendersInBothModes() async throws {
        let app = try await signedIn("k.osei")
        for screen in ConsoleCatalog.consoles[0].screens {
            for compact in [false, true] {
                for scheme in [ColorScheme.dark, .light] {
                    let view = MidgardScreen(app: app, persona: "k.osei", screen: screen, compact: compact) { _, _ in }
                        .frame(width: compact ? 402 : 1100, height: 800)
                        .environment(\.colorScheme, scheme)
                    let renderer = ImageRenderer(content: view)
                    XCTAssertNotNil(renderer.cgImage, "\(screen) did not render")
                }
            }
        }
    }
}

import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Mimir's screen models, over the release's sample data, against the real core: the
/// lists (agents, models, feedback; session traces withheld above the clearance by
/// count), the Agent Studio (the exchange and its follow-up answer; the proposal refused
/// without a token, authorised with one and decided once), the Logic Builder (settings
/// edited, steps added, moved and deleted, the gate's Delete disabled and the core
/// refusing a save that loses the gate), Evals (a run through the core's port with its
/// cases, score and gate, not queued offline), Guardrails (the three presets through the
/// rails), and every screen rendered in both modes. [FR-C-1] [FR-C-4] [FR-MI-2]
/// [FR-MI-3] [FR-MI-4] [FR-MI-7] [AC-C-13]
@MainActor
final class MimirConsoleTests: XCTestCase {
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
                                       replicaId: "mimir-test"))
        await app.enrol()
        await app.signIn(try XCTUnwrap(app.personas.first { $0.id == id }))
        guard case .unlocked = app.phase else {
            XCTFail("not signed in: \(app.phase)")
            return app
        }
        return app
    }

    func testTheListsShowTheSampleAndTracesAboveTheClearanceAreWithheld() async throws {
        let app = try await signedIn("j.benjamin")
        let agents = MidgardListModel.agents(app)
        await agents.load()
        XCTAssertEqual(agents.rows.map(\.item.name), ["dispatch-01", "feed-triage", "release-risk"])
        let models = MidgardListModel.models(app)
        await models.load()
        XCTAssertEqual(models.rows.map(\.item.name), ["Mistral Small 3.2", "Nemotron", "Gemma", "Granite"])
        XCTAssertEqual(MimirModelPlaneScreen.canaryName("Mistral Small 3.2"), "mistral-small 3.2")
        let feedback = MidgardListModel.feedback(app)
        await feedback.load()
        XCTAssertEqual(feedback.rows[3].item.prov, "FAILED")
        XCTAssertEqual(feedback.rows[3].item.status, "danger")

        // J. Benjamin (OPS-N) reads S-8812, not the SIG trace; K. Rao neither.
        let sessions = MidgardListModel.sessions(app)
        await sessions.load()
        XCTAssertEqual(sessions.rows.map(\.item.id), ["S-8812"])
        XCTAssertEqual(sessions.withheld, 1)
        XCTAssertEqual(sessions.rows[0].marking.label, "CONFIDENTIAL // OPS-N")
        XCTAssertEqual(sessions.rows[0].item.steps.last?.kind, "gate")
        await app.switchPersona(try XCTUnwrap(app.personas.first { $0.id == "k.rao" }))
        await sessions.load()
        XCTAssertTrue(sessions.rows.isEmpty)
        XCTAssertEqual(sessions.withheld, 2)
    }

    func testTheStudioExchangeAndItsProposalThroughTheGate() async throws {
        let app = try await signedIn("j.benjamin")
        let model = MimirStudioModel(app: app)
        model.answerDelay = 0
        await model.load()
        XCTAssertEqual(model.agent?.name, "dispatch-01")
        XCTAssertEqual(model.messages.count, 2)
        XCTAssertTrue(model.messages[0].operatorSaid)
        XCTAssertEqual(model.messages[0].text, "Any aircraft trending toward a maintenance risk this week?")
        XCTAssertEqual(model.messages[1].cites, ["Aircraft/AC-2214", "MaintenanceEvent/ME-88121", "fn check_slack()"])
        XCTAssertEqual(model.messages[1].proposal, "Schedule maintenance · AC-2214 → FAC-DUB")

        // A question gets the exchange's follow-up answer.
        model.input = "  "
        await model.send()
        XCTAssertEqual(model.messages.count, 2, "an empty question is not sent")
        model.input = "Which crews are free?"
        await model.send()
        XCTAssertEqual(model.messages.count, 4)
        XCTAssertEqual(model.messages[3].cites, ["ObjectSet/aircraft·risk>0.6", "Policy/purpose:maintenance"])
        XCTAssertFalse(model.thinking)
        XCTAssertEqual(model.input, "")

        // Without a token the core refuses; the press sends one; a second is a conflict.
        await model.decide(token: false)
        XCTAssertEqual(model.gate, .refused("Human gate required to authorise \"Schedule maintenance · AC-2214 → FAC-DUB\""))
        await model.authorise()
        XCTAssertEqual(model.gate, .authorised("A-77122"))
        await model.authorise()
        guard case let .refused(reason) = model.gate else { return XCTFail("not refused: \(model.gate)") }
        XCTAssertTrue(reason.contains("already authorised"), reason)

        // Choosing another agent keeps the session.
        model.agentId = "risk"
        XCTAssertEqual(model.agent?.name, "release-risk")
    }

    func testTheLogicIsEditedAndTheGateHoldsInTheCore() async throws {
        let app = try await signedIn("j.benjamin")
        let model = MimirLogicModel(app: app)
        await model.load()
        XCTAssertEqual(model.steps.map(\.kind), ["trigger", "retrieve", "infer", "branch", "action", "gate", "audit"])
        XCTAssertEqual(model.selected, 4, "the web selects the proposal first")
        XCTAssertEqual(model.logic?.version, "v7")
        XCTAssertEqual(model.gates, 1)
        XCTAssertFalse(model.edited)

        // The gate's Delete is disabled here; any other step's is not.
        XCTAssertNotNil(model.deleteDisabled(5))
        model.delete(5)
        XCTAssertEqual(model.gates, 1, "the gate is not deleted")
        XCTAssertNil(model.deleteDisabled(3))

        // Moving the gate above the action it guards is allowed on the canvas: the core
        // refuses the save.
        model.move(5, by: -1)
        XCTAssertEqual(model.steps.map(\.kind)[4], "gate")
        XCTAssertTrue(model.edited)
        await model.save()
        XCTAssertEqual(model.message, "The human gate cannot be deleted: logic with a state-mutating action needs a human gate between proposal and execution")
        model.discard()
        XCTAssertFalse(model.edited)

        // A title edited, a step added and the confidence check deleted: saved.
        model.setTitle(0, "Trigger · severity high")
        model.selected = 6
        model.add(kind: "audit")
        XCTAssertEqual(model.selected, 7)
        XCTAssertEqual(model.step?.title, "New audit step")
        model.delete(3)
        XCTAssertEqual(model.steps.count, 7)
        await model.save()
        XCTAssertEqual(model.message, "Saved · audit #A-77122")
        XCTAssertFalse(model.edited, "reloaded from the enclosure")
        XCTAssertEqual(model.steps.first?.title, "Trigger · severity high")
        XCTAssertEqual(model.steps.map(\.kind), ["trigger", "retrieve", "infer", "action", "gate", "audit", "audit"])
    }

    func testAnEvalRunsThroughTheCoreAndIsNotQueuedOffline() async throws {
        let app = try await signedIn("a.holt")
        let model = MimirEvalsModel(app: app)
        await model.load()
        XCTAssertEqual(model.rows.map(\.item.id), ["dispatch-core", "dispatch-adversarial", "triage-grounding", "risk-calibration"])
        XCTAssertEqual(model.current?.item.id, "risk-calibration", "the web selects the fourth suite")
        model.kind = 2
        XCTAssertEqual(model.visible.map(\.item.id), ["triage-grounding"])
        model.kind = 0

        await model.runSuite()
        let run = try XCTUnwrap(model.run)
        XCTAssertEqual(run.auditId, "A-77121")
        XCTAssertEqual(run.answer.score, 87.5)
        XCTAssertEqual(run.answer.passed, 7)
        XCTAssertEqual(run.answer.total, 8)
        XCTAssertFalse(run.answer.gateClear)
        XCTAssertEqual(run.answer.cases.filter { !$0.passed }.map(\.name), ["config-only diff risk score"])

        // Another suite: the stale run is cleared; this one clears the gate.
        model.select(0)
        XCTAssertNil(model.run)
        await model.runSuite()
        XCTAssertEqual(model.run?.answer.score, 91.7)
        XCTAssertEqual(model.run?.answer.gateClear, true)
        XCTAssertEqual(model.run?.auditId, "A-77122")

        // Offline a run is not queued.
        await app.setOffline(true)
        model.select(1)
        await model.runSuite()
        XCTAssertNil(model.run)
        XCTAssertEqual(model.message, MidgardOutcome.offlineRead)
    }

    func testThePresetsRunThroughTheRails() async throws {
        let app = try await signedIn("k.rao")
        let model = MimirGuardrailsModel(app: app)
        await model.load()
        XCTAssertEqual(model.rows.map(\.item.id), ["GR-INJECTION", "GR-TOOLSCOPE", "GR-EXFIL", "GR-CITATION", "GR-ANOMALY"])
        XCTAssertEqual(model.rows.filter(\.item.core).count, 4)
        var codes: [String] = []
        for i in MimirGuardrailsModel.presets.indices {
            model.choose(i)
            XCTAssertNil(model.check)
            await model.runProbe()
            let check = try XCTUnwrap(model.check)
            codes.append(check.answer.code)
            XCTAssertEqual(check.auditId, "A-\(77121 + i)")
        }
        XCTAssertEqual(codes, ["REFUSED · PROMPT-INJECTION", "REFUSED · EXFILTRATION", "PASS"])
        model.probe = "   "
        await model.runProbe()
        XCTAssertEqual(model.check?.answer.code, "PASS", "an empty input is not sent")
    }

    func testEveryScreenRendersInBothModes() async throws {
        MGFonts.registerAll()
        let app = try await signedIn("j.benjamin")
        for screen in MimirChrome.screens {
            for scheme in [ColorScheme.dark, .light] {
                for compact in [false, true] {
                    let view = MimirScreen(app: app, persona: "j.benjamin", screen: screen, compact: compact)
                        .environment(\.colorScheme, scheme)
                        .frame(width: compact ? 402 : 1170, height: compact ? 874 : 900)
                    XCTAssertNotNil(ImageRenderer(content: view).cgImage, "\(screen) \(scheme) \(compact)")
                }
            }
        }
        XCTAssertEqual(Set(MimirChrome.screens), Set(ConsoleCatalog.console(number: 5).screens))
    }
}

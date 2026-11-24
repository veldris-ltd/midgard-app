import MidgardCore
import SwiftUI

// Mimir's GOVERN and SERVE screens, matched to the web console: Evals
// (`EvalsScreen.tsx`), Guardrails (`GuardrailsScreen.tsx`), Feedback
// (`FeedbackScreen.tsx`) and Model Plane (`ModelPlaneScreen.tsx`). [FR-MI-1] [FR-MI-3]
// [FR-MI-5] [FR-MI-7] [FR-MI-8] [NFR-AI-1] [DD-4] [DD-5] [DD-7]

// MARK: - Evals

struct MimirEvalsScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MimirEvalsModel
    @State private var inspecting = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: MimirEvalsModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Evals"), asideIdentifier: ctx.detail("Evals"),
                       inspecting: $inspecting, top: ctx.header("Evals"), asideWidth: ctx.asideWidth("Evals")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("FR-MI-3", "Agent runtime with tools, memory, planning, and per-agent Evals"),
                           ("NFR-AI-1", "Traceable, evaluated, reversible or human-gated")])
        }
        .task(id: ctx.persona) {
            await model.load()
            publish()
        }
        .onChange(of: model.running) { _, _ in publish() }
        .onChange(of: model.selected) { _, _ in publish() }
    }

    /// The header's Run suite (the web's toolbar button) runs the selected suite.
    private func publish() {
        ctx.bar.running = model.running
        ctx.bar.runDisabled = model.running ? "A run is in flight" : model.current == nil ? "No suite is selected" : nil
        ctx.bar.runSuite = { [weak model] in
            Task { @MainActor in
                await model?.runSuite()
                if ctx.compact { inspecting = true }
            }
        }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 1, section: "EVALS", title: "Evaluation suites", accent: ctx.accent) {
            WebStatBlock(value: "18", label: "SUITES")
            WebStatBlock(value: "93.4%", label: "FLEET PASS RATE", accent: ctx.accent)
            WebStatBlock(value: "2", label: "GATES BLOCKING")
        }
        FlowRow(spacing: 10) {
            ForEach(Array(MimirEvalsModel.kinds.enumerated()), id: \.offset) { i, k in
                WebTag(k, active: model.kind == i, accent: ctx.accent, identifier: "mg.mimir.evals.kind.\(i)") {
                    model.kind = i
                }
            }
        }
        if ctx.compact, let s = model.current {
            MidgardInspectButton(title: "Inspect \(s.item.id)", identifier: "mg.mimir.inspect") { inspecting = true }
        }
        let visible = model.visible
        WebTable([
            WebColumn("Suite", text: { (r: MidgardRow<MimirEval>) in r.item.id }) { r in
                MimirIconCell(symbol: WebIcon.symbol("gauge"), text: r.item.id)
            },
            WebColumn("Agent", width: 130, mono: true) { $0.item.agent },
            WebColumn("Kind", width: 110, mono: true) { $0.item.kind },
            WebColumn("Cases", width: 70, align: .trailing, mono: true) { "\(Int($0.item.cases))" },
            WebColumn("Pass rate", width: 150, text: { "\(mimirPercent($0.item.pass))%" }) { r in
                MimirPassBar(r.item.pass, warn: r.item.pass < 90 || r.item.status == "warn")
            },
            WebColumn("Δ 7d", width: 70, align: .trailing, mono: true) { $0.item.delta },
            WebColumn("State", width: 130, text: { $0.item.statusLabel }) { r in mimirPill(r.item.status, r.item.statusLabel) },
        ], rows: visible, id: \.id,
           selected: visible.firstIndex { $0.id == model.current?.id },
           minWidth: 760, asList: ctx.compact, accent: ctx.accent, identifier: "mg.mimir.evals.table") { i in
            let target = visible[i]
            let index = model.rows.firstIndex { $0.id == target.id } ?? 0
            ctx.select(index, { model.select($0) }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "suite")
        if model.state == .loading { WebMicro("LISTING SUITES…") }
        WebTerminal("LAST RUN · 14:20", lines: [
            "> mimir evals run --all --model mistral-small-3.2",
            "dispatch-core        412/412 · 96% · 4m02s",
            "dispatch-adversarial 128/128 · 92% · 2m11s ⚠ drift −0.8",
            "risk-calibration      96/96  · 88% · 1m38s ⚠ below gate",
            "# 2 gates blocking · policy updates queued",
        ], accent: ctx.accent)
    }

    @ViewBuilder private var aside: some View {
        if let r = model.current {
            let s = r.item
            WebInspectorTitle(symbol: WebIcon.symbol("gauge"), title: s.id, accent: ctx.accent,
                              identifier: "mg.mimir.inspector.title") {
                mimirPill(s.status, s.statusLabel)
            }
            WebStatPair(("\(mimirPercent(s.pass))%", "PASS · GATE 90%", s.pass < 90), (s.delta, "7-DAY DELTA", false),
                        accent: ctx.accent)
            WebButton(model.running ? "Running…" : "Run \(s.id)", symbol: WebIcon.symbol("play"), size: .sm, fullWidth: true,
                      disabledReason: model.running ? "A run is in flight" : nil, accent: ctx.accent,
                      identifier: "mg.mimir.evals.run") {
                Task { await model.runSuite() }
            }
            if model.running { WebMicro("EVALUATING CASES…") }
            if let message = model.message {
                Text(message)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.warnText)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mg.mimir.evals.message")
            }
            if let run = model.run {
                result(run)
            } else if !model.running {
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("FAILING CASES · TOP 3").padding(.bottom, 8)
                    ForEach(["multi-leg schedule with crew constraint", "conflicting maintenance windows",
                             "risk score on config-only diffs"], id: \.self) { l in
                        MimirCheckRow(done: false, failed: true, text: l)
                    }
                }
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every suite is above your clearance." : "No suite is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }

    private func result(_ run: MimirAudited<MimirEvalAnswer>) -> some View {
        let a = run.answer
        return VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 10) {
                Text(a.gateClear ? "GATE · CLEAR" : "GATE · BLOCKING")
                    .font(MGWebFont.mono(10))
                    .mgTracking(0.12, size: MGWebFont.monoSize(10))
                    .foregroundStyle(a.gateClear ? MGWeb.secureText : MGWeb.warnText)
                    .accessibilityIdentifier("mg.mimir.evals.gate")
                Spacer(minLength: 0)
                Text("\(a.passed)/\(a.total)").font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textFaint)
            }
            MimirScoreMeter(a.score)
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("CASES").padding(.bottom, 6)
                ForEach(Array(a.cases.enumerated()), id: \.offset) { _, c in
                    MimirCheckRow(done: c.passed, failed: !c.passed, text: c.name, detail: c.detail)
                }
            }
            Text("AUDIT \(run.auditId ?? "—") · ML-DSA ✓")
                .font(MGWebFont.mono(10))
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityIdentifier("mg.mimir.evals.audit")
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(a.gateClear ? MGWeb.secure : MGWeb.warn))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.mimir.evals.result")
    }
}

// MARK: - Guardrails

struct MimirGuardrailsScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MimirGuardrailsModel
    @State private var inspecting = false
    @State private var monitor = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: MimirGuardrailsModel(app: ctx.app))
    }

    /// The web's INCIDENTS · 7D (fixed on the web screen).
    private static let incidents: [[String]] = [
        ["#G-1182", "Prompt-injection filter", "dispatch-01", "Input quarantined · no tool calls made", "today 11:02"],
        ["#G-1181", "Citation requirement", "feed-triage", "Uncited claim stripped · answer regenerated", "today 09:44"],
        ["#G-1179", "Anomalous-plan detector", "release-risk", "Flagged for review · monitor mode", "yesterday"],
    ]

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Guardrails"), asideIdentifier: ctx.detail("Guardrails"),
                       inspecting: $inspecting, top: ctx.header("Guardrails"), asideWidth: ctx.asideWidth("Guardrails")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("DD-5", "Agentic self-* AI governed by evaluation and guardrails"),
                           ("FR-MI-7", "Self-defending: filter prompt-injection, scope tools, block exfiltration")])
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 2, section: "GUARDRAILS", title: "Guardrail console", accent: ctx.accent) {
            WebStatBlock(value: "5", label: "RAILS ACTIVE")
            WebStatBlock(value: "46", label: "TRIPS · 7D")
            WebStatBlock(value: "0", label: "EXFIL EVENTS · EVER", accent: ctx.accent)
        }
        if ctx.compact, let r = model.current {
            MidgardInspectButton(title: "Inspect \(r.item.name)", identifier: "mg.mimir.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Guardrail", text: { (r: MidgardRow<MimirGuardrail>) in r.item.name }) { r in
                MimirIconCell(symbol: WebIcon.symbol("shield-check"), text: r.item.name)
            },
            WebColumn("Layer", width: 110, mono: true) { $0.item.layer },
            WebColumn("Mode", width: 110, text: { $0.item.mode.uppercased() }) { r in
                WebBadge(r.item.mode.uppercased(), tone: r.item.mode == "enforce" ? .secure : .warn)
            },
            WebColumn("Trips · 7d", width: 90, align: .trailing, mono: true) { "\(Int($0.item.trips))" },
            WebColumn("State", width: 140, text: { $0.item.statusLabel }) { r in mimirPill(r.item.status, r.item.statusLabel) },
        ], rows: model.rows, id: \.id, selected: model.current == nil ? nil : model.selected, minWidth: 720,
           asList: ctx.compact, accent: ctx.accent, identifier: "mg.mimir.guardrails.table") { i in
            ctx.select(i, { model.selected = $0 }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "guardrail")
        if model.state == .loading { WebMicro("LISTING RAILS…") }
        interception
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 4, "INCIDENTS · 7D", accent: ctx.accent, muted: true)
            WebTable([
                WebColumn("Incident", width: 90, mono: true) { (r: [String]) in r[0] },
                WebColumn("Rail") { $0[1] },
                WebColumn("Agent", width: 130, mono: true) { $0[2] },
                WebColumn("Disposition") { $0[3] },
                WebColumn("TS", width: 110, mono: true) { $0[4] },
            ], rows: Self.incidents, id: { $0[0] }, compact: true, minWidth: 640, asList: ctx.compact, accent: ctx.accent,
               identifier: "mg.mimir.guardrails.incidents")
        }
    }

    /// LIVE INTERCEPTION · SELF-DEFENDING: the presets, the input, Run through rails, and
    /// the core's verdict.
    private var interception: some View {
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 3, "LIVE INTERCEPTION · SELF-DEFENDING", accent: ctx.accent, muted: true)
            VStack(alignment: .leading, spacing: 12) {
                FlowRow(spacing: 6) {
                    ForEach(Array(MimirGuardrailsModel.presets.enumerated()), id: \.offset) { i, p in
                        WebTag(p.label, active: model.probe == p.value, accent: ctx.accent,
                               identifier: "mg.mimir.guardrails.preset.\(p.label)") { model.choose(i) }
                    }
                }
                WebTextField(nil, placeholder: "Enter an agent input to run through the rails…", text: $model.probe,
                             identifier: "mg.mimir.guardrails.input") {
                    Task { await model.runProbe() }
                }
                WebButton(model.checking ? "Screening…" : "Run through rails", symbol: WebIcon.symbol("shield-check"),
                          disabledReason: model.checking ? "A check is in flight"
                              : model.probe.trimmingCharacters(in: .whitespaces).isEmpty ? "Enter an input first" : nil,
                          accent: ctx.accent, identifier: "mg.mimir.guardrails.run") {
                    Task { await model.runProbe() }
                }
                if model.checking { WebMicro("SCREENING · INPUT → EXECUTION → OUTPUT…") }
                if let message = model.message {
                    Text(message)
                        .font(MGWebFont.sans(12, .caption))
                        .foregroundStyle(MGWeb.warnText)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("mg.mimir.guardrails.message")
                }
                if let check = model.check { verdict(check) }
            }
            .padding(18)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(
                model.check.map { $0.answer.intercepted ? MGWeb.dangerText : MGWeb.secure } ?? MGWeb.border))
        }
    }

    private func verdict(_ check: MimirAudited<MimirGuardrailAnswer>) -> some View {
        let a = check.answer
        return HStack(alignment: .top, spacing: 12) {
            Image(systemName: a.intercepted ? "checkmark.shield" : "checkmark")
                .font(MGWebFont.sans(16))
                .foregroundStyle(a.intercepted ? MGWeb.dangerText : MGWeb.secureText)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 8) {
                FlowRow(spacing: 10) {
                    WebBadge(a.code, tone: a.intercepted ? .danger : .secure)
                    Text(a.rail.uppercased())
                        .font(MGWebFont.mono(10))
                        .mgTracking(0.08, size: MGWebFont.monoSize(10))
                        .foregroundStyle(MGWeb.textFaint)
                }
                Text(a.reason)
                    .font(MGWebFont.sans(14))
                    .foregroundStyle(MGWeb.textBody)
                    .lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
                Text("AUDIT \(check.auditId ?? "—") · ML-DSA ✓")
                    .font(MGWebFont.mono(10))
                    .foregroundStyle(MGWeb.textFaint)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(a.intercepted ? MGWeb.dangerText : MGWeb.secure))
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.mimir.guardrails.verdict")
    }

    @ViewBuilder private var aside: some View {
        if let row = model.current {
            let r = row.item
            WebInspectorTitle(symbol: WebIcon.symbol("shield-check"), title: r.name, mono: false, accent: ctx.accent,
                              identifier: "mg.mimir.inspector.title") {
                mimirPill(r.status, r.statusLabel)
            }
            WebStatPair(("\(Int(r.trips))", "TRIPS · 7D", r.trips > 10), (r.layer.uppercased(), "LAYER", false),
                        accent: ctx.accent)
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("MODE")
                if r.core {
                    HStack(spacing: 10) {
                        Image(systemName: "lock.fill").font(MGWebFont.sans(12)).foregroundStyle(MGWeb.textFaint)
                            .accessibilityHidden(true)
                        Text(r.mode == "enforce" ? "Enforce (blocks on trip)" : "Monitor only (logs, never blocks)")
                            .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody)
                    }
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("mg.mimir.guardrails.mode")
                    Text("CORE RAILS CANNOT BE DISABLED [FR-MI-7]")
                        .font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                } else {
                    WebButton(r.mode == "enforce" ? "Enforce (blocks on trip)" : "Monitor only (logs, never blocks)",
                              variant: .secondary, size: .sm,
                              disabledReason: "Not available in this app: a rail's mode is set in the enclosure",
                              accent: ctx.accent, identifier: "mg.mimir.guardrails.mode")
                }
            }
            WebTerminal("LAST TRIP", lines: ["agent    dispatch-01", "vector   indirect injection via ticket body",
                                             "action   input quarantined · session flagged",
                                             "audit    #G-1182 · ML-DSA signed"], accent: ctx.accent)
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every rail is above your clearance." : "No rail is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Feedback

struct MimirFeedbackScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MidgardListModel<MimirFeedbackItem>
    /// The web selects the poisoning attempt first.
    @State private var selected = 3
    @State private var inspecting = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .feedback(ctx.app))
    }

    private var currentIndex: Int? { model.rows.isEmpty ? nil : min(selected, model.rows.count - 1) }
    private var current: MimirFeedbackItem? { currentIndex.map { model.rows[$0].item } }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Feedback"), asideIdentifier: ctx.detail("Feedback"),
                       inspecting: $inspecting, top: ctx.header("Feedback"), asideWidth: ctx.asideWidth("Feedback")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("FR-MI-5", "Self-learning from governed, provenance-checked feedback only"),
                           ("NFR-AI-1", "Traceable, evaluated, reversible")])
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 8, section: "FEEDBACK", title: "Governed feedback", accent: ctx.accent) {
            WebStatBlock(value: "212", label: "RECORDS · 30D")
            WebStatBlock(value: "100%", label: "PROVENANCE-CHECKED", accent: ctx.accent)
            WebStatBlock(value: "1", label: "POISONING ATTEMPTS BLOCKED")
        }
        if ctx.compact, let f = current {
            MidgardInspectButton(title: "Inspect \(f.id)", identifier: "mg.mimir.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Record", width: 90, text: { (r: MidgardRow<MimirFeedbackItem>) in r.item.id }) { r in
                Text(r.item.id).font(MGWebFont.mono(12, .caption))
                    .foregroundStyle(r.item.status == "danger" ? MGWeb.dangerText : ctx.accent.text)
            },
            WebColumn("Source", width: 140, mono: true) { $0.item.src },
            WebColumn("Kind", width: 100, mono: true) { $0.item.kind },
            WebColumn("Agent", width: 120, mono: true) { $0.item.target },
            WebColumn("Feedback") { $0.item.note },
            WebColumn("Provenance", width: 100, text: { $0.item.prov.uppercased() }) { r in
                WebBadge(r.item.prov.uppercased(), tone: r.item.prov == "FAILED" ? .danger : .secure)
            },
            WebColumn("State", width: 120, text: { $0.item.statusLabel }) { r in mimirPill(r.item.status, r.item.statusLabel) },
        ], rows: model.rows, id: \.id, selected: currentIndex, minWidth: 820, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.mimir.feedback.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "record")
        if model.state == .loading { WebMicro("LISTING FEEDBACK…") }
        WebTerminal("LEARNING LEDGER", lines: [
            "> mimir feedback ledger --window 30d",
            "received   212 · verified 211 · discarded 1",
            "updates    3 policy versions · all evals-gated",
            "reverts    0 · every update reversible",
            "# self-learning is bounded autonomy",
        ], accent: ctx.accent)
    }

    @ViewBuilder private var aside: some View {
        if let f = current {
            WebInspectorTitle(symbol: WebIcon.symbol("refresh-cw"), title: f.id, accent: ctx.accent,
                              identifier: "mg.mimir.inspector.title") {
                mimirPill(f.status, f.statusLabel)
            }
            WebPanel {
                VStack(alignment: .leading, spacing: 8) {
                    Text(f.note).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody)
                        .fixedSize(horizontal: false, vertical: true)
                    Text("\(f.kind.uppercased()) · FROM \(f.src.uppercased())")
                        .font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                }
            }
            if f.status == "danger" {
                MimirNoteCard(heading: "POISONING DEFENCE · DISCARDED", headingColour: MGWeb.dangerText, border: MGWeb.dangerText,
                              text: "Source identity could not be provenance-checked and the correction pattern matches a known poisoning vector. Nothing entered retrieval or policy state; the attempt is signed lineage.",
                              identifier: "mg.mimir.feedback.poisoning")
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("PATH TO POLICY").padding(.bottom, 8)
                    ForEach(Array([("Provenance check", true),
                                   ("Aggregation window", f.statusLabel != "Queued"),
                                   ("Evals gate (no regression)", f.statusLabel == "Applied"),
                                   ("Author approval", f.statusLabel == "Applied"),
                                   ("Versioned + reversible", f.statusLabel == "Applied")].enumerated()),
                            id: \.offset) { _, step in
                        MimirCheckRow(done: step.1, failed: false, text: step.0)
                    }
                }
                .accessibilityIdentifier("mg.mimir.feedback.path")
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every record is above your clearance." : "No record is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Model Plane

struct MimirModelPlaneScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MidgardListModel<MimirModel>
    @State private var selected = 0
    @State private var inspecting = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .models(ctx.app))
    }

    /// The web's CONSUMERS · BY PRODUCT (fixed on the web screen).
    private static let consumers: [[String]] = [
        ["mimir/agents", "OAG chat, dispatch, triage", "Mistral Small 3.2", "4,812"],
        ["forge/assist", "NL→transform, drift diagnosis", "Mistral Small 3.2", "210"],
        ["bifrost/risk", "Release risk advisories", "Nemotron", "38"],
        ["heimdall/options", "Ranked courses of action", "Mistral Small 3.2", "96"],
        ["mcp/tools", "Governed agent/tool access to the fabric (MCP)", "Mistral Small 3.2", "1,204"],
    ]

    /// The web's `name.toLowerCase().replace(' ', '-')`: the first space only.
    static func canaryName(_ name: String) -> String {
        var lower = name.lowercased()
        if let r = lower.range(of: " ") { lower.replaceSubrange(r, with: "-") }
        return lower
    }

    private var currentIndex: Int? { model.rows.isEmpty ? nil : min(selected, model.rows.count - 1) }
    private var current: MimirModel? { currentIndex.map { model.rows[$0].item } }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Model Plane"), asideIdentifier: ctx.detail("Model Plane"),
                       inspecting: $inspecting, top: ctx.header("Model Plane"), asideWidth: ctx.asideWidth("Model Plane")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("DD-4", "Local AI processing via the Mimir layer"),
                           ("FR-MI-8", "Expose inference to Forge, Bifrost, Heimdall over internal PQ-mTLS"),
                           ("FR-MI-1", "Serve open-weight model families in-enclosure")])
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 4, section: "MODEL PLANE", title: "Model serving", accent: ctx.accent) {
            WebStatBlock(value: "\(model.rows.count)", label: "FAMILIES")
            WebStatBlock(value: "8×", label: "GPU POOL")
            WebStatBlock(value: "0", label: "EXTERNAL CALLS", accent: ctx.accent)
        }
        if ctx.compact, let m = current {
            MidgardInspectButton(title: "Inspect \(m.name)", identifier: "mg.mimir.inspect") { inspecting = true }
        }
        FlowRow(spacing: 12) {
            ForEach(Array(model.rows.enumerated()), id: \.offset) { i, row in
                let m = row.item
                WebPanel(padding: 18, accentRule: i == currentIndex, accent: ctx.accent,
                         identifier: "mg.mimir.models.\(ConsoleCatalog.slug(m.name))",
                         label: "\(m.name), \(m.statusLabel), \(m.role)",
                         action: { ctx.select(i, { selected = $0 }, $inspecting) }) {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack(spacing: 10) {
                            Image(systemName: WebIcon.symbol("cpu")).font(MGWebFont.sans(15))
                                .foregroundStyle(ctx.accent.text).accessibilityHidden(true)
                            Text(m.name).font(MGWebFont.display(17, .headline).weight(.bold))
                                .foregroundStyle(MGWeb.textHeading)
                            Spacer(minLength: 8)
                            mimirPill(m.status, m.statusLabel)
                        }
                        Text(m.role).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                        FlowRow(spacing: 16) {
                            ForEach([m.quant, "CTX \(m.ctx)", m.gpu, m.tps], id: \.self) { t in
                                Text(t).font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                            }
                        }
                    }
                    .frame(width: ctx.compact ? nil : 250, alignment: .leading)
                }
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md)
                    .strokeBorder(i == currentIndex ? ctx.accent.fill : .clear))
            }
        }
        MidgardWithheld(count: model.withheld, noun: "model")
        if model.state == .loading { WebMicro("LISTING MODEL FAMILIES…") }
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 5, "CONSUMERS · BY PRODUCT", accent: ctx.accent, muted: true)
            WebTable([
                WebColumn("Consumer", mono: true) { (r: [String]) in r[0] },
                WebColumn("Use") { $0[1] },
                WebColumn("Model", width: 110, mono: true) { $0[2] },
                WebColumn("Calls · 24h", width: 100, align: .trailing, mono: true) { $0[3] },
            ], rows: Self.consumers, id: { $0[0] }, compact: true, minWidth: 640, asList: ctx.compact, accent: ctx.accent,
               identifier: "mg.mimir.models.consumers")
        }
    }

    @ViewBuilder private var aside: some View {
        if let m = current {
            WebInspectorTitle(symbol: WebIcon.symbol("cpu"), title: m.name, mono: false, accent: ctx.accent,
                              identifier: "mg.mimir.inspector.title") {
                mimirPill(m.status, m.statusLabel)
            }
            WebStatPair((m.tps, "THROUGHPUT", false), ("0.42s", "FIRST TOKEN · NFR-PERF-1", false), accent: ctx.accent)
            MimirRules(title: "WEIGHTS PROVENANCE", rows: [
                ("Source", "open weights · vendored"),
                ("Digest", "sha3:77aa…41c9"),
                ("Signature", "ML-DSA verified on load · fails closed"),
                ("Serving", "continuous batching · KV-cache"),
                ("Egress", "none · weights never leave"),
            ])
            WebTerminal("CANARY PROMPTS", lines: ["> mimir models canary \(Self.canaryName(m.name))",
                                                  "grounding   24/24 OK", "refusals    18/18 OK", "latency     p99 0.61s",
                                                  "# canary battery clean"], accent: ctx.accent)
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every model family is above your clearance." : "No model family is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

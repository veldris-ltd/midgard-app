import MidgardCore
import SwiftUI

// Mimir's BUILD screens, matched to the web console: Agent Studio
// (`AgentStudioScreen.tsx`), Logic Builder (`LogicBuilderScreen.tsx`) and Session
// Traces (`SessionTracesScreen.tsx`). [FR-MI-2] [FR-MI-3] [FR-MI-4] [NFR-AI-1]
// [NFR-PQ-2] [DD-5] [DD-7]

// MARK: - Agent Studio

struct MimirStudioScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MimirStudioModel
    @State private var inspecting = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: MimirStudioModel(app: ctx.app))
    }

    /// The web's SELF-* LOOPS.
    private static let loops: [(String, String)] = [
        ("self-learning", "governed feedback → retrieval"),
        ("self-healing", "reroute · retry · rollback"),
        ("self-defending", "injection filter · tool scope"),
    ]

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Agent Studio"),
                       asideIdentifier: ctx.detail("Agent Studio"), inspecting: $inspecting,
                       top: ctx.header("Agent Studio"), asideWidth: ctx.asideWidth("Agent Studio")) {
            main
        } aside: {
            aside
        } asideFoot: {
            Text("MODEL PLANE · IN-ENCLOSURE\nWEIGHTS ML-DSA VERIFIED\nPROMPTS + MEMORY PQ-SEALED")
                .font(MGWebFont.mono(10))
                .mgTracking(0.1, size: MGWebFont.monoSize(10))
                .foregroundStyle(MGWeb.textFaint)
                .lineSpacing(8)
                .fixedSize(horizontal: false, vertical: true)
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        agentPicker
        if ctx.compact, let a = model.agent {
            MidgardInspectButton(title: "Inspect \(a.name)", identifier: "mg.mimir.inspect") { inspecting = true }
        }
        MidgardWithheld(count: model.withheld, noun: "row")
        session
    }

    /// The AGENTS column (a row of cards on a narrow page, as the web's `isNarrow`). On
    /// the iPhone the agents are a row of tags with only the chosen agent's card under
    /// them, not three cards stacked: stacked, they pushed the session down to the
    /// console bar's edge, where its opening lines were cut by the bar and the audit
    /// measured the bar's contrast against them (Phase G, both appearances). A row that
    /// scrolled sideways would leave a card cut by the screen's edge instead.
    @ViewBuilder private var agentPicker: some View {
        if ctx.compact {
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("AGENTS")
                FlowRow(spacing: 8) {
                    ForEach(model.agents, id: \.id) { row in
                        WebTag(row.item.name, active: row.item.id == model.agent?.id, accent: ctx.accent,
                               identifier: "mg.mimir.agent-tag.\(row.item.id)") {
                            model.agentId = row.item.id
                        }
                    }
                }
                agentCards
            }
        } else {
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("AGENTS")
                FlowRow(spacing: 8) { agentCards }
            }
        }
    }

    /// Every agent's card on the Mac; the chosen one's on the iPhone.
    @ViewBuilder private var agentCards: some View {
                ForEach(Array(model.agents.filter { !ctx.compact || $0.item.id == model.agent?.id }.enumerated()),
                        id: \.offset) { _, row in
                    let a = row.item
                    let selected = a.id == (model.agent?.id ?? "")
                    WebPanel(accentRule: selected, accent: ctx.accent, identifier: "mg.mimir.agent.\(a.id)",
                             label: "\(a.name), \(a.desc), evals \(mimirPercent(a.evals)) percent",
                             action: { model.agentId = a.id }) {
                        VStack(alignment: .leading, spacing: 5) {
                            HStack(spacing: 8) {
                                Image(systemName: WebIcon.symbol("bot")).font(MGWebFont.sans(13))
                                    .foregroundStyle(ctx.accent.text).accessibilityHidden(true)
                                Text(a.name).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.grey100)
                                Spacer(minLength: 8)
                                mimirPill(a.status, "\(mimirPercent(a.evals))%")
                            }
                            Text(a.desc).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                        }
                        .frame(width: ctx.compact ? nil : 220, alignment: .leading)
                    }
                    .overlay(RoundedRectangle(cornerRadius: MGRadius.md)
                        .strokeBorder(selected ? ctx.accent.fill : .clear))
                }
    }

    @ViewBuilder private var session: some View {
        VStack(alignment: .leading, spacing: 0) {
            FlowRow(spacing: 12) {
                WebSectionLabel(number: 3, "OAG SESSION", accent: ctx.accent)
                Text(model.agent?.name ?? "—").font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textMuted)
                WebBadge("ONTOLOGY-GROUNDED", tone: .accent, accent: ctx.accent)
                if case let .authorised(id) = model.gate {
                    WebBadge("ACTION AUTHORISED · AUDIT #\(id ?? "—")", tone: .secure)
                        .accessibilityIdentifier("mg.mimir.studio.authorised")
                }
                WebProvenanceTag("FR-MI-2", title: "Ground generation in the object fabric with citations")
                WebProvenanceTag("NFR-AI-1", title: "Every agentic action traceable, evaluated, reversible or human-gated")
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .frame(maxWidth: .infinity, alignment: .leading)
            Rectangle().fill(MGWeb.border).frame(height: 1).accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 18) {
                ForEach(model.messages) { m in message(m) }
                if model.thinking {
                    Text("mimir is retrieving from the fabric…")
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(MGWeb.textFaint)
                        .accessibilityIdentifier("mg.mimir.studio.thinking")
                }
                if model.state == .loading { WebMicro("OPENING THE SESSION…") }
            }
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(MGWeb.bgSunken)
            Rectangle().fill(MGWeb.border).frame(height: 1).accessibilityHidden(true)
            HStack(spacing: 10) {
                WebTextField(nil, placeholder: "Ask across the object fabric… (grounded, cited, governed)",
                             text: $model.input, identifier: "mg.mimir.studio.input") {
                    Task { await model.send() }
                }
                WebButton("Send", trailingSymbol: WebIcon.symbol("arrow-right"),
                          disabledReason: model.thinking ? "Mimir is answering" : model.exchange == nil ? "No session is open" : nil,
                          accent: ctx.accent, identifier: "mg.mimir.studio.send") {
                    Task { await model.send() }
                }
            }
            .padding(14)
        }
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgPage))
        .clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
    }

    private func message(_ m: MimirStudioModel.Message) -> some View {
        VStack(alignment: m.operatorSaid ? .trailing : .leading, spacing: 6) {
            // One element over the drawn speaker line (`WholeElement`), in the muted
            // colour: where the console bar cuts it at the scroll's edge it is not
            // measured as a cut-off faint text (the first audit pass).
            Text(m.operatorSaid ? "OPERATOR" : "MIMIR · Mistral Small 3.2 · LOCAL")
                .font(MGWebFont.mono(10))
                .mgTracking(0.14, size: MGWebFont.monoSize(10))
                .foregroundStyle(MGWeb.textMuted)
                .modifier(WholeElement(label: m.operatorSaid ? "Operator" : "Mimir, Mistral Small 3.2, local",
                                       traits: .isStaticText, identifier: "", action: nil))
            VStack(alignment: .leading, spacing: 10) {
                Text(m.text)
                    .font(MGWebFont.sans(14))
                    .foregroundStyle(MGWeb.textBody)
                    .lineSpacing(4)
                    .fixedSize(horizontal: false, vertical: true)
                if !m.cites.isEmpty {
                    FlowRow(spacing: 6) {
                        ForEach(m.cites, id: \.self) { c in
                            HStack(spacing: 5) {
                                Image(systemName: WebIcon.symbol("boxes")).font(MGWebFont.sans(10)).accessibilityHidden(true)
                                Text(c).font(MGWebFont.mono(10))
                            }
                            .foregroundStyle(ctx.accent.text)
                            .padding(.horizontal, 7)
                            .padding(.vertical, 2)
                            .background(RoundedRectangle(cornerRadius: 4).fill(MGWeb.surfaceInset))
                            .overlay(RoundedRectangle(cornerRadius: 4).strokeBorder(MGWeb.border))
                            .accessibilityElement(children: .ignore)
                            .accessibilityLabel(Text("Citation: \(c)"))
                        }
                    }
                }
                if let proposal = m.proposal {
                    proposalPanel(proposal)
                }
            }
            .padding(.horizontal, 15)
            .padding(.vertical, 12)
            .frame(maxWidth: 560, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(m.operatorSaid ? ctx.accent.tint : MGWeb.surfaceCard))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(m.operatorSaid ? ctx.accent.fill : MGWeb.border))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("mg.mimir.studio.message.\(m.id)")
        }
        .frame(maxWidth: .infinity, alignment: m.operatorSaid ? .trailing : .leading)
    }

    /// The proposal behind the human gate: hidden once authorised, as on the web (the
    /// header's badge then carries the audit id).
    @ViewBuilder private func proposalPanel(_ proposal: String) -> some View {
        switch model.gate {
        case .authorised:
            EmptyView()
        case .pending, .queued, .refused:
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: WebIcon.symbol("zap")).font(MGWebFont.sans(14))
                        .foregroundStyle(MGWeb.warnText).accessibilityHidden(true)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(proposal).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.grey100)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(model.gate == .queued ? "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE"
                             : "STATE MUTATION · HUMAN GATE REQUIRED")
                            .font(MGWebFont.mono(9))
                            .mgTracking(0.1, size: MGWebFont.monoSize(9))
                            .foregroundStyle(MGWeb.warnText)
                    }
                }
                HStack(spacing: 8) {
                    WebButton("Authorise", size: .sm,
                              disabledReason: model.busy ? "The authorisation is in flight"
                                  : model.gate == .queued ? "Queued: sent when the enclosure is reachable" : nil,
                              accent: ctx.accent, identifier: "mg.mimir.studio.authorise") {
                        Task { await model.authorise() }
                    }
                    WebButton("Reject", variant: .ghost, size: .sm,
                              disabledReason: "Not available in this app: the web console's Reject records nothing",
                              accent: ctx.accent, identifier: "mg.mimir.studio.reject")
                }
                if case let .refused(reason) = model.gate {
                    Text(reason)
                        .font(MGWebFont.sans(12, .caption))
                        .foregroundStyle(MGWeb.warnText)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("mg.mimir.studio.refused")
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.warn))
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text("Human gate: \(proposal)"))
            .accessibilityIdentifier("mg.mimir.studio.proposal")
        }
    }

    @ViewBuilder private var aside: some View {
        let a = model.agent
        WebSectionLabel(number: 4, "EVALS, \((a?.name ?? "—").uppercased())", accent: ctx.accent, muted: true)
        WebPanel {
            VStack(alignment: .leading, spacing: 14) {
                HStack(alignment: .top, spacing: 14) {
                    WebStatBlock(value: "\(mimirPercent(a?.evals ?? 0))%", label: "EVAL PASS", accent: ctx.accent)
                        .frame(maxWidth: .infinity, alignment: .leading)
                    WebStatBlock(value: a?.firstToken ?? "—", label: "FIRST TOKEN")
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                HStack(alignment: .top, spacing: 14) {
                    WebStatBlock(value: "\(Int(a?.runsToday ?? 0))", label: "RUNS TODAY")
                        .frame(maxWidth: .infinity, alignment: .leading)
                    WebStatBlock(value: "\(Int(a?.humanGates ?? 0))", label: "HUMAN GATES")
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
        }
        .accessibilityIdentifier("mg.mimir.inspector.title")
        VStack(alignment: .leading, spacing: 0) {
            WebLabel("GUARDRAILS").padding(.bottom, 8)
            ForEach([("Prompt-injection filter", true), ("Tool scoping", true), ("Exfiltration block", true),
                     ("Adversarial eval drift", false)], id: \.0) { name, secure in
                HStack(spacing: 8) {
                    Circle().fill(secure ? MGWeb.secure : MGWeb.warn).frame(width: 7, height: 7).accessibilityHidden(true)
                    Text(name).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 7)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("\(name): \(secure ? "secure" : "warning")"))
            }
        }
        VStack(alignment: .leading, spacing: 0) {
            WebLabel("SELF-* LOOPS").padding(.bottom, 8)
            ForEach(Self.loops, id: \.0) { k, v in
                VStack(alignment: .leading, spacing: 1) {
                    Text(k).font(MGWebFont.mono(11)).foregroundStyle(ctx.accent.text)
                    Text(v).font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                }
                .padding(.vertical, 6)
                .frame(maxWidth: .infinity, alignment: .leading)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                .accessibilityElement(children: .combine)
            }
        }
    }
}

// MARK: - Logic Builder

struct MimirLogicScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MimirLogicModel
    @State private var inspecting = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: MimirLogicModel(app: ctx.app))
    }

    private func colour(_ kind: String) -> Color {
        switch kind {
        case "gate": return MGWeb.warnText
        case "audit": return MGWeb.secureText
        case "trigger": return MGWeb.infoText
        default: return ctx.accent.text
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Logic Builder"),
                       asideIdentifier: ctx.detail("Logic Builder"), inspecting: $inspecting,
                       top: ctx.header("Logic Builder"), asideWidth: ctx.asideWidth("Logic Builder")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenanceTag("FR-MI-4", title: "No-code (Logic) and pro-code builders for AI functions and agents")
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 6, section: "LOGIC", title: "dispatch-01 · workflow \(model.logic?.version ?? "v7")",
                      accent: ctx.accent) {
            WebStatBlock(value: "\(model.steps.count)", label: "STEPS")
            WebStatBlock(value: "\(model.gates)", label: "HUMAN GATES", accent: ctx.accent)
            WebStatBlock(value: "96%", label: "EVALS · V6 LIVE")
        }
        if ctx.compact, let s = model.step {
            MidgardInspectButton(title: "Inspect step \(model.selected + 1) · \(s.title)", identifier: "mg.mimir.inspect") {
                inspecting = true
            }
        }
        MidgardWithheld(count: model.withheld, noun: "logic function")
        if model.state == .loading { WebMicro("LOADING THE WORKFLOW…") }
        if model.edited {
            HStack(spacing: 8) {
                WebBadge(ctx.compact ? "DRAFT · NOT SAVED" : "DRAFT · NOT SAVED · SAVE SENDS IT TO THE ENCLOSURE", tone: .warn)
                    .accessibilityIdentifier("mg.mimir.logic.draft")
                Spacer(minLength: 0)
            }
        }
        canvas
    }

    /// The steps as the web draws them: one card per step, joined by a rule, the gate
    /// outlined in warn with its MANDATORY badge.
    private var canvas: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(model.steps.enumerated()), id: \.element.id) { i, st in
                let selected = i == model.selected
                Button {
                    ctx.select(i, { model.selected = $0 }, $inspecting)
                } label: {
                    HStack(spacing: 12) {
                        Text(String(format: "%02d", i + 1))
                            .font(MGWebFont.display(15, .headline).weight(.heavy))
                            .foregroundStyle(selected ? ctx.accent.text : MGWeb.textFaint)
                            .frame(width: 24, alignment: .leading)
                        Image(systemName: WebIcon.symbol(st.icon)).font(MGWebFont.sans(14))
                            .foregroundStyle(colour(st.kind)).accessibilityHidden(true)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(st.title).font(MGWebFont.sans(14).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                            Text(st.body).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer(minLength: 8)
                        if st.kind == "gate" { WebBadge("MANDATORY", tone: .warn) }
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                    .frame(maxWidth: 560, alignment: .leading)
                    .frame(minHeight: 44)
                    .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(selected ? MGWeb.surfaceRaised : MGWeb.surfaceCard))
                    .overlay(RoundedRectangle(cornerRadius: MGRadius.md)
                        .strokeBorder(selected ? ctx.accent.fill : st.kind == "gate" ? MGWeb.warn : MGWeb.border))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("Step \(i + 1), \(st.kind): \(st.title). \(st.body)\(st.kind == "gate" ? ". Mandatory" : "")"))
                .accessibilityAddTraits(selected ? [.isButton, .isSelected] : .isButton)
                .accessibilityIdentifier("mg.mimir.logic.step.\(i + 1)")
                if i < model.steps.count - 1 {
                    Rectangle().fill(MGWeb.borderStrong).frame(width: 2, height: 14).padding(.leading, 43)
                        .accessibilityHidden(true)
                }
            }
        }
        .padding(ctx.compact ? 14 : 24)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.mimir.logic.canvas")
    }

    @ViewBuilder private var aside: some View {
        if let s = model.step {
            let i = model.selected
            WebInspectorTitle(symbol: WebIcon.symbol(s.icon), title: "Step \(i + 1) · \(s.title)", mono: false,
                              accent: ctx.accent, identifier: "mg.mimir.inspector.title") { EmptyView() }
            WebPanel {
                Text(s.body).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody).lineSpacing(4)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if s.kind == "gate" {
                MimirNoteCard(heading: "MANDATORY STEP", headingColour: MGWeb.warnText, border: MGWeb.warn,
                              text: "Logic containing a state-mutating action cannot publish without a human gate between proposal and execution. This step cannot be deleted.",
                              identifier: "mg.mimir.logic.mandatory")
            }
            stepSettings(i)
            MimirRules(title: "VERSIONS", rows: (model.logic?.versions ?? []).map { ($0.label, $0.meta) },
                       keyWidth: 76, upperKeys: false, firstHighlighted: true)
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "The workflow is above your clearance." : "No step is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }

    /// A step's settings (both apps), and on the Mac the canvas's edits: add after it,
    /// move, delete (never the gate). Save sends the steps to the core, which holds the
    /// gate rule whatever is sent.
    private func stepSettings(_ i: Int) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            WebLabel("STEP SETTINGS · SAVED THROUGH THE ENCLOSURE")
            WebTextField("Title", placeholder: "Step title",
                         text: Binding(get: { model.step?.title ?? "" }, set: { model.setTitle(i, $0) }),
                         identifier: "mg.mimir.logic.title")
            WebTextField("Detail", placeholder: "What the step does",
                         text: Binding(get: { model.step?.body ?? "" }, set: { model.setBody(i, $0) }),
                         identifier: "mg.mimir.logic.body")
            if !ctx.compact {
                FlowRow(spacing: 8) {
                    WebButton("Move up", symbol: WebIcon.symbol("arrow-up"), variant: .secondary, size: .sm,
                              disabledReason: i == 0 ? "The first step" : nil, accent: ctx.accent,
                              identifier: "mg.mimir.logic.up") { model.move(i, by: -1) }
                    WebButton("Move down", symbol: WebIcon.symbol("arrow-down"), variant: .secondary, size: .sm,
                              disabledReason: i >= model.steps.count - 1 ? "The last step" : nil, accent: ctx.accent,
                              identifier: "mg.mimir.logic.down") { model.move(i, by: 1) }
                    WebButton("Delete", symbol: WebIcon.symbol("trash"), variant: .outline, size: .sm,
                              disabledReason: model.deleteDisabled(i), accent: ctx.accent,
                              identifier: "mg.mimir.logic.delete") { model.delete(i) }
                    Menu {
                        ForEach(MimirLogicModel.kinds, id: \.kind) { k in
                            Button(k.kind) { model.add(kind: k.kind) }
                        }
                    } label: {
                        Label("Add step", systemImage: "plus")
                            .font(MGWebFont.sans(12, .caption).weight(.semibold))
                    }
                    .menuStyle(.button)
                    .fixedSize()
                    .accessibilityIdentifier("mg.mimir.logic.add")
                }
            }
            HStack(spacing: 8) {
                WebButton(model.busy ? "Saving…" : "Save", symbol: WebIcon.symbol("check"), size: .sm,
                          disabledReason: model.busy ? "A save is in flight" : model.edited ? nil : "Nothing is edited",
                          accent: ctx.accent, identifier: "mg.mimir.logic.save") {
                    Task { await model.save() }
                }
                WebButton("Discard", variant: .ghost, size: .sm,
                          disabledReason: model.edited ? nil : "Nothing is edited", accent: ctx.accent,
                          identifier: "mg.mimir.logic.discard") { model.discard() }
            }
            if let message = model.message {
                Text(message)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(message.hasPrefix("Saved") ? MGWeb.secureText : MGWeb.warnText)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mg.mimir.logic.message")
            }
        }
    }
}

// MARK: - Session Traces

struct MimirSessionsScreen: View {
    let ctx: MimirScreenContext
    @StateObject private var model: MidgardListModel<MimirSession>
    @State private var selected = 6
    @State private var inspecting = false

    init(ctx: MimirScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .sessions(ctx.app))
    }

    /// The trace the web shows (`getSessionTrace('S-8812')`), or the first one the
    /// persona may read.
    private var trace: MimirSession? {
        model.rows.first { $0.item.id == "S-8812" }?.item ?? model.rows.first?.item
    }

    private var stepIndex: Int? {
        guard let t = trace, !t.steps.isEmpty else { return nil }
        return min(selected, t.steps.count - 1)
    }

    private func colour(_ kind: String) -> Color {
        switch kind {
        case "plan": return MGWeb.infoText
        case "act": return ctx.accent.text
        case "observe": return MGWeb.textMuted
        case "guard", "eval": return MGWeb.secureText
        case "gate": return MGWeb.warnText
        default: return MGWeb.textMuted
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Session Traces"),
                       asideIdentifier: ctx.detail("Session Traces"), inspecting: $inspecting,
                       top: ctx.header("Session Traces"), asideWidth: ctx.asideWidth("Session Traces")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if let t = trace {
                WebTerminal("SESSION LEDGER", lines: [
                    "agent    \(t.agent) · policy \(t.policyVersion)",
                    "steps    \(t.steps.count) · tools \(t.steps.filter { $0.kind == "act" }.count) · guards \(t.steps.filter { $0.kind == "guard" }.count)",
                    "evals    per-step · all passed",
                    "sig      ML-DSA · session lineage",
                ], accent: ctx.accent)
                WebProvenance([("FR-MI-3", "Agent runtime with tools, memory, planning, and per-agent Evals"),
                               ("NFR-PQ-2", "Agent memory PQ-encrypted at rest")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 7, section: "SESSIONS", title: "Session trace · \(trace?.id ?? "—")", accent: ctx.accent) {
            WebStatBlock(value: "\(trace?.steps.count ?? 0)", label: "STEPS")
            WebStatBlock(value: trace?.total ?? "—", label: "TOTAL · FIRST TOKEN \((trace?.firstToken ?? "—").uppercased())")
            WebStatBlock(value: "\(trace?.steps.filter { $0.kind == "gate" }.count ?? 0)", label: "HUMAN GATE PENDING",
                         accent: ctx.accent)
        }
        if ctx.compact, let i = stepIndex, let s = trace?.steps[i] {
            MidgardInspectButton(title: "Inspect \(s.label)", identifier: "mg.mimir.inspect") { inspecting = true }
        }
        MidgardWithheld(count: model.withheld, noun: "session")
        if model.state == .loading { WebMicro("LOADING THE TRACE…") }
        if trace == nil, model.state != .loading {
            Text(model.withheld > 0 ? "Every session trace is above your clearance." : "No session trace.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
                .accessibilityIdentifier("mg.mimir.sessions.none")
        }
        if let t = trace {
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(t.steps.enumerated()), id: \.offset) { i, st in
                    let sel = i == stepIndex
                    Button {
                        ctx.select(i, { selected = $0 }, $inspecting)
                    } label: {
                        HStack(spacing: 12) {
                            Text(st.kind.uppercased())
                                .font(MGWebFont.mono(10))
                                .mgTracking(0.1, size: MGWebFont.monoSize(10))
                                .foregroundStyle(colour(st.kind))
                                .frame(width: 62, alignment: .leading)
                            VStack(alignment: .leading, spacing: 2) {
                                Text(st.label).font(MGWebFont.sans(14).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                                Text(st.detail).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                                    .fixedSize(horizontal: false, vertical: true)
                            }
                            Spacer(minLength: 8)
                            if let ms = st.ms {
                                Text("\(Int(ms))ms").font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                            }
                            if st.kind == "gate" { WebBadge("HUMAN GATE", tone: .warn) }
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 10)
                        .frame(maxWidth: 640, alignment: .leading)
                        .frame(minHeight: 44)
                        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(sel ? MGWeb.surfaceRaised : MGWeb.surfaceCard))
                        .overlay(RoundedRectangle(cornerRadius: MGRadius.md)
                            .strokeBorder(sel ? ctx.accent.fill : st.kind == "gate" ? MGWeb.warn : MGWeb.border))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(Text("\(st.kind): \(st.label). \(st.detail)\(st.ms.map { ", \(Int($0)) milliseconds" } ?? "")"))
                    .accessibilityAddTraits(sel ? [.isButton, .isSelected] : .isButton)
                    .accessibilityIdentifier("mg.mimir.sessions.step.\(i + 1)")
                    if i < t.steps.count - 1 {
                        Rectangle().fill(MGWeb.borderStrong).frame(width: 2, height: 10).padding(.leading, 40)
                            .accessibilityHidden(true)
                    }
                }
            }
            .padding(ctx.compact ? 14 : 22)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("mg.mimir.sessions.trace")
        }
    }

    @ViewBuilder private var aside: some View {
        if let t = trace, let i = stepIndex {
            let s = t.steps[i]
            WebInspectorTitle(symbol: WebIcon.symbol("terminal"), title: s.label, mono: false, accent: ctx.accent,
                              identifier: "mg.mimir.inspector.title") {
                if s.ok == true { WebBadge("OK", tone: .secure) }
                if s.ok == nil { WebBadge("PENDING", tone: .warn) }
            }
            WebPanel {
                VStack(alignment: .leading, spacing: 8) {
                    Text(s.detail).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                    if let ms = s.ms {
                        Text("\(Int(ms))MS").font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                    }
                }
            }
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("MEMORY RECORDS · PQ-ENCRYPTED").padding(.bottom, 8)
                ForEach(Array(t.memory.enumerated()), id: \.offset) { _, m in
                    HStack(spacing: 10) {
                        Text(m.key).foregroundStyle(MGWeb.grey100).lineLimit(1).truncationMode(.tail)
                        Spacer(minLength: 4)
                        Text(m.kind).foregroundStyle(MGWeb.textFaint)
                        Text(m.age).foregroundStyle(MGWeb.textFaint)
                    }
                    .font(MGWebFont.mono(10))
                    .padding(.vertical, 7)
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                    .accessibilityElement(children: .combine)
                }
            }
            .accessibilityIdentifier("mg.mimir.sessions.memory")
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every session trace is above your clearance." : "No step is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

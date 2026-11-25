import MidgardCore
import SwiftUI

// Admin's EMERGENCY and ASSIST screens, matched to the web console: Break-Glass
// (`BreakGlassScreen.tsx`) and Copilot (`CopilotScreen.tsx`). [FR-AD-8] [AC-AD-8]
// [FR-AD-10] [DD-7]

// MARK: - Break-glass

struct AdminBreakGlassScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var model: AdminBreakGlassModel
    @State private var inspecting = false

    static let lifecycle: [(String, String)] = [
        ("Request + justification", "mandatory, written"), ("Dual-control open", "second officer approves"),
        ("Time-boxed grant", "never wider than the emergency"), ("Real-time alerts", "all security officers paged"),
        ("Auto-expiry", "hard stop at the box"), ("Post-hoc review", "forced · itself audited"),
    ]
    static let alerts: [(o: String, c: String, ack: Bool)] = [
        ("a.novak", "page + console", true), ("j.benjamin", "page + console", true), ("security-duty", "ops channel", false),
    ]
    static let constraints: [(String, String)] = [
        ("Scope", "unwrap only · one key family"), ("Excluded", "no policy edits · no IAM · no export"),
        ("Session recording", "every call ledgered as it happens"), ("Hard stop", "07:00:00 · no extension without re-approval"),
    ]

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: AdminBreakGlassModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Break-Glass"),
                       asideIdentifier: ctx.detail("Break-Glass"), inspecting: $inspecting, top: ctx.header("Break-Glass"),
                       asideWidth: ctx.asideWidth("Break-Glass")) {
            main
        } aside: {
            WebSectionLabel(number: 7, "LIFECYCLE", accent: .midgard, muted: true)
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(Self.lifecycle.enumerated()), id: \.offset) { i, step in
                    AdminStep(number: i + 1) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(step.0).font(MGWebFont.sans(12, .caption).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                            Text(step.1.uppercased()).font(MGWebFont.mono(9.5)).foregroundStyle(MGWeb.textFaint)
                        }
                    }
                }
            }
            WebPanel {
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("ABUSE GUARDS")
                    HStack(alignment: .top, spacing: 14) {
                        WebStatBlock(value: "2/mo", label: "RATE LIMIT")
                        WebStatBlock(value: "3", label: "USES · 12MO")
                        WebStatBlock(value: "0", label: "OVERDUE REVIEWS")
                    }
                }
            }
        } asideFoot: {
            WebProvenance([("FR-AD-8", "Time-boxed, justified, alerted break-glass with forced post-hoc review"),
                           ("AC-AD-8", "Granted break-glass is time-boxed, alerted in real time, review forced"),
                           ("AC-AD-12", "Break-glass and audit work fully offline")])
        }
        .task(id: ctx.persona) { await model.load() }
        .onChange(of: model.phase, initial: true) { _, phase in ctx.bar.breakGlassPhase = phase }
    }

    private var phase: WebBreakGlass.State { WebBreakGlass.State(rawValue: model.phase) ?? .review }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 6, section: "EMERGENCY", title: "Break-glass console") {
            HStack(spacing: 6) {
                ForEach([("active", "Active"), ("review", "Review"), ("idle", "Sealed")], id: \.0) { k in
                    WebTag(k.1, active: model.phase == k.0, identifier: "mg.admin.breakglass.phase.\(k.0)") { model.phase = k.0 }
                }
            }
        }
        if ctx.compact {
            MidgardInspectButton(title: "Inspect the lifecycle", identifier: "mg.admin.inspect") { inspecting = true }
        }
        if let s = model.session {
            WebBreakGlass(state: phase, holder: s.holder, scope: s.scope, remaining: s.remaining,
                          justification: phase == .idle ? nil : s.justification)
        } else {
            MidgardWithheld(count: model.withheld, noun: "break-glass state")
        }
        if let notice = model.notice {
            WebNotice(notice.text, tone: notice.tone == .warn ? .warn : .danger, identifier: "mg.admin.breakglass.notice")
        }
        switch phase {
        case .idle:
            WebPanel {
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("OPEN AN EMERGENCY ELEVATION · DUAL-CONTROL")
                    Text("Opening break-glass requires a written justification and a second officer's approval. The grant is time-boxed and never wider than the emergency requires.")
                        .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: 620, alignment: .leading)
                        .padding(.top, 8)
                    WebButton("Request break-glass · second officer", size: .sm,
                              disabledReason: model.session == nil && model.withheld > 0 ? "Above your clearance"
                                  : model.busy ? "The request is in progress" : nil,
                              identifier: "mg.admin.breakglass.request") { Task { await model.request() } }
                        .padding(.top, 12)
                }
            }
        case .active:
            if ctx.compact {
                VStack(alignment: .leading, spacing: 12) {
                    constraints
                    alerts
                }
            } else {
                HStack(alignment: .top, spacing: 12) {
                    constraints.frame(minWidth: 260, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                    alerts.frame(minWidth: 260, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                }
                .fixedSize(horizontal: false, vertical: true)
            }
        case .review:
            WebPanel {
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("POST-HOC REVIEW · DUE IN 18H · FORCED", colour: MGWeb.warnText)
                    Text("Session transcript: 4 unwrap calls, 0 out-of-scope attempts, closed 12 minutes early. Reviewer must confirm the emergency was real, the scope minimal, and the actions proportionate. The review itself becomes a signed audit entry.")
                        .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                        .frame(maxWidth: 680, alignment: .leading)
                        .padding(.top, 8)
                    FlowRow(spacing: 8) {
                        WebButton("Open review · second officer", size: .sm,
                                  disabledReason: "Not available yet: the review is opened by a second officer in the governance service")
                        WebButton("Export transcript", variant: .ghost, size: .sm,
                                  disabledReason: "Not available yet: transcripts are exported by the governance service")
                    }
                    .padding(.top, 12)
                }
            }
        }
        WebTerminal("LEDGER · BREAK-GLASS CLASS", lines: [
            "> admin audit tail --class break-glass", "#B-0331  opened   k.osei · dual-control a.novak · box 60m",
            "#B-0331  call     kms.unwrap enclosure-root-03 · in scope", "#B-0331  closed   47m52s · auto-review queued",
            "# every entry ML-DSA signed · offline capable",
        ])
    }

    private var constraints: some View {
        WebPanel {
            VStack(alignment: .leading, spacing: 4) {
                WebLabel("ACTIVE SESSION CONSTRAINTS")
                WebKeyValues(Self.constraints, keyWidth: 120, upperKeys: true)
            }
        }
    }

    private var alerts: some View {
        WebPanel {
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("ALERTS FIRED · REAL-TIME")
                WebTable([
                    WebColumn("Officer", mono: true) { (r: (o: String, c: String, ack: Bool)) in r.o },
                    WebColumn("Channel", width: 110, mono: true) { $0.c },
                    WebColumn("Ack", width: 90, text: { $0.ack ? "acknowledged" : "pending" }) { r in
                        WebBadge(r.ack ? "ACK" : "PENDING", tone: r.ack ? .secure : .warn)
                    },
                ], rows: Self.alerts, id: \.o, compact: true, asList: ctx.compact, identifier: "mg.admin.breakglass.alerts")
            }
        }
    }
}

// MARK: - Copilot

struct AdminCopilotScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var model: MidgardListModel<AdminCopilotTurn>
    @State private var input = ""
    @State private var inspecting = false

    static let assistants: [(String, String, String)] = [
        ("Admin copilot", "drafts proposed changes · explains state · cites resource ids", "FR-AD-10"),
        ("Sim explainer", "turns a policy-sim diff into plain impact language", "FR-AD-6"),
        ("Audit investigator", "read-only ledger Q&A · every finding cites [audit:id]", "FR-AD-9"),
    ]
    static let refusalCodes: [(String, String)] = [
        ("CONTROL_BYPASS_REFUSED", "asked to skip dual-control or audit"),
        ("OUT_OF_SCOPE", "task exceeds the admin’s role; names the required role"),
    ]

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .copilot(ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Copilot"), asideIdentifier: ctx.detail("Copilot"),
                       inspecting: $inspecting, top: ctx.header("Copilot"), asideWidth: ctx.asideWidth("Copilot")) {
            main
        } aside: {
            WebSectionLabel(number: 9, "THE THREE ASSISTANTS", accent: .midgard, muted: true)
            ForEach(Self.assistants, id: \.0) { a in
                WebPanel {
                    VStack(alignment: .leading, spacing: 6) {
                        HStack(spacing: 8) {
                            Image(systemName: WebIcon.symbol("sparkles")).font(MGWebFont.sans(13))
                                .foregroundStyle(MGWebAccent.midgard.text).accessibilityHidden(true)
                            Text(a.0).font(MGWebFont.sans(14).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                            Spacer(minLength: 4)
                            WebProvenanceTag(a.2, title: "")
                        }
                        Text(a.1).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("REFUSAL CODES").padding(.bottom, 8)
                ForEach(Self.refusalCodes, id: \.0) { c in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(c.0).font(MGWebFont.mono(10)).foregroundStyle(MGWeb.dangerText)
                        Text(c.1.uppercased()).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.textFaint)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                    .accessibilityElement(children: .combine)
                }
            }
        } asideFoot: {
            Text("RUNS ON MIMIR · IN-ENCLOSURE\nEVALS: EV-AD1-ADVISORY … EV-AD3-PATTERN\nALL OUTPUT JSON-CONTRACTED")
                .font(MGWebFont.mono(9)).mgTracking(0.1, size: MGWebFont.monoSize(9))
                .foregroundStyle(MGWeb.textFaint).lineSpacing(6)
                .fixedSize(horizontal: false, vertical: true)
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 8, section: "COPILOT", title: "Advisory copilot") {
            WebStatBlock(value: "0", label: "ACTIONS EXECUTED BY AI · EVER", accent: .midgard)
            WebStatBlock(value: "184", label: "DRAFTS · 30D")
            WebStatBlock(value: "12", label: "REFUSALS · 30D")
        }
        if ctx.compact {
            MidgardInspectButton(title: "Inspect the assistants", identifier: "mg.admin.inspect") { inspecting = true }
        }
        VStack(alignment: .leading, spacing: 16) {
            ForEach(Array(model.rows.enumerated()), id: \.offset) { _, row in
                AdminCopilotTurnView(turn: row.item)
            }
            MidgardWithheld(count: model.withheld, noun: "turn")
            Spacer(minLength: 0)
        }
        .padding(ctx.compact ? 14 : 20)
        .frame(maxWidth: .infinity, minHeight: 240, alignment: .topLeading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.admin.copilot.thread")
        WebFormRow {
            WebTextField(nil, placeholder: "Ask about state, draft a change, investigate audit… (advisory only)",
                         text: $input, identifier: "mg.admin.copilot.input")
        } action: {
            WebButton("Send", trailingSymbol: WebIcon.symbol("arrow-right"),
                      disabledReason: "Not available yet: the copilot thread is read-only in this release",
                      identifier: "mg.admin.copilot.send")
        }
    }
}

/// One turn of the advisory thread (`AdminTurn`): the admin's message on the right, the
/// copilot's proposal (drafted, never executed) or its refusal with the named code on
/// the left.
struct AdminCopilotTurnView: View {
    let turn: AdminCopilotTurn

    var body: some View {
        if turn.role == "admin" {
            HStack {
                Spacer(minLength: 40)
                VStack(alignment: .trailing, spacing: 5) {
                    Text("PLATFORM ADMIN · J.BENJAMIN").font(MGWebFont.mono(9)).mgTracking(0.12, size: MGWebFont.monoSize(9))
                        .foregroundStyle(MGWeb.textFaint)
                    Text(turn.text).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(.horizontal, 14).padding(.vertical, 11)
                        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWebAccent.midgard.tint))
                        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWebAccent.midgard.fill))
                }
                .frame(maxWidth: 520, alignment: .trailing)
            }
            .accessibilityElement(children: .combine)
        } else if turn.variant == "refusal" {
            VStack(alignment: .leading, spacing: 5) {
                Text("REFUSAL · \(turn.refusalCode ?? "")").font(MGWebFont.mono(10)).mgTracking(0.08, size: MGWebFont.monoSize(10))
                    .foregroundStyle(MGWeb.dangerText)
                Text(turn.text).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody).lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 15).padding(.vertical, 12)
            .frame(maxWidth: 620, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.danger))
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("mg.admin.copilot.refusal")
        } else {
            VStack(alignment: .leading, spacing: 5) {
                Text("COPILOT · ADVISORY · CITED").font(MGWebFont.mono(9)).mgTracking(0.12, size: MGWebFont.monoSize(9))
                    .foregroundStyle(MGWeb.textFaint)
                VStack(alignment: .leading, spacing: 10) {
                    Text(turn.text).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody).lineSpacing(3)
                        .fixedSize(horizontal: false, vertical: true)
                    VStack(alignment: .leading, spacing: 9) {
                        FlowRow(spacing: 8) {
                            Text("PROPOSED CHANGE · NOT EXECUTED").font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textHeading)
                            if let risk = turn.proposalRisk { WebRiskBadge(Int(risk), dualControl: false) }
                            if let note = turn.proposalNote {
                                Text(note).font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textFaint)
                            }
                        }
                        FlowRow(spacing: 8) {
                            WebButton("Simulate → review → commit", size: .sm,
                                      disabledReason: "Not available yet: drafts are simulated from Config & policy")
                            WebButton("Discard", variant: .ghost, size: .sm,
                                      disabledReason: "Not available yet: the copilot thread is read-only in this release")
                        }
                    }
                    .padding(.horizontal, 11).padding(.vertical, 9)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
                    .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
                }
                .padding(.horizontal, 15).padding(.vertical, 12)
                .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
            }
            .frame(maxWidth: 620, alignment: .leading)
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("mg.admin.copilot.proposal")
        }
    }
}

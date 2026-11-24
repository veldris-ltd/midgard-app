import MidgardCore
import SwiftUI

// Admin's ADMINISTER screens, matched to the web console: IAM (`IamScreen.tsx`), Config
// (`ConfigScreen.tsx`), Registry (`RegistryScreen.tsx`) and Ledger (`LedgerScreen.tsx`).
// [FR-AD-2] [FR-AD-3] [FR-AD-4] [FR-AD-6] [FR-AD-7] [FR-AD-9] [NFR-PQ-3] [DD-7]

/// A config change's or a model's state as the web's tag tones it.
func adminStateBadge(_ state: String) -> WebBadge {
    let tone: WebBadge.Tone
    switch state {
    case "committed", "approved": tone = .secure
    case "in review", "pending": tone = .warn
    case "simulated": tone = .info
    case "revoked": tone = .danger
    default: tone = .neutral
    }
    return WebBadge(state.uppercased(), tone: tone)
}

// MARK: - IAM

struct AdminIamScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var roles: MidgardListModel<AdminRole>
    @StateObject private var bindings: MidgardListModel<AdminBinding>
    @State private var tab = 0
    @State private var inspecting = false

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _roles = StateObject(wrappedValue: .roles(ctx.app))
        _bindings = StateObject(wrappedValue: .bindings(ctx.app))
    }

    /// What a role can and cannot do (the web screen's split).
    static func canCannot(_ r: AdminRole) -> [(String, Bool)] {
        r.readonly
            ? [("Read all audit + config history", true), ("Export evidence for review", true), ("Mutate any state", false)]
            : [("Administer " + r.scope, true), ("Approve as second controller", true), ("Act outside scope", false),
               ("Self-approve own requests", false)]
    }

    private static func ceiling(_ r: AdminRole) -> AnyView {
        r.readonly ? AnyView(WebBadge("READ-ONLY", tone: .info)) : AnyView(WebRiskBadge(Int(r.ceiling), dualControl: false))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("IAM"), asideIdentifier: ctx.detail("IAM"),
                       inspecting: $inspecting, top: ctx.header("IAM"), asideWidth: ctx.asideWidth("IAM")) {
            main
        } aside: {
            if let r = roles.current {
                WebInspectorTitle(symbol: WebIcon.symbol("fingerprint"), title: r.name, identifier: "mg.admin.inspector.title") {
                    if r.readonly { WebBadge("READ-ONLY", tone: .info) } else { WebRiskBadge(Int(r.ceiling)) }
                }
                WebStatPair(("\(Int(r.holders))", "HOLDERS", false),
                            (r.readonly ? "NONE" : "R\(r.ceiling)", "RISK CEILING", r.ceiling >= 5))
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("CAPABILITY SPLIT · NO SUPER-ADMIN")
                    Text("No role administers more than its column. Cross-column actions require the other column's holder as second approver, so compromise of one admin never spans the suite.")
                        .font(MGWebFont.sans(12, .caption))
                        .foregroundStyle(MGWeb.textMuted)
                        .lineSpacing(3)
                        .fixedSize(horizontal: false, vertical: true)
                }
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("\(r.name.uppercased()) · CAN / CANNOT").padding(.bottom, 8)
                    ForEach(Self.canCannot(r), id: \.0) { item in
                        HStack(spacing: 8) {
                            Image(systemName: item.1 ? "checkmark" : "xmark")
                                .font(MGWebFont.sans(11).weight(.bold))
                                .foregroundStyle(item.1 ? MGWeb.secureText : MGWeb.dangerText)
                                .accessibilityHidden(true)
                            Text(item.0).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                            Spacer(minLength: 0)
                        }
                        .padding(.vertical, 6)
                        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel(Text((item.1 ? "Can: " : "Cannot: ") + item.0))
                        .accessibilityAddTraits(.isStaticText)
                    }
                }
            }
        } asideFoot: {
            if roles.current != nil {
                WebProvenance([("FR-AD-3", "Manage users, groups, roles, markings, purposes and policies with validation"),
                               ("FR-AD-2", "Role separation, expiring bindings, read-only Auditor, no all-powerful role"),
                               ("AC-AD-2", "No single role grants all capabilities")])
            }
        }
        .task(id: ctx.persona) {
            roles.selected = 1
            await roles.load()
            await bindings.load()
        }
    }

    @ViewBuilder private var main: some View {
        let roleRows = roles.rows.map(\.item)
        let roleCount = roleRows.isEmpty ? 5 : roleRows.count
        WebPageHeader(number: 4, section: "IDENTITY", title: "Admin RBAC") {
            WebStatBlock(value: "\(roleCount)", label: "SEPARATED ROLES")
            WebStatBlock(value: "13", label: "LIVE BINDINGS · ALL EXPIRING")
            WebStatBlock(value: "0", label: "SUPER-ADMIN ROLES", accent: .midgard)
        }
        WebTabs([("Roles", "\(roleCount)"), ("Bindings", "13")], selection: $tab, identifier: "mg.admin.iam.tab")
        if tab == 0 {
            if ctx.compact, let r = roles.current {
                MidgardInspectButton(title: "Inspect \(r.name)", identifier: "mg.admin.inspect") { inspecting = true }
            }
            WebTable([
                WebColumn("Role", text: { (r: AdminRole) in r.name }) { r in
                    MidgardIconCell(symbol: WebIcon.symbol("shield-check"), text: r.name)
                },
                WebColumn("Scope", width: 240) { $0.scope },
                WebColumn("Risk ceiling", width: 150, text: { $0.readonly ? "read-only" : "risk \($0.ceiling)" }) { r in
                    Self.ceiling(r)
                },
                // Wider than the web's hints (70, 130), as a browser widens a column to
                // its header and its pill.
                WebColumn("Holders", width: 90, align: .trailing, mono: true) { "\(Int($0.holders))" },
                WebColumn("State", width: 150, text: { $0.statusLabel }) { r in adminPill(r.status, r.statusLabel) },
            ], rows: roleRows, id: \.name, selected: roles.selected, minWidth: 760, asList: ctx.compact,
               identifier: "mg.admin.iam.roles") { i in ctx.select(i, { roles.selected = $0 }, $inspecting) }
            MidgardWithheld(count: roles.withheld, noun: "role")
        } else {
            WebTable([
                WebColumn("Admin", mono: true) { (b: AdminBinding) in b.user },
                WebColumn("Role", width: 150, mono: true) { $0.role },
                WebColumn("Scope", width: 140, mono: true) { $0.scope },
                WebColumn("Expires", width: 80, align: .trailing, mono: true) { $0.expires },
                WebColumn("MFA", width: 160, mono: true) { $0.mfa },
                WebColumn("State", width: 100, text: { _ in "Bound" }) { b in adminPill(b.status, "Bound") },
            ], rows: bindings.rows.map(\.item), id: { "\($0.user):\($0.role)" }, minWidth: 760, asList: ctx.compact,
               identifier: "mg.admin.iam.bindings")
            MidgardWithheld(count: bindings.withheld, noun: "binding")
        }
        WebTerminal("STEP-UP · AC-AD-1", lines: [
            "> session inspect j.benjamin", "login      WebAuthn passkey ✓",
            "admin scope entered → step-up re-auth ✓ (2m ago)",
            "high-risk approval → fresh re-auth REQUIRED at moment of approval",
        ])
    }
}

// MARK: - Config

struct AdminConfigScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var model: AdminConfigModel
    @State private var inspecting = false

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: AdminConfigModel(app: ctx.app))
    }

    private var rollbackDisabled: String? {
        model.busy ? "A change is in progress" : model.current == nil ? "Choose a change to roll back" : nil
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Config"), asideIdentifier: ctx.detail("Config"),
                       inspecting: $inspecting, top: ctx.header("Config"), asideWidth: ctx.asideWidth("Config")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("FR-AD-6", "Policy simulation and dry-run before commit"),
                               ("FR-AD-7", "Versioned config with diff, review, rollback, signed audit"),
                               ("AC-AD-7", "Committed changes are versioned and roll-backable")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
        .onAppear {
            ctx.bar.rollback = { [weak model] in Task { await model?.rollback() } }
        }
        .onChange(of: rollbackDisabled, initial: true) { _, why in ctx.bar.rollbackDisabled = why }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 5, section: "CONFIG", title: "Config as code") {
            WebStatBlock(value: "v481", label: "CURRENT · ALL SUBSYSTEMS")
            WebStatBlock(value: "100%", label: "CHANGES SIMULATED FIRST", accent: .midgard)
            WebStatBlock(value: "4", label: "ROLLBACKS · 90D")
        }
        if ctx.compact, let c = model.current {
            MidgardInspectButton(title: "Inspect \(c.id)", identifier: "mg.admin.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Change", text: { (r: MidgardRow<AdminConfigCommit>) in r.item.title }) { r in
                MidgardIconCell(symbol: WebIcon.symbol("git-commit"), text: r.item.title)
            },
            WebColumn("Id", width: 100, mono: true) { $0.item.id },
            WebColumn("Subsystem", width: 120, mono: true) { $0.item.subsystem },
            WebColumn("Risk", width: 110, text: { "risk \($0.item.risk)" }) { r in WebRiskBadge(Int(r.item.risk), dualControl: false) },
            WebColumn("Author", width: 110, mono: true) { $0.item.author },
            WebColumn("State", width: 120, text: { $0.item.state }) { r in adminStateBadge(r.item.state) },
        ], rows: model.commits, id: \.id, selected: model.selected, minWidth: 760, asList: ctx.compact,
           identifier: "mg.admin.config.commits") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "change")
        WebTerminal("DRIFT RECONCILIATION", lines: [
            "> admin config drift --all", "iam         in sync · v481", "governance  in sync · v481",
            "bifrost     DRIFT · channel beta-eu manual edit → reconcile queued",
            "# drift alerts page the owning admin [FR-AD-7]",
        ])
    }

    @ViewBuilder private var aside: some View {
        if let c = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("git-branch"), title: c.id, identifier: "mg.admin.inspector.title") {
                WebRiskBadge(Int(c.risk))
            }
            VStack(alignment: .leading, spacing: 6) {
                WebLabel("DIFF · VERSIONED LIKE SOURCE").padding(.bottom, 2)
                WebTerminal(lines: c.diff)
                Text("CONTENT-HASH · \(c.contentHash)")
                    .font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textFaint)
            }
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("SIMULATION · BEFORE COMMIT")
                simulation
            }
            if let error = model.error {
                WebNotice(error, tone: .danger, identifier: "mg.admin.config.error")
            }
            if let back = model.rolledBackAt {
                WebNotice("ROLLED BACK · \(back.uppercased()) · SIGNED", tone: .neutral, identifier: "mg.admin.config.rolledback")
            }
            if let version = model.committedAt {
                WebNotice("COMMITTED · \(version.uppercased()) · SIGNED · ROLLBACK AVAILABLE", tone: .secure,
                          identifier: "mg.admin.config.committed")
            } else {
                HStack(spacing: 8) {
                    WebButton(model.simulation == nil ? "Simulate" : "Re-simulate", variant: .secondary, size: .sm,
                              fullWidth: true, disabledReason: model.busy ? "A change is in progress" : nil,
                              identifier: "mg.admin.config.simulate") { Task { await model.simulate() } }
                    WebButton("Commit", size: .sm, fullWidth: true, disabledReason: model.busy ? "A change is in progress" : nil,
                              identifier: "mg.admin.config.commit") { Task { await model.commit() } }
                }
            }
            if ctx.compact {
                WebButton("Rollback…", symbol: WebIcon.symbol("undo-2"), variant: .secondary, size: .sm, fullWidth: true,
                          disabledReason: rollbackDisabled, identifier: "mg.admin.config.rollback.compact") {
                    Task { await model.rollback() }
                }
            }
        }
    }

    private var simulation: some View {
        let sim = model.simulation
        return VStack(alignment: .leading, spacing: 0) {
            Text("EFFECT · \(sim == nil ? "WOULD GRANT" : "SIMULATED")")
                .font(MGWebFont.mono(9)).mgTracking(0.1, size: MGWebFont.monoSize(9))
                .foregroundStyle(MGWeb.warnText)
                .padding(.bottom, 6)
            if let sim {
                Text(sim.summary).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                    .lineSpacing(3).fixedSize(horizontal: false, vertical: true)
                VStack(alignment: .leading, spacing: 3) {
                    ForEach(Array(sim.grants.enumerated()), id: \.offset) { _, g in
                        Text("+ \(g)").foregroundStyle(MGWeb.secureText).fixedSize(horizontal: false, vertical: true)
                    }
                    ForEach(Array(sim.revokes.enumerated()), id: \.offset) { _, r in
                        Text("- \(r)").foregroundStyle(MGWeb.dangerText).fixedSize(horizontal: false, vertical: true)
                    }
                }
                .font(MGWebFont.mono(11))
                .padding(.top, 8)
                .accessibilityIdentifier("mg.admin.config.simulation")
            } else {
                (Text("Grants ") + Text("14 users").foregroundStyle(MGWeb.textHeading).bold() + Text(" read on ")
                    + Text("INTERNAL-marked").foregroundStyle(MGWeb.textHeading).bold()
                    + Text(" maintenance data. No marking exposure above clearance. Reversible. Blast radius: one dataset family."))
                    .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                    .lineSpacing(3).fixedSize(horizontal: false, vertical: true)
            }
            Text("SIM ENGINE · EXPLAINED BY COPILOT · ADVISORY [FR-AD-10]")
                .font(MGWebFont.mono(8.5)).foregroundStyle(MGWeb.textFaint)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 7)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.warn))
    }
}

// MARK: - Registry

struct AdminRegistryScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var model: MidgardListModel<AdminRegistryItem>
    @State private var tab = 0
    @State private var inspecting = false
    /// The candidate's approval on this screen (the web screen's own, not sent).
    @State private var candidateApproved = false

    static let agents: [(a: String, s: String, g: String, e: String)] = [
        ("dispatch-01", "fabric read + schedule_maintenance action", "human", "96%"),
        ("feed-triage", "fabric read only", "pre", "94%"),
        ("release-risk", "artefact metadata read only", "pre", "91%"),
    ]
    static let gates: [(String, String, String)] = [
        ("dispatch agents", "92%", "raised from 90% · CFG-2209"), ("triage agents", "90%", "floor"),
        ("advisory (risk, options)", "88%", "advisory-only surfaces"),
    ]
    static let releases: [(s: String, v: String, a: String, dual: Bool)] = [
        ("Channels", "stable 2.4.1 · beta 2.5.0", "Edit channel policy", false),
        ("Rollout", "v2.5.0 · ring 2 of 4", "Pause / resume / rollback", false),
        ("Fleet-wide release", "73 installations", "Promote to all rings", true),
    ]

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .registry(ctx.app))
    }

    static func lifecycle(_ m: AdminRegistryItem) -> [(String, String, Bool)] {
        [("Approve", "evals gate + licence + hardware eligibility", m.state == "pending"),
         ("Deprecate", "consumers warned · grace window", m.state == "approved"),
         ("Revoke", "immediate · fleet stops serving it", m.state != "revoked")]
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Registry"), asideIdentifier: ctx.detail("Registry"),
                       inspecting: $inspecting, top: ctx.header("Registry"), asideWidth: ctx.asideWidth("Registry")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("FR-AD-4", "Administer the model registry, agent governance, Evals gates, releases and fleet"),
                               ("AC-MI-7", "Weights verified: signature, licence, hardware eligibility · fails closed")])
            }
        }
        .task(id: ctx.persona) {
            model.selected = 4
            await model.load()
        }
    }

    @ViewBuilder private var main: some View {
        let models = model.rows.map(\.item)
        WebPageHeader(number: 10, section: "REGISTRY", title: "Suite governance") {
            WebStatBlock(value: "3", label: "MODELS APPROVED")
            WebStatBlock(value: "12", label: "AGENT SCOPES")
            WebStatBlock(value: "90%", label: "EVALS GATE · FLOOR", accent: .midgard)
        }
        WebTabs([("Model registry", "\(models.isEmpty ? 5 : models.count)"), ("Agent governance", "12"),
                 ("Evals gates", nil), ("Releases & fleet", nil)], selection: $tab, identifier: "mg.admin.registry.tab")
        switch tab {
        case 1:
            WebTable([
                WebColumn("Agent", mono: true) { (r: (a: String, s: String, g: String, e: String)) in r.a },
                WebColumn("Tool scope") { $0.s },
                WebColumn("Gate", width: 160, text: { $0.g == "human" ? "human gate" : "pre-authorised, narrow" }) { r in
                    WebBadge(r.g == "human" ? "HUMAN GATE" : "PRE-AUTH · NARROW", tone: r.g == "human" ? .warn : .secure)
                },
                WebColumn("Evals", width: 80, align: .trailing, mono: true) { $0.e },
            ], rows: Self.agents, id: \.a, minWidth: 620, asList: ctx.compact, identifier: "mg.admin.registry.agents")
        case 2:
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 12)], spacing: 12) {
                ForEach(Self.gates, id: \.0) { g in
                    WebPanel {
                        VStack(alignment: .leading, spacing: 8) {
                            WebStatBlock(value: g.1, label: "GATE · \(g.0)".uppercased(), accent: .midgard)
                            Text(g.2.uppercased()).font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
        case 3:
            WebTable([
                WebColumn("Surface") { (r: (s: String, v: String, a: String, dual: Bool)) in r.s },
                WebColumn("State", width: 200, mono: true) { $0.v },
                WebColumn("Admin action", width: 240) { $0.a },
                WebColumn("Control", width: 140, text: { $0.dual ? "dual-control" : "standard" }) { r in
                    WebBadge(r.dual ? "DUAL-CONTROL" : "STANDARD", tone: r.dual ? .warn : .neutral)
                },
            ], rows: Self.releases, id: \.s, minWidth: 780, asList: ctx.compact, identifier: "mg.admin.registry.releases")
        default:
            if ctx.compact, let m = model.current {
                MidgardInspectButton(title: "Inspect \(m.name)", identifier: "mg.admin.inspect") { inspecting = true }
            }
            WebTable([
                WebColumn("Model", text: { (r: AdminRegistryItem) in r.name }) { r in
                    MidgardIconCell(symbol: WebIcon.symbol("cpu"), text: r.name)
                },
                WebColumn("Family", width: 100, mono: true) { $0.family },
                WebColumn("Signature", width: 110, text: { $0.signature ? "ML-DSA ✓" : "failed" }) { r in
                    WebBadge(r.signature ? "ML-DSA ✓" : "FAILED", tone: r.signature ? .secure : .danger)
                },
                WebColumn("Use", width: 220) { $0.uses },
                WebColumn("Lifecycle", width: 110, text: { $0.state }) { r in adminStateBadge(r.state) },
                WebColumn("Risk", width: 110, text: { "risk \($0.risk)" }) { r in WebRiskBadge(Int(r.risk), dualControl: false) },
            ], rows: models, id: \.name, selected: model.selected, minWidth: 820, asList: ctx.compact,
               identifier: "mg.admin.registry.models") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
            MidgardWithheld(count: model.withheld, noun: "model")
        }
        WebProvenance([("FR-AD-4", ""), ("FR-AD-5", "Model revocation and fleet-wide release are dual-controlled"),
                       ("AC-AD-3", "")])
    }

    @ViewBuilder private var aside: some View {
        if let m = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("cpu"), title: m.name, identifier: "mg.admin.inspector.title") {
                adminStateBadge(m.state)
            }
            WebStatPair((m.family, "FAMILY", false), (m.signature ? "ML-DSA ✓" : "FAILED", "WEIGHTS SIGNATURE", false))
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("LIFECYCLE · ALL DUAL-CONTROL").padding(.bottom, 8)
                ForEach(Self.lifecycle(m), id: \.0) { step in
                    HStack(alignment: .firstTextBaseline, spacing: 10) {
                        Text(step.0.uppercased()).font(MGWebFont.mono(12, .caption))
                            .foregroundStyle(step.2 ? MGWeb.grey100 : MGWeb.textMuted)
                            .frame(width: 76, alignment: .leading)
                        Text(step.1.uppercased()).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.textFaint)
                            .fixedSize(horizontal: false, vertical: true)
                        Spacer(minLength: 0)
                    }
                    .padding(.vertical, 7)
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                    .accessibilityElement(children: .combine)
                    .accessibilityValue(Text(step.2 ? "available" : "not available"))
                }
            }
            if m.state == "pending" {
                WebGuardedAction(action: "Approve " + m.name, subsystem: "MIMIR REGISTRY", risk: 4, requester: "p.iwu",
                                 approver: "j.benjamin", state: candidateApproved ? .approved : .awaiting,
                                 justification: "Evals 94.1% (gate 90%) · KAT + licence verified · hardware: GPU pool only",
                                 identifier: "mg.admin.registry.candidate",
                                 onApprove: { candidateApproved = true }, onDeny: { candidateApproved = false })
            } else {
                WebTerminal("VERIFICATION", lines: [
                    "> admin registry verify " + m.name, "weights   sha3 match · ML-DSA ✓", "licence   policy OK",
                    "hardware  eligibility verified",
                    m.state == "revoked" ? "# REVOKED · serving blocked fleet-wide" : "# approved for serving",
                ])
            }
        }
    }
}

// MARK: - Ledger

struct AdminLedgerScreen: View {
    let ctx: AdminScreenContext
    let link: MidgardLink?
    @StateObject private var model: AdminLedgerModel
    @State private var inspecting = false

    init(ctx: AdminScreenContext, link: MidgardLink?) {
        self.ctx = ctx
        self.link = link
        _model = StateObject(wrappedValue: AdminLedgerModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Ledger"), asideIdentifier: ctx.detail("Ledger"),
                       inspecting: $inspecting, top: ctx.header("Ledger"), asideWidth: ctx.asideWidth("Ledger")) {
            main
        } aside: {
            assistant
        } asideFoot: {
            if let r = model.current {
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("SELECTED ENTRY · #\(r.id)")
                    WebTerminal(lines: ["actor     " + r.actor, "action    " + r.action, "approver  " + r.approver,
                                        "sig       UNSIGNED · sample entry, not checked here",
                                        "# denied + simulated actions are entries too [AC-AD-6]"])
                        .accessibilityIdentifier("mg.admin.ledger.entry")
                }
            }
        }
        .task(id: ctx.persona) {
            await model.load()
            if case let .text(q)? = link {
                model.draft = q
                await model.search()
            }
        }
    }

    /// The live ledger's figure: the share of its records whose signature the core
    /// checked (the seed records carry none).
    private var signedShare: String {
        let rows = model.live
        guard !rows.isEmpty else { return "—" }
        let signed = rows.filter { $0.signature == .verified || $0.signature == .device }.count
        return "\(Int((100 * Double(signed) / Double(rows.count)).rounded()))%"
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 11, section: "LEDGER", title: "Audit search") {
            WebStatBlock(value: "7", label: "SUBSYSTEMS · ONE LEDGER")
            WebStatBlock(value: signedShare, label: "SIGNED · TAMPER-EVIDENT", accent: .midgard)
            WebStatBlock(value: "0", label: "ENTRIES ESCAPED")
        }
        let layout = ctx.compact ? AnyLayout(VStackLayout(alignment: .leading, spacing: 10))
                                 : AnyLayout(HStackLayout(alignment: .center, spacing: 10))
        layout {
            WebTextField(nil, placeholder: "actor:k.osei subsystem:kms since:24h…", text: $model.draft, symbol: "magnifyingglass",
                         identifier: "mg.admin.ledger.search", width: ctx.compact ? nil : 320) {
                Task { await model.search() }
            }
            FlowRow(spacing: 6) {
                ForEach(AdminLedgerModel.filters, id: \.self) { f in
                    WebTag(f, active: model.filter == f, identifier: "mg.admin.ledger.filter.\(f)") { model.filter = f }
                }
            }
        }
        if let error = model.error { WebNotice(error, tone: .danger, identifier: "mg.admin.ledger.error") }
        if ctx.compact, let r = model.current {
            MidgardInspectButton(title: "Inspect #\(r.id)", identifier: "mg.admin.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Entry", width: 90, text: { (x: AdminLedgerEntry) in "#\(x.id)" }) { x in
                Text("#\(x.id)").font(MGWebFont.mono(12, .caption))
                    .foregroundStyle(x.action.contains("DENIED") ? MGWeb.dangerText : MGWebAccent.midgard.text)
            },
            WebColumn("Actor", width: 110, mono: true) { $0.actor },
            WebColumn("Action") { $0.action },
            WebColumn("Subsystem", width: 120, mono: true) { $0.subsystem },
            WebColumn("Justification", width: 180) { $0.justification },
            WebColumn("Approver", width: 100, mono: true) { $0.approver },
            WebColumn("When", width: 110, mono: true) { $0.ts },
        ], rows: model.rows, id: \.id, selected: model.currentIndex, minWidth: 880, asList: ctx.compact,
           identifier: "mg.admin.ledger.entries") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
        if model.rows.isEmpty, model.state != .loading { WebMicro("NO ENTRIES MATCH THIS SEARCH · APPEND-ONLY LEDGER") }
        MidgardWithheld(count: model.withheld, noun: "entry")
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 13, "LEDGER LINEAGE", accent: .midgard, muted: true)
            MidgardAuditTable(rows: model.live, compact: ctx.compact, withId: false, selected: nil,
                              identifier: "mg.admin.ledger.live", onSelect: nil)
            MidgardWithheld(count: model.liveWithheld, noun: "record")
        }
        WebProvenance([("FR-AD-9", "Aggregated observability: health, SLOs, security events, audit search"),
                       ("AC-AD-6", "Every admin action including denied and simulated is ledgered"),
                       ("NFR-PQ-3", "Append-only, ML-DSA-signed, tamper-evident, exportable")])
    }

    @ViewBuilder private var assistant: some View {
        WebSectionLabel(number: 12, "INVESTIGATION ASSISTANT · READ-ONLY", accent: .midgard, muted: true)
        Text("Who touched the KMS in the last 24 hours, and was anything unusual?")
            .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 13).padding(.vertical, 11)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWebAccent.midgard.tint))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWebAccent.midgard.fill))
        VStack(alignment: .leading, spacing: 0) {
            (Text("Two KMS events in 24h: a dual-controlled rotation approval ")
                + Text("[audit:A-9108]").font(MGWebFont.mono(10)).foregroundStyle(MGWebAccent.midgard.text)
                + Text(" and a break-glass session ")
                + Text("[audit:B-0331]").font(MGWebFont.mono(10)).foregroundStyle(MGWebAccent.midgard.text)
                + Text(", both by k.osei with a.novak as second officer."))
                .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                .lineSpacing(3).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 3) {
                Text("PATTERN FOR HUMAN REVIEW").font(MGWebFont.mono(9)).mgTracking(0.1, size: MGWebFont.monoSize(9))
                    .foregroundStyle(MGWeb.warnText)
                Text("SAME REQUESTER/APPROVER PAIR ON BOTH EVENTS · NO INTENT ASSERTED")
                    .font(MGWebFont.mono(11)).foregroundStyle(MGWeb.textBody)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 10).padding(.vertical, 8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.warn))
            .padding(.top, 9)
            Text("EVERY FINDING CITED · OBSERVE-ONLY · CANNOT MUTATE [FR-AD-10]")
                .font(MGWebFont.mono(8.5)).foregroundStyle(MGWeb.textFaint)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.top, 8)
        }
        .padding(.horizontal, 14).padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .combine)
    }
}

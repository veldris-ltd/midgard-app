import MidgardCore
import SwiftUI

// Admin's OVERSEE screens, matched to the web console: Overview (`OverviewScreen.tsx`)
// and Approvals (`ApprovalsScreen.tsx`). [FR-AD-1] [FR-AD-5] [FR-AD-9] [AC-AD-4]
// [AC-AD-6] [DD-7]

// MARK: - Overview

struct AdminOverviewScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var model: MidgardListModel<AdminOverview>
    /// Whether the persona may read the break-glass state the strip repeats.
    @StateObject private var breakGlass: AdminBreakGlassModel
    @State private var selected = 4
    @State private var inspecting = false

    static let binding: [(String, String)] = [
        ("Scope", "admin API only · no data plane"), ("Transport", "PQ-mTLS · admin zone"),
        ("Blast radius", "this subsystem only"),
    ]

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .overview(ctx.app))
        _breakGlass = StateObject(wrappedValue: AdminBreakGlassModel(app: ctx.app))
    }

    private var overview: AdminOverview? { model.rows.first?.item }
    private var subsystems: [AdminSubsystem] { overview?.subsystems ?? [] }
    private var current: AdminSubsystem? { subsystems.indices.contains(selected) ? subsystems[selected] : nil }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Overview"), asideIdentifier: ctx.detail("Overview"),
                       inspecting: $inspecting, top: ctx.header("Overview"), asideWidth: ctx.asideWidth("Overview")) {
            main
        } aside: {
            if let s = current {
                WebInspectorTitle(symbol: WebIcon.symbol("server"), title: s.name, mono: false,
                                  identifier: "mg.admin.inspector.title") { adminPill(s.status, s.statusLabel) }
                WebStatPair((s.slo, "SLO · 30D", false), ("\(Int(s.openFindings))", "OPEN FINDINGS", s.openFindings > 0))
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("PANEL BINDING · LEAST PRIVILEGE")
                    WebKeyValues([("Identity", "svc:admin/" + s.api)] + Self.binding, keyWidth: 90, upperKeys: true)
                }
            }
        } asideFoot: {
            if current != nil {
                WebTerminal("AR-SEC · SELF-GOVERNANCE", lines: [
                    "> admin policy self-check", "policy point   shared · no bypass", "audit path     shared ledger",
                    "super-admin    NOT DEFINED", "# the panel is governed by what it governs",
                ])
            }
        }
        .task(id: ctx.persona) {
            await model.load()
            await breakGlass.load()
        }
    }

    @ViewBuilder private var main: some View {
        let o = overview
        WebPageHeader(number: 1, section: "ADMIN", title: "Suite oversight") {
            WebStatBlock(value: "\(subsystems.isEmpty ? 7 : subsystems.count)", label: "SUBSYSTEMS")
            WebStatBlock(value: "\(Int(o?.pendingApprovals ?? 0))", label: "PENDING APPROVALS", accent: .midgard)
            WebStatBlock(value: "\(Int(o?.breakGlassReview ?? 0))", label: "BREAK-GLASS · REVIEW")
            WebStatBlock(value: "\(Int(o?.superAdminRoles ?? 0))", label: "SUPER-ADMIN ROLES")
        }
        // Side by side, two to one, as the web's flex row; stacked on the iPhone.
        if ctx.compact {
            VStack(alignment: .leading, spacing: 12) {
                strip
                security
            }
        } else {
            HStack(alignment: .top, spacing: 12) {
                strip.frame(minWidth: 320, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
                security.frame(minWidth: 220, maxWidth: 300, maxHeight: .infinity, alignment: .topLeading)
            }
            .fixedSize(horizontal: false, vertical: true)
        }
        if ctx.compact, let s = current {
            MidgardInspectButton(title: "Inspect \(s.name)", identifier: "mg.admin.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Subsystem", text: { (r: AdminSubsystem) in r.name }) { r in
                MidgardIconCell(symbol: WebIcon.symbol("server"), text: r.name)
            },
            WebColumn("Admin surface", width: 160, mono: true) { $0.api },
            WebColumn("SLO · 30d", width: 100, align: .trailing, mono: true) { $0.slo },
            WebColumn("State", width: 130, text: { $0.statusLabel }) { r in adminPill(r.status, r.statusLabel) },
        ], rows: subsystems, id: { ConsoleCatalog.slug($0.name) }, selected: selected, minWidth: 640, asList: ctx.compact,
           identifier: "mg.admin.overview.subsystems") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "overview")
        if model.state == .loading { WebMicro("AGGREGATING SUBSYSTEMS…") }
        WebProvenance([("FR-AD-1", "One console composing existing subsystem admin APIs"),
                       ("FR-AD-9", "Aggregated cross-subsystem observability"),
                       ("AC-AD-10", "Panel outage pauses administration only, never the data plane")])
        WebButton("Audit search", symbol: WebIcon.symbol("search"), variant: .secondary, size: .sm,
                  identifier: "mg.admin.overview.audit-search") { ctx.open("Ledger", nil) }
            .help("Open the audit ledger")
    }

    /// The post-hoc review the break-glass state is in, as the web's strip draws it;
    /// withheld for a persona below the state's marking.
    @ViewBuilder private var strip: some View {
        if breakGlass.session != nil {
            WebBreakGlass(state: .review, holder: "k.osei", scope: "KMS · unwrap only",
                          justification: "HSM failover during rotation window · expired 06:12 · review due in 18h")
        } else if breakGlass.withheld > 0 {
            WebNotice("BREAK-GLASS STATE WITHHELD · ABOVE YOUR CLEARANCE", tone: .neutral, identifier: "mg.admin.overview.withheld")
        }
    }

    private var security: some View {
        let s = overview?.security
        return WebPanel {
            VStack(alignment: .leading, spacing: 10) {
                WebLabel("SECURITY EVENTS · 24H")
                HStack(alignment: .top, spacing: 16) {
                    WebStatBlock(value: "\(Int(s?.deniedActions ?? 0))", label: "DENIED ACTIONS")
                    WebStatBlock(value: "\(Int(s?.selfApprovals ?? 0))", label: "SELF-APPROVALS")
                    WebStatBlock(value: "\(Int(s?.anomalyFlags ?? 0))", label: "ANOMALY FLAGS",
                                 accent: (s?.anomalyFlags ?? 0) > 0 ? .midgard : nil)
                }
            }
        }
    }
}

// MARK: - Approvals

struct AdminApprovalsScreen: View {
    let ctx: AdminScreenContext
    @StateObject private var model: AdminApprovalsModel
    @State private var inspecting = false

    static let guardedPath = [
        "Risk classified (1–5)", "Justification captured", "Second approver routed out-of-band",
        "Separation of duties verified", "Step-up re-auth at approval", "Time-boxed single-use token",
        "PQ-signed audit · incl. denials",
    ]

    init(ctx: AdminScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: AdminApprovalsModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Approvals"), asideIdentifier: ctx.detail("Approvals"),
                       inspecting: $inspecting, top: ctx.header("Approvals"), asideWidth: ctx.asideWidth("Approvals")) {
            main
        } aside: {
            WebSectionLabel(number: 3, "THE GUARDED PATH", accent: .midgard, muted: true)
            VStack(alignment: .leading, spacing: 0) {
                ForEach(Array(Self.guardedPath.enumerated()), id: \.offset) { i, step in
                    AdminStep(number: i + 1) {
                        Text(step).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                    }
                }
            }
            WebPanel {
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("APPROVER POOL HEALTH")
                    HStack(alignment: .top, spacing: 14) {
                        WebStatBlock(value: "8", label: "ELIGIBLE")
                        WebStatBlock(value: "31d", label: "LAST ROTATION")
                        WebStatBlock(value: "0", label: "COLLUSION FLAGS")
                    }
                }
            }
        } asideFoot: {
            WebProvenance([("FR-AD-5", "Dual-control on key rotation, escalation, model revocation, fleet release, break-glass"),
                           ("AC-AD-4", "Self-approval denied by separation-of-duties check"),
                           ("AC-AD-6", "Denied and simulated actions are audited too")])
        }
        .task(id: ctx.persona) { await model.load() }
        .onChange(of: model.awaiting, initial: true) { _, n in ctx.bar.awaiting = n }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 2, section: "DUAL-CONTROL", title: "Approval queue") {
            WebStatBlock(value: "\(model.awaiting)", label: "AWAITING", accent: .midgard)
            WebStatBlock(value: "\(14 + model.approved)", label: "APPROVED · 7D")
            WebStatBlock(value: "\(2 + model.deniedCount)", label: "DENIED · 7D")
        }
        if ctx.compact {
            MidgardInspectButton(title: "Inspect the guarded path", identifier: "mg.admin.inspect") { inspecting = true }
        }
        if let notice = model.notice {
            WebNotice(notice.text, tone: notice.tone == .warn ? .warn : notice.tone == .secure ? .secure : .danger,
                      identifier: "mg.admin.approvals.notice")
        }
        VStack(spacing: 10) {
            ForEach(model.items, id: \.id) { it in
                WebGuardedAction(action: it.action, subsystem: it.subsystem, risk: Int(it.risk), requester: it.requester,
                                 approver: it.approver, state: WebGuardedAction.State(rawValue: it.state) ?? .awaiting,
                                 justification: it.justification, deniedReason: it.deniedReason,
                                 busy: model.busy != nil, identifier: "mg.admin.approval.\(it.id)",
                                 onApprove: { Task { await model.approve(it) } },
                                 onDeny: { model.deny(it) })
            }
            if model.state == .loading && model.items.isEmpty { WebMicro("ROUTING SECOND APPROVERS…") }
        }
        MidgardWithheld(count: model.withheld, noun: "approval")
        FlowRow(spacing: 8) {
            WebLabel("RISK LEGEND")
            WebRiskBadge(3, dualControl: false)
            WebRiskBadge(4)
            WebRiskBadge(5)
        }
        WebTerminal("AUDIT · INCLUDING DENIALS", lines: model.tail)
            .accessibilityIdentifier("mg.admin.approvals.tail")
    }
}

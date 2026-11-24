import MidgardCore
import SwiftUI

// Bifrost's FLEET screens, matched to the web console's: Fleet & drift
// (`FleetScreen.tsx`) and Rollback centre (`RollbackScreen.tsx`). [FR-B-2] [FR-B-3]
// [NFR-SEC-1]

// MARK: - Fleet & drift

struct BifrostFleetScreen: View {
    let ctx: BifrostScreenContext
    @StateObject private var model: MidgardListModel<BifrostFleetNode>
    @State private var inspecting = false

    init(ctx: BifrostScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .fleet(ctx.app))
    }

    private var converged: Int { model.rows.filter { $0.item.drift == "none" }.count }
    private var versionDrift: Int {
        model.rows.filter { $0.item.drift.hasPrefix("version") || $0.item.drift.contains("bundle") }.count
    }
    private var configDrift: Int { model.rows.filter { $0.item.drift.hasPrefix("config") }.count }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Fleet"), asideIdentifier: ctx.detail("Fleet"),
                       inspecting: $inspecting, top: ctx.header("Fleet"), asideWidth: ctx.asideWidth("Fleet")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("FR-B-3", "Declarative release channels and fleet/installation management"),
                           ("NFR-SEC-1", "Least-privilege on-host agents, signed instructions")])
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 6, section: "FLEET", title: "Fleet & drift", accent: ctx.accent) {
            WebStatBlock(value: "\(model.rows.count)", label: "INSTALLATIONS")
            WebStatBlock(value: "\(converged)", label: "CONVERGED", accent: ctx.accent)
            WebStatBlock(value: "\(model.rows.count - converged)", label: "DRIFTED / HELD")
            WebButton("Reconcile all", symbol: WebIcon.symbol("refresh-cw"), size: .sm,
                      disabledReason: "Not available in this app: the fleet is reconciled in the enclosure",
                      accent: ctx.accent, identifier: "mg.bifrost.fleet.reconcile")
        }
        FlowRow(spacing: 12) {
            ForEach([("CONVERGED", converged, MGWeb.secure), ("VERSION DRIFT", versionDrift, MGWeb.warn),
                     ("CONFIG DRIFT", configDrift, MGWeb.warn)], id: \.0) { title, n, colour in
                WebPanel {
                    HStack(spacing: 10) {
                        Circle().fill(colour).frame(width: 8, height: 8).accessibilityHidden(true)
                        Text("\(n)").font(MGWebFont.display(22, .title2).weight(.heavy)).foregroundStyle(MGWeb.textHeading)
                        WebLabel(title)
                    }
                    .frame(width: ctx.compact ? nil : 180, alignment: .leading)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("\(title.capitalized): \(n)"))
                .accessibilityAddTraits(.isStaticText)
            }
        }
        if ctx.compact, let f = model.current {
            MidgardInspectButton(title: "Inspect \(f.id)", identifier: "mg.bifrost.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Installation", text: { (r: MidgardRow<BifrostFleetNode>) in r.item.id }) { r in
                BifrostIconCell(symbol: WebIcon.symbol("server"), text: r.item.id)
            },
            WebColumn("Ring", width: 100, mono: true) { $0.item.ring },
            WebColumn("Desired", width: 90, mono: true) { $0.item.desired },
            WebColumn("Actual", width: 100, mono: true) { $0.item.actual },
            WebColumn("Drift", width: 140, mono: true) { $0.item.drift },
            // Wider than the web's 110 and 160: the core's "not checked (demo)" in place of
            // the mock's agent claim, wrapped, and "Awaiting courier" were cut at the Mac's
            // default window.
            WebColumn("Agent", width: 126, fitted: true, text: { $0.item.agent }) { r in
                WebBadge(r.item.agent, tone: r.item.agent == "offline" ? .neutral : .secure, lines: 2)
            },
            WebColumn("State", width: 184, text: { $0.item.stateLabel }) { r in bifrostPill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: model.rows.isEmpty ? nil : model.selected, minWidth: 860,
           asList: ctx.compact, accent: ctx.accent, identifier: "mg.bifrost.fleet.table") { i in
            ctx.select(i, { model.selected = $0 }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "installation")
        if model.state == .loading { WebMicro("RECONCILING FLEET…") }
    }

    @ViewBuilder private var aside: some View {
        if let f = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("server"), title: f.id, accent: ctx.accent,
                              identifier: "mg.bifrost.inspector.title") {
                bifrostPill(f.state, f.stateLabel)
            }
            WebStatPair((f.desired, "DESIRED", false), (f.actual, "ACTUAL", f.drift != "none"), accent: ctx.accent)
            if f.drift.hasPrefix("config") {
                BifrostNoteCard(heading: "CONFIG DRIFT", colour: MGWeb.warnText, border: MGWeb.warn, text: nil,
                                identifier: "mg.bifrost.fleet.config-drift") {
                    WebTerminal(lines: ["- fusion.batch_window: 200ms", "+ fusion.batch_window: 350ms  # local edit"],
                                accent: ctx.accent, small: true)
                    HStack(spacing: 8) {
                        WebButton("Revert to desired", size: .sm, fullWidth: true,
                                  disabledReason: "Not available in this app: drift is reverted by the on-host agent",
                                  accent: ctx.accent, identifier: "mg.bifrost.fleet.revert")
                        WebButton("Adopt", variant: .ghost, size: .sm,
                                  disabledReason: "Not available in this app: a local edit is adopted in the enclosure",
                                  accent: ctx.accent, identifier: "mg.bifrost.fleet.adopt")
                    }
                    .padding(.top, 4)
                }
            }
            BifrostRules(title: "ON-HOST AGENT", rows: [
                ("Identity", f.agent == "offline" ? "signed bundle only" : "workload · ML-DSA"),
                ("Instructions", "signed · least-privilege"),
                ("Heartbeat", f.agent == "offline" ? "courier cycle" : "10s · healthy"),
            ], keyWidth: 100)
        } else if model.state != .loading {
            BifrostNothingSelected(withheld: model.withheld, noun: "installation")
        }
    }
}

// MARK: - Rollback centre

struct BifrostRollbackScreen: View {
    let ctx: BifrostScreenContext
    @StateObject private var model: BifrostRollbackModel
    @State private var inspecting = false

    init(ctx: BifrostScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: BifrostRollbackModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Rollback"), asideIdentifier: ctx.detail("Rollback"),
                       inspecting: $inspecting, top: ctx.header("Rollback"), asideWidth: ctx.asideWidth("Rollback")) {
            main
        } aside: {
            aside
        } asideFoot: {
            executeButton
        }
        .task(id: ctx.persona) {
            await model.load()
            publish()
        }
        .onChange(of: model.drilling) { _, _ in publish() }
    }

    /// The header's Run drill (the web's toolbar button).
    private func publish() {
        ctx.bar.drilling = model.drilling
        ctx.bar.runDrill = { [weak model] in
            Task { @MainActor in await model?.runDrill() }
        }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 5, section: "ROLLBACK", title: "Rollback centre", accent: ctx.accent) {
            WebStatBlock(value: "\(model.rows.count)", label: "STANDING PLANS")
            WebStatBlock(value: "41s", label: "LAST DRILL · TO HEALTHY", accent: ctx.accent)
            WebStatBlock(value: "0", label: "UNPLANNED ROLLBACKS · 90D")
        }
        if ctx.compact, let p = model.current {
            MidgardInspectButton(title: "Inspect \(p.id)", identifier: "mg.bifrost.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Plan", text: { (r: MidgardRow<BifrostRollback>) in r.item.scope }) { r in
                BifrostIconCell(symbol: WebIcon.symbol("undo-2"), text: r.item.scope)
            },
            WebColumn("Kind", width: 120, mono: true) { $0.item.kind },
            WebColumn("Trigger", width: 170) { $0.item.trigger },
            WebColumn("Readiness", width: 160, mono: true) { $0.item.readiness },
            WebColumn("Last drill", width: 100, mono: true) { $0.item.lastDrill },
            WebColumn("State", width: 120, text: { $0.item.stateLabel }) { r in bifrostPill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: model.rows.isEmpty ? nil : model.selected, minWidth: 840,
           asList: ctx.compact, accent: ctx.accent, identifier: "mg.bifrost.rollback.table") { i in
            ctx.select(i, { model.select($0) }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "plan")
        if model.state == .loading { WebMicro("LOADING PLANS…") }
        WebTerminal(model.drilling ? "DRILL · RUNNING" : "DRILL LOG", lines: model.log, accent: ctx.accent)
            .accessibilityIdentifier("mg.bifrost.rollback.log")
    }

    @ViewBuilder private var aside: some View {
        if let row = model.current {
            let p = row.item
            WebInspectorTitle(symbol: WebIcon.symbol("undo-2"), title: "\(p.kind) rollback", mono: false, accent: ctx.accent,
                              identifier: "mg.bifrost.inspector.title") {
                bifrostPill(p.state, p.stateLabel)
            }
            WebPanel {
                Text(p.scope).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.grey100)
                    .lineSpacing(4).fixedSize(horizontal: false, vertical: true)
            }
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("AUTOMATIC TRIGGERS").padding(.bottom, 8)
                ForEach(model.triggers, id: \.self) { t in
                    HStack(spacing: 8) {
                        Image(systemName: WebIcon.symbol("zap")).font(MGWebFont.sans(12)).foregroundStyle(MGWeb.warnText)
                            .accessibilityHidden(true)
                        Text(t).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                        Spacer(minLength: 0)
                    }
                    .padding(.vertical, 6)
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                }
            }
            VStack(alignment: .leading, spacing: 10) {
                WebLabel("HEALTH GATE")
                WebBadge(model.gated ? "GATE HELD" : "GATE CLEAR", tone: model.gated ? .warn : .secure)
                    .accessibilityIdentifier("mg.bifrost.rollback.gate")
                Text("Execution is health-gated: a plan runs only while armed and its health gate is clear. The reversal reverses ring order — broad before canary — with zero downtime, and the decision is signed lineage.")
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.textMuted)
                    .lineSpacing(4)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let id = model.executedAudit {
                BifrostAuditLine(label: "EXECUTED · AUDIT", auditId: id, identifier: "mg.bifrost.rollback.audit")
            }
            if let message = model.message {
                BifrostMessage(text: message, identifier: "mg.bifrost.rollback.message")
            }
            WebProvenance([("FR-B-2", "Zero-downtime upgrades with health-gated automatic rollback")])
        } else if model.state != .loading {
            BifrostNothingSelected(withheld: model.withheld, noun: "plan")
        }
    }

    @ViewBuilder private var executeButton: some View {
        if model.current != nil {
            WebButton(model.executing ? "Rolling back…" : model.gated ? "Rollback held by gate" : "Execute rollback",
                      symbol: WebIcon.symbol("undo-2"), size: .sm, fullWidth: true,
                      disabledReason: model.gated ? "Rollback held by gate" : model.executing ? "The rollback is in flight" : nil,
                      accent: ctx.accent, identifier: "mg.bifrost.rollback.execute") {
                Task { await model.execute() }
            }
        }
    }
}

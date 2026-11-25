import MidgardCore
import SwiftUI

// Bifrost's DELIVERY screens, matched to the web console's: Release Console
// (`ReleaseConsoleScreen.tsx`), Artefacts (`ArtefactsScreen.tsx`), Channels
// (`ChannelsScreen.tsx`) and Offline bundles (`BundlesScreen.tsx`). Buttons the web
// leaves without a handler are disabled with the reason. [FR-B-2] [FR-B-3] [FR-B-4]
// [FR-B-5] [FR-B-6]

// MARK: - Release Console

struct BifrostReleaseScreen: View {
    let ctx: BifrostScreenContext
    @StateObject private var model: BifrostReleaseModel
    @State private var inspecting = false

    init(ctx: BifrostScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: BifrostReleaseModel(app: ctx.app))
    }

    private var version: String { model.release?.version ?? "2.5.0" }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Release Console"),
                       asideIdentifier: ctx.detail("Release Console"), inspecting: $inspecting,
                       top: ctx.header("Release Console"), asideWidth: ctx.asideWidth("Release Console")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebButton("Prepare rollback plan", symbol: WebIcon.symbol("undo-2"), variant: .secondary, size: .sm,
                      disabledReason: "Not available in this app: rollback plans are prepared in the enclosure",
                      accent: ctx.accent, identifier: "mg.bifrost.release.prepare")
        }
        .task(id: ctx.persona) {
            await model.load()
            publish()
        }
        .onChange(of: model.promoting) { _, _ in publish() }
        .onChange(of: model.gate) { _, _ in publish() }
    }

    /// The header's Promote (the web's toolbar button).
    private func publish() {
        ctx.bar.version = version
        ctx.bar.channel = model.release?.channel ?? "stable"
        ctx.bar.gate = model.gate
        ctx.bar.promoting = model.promoting
        ctx.bar.promoteDisabled = model.promoteDisabled
        ctx.bar.promote = { [weak model] in
            Task { @MainActor in await model?.promote() }
        }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 1, section: "ROLLOUT · v\(version)", title: "Zero-downtime rollout", accent: ctx.accent) {
            WebStatBlock(value: "\(model.rolled)/\(model.capacity)", label: "INSTALLATIONS")
            WebStatBlock(value: bifrostCount(model.release?.failedRequests ?? 0), label: "FAILED REQUESTS",
                         accent: ctx.accent)
            WebStatBlock(value: model.release?.rollbackGate ?? "AUTO", label: "ROLLBACK GATE")
        }
        if ctx.compact {
            MidgardInspectButton(title: "Inspect release intelligence", identifier: "mg.bifrost.inspect") {
                inspecting = true
            }
        }
        FlowRow(spacing: 12) {
            ForEach(model.rings, id: \.id) { row in ring(row.item) }
        }
        if let message = model.message {
            BifrostMessage(text: message, identifier: "mg.bifrost.release.message")
        }
        if ctx.compact, let id = model.lastAudit {
            BifrostAuditLine(label: "LAST STEP · AUDIT", auditId: id, identifier: "mg.bifrost.release.audit")
        }
        MidgardWithheld(count: model.withheld, noun: "row")
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 2, "FLEET", accent: ctx.accent, muted: true)
            WebTable([
                WebColumn("Installation", text: { (r: MidgardRow<BifrostFleetNode>) in r.item.id }) { r in
                    BifrostIconCell(symbol: WebIcon.symbol("server"), text: r.item.id)
                },
                WebColumn("Ring", width: 110, mono: true) { $0.item.ring },
                WebColumn("Version", width: 110, mono: true) { $0.item.actual },
                WebColumn("Target", width: 90, mono: true) { $0.item.desired },
                // Wider than the web's 160: "Awaiting courier" was cut at the table's edge.
                WebColumn("State", width: 184, text: { $0.item.stateLabel }) { r in
                    bifrostPill(r.item.state, r.item.stateLabel)
                },
            ], rows: model.fleet, id: \.id, minWidth: 640, asList: ctx.compact, accent: ctx.accent,
               identifier: "mg.bifrost.release.fleet")
            if model.state == .loading { WebMicro("RECONCILING RINGS…") }
        }
        WebTerminal("SUPPLY CHAIN · v\(version)", lines: [
            "> bifrost verify \(model.release?.name ?? "midgard-suite"):\(version)",
            "build      reproducible ✓ (bit-identical 2/2)",
            "sbom       CycloneDX attached · 0 critical CVEs",
            "signature  ML-DSA verified · key enclosure-root-03",
            "# verified, promotion permitted [FR-B-4]",
        ], accent: ctx.accent)
    }

    /// A ring's card: its name, OFFLINE BUNDLE on the air-gap ring, done/total and the
    /// meter, warn on the broad ring while it is promoted.
    private func ring(_ r: BifrostRing) -> some View {
        let pct = bifrostPercent(done: r.done, total: r.total)
        let rolling = r.stage == BifrostReleaseModel.promoteStage && model.promoting
        return WebPanel(accentRule: rolling, accent: ctx.accent) {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .top, spacing: 8) {
                    // OFFLINE BUNDLE under the name: beside it, both were cut in the
                    // card's 190 pt ("RING 3 · …", "OFFLIN…").
                    VStack(alignment: .leading, spacing: 6) {
                        Text(r.name)
                            .font(MGWebFont.mono(10))
                            .mgTracking(0.1, size: MGWebFont.monoSize(10))
                            .foregroundStyle(MGWeb.textMuted)
                            .lineLimit(1)
                        if r.offlineBundle == true { WebBadge("OFFLINE BUNDLE", tone: .neutral) }
                    }
                    Spacer(minLength: 4)
                    Text("\(bifrostCount(r.done))/\(bifrostCount(r.total))")
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(pct == 100 ? MGWeb.secureText : ctx.accent.text)
                }
                WebMeterBar(Double(pct), width: 140, warnAt: rolling && pct < 100 ? 0 : nil, accent: ctx.accent,
                            format: { "\(Int($0))%" })
            }
            .frame(width: ctx.compact ? nil : 190, alignment: .leading)
        }
        // One element over the drawn card (`WholeElement`): on the iPhone the last ring
        // rests under the console bar, where its texts were measured against the bar
        // (first audit pass, "0/9").
        .modifier(WholeElement(label: "\(r.name)\(r.offlineBundle == true ? ", offline bundle" : ""): \(bifrostCount(r.done)) of \(bifrostCount(r.total)), \(pct) percent",
                               traits: .isStaticText, identifier: "mg.bifrost.ring.\(r.id)", action: nil))
    }

    @ViewBuilder private var aside: some View {
        WebSectionLabel(number: 3, "RELEASE INTELLIGENCE", accent: ctx.accent, muted: true)
        WebPanel(accentRule: true, accent: ctx.accent) {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    WebStatBlock(value: model.release?.riskScore ?? "LOW", label: "RISK SCORE · MIMIR", accent: ctx.accent)
                    Spacer(minLength: 4)
                    Text("LOCAL · Mistral Small 3.2").font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textFaint)
                }
                Text("Diff surface is 84% config, 16% code; no schema migrations. Historical anomaly match: none. Advisory by default; configurable to gate the rollout. Rule-based health gates always apply, with or without Mimir.")
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.textBody)
                    .lineSpacing(4)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        VStack(alignment: .leading, spacing: 0) {
            WebLabel("HEALTH GATES").padding(.bottom, 8)
            ForEach([("p99 latency < 250ms", true), ("error rate < 0.1%", true),
                     ("enclave-ams-03 CPU", model.gate == "pass")], id: \.0) { label, secure in
                HStack(spacing: 8) {
                    Circle().fill(secure ? MGWeb.secure : MGWeb.warn).frame(width: 7, height: 7).accessibilityHidden(true)
                    Text(label).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.textBody)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 7)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("\(label): \(secure ? "holding" : "watch")"))
                .accessibilityAddTraits(.isStaticText)
            }
        }
        WebProvenance([("FR-B-6", "AI release intelligence: risk scoring, anomaly detection via Mimir"),
                       ("FR-B-2", "Zero-downtime rolling upgrades, health-gated rollback"),
                       ("NFR-PQ-3", "ML-DSA / SLH-DSA artefact signing"),
                       ("FR-B-5", "Signed offline bundles for air-gapped sites")], titled: true)
        if !ctx.compact, let id = model.lastAudit {
            BifrostAuditLine(label: "LAST STEP · AUDIT", auditId: id, identifier: "mg.bifrost.release.audit")
        }
        if !model.steps.isEmpty {
            VStack(alignment: .leading, spacing: 4) {
                WebLabel("STEPS · THIS SESSION")
                ForEach(Array(model.steps.enumerated()), id: \.offset) { _, s in
                    Text("\(s.auditId ?? "—")  \(s.step)")
                        .font(MGWebFont.mono(10))
                        .foregroundStyle(MGWeb.textFaint)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("mg.bifrost.release.steps")
        }
    }
}

// MARK: - Artefacts

struct BifrostArtefactsScreen: View {
    let ctx: BifrostScreenContext
    @StateObject private var model: MidgardListModel<BifrostArtefact>
    @State private var inspecting = false
    @State private var filter = 0

    init(ctx: BifrostScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .artefacts(ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Artefacts"), asideIdentifier: ctx.detail("Artefacts"),
                       inspecting: $inspecting, top: ctx.header("Artefacts"), asideWidth: ctx.asideWidth("Artefacts")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("FR-B-4", "Reproducible builds + SBOM + PQ signature, verified before promotion"),
                           ("NFR-PQ-3", "ML-DSA / SLH-DSA artefact signing (NIST FIPS 204/205)")])
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 1, section: "ARTEFACTS", title: "Artefact registry", accent: ctx.accent) {
            WebStatBlock(value: "58", label: "ARTEFACTS")
            WebStatBlock(value: model.rows.isEmpty ? "—" : "100%", label: "SIGNED", accent: ctx.accent)
            WebStatBlock(value: "\(model.rows.filter { $0.item.state == "danger" }.count)", label: "BLOCKED")
            WebButton("Register artefact", symbol: WebIcon.symbol("upload"), variant: .secondary, size: .sm,
                      disabledReason: "Not available in this app: artefacts are registered in the enclosure",
                      accent: ctx.accent, identifier: "mg.bifrost.artefacts.register")
        }
        FlowRow(spacing: 10) {
            ForEach(Array(bifrostArtefactFilters.enumerated()), id: \.offset) { i, f in
                WebTag(f, active: filter == i, accent: ctx.accent, identifier: "mg.bifrost.artefacts.filter.\(f)") {
                    filter = i
                }
            }
        }
        if ctx.compact, let a = model.current {
            MidgardInspectButton(title: "Inspect \(a.name)", identifier: "mg.bifrost.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Artefact", text: { (r: MidgardRow<BifrostArtefact>) in r.item.name }) { r in
                BifrostIconCell(symbol: WebIcon.symbol("package"), text: r.item.name)
            },
            WebColumn("Version", width: 100, mono: true) { $0.item.version },
            WebColumn("Kind", width: 130) { $0.item.kind },
            WebColumn("Size", width: 90, align: .trailing, mono: true) { $0.item.size },
            WebColumn("Reproducible", width: 110, mono: true) { $0.item.reproducible },
            WebColumn("Crit. CVEs", width: 90, align: .trailing, mono: true) { bifrostCount($0.item.criticalCves) },
            // Wider than the web's 150: the core's "not checked (sample)" and "Not reproducible"
            // were cut at the table's edge.
            WebColumn("State", width: 200, text: { $0.item.stateLabel }) { r in bifrostPill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: model.rows.isEmpty ? nil : model.selected, minWidth: 820,
           asList: ctx.compact, accent: ctx.accent, identifier: "mg.bifrost.artefacts.table") { i in
            ctx.select(i, { model.selected = $0 }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "artefact")
        if model.state == .loading { WebMicro("VERIFYING ARTEFACTS…") }
    }

    @ViewBuilder private var aside: some View {
        if let a = model.current {
            let blocked = a.state == "danger"
            WebInspectorTitle(symbol: WebIcon.symbol("package"), title: "\(a.name):\(a.version)", accent: ctx.accent,
                              identifier: "mg.bifrost.inspector.title") {
                bifrostPill(a.state, a.stateLabel)
            }
            WebStatPair((a.reproducible, "REPRODUCIBLE", a.reproducible.contains("⚠")),
                        (bifrostCount(a.criticalCves), "CRITICAL CVES", false), accent: ctx.accent)
            WebTerminal("VERIFICATION", lines: blocked ? [
                "> bifrost verify \(a.name):\(a.version)",
                "build      NOT bit-identical (1/2)",
                "diff       embedded timestamp in layer 4",
                "# promotion BLOCKED · fail closed [FR-B-4]",
            ] : [
                "> bifrost verify \(a.name):\(a.version)",
                "build      reproducible ✓",
                "sbom       CycloneDX attached",
                "signature  ML-DSA verified · artefact-sign-01",
                "# promotion permitted",
            ], accent: ctx.accent)
            if blocked {
                BifrostNoteCard(heading: "BLOCKED · NOTHING WAS PROMOTED", colour: MGWeb.dangerText, border: MGWeb.danger,
                                text: "Rebuild determinism failed: a timestamp leaks into layer 4. Fix the build recipe; verification will clear automatically on the next registration.",
                                identifier: "mg.bifrost.artefacts.blocked") { EmptyView() }
            }
        } else if model.state != .loading {
            BifrostNothingSelected(withheld: model.withheld, noun: "artefact")
        }
    }
}

// MARK: - Channels

struct BifrostChannelsScreen: View {
    let ctx: BifrostScreenContext
    @StateObject private var model: MidgardListModel<BifrostChannel>
    @State private var inspecting = false

    init(ctx: BifrostScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .channels(ctx.app))
    }

    /// `HISTORY`: the web screen's fixed promotion history.
    static let history: [[String]] = [
        ["2.5.0", "beta → stable · staged", "ok", "in progress"],
        ["2.4.1", "beta → stable", "ok", "32d ago"],
        ["2.4.0", "beta → stable", "rolled-back", "61d ago"],
    ]

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Channels"), asideIdentifier: ctx.detail("Channels"),
                       inspecting: $inspecting, top: ctx.header("Channels"), asideWidth: ctx.asideWidth("Channels")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebButton("Open release console", symbol: WebIcon.symbol("rocket"), variant: .secondary, size: .sm,
                      disabledReason: "Not available in this app: the web's button has no action; Releases is in the navigation",
                      accent: ctx.accent, identifier: "mg.bifrost.channels.open")
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 2, section: "CHANNELS", title: "Release channels", accent: ctx.accent) {
            WebStatBlock(value: "\(model.rows.count)", label: "CHANNELS")
            WebStatBlock(value: bifrostCount(model.rows.reduce(0) { $0 + $1.item.subscribers }), label: "TOTAL SUBSCRIBERS")
            WebStatBlock(value: "7d", label: "STABLE SOAK", accent: ctx.accent)
            WebButton("New channel", symbol: WebIcon.symbol("plus"), size: .sm,
                      disabledReason: "Not available in this app: channels are declared in the enclosure",
                      accent: ctx.accent, identifier: "mg.bifrost.channels.new")
        }
        if ctx.compact, let c = model.current {
            MidgardInspectButton(title: "Inspect \(c.name)", identifier: "mg.bifrost.inspect") { inspecting = true }
        }
        FlowRow(spacing: 12) {
            ForEach(Array(model.rows.enumerated()), id: \.offset) { i, row in
                let c = row.item
                let selected = i == model.selected
                WebPanel(padding: 18, accentRule: selected, accent: ctx.accent, identifier: "mg.bifrost.channel.\(c.name)",
                         label: "\(c.name), \(c.cadence), \(c.stateLabel), current \(c.current), \(bifrostCount(c.subscribers)) subscribers",
                         action: { ctx.select(i, { model.selected = $0 }, $inspecting) }) {
                    VStack(alignment: .leading, spacing: 12) {
                        HStack(spacing: 10) {
                            Text(c.name).font(MGWebFont.mono(14, .headline).weight(.semibold))
                                .foregroundStyle(MGWeb.textHeading)
                            WebBadge(c.cadence.uppercased(), tone: i == 0 ? .accent : .neutral, accent: ctx.accent)
                            Spacer(minLength: 8)
                            // One line: in the 230 pt card "Healthy" and "Rolling" wrapped
                            // inside their pills beside the MONTHLY and ON MERGE badges.
                            bifrostPill(c.state, c.stateLabel).fixedSize()
                        }
                        HStack(alignment: .top, spacing: 18) {
                            WebStatBlock(value: c.current, label: "CURRENT")
                            WebStatBlock(value: bifrostCount(c.subscribers), label: "SUBS")
                        }
                        Text("NEXT · \(c.next.uppercased())").font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                    }
                    .frame(width: ctx.compact ? nil : 250, alignment: .leading)
                }
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(selected ? ctx.accent.fill : .clear))
            }
        }
        MidgardWithheld(count: model.withheld, noun: "channel")
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 3, "PROMOTION HISTORY", accent: ctx.accent, muted: true)
            WebTable([
                WebColumn("Release", width: 110, mono: true) { (r: [String]) in r[0] },
                WebColumn("Path", mono: true) { $0[1] },
                WebColumn("Verification", width: 120, text: { _ in "ML-DSA ✓" }) { _ in WebBadge("ML-DSA ✓", tone: .secure) },
                WebColumn("Outcome", width: 130, text: { $0[2] == "ok" ? "COMPLETED" : "ROLLED BACK" }) { r in
                    WebBadge(r[2] == "ok" ? "COMPLETED" : "ROLLED BACK", tone: r[2] == "ok" ? .secure : .danger)
                },
                WebColumn("When", width: 110, mono: true) { $0[3] },
            ], rows: Self.history, id: { $0[0] }, compact: true, minWidth: 620, asList: ctx.compact, accent: ctx.accent,
               identifier: "mg.bifrost.channels.history")
            if model.state == .loading { WebMicro("LOADING CHANNELS…") }
        }
    }

    @ViewBuilder private var aside: some View {
        if let c = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("git-branch"), title: c.name, accent: ctx.accent,
                              identifier: "mg.bifrost.inspector.title") {
                bifrostPill(c.state, c.stateLabel)
            }
            WebStatPair((c.current, "CURRENT", false), (bifrostCount(c.subscribers), "SUBSCRIBERS", false), accent: ctx.accent)
            BifrostRules(title: "PROMOTION POLICY", rows: [
                ("Verification", "reproducible + SBOM + ML-DSA"),
                ("Rings", "canary → staging → prod"),
                ("Gates", c.policy),
                ("Rollback", "automatic on health failure"),
            ])
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("SUBSCRIBER MIX").padding(.bottom, 8)
                ForEach([("Connected enclaves", c.name == "stable" ? 52 : max(0, Int(c.subscribers) - 1), "server"),
                         ("Air-gapped sites", c.name == "stable" ? 9 : 1, "download")], id: \.0) { label, n, icon in
                    HStack(spacing: 10) {
                        Image(systemName: WebIcon.symbol(icon)).font(MGWebFont.sans(12)).foregroundStyle(ctx.accent.text)
                            .accessibilityHidden(true)
                        Text(label).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                        Spacer(minLength: 8)
                        Text("\(n)").font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                    }
                    .padding(.vertical, 6)
                    .accessibilityElement(children: .combine)
                }
            }
            WebProvenance([("FR-B-3", "Declarative release channels and fleet/installation management")])
        } else if model.state != .loading {
            BifrostNothingSelected(withheld: model.withheld, noun: "channel")
        }
    }
}

// MARK: - Offline bundles

struct BifrostBundlesScreen: View {
    let ctx: BifrostScreenContext
    @StateObject private var model: MidgardListModel<BifrostBundle>
    @State private var inspecting = false

    init(ctx: BifrostScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .bundles(ctx.app))
    }

    /// `PIPELINE`: the six stages of a bundle.
    static let pipeline: [(icon: String, label: String)] = [
        ("package", "Cut & sign"), ("download", "Export media"), ("arrow-right", "Courier"),
        ("shield-check", "Verify offline"), ("refresh-cw", "Apply · rolling"), ("upload", "Receipt returns"),
    ]

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Bundles"), asideIdentifier: ctx.detail("Bundles"),
                       inspecting: $inspecting, top: ctx.header("Bundles"), asideWidth: ctx.asideWidth("Bundles")) {
            main
        } aside: {
            aside
        } asideFoot: {
            WebProvenance([("DD-7", "Sovereignty: on-prem, self-hosted, air-gapped by design"),
                           ("FR-B-5", "Signed offline bundles for air-gapped sites"),
                           ("AC-M-5", "Full air-gap acceptance criterion")])
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 4, section: "AIR-GAP", title: "Offline bundles", accent: ctx.accent) {
            WebStatBlock(value: "9", label: "AIR-GAPPED SITES")
            WebStatBlock(value: "100%", label: "RECEIPTS RETURNED", accent: ctx.accent)
            WebStatBlock(value: "\(model.rows.filter { $0.item.state == "gated" }.count)", label: "AWAITING COURIER")
            WebButton("Cut new bundle", symbol: WebIcon.symbol("package"), size: .sm,
                      disabledReason: "Not available in this app: bundles are cut and signed in the enclosure",
                      accent: ctx.accent, identifier: "mg.bifrost.bundles.cut")
        }
        pipeline
        if ctx.compact, let b = model.current {
            MidgardInspectButton(title: "Inspect \(b.id)", identifier: "mg.bifrost.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Bundle", width: 120, text: { (r: MidgardRow<BifrostBundle>) in r.item.id }) { r in
                Text(r.item.id).font(MGWebFont.mono(12, .caption)).foregroundStyle(ctx.accent.text).lineLimit(1)
            },
            WebColumn("Release", width: 90, mono: true) { $0.item.release },
            WebColumn("Target sites", width: 160, mono: true) { $0.item.sites },
            WebColumn("Size", width: 80, align: .trailing, mono: true) { $0.item.size },
            WebColumn("Cut", width: 110, mono: true) { $0.item.cut },
            WebColumn("Status") { $0.item.status },
            WebColumn("State", width: 110, text: { $0.item.stateLabel }) { r in bifrostPill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: model.rows.isEmpty ? nil : model.selected, minWidth: 820,
           asList: ctx.compact, accent: ctx.accent, identifier: "mg.bifrost.bundles.table") { i in
            ctx.select(i, { model.selected = $0 }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "bundle")
        if model.state == .loading { WebMicro("LOADING BUNDLES…") }
    }

    /// The bundle's six stages, the first two outlined in the accent while the selected
    /// bundle awaits its courier.
    private var pipeline: some View {
        let awaiting = model.current?.state == "gated"
        return FlowRow(spacing: 10) {
            ForEach(Array(Self.pipeline.enumerated()), id: \.offset) { i, stage in
                HStack(spacing: 10) {
                    HStack(spacing: 7) {
                        Image(systemName: WebIcon.symbol(stage.icon)).font(MGWebFont.sans(12))
                            .foregroundStyle(ctx.accent.text).accessibilityHidden(true)
                        Text(stage.label).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.grey100).lineLimit(1)
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 6).fill(MGWeb.surfaceCard))
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(i <= 1 && awaiting ? ctx.accent.fill : MGWeb.border))
                    if i < Self.pipeline.count - 1 {
                        Image(systemName: "chevron.right").font(MGWebFont.sans(11)).foregroundStyle(MGWeb.textFaint)
                            .accessibilityHidden(true)
                    }
                }
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Bundle pipeline: " + Self.pipeline.map(\.label).joined(separator: ", then ")))
        .accessibilityAddTraits(.isStaticText)
        .accessibilityIdentifier("mg.bifrost.bundles.pipeline")
    }

    @ViewBuilder private var aside: some View {
        if let b = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("download"), title: b.id, accent: ctx.accent,
                              identifier: "mg.bifrost.inspector.title") {
                bifrostPill(b.state, b.stateLabel)
            }
            WebStatPair((b.release, "RELEASE", false), (b.size, "SIZE", false), accent: ctx.accent)
            BifrostRules(title: "CONTENTS", rows: [
                ("Platform artefacts", "14 images · signed"),
                ("Model weights", "Mistral Small 3.2.1 · digest-pinned"),
                ("SBOMs", "CycloneDX · every artefact"),
                ("Rollback set", "previous version included"),
            ], keyWidth: 110, monoValues: false)
            WebTerminal("OFFLINE VERIFICATION · AT SITE", lines: [
                "> bifrost bundle verify \(b.id)",
                "manifest   ML-DSA verified",
                "artefacts  14/14 digests OK",
                "weights    sha3 match · sig OK",
                "# apply permitted · rollback staged",
            ], accent: ctx.accent)
        } else if model.state != .loading {
            BifrostNothingSelected(withheld: model.withheld, noun: "bundle")
        }
    }
}

import MidgardCore
import SwiftUI

// Midgard's PLATFORM screens, matched to the web console: Identity
// (`IdentityScreen.tsx`), Keys (`KeysScreen.tsx`), Compute (`ComputeScreen.tsx`),
// Observability (`ObservabilityScreen.tsx`), Resilience (`ResilienceScreen.tsx`) and
// SDK (`SdkScreen.tsx`). The gateway serves each from fixed example data, so each is
// labelled SAMPLE DATA, and its controls are disabled and say why. [AC-M-4] [DD-7]

/// `StatBlock` size md: the readout in Oxanium black at the web's h2.
struct WebStatBlockLarge: View {
    let value: String
    let label: String
    let accent: MGWebAccent?

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(value)
                .font(MGWebFont.display(34, .largeTitle).weight(.black))
                .mgTracking(-0.02, size: 34)
                .foregroundStyle(accent?.text ?? MGWeb.textHeading)
                .accessibilityLabel(Text("\(label.capitalized): \(value)"))
            Text(label)
                .font(MGWebFont.mono(11))
                .mgTracking(0.14, size: MGWebFont.monoSize(11))
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityHidden(true)
        }
    }
}

/// An icon in the accent beside a row's first column (the tables' first cells).
struct MidgardIconCell: View {
    let symbol: String
    let text: String

    var body: some View {
        HStack(spacing: 9) {
            Image(systemName: symbol).font(MGWebFont.sans(13)).foregroundStyle(MGWebAccent.midgard.text)
                .accessibilityHidden(true)
            Text(text).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.grey100).lineLimit(1)
        }
    }
}

/// The small posture and telemetry cards (`Card size="sm"`: a label, a mono line and a
/// muted line).
struct MidgardFactCard: View {
    let title: String
    let value: String
    let detail: String

    var body: some View {
        WebPanel {
            VStack(alignment: .leading, spacing: 0) {
                WebLabel(title)
                Text(value).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.grey100)
                    .padding(.top, 8).padding(.bottom, 4)
                    .fixedSize(horizontal: false, vertical: true)
                Text(detail).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .accessibilityElement(children: .combine)
    }
}

/// The selection rule every list screen shares: select the row; on the iPhone also
/// open the inspector.
extension MidgardScreenContext {
    func select(_ index: Int, _ set: (Int) -> Void, _ inspecting: Binding<Bool>) {
        set(index)
        if compact { inspecting.wrappedValue = true }
    }
}

// MARK: - Identity

struct IdentityScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: MidgardListModel<MidgardIdentity>
    @State private var inspecting = false

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .identities(ctx.app))
    }

    static func kindSymbol(_ kind: String) -> String {
        WebIcon.symbol(["user": "user", "service": "server", "agent": "bot"][kind] ?? "user")
    }

    static func pill(_ i: MidgardIdentity) -> WebStatusPill {
        i.status == "active" ? WebStatusPill(.online, "Active") : WebStatusPill(.warn, "Suspended")
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Identity"), asideIdentifier: ctx.detail("Identity"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Identity"))) {
            let rows = model.rows.map(\.item)
            WebPageHeader(number: 5, section: "IDENTITY & ACCESS", title: "Identities", sampleData: true) {
                WebStatBlock(value: "\(rows.count)", label: "SUBJECTS")
                WebStatBlock(value: "\(rows.filter { $0.status == "active" }.count)", label: "ACTIVE", accent: .midgard)
                WebStatBlock(value: "\(rows.filter { $0.kind == "agent" }.count)", label: "AGENTS")
                WebButton("New identity", symbol: "plus",
                          disabledReason: "Not available yet: identities are enrolled with mg-admin user add")
            }
            if ctx.compact, let u = model.current {
                MidgardInspectButton(title: "Inspect \(u.subject)") { inspecting = true }
            }
            WebTable([
                WebColumn("Identity", text: { (r: MidgardIdentity) in r.subject }) { r in
                    MidgardIconCell(symbol: Self.kindSymbol(r.kind), text: r.subject)
                },
                WebColumn("Kind", width: 90, mono: true) { $0.kind },
                WebColumn("Clearance", width: 170, text: { $0.clearance }) { r in WebMarkingChip(midgardChip(r.clearance)) },
                WebColumn("Roles", width: 190, text: { $0.roles.joined(separator: ", ") }) { r in
                    HStack(spacing: 5) { ForEach(r.roles, id: \.self) { WebBadge($0, tone: .info) } }
                },
                WebColumn("Groups", width: 130, mono: true) { $0.groups.joined(separator: " · ") },
                WebColumn("Last seen", width: 100, mono: true) { $0.lastSeen },
                WebColumn("Status", width: 130, text: { $0.status }) { r in Self.pill(r) },
            ], rows: rows, id: \.subject, selected: model.selected, minWidth: 840, asList: ctx.compact,
               identifier: "mg.midgard.identity.table") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
            MidgardWithheld(count: model.withheld, noun: "identity")
            if model.state == .loading { WebMicro("RESOLVING IDENTITIES…") }
        } aside: {
            if let u = model.current {
                WebInspectorTitle(symbol: Self.kindSymbol(u.kind), title: u.subject,
                                  identifier: "mg.midgard.inspector.title") { Self.pill(u) }
                WebStatPair((u.kind.uppercased(), "KIND", false), ("\(u.roles.count)", "ROLES", u.kind == "agent"))
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("CLEARANCE")
                    WebMarkingChip(midgardChip(u.clearance))
                }
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("GRANTS · ROLES")
                    FlowRow(spacing: 6) { ForEach(u.roles, id: \.self) { WebBadge($0, tone: .info) } }
                }
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("GROUPS")
                    FlowRow(spacing: 6) { ForEach(u.groups, id: \.self) { WebTag($0) } }
                }
                WebTerminal("TOKEN", lines: ["> midgard token inspect", "alg     ML-DSA-65", "subject " + u.subject,
                                             "# zero-trust: re-authorised per call"])
                WebProvenance([("NFR-PRV-1", "Every identity and access decision is provenance-tracked"),
                               ("NFR-SEC-2", "Role/marking/purpose enforced at query time")], titled: true)
            }
        } asideFoot: {
            if model.current != nil {
                HStack(spacing: 8) {
                    WebButton("Edit grants", variant: .secondary, size: .sm, fullWidth: true,
                              disabledReason: "Not available yet: grants are managed with mg-admin")
                    WebButton("Revoke session", variant: .outline, size: .sm, fullWidth: true,
                              disabledReason: "Not available yet: sessions end when their token expires")
                }
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }
}

// MARK: - Keys

struct KeysScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: MidgardListModel<MidgardKeyRecord>
    @State private var inspecting = false

    static let posture: [(String, String, String)] = [
        ("TRANSIT", "TLS 1.3 · X25519 + ML-KEM-768", "hybrid, fails closed"),
        ("AT REST", "AES-256-GCM · ML-KEM-1024 wrap", "per-dataset DEKs"),
        ("SIGNATURES", "ML-DSA-65/87 · SLH-DSA", "lineage, artefacts, firmware"),
        ("AGILITY", "negotiated · versioned · hot-swap", "no data migration · KAT-verified"),
    ]

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .keys(ctx.app))
    }

    static func pill(_ k: MidgardKeyRecord) -> WebStatusPill {
        switch k.status {
        case "rotating": return WebStatusPill(.live, "Rotating")
        case "retired": return WebStatusPill(.offline, "Retired")
        default: return WebStatusPill(.secure, "Active")
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Keys"), asideIdentifier: ctx.detail("Keys"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Keys"))) {
            let rows = model.rows.map(\.item)
            let wrapped = rows.isEmpty ? 0 : Int((100 * Double(rows.filter(\.wrapped).count) / Double(rows.count)).rounded())
            WebPageHeader(number: 7, section: "KEYS & CRYPTO", title: "Key hierarchy", sampleData: true) {
                WebStatBlock(value: "\(rows.count)", label: "KEYS")
                WebStatBlock(value: "\(wrapped)%", label: "WRAPPED", accent: .midgard)
                WebStatBlock(value: "\(rows.filter { $0.status == "rotating" }.count)", label: "ROTATING")
                WebButton("Rotate now", symbol: WebIcon.symbol("refresh-cw"),
                          disabledReason: "Not available yet: keys are rotated by the KMS on its schedule")
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 180), spacing: 12)], spacing: 12) {
                ForEach(Self.posture, id: \.0) { MidgardFactCard(title: $0.0, value: $0.1, detail: $0.2) }
            }
            if ctx.compact, let k = model.current {
                MidgardInspectButton(title: "Inspect \(k.id)") { inspecting = true }
            }
            WebTable([
                WebColumn("Key", text: { (r: MidgardKeyRecord) in r.id }) { r in MidgardIconCell(symbol: "key", text: r.id) },
                WebColumn("Purpose", width: 160) { $0.purpose },
                WebColumn("Algorithm", width: 130, mono: true) { $0.alg },
                WebColumn("Custody", width: 110, text: { $0.custody }) { r in
                    WebBadge(r.custody.uppercased(), tone: r.custody == "hsm" ? .secure : .neutral)
                },
                WebColumn("Wrapped", width: 110, text: { $0.wrapped ? "wrapped" : "anchor" }) { r in
                    WebBadge(r.wrapped ? "WRAPPED" : "ANCHOR", tone: r.wrapped ? .secure : .neutral)
                },
                WebColumn("Rotated", width: 110, mono: true) { $0.rotated },
                WebColumn("Status", width: 130, text: { $0.status }) { r in Self.pill(r) },
            ], rows: rows, id: \.id, selected: model.selected, minWidth: 820, asList: ctx.compact,
               identifier: "mg.midgard.keys.table") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
            MidgardWithheld(count: model.withheld, noun: "key")
            if model.state == .loading { WebMicro("ENUMERATING KEY RING…") }
        } aside: {
            if let k = model.current {
                WebInspectorTitle(symbol: "key", title: k.id, identifier: "mg.midgard.inspector.title") { Self.pill(k) }
                WebStatPair((k.alg, "ALGORITHM", false), (k.custody.uppercased(), "CUSTODY", k.custody == "hsm"))
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("ENVELOPE · WRAP CHAIN").padding(.bottom, 8)
                    Text("kek-root (ML-KEM-1024)").foregroundStyle(MGWeb.grey100).padding(.vertical, 5)
                    Text("└ \(k.id) (\(k.alg)) · \(k.wrapped ? "WRAPPED" : "hardware anchor")")
                        .foregroundStyle(MGWeb.textMuted).padding(.vertical, 5)
                }
                .font(MGWebFont.mono(12, .caption))
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("DETAIL")
                    WebKeyValues([("Purpose", k.purpose), ("Custody", k.custody),
                                  ("Wrapped", k.wrapped ? "yes · AES-256-GCM" : "no · root anchor"), ("Rotated", k.rotated)],
                                 keyWidth: 78, upperKeys: true)
                }
                WebProvenance([("NFR-PQ-2", "Data at rest: AES-256-GCM under ML-KEM-1024-wrapped keys"),
                               ("NFR-PQ-3", "Signatures: ML-DSA / SLH-DSA over lineage and artefacts"),
                               ("STD:NIST FIPS 203", "NIST FIPS 203")], titled: true)
            }
        } asideFoot: {
            if let k = model.current {
                WebTerminal("ROTATION DRILL", lines: ["> midgard kms rehearse --key " + k.id, "unwrap    OK · 1.2s",
                                                      "rewrap    OK · 0.9s", "# zero-downtime rotation verified"])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }
}

// MARK: - Compute

struct ComputeScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: MidgardListModel<MidgardComputeJob>
    @State private var inspecting = false

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .jobs(ctx.app))
    }

    static func pill(_ j: MidgardComputeJob) -> WebStatusPill {
        switch j.state {
        case "running": return WebStatusPill(.live, "Running")
        case "queued": return WebStatusPill(.offline, "Queued")
        case "done": return WebStatusPill(.secure, "Done")
        default: return WebStatusPill(.danger, "Failed")
        }
    }

    static func symbol(_ j: MidgardComputeJob) -> String {
        WebIcon.symbol(["running": "cpu", "queued": "clock", "done": "check"][j.state] ?? "x")
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Compute"), asideIdentifier: ctx.detail("Compute"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Compute"))) {
            let rows = model.rows.map(\.item)
            WebPageHeader(number: 10, section: "COMPUTE", title: "Jobs", sampleData: true) {
                WebStatBlock(value: "\(rows.filter { $0.state == "running" }.count)", label: "RUNNING", accent: .midgard)
                WebStatBlock(value: "\(rows.filter { $0.state == "queued" }.count)", label: "QUEUED")
                WebStatBlock(value: "\(rows.filter { $0.state == "failed" }.count)", label: "FAILED")
                WebButton("Schedule job", symbol: "plus",
                          disabledReason: "Not available yet: jobs are scheduled by the compute service, not from here")
            }
            if ctx.compact, let j = model.current {
                MidgardInspectButton(title: "Inspect \(j.id)") { inspecting = true }
            }
            WebTable([
                WebColumn("Job", text: { (r: MidgardComputeJob) in r.id }) { r in MidgardIconCell(symbol: Self.symbol(r), text: r.id) },
                WebColumn("Kind", width: 150) { $0.kind },
                WebColumn("Backend", width: 120, mono: true) { $0.backend },
                WebColumn("Progress", width: 160, text: { "\(Int($0.progress))%" }) { r in WebMeterBar(r.progress, width: 90) },
                WebColumn("Submitted", width: 100, mono: true) { $0.submitted },
                WebColumn("State", width: 120, text: { $0.state }) { r in Self.pill(r) },
            ], rows: rows, id: \.id, selected: model.selected, minWidth: 760, asList: ctx.compact,
               identifier: "mg.midgard.compute.table") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
            MidgardWithheld(count: model.withheld, noun: "job")
            if model.state == .loading { WebMicro("DISPATCHING SCHEDULER…") }
        } aside: {
            if let j = model.current {
                WebInspectorTitle(symbol: Self.symbol(j), title: j.id, identifier: "mg.midgard.inspector.title") { Self.pill(j) }
                WebStatPair(("\(Int(j.progress))%", "PROGRESS", j.state == "running"), (j.backend, "BACKEND", false))
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("PLACEMENT")
                    WebKeyValues([("Kind", j.kind), ("Backend", j.backend), ("Isolation", "enclave · no host net"),
                                  ("Egress", "none · enclosure only"), ("Submitted", j.submitted)], keyWidth: 84, upperKeys: true)
                }
            }
        } asideFoot: {
            if let j = model.current {
                WebTerminal("SCHEDULER", lines: ["> midgard compute describe " + j.id, "anti-affinity  zone-spread",
                                                 "restart        health-gated", "# self-healing: reroute on fault"])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }
}

// MARK: - Observability

struct ObservabilityScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: MidgardListModel<MidgardSloMetric>
    @State private var inspecting = false

    static let sinks: [(String, String, String)] = [
        ("LOGS", "structured · correlation-tagged", "in-enclosure aggregation"),
        ("METRICS", "control plane + inference", "SLO dashboards, burn alerts"),
        ("TRACES", "distributed · mesh-wide", "request + action paths"),
        ("BACKUP", "encrypted snapshots · nightly", "restore tested 6d ago"),
    ]

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .slos(ctx.app))
    }

    /// The leading number of a percentage readout, for the meter (`pct`).
    static func percent(_ value: String) -> Double? {
        guard value.contains("%") else { return nil }
        return Double(value.prefix { $0.isNumber || $0 == "." })
    }

    static func pill(_ m: MidgardSloMetric) -> WebStatusPill {
        m.healthy ? WebStatusPill(.healthy, "Met") : WebStatusPill(.warn, "Degraded")
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Observability"),
                       asideIdentifier: ctx.detail("Observability"), inspecting: $inspecting,
                       top: AnyView(ctx.header("Observability"))) {
            WebPageHeader(number: 12, section: "OBSERVABILITY", title: "SLOs & traces", sampleData: true) {
                WebStatBlock(value: "99.9%", label: "CONTROL-PLANE SLO", accent: .midgard)
                WebStatBlock(value: "\(model.rows.count)", label: "SLOS TRACKED")
                WebStatBlock(value: "0", label: "EXTERNAL SINKS")
            }
            if ctx.compact, let m = model.current {
                MidgardInspectButton(title: "Inspect \(m.label)") { inspecting = true }
            }
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 12)], spacing: 12) {
                ForEach(Array(model.rows.enumerated()), id: \.element.id) { index, row in
                    let m = row.item
                    WebPanel(padding: 18, accentRule: index == model.selected, identifier: "mg.midgard.slo.\(index)",
                             label: "\(m.label): \(m.value), target \(m.target), \(m.healthy ? "met" : "degraded")",
                             action: { ctx.select(index, { model.selected = $0 }, $inspecting) }) {
                        VStack(alignment: .leading, spacing: 12) {
                            HStack(alignment: .top, spacing: 8) {
                                WebLabel(m.label).frame(maxWidth: .infinity, alignment: .leading)
                                Self.pill(m)
                            }
                            WebStatBlockLarge(value: m.value, label: "TARGET \(m.target)", accent: m.healthy ? .midgard : nil)
                            if let p = Self.percent(m.value) {
                                WebMeterBar(p, width: 140) { v in "\(v.formatted(.number.precision(.fractionLength(0...2))))%" }
                            }
                        }
                    }
                }
            }
            MidgardWithheld(count: model.withheld, noun: "SLO")
            if model.state == .loading { WebMicro("SAMPLING SLOS…") }
            VStack(alignment: .leading, spacing: 8) {
                WebSectionLabel(number: 13, "TELEMETRY · IN-ENCLOSURE SINKS", accent: .midgard)
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 200), spacing: 12)], spacing: 12) {
                    ForEach(Self.sinks, id: \.0) { MidgardFactCard(title: $0.0, value: $0.1, detail: $0.2) }
                }
            }
        } aside: {
            if let s = model.current {
                WebInspectorTitle(symbol: WebIcon.symbol("activity"), title: s.label, mono: false,
                                  identifier: "mg.midgard.inspector.title") { Self.pill(s) }
                WebStatPair((s.value, "TARGET \(s.target)", s.healthy), (s.healthy ? "MET" : "BURN", "STATUS", false))
                WebTerminal("TRACE · CORRELATION-TAGGED", lines: [
                    "> midgard trace 8f2c-44e1", "gateway     2ms   identity  4ms", "policy      1ms   fabric   38ms",
                    "audit       9ms   # every hop in-enclosure",
                ])
            }
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("NFR-AVA-1", "99.9% control-plane availability target"),
                               ("NFR-PRV-1", "No external telemetry; in-enclosure sinks only")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }
}

// MARK: - Resilience

struct ResilienceScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: MidgardListModel<MidgardRolloutState>
    @State private var inspecting = false

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .rollouts(ctx.app))
    }

    static func pill(_ r: MidgardRolloutState) -> WebStatusPill {
        switch r.state {
        case "in-progress": return WebStatusPill(.live, "In progress")
        case "healthy": return WebStatusPill(.healthy, "Healthy")
        case "rolled-back": return WebStatusPill(.danger, "Rolled back")
        default: return WebStatusPill(.offline, "Pending")
        }
    }

    static func gate(_ r: MidgardRolloutState) -> WebBadge {
        switch r.healthGate {
        case "watch": return WebBadge("GATE WATCH", tone: .warn)
        case "fail": return WebBadge("GATE FAIL", tone: .danger)
        default: return WebBadge("GATE PASS", tone: .secure)
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Resilience"), asideIdentifier: ctx.detail("Resilience"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Resilience"))) {
            let rows = model.rows.map(\.item)
            WebPageHeader(number: 12, section: "RESILIENCE", title: "Rollouts & health gates", sampleData: true) {
                WebStatBlock(value: "\(rows.filter { $0.state == "healthy" }.count)", label: "ROLLOUTS HEALTHY", accent: .midgard)
                WebStatBlock(value: "\(rows.filter { $0.state == "in-progress" }.count)", label: "IN PROGRESS")
                WebStatBlock(value: "\(rows.filter { $0.healthGate == "watch" }.count)", label: "GATE WATCHES")
            }
            if ctx.compact, let r = model.current {
                MidgardInspectButton(title: "Inspect \(r.id)") { inspecting = true }
            }
            WebTable([
                WebColumn("Rollout", text: { (r: MidgardRolloutState) in r.id }) { r in
                    MidgardIconCell(symbol: WebIcon.symbol("git-branch"), text: r.id)
                },
                WebColumn("Ring", width: 110, text: { $0.ring }) { r in
                    WebBadge(r.ring.uppercased(), tone: r.ring == "canary" ? .info : .neutral)
                },
                WebColumn("Version", width: 120, mono: true) { $0.version },
                WebColumn("Progress", width: 190, text: { "\(Int($0.progress))%" }) { r in
                    WebMeterBar(r.progress, width: 90, warnAt: r.healthGate == "watch" ? 0 : nil)
                },
                WebColumn("State", width: 150, text: { $0.state }) { r in Self.pill(r) },
                WebColumn("Health gate", width: 140, text: { $0.healthGate }) { r in Self.gate(r) },
            ], rows: rows, id: \.id, selected: model.selected, minWidth: 820, asList: ctx.compact,
               identifier: "mg.midgard.resilience.table") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
            MidgardWithheld(count: model.withheld, noun: "rollout")
            if model.state == .loading { WebMicro("RECONCILING RINGS…") }
            VStack(alignment: .leading, spacing: 8) {
                WebSectionLabel(number: 13, "PRINCIPLE", accent: .midgard)
                WebPanel {
                    Text("Rollouts move ring by ring — canary before broad — and never without a passing health gate. A gate that trips triggers an automatic, zero-downtime rollback to the last-good version; the decision and its lineage are signed, so every advance and reversal is auditable after the fact.")
                        .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        } aside: {
            if let r = model.current {
                WebInspectorTitle(symbol: WebIcon.symbol("git-branch"), title: r.id, mono: false,
                                  identifier: "mg.midgard.inspector.title") { Self.pill(r) }
                WebStatPair((r.version, "TARGET VERSION", false),
                            ("\(Int(r.progress))%", "RING · \(r.ring.uppercased())", r.state == "healthy"))
                VStack(alignment: .leading, spacing: 10) {
                    WebLabel("HEALTH GATE")
                    Self.gate(r)
                    Text("Each ring advances only while its health gate holds. A failing gate halts the fleet and rolls the ring back to the last-good version — zero-downtime, no operator in the loop.")
                        .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                }
                WebTerminal("ROLLOUT · HEALTH-GATED", lines: [
                    "> midgard rollout status \(r.id)", "ring       \(r.ring) · \(Int(r.progress))%", "gate       \(r.healthGate)",
                    r.healthGate == "fail" ? "# gate FAIL → auto rollback to last-good" : "# advancing under gate · signed lineage",
                ])
            }
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("NFR-AVA-1", "Zero-downtime, health-gated progressive rollouts"),
                               ("DD-6", "Ring-based fleet reconciliation across the enclosure")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }
}

// MARK: - SDK

struct SdkScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: MidgardListModel<MidgardRpcSurface>
    @State private var inspecting = false
    @State private var lang = "ts"

    static let snippets: [String: [String]] = [
        "ts": ["import { fabric } from \"@midgard/sdk\";", "", "const risky = await fabric.objects(\"aircraft\")",
               "  .where(a => a.riskScore.gt(0.6))", "  .searchAround(\"maintenanceEvents\")",
               "  .fetch(); // policy applied at query time"],
        "py": ["from midgard import fabric", "", "risky = (fabric.objects(\"aircraft\")", "  .where(lambda a: a.risk_score > 0.6)",
               "  .search_around(\"maintenance_events\")", "  .fetch())  # policy applied at query time"],
        "cli": ["> midgard query --type aircraft \\", "    --where \"risk_score > 0.6\" \\",
                "    --search-around maintenance_events", "# 14 objects · purpose: maintenance · audited"],
    ]

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .surfaces(ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("SDK"), asideIdentifier: ctx.detail("SDK"),
                       inspecting: $inspecting, top: AnyView(ctx.header("SDK"))) {
            let surfaces = model.rows.map(\.item)
            WebPageHeader(number: 11, section: "SDK & APIS", title: "SDK & API surface", sampleData: true) {
                WebStatBlock(value: "\(surfaces.count)", label: "SERVICES")
                WebStatBlock(value: "\(surfaces.reduce(0) { $0 + $1.rpcs.count })", label: "RPCS")
                WebStatBlock(value: "TS · PY · RS · GO", label: "SDK TARGETS", accent: .midgard)
                WebButton("Download SDKs", symbol: WebIcon.symbol("download"),
                          disabledReason: "Not available yet: the SDKs ship in the repository (sdk/)")
            }
            if ctx.compact, let svc = model.current {
                MidgardInspectButton(title: "Inspect \(svc.service)") { inspecting = true }
            }
            ForEach(Array(surfaces.enumerated()), id: \.offset) { index, svc in
                VStack(alignment: .leading, spacing: 12) {
                    Button {
                        ctx.select(index, { model.selected = $0 }, $inspecting)
                    } label: {
                        HStack(spacing: 10) {
                            Image(systemName: WebIcon.symbol("code")).foregroundStyle(MGWebAccent.midgard.text)
                                .accessibilityHidden(true)
                            Text(svc.service).font(MGWebFont.mono(14, .callout)).foregroundStyle(MGWeb.textHeading)
                                .fixedSize(horizontal: false, vertical: true)
                            Spacer(minLength: 4)
                            WebStatusPill(.online, "\(svc.rpcs.count) RPCS")
                        }
                        .frame(minHeight: webTarget(28))
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityElement(children: .combine)
                    .accessibilityAddTraits(index == model.selected ? [.isSelected] : [])
                    .accessibilityIdentifier("mg.midgard.sdk.service.\(index)")
                    WebTable([
                        WebColumn("RPC", width: 180, text: { (r: MidgardRpcSurfaceRpcs) in r.name }) { r in
                            MidgardIconCell(symbol: WebIcon.symbol("zap"), text: r.name)
                        },
                        WebColumn("Request", width: 180, mono: true) { $0.request },
                        WebColumn("Response", width: 180, mono: true) { $0.response },
                        WebColumn("Note") { $0.note },
                    ], rows: svc.rpcs, id: \.name, compact: true, minWidth: 720, asList: ctx.compact,
                       identifier: "mg.midgard.sdk.rpcs.\(index)")
                }
                .padding(18)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
                .overlay(alignment: .top) {
                    if index == model.selected {
                        UnevenRoundedRectangle(topLeadingRadius: MGRadius.md, topTrailingRadius: MGRadius.md)
                            .fill(MGWebAccent.midgard.fill).frame(height: 2).accessibilityHidden(true)
                    }
                }
                .clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
            }
            MidgardWithheld(count: model.withheld, noun: "service")
            if model.state == .loading { WebMicro("RESOLVING CONTRACT SURFACE…") }
            VStack(alignment: .leading, spacing: 8) {
                WebSectionLabel(number: 12, "TYPED CLIENT · EXAMPLE", accent: .midgard)
                HStack(spacing: 8) {
                    ForEach([("ts", "TypeScript"), ("py", "Python"), ("cli", "CLI")], id: \.0) { key, label in
                        WebTag(label, active: lang == key, identifier: "mg.midgard.sdk.lang.\(key)") { lang = key }
                    }
                }
                .padding(.top, 2)
                WebTerminal("TYPED CLIENT · \(lang.uppercased())", lines: Self.snippets[lang] ?? [])
                WebMicro("CODEGEN · BUF GENERATE FROM CONTRACTS/PROTO/** · TRANSPORT GRPC OVER PQ-MTLS", tracking: 0.14)
            }
        } aside: {
            if let s = model.current {
                WebInspectorTitle(symbol: WebIcon.symbol("code"), title: s.service, mono: false,
                                  identifier: "mg.midgard.inspector.title") { WebStatusPill(.online, "Serving") }
                WebStatPair(("\(s.rpcs.count)", "RPCS", false), ("100%", "POLICY-CHECKED", true))
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("EVERY CALL")
                    WebCheckList(["PQ-mTLS · workload identity", "Role / marking / purpose evaluated",
                                  "Deny-by-default · audited", "Generated, typed clients"])
                }
            }
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("DD-6", "Ports & adapters: products and developers depend on Midgard contracts, not implementations"),
                               ("STD:NIST FIPS 203/204", "ML-KEM-1024 (FIPS 203) key wrap + ML-DSA (FIPS 204) signatures on the wire")],
                              titled: true)
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }
}

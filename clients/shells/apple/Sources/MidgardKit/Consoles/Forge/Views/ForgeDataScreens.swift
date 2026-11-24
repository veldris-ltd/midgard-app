import MidgardCore
import SwiftUI

// Forge's catalogue screens, matched to the web console: Connectors
// (`ConnectorsScreen.tsx`), Datasets (`DatasetsScreen.tsx`), Data Health
// (`DataHealthScreen.tsx`), Schedules (`SchedulesScreen.tsx`) and Object Projection
// (`ObjectProjectionScreen.tsx`). [FR-F-2] [FR-F-3] [FR-F-4] [FR-F-5] [FR-F-6]
// [NFR-SEC-2] [DD-7]

// MARK: - Connectors

struct ForgeConnectorsScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: MidgardListModel<ForgeConnector>
    @State private var selected = 0
    @State private var inspecting = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .connectors(ctx.app))
    }

    private var currentIndex: Int? { model.rows.isEmpty ? nil : min(selected, model.rows.count - 1) }
    private var current: ForgeConnector? { currentIndex.map { model.rows[$0].item } }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Connectors"), asideIdentifier: ctx.detail("Connectors"),
                       inspecting: $inspecting, top: ctx.header("Connectors"), asideWidth: ctx.asideWidth("Connectors")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if let c = current {
                WebTerminal("LAST PULL", lines: ["> forge connector test \(c.name)", "handshake  PQ-mTLS OK",
                                                 "rows       48,112 · schema OK", "# contract satisfied"], accent: ctx.accent)
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 1, section: "CONNECTORS", title: "Connector catalogue", accent: ctx.accent) {
            WebStatBlock(value: "9", label: "CONNECTORS")
            WebStatBlock(value: "16.5 GB", label: "DAILY VOLUME")
            WebStatBlock(value: "0", label: "OUTBOUND PATHS", accent: ctx.accent)
            WebButton("Add connector", symbol: WebIcon.symbol("plus"),
                      disabledReason: "Not available in this app: connectors are registered in the enclosure",
                      accent: ctx.accent, identifier: "mg.forge.connectors.add")
        }
        ForgeStaticTags(tags: ["all", "streaming", "batch", "needs attention"])
        if ctx.compact, let c = current {
            MidgardInspectButton(title: "Inspect \(c.name)", identifier: "mg.forge.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Connector", text: { (r: MidgardRow<ForgeConnector>) in r.item.name }) { r in
                ForgeIconCell(symbol: WebIcon.symbol(r.item.kind == "Stream" ? "radio-tower" : "database"), text: r.item.name)
            },
            WebColumn("Kind", width: 120) { $0.item.kind },
            WebColumn("Cadence", width: 130, mono: true) { $0.item.cadence },
            WebColumn("Datasets", width: 90, align: .trailing, mono: true) { "\(Int($0.item.datasetsFed))" },
            WebColumn("Volume", width: 100, align: .trailing, mono: true) { $0.item.volume },
            WebColumn("State", width: 150, text: { $0.item.stateLabel }) { r in forgePill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: currentIndex, minWidth: 760, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.forge.connectors.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "connector")
        if model.state == .loading { WebMicro("PROBING CONNECTORS…") }
    }

    @ViewBuilder private var aside: some View {
        if let c = current {
            WebInspectorTitle(symbol: WebIcon.symbol("database"), title: c.name, accent: ctx.accent,
                              identifier: "mg.forge.inspector.title") {
                forgePill(c.state, c.stateLabel)
            }
            WebStatPair(("\(Int(c.datasetsFed))", "DATASETS FED", false), (c.volume, "VOLUME", false), accent: ctx.accent)
            ForgeRules(title: "TRUST BOUNDARY", rows: [("Direction", "inbound pull only"), ("Transport", c.transport),
                                                      ("Egress", c.egress), ("Schema", c.schema)])
            if let action = c.action {
                ForgeNoteCard(heading: "ACTION REQUIRED", headingColour: MGWeb.warnText, border: MGWeb.warn, text: action,
                              identifier: "mg.forge.connectors.action") {
                    WebButton("Rotate certificate", variant: .outline, size: .sm,
                              disabledReason: "Not available in this app: certificates are rotated in the enclosure",
                              accent: ctx.accent, identifier: "mg.forge.connectors.rotate")
                        .padding(.top, 4)
                }
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every connector is above your clearance." : "No connector is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Datasets

struct ForgeDatasetsScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: MidgardListModel<ForgeDataset>
    @State private var selected = 0
    @State private var tab = 0
    @State private var inspecting = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .datasets(ctx.app))
    }

    private var currentIndex: Int? { model.rows.isEmpty ? nil : min(selected, model.rows.count - 1) }
    private var current: MidgardRow<ForgeDataset>? { currentIndex.map { model.rows[$0] } }

    private func diffColour(_ tone: String) -> Color {
        switch tone {
        case "add": return MGWeb.secureText
        case "remove": return MGWeb.dangerText
        default: return MGWeb.warnText
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Datasets"), asideIdentifier: ctx.detail("Datasets"),
                       inspecting: $inspecting, top: ctx.header("Datasets"), asideWidth: ctx.asideWidth("Datasets")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if current != nil {
                HStack(spacing: 8) {
                    WebButton("Time travel", variant: .secondary, size: .sm, fullWidth: true,
                              disabledReason: "Not available in this app: earlier versions are read in the enclosure",
                              accent: ctx.accent, identifier: "mg.forge.datasets.time-travel")
                    WebButton("Open in builder", trailingSymbol: WebIcon.symbol("arrow-right"), size: .sm, fullWidth: true,
                              disabledReason: "Not available in this app: open Pipelines from the navigation",
                              accent: ctx.accent, identifier: "mg.forge.datasets.open-builder")
                }
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 2, section: "DATASETS", title: "Dataset browser", accent: ctx.accent) {
            WebStatBlock(value: "412", label: "DATASETS")
            WebStatBlock(value: "100%", label: "SEALED AT REST", accent: ctx.accent)
            WebStatBlock(value: "31", label: "OPEN BRANCHES")
            WebButton("New branch", symbol: WebIcon.symbol("git-branch"), variant: .secondary,
                      disabledReason: "Not available in this app: branches are cut in the enclosure",
                      accent: ctx.accent, identifier: "mg.forge.datasets.new-branch")
        }
        WebTabs([("All datasets", "412"), ("My branches", "3"), ("Pending merges", "1")], selection: $tab,
                accent: ctx.accent, identifier: "mg.forge.datasets.tabs")
        if ctx.compact, let d = current {
            MidgardInspectButton(title: "Inspect \(d.item.name)", identifier: "mg.forge.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Dataset", text: { (r: MidgardRow<ForgeDataset>) in r.item.name }) { r in
                ForgeIconCell(symbol: WebIcon.symbol("table"), text: r.item.name)
            },
            WebColumn("Version", width: 80, mono: true) { $0.item.version },
            WebColumn("Branch", width: 130, mono: true) { $0.item.branch },
            WebColumn("Rows", width: 90, align: .trailing, mono: true) { $0.item.rows },
            WebColumn("Columns", width: 90, align: .trailing, mono: true) { "\(Int($0.item.columns))" },
            WebColumn("Marking", width: 150, text: { $0.marking.label }) { r in WebMarkingChip(r.marking) },
            WebColumn("State", width: 150, text: { $0.item.stateLabel }) { r in forgePill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: currentIndex, minWidth: 820, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.forge.datasets.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "dataset")
        if model.state == .loading { WebMicro("LISTING DATASETS…") }
        if let d = current?.item {
            VStack(alignment: .leading, spacing: 8) {
                WebSectionLabel(number: 3, "LINEAGE · \(d.name)", accent: ctx.accent, muted: true)
                ForgeHopChain(d.lineage.enumerated().map { i, hop in
                    ForgeHopChain.Hop(symbol: WebIcon.symbol(hop.icon), label: hop.label, highlighted: i == 2)
                }, identifier: "mg.forge.datasets.lineage")
            }
        }
    }

    @ViewBuilder private var aside: some View {
        if let r = current {
            let d = r.item
            WebInspectorTitle(symbol: WebIcon.symbol("table"), title: d.name, accent: ctx.accent,
                              identifier: "mg.forge.inspector.title") {
                WebMarkingChip(r.marking)
            }
            WebStatPair((d.version, "BRANCH · \(d.branch.uppercased())", false), (d.rows, "ROWS", false), accent: ctx.accent)
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("VERSION DIFF · \(d.version) ← \(d.previousVersion)").padding(.bottom, 8)
                ForEach(Array(d.diff.enumerated()), id: \.offset) { _, line in
                    Text(line.text)
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(diffColour(line.tone))
                        .padding(.vertical, 5)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                }
            }
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("mg.forge.datasets.diff")
            WebTerminal("SEAL", lines: ["> forge seal status \(d.name)", "dek     fabric-dek-ring-a",
                                        "wrap    ML-KEM-1024 · root-03", "addr    content-hash sha3:22f1…9e04",
                                        "sig     ML-DSA · lineage OK", "# sealed · rebuild reproducible"], accent: ctx.accent)
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every dataset is above your clearance." : "No dataset is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Data health

struct ForgeHealthScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: MidgardListModel<ForgeHealthCheck>
    @State private var selected = 0
    @State private var inspecting = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .healthChecks(ctx.app))
    }

    private var currentIndex: Int? { model.rows.isEmpty ? nil : min(selected, model.rows.count - 1) }
    private var current: ForgeHealthCheck? { currentIndex.map { model.rows[$0].item } }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Data Health"), asideIdentifier: ctx.detail("Data Health"),
                       inspecting: $inspecting, top: ctx.header("Data Health"), asideWidth: ctx.asideWidth("Data Health")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if current != nil {
                WebProvenance([("FR-F-6", "Expectations and health checks gate every build")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 4, section: "DATA HEALTH", title: "Expectations & drift", accent: ctx.accent) {
            WebStatBlock(value: "1,204", label: "EXPECTATIONS")
            WebStatBlock(value: "3", label: "FAILING", accent: ctx.accent)
            WebStatBlock(value: "2", label: "PROJECTIONS BLOCKED")
            WebButton("New expectation", symbol: WebIcon.symbol("plus"),
                      disabledReason: "Not available in this app: expectations are written with the transform, in the enclosure",
                      accent: ctx.accent, identifier: "mg.forge.health.new")
        }
        FlowRow(spacing: 12) {
            ForEach([("PASSING", "1,199", MGWeb.secure), ("DRIFTING", "3", MGWeb.warn), ("BLOCKING", "2", MGWeb.danger)],
                    id: \.0) { t, v, colour in
                HStack(spacing: 10) {
                    Circle().fill(colour).frame(width: 8, height: 8).accessibilityHidden(true)
                    Text(v).font(MGWebFont.display(22, .title2).weight(.heavy)).foregroundStyle(MGWeb.textHeading)
                    WebLabel(t)
                }
                .padding(14)
                .frame(minWidth: 160, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("\(t.capitalized): \(v)"))
            }
        }
        if ctx.compact, let c = current {
            MidgardInspectButton(title: "Inspect \(c.kind) on \(c.dataset)", identifier: "mg.forge.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Dataset", width: 170, mono: true) { (r: MidgardRow<ForgeHealthCheck>) in r.item.dataset },
            WebColumn("Expectation") { $0.item.check },
            WebColumn("Kind", width: 100, mono: true) { $0.item.kind },
            WebColumn("Pass rate", width: 90, align: .trailing, mono: true) { $0.item.passRate },
            WebColumn("Trend", width: 130, mono: true) { $0.item.trend },
            WebColumn("State", width: 120, text: { $0.item.stateLabel }) { r in forgePill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: currentIndex, minWidth: 780, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.forge.health.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "expectation")
        if model.state == .loading { WebMicro("EVALUATING EXPECTATIONS…") }
    }

    @ViewBuilder private var aside: some View {
        if let c = current {
            WebInspectorTitle(symbol: WebIcon.symbol("activity"), title: c.kind, accent: ctx.accent,
                              identifier: "mg.forge.inspector.title") {
                forgePill(c.state, c.stateLabel)
            }
            WebPanel {
                VStack(alignment: .leading, spacing: 6) {
                    Text(c.check).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.grey100).lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                    Text("ON \(c.dataset.uppercased())").font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .accessibilityElement(children: .combine)
            }
            WebPanel {
                VStack(alignment: .leading, spacing: 10) {
                    HStack(alignment: .top, spacing: 12) {
                        WebStatBlock(value: c.passRate, label: "PASS RATE", accent: c.state == "warn" ? ctx.accent : nil)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        WebStatBlock(value: c.projectionGate == "BLOCKS" ? "BLOCKS" : "CLEAR", label: "PROJECTION GATE")
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    ForgePassMeter(c.passValue)
                        .accessibilityIdentifier("mg.forge.health.meter")
                }
            }
            if let diagnosis = c.diagnosis {
                ForgeNoteCard(heading: "MIMIR DIAGNOSIS · LOCAL", headingColour: ctx.accent.text, border: ctx.accent.fill,
                              text: diagnosis, identifier: "mg.forge.health.diagnosis") {
                    HStack(spacing: 8) {
                        WebButton("Preview quarantine", variant: .outline, size: .sm, fullWidth: true,
                                  disabledReason: "Not available in this app: a quarantine rule is previewed in the enclosure",
                                  accent: ctx.accent, identifier: "mg.forge.health.quarantine")
                        WebButton("Dismiss", variant: .ghost, size: .sm,
                                  disabledReason: "Not available in this app: the diagnosis is the enclosure's",
                                  accent: ctx.accent, identifier: "mg.forge.health.dismiss")
                    }
                    .padding(.top, 4)
                }
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every expectation is above your clearance." : "No expectation is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Schedules

struct ForgeSchedulesScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: MidgardListModel<ForgeSchedule>
    @State private var selected = 0
    @State private var inspecting = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .schedules(ctx.app))
    }

    private var currentIndex: Int? { model.rows.isEmpty ? nil : min(selected, model.rows.count - 1) }
    private var current: ForgeSchedule? { currentIndex.map { model.rows[$0].item } }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Schedules"), asideIdentifier: ctx.detail("Schedules"),
                       inspecting: $inspecting, top: ctx.header("Schedules"), asideWidth: ctx.asideWidth("Schedules")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if current != nil {
                WebProvenance([("FR-F-5", "Batch and streaming pipelines with schedule/trigger management")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 6, section: "SCHEDULES", title: "Schedules & triggers", accent: ctx.accent) {
            WebStatBlock(value: "38", label: "SCHEDULED")
            WebStatBlock(value: "4", label: "STREAMING", accent: ctx.accent)
            WebStatBlock(value: "1", label: "RETRYING")
            WebButton("New schedule", symbol: WebIcon.symbol("plus"),
                      disabledReason: "Not available in this app: schedules are set in the enclosure",
                      accent: ctx.accent, identifier: "mg.forge.schedules.new")
        }
        ForgeStaticTags(tags: ["all", "batch", "streaming", "event-driven", "failing"])
        if ctx.compact, let s = current {
            MidgardInspectButton(title: "Inspect \(s.pipeline)", identifier: "mg.forge.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Pipeline", text: { (r: MidgardRow<ForgeSchedule>) in r.item.pipeline }) { r in
                ForgeIconCell(symbol: WebIcon.symbol(r.item.mode == "stream" ? "radio-tower" : "workflow"), text: r.item.pipeline)
            },
            WebColumn("Mode", width: 90, mono: true) { $0.item.mode },
            WebColumn("Trigger", width: 190, mono: true) { $0.item.trigger },
            WebColumn("Last run", width: 150, mono: true) { $0.item.last },
            WebColumn("Next", width: 90, mono: true) { $0.item.next },
            WebColumn("State", width: 150, text: { $0.item.stateLabel }) { r in forgePill(r.item.state, r.item.stateLabel) },
        ], rows: model.rows, id: \.id, selected: currentIndex, minWidth: 820, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.forge.schedules.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "schedule")
        if model.state == .loading { WebMicro("LOADING SCHEDULES…") }
        WebTerminal("RUN LEDGER · TODAY", lines: [
            "02:00  crew_roster_sync      FAIL  conn refused · retry queued",
            "13:55  flight_tracks_eu      OK    1m42s · v41 → v42 · sealed",
            "14:00  maintenance_rollup    OK    22s · triggered by v42",
            "# every run recorded with lineage",
        ], accent: ctx.accent)
    }

    @ViewBuilder private var aside: some View {
        if let s = current {
            WebInspectorTitle(symbol: WebIcon.symbol(s.mode == "stream" ? "radio-tower" : "clock"), title: s.pipeline,
                              accent: ctx.accent, identifier: "mg.forge.inspector.title") {
                forgePill(s.state, s.stateLabel)
            }
            WebStatPair((s.mode.uppercased(), "MODE", false),
                        (s.mode == "stream" ? "0.4s" : "5m", s.mode == "stream" ? "FUSION LAG" : "CADENCE", false),
                        accent: ctx.accent)
            ForgeRules(title: "TRIGGER RULES", rows: [("Trigger", s.trigger), ("Backfill", s.backfill),
                                                     ("Concurrency", s.concurrency), ("On failure", s.onFailure)],
                       keyWidth: 92)
            if let note = s.note {
                ForgeNoteCard(heading: "RETRY 2/3", headingColour: MGWeb.warnText, border: MGWeb.warn, text: note,
                              identifier: "mg.forge.schedules.note")
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every schedule is above your clearance." : "No schedule is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Object projection

struct ForgeProjectionScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: MidgardListModel<ForgeProjection>
    @State private var selected = 0
    @State private var inspecting = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .projections(ctx.app))
    }

    /// The web shows the first projection (`projections[0]`).
    private var projection: MidgardRow<ForgeProjection>? { model.rows.first }
    private var mappings: [ForgeProjectionMapping] { projection?.item.mappings ?? [] }
    private var currentIndex: Int? { mappings.isEmpty ? nil : min(selected, mappings.count - 1) }
    private var current: ForgeProjectionMapping? { currentIndex.map { mappings[$0] } }

    private func kindTone(_ kind: String) -> WebBadge.Tone {
        switch kind {
        case "link": return .accent
        case "governance": return .warn
        default: return .neutral
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Object Projection"),
                       asideIdentifier: ctx.detail("Object Projection"), inspecting: $inspecting,
                       top: ctx.header("Object Projection"), asideWidth: ctx.asideWidth("Object Projection")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if projection != nil {
                WebButton("Open type in Midgard", symbol: WebIcon.symbol("arrow-up-right"), variant: .secondary, size: .sm,
                          disabledReason: "Not available in this app: the Aircraft type is not in this release's Midgard sample",
                          accent: ctx.accent, identifier: "mg.forge.projection.open-midgard")
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        let p = projection?.item
        WebPageHeader(number: 5, section: "PROJECTION", title: "flight_tracks_eu → Aircraft", accent: ctx.accent) {
            WebStatBlock(value: p?.columnsMapped ?? "—", label: "COLUMNS MAPPED", accent: ctx.accent)
            WebStatBlock(value: p.map { "\(Int($0.linkTypes))" } ?? "—", label: "LINK TYPES")
            WebStatBlock(value: p.map { "\(Int($0.unmarkedRows))" } ?? "—", label: "UNMARKED ROWS")
            WebButton("Sync now", symbol: WebIcon.symbol("boxes"),
                      disabledReason: "Not available in this app: the projection is synced by the enclosure",
                      accent: ctx.accent, identifier: "mg.forge.projection.sync")
        }
        if let r = projection {
            VStack(alignment: .leading, spacing: 8) {
                ForgeHopChain([
                    .init(symbol: WebIcon.symbol("table"), label: r.item.source),
                    .init(symbol: "", label: "MAPPING · \(mappings.count) RULES", caption: true),
                    .init(symbol: WebIcon.symbol("boxes"), label: r.item.target, highlighted: true),
                ], identifier: "mg.forge.projection.flow")
                HStack {
                    Spacer(minLength: 0)
                    WebMarkingChip(r.marking)
                }
            }
        }
        if ctx.compact, let m = current {
            MidgardInspectButton(title: "Inspect \(m.column)", identifier: "mg.forge.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Source column(s)", mono: true) { (m: ForgeProjectionMapping) in m.column },
            WebColumn("Maps to", mono: true) { $0.to },
            WebColumn("Kind", width: 130, text: { $0.kind.uppercased() }) { m in
                WebBadge(m.kind.uppercased(), tone: kindTone(m.kind), accent: ctx.accent)
            },
            WebColumn("Transform", width: 220) { $0.note },
        ], rows: mappings, id: \.id, selected: currentIndex, minWidth: 720, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.forge.projection.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "projection")
        if model.state == .loading { WebMicro("LOADING MAPPING…") }
    }

    @ViewBuilder private var aside: some View {
        if let r = projection {
            let p = r.item
            WebInspectorTitle(symbol: WebIcon.symbol("boxes"), title: p.name, mono: false, accent: ctx.accent,
                              identifier: "mg.forge.inspector.title") {
                forgePill("healthy", "In sync")
            }
            WebStatPair((p.objects, "OBJECTS", false), (p.syncLag, "SYNC LAG", false), accent: ctx.accent)
            if let m = current {
                VStack(alignment: .leading, spacing: 8) {
                    WebLabel("SELECTED MAPPING")
                    WebTerminal(lines: ["\(m.column)  →  \(m.to)", "# \(m.note)"], accent: ctx.accent)
                        .accessibilityIdentifier("mg.forge.projection.selected")
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("GOVERNANCE CARRY-THROUGH")
                Text("Row markings survive projection: a CONFIDENTIAL source row yields a CONFIDENTIAL object. Policy applies at query time in the fabric, not at copy time.")
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.textMuted)
                    .lineSpacing(3)
                    .fixedSize(horizontal: false, vertical: true)
                WebProvenance([("FR-F-4", "Project curated datasets into Midgard object/link types via a mapping layer"),
                               ("NFR-SEC-2", "Markings enforced at query time")])
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "The projection is above your clearance." : "No projection is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

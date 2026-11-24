import MidgardCore
import SwiftUI

// Heimdall's PICTURE screens after the Operating Picture, matched to the web console:
// Entities (`EntitiesScreen.tsx`), Feeds (`FeedsScreen.tsx`) and Picture Sync
// (`PictureSyncScreen.tsx`). [FR-H-1] [FR-H-2] [FR-H-3] [FR-H-6] [NFR-SEC-2] [DD-7]

// MARK: - Entities

struct HeimdallEntitiesScreen: View {
    let ctx: HeimdallScreenContext
    @StateObject private var model: MidgardListModel<HeimdallEntity>
    @State private var filter = HeimdallEntityFilter.all
    @State private var selected = 0
    @State private var inspecting = false

    init(ctx: HeimdallScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .entities(ctx.app))
    }

    private var rows: [MidgardRow<HeimdallEntity>] { model.rows.filter { filter.admits($0.item) } }
    private var currentIndex: Int? { rows.isEmpty ? nil : min(selected, rows.count - 1) }
    private var current: MidgardRow<HeimdallEntity>? { currentIndex.map { rows[$0] } }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Entities"), asideIdentifier: ctx.detail("Entities"),
                       inspecting: $inspecting, top: ctx.header("Entities") { EmptyView() },
                       asideWidth: ctx.asideWidth("Entities")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if current != nil {
                WebProvenance([("FR-H-2", "Track and classify entities over time against the object fabric")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 2, section: "ENTITIES", title: "Entity browser", accent: ctx.accent) {
            WebStatBlock(value: "1,204", label: "TRACKED")
            WebStatBlock(value: "96.8%", label: "MEAN CONFIDENCE")
            WebStatBlock(value: "1", label: "ANOMALOUS", accent: ctx.accent)
        }
        FlowRow(spacing: 10) {
            ForEach(HeimdallEntityFilter.allCases, id: \.self) { f in
                WebTag(f.rawValue, active: filter == f, accent: ctx.accent,
                       identifier: "mg.heimdall.entities.filter.\(ConsoleCatalog.slug(f.rawValue))") {
                    filter = f
                    selected = 0
                }
            }
        }
        if ctx.compact, let e = current {
            MidgardInspectButton(title: "Inspect \(e.item.id)", identifier: "mg.heimdall.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Entity", width: 100, text: { (r: MidgardRow<HeimdallEntity>) in
                r.item.id + (r.item.track == "anomalous" ? " ⚠\u{FE0E}" : "")
            }) { r in
                HeimdallIdCell(text: r.item.id + (r.item.track == "anomalous" ? " ⚠\u{FE0E}" : ""), warn: r.item.track == "anomalous")
            },
            WebColumn("Class", width: 120, mono: true) { $0.item.cls },
            WebColumn("Confidence", width: 140, text: { "\(Int($0.item.conf))%" }) { r in
                HeimdallConfBar(value: r.item.conf)
            },
            WebColumn("Feeds", width: 60, align: .trailing, mono: true) { "\(Int($0.item.feeds))" },
            WebColumn("First seen", width: 90, mono: true) { $0.item.firstSeen },
            // Fitted: "CONFIDENTIAL // OPS-N" was cut at the column's edge; in 150 pt the
            // chip drops its compartment (the inspector and VoiceOver say it whole).
            WebColumn("Marking", width: 150, fitted: true, text: { $0.marking.label }) { r in WebMarkingChip(r.marking) },
            WebColumn("State", width: 120, text: { $0.item.statusLabel }) { r in heimdallPill(r.item.status, r.item.statusLabel) },
        ], rows: rows, id: \.id, selected: currentIndex, minWidth: 780, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.heimdall.entities.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        if rows.isEmpty, model.state != .loading { WebMicro("NO ENTITY MATCHES THIS FILTER") }
        MidgardWithheld(count: model.withheld, noun: "entity")
        if model.state == .loading { WebMicro("RESOLVING ENTITIES…") }
    }

    @ViewBuilder private var aside: some View {
        if let r = current {
            let e = r.item
            let anomalous = e.track == "anomalous"
            WebInspectorTitle(symbol: "scope", title: e.id, accent: ctx.accent, identifier: "mg.heimdall.inspector.title") {
                WebMarkingChip(r.marking)
            }
            WebStatPair(("\(Int(e.conf))%", "CLASSIFICATION CONF.", e.conf < 70), ("\(Int(e.feeds))", "CORROBORATING FEEDS", false),
                        accent: ctx.accent)
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("CLASSIFIER")
                VStack(alignment: .leading, spacing: 2) {
                    Text("rules + mimir · provenance logged").font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                    Text("rules-only when Mimir is absent (degraded mode)").font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                }
                .accessibilityElement(children: .combine)
                .padding(.bottom, 4)
                HeimdallHistory(title: "TRACK HISTORY", rows: [
                    (e.firstSeen, "first observed · " + (e.feeds > 2 ? "NORTH-2 + ADS-B" : "NORTH-2")),
                    ("13:58", anomalous ? "transponder ceased · reclassified UNKNOWN" : "course steady"),
                    ("14:31", anomalous ? "descent through FL180 · flagged" : "position nominal"),
                ])
            }
            if anomalous {
                HeimdallNoteCard(heading: "OPTIONS AVAILABLE", headingColour: MGWeb.warnText, border: MGWeb.warn,
                                 text: "Mimir has 3 ranked options for improving classification. Review on the operating picture.",
                                 identifier: "mg.heimdall.entities.options") {
                    WebButton("Open picture", trailingSymbol: WebIcon.symbol("arrow-right"), size: .sm, accent: ctx.accent,
                              identifier: "mg.heimdall.entities.open-picture") {
                        inspecting = false
                        ctx.open("Operating Picture")
                    }
                    .padding(.top, 4)
                }
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every entity is above your clearance." : "No entity is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Feeds

struct HeimdallFeedsScreen: View {
    let ctx: HeimdallScreenContext
    @StateObject private var model: MidgardListModel<HeimdallFeed>
    /// The web screen opens on the quarantined bridge (`useState(5)`).
    @State private var selected = 5
    @State private var inspecting = false

    init(ctx: HeimdallScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .feeds(ctx.app))
    }

    private var rows: [MidgardRow<HeimdallFeed>] { model.rows }
    private var currentIndex: Int? { rows.isEmpty ? nil : min(selected, rows.count - 1) }
    private var current: MidgardRow<HeimdallFeed>? { currentIndex.map { rows[$0] } }
    /// The user's own feeds (Your Data), listed with the sample's.
    private var mine: Int { rows.filter { $0.id.hasPrefix("user:") }.count }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Feeds"), asideIdentifier: ctx.detail("Feeds"),
                       inspecting: $inspecting, top: ctx.header("Feeds") { HeimdallToolbarItem(screen: "Feeds", toolbar: ctx.bar) },
                       asideWidth: ctx.asideWidth("Feeds")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if current != nil {
                WebProvenance([("FR-H-1", "Fuse multiple live feeds into one common picture"),
                               ("FR-H-3", "Authenticate feed sources with PQ signatures; reject unverified feeds")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
        // The user's feeds change as Your Data adds, polls or removes them.
        .onChange(of: ctx.app.userData.feeds.count) { Task { await model.load() } }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 1, section: "FEEDS", title: "Feed manager", accent: ctx.accent) {
            WebStatBlock(value: "\(12 + mine)", label: "REGISTERED")
            WebStatBlock(value: "11", label: "IN FUSION")
            WebStatBlock(value: "\(rows.filter(\.item.quarantined).count)", label: "QUARANTINED", accent: ctx.accent)
        }
        if mine > 0 {
            WebNotice("\(mine) FEED\(mine == 1 ? "" : "S") ADDED ON THIS DEVICE (YOUR DATA) · UNVERIFIED · QUARANTINED OUT OF FUSION, DRAWN AS THEIR OWN LAYERS",
                      tone: .warn, identifier: "mg.heimdall.feeds.mine")
        }
        if ctx.compact, let f = current {
            MidgardInspectButton(title: "Inspect \(f.item.name)", identifier: "mg.heimdall.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Feed", text: { (r: MidgardRow<HeimdallFeed>) in r.item.name }) { r in
                HeimdallIconCell(symbol: "antenna.radiowaves.left.and.right", text: r.item.name,
                                 colour: r.item.quarantined ? MGWeb.dangerText : ctx.accent.text)
            },
            WebColumn("Kind", width: 140) { $0.item.kind },
            WebColumn("Rate", width: 80, mono: true) { $0.item.rate },
            WebColumn("Latency", width: 80, align: .trailing, mono: true) { $0.item.latency },
            WebColumn("Weight", width: 80, mono: true) { $0.item.weight },
            WebColumn("PQ auth", width: 130, text: { heimdallFeedAuth($0.item) }) { r in
                WebBadge(heimdallFeedAuth(r.item), tone: r.item.auth ? .secure : .danger)
            },
            WebColumn("State", width: 150, text: { $0.item.statusLabel }) { r in heimdallPill(r.item.status, r.item.statusLabel) },
        ], rows: rows, id: \.id, selected: currentIndex, minWidth: 760, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.heimdall.feeds.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "feed")
        if model.state == .loading { WebMicro("VERIFYING FEEDS…") }
    }

    @ViewBuilder private var aside: some View {
        if let r = current {
            let f = r.item
            let user = r.id.hasPrefix("user:")
            WebInspectorTitle(symbol: "antenna.radiowaves.left.and.right", title: f.name, accent: ctx.accent,
                              identifier: "mg.heimdall.inspector.title") {
                heimdallPill(f.status, f.statusLabel)
            }
            WebStatPair((f.latency, "LATENCY", false), (f.weight.uppercased(), "FUSION WEIGHT", false), accent: ctx.accent)
            if user {
                HeimdallNoteCard(heading: "UNVERIFIED · QUARANTINED OUT OF FUSION", headingColour: MGWeb.warnText,
                                 border: MGWeb.warn,
                                 text: "Added on this device (Your Data). It is not signed by a certified mesh identity, so nothing of it reaches fusion: the picture draws it as its own layer, marked UNVERIFIED FEED.",
                                 identifier: "mg.heimdall.feeds.unverified")
            } else if f.status == "danger" {
                HeimdallNoteCard(heading: "EXCLUDED FROM FUSION", headingColour: MGWeb.dangerText, border: MGWeb.danger,
                                 text: "PQ signature check failed on the bridge relay at 13:58. The feed is quarantined; the picture runs without it. Nothing unauthenticated reaches fusion.",
                                 identifier: "mg.heimdall.feeds.excluded") {
                    WebButton("Re-key bridge", variant: .outline, size: .sm,
                              disabledReason: "Not available in this app: a bridge is re-keyed in the enclosure", accent: ctx.accent)
                        .padding(.top, 4)
                }
            } else {
                WebTerminal("AUTHENTICATION", lines: [
                    "> heimdall feed verify \(f.name)", "transport  PQ-mTLS OK", "payload    \(f.scheme) per frame OK",
                    "# admitted to fusion",
                ], accent: ctx.accent)
            }
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("CONTRIBUTION")
                WebKeyValues([
                    ("Entities fed", user ? "0" : f.weight == "high" ? "412" : f.weight == "medium" ? "188" : "9"),
                    ("Unique coverage", user ? "—" : f.name == "SAT-IR-7" ? "31%" : "12%"),
                    ("Marking", r.marking.label),
                ], keyWidth: 110, upperKeys: true)
            }
        }
    }
}

// MARK: - Picture Sync

struct HeimdallSyncScreen: View {
    let ctx: HeimdallScreenContext
    @StateObject private var model: MidgardListModel<HeimdallSyncSite>
    /// The web screen opens on the filtered replica (`useState(2)`).
    @State private var selected = 2
    @State private var inspecting = false

    init(ctx: HeimdallScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: .syncSites(ctx.app))
    }

    private var rows: [MidgardRow<HeimdallSyncSite>] { model.rows }
    private var currentIndex: Int? { rows.isEmpty ? nil : min(selected, rows.count - 1) }
    private var current: MidgardRow<HeimdallSyncSite>? { currentIndex.map { rows[$0] } }

    static func symbol(_ role: String) -> String {
        role == "air-gap" ? "arrow.down.to.line" : role == "primary" ? "eye" : "dot.radiowaves.left.and.right"
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Picture Sync"), asideIdentifier: ctx.detail("Picture Sync"),
                       inspecting: $inspecting, top: ctx.header("Picture Sync") { HeimdallToolbarItem(screen: "Picture Sync", toolbar: ctx.bar) },
                       asideWidth: ctx.asideWidth("Picture Sync")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if current != nil {
                WebProvenance([("FR-H-6", "Synchronise the common picture across nodes/sites over PQ-mTLS"),
                               ("NFR-SEC-2", "Marking-based access enforced at the object layer")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 6, section: "SYNC", title: "Picture sync", accent: ctx.accent) {
            WebStatBlock(value: "4", label: "SITES")
            WebStatBlock(value: "0.8s", label: "WORST LIVE LAG", accent: ctx.accent)
            WebStatBlock(value: "216", label: "MARKING-FILTERED OBJECTS")
        }
        topology
        if ctx.compact, let s = current {
            MidgardInspectButton(title: "Inspect \(s.item.name)", identifier: "mg.heimdall.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Site", text: { (r: MidgardRow<HeimdallSyncSite>) in r.item.name }) { r in
                HeimdallIconCell(symbol: "dot.radiowaves.left.and.right", text: r.item.name)
            },
            WebColumn("Role", width: 90, mono: true) { $0.item.role },
            WebColumn("Lag", width: 80, mono: true) { $0.item.lag },
            WebColumn("Entities", width: 90, align: .trailing, mono: true) { $0.item.entities },
            WebColumn("Max marking", width: 130, mono: true) { $0.item.markings },
            WebColumn("State", width: 140, text: { $0.item.statusLabel }) { r in heimdallPill(r.item.status, r.item.statusLabel) },
        ], rows: rows, id: \.id, selected: currentIndex, minWidth: 720, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.heimdall.sync.table") { i in ctx.select(i, { selected = $0 }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "site")
        if model.state == .loading { WebMicro("RESOLVING SITES…") }
    }

    /// The sites as nodes joined by their links (PQ-mTLS, or the courier to the air-gap).
    private var topology: some View {
        FlowRow(spacing: 14) {
            ForEach(Array(rows.enumerated()), id: \.element.id) { i, row in
                let site = row.item
                let chosen = i == currentIndex
                HStack(spacing: 14) {
                    Button {
                        ctx.select(i, { selected = $0 }, $inspecting)
                    } label: {
                        VStack(spacing: 6) {
                            Image(systemName: Self.symbol(site.role))
                                .font(MGWebFont.sans(18))
                                .foregroundStyle(site.status == "offline" ? MGWeb.textFaint : ctx.accent.text)
                                .frame(width: 54, height: 54)
                                .background(RoundedRectangle(cornerRadius: 12).fill(chosen ? ctx.accent.tint : MGWeb.surfaceCard))
                                .overlay(RoundedRectangle(cornerRadius: 12).strokeBorder(chosen ? ctx.accent.fill : MGWeb.border))
                            Text(site.name.split(separator: " ").first.map(String.init) ?? site.name)
                                .font(MGWebFont.mono(10))
                                .foregroundStyle(chosen ? MGWeb.grey100 : MGWeb.textFaint)
                            Text(site.lag).font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint)
                        }
                        .frame(minWidth: 72, minHeight: 44)
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(Text("\(site.name), \(site.role), lag \(site.lag)"))
                    .accessibilityAddTraits(chosen ? [.isButton, .isSelected] : .isButton)
                    .accessibilityIdentifier("mg.heimdall.sync.node.\(ConsoleCatalog.slug(site.name))")
                    if i < rows.count - 1 {
                        let courier = rows[i + 1].item.status == "offline"
                        VStack(spacing: 3) {
                            Rectangle().fill(courier ? MGWeb.borderStrong : ctx.accent.fill.opacity(0.6))
                                .frame(width: 44, height: 2)
                            Text(courier ? "COURIER" : "PQ-mTLS").font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textFaint)
                        }
                        .accessibilityHidden(true)
                    }
                }
            }
        }
        .padding(20)
        .frame(maxWidth: .infinity)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Sync topology"))
    }

    @ViewBuilder private var aside: some View {
        if let r = current {
            let s = r.item
            WebInspectorTitle(symbol: WebIcon.symbol("network"), title: s.name, accent: ctx.accent,
                              identifier: "mg.heimdall.inspector.title") {
                heimdallPill(s.status, s.statusLabel)
            }
            WebStatPair((s.entities, "ENTITIES VISIBLE", false), (s.lag, "REPLICATION LAG", false), accent: ctx.accent)
            if s.markings == "RESTRICTED" && s.role == "replica" {
                HeimdallNoteCard(heading: "MARKING FILTER ACTIVE", headingColour: ctx.accent.text, border: ctx.accent.fill,
                                 text: "216 CONFIDENTIAL // OPS-N entities are withheld from this site: its operators hold RESTRICTED clearance. The filter is evaluated per object, per replication cycle.",
                                 identifier: "mg.heimdall.sync.filter")
            }
            WebTerminal("REPLICATION", lines: [
                "> heimdall sync status " + (s.name.split(separator: " ").first.map(String.init) ?? s.name).lowercased(),
                "transport  PQ-mTLS · X25519+ML-KEM-768",
                "mode       " + (s.role == "air-gap" ? "signed snapshot · courier" : "delta stream · CRDT merge"),
                "markings   filtered per site clearance",
                "# one picture, per-clearance views",
            ], accent: ctx.accent)
        }
    }
}

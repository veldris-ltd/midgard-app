import MidgardCore
import SwiftUI

// Midgard's FABRIC screens, matched to the web console: Object Fabric
// (`ObjectFabricScreen.tsx`), Data Search (`DataSearchScreen.tsx`) and Search Around
// (`SearchAroundScreen.tsx`). [DD-1] [NFR-SEC-2] [NFR-AI-1] [DD-7]

/// A record's signature badge: the web's `ML-DSA ✓` only for a signature the core
/// checked (the pinned demo key, or this device's audit key for its own actions);
/// otherwise UNSIGNED, since the sample's seeded rows carry no signature. [AC-C-13]
struct MidgardSignatureBadge: View {
    let status: SignatureStatus?

    var body: some View {
        let checked = status == .verified || status == .device
        WebBadge(AuditModel.signatureText(status), tone: checked ? .secure : .warn)
            .accessibilityLabel(Text(checked ? SignatureBadge(status ?? .device).accessibilityLabel : "Not signed"))
    }
}

/// The audit lineage table (Object Fabric's, and the Audit screen's columns less the id).
struct MidgardAuditTable: View {
    let rows: [MidgardRow<AuditRecord>]
    let compact: Bool
    let withId: Bool
    let selected: Int?
    let identifier: String
    let onSelect: ((Int) -> Void)?

    var body: some View {
        var columns: [WebColumn<MidgardRow<AuditRecord>>] = []
        if withId {
            columns.append(WebColumn("Audit id", width: 95, text: { "#\($0.item.id)" }) { r in
                Text("#\(r.item.id)").font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWebAccent.midgard.text)
            })
        }
        columns += [
            WebColumn("Actor", mono: true) { $0.item.actor },
            WebColumn("Operation") { $0.item.action },
            WebColumn("Resource", width: withId ? 170 : nil, mono: true) { $0.item.resource },
            WebColumn("Signature", width: 110, text: { AuditModel.signatureText($0.signature) }) { r in
                MidgardSignatureBadge(status: r.signature)
            },
            WebColumn("TS", width: 90, mono: true) { $0.item.ts },
        ]
        return WebTable(columns, rows: rows, id: \.id, selected: selected, compact: true, minWidth: withId ? 720 : 640,
                        asList: compact, identifier: identifier, onSelect: onSelect)
    }
}

// MARK: - Object Fabric

struct ObjectFabricScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: ObjectFabricModel
    @State private var inspecting = false

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: ObjectFabricModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Object Fabric"),
                       asideIdentifier: ctx.detail("Object Fabric"), inspecting: $inspecting,
                       top: AnyView(ctx.header("Object Fabric"))) {
            main
        } aside: {
            aside
        } asideFoot: {
            if let o = model.selectedType {
                HStack(spacing: 8) {
                    WebButton("Edit schema", variant: .secondary, size: .sm, fullWidth: true,
                              disabledReason: "Not available yet: the schema comes from the ontology file at boot")
                    WebButton("Explore instances", trailingSymbol: "arrow.right", size: .sm, fullWidth: true,
                              identifier: "mg.midgard.fabric.explore") {
                        inspecting = false
                        ctx.open("Search Around", .exploreType(o.apiName))
                    }
                }
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        let loaded = model.state != .loading
        WebPageHeader(number: 2, section: "OBJECT FABRIC", title: "Object types") {
            WebStatBlock(value: loaded ? "\(model.types.count)" : "—", label: "OBJECT TYPES")
            WebStatBlock(value: loaded ? model.instanceTotal.formatted(.number.locale(Locale(identifier: "en_GB"))) : "—",
                         label: "INSTANCES")
            WebStatBlock(value: model.signedShare, label: "LINEAGE SIGNED", accent: .midgard)
            WebButton("New object type", symbol: "plus",
                      disabledReason: "Not available yet: object types come from the ontology file at boot")
        }
        WebTabs([("All types", loaded ? "\(model.types.count)" : nil), ("Interfaces", nil), ("Shared properties", nil)],
                selection: $model.tab, identifier: "mg.midgard.fabric.tab")
        if let unserved = ObjectFabricModel.unservedTabs[model.tab] {
            Text(unserved)
                .font(MGWebFont.sans(14))
                .foregroundStyle(MGWeb.textMuted)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16)
                .padding(.vertical, 18)
                .frame(maxWidth: .infinity, alignment: .leading)
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border, style: StrokeStyle(lineWidth: 1, dash: [4, 3])))
                .accessibilityIdentifier("mg.midgard.fabric.unserved")
        } else {
            if ctx.compact, let o = model.selectedType {
                MidgardInspectButton(title: "Inspect \(o.name)") { inspecting = true }
            }
            WebTable([
                WebColumn("Object type", text: { (r: MidgardRow<MidgardObjectType>) in r.item.name }) { r in
                    MidgardIconCell(symbol: WebIcon.symbol("boxes"), text: r.item.name)
                },
                WebColumn("API name", width: 130, mono: true) { $0.item.apiName },
                WebColumn("Props", width: 75, align: .trailing, mono: true) { "\(Int($0.item.propertyCount))" },
                WebColumn("Instances", width: 105, align: .trailing, mono: true) { $0.item.instanceCount },
                WebColumn("Marking", width: 210, text: { $0.marking.label }) { r in
                    WebMarkingChip(midgardChip(r.item.marking, compartments: r.item.compartment.map { [$0] } ?? []))
                },
                WebColumn("Updated", width: 85, mono: true) { $0.item.updated },
            ], rows: model.types, id: \.id, selected: model.selected, minWidth: 720, asList: ctx.compact,
               identifier: "mg.midgard.fabric.types") { i in
                model.select(i)
                if ctx.compact { inspecting = true }
            }
            MidgardWithheld(count: model.typesWithheld, noun: "object type")
            if model.state == .loading { WebMicro("QUERYING FABRIC…") }
        }
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 3, "AUDIT LINEAGE", accent: .midgard)
            MidgardAuditTable(rows: model.audit, compact: ctx.compact, withId: false, selected: nil,
                              identifier: "mg.midgard.fabric.audit", onSelect: nil)
        }
    }

    @ViewBuilder private var aside: some View {
        if let o = model.selectedType {
            WebInspectorTitle(symbol: WebIcon.symbol("boxes"), title: o.name, mono: false,
                              identifier: "mg.midgard.inspector.title") {
                WebMarkingChip(midgardChip(o.marking, compartments: o.compartment.map { [$0] } ?? []))
            }
            WebStatPair(("\(Int(o.propertyCount))", "PROPERTIES", false), (o.instanceCount, "INSTANCES", false))
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("LINK TYPES").padding(.bottom, 8)
                ForEach(o.links, id: \.label) { l in
                    HStack(spacing: 8) {
                        Image(systemName: WebIcon.symbol("network")).font(MGWebFont.sans(12)).foregroundStyle(MGWeb.textFaint)
                            .accessibilityHidden(true)
                        Text(l.label).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                        Spacer(minLength: 0)
                    }
                    .padding(.vertical, 7)
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                }
            }
            actions(o)
            if let notice = model.notice {
                WebNotice(notice.text, tone: notice.tone == .secure ? .secure : notice.tone == .warn ? .warn : .danger,
                          identifier: "mg.midgard.fabric.notice")
            }
            approvals
            WebProvenance([("DD-1", "Object fabric at the centre"),
                           ("NFR-SEC-2", "Role/marking/purpose enforced at query time"),
                           ("NFR-PQ-2", "AES-256-GCM under ML-KEM-1024-wrapped keys"),
                           ("NFR-AI-1", "Consequential actions need a second person's approval")], titled: true)
        }
    }

    private func actions(_ o: MidgardObjectType) -> some View {
        VStack(alignment: .leading, spacing: 0) {
            WebLabel("ACTION TYPES").padding(.bottom, 8)
            WebSelect("Act on",
                      options: [("", model.instances.isEmpty ? "No objects you can act on" : "Choose an object…")]
                          + model.instances.map { ($0.id, Self.instanceLabel($0)) },
                      selection: Binding(get: { model.target }, set: { model.setTarget($0) }),
                      identifier: "mg.midgard.fabric.target", fullWidth: true)
                .padding(.bottom, 6)
            if let status = model.picked?.propertyValues.first(where: { $0.key == "status" })?.value {
                (Text("STATUS · ").foregroundStyle(MGWeb.textFaint) + Text(status).foregroundStyle(MGWeb.textBody))
                    .font(MGWebFont.mono(11))
                    .padding(.bottom, 6)
            }
            ForEach(o.actions, id: \.name) { a in
                let disabled = model.busy || model.target.isEmpty
                Button {
                    Task { await model.trigger(a) }
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: WebIcon.symbol("zap")).font(MGWebFont.sans(12)).foregroundStyle(MGWebAccent.midgard.text)
                            .accessibilityHidden(true)
                        Text(a.name).font(MGWebFont.sans(14)).foregroundStyle(disabled ? MGWeb.textMuted : MGWeb.textBody)
                        Spacer(minLength: 4)
                        if a.humanGated { WebBadge("HUMAN GATE", tone: .warn) }
                    }
                    .padding(.vertical, 7)
                    .frame(minHeight: webTarget(32))
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                    .contentShape(Rectangle())
                }
                .buttonStyle(WebPressStyle())
                .disabled(disabled)
                .help(model.target.isEmpty ? "Choose an object to act on" : a.name)
                .accessibilityLabel(Text(a.humanGated ? "\(a.name), human gate" : a.name))
                .accessibilityHint(Text(model.target.isEmpty ? "Choose an object to act on" : ""))
                .accessibilityIdentifier("mg.midgard.fabric.action.\(ConsoleCatalog.slug(a.name))")
            }
        }
    }

    @ViewBuilder private var approvals: some View {
        let list = model.typeApprovals
        if !list.isEmpty {
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("APPROVALS")
                ForEach(list, id: \.id) { a in
                    let status = midgardApprovalStatus(a)
                    if !model.isMine(a) && status == "pending" {
                        WebHumanGate(title: "\(a.actionType) · \(a.target)",
                                     detail: "\(a.id) · REQUESTED BY \(a.requester.uppercased()) · YOUR APPROVAL NEEDED",
                                     state: .pending, accent: .midgard, busy: model.busy,
                                     onAuthorise: { Task { await model.decide(a, approve: true) } },
                                     onReject: { Task { await model.decide(a, approve: false) } },
                                     idPrefix: "mg.midgard.approval.\(a.id)")
                    } else {
                        MidgardApprovalRow(approval: a, status: status, mine: model.isMine(a), busy: model.busy) {
                            Task { await model.run(a.actionType, on: a.target, token: a.id) }
                        }
                    }
                }
            }
        }
    }

    static func instanceLabel(_ i: MidgardObjectInstance) -> String {
        let name = i.propertyValues.first { $0.key == "name" }?.value ?? i.propertyValues.first { $0.key == "tail" }?.value
        return name.map { "\(i.id) · \($0)" } ?? i.id
    }
}

/// One approval in the inspector (`ApprovalRow`): the persona's own (waiting, or ready
/// to run), or someone else's that is already decided.
struct MidgardApprovalRow: View {
    let approval: MidgardApproval
    let status: String
    let mine: Bool
    let busy: Bool
    let onRun: () -> Void

    private var who: String {
        if mine {
            if status == "pending" { return "WAITING FOR A SECOND PERSON" }
            return approval.approver.map { "DECIDED BY \($0.uppercased())" } ?? ""
        }
        return "REQUESTED BY \(approval.requester.uppercased())"
    }

    private var tone: WebBadge.Tone {
        switch status {
        case "pending": return .warn
        case "approved": return .secure
        case "rejected": return .danger
        default: return .neutral
        }
    }

    var body: some View {
        HStack(spacing: 8) {
            VStack(alignment: .leading, spacing: 2) {
                Text("\(approval.actionType) · \(approval.target)")
                    .font(MGWebFont.mono(11)).foregroundStyle(MGWeb.textHeading)
                    .fixedSize(horizontal: false, vertical: true)
                Text(approval.id + (who.isEmpty ? "" : " · \(who)"))
                    .font(MGWebFont.mono(9)).mgTracking(0.1, size: MGWebFont.monoSize(9)).foregroundStyle(MGWeb.textFaint)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            if mine && status == "approved" {
                WebButton("Run", size: .sm, disabledReason: busy ? "An action is in progress" : nil,
                          identifier: "mg.midgard.approval.run.\(approval.id)", action: onRun)
            } else {
                WebBadge(status.uppercased(), tone: tone)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceInset))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.divider))
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.midgard.approval.\(approval.id)")
    }
}

// MARK: - Data Search

struct DataSearchScreen: View {
    let ctx: MidgardScreenContext
    let link: MidgardLink?
    @StateObject private var model: DataSearchModel
    @State private var inspecting = false
    @State private var everyType = ""

    init(ctx: MidgardScreenContext, link: MidgardLink?) {
        self.ctx = ctx
        self.link = link
        _model = StateObject(wrappedValue: DataSearchModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Data Search"), asideIdentifier: ctx.detail("Data Search"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Data Search"))) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.text.isEmpty, model.picked != nil {
                WebProvenance([("DD-1", "Typed query, search-around, and action APIs on the object fabric"),
                               ("NFR-SEC-2", "Role/marking/purpose enforced at query time")])
            }
        }
        .task(id: ctx.persona) {
            await model.load()
            if case let .text(q)? = link { await model.search(text: q) }
        }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 3, section: "QUERY ENGINE",
                      title: model.text.isEmpty ? "Data search" : "Data search · “\(model.text)”") {
            if model.text.isEmpty, model.spec != nil, model.error == nil, !model.searching {
                WebStatBlock(value: "\(model.total)", label: "MATCHES")
            }
        }
        if ctx.compact {
            // The header's search box, which the iPhone's narrow header has no room for.
            WebTextField(nil, placeholder: "Search all object data…", text: $everyType, symbol: "magnifyingglass",
                         identifier: "mg.midgard.data.every") {
                Task { await model.search(text: everyType) }
            }
        }
        if !model.text.isEmpty {
            textResults
        } else {
            form
            results
        }
    }

    @ViewBuilder private var textResults: some View {
        HStack {
            if model.searching { WebMicro("SEARCHING EVERY TYPE…") }
            Spacer()
            WebButton("Back to the query builder", variant: .ghost, size: .sm, identifier: "mg.midgard.data.clear") {
                everyType = ""
                model.clearText()
            }
        }
        if let hits = model.textHits {
            if hits.allSatisfy({ $0.total == 0 }) {
                Text("Nothing you can read contains “\(model.text)”.")
                    .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
                    .accessibilityIdentifier("mg.midgard.data.nothing")
            }
            ForEach(hits.filter { $0.total > 0 }, id: \.type.apiName) { h in
                VStack(alignment: .leading, spacing: 8) {
                    HStack {
                        WebLabel("\(h.type.name.uppercased()) · \(h.total)", colour: MGWeb.textMuted)
                        Spacer()
                        WebButton(h.total > h.results.count ? "See all \(h.total)" : "Refine", variant: .ghost, size: .sm,
                                  identifier: "mg.midgard.data.refine.\(h.type.apiName)") {
                            Task { await model.refine(h) }
                        }
                    }
                    WebTable([
                        WebColumn("Object", mono: true) { (r: MidgardFoundObject) in r.id },
                        WebColumn("Name") { $0.propertyValues["name"] ?? "—" },
                        WebColumn("Marking", width: 160, text: { $0.markings.first?.level ?? "" }) { r in
                            if let m = r.markings.first { WebMarkingChip(midgardChip(m.level)) }
                        },
                    ], rows: h.results, id: \.id, compact: true, minWidth: 520, asList: ctx.compact,
                       identifier: "mg.midgard.data.hits.\(h.type.apiName)") { i in
                        ctx.open("Search Around", .seed(h.results[i].id))
                    }
                }
            }
        }
    }

    private var form: some View {
        WebPanel {
            VStack(alignment: .leading, spacing: 12) {
                FlowRow(spacing: 12) {
                    WebSelect("Object type", options: model.types.map { ($0.apiName, $0.name) },
                              selection: Binding(get: { model.type?.apiName ?? "" }, set: { model.pickType($0) }),
                              identifier: "mg.midgard.data.type")
                    WebSelect("Order by", options: [("", "Object id")] + model.properties.map { ($0.name, $0.name) },
                              selection: $model.orderBy, identifier: "mg.midgard.data.order")
                    WebSelect("Direction", options: [("asc", "Smallest first"), ("desc", "Largest first")],
                              selection: Binding(get: { model.descending ? "desc" : "asc" }, set: { model.descending = $0 == "desc" }),
                              identifier: "mg.midgard.data.direction")
                }
                ForEach(Array(model.conditions.enumerated()), id: \.offset) { i, c in
                    FlowRow(spacing: 10) {
                        WebSelect(i == 0 ? "Where" : "And",
                                  options: [(DataSearchModel.anyText, "any text")]
                                      + model.properties.map { ($0.name, "\($0.name) · \($0.typeName)") },
                                  selection: Binding(get: { c.property }, set: { model.setCondition(i, property: $0) }),
                                  identifier: "mg.midgard.data.where.\(i)")
                        WebSelect("Is", options: model.ops(for: c).map { ($0, DataSearchModel.opLabel[$0] ?? $0) },
                                  selection: Binding(get: { c.op }, set: { model.setCondition(i, op: $0) }),
                                  identifier: "mg.midgard.data.op.\(i)")
                        WebTextField("Value", placeholder: "", text: Binding(get: { c.value }, set: { model.setCondition(i, value: $0) }),
                                     identifier: "mg.midgard.data.value.\(i)", width: 200) {
                            Task { await model.run() }
                        }
                        Button {
                            model.removeCondition(i)
                        } label: {
                            Image(systemName: "xmark").font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
                                .frame(width: webTarget(32), height: webTarget(32))
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(Text("Remove condition"))
                        .accessibilityIdentifier("mg.midgard.data.remove.\(i)")
                    }
                }
                HStack(spacing: 10) {
                    WebButton("Add condition", symbol: "plus", variant: .secondary, size: .sm,
                              disabledReason: model.type == nil ? "No object type to search" : nil,
                              identifier: "mg.midgard.data.add") { model.addCondition() }
                    Spacer(minLength: 0)
                    WebButton("Search", symbol: "magnifyingglass",
                              disabledReason: model.type == nil ? "No object type to search" : nil,
                              identifier: "mg.midgard.data.search") {
                        Task {
                            await model.run()
                            if ctx.compact, model.picked != nil { inspecting = false }
                        }
                    }
                }
            }
        }
    }

    @ViewBuilder private var results: some View {
        if model.searching { WebMicro("SEARCHING · POLICY AT QUERY TIME…") }
        if let error = model.error { WebNotice(error, tone: .danger, identifier: "mg.midgard.data.error") }
        if let note = model.offlineNote { WebNotice(note, tone: .warn, identifier: "mg.midgard.data.offline") }
        if model.answer != nil {
            FlowRow(spacing: 10) {
                Text(model.total == 0 ? "No matches you can read" : "\(model.from)–\(model.to) of \(model.total)")
                    .font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textMuted)
                    .accessibilityIdentifier("mg.midgard.data.count")
                ForEach(Array((model.spec?.conditions ?? []).enumerated()), id: \.offset) { _, c in
                    WebBadge(DataSearchModel.describe(c), tone: .info)
                }
            }
            HStack(spacing: 10) {
                Spacer(minLength: 0)
                WebButton("Previous", variant: .secondary, size: .sm,
                          disabledReason: (model.spec?.offset ?? 0) == 0 ? "This is the first page" : nil,
                          identifier: "mg.midgard.data.previous") { Task { await model.previous() } }
                WebButton("Next", variant: .secondary, size: .sm,
                          disabledReason: model.to >= model.total ? "This is the last page" : nil,
                          identifier: "mg.midgard.data.next") { Task { await model.next() } }
            }
            if !model.results.isEmpty {
                if ctx.compact, let p = model.picked {
                    MidgardInspectButton(title: "Inspect \(p.id)") { inspecting = true }
                }
                WebTable([WebColumn("Object", mono: true) { (r: MidgardFoundObject) in r.id }]
                         + model.columns.map { name in WebColumn(name, mono: true) { $0.propertyValues[name] ?? "—" } },
                         rows: model.results, id: \.id, selected: min(model.selected, model.results.count - 1), compact: true,
                         minWidth: max(560, 160 + CGFloat(model.columns.count) * 130), asList: ctx.compact,
                         identifier: "mg.midgard.data.results") { i in
                    model.selected = i
                    if ctx.compact { inspecting = true }
                }
            }
        }
    }

    @ViewBuilder private var aside: some View {
        if model.text.isEmpty, let p = model.picked {
            WebInspectorTitle(symbol: WebIcon.symbol("boxes"), title: p.id, identifier: "mg.midgard.inspector.title") {
                if let m = p.markings.first { WebMarkingChip(midgardChip(m.level)) }
            }
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("PROPERTIES")
                WebKeyValues(p.properties, keyWidth: 120)
            }
            WebButton("Open in Search-around", trailingSymbol: "arrow.right", size: .sm, fullWidth: true,
                      identifier: "mg.midgard.data.open") {
                inspecting = false
                ctx.open("Search Around", .seed(p.id))
            }
            WebTerminal("QUERY · AUDITED", lines: ["> fabric.search(\"\(model.spec?.objectType ?? "")\")"]
                + (model.spec?.conditions.map { "where     \(DataSearchModel.describe($0))" } ?? [])
                + ["matches   \(model.total) readable", "# decision audited · #\(model.auditId ?? "—")"])
        } else if !ctx.compact {
            // The web shows no inspector until a match is picked; the Mac keeps the
            // rail and says what will appear in it.
            WebMicro(model.text.isEmpty ? "RUN A SEARCH, THEN PICK A MATCH TO SEE ITS PROPERTIES."
                                        : "PICK A MATCH TO OPEN IT IN SEARCH-AROUND.", tracking: 0.08)
        }
    }
}

// MARK: - Search Around

struct SearchAroundScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: SearchAroundModel
    @State private var inspecting = false

    init(ctx: MidgardScreenContext, link: MidgardLink?) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: SearchAroundModel(app: ctx.app, link: link))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Search Around"),
                       asideIdentifier: ctx.detail("Search Around"), inspecting: $inspecting,
                       top: AnyView(ctx.header("Search Around"))) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.node != nil {
                WebProvenance([("DD-1", "Typed query, search-around, and action APIs on the object fabric"),
                               ("NFR-SEC-2", "Role/marking/purpose enforced at query time")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    @ViewBuilder private var main: some View {
        let run = model.run
        WebPageHeader(number: 13, section: "QUERY ENGINE",
                      title: "Search-around · \(run.seedId.isEmpty ? "every \(run.seedType)" : run.seedId)") {
            WebStatBlock(value: "\(model.nodes.count)", label: "OBJECTS")
            WebStatBlock(value: "\(model.edges.count) · \(run.hops) HOPS", label: "LINKS")
        }
        WebPanel {
            WebFormRow {
                WebSelect("Seed object type", options: model.types.map { ($0.apiName, $0.name) }, selection: $model.seedType,
                          identifier: "mg.midgard.search.type")
                WebTextField("Seed id", placeholder: "AC-2214", text: $model.seedId, identifier: "mg.midgard.search.seed",
                             width: 160) { Task { await model.runTraversal() } }
                WebSelect("Hops", options: (1...4).map { (String($0), $0 == 1 ? "1 hop" : "\($0) hops") },
                          selection: Binding(get: { String(model.hops) }, set: { model.hops = Int($0) ?? 1 }),
                          identifier: "mg.midgard.search.hops")
                WebSelect("Most objects", options: SearchAroundModel.limits.map { (String($0), String($0)) },
                          selection: Binding(get: { String(model.limit) }, set: { model.limit = Int($0) ?? 60 }),
                          identifier: "mg.midgard.search.limit")
            } action: {
                WebButton("Run traversal", symbol: "magnifyingglass", identifier: "mg.midgard.search.run") {
                    Task { await model.runTraversal() }
                }
            }
        }
        if model.running { WebMicro("EXPANDING LINKS · POLICY AT QUERY TIME…") }
        if let error = model.error { WebNotice(error, tone: .danger, identifier: "mg.midgard.search.error") }
        if let g = model.graph, g.withheld > 0 {
            WebMicro("\(g.withheld) OBJECT\(g.withheld == 1 ? "" : "S") WITHHELD · ABOVE YOUR CLEARANCE", tracking: 0.1)
                .accessibilityIdentifier("mg.midgard.withheld")
        }
        if ctx.compact, let n = model.node {
            MidgardInspectButton(title: "Inspect \(n.label)") { inspecting = true }
        }
        ForEach(model.hopLevels, id: \.self) { hop in
            let group = model.nodes.filter { $0.hop == hop }
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("\(hop == 0 ? "SEED · ROOT" : "HOP \(hop)") · \(group.count) OBJECT\(group.count == 1 ? "" : "S")")
                LazyVGrid(columns: [GridItem(.adaptive(minimum: 220), spacing: 10)], spacing: 10) {
                    ForEach(group, id: \.id) { n in
                        WebPanel(accentRule: n.id == model.node?.id, identifier: "mg.midgard.search.node.\(n.id)",
                                 label: "\(n.label), \(n.typeName), \(n.marking)",
                                 action: {
                                     model.selectedId = n.id
                                     if ctx.compact { inspecting = true }
                                 }) {
                            VStack(alignment: .leading, spacing: 8) {
                                HStack(spacing: 9) {
                                    Image(systemName: WebIcon.symbol("boxes")).font(MGWebFont.sans(13))
                                        .foregroundStyle(MGWebAccent.midgard.text).accessibilityHidden(true)
                                    Text(n.label).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.grey100)
                                        .lineLimit(1)
                                }
                                HStack(spacing: 8) {
                                    WebMarkingChip(midgardChip(n.marking))
                                    Text(n.typeName).font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textFaint).lineLimit(1)
                                }
                            }
                        }
                    }
                }
            }
        }
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 14, "LINK TRAVERSALS · \(model.edges.count)", accent: .midgard)
            WebTable([
                WebColumn("From", width: 200, mono: true) { (r: MidgardGraphAnswer.Edge) in r.from },
                WebColumn("Link", text: { $0.label }) { r in WebBadge(r.label, tone: .info) },
                WebColumn("To", width: 160, mono: true) { $0.to },
            ], rows: model.edges, id: { "\($0.from)-\($0.to)" }, compact: true, minWidth: 560, asList: ctx.compact,
               identifier: "mg.midgard.search.links")
        }
        if let g = model.graph {
            WebMicro("SEARCH-AROUND · \(run.hops) HOPS · POLICY AT QUERY TIME · DECISION AUDITED #\(model.auditId ?? "—")",
                     tracking: 0.12)
                .accessibilityIdentifier("mg.midgard.search.audited")
                .id(g.seed)
        }
    }

    @ViewBuilder private var aside: some View {
        if let n = model.node {
            WebInspectorTitle(symbol: WebIcon.symbol("boxes"), title: n.label, identifier: "mg.midgard.inspector.title") {
                WebMarkingChip(midgardChip(n.marking))
            }
            WebStatPair((n.typeName.uppercased(), "OBJECT TYPE", false), ("\(n.hop)", "HOPS FROM ROOT", false))
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("PROPERTIES")
                WebKeyValues([("object_id", n.id)] + n.propertyValues.sorted { $0.key < $1.key }.map { ($0.key, $0.value) })
            }
            WebTerminal("QUERY · AUDITED", lines: [
                "> fabric.searchAround(\"\(model.run.seed)\")",
                "hops      \(model.run.hops) · links \(model.edges.count) · objects \(model.nodes.count)",
                "policy    per object, at query time",
                "# decision audited · #\(model.auditId ?? "—")",
            ])
        }
    }
}

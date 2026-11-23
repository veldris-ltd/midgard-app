import MidgardCore
import SwiftUI
import UniformTypeIdentifiers

// Midgard's GOVERNANCE screens, matched to the web console: Policies
// (`PoliciesScreen.tsx`) and Audit (`AuditScreen.tsx`). [NFR-SEC-2] [AC-M-4] [DD-7]

// MARK: - Policies

struct PoliciesScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: PoliciesModel
    @State private var inspecting = false

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: PoliciesModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Policies"), asideIdentifier: ctx.detail("Policies"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Policies"))) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("NFR-SEC-2", "Role, marking and purpose enforced at query time"),
                               ("DD-8", "Standards-aligned assurance: NIST SP 800-53, OWASP ASVS, ISO/IEC 27001")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
    }

    static func ruleState(_ r: MidgardPolicyRule) -> WebStatusPill {
        r.effect == "deny" ? WebStatusPill(.gated, "Deny") : WebStatusPill(.secure, "Enforced")
    }

    @ViewBuilder private var main: some View {
        let rules = model.rules.map(\.item)
        WebPageHeader(number: 8, section: "GOVERNANCE", title: "Policies & markings") {
            WebStatBlock(value: "\(rules.count)", label: "POLICIES")
            WebStatBlock(value: "4", label: "MARKING LEVELS")
            WebStatBlock(value: "0", label: "UNGOVERNED TYPES", accent: .midgard)
            WebButton("New policy", symbol: "plus",
                      disabledReason: "Not available yet: policies come from the policy file at boot")
        }
        WebTabs([("Policies", "\(rules.count)"), ("Markings", nil), ("Purposes", "\(model.purposes.count)")],
                selection: $model.tab, identifier: "mg.midgard.policies.tab")
        switch model.tab {
        case 1:
            VStack(spacing: 10) {
                ForEach(PoliciesModel.markings, id: \.self) { level in
                    let count = rules.filter { $0.marking == level }.count
                    WebPanel {
                        FlowRow(spacing: 14) {
                            WebMarkingChip(midgardChip(level))
                            Text("\(count) polic\(count == 1 ? "y" : "ies") reference this marking")
                                .font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textMuted)
                            WebButton("Edit clearances", variant: .ghost, size: .sm,
                                      disabledReason: "Not available yet: clearances are set when an identity is enrolled")
                        }
                    }
                }
            }
        case 2:
            VStack(spacing: 10) {
                ForEach(model.purposes, id: \.self) { purpose in
                    let count = rules.filter { $0.purpose == purpose }.count
                    WebPanel {
                        HStack(spacing: 14) {
                            Image(systemName: WebIcon.symbol("target")).foregroundStyle(MGWebAccent.midgard.text)
                                .accessibilityHidden(true)
                            Text(purpose).font(MGWebFont.mono(14, .callout)).foregroundStyle(MGWeb.grey100)
                            Spacer(minLength: 0)
                            Text("\(count) rule\(count == 1 ? "" : "s")").font(MGWebFont.mono(12, .caption))
                                .foregroundStyle(MGWeb.textMuted)
                        }
                    }
                    .accessibilityElement(children: .combine)
                }
            }
        default:
            if ctx.compact, let p = model.current {
                MidgardInspectButton(title: "Inspect \(p.name)") { inspecting = true }
            }
            WebTable([
                WebColumn("Policy", text: { (r: MidgardPolicyRule) in r.name }) { r in
                    MidgardIconCell(symbol: WebIcon.symbol("shield-check"), text: r.name)
                },
                WebColumn("Effect", width: 80, text: { $0.effect }) { r in
                    WebBadge(r.effect.uppercased(), tone: r.effect == "deny" ? .danger : .secure)
                },
                WebColumn("Roles", width: 150, mono: true) { $0.roles.joined(separator: ", ") },
                WebColumn("Marking", width: 120, mono: true) { $0.marking },
                WebColumn("Purpose", width: 110, mono: true) { $0.purpose },
                WebColumn("State", width: 120, text: { $0.effect == "deny" ? "Deny" : "Enforced" }) { r in Self.ruleState(r) },
            ], rows: rules, id: \.id, selected: model.selected, minWidth: 720, asList: ctx.compact,
               identifier: "mg.midgard.policies.table") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
            MidgardWithheld(count: model.withheld, noun: "policy")
            if model.state == .loading { WebMicro("LOADING POLICY SET…") }
        }
        dryRun
    }

    private var dryRun: some View {
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 9, "POLICY DRY-RUN · DENY BY DEFAULT", accent: .midgard)
            WebPanel(padding: 18, accentRule: true) {
                VStack(alignment: .leading, spacing: 14) {
                    WebFormRow {
                        WebSelect("Purpose", options: model.purposes.map { ($0, $0) }, selection: $model.purpose,
                                  identifier: "mg.midgard.policies.purpose")
                        WebSelect("Marking", options: PoliciesModel.markings.map { ($0, $0) }, selection: $model.marking,
                                  identifier: "mg.midgard.policies.marking")
                    } action: {
                        WebButton("Evaluate", symbol: WebIcon.symbol("play"),
                                  disabledReason: model.busy ? "Evaluating" : nil,
                                  identifier: "mg.midgard.policies.evaluate") {
                            Task { await model.evaluate() }
                        }
                    }
                    let contextLayout = ctx.compact ? AnyLayout(VStackLayout(alignment: .leading, spacing: 6))
                                                    : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: 18))
                    contextLayout {
                        context("SUBJECT", model.subject)
                        context("CLEARANCE", model.clearance)
                        context("ROLES", "[\(model.roles.joined(separator: ", "))]")
                    }
                    if let error = model.error {
                        WebNotice(error, tone: .danger, identifier: "mg.midgard.policies.error")
                    } else if let d = model.decision, let asked = model.decided {
                        decision(d, asked)
                    } else {
                        WebMicro("NO ALLOW RULE MATCHES UNLESS PROVEN · EVALUATE TO SEE THE SIGNED DECISION")
                    }
                }
            }
        }
    }

    private func context(_ key: String, _ value: String) -> some View {
        (Text(key + "  ").foregroundStyle(MGWeb.textMuted) + Text(value).foregroundStyle(MGWeb.grey100))
            .font(MGWebFont.mono(12, .caption))
            .fixedSize(horizontal: false, vertical: true)
    }

    private func decision(_ d: MidgardPolicyAnswer, _ asked: (purpose: String, marking: String, auditId: String?)) -> some View {
        let granted = d.decision == "grant"
        return VStack(alignment: .leading, spacing: 10) {
            FlowRow(spacing: 12) {
                WebBadge(d.decision.uppercased(), tone: granted ? .secure : .danger)
                Text("\(asked.marking.uppercased()) · \(asked.purpose)").font(MGWebFont.mono(12, .caption))
                    .foregroundStyle(MGWeb.textMuted)
                Text("MATCHED RULE  \(d.matchedRule ?? "none — deny-by-default")").font(MGWebFont.mono(11))
                    .foregroundStyle(MGWeb.textFaint)
            }
            Text(d.reason).font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textBody)
                .fixedSize(horizontal: false, vertical: true)
            WebMicro("DECISION AUDITED · ML-DSA SIGNED · #\(asked.auditId ?? "—")", tracking: 0.1)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceInset))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(granted ? MGWeb.secure : MGWeb.danger))
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.midgard.policies.decision")
    }

    @ViewBuilder private var aside: some View {
        if let p = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("shield-check"), title: p.name, identifier: "mg.midgard.inspector.title") {
                EmptyView()
            }
            WebStatPair((p.effect.uppercased(), "EFFECT", p.effect != "allow"), (p.marking.uppercased(), "MARKING", false))
            WebTerminal("POLICY · DECLARATIVE", lines: [
                "\(p.effect) when {", "  role    in [\(p.roles.joined(separator: ", "))]",
                "  marking <= \(p.marking.uppercased())", "  purpose == \(p.purpose)", "}",
            ])
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("EVALUATED AT")
                WebCheckList(["Query time · row and column level", "Action invocation · pre-execution",
                              "Agent retrieval · OAG scope"])
            }
        }
    }
}

// MARK: - Audit

/// The records Export chain saves, as a JSON file (the Mac's save panel, the iPhone's
/// share sheet).
public struct AuditChainFile: FileDocument, Transferable {
    public static var readableContentTypes: [UTType] { [.json] }
    public let data: Data
    public let name: String

    public init(data: Data, name: String) {
        self.data = data
        self.name = name
    }

    public init(configuration: ReadConfiguration) throws {
        data = configuration.file.regularFileContents ?? Data()
        name = "audit-chain.json"
    }

    public func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        FileWrapper(regularFileWithContents: data)
    }

    public static var transferRepresentation: some TransferRepresentation {
        DataRepresentation(exportedContentType: .json) { $0.data }
            .suggestedFileName { $0.name }
    }
}

struct AuditScreen: View {
    let ctx: MidgardScreenContext
    @StateObject private var model: AuditModel
    @State private var inspecting = false
    @State private var exporting = false
    @State private var exported: String?

    init(ctx: MidgardScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: AuditModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Audit"), asideIdentifier: ctx.detail("Audit"),
                       inspecting: $inspecting, top: AnyView(ctx.header("Audit"))) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("NFR-SEC-2", "Role/marking/purpose access control with full audit lineage"),
                               ("NFR-AI-1", "Every agentic action traceable, evaluated, reversible or human-gated")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
        .fileExporter(isPresented: $exporting, document: AuditChainFile(data: model.exportJSON(), name: AuditModel.exportName()),
                      contentType: .json, defaultFilename: AuditModel.exportName()) { result in
            if case let .success(url) = result { exported = "SAVED · \(url.lastPathComponent)" }
        }
    }

    @ViewBuilder private var exportButton: some View {
        let rows = model.rows
        #if os(iOS)
        if rows.isEmpty {
            WebButton("Export chain", symbol: WebIcon.symbol("download"), variant: .secondary,
                      disabledReason: "No records are shown", identifier: "mg.midgard.audit.export")
        } else {
            ShareLink(item: AuditChainFile(data: model.exportJSON(), name: AuditModel.exportName()),
                      preview: SharePreview("Audit chain · \(rows.count) records")) {
                HStack(spacing: 8) {
                    Image(systemName: WebIcon.symbol("download")).accessibilityHidden(true)
                    Text("Export chain").font(MGWebFont.sans(14, .callout).weight(.semibold))
                }
                .foregroundStyle(MGWeb.textHeading)
                .padding(.horizontal, 18)
                .frame(minHeight: 44)
                .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceCard))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
            }
            .accessibilityHint(Text("Shares the records shown, as JSON"))
            .accessibilityIdentifier("mg.midgard.audit.export")
        }
        #else
        WebButton("Export chain", symbol: WebIcon.symbol("download"), variant: .secondary,
                  disabledReason: rows.isEmpty ? "No records are shown" : nil, identifier: "mg.midgard.audit.export") {
            exporting = true
        }
        .help("Save the records shown, as JSON")
        #endif
    }

    @ViewBuilder private var main: some View {
        let rows = model.rows
        WebPageHeader(number: 9, section: "AUDIT", title: "Audit lineage") {
            WebStatBlock(value: "\(model.total)", label: "RECORDS")
            WebStatBlock(value: model.signedPercent.map { "\($0)%" } ?? "—", label: "ML-DSA SIGNED", accent: .midgard)
            WebStatBlock(value: "\(model.total - model.signed)", label: "UNVERIFIED")
            exportButton
        }
        if let exported { WebNotice(exported, tone: .secure) }
        let layout = ctx.compact ? AnyLayout(VStackLayout(alignment: .leading, spacing: 10))
                                 : AnyLayout(HStackLayout(alignment: .center, spacing: 10))
        layout {
            WebTextField(nil, placeholder: "Filter by actor or resource…", text: $model.query, symbol: "magnifyingglass",
                         identifier: "mg.midgard.audit.filter", width: ctx.compact ? nil : 320)
            FlowRow(spacing: 6) {
                ForEach(AuditModel.Category.allCases, id: \.self) { c in
                    WebTag(c.label, active: model.category == c, identifier: "mg.midgard.audit.category.\(c.rawValue)") {
                        model.setCategory(c)
                    }
                }
            }
        }
        if ctx.compact, let e = model.current {
            MidgardInspectButton(title: "Inspect #\(e.item.id)") { inspecting = true }
        }
        MidgardAuditTable(rows: rows, compact: ctx.compact, withId: true, selected: model.currentIndex,
                          identifier: "mg.midgard.audit.table") { i in ctx.select(i, { model.selected = $0 }, $inspecting) }
        if rows.isEmpty, model.state != .loading { WebMicro("NO RECORDS MATCH THIS FILTER · APPEND-ONLY LEDGER") }
        MidgardWithheld(count: model.withheld, noun: "record")
        WebMicro("APPEND-ONLY · ML-DSA-SIGNED · \(model.signed)/\(model.total) RECORDS VERIFIED", tracking: 0.12)
    }

    @ViewBuilder private var aside: some View {
        if let e = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("scroll-text"), title: "#\(e.item.id)",
                              identifier: "mg.midgard.inspector.title") {
                MidgardSignatureBadge(status: e.signature)
            }
            VStack(alignment: .leading, spacing: 6) {
                WebLabel("LINEAGE TRACE").padding(.bottom, 2)
                ForEach(Array(model.trace.enumerated()), id: \.offset) { i, step in
                    VStack(alignment: .leading, spacing: 3) {
                        Text(step.0.uppercased()).font(MGWebFont.mono(9)).mgTracking(0.12, size: MGWebFont.monoSize(9))
                            .foregroundStyle(MGWeb.textFaint)
                        Text(step.1).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.grey100)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 6).fill(i == 1 ? MGWebAccent.midgard.tint : MGWeb.surfaceInset))
                    .overlay(RoundedRectangle(cornerRadius: 6).strokeBorder(i == 1 ? MGWebAccent.midgard.fill : MGWeb.border))
                    .accessibilityElement(children: .combine)
                }
            }
            let checked = e.signature == .verified || e.signature == .device
            WebTerminal("RECORD", lines: [
                "actor    \(e.item.actor)", "action   \(e.item.action)", "resource \(e.item.resource)",
                "sig      \(AuditModel.signatureText(e.signature))",
                checked ? "# append-only · verified" : "# signature not checked",
            ])
        }
    }
}

import MidgardCore
import SwiftUI

// Heimdall's DECIDE screens, matched to the web console: Workflows
// (`WorkflowsScreen.tsx`) and Decision Lineage (`DecisionLineageScreen.tsx`). [FR-H-5]
// [NFR-AI-1] [NFR-SEC-2] [DD-7]

// MARK: - Workflows

struct HeimdallWorkflowsScreen: View {
    let ctx: HeimdallScreenContext
    @StateObject private var model: HeimdallWorkflowsModel
    @State private var inspecting = false

    init(ctx: HeimdallScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: HeimdallWorkflowsModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Workflows"), asideIdentifier: ctx.detail("Workflows"),
                       inspecting: $inspecting,
                       top: ctx.header("Workflows") { HeimdallToolbarItem(screen: "Workflows", toolbar: ctx.bar) },
                       asideWidth: ctx.asideWidth("Workflows")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if let w = model.current {
                WebTerminal("DECISION RECORD", lines: [
                    "workflow  \(w.id)", "authoriser \(w.owner)", "basis     \(Int(w.opts)) cited options · reviewed",
                    "sig       ML-DSA · decision lineage",
                ], accent: ctx.accent)
            }
        }
        .task(id: ctx.persona) { await model.load() }
        .onChange(of: model.awaiting, initial: true) { _, n in ctx.bar.awaiting = n }
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 3, section: "WORKFLOWS", title: "Action workflows", accent: ctx.accent) {
            WebStatBlock(value: "\(model.rows.count)", label: "ACTIVE")
            WebStatBlock(value: "100%", label: "HUMAN-AUTHORISED", accent: ctx.accent)
            WebStatBlock(value: "0", label: "AUTO-EXECUTED · EVER")
        }
        if ctx.compact, let w = model.current {
            MidgardInspectButton(title: "Inspect \(w.id)", identifier: "mg.heimdall.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Workflow", width: 90, text: { (r: MidgardRow<HeimdallWorkflow>) in r.item.id }) { r in
                HeimdallIdCell(text: r.item.id)
            },
            WebColumn("Action") { $0.item.title },
            WebColumn("Stage", width: 190) { $0.item.stage },
            WebColumn("Owner", width: 110, mono: true) { $0.item.owner },
            WebColumn("Options", width: 70, align: .trailing, mono: true) { "\(Int($0.item.opts))" },
            WebColumn("Age", width: 60, mono: true) { $0.item.age },
            WebColumn("State", width: 130, text: { $0.item.statusLabel }) { r in heimdallPill(r.item.status, r.item.statusLabel) },
        ], rows: model.rows, id: \.id, selected: model.currentIndex, minWidth: 760, asList: ctx.compact, accent: ctx.accent,
           identifier: "mg.heimdall.workflows.table") { i in ctx.select(i, { model.select($0) }, $inspecting) }
        MidgardWithheld(count: model.withheld, noun: "workflow")
        if model.state == .loading { WebMicro("LOADING WORKFLOWS…") }
        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 4, "PRINCIPLE", accent: ctx.accent, muted: true)
            WebPanel {
                VStack(alignment: .leading, spacing: 10) {
                    Text("AI generates and ranks options; it never executes. Every consequential action passes a named human, and the authorisation, the basis for it, and the outcome are one signed record.")
                        .font(MGWebFont.sans(14))
                        .foregroundStyle(MGWeb.textBody)
                        .lineSpacing(4)
                        .fixedSize(horizontal: false, vertical: true)
                    WebProvenance([("FR-H-5", "Human-gated action workflows with full audit lineage"),
                                   ("NFR-AI-1", "Traceable, evaluated, reversible or human-gated")])
                }
            }
        }
    }

    @ViewBuilder private var aside: some View {
        if let w = model.current {
            WebInspectorTitle(symbol: "arrow.triangle.branch", title: w.title, mono: false, accent: ctx.accent,
                              identifier: "mg.heimdall.inspector.title") { EmptyView() }
            VStack(alignment: .leading, spacing: 0) {
                WebLabel("STAGES").padding(.bottom, 8)
                ForEach(Array(model.stages(w).enumerated()), id: \.offset) { i, stage in
                    HStack(spacing: 10) {
                        Image(systemName: stage.1 ? "checkmark" : "clock")
                            .font(MGWebFont.sans(13))
                            .foregroundStyle(stage.1 ? MGWeb.secureText : MGWeb.textFaint)
                            .accessibilityHidden(true)
                        Text(stage.0)
                            .font(MGWebFont.sans(12, .caption))
                            .foregroundStyle(stage.1 ? MGWeb.textBody : MGWeb.textFaint)
                        Spacer(minLength: 0)
                    }
                    .padding(.vertical, 7)
                    .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                    .accessibilityElement(children: .ignore)
                    .accessibilityLabel(Text("\(stage.0): \(stage.1 ? "done" : "waiting")"))
                    .accessibilityIdentifier("mg.heimdall.workflows.stage.\(i + 1)")
                }
            }
            if w.humanGated && !model.authorised {
                WebHumanGate(title: "\(w.id) · authorise", detail: "HUMAN AUTHORISATION REQUIRED · AI IS ADVISORY",
                             state: {
                                 switch model.gate {
                                 case .queued?: return .queued
                                 case .rejected?: return .rejected
                                 default: return .pending
                                 }
                             }(),
                             accent: ctx.accent, busy: model.busy,
                             onAuthorise: { Task { await model.authorise() } },
                             onReject: { model.reject() },
                             idPrefix: "mg.heimdall.workflows")
            } else {
                HStack(spacing: 9) {
                    Image(systemName: "checkmark.shield").foregroundStyle(MGWeb.secureText).accessibilityHidden(true)
                    Text(model.signedLabel(w))
                        .font(MGWebFont.mono(10))
                        .mgTracking(0.08, size: MGWebFont.monoSize(10))
                        .foregroundStyle(MGWeb.secureText)
                        .accessibilityIdentifier("mg.heimdall.workflows.signed")
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceInset))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.secure))
            }
            if let message = model.message {
                Text(message)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.warnText)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mg.heimdall.workflows.message")
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "Every workflow is above your clearance." : "No workflow is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

// MARK: - Decision lineage

struct HeimdallLineageScreen: View {
    let ctx: HeimdallScreenContext
    @StateObject private var model: HeimdallLineageModel
    @State private var inspecting = false

    init(ctx: HeimdallScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: HeimdallLineageModel(app: ctx.app))
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Decision Lineage"),
                       asideIdentifier: ctx.detail("Decision Lineage"), inspecting: $inspecting,
                       top: ctx.header("Decision Lineage") { HeimdallToolbarItem(screen: "Decision Lineage", toolbar: ctx.bar) },
                       asideWidth: ctx.asideWidth("Decision Lineage")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.current != nil {
                WebProvenance([("FR-H-5", "Full audit lineage on decision workflows"),
                               ("NFR-SEC-2", "Full audit lineage enforced at the data and object layer")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
        // The live tail, re-read while the screen is open.
        .task(id: ctx.persona) { await model.follow() }
    }

    /// A decision's signature as delivered: the core replaces the sample's claim with
    /// "not checked (demo)", which is shown as such, never as a check mark.
    private func signature(_ text: String) -> some View {
        let checked = text.contains("\u{2713}")
        return WebBadge(checked ? text : text.uppercased(), tone: checked ? .secure : .warn)
    }

    @ViewBuilder private var main: some View {
        WebPageHeader(number: 5, section: "LINEAGE", title: "Decision lineage", accent: ctx.accent) {
            WebStatBlock(value: "312", label: "DECISIONS · 90D")
            WebStatBlock(value: model.signedText, label: "TAIL SIGNED · CHECKED", accent: ctx.accent)
            WebStatBlock(value: "0", label: "CHAIN BREAKS")
        }
        if ctx.compact, let d = model.current {
            MidgardInspectButton(title: "Inspect #\(d.id)", identifier: "mg.heimdall.inspect") { inspecting = true }
        }
        WebTable([
            WebColumn("Decision", width: 90, text: { (r: MidgardRow<HeimdallDecision>) in "#\(r.item.id)" }) { r in
                HeimdallIdCell(text: "#\(r.item.id)")
            },
            WebColumn("Action") { $0.item.action },
            WebColumn("Authoriser", width: 110, mono: true) { $0.item.authoriser },
            WebColumn("Basis", width: 150, mono: true) { $0.item.basis },
            WebColumn("Outcome", width: 170) { $0.item.outcome },
            WebColumn("Signature", width: 170, text: { $0.item.signature }) { r in signature(r.item.signature) },
            WebColumn("When", width: 100, mono: true) { $0.item.when },
        ], rows: model.decisions, id: \.id, selected: model.currentIndex, minWidth: 880, asList: ctx.compact,
           accent: ctx.accent, identifier: "mg.heimdall.lineage.decisions") { i in
            ctx.select(i, { model.selected = $0 }, $inspecting)
        }
        MidgardWithheld(count: model.withheld, noun: "decision")
        if model.state == .loading { WebMicro("VERIFYING CHAIN…") }

        VStack(alignment: .leading, spacing: 8) {
            WebSectionLabel(number: 6, "DECISION LINEAGE · LIVE", accent: ctx.accent, muted: true)
            WebTable([
                WebColumn("Authoriser", mono: true) { (r: MidgardRow<AuditRecord>) in r.item.actor },
                WebColumn("Decision") { $0.item.action },
                WebColumn("Resource", mono: true) { $0.item.resource },
                WebColumn("Signature", width: 110, text: { AuditModel.signatureText($0.signature) }) { r in
                    MidgardSignatureBadge(status: r.signature)
                },
                WebColumn("TS", width: 90, mono: true) { $0.item.ts },
            ], rows: model.tail, id: \.id, compact: true, minWidth: 660, asList: ctx.compact, accent: ctx.accent,
               identifier: "mg.heimdall.lineage.tail")
            MidgardWithheld(count: model.tailWithheld, noun: "record")
        }
        WebTerminal("CHAIN VERIFICATION", lines: [
            "> heimdall lineage verify --range 90d",
            "records    \(model.tail.count) shown · signatures \(model.signedText) checked here",
            "chain      contiguous · no gaps",
            model.signed == model.tail.count && !model.tail.isEmpty ? "# lineage intact"
                : "# the sample's seeded records carry no signature; this device signs its own",
        ], accent: ctx.accent)
            .accessibilityIdentifier("mg.heimdall.lineage.chain")
    }

    @ViewBuilder private var aside: some View {
        if let d = model.current {
            WebInspectorTitle(symbol: WebIcon.symbol("scroll-text"), title: "#\(d.id)", accent: ctx.accent,
                              identifier: "mg.heimdall.inspector.title") {
                signature(d.signature)
            }
            HeimdallHistory(title: "REPLAY · EVIDENCE AT DECISION TIME", rows: [
                ("Picture state", "snapshot #S-88121 · 11 feeds"),
                ("Entity", "E-0871 · UNKNOWN · conf 54%"),
                ("Options shown", "3 · all cited · ranks preserved"),
                ("Chosen", "opt 01 · re-task NORTH-2"),
                ("Authoriser", d.authoriser + " · WebAuthn verified"),
            ], keyWidth: 100, upperKeys: true)
            HeimdallNoteCard(heading: "OUTCOME", headingColour: MGWeb.secureText, border: MGWeb.border,
                             text: "\(d.outcome) · The decision, its evidence, and its result are one chain: audit, review, and after-action all read the same record.",
                             identifier: "mg.heimdall.lineage.outcome")
            WebButton("Replay picture at decision time", symbol: WebIcon.symbol("play"), size: .sm,
                      disabledReason: "Not available in this app: replay needs the picture's recorded snapshots", accent: ctx.accent,
                      identifier: "mg.heimdall.lineage.replay")
        }
    }
}

import MidgardCore
import SwiftUI

// Forge's build screens, matched to the web console: Pipeline Builder
// (`PipelineBuilderScreen.tsx`) and Transform IDE (`TransformIdeScreen.tsx`). [FR-F-1]
// [FR-F-3] [FR-F-7] [NFR-AI-1] [DD-7]

// MARK: - Pipeline builder

struct ForgePipelineScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: ForgePipelineModel
    @State private var inspecting = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: ForgePipelineModel(app: ctx.app))
    }

    var body: some View {
        Group {
            if ctx.compact {
                WebConsolePage(compact: true, identifier: ctx.content("Pipeline Builder"),
                               asideIdentifier: ctx.detail("Pipeline Builder"), inspecting: $inspecting,
                               top: ctx.header("Pipeline Builder"), asideWidth: 320) {
                    compactMain
                } aside: {
                    aside
                } asideFoot: {
                    EmptyView()
                }
            } else {
                HStack(spacing: 0) {
                    wideMain
                    Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true)
                    rail.frame(width: ctx.asideWidth("Pipeline Builder"))
                }
            }
        }
        .task(id: ctx.persona) { await model.load() }
        .onChange(of: model.context, initial: true) { _, c in ctx.bar.context = c }
        .onChange(of: model.pipeline?.branch, initial: true) { _, b in ctx.bar.branch = b ?? "MAIN" }
        .onChange(of: model.building, initial: true) { _, b in ctx.bar.building = b }
        .onChange(of: model.runDisabled, initial: true) { _, r in ctx.bar.runDisabled = r }
        .onAppear { ctx.bar.run = { Task { await model.run() } } }
        .onDisappear {
            ctx.bar.run = nil
            ctx.bar.context = nil
        }
    }

    // The Mac: the strip, the graph filling the column, the build log and Mimir's
    // suggestion under it, and the node's inspector in the rail.
    private var wideMain: some View {
        VStack(spacing: 0) {
            strip
                .padding(.horizontal, 16)
                .padding(.vertical, 14)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
            editTools
            ScrollView([.horizontal, .vertical]) {
                ForgeGraphCanvas(model: model, editable: true) { _ in }
            }
            // From the graph's first column: centred, the sources were cut at the left
            // edge at the default window (the graph is wider than the column).
            .defaultScrollAnchor(.topLeading)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(MGWeb.bgSunken)
            .overlay(alignment: .bottomLeading) {
                if model.state == .loading { WebMicro("RESOLVING PIPELINE GRAPH…").padding(16) }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text("Pipeline graph"))
            .accessibilityIdentifier("mg.forge.pipeline.graph")
            Rectangle().fill(MGWeb.border).frame(height: 1).accessibilityHidden(true)
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .top, spacing: 0) {
                    buildLog.frame(minWidth: 320, maxWidth: .infinity)
                    Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true)
                    suggestion.frame(width: 330)
                }
                VStack(spacing: 0) {
                    buildLog
                    Rectangle().fill(MGWeb.border).frame(height: 1).accessibilityHidden(true)
                    suggestion
                }
            }
            .fixedSize(horizontal: false, vertical: true)
        }
        .background(MGWeb.bgPage)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(ctx.content("Pipeline Builder"))
    }

    private var rail: some View {
        GeometryReader { geo in
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    aside
                }
                .padding(18)
                .frame(maxWidth: .infinity, minHeight: max(0, geo.size.height), alignment: .topLeading)
            }
        }
        .background(MGWeb.bgSunken)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Inspector"))
        .accessibilityIdentifier(ctx.detail("Pipeline Builder"))
    }

    // The iPhone: the strip, the graph scrolled sideways at its own height, the log and
    // the suggestion, each node's inspector (with its settings) in a sheet.
    @ViewBuilder private var compactMain: some View {
        strip
        WebNotice("ON THE IPHONE THE GRAPH IS VIEWED AND A NODE'S SETTINGS EDITED; GRAPHS ARE DRAWN ON THE MAC.",
                  tone: .neutral, identifier: "mg.forge.pipeline.ios-note")
        if model.node != nil {
            MidgardInspectButton(title: "Inspect \(model.node?.label ?? "")", identifier: "mg.forge.inspect") { inspecting = true }
        }
        ScrollView(.horizontal) {
            ForgeGraphCanvas(model: model, editable: false) { _ in inspecting = true }
        }
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Pipeline graph"))
        .accessibilityIdentifier("mg.forge.pipeline.graph")
        MidgardWithheld(count: model.withheld, noun: "pipeline")
        if model.state == .loading { WebMicro("RESOLVING PIPELINE GRAPH…") }
        buildLog.clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        suggestion
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
    }

    /// The build status, meter and the pipeline tags (the web's strip above the graph).
    private var strip: some View {
        FlowRow(spacing: 10) {
            // The pill says the meter's figure too: the meter is drawn only (as one
            // element it measured under the touch target's height in the second audit
            // pass, a frame around it notwithstanding).
            forgePill(model.building ? "live" : "healthy", model.building ? model.phase : "Ready")
                .accessibilityLabel(Text("\(model.building ? model.phase : "Ready"), build \(model.progress)%"))
                .accessibilityIdentifier("mg.forge.pipeline.status")
            HStack(spacing: 8) {
                WebLabel("BUILD")
                WebMeterBar(Double(model.progress), width: 90, accent: ctx.accent)
            }
            .accessibilityHidden(true)

            ForEach(Array(model.rows.enumerated()), id: \.offset) { i, r in
                WebTag(r.item.name, active: model.pipelineIndex == i, accent: ctx.accent,
                       identifier: "mg.forge.pipeline.tag.\(r.item.id)") {
                    model.choose(i)
                }
            }
            if model.drawingNew {
                WebTag("new graph · this Mac", active: true, accent: ctx.accent, identifier: "mg.forge.pipeline.tag.new")
            }
            if !ctx.compact {
                HStack(spacing: 8) {
                    WebTag("low-code", active: true, accent: ctx.accent)
                    WebTag("pro-code", accent: ctx.accent)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text("Mode: low-code"))
                .accessibilityAddTraits(.isStaticText)
            }
        }
    }

    /// The Mac's drawing tools: new graph, add a node, connect, remove, discard; and
    /// what an edit is (a draft on this Mac, never sent).
    @ViewBuilder private var editTools: some View {
        FlowRow(spacing: 8) {
            WebButton("New graph", symbol: WebIcon.symbol("plus"), variant: .secondary, size: .sm, accent: ctx.accent,
                      identifier: "mg.forge.pipeline.new") { model.drawNew() }
            ForEach(ForgeGraph.kinds, id: \.kind) { k in
                WebButton("Add \(k.kind)", symbol: WebIcon.symbol(k.icon), variant: .ghost, size: .sm, accent: ctx.accent,
                          identifier: "mg.forge.pipeline.add.\(k.kind)") { model.addNode(kind: k.kind) }
            }
            WebButton(model.connectFrom == nil ? "Connect from node" : "Choose the target…",
                      symbol: WebIcon.symbol("arrow-right"), variant: model.connectFrom == nil ? .ghost : .outline, size: .sm,
                      disabledReason: model.node == nil ? "Choose a node to connect from" : nil, accent: ctx.accent,
                      identifier: "mg.forge.pipeline.connect") {
                model.connectFrom = model.connectFrom == nil ? model.selectedNode : nil
            }
            WebButton("Remove node", symbol: WebIcon.symbol("x"), variant: .ghost, size: .sm,
                      disabledReason: model.node == nil ? "Choose a node to remove" : nil, accent: ctx.accent,
                      identifier: "mg.forge.pipeline.remove") { model.remove(model.selectedNode) }
            if model.edited || model.drawingNew {
                WebButton("Discard edits", symbol: WebIcon.symbol("undo-2"), variant: .ghost, size: .sm, accent: ctx.accent,
                          identifier: "mg.forge.pipeline.discard") { model.discard() }
                WebBadge("DRAFT · THIS MAC ONLY · NOT SENT", tone: .warn)
                    .accessibilityIdentifier("mg.forge.pipeline.draft")
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
    }

    private var buildLog: some View {
        ScrollViewReader { proxy in
            ScrollView {
                WebTerminal(model.building ? "BUILD · RUNNING" : "BUILD LOG", lines: model.log, accent: ctx.accent)
                    .accessibilityIdentifier("mg.forge.pipeline.log")
                    .id("log")
            }
            .frame(maxHeight: 200)
            .onChange(of: model.log.count) { proxy.scrollTo("log", anchor: .bottom) }
        }
    }

    private var suggestion: some View {
        VStack(alignment: .leading, spacing: 10) {
            WebSectionLabel(number: 4, "MIMIR SUGGESTION", accent: ctx.accent, muted: true)
            Text(model.drawingNew ? "—" : model.pipeline?.suggestion ?? "—")
                .font(MGWebFont.sans(12, .caption))
                .foregroundStyle(MGWeb.textBody)
                .lineSpacing(4)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                WebButton("Preview transform", symbol: WebIcon.symbol("sparkles"), variant: .outline, size: .sm, fullWidth: true,
                          disabledReason: "Not available in this app: open the Transform IDE to preview a change",
                          accent: ctx.accent, identifier: "mg.forge.pipeline.suggestion.preview")
                WebButton("Dismiss", variant: .ghost, size: .sm,
                          disabledReason: "Not available in this app: the suggestion is the enclosure's",
                          accent: ctx.accent, identifier: "mg.forge.pipeline.suggestion.dismiss")
            }
            Text("LOCAL INFERENCE · Mistral Small 3.2 · PREVIEW-BEFORE-COMMIT [FR-F-7]")
                .font(MGWebFont.mono(MGWebFont.macLegible(9)))
                .foregroundStyle(MGWeb.textFaint)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(16)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private var aside: some View {
        if let n = model.node {
            WebInspectorTitle(symbol: WebIcon.symbol(n.icon), title: n.label, accent: ctx.accent,
                              identifier: "mg.forge.inspector.title") {
                forgePill(n.status, n.status == "warn" ? "Drift" : "Healthy")
            }
            WebStatPair((model.drawingNew ? "—" : model.pipeline?.currentVersion ?? "—", "VERSION", false),
                        (model.drawingNew ? "—" : model.pipeline?.expectations ?? "—", "EXPECTATIONS", false),
                        accent: ctx.accent)
            if let p = model.pipeline, !model.drawingNew {
                VStack(alignment: .leading, spacing: 0) {
                    WebLabel("VERSION HISTORY").padding(.bottom, 8)
                    ForEach(Array(p.versions.enumerated()), id: \.offset) { _, v in
                        HStack(spacing: 8) {
                            Image(systemName: WebIcon.symbol("git-commit")).font(MGWebFont.sans(12))
                                .foregroundStyle(MGWeb.decorFaint).accessibilityHidden(true)
                            Text(v.version).foregroundStyle(MGWeb.grey100)
                            Text(v.branch).foregroundStyle(MGWeb.textFaint).lineLimit(1)
                            Spacer(minLength: 4)
                            Text(v.ts).foregroundStyle(MGWeb.textFaint)
                            WebBadge(v.status, tone: v.status == "OK" ? .secure : .danger)
                        }
                        .font(MGWebFont.mono(11))
                        .padding(.vertical, 7)
                        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                        .accessibilityElement(children: .combine)
                    }
                }
            }
            VStack(alignment: .leading, spacing: 8) {
                WebLabel("ENCRYPTION")
                Text("AT REST  AES-256-GCM · ML-KEM-1024\nTRANSIT  X25519 + ML-KEM-768\nLINEAGE  ML-DSA SIGNED")
                    .font(MGWebFont.mono(10))
                    .foregroundStyle(MGWeb.textMuted)
                    .lineSpacing(8)
                    .fixedSize(horizontal: false, vertical: true)
            }
            nodeSettings(n)
            WebButton("Open projection mapping", symbol: WebIcon.symbol("boxes"), variant: .secondary, size: .sm,
                      disabledReason: "Not available in this app: open Object projection from the navigation",
                      accent: ctx.accent, identifier: "mg.forge.pipeline.open-projection")
            if let message = model.message {
                Text(message)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.warnText)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mg.forge.pipeline.message")
            }
        } else if model.state != .loading {
            Text(model.drawingNew ? "Add a node to start the graph." :
                    model.withheld > 0 && model.rows.isEmpty ? "Every pipeline is above your clearance." : "No node is selected.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }

    /// A node's settings, edited on this device (both apps): its label and the line
    /// under it. Not sent: the enclosure's graph is what a build runs.
    private func nodeSettings(_ n: ForgeGraph.Node) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            WebLabel("NODE SETTINGS · ON THIS DEVICE")
            WebTextField("Label", placeholder: "Node label",
                         text: Binding(get: { model.node?.label ?? "" }, set: { model.setNode(n.id, label: $0) }),
                         identifier: "mg.forge.node.label")
            WebTextField("Detail", placeholder: "What the node does",
                         text: Binding(get: { model.node?.meta ?? "" }, set: { model.setNode(n.id, meta: $0) }),
                         identifier: "mg.forge.node.meta")
            Text(model.edited ? "Edited here; a build runs the enclosure's graph." : "As the enclosure has it.")
                .font(MGWebFont.sans(12, .caption))
                .foregroundStyle(model.edited ? MGWeb.warnText : MGWeb.textMuted)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("mg.forge.node.state")
        }
    }
}

/// The pipeline's graph as the web draws it: nodes 190 × 84 on a 250 × 150 grid, edges
/// as curves from a node's right edge to the next one's left. Choosing a node selects it
/// (or, in Connect, makes the edge); `editable` (the Mac) lets a node be dragged to a
/// new place. [FR-F-1]
struct ForgeGraphCanvas: View {
    @ObservedObject var model: ForgePipelineModel
    let editable: Bool
    let onChoose: (String) -> Void
    @State private var drag: (id: String, offset: CGSize)?

    private func origin(_ n: ForgeGraph.Node) -> CGPoint {
        let o = ForgeGraph.origin(n)
        guard let drag, drag.id == n.id else { return o }
        return CGPoint(x: o.x + drag.offset.width, y: o.y + drag.offset.height)
    }

    var body: some View {
        let graph = model.graph
        let size = graph.size
        ZStack(alignment: .topLeading) {
            WebGridBackground().frame(width: size.width, height: size.height)
            Path { path in
                for e in graph.edges {
                    guard let a = graph.nodes.first(where: { $0.id == e.from }),
                          let b = graph.nodes.first(where: { $0.id == e.to }) else { continue }
                    let pa = origin(a), pb = origin(b)
                    let p1 = CGPoint(x: pa.x + ForgeGraph.nodeWidth, y: pa.y + ForgeGraph.nodeHeight / 2)
                    let p2 = CGPoint(x: pb.x, y: pb.y + ForgeGraph.nodeHeight / 2)
                    let mx = (p1.x + p2.x) / 2
                    path.move(to: p1)
                    path.addCurve(to: p2, control1: CGPoint(x: mx, y: p1.y), control2: CGPoint(x: mx, y: p2.y))
                }
            }
            .stroke(MGWeb.borderStrong, lineWidth: 1.5)
            .accessibilityHidden(true)
            ForEach(graph.nodes) { n in
                let o = origin(n)
                ForgeNodeView(node: n, selected: model.selectedNode == n.id, connecting: model.connectFrom == n.id) {
                    model.tap(n.id)
                    onChoose(n.id)
                }
                .offset(x: o.x, y: o.y)
                .gesture(editable ? DragGesture(minimumDistance: 4)
                    .onChanged { v in drag = (n.id, v.translation) }
                    .onEnded { v in
                        drag = nil
                        model.move(n.id, x: n.x + v.translation.width / ForgeGraph.column,
                                   y: n.y + v.translation.height / ForgeGraph.row)
                    } : nil)
            }
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
    }
}

/// The web's `mg-grid-bg`: faint 24 pt grid lines.
struct WebGridBackground: View {
    var body: some View {
        Canvas { context, size in
            var path = Path()
            var x: CGFloat = 0
            while x <= size.width {
                path.move(to: CGPoint(x: x, y: 0))
                path.addLine(to: CGPoint(x: x, y: size.height))
                x += 24
            }
            var y: CGFloat = 0
            while y <= size.height {
                path.move(to: CGPoint(x: 0, y: y))
                path.addLine(to: CGPoint(x: size.width, y: y))
                y += 24
            }
            context.stroke(path, with: .color(MGWeb.gridLine), lineWidth: 1)
        }
        .accessibilityHidden(true)
    }
}

/// One node (the web's `Node`): its icon and label, the status dot, the line under it
/// and its kind; on the raised surface with the accent rule when selected.
struct ForgeNodeView: View {
    let node: ForgeGraph.Node
    let selected: Bool
    let connecting: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 8) {
                    Image(systemName: WebIcon.symbol(node.icon)).font(MGWebFont.sans(14))
                        .foregroundStyle(MGWebAccent.forge.text)
                    Text(node.label)
                        .font(MGWebFont.mono(12, .caption).weight(.medium))
                        .foregroundStyle(MGWeb.grey100)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Circle().fill(node.status == "warn" ? MGWeb.warn : MGWeb.secure).frame(width: 7, height: 7)
                }
                Text(node.meta).font(MGWebFont.mono(11)).foregroundStyle(MGWeb.textFaint).lineLimit(1)
                Text(node.kind.uppercased())
                    .font(MGWebFont.mono(9))
                    .mgTracking(0.12, size: MGWebFont.monoSize(9))
                    .foregroundStyle(MGWeb.textFaint)
                    .lineLimit(1)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .frame(width: ForgeGraph.nodeWidth, height: ForgeGraph.nodeHeight, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(selected ? MGWeb.surfaceRaised : MGWeb.surfaceCard))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md)
                .strokeBorder(connecting ? MGWeb.warn : selected ? MGWebAccent.forge.fill : MGWeb.border,
                              lineWidth: connecting ? 2 : 1))
            .shadow(color: selected ? MGWebAccent.forge.glow : .clear, radius: selected ? 10 : 0)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("\(node.label), \(node.kind), \(node.meta), \(node.status == "warn" ? "drift" : "healthy")"))
        .accessibilityAddTraits(selected ? [.isButton, .isSelected] : [.isButton])
        .accessibilityIdentifier("mg.forge.node.\(node.id)")
    }
}

// MARK: - Transform IDE

struct ForgeTransformScreen: View {
    let ctx: ForgeScreenContext
    @StateObject private var model: ForgeTransformModel
    @State private var inspecting = false
    @State private var editing = false

    init(ctx: ForgeScreenContext) {
        self.ctx = ctx
        _model = StateObject(wrappedValue: ForgeTransformModel(app: ctx.app))
    }

    /// A file name as VoiceOver says it ("fuse positions, Python file"): the audit reads
    /// "fuse_positions.py" as not human-readable.
    static func spokenFile(_ name: String) -> String {
        let parts = name.split(separator: ".", maxSplits: 1).map(String.init)
        let kind: String
        switch parts.count > 1 ? parts[1] : "" {
        case "py": kind = "Python file"
        case "yml", "yaml": kind = "YAML file"
        case "": kind = "file"
        default: kind = "\(parts[1].uppercased()) file"
        }
        return "\(parts[0].replacingOccurrences(of: "_", with: " ")), \(kind)"
    }

    private func hunkColour(_ op: String) -> Color {
        switch op {
        case "+": return MGWeb.secureText
        case "-": return MGWeb.dangerText
        default: return MGWeb.warnText
        }
    }

    var body: some View {
        WebConsolePage(compact: ctx.compact, identifier: ctx.content("Transform IDE"), asideIdentifier: ctx.detail("Transform IDE"),
                       inspecting: $inspecting, top: ctx.header("Transform IDE"), asideWidth: ctx.asideWidth("Transform IDE")) {
            main
        } aside: {
            aside
        } asideFoot: {
            if model.transform != nil {
                WebProvenance([("FR-F-1", "Low-code builder and pro-code IDE over identical dataset contracts"),
                               ("FR-F-3", "Versioned with lineage; reproducible rebuilds")])
            }
        }
        .task(id: ctx.persona) { await model.load() }
        .onChange(of: model.transform?.branch, initial: true) { _, b in ctx.bar.branch = b ?? "DEV" }
        .onChange(of: model.transform?.nextVersion, initial: true) { _, v in ctx.bar.nextVersion = v ?? "" }
        .onChange(of: model.busy, initial: true) { _, b in ctx.bar.busy = b }
        // On the iPhone the preview and the gate are in the inspector: it opens on them.
        .onChange(of: model.gate) { _, g in if ctx.compact, g == .pending { inspecting = true } }
        .onAppear { ctx.bar.preview = { Task { await model.preview() } } }
        .onDisappear { ctx.bar.preview = nil }
    }

    @ViewBuilder private var main: some View {
        let t = model.transform
        WebPageHeader(number: 7, section: "PRO-CODE", title: "Transform IDE", accent: ctx.accent) {
            WebStatBlock(value: t.map { "\(Int($0.filesCount))" } ?? "—", label: "FILES ON BRANCH")
            WebStatBlock(value: t?.expectations ?? "—", label: "EXPECTATIONS", accent: ctx.accent)
            WebStatBlock(value: t?.nextVersion ?? "—", label: "NEXT VERSION")
        }
        if ctx.compact, t != nil {
            MidgardInspectButton(title: model.diff == nil ? "Inspect contract check" : "Inspect preview and gate",
                                 identifier: "mg.forge.inspect") { inspecting = true }
        }
        FlowRow(spacing: 8) {
            WebButton(editing ? "Done editing" : "Edit code", symbol: WebIcon.symbol("code"), variant: .secondary, size: .sm,
                      disabledReason: t == nil ? "No transform is loaded" : nil, accent: ctx.accent,
                      identifier: "mg.forge.ide.edit") { editing.toggle() }
            if model.edited {
                WebButton("Revert", symbol: WebIcon.symbol("undo-2"), variant: .ghost, size: .sm, accent: ctx.accent,
                          identifier: "mg.forge.ide.revert") { model.revert() }
                WebBadge("EDITED ON THIS DEVICE · PREVIEWED AS EDITED", tone: .warn)
            }
        }
        editor
        MidgardWithheld(count: model.withheld, noun: "transform")
        if let message = model.message, model.gate == nil {
            Text(message)
                .font(MGWebFont.sans(12, .caption))
                .foregroundStyle(MGWeb.warnText)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    /// The files and the code, as the web's editor box: the files on the left (on the
    /// iPhone above), the code with line numbers (or, editing, a monospaced editor).
    private var editor: some View {
        let files = model.transform?.files ?? []
        let layout = ctx.compact ? AnyLayout(VStackLayout(spacing: 0)) : AnyLayout(HStackLayout(spacing: 0))
        return layout {
            VStack(alignment: .leading, spacing: 2) {
                WebLabel("TRANSFORMS").padding(.horizontal, 6).padding(.top, 2).padding(.bottom, 6)
                ForEach(Array(files.enumerated()), id: \.offset) { i, f in
                    Button { model.fileIndex = i } label: {
                        HStack(spacing: 8) {
                            Image(systemName: WebIcon.symbol("file-text")).font(MGWebFont.sans(12)).accessibilityHidden(true)
                            Text(f).font(MGWebFont.mono(11)).lineLimit(1)
                            Spacer(minLength: 0)
                        }
                        .foregroundStyle(i == model.fileIndex ? MGWeb.textHeading : MGWeb.textMuted)
                        .padding(.horizontal, 8)
                        .padding(.vertical, 7)
                        .frame(minHeight: webTarget(28))
                        .background(RoundedRectangle(cornerRadius: 6).fill(i == model.fileIndex ? ctx.accent.tint : .clear))
                        .overlay(alignment: .leading) {
                            if i == model.fileIndex { Rectangle().fill(ctx.accent.fill).frame(width: 2) }
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(Text(Self.spokenFile(f)))
                    .accessibilityAddTraits(i == model.fileIndex ? [.isSelected] : [])
                    .accessibilityIdentifier("mg.forge.ide.file.\(i)")
                }
            }
            .padding(10)
            .frame(width: ctx.compact ? nil : 200, alignment: .topLeading)
            .frame(maxWidth: ctx.compact ? .infinity : nil, maxHeight: ctx.compact ? nil : .infinity, alignment: .topLeading)
            .background(MGWeb.bgSunken)
            Rectangle().fill(MGWeb.border).frame(width: ctx.compact ? nil : 1, height: ctx.compact ? 1 : nil)
                .accessibilityHidden(true)
            code
        }
        .frame(minHeight: 320, alignment: .top)
        .clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
    }

    @ViewBuilder private var code: some View {
        if editing {
            TextEditor(text: $model.code)
                .font(MGWebFont.mono(12, .caption))
                .foregroundStyle(MGWeb.grey300)
                .scrollContentBackground(.hidden)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, minHeight: 320)
                .background(MGWeb.bgSunken)
                .accessibilityLabel(Text("Transform code"))
                .accessibilityIdentifier("mg.forge.ide.editor")
        } else {
            let lines = model.code.components(separatedBy: "\n")
            ScrollView(.horizontal) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(lines.enumerated()), id: \.offset) { i, line in
                        HStack(alignment: .firstTextBaseline, spacing: 14) {
                            Text("\(i + 1)").foregroundStyle(MGWeb.textFaint).frame(width: 18, alignment: .trailing)
                                .accessibilityHidden(true)
                            Text(line.isEmpty ? " " : line)
                                .foregroundStyle(line.trimmingCharacters(in: .whitespaces).hasPrefix("#") || line.contains("# bounds")
                                                 ? MGWeb.textFaint : line.hasPrefix("@") ? ctx.accent.text : MGWeb.grey300)
                                .fixedSize()
                        }
                        .font(MGWebFont.mono(12, .caption))
                        .padding(.vertical, 3)
                    }
                    if model.state == .loading { WebMicro("LOADING TRANSFORM…") }
                }
                .padding(.horizontal, 18)
                .padding(.vertical, 14)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .background(MGWeb.bgSunken)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text("Transform code, \(lines.count) lines: \(model.code)"))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.forge.ide.code")
        }
    }

    @ViewBuilder private var aside: some View {
        if let t = model.transform {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Image(systemName: WebIcon.symbol("code")).font(MGWebFont.sans(16)).foregroundStyle(ctx.accent.text)
                    .accessibilityHidden(true)
                Text("Same contracts as the builder")
                    .font(MGWebFont.sans(15).weight(.semibold))
                    .foregroundStyle(MGWeb.textHeading)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                    .accessibilityIdentifier("mg.forge.inspector.title")
                Spacer(minLength: 0)
            }
            Text("Pro-code transforms compile to the same typed execution plan as low-code graphs; a dataset can mix both in one pipeline. Type errors surface before run, not after.")
                .font(MGWebFont.sans(12, .caption))
                .foregroundStyle(MGWeb.textMuted)
                .lineSpacing(3)
                .fixedSize(horizontal: false, vertical: true)
            ForgeRules(title: "CONTRACT CHECK", rows: t.contractChecks.map { ($0.key, $0.value) }, keyWidth: 100,
                       monoValues: false)
            if let diff = model.diff {
                VStack(alignment: .leading, spacing: 6) {
                    Text("PREVIEW · SANDBOXED")
                        .font(MGWebFont.mono(10))
                        .mgTracking(0.12, size: MGWebFont.monoSize(10))
                        .foregroundStyle(MGWeb.secureText)
                    Text("rows in \(diff.rowsIn.formatted()) → out \(diff.rowsOut.formatted())\nexpectations \(diff.expectations) ok · \(diff.durationLabel)\nnothing committed")
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(MGWeb.textBody)
                        .lineSpacing(5)
                        .fixedSize(horizontal: false, vertical: true)
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(diff.hunks.enumerated()), id: \.offset) { _, h in
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                Text(h.op)
                                Text(h.text).fixedSize(horizontal: false, vertical: true)
                            }
                            .font(MGWebFont.mono(11))
                            .foregroundStyle(hunkColour(h.op))
                        }
                    }
                    .padding(.top, 6)
                }
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.secure))
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("mg.forge.ide.diff")
            }
            if let gate = model.gate {
                WebHumanGate(title: "Commit \(t.nextVersion) · fuse_positions",
                             detail: "STATE MUTATION · PREVIEW APPROVED · HUMAN GATE",
                             state: {
                                 switch gate {
                                 case .pending: return .pending
                                 case .queued: return .queued
                                 case let .authorised(id): return .authorised(auditId: id)
                                 case .rejected: return .rejected
                                 }
                             }(),
                             accent: ctx.accent, busy: model.busy,
                             onAuthorise: { Task { await model.authorise() } },
                             onReject: { model.reject() },
                             idPrefix: "mg.forge.ide")
            }
            if let message = model.message {
                Text(message)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.warnText)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mg.forge.ide.message")
            }
        } else if model.state != .loading {
            Text(model.withheld > 0 ? "The transform is above your clearance." : "No transform is loaded.")
                .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
        }
    }
}

import MidgardCore
import SwiftUI

// Heimdall's Operating Picture, matched to the web console's 2D reference layout
// (`products/heimdall/ui/src/screens/OperatingPictureScreen.tsx` at the signed tag):
// the picture with its range rings and tracks, the inspector with the ranked options
// and the human gate, and the stats footer. The apps place these in their own layout:
// three columns on the Mac, the picture first and the inspector as a sheet on the
// iPhone. [FR-H-4] [FR-H-5] [DD-7] [RC-49] [AC-C-11]

/// The header under the classification banner (`ConsoleShell`'s top bar): the product
/// mark, the context line, the seals and whatever the app puts at the end.
public struct HeimdallHeader<Trailing: View>: View {
    let compact: Bool
    let trailing: Trailing
    private let accent = MGWebAccent.heimdall

    public init(compact: Bool = false, @ViewBuilder trailing: () -> Trailing) {
        self.compact = compact
        self.trailing = trailing()
    }

    public var body: some View {
        HStack(spacing: compact ? 8 : 14) {
            WebProductMark(name: "Heimdall", accent: accent)
            if !compact {
                Text(HeimdallChrome.context)
                    .font(MGWebFont.mono(11))
                    .mgTracking(0.14, size: 11)
                    .foregroundStyle(MGWeb.textFaint)
                    .lineLimit(1)
                    .layoutPriority(-1)
            }
            Spacer(minLength: 8)
            ForEach(Array((compact ? Array(HeimdallChrome.seals.prefix(1)) : HeimdallChrome.seals).enumerated()),
                    id: \.offset) { _, seal in
                WebStatusPill(seal.0, seal.1)
            }
            trailing
        }
        .padding(.horizontal, compact ? 10 : 16)
        .padding(.vertical, 6)
        .frame(minHeight: 52)
        .background(MGWeb.bgSunken)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.border).frame(height: 1) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Heimdall header"))
    }
}

/// The LAYERS switches under the navigation (the screen's `navFooter`).
public struct HeimdallLayersList: View {
    @ObservedObject var model: HeimdallPictureModel

    public init(model: HeimdallPictureModel) { self.model = model }

    public var body: some View {
        VStack(alignment: .leading, spacing: 9) {
            Text("LAYERS")
                .font(MGWebFont.mono(11))
                .mgTracking(0.14, size: 11)
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityAddTraits(.isHeader)
            ForEach(model.layers, id: \.id) { layer in
                WebSwitch(layer.label,
                          isOn: Binding(get: { model.isOn(layer.id) }, set: { model.setLayer(layer.id, $0) }),
                          accent: .heimdall, identifier: "mg.heimdall.layer.\(layer.id)")
            }
        }
        .padding(.horizontal, 10)
        .padding(.top, 10)
        .padding(.bottom, 4)
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

/// The picture: grid, the accent glow, range rings, the north arrow and scale, the
/// focus readout, the feed coverage (when its layer is on), the tracks and, on the Mac,
/// the hover card.
public struct HeimdallPictureCanvas: View {
    @ObservedObject var model: HeimdallPictureModel
    let showsFooter: Bool
    let onSelect: (String) -> Void
    private let accent = MGWebAccent.heimdall

    public init(model: HeimdallPictureModel, showsFooter: Bool = true, onSelect: @escaping (String) -> Void = { _ in }) {
        self.model = model
        self.showsFooter = showsFooter
        self.onSelect = onSelect
    }

    public var body: some View {
        GeometryReader { geo in
            let size = geo.size
            ZStack(alignment: .topLeading) {
                background(size)
                rings(size)
                readout
                if let focus = model.selectedTrack { focusReadout(focus).frame(width: size.width, alignment: .topTrailing) }
                if model.isOn("feeds") { coverage(size) }
                ForEach(model.visibleTracks, id: \.id) { track in
                    trackButton(track)
                        .position(x: size.width * track.x / 100, y: size.height * track.y / 100)
                }
                // In an overlay, so the card never changes the picture's layout: a card
                // that moved the tracks would move them out from under the pointer.
                Color.clear
                    .frame(width: size.width, height: size.height)
                    .allowsHitTesting(false)
                    .overlay(alignment: .topLeading) {
                        if let id = model.hovered, let t = model.visibleTracks.first(where: { $0.id == id }) {
                            hoverCard(t, in: size)
                        }
                    }
                if showsFooter {
                    HeimdallStatsFooter()
                        .padding(.horizontal, 12)
                        .padding(.bottom, 10)
                        .frame(width: size.width, height: size.height, alignment: .bottomLeading)
                }
            }
            .frame(width: size.width, height: size.height)
        }
        .clipped()
        .background(MGWeb.bgPage)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(pictureLabel))
        .accessibilityIdentifier("mg.heimdall.picture")
    }

    private func background(_ size: CGSize) -> some View {
        ZStack {
            Canvas { context, size in
                var path = Path()
                var x: CGFloat = 0
                while x <= size.width {
                    path.addRect(CGRect(x: x, y: 0, width: 1, height: size.height))
                    x += 48
                }
                var y: CGFloat = 0
                while y <= size.height {
                    path.addRect(CGRect(x: 0, y: y, width: size.width, height: 1))
                    y += 48
                }
                context.fill(path, with: .color(MGWeb.gridLine))
            }
            RadialGradient(colors: [accent.tint, .clear],
                           center: UnitPoint(x: 0.55, y: 0.45), startRadius: 0,
                           endRadius: 0.6 * hypot(size.width, size.height) / 2)
        }
        .frame(width: size.width, height: size.height)
        .accessibilityHidden(true)
    }

    private func rings(_ size: CGSize) -> some View {
        ZStack {
            ForEach([(160.0, "20 NM"), (320.0, "40 NM"), (480.0, "60 NM")], id: \.1) { r, label in
                Circle()
                    .strokeBorder(MGWeb.divider, lineWidth: 1)
                    .frame(width: r * 2, height: r * 2)
                    .position(x: size.width / 2, y: size.height / 2)
                Text(label)
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: 8)
                    .foregroundStyle(MGWeb.textFaint)
                    .padding(.horizontal, 5)
                    .background(MGWeb.bgSunken)
                    .position(x: size.width / 2, y: size.height / 2 - r)
            }
        }
        .frame(width: size.width, height: size.height)
        .accessibilityHidden(true)
    }

    private var readout: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("N ↑").accessibilityHidden(true)
                .font(MGWebFont.mono(11))
                .mgTracking(0.14, size: 11)
                .foregroundStyle(MGWeb.textMuted)
                .readoutPlate()
            HStack(spacing: 6) {
                ZStack {
                    Rectangle().fill(MGWeb.decorFaint).frame(width: 80, height: 2)
                    HStack {
                        Rectangle().fill(MGWeb.decorFaint).frame(width: 2, height: 6)
                        Spacer()
                        Rectangle().fill(MGWeb.decorFaint).frame(width: 2, height: 6)
                    }
                    .frame(width: 80)
                }
                .accessibilityHidden(true)
                Text("10 NM").accessibilityHidden(true)
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: 8)
                    .foregroundStyle(MGWeb.textFaint)
                .readoutPlate()
            }
            Text("VECTOR · 1 MIN PROJECTION").accessibilityHidden(true)
                .font(MGWebFont.mono(8))
                .mgTracking(0.12, size: 8)
                .foregroundStyle(MGWeb.textFaint)
                .readoutPlate()
            if model.withheld > 0 {
                Text("\(model.withheld) TRACK\(model.withheld == 1 ? "" : "S") WITHHELD · ABOVE YOUR CLEARANCE")
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: 8)
                    .foregroundStyle(MGWeb.warnText)
                .readoutPlate()
                    .accessibilityIdentifier("mg.heimdall.withheld")
            }
        }
        .padding(.top, 12)
        .padding(.leading, 14)
    }

    /// What the readouts say, for VoiceOver: the picture's own label carries it, and the
    /// readouts themselves (7–9 pt mono, as the web draws them, below what the contrast
    /// audit can read) are decoration. The focused track's details are also the
    /// inspector's.
    private var pictureLabel: String {
        var text = "Operating picture, \(model.visibleTracks.count) tracks shown. North up, scale 10 nautical miles, "
            + "vectors show a one minute projection."
        if let t = model.selectedTrack {
            text += " Focus \(t.id): heading \(Int(t.hdg)) degrees, \(Int(t.spd)) knots, \(t.alt), "
                + "\(HeimdallPictureModel.range(t)) nautical miles, \(Int(t.feeds)) feeds, \(t.track)."
        }
        return text
    }

    private func focusReadout(_ t: HeimdallTrack) -> some View {
        VStack(alignment: .trailing, spacing: 3) {
            Text("FOCUS · \(t.id)")
                .font(MGWebFont.mono(8))
                .mgTracking(0.14, size: 8)
                .foregroundStyle(accent.text)
                .readoutPlate()
            Text("\(HeimdallPictureModel.heading(t.hdg))° · \(Int(t.spd)) KT · \(t.alt)")
                .font(MGWebFont.mono(9))
                .foregroundStyle(MGWeb.textMuted)
                .readoutPlate()
            Text("\(HeimdallPictureModel.range(t)) NM · \(Int(t.feeds)) FEEDS · \(t.track.uppercased())")
                .font(MGWebFont.mono(9))
                .foregroundStyle(MGWeb.textFaint)
                .readoutPlate()
        }
        .multilineTextAlignment(.trailing)
        .padding(.top, 12)
        .padding(.trailing, 14)
        .accessibilityHidden(true)
    }

    private func coverage(_ size: CGSize) -> some View {
        ZStack(alignment: .topLeading) {
            Ellipse()
                .stroke(accent.glow, style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
                .frame(width: size.width * 0.55, height: size.height * 0.62)
                .offset(x: size.width * 0.12, y: size.height * 0.08)
                .accessibilityHidden(true)
            Text("NORTH-2 · RADAR · 4 HZ · R 60 NM")
                .font(MGWebFont.mono(8))
                .mgTracking(0.12, size: 8)
                .foregroundStyle(accent.text)
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
                .background(MGWeb.bgSunken)
                .position(x: size.width * 0.39, y: size.height * 0.08)
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
    }

    private func trackButton(_ t: HeimdallTrack) -> some View {
        let anomalous = t.track == "anomalous"
        let selected = t.id == model.selected
        return Button {
            model.select(t.id)
            onSelect(t.id)
        } label: {
            VStack(spacing: 5) {
                TrackGlyph(track: t, selected: selected, accent: accent)
                Text("\(t.id)\(anomalous ? " ⚠\u{FE0E}" : "") · \(Int(t.hdg))°")
                    .font(MGWebFont.mono(9))
                    .mgTracking(0.08, size: 9)
                    .foregroundStyle(selected ? MGWeb.grey100 : MGWeb.textFaint)
                    .lineLimit(1)
                    .padding(.horizontal, 2)
                    .background(MGWeb.bgPage.opacity(0.55))
            }
            .frame(minWidth: 44, minHeight: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { inside in
            if inside { model.hovered = t.id } else if model.hovered == t.id { model.hovered = nil }
        }
        .accessibilityLabel(Text("\(t.id) · \(t.cls) · heading \(Int(t.hdg)) degrees\(anomalous ? ", anomalous" : "")"))
        .accessibilityAddTraits(selected ? [.isSelected] : [])
        .accessibilityIdentifier("mg.heimdall.track.\(t.id)")
    }

    private func hoverCard(_ t: HeimdallTrack, in size: CGSize) -> some View {
        let rows: [(String, String)] = [
            ("HDG / SPD", "\(HeimdallPictureModel.heading(t.hdg))° · \(Int(t.spd)) KT"),
            ("ALT", t.alt),
            ("RNG", "\(HeimdallPictureModel.range(t)) NM FROM SITE"),
            ("CAP", HeimdallPictureModel.capability[t.cls] ?? "—"),
            ("TRACK", "\(t.track.uppercased()) · \(Int(t.feeds)) FEEDS"),
        ]
        let x = min(max(size.width * t.x / 100, 110), size.width - 110)
        let y = size.height * t.y / 100
        let flip = t.y < 36
        return VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(t.id).font(MGWebFont.mono(11).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                Text(t.cls).font(MGWebFont.mono(9)).mgTracking(0.1, size: 9).foregroundStyle(MGWeb.textFaint)
                Spacer(minLength: 8)
                Text("\(Int((t.conf * 100).rounded()))%")
                    .font(MGWebFont.mono(10))
                    .foregroundStyle(t.conf < 0.7 ? MGWeb.warnText : MGWeb.secureText)
            }
            .padding(.bottom, 6)
            ForEach(rows, id: \.0) { k, v in
                HStack(spacing: 10) {
                    Text(k).mgTracking(0.08, size: 9).foregroundStyle(MGWeb.textFaint).frame(width: 62, alignment: .leading)
                    Text(v).foregroundStyle(MGWeb.textBody)
                }
                .font(MGWebFont.mono(9))
                .padding(.vertical, 2)
            }
            if t.id == model.selected {
                Text("FOCUSED · OPTIONS IN INSPECTOR →")
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.14, size: 8)
                    .foregroundStyle(accent.text)
                    .padding(.top, 5)
            }
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 9)
        .frame(minWidth: 200, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm)
            .strokeBorder(t.track == "anomalous" ? MGWeb.warn : MGWeb.borderStrong))
        .shadow(color: .black.opacity(0.45), radius: 10, y: 6)
        .alignmentGuide(.leading) { d in d.width / 2 - x }
        .alignmentGuide(.top) { d in flip ? -(y + 30) : d.height - y + 26 }
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

/// One track's symbol, as the web draws it: three fading history dots behind it, the
/// velocity vector along the heading (12 pt plus speed / 14, at most 46), the dot (a
/// rounded square at sea), and the selected track's ring and glow. Anomalous tracks in
/// the warning tone, the rest in the accent.
struct TrackGlyph: View {
    let track: HeimdallTrack
    let selected: Bool
    let accent: MGWebAccent

    var body: some View {
        let anomalous = track.track == "anomalous"
        let color = anomalous ? MGWeb.warn : accent.fill
        let rad = track.hdg * .pi / 180
        let vlen = min(46, 12 + track.spd / 14)
        let corner: CGFloat = track.layer == "maritime" ? 2 : 7
        ZStack {
            ForEach(1...3, id: \.self) { k in
                let d = 4 - Double(k) * 0.7
                Circle()
                    .fill(color.opacity(0.45 - Double(k) * 0.11))
                    .frame(width: d, height: d)
                    .offset(x: -sin(rad) * Double(k) * 10, y: cos(rad) * Double(k) * 10)
            }
            LinearGradient(colors: [color, color.opacity(0)], startPoint: .bottom, endPoint: .top)
                .frame(width: 2, height: vlen)
                .opacity(0.9)
                .offset(y: -vlen / 2)
                .rotationEffect(.degrees(track.hdg), anchor: .center)
            RoundedRectangle(cornerRadius: corner)
                .fill(color)
                .frame(width: 14, height: 14)
                .shadow(color: selected ? (anomalous ? MGWeb.warn : accent.glow) : .clear, radius: selected ? 9 : 0)
                .background {
                    if selected {
                        RoundedRectangle(cornerRadius: corner + 5)
                            .fill(anomalous ? MGWeb.warnTint : accent.tint)
                            .frame(width: 24, height: 24)
                    }
                }
                .overlay {
                    if selected {
                        RoundedRectangle(cornerRadius: corner + 3).strokeBorder(color, lineWidth: 1).frame(width: 22, height: 22)
                    }
                }
        }
        .frame(width: 14, height: 14)
        .accessibilityHidden(true)
    }
}

/// The stats footer over the picture: tracked entities, fusion latency, anomalies, and
/// the web's honesty note.
public struct HeimdallStatsFooter: View {
    @Environment(\.dynamicTypeSize) private var typeSize
    private let accent = MGWebAccent.heimdall

    public init() {}

    public var body: some View {
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(alignment: .bottom, spacing: 18))
        layout {
            WebStatBlock(value: "1,204", label: "TRACKED ENTITIES")
            WebStatBlock(value: "0.4s", label: "FUSION LATENCY")
            WebStatBlock(value: "1", label: "ANOMALIES", accent: accent)
            Text("SYNTHETIC PICTURE · REFERENCE LAYOUT ONLY")
                .font(MGWebFont.mono(9))
                .mgTracking(0.14, size: 9)
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityIdentifier("mg.heimdall.honesty")
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.bgSunken))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.divider))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Picture statistics"))
        .accessibilityIdentifier("mg.heimdall.stats")
    }
}

/// The inspector (the screen's `aside`): the selected track's header and marking, its
/// confidence and state, the ranked options and the human gate, and the provenance.
public struct HeimdallInspector: View {
    @ObservedObject var model: HeimdallPictureModel
    private let accent = MGWebAccent.heimdall

    public init(model: HeimdallPictureModel) { self.model = model }

    public var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let t = model.selectedTrack {
                header(t)
                WebCard {
                    HStack(alignment: .top, spacing: 12) {
                        WebStatBlock(value: "\(Int((t.conf * 100).rounded()))%", label: "CLASSIFICATION CONF.",
                                     accent: t.conf < 0.7 ? accent : nil)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        WebStatBlock(value: t.track == "anomalous" ? "ANOM" : "STABLE", label: "TRACK STATE")
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                }
                if !model.options.isEmpty {
                    VStack(alignment: .leading, spacing: 10) {
                        WebSectionLabel(number: 5, "RANKED OPTIONS · ADVISORY", accent: accent)
                        ForEach(model.options, id: \.rank) { option in optionCard(option) }
                    }
                    gate(t)
                }
                if let message = model.message {
                    Text(message)
                        .font(MGWebFont.sans(12, .caption))
                        .foregroundStyle(MGWeb.warnText)
                        .accessibilityIdentifier("mg.heimdall.message")
                }
                Spacer(minLength: 8)
                HStack(spacing: 6) {
                    WebProvenanceTag("FR-H-4", title: "AI-assisted, explainable, cited ranked options, advisory")
                    WebProvenanceTag("FR-H-5", title: "Human-gated action workflows with full audit lineage")
                    WebProvenanceTag("NFR-AI-1", title: "Every agentic action traceable, reversible or human-gated")
                }
            } else {
                Text(model.loaded ? "No track you may see is selected." : "Loading the picture…")
                    .font(MGWebFont.sans(14))
                    .foregroundStyle(MGWeb.textMuted)
            }
        }
        .padding(18)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(MGWeb.bgSunken)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Inspector"))
    }

    private func header(_ t: HeimdallTrack) -> some View {
        let anomalous = t.track == "anomalous"
        return ViewThatFits(in: .horizontal) {
            HStack(spacing: 10) {
                headerTitle(t, anomalous: anomalous)
                Spacer(minLength: 8)
                if let chip = model.markings[t.id] { WebMarkingChip(chip) }
            }
            VStack(alignment: .leading, spacing: 8) {
                headerTitle(t, anomalous: anomalous)
                if let chip = model.markings[t.id] {
                    WebMarkingChip(chip).frame(maxWidth: .infinity, alignment: .trailing)
                }
            }
        }
    }

    private func headerTitle(_ t: HeimdallTrack, anomalous: Bool) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "scope")
                .font(MGWebFont.sans(17))
                .foregroundStyle(anomalous ? MGWeb.warn : accent.fill)
                .accessibilityHidden(true)
            Text(t.id)
                .font(MGWebFont.display(20, .title3).weight(.bold))
                .foregroundStyle(MGWeb.textHeading)
                .accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("mg.heimdall.inspector.track")
            WebBadge(t.cls, tone: anomalous ? .warn : .neutral)
        }
    }

    private func optionCard(_ o: HeimdallOption) -> some View {
        let rank = Int(o.rank)
        let chosen = model.chosen == rank
        return Button {
            model.choose(rank)
        } label: {
            WebCard(borderColor: chosen ? accent.fill : MGWeb.border) {
                HStack(alignment: .firstTextBaseline, spacing: 9) {
                    Text(String(format: "%02d", rank))
                        .font(MGWebFont.display(15, .subheadline).weight(.heavy))
                        .foregroundStyle(chosen ? accent.text : MGWeb.textFaint)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(o.title)
                            .font(MGWebFont.sans(14).weight(.medium))
                            .foregroundStyle(MGWeb.grey100)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                        Text(o.why)
                            .font(MGWebFont.sans(12, .caption))
                            .foregroundStyle(MGWeb.textMuted)
                            .lineSpacing(3)
                            .multilineTextAlignment(.leading)
                            .fixedSize(horizontal: false, vertical: true)
                        FlowRow(spacing: 5) {
                            ForEach(o.cites, id: \.self) { cite in
                                Text(cite)
                                    .font(MGWebFont.mono(9))
                                    .foregroundStyle(accent.text)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 1)
                                    .background(RoundedRectangle(cornerRadius: MGRadius.xs).fill(MGWeb.surfaceInset))
                                    .overlay(RoundedRectangle(cornerRadius: MGRadius.xs).strokeBorder(MGWeb.border))
                            }
                        }
                        .padding(.top, 3)
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(chosen ? [.isSelected] : [])
        .accessibilityHint(Text("Chooses this option for the human gate"))
        .accessibilityIdentifier("mg.heimdall.option.\(rank)")
    }

    @ViewBuilder private func gate(_ t: HeimdallTrack) -> some View {
        if HeimdallPictureModel.workflowForRank[model.chosen] != nil {
            WebHumanGate(
                title: "Authorise option \(String(format: "%02d", model.chosen)) · \(t.id)",
                detail: "HUMAN AUTHORISATION REQUIRED · AI IS ADVISORY",
                state: {
                    switch model.gate {
                    case .pending: return .pending
                    case .queued: return .queued
                    case let .authorised(id): return .authorised(auditId: id)
                    case .rejected: return .rejected
                    }
                }(),
                accent: accent, busy: model.busy,
                onAuthorise: { Task { await model.authorise() } },
                onReject: { model.reject() })
        } else {
            Text("HOLD · NO ACTION · PASSIVE FUSION CONTINUES")
                .font(MGWebFont.mono(9))
                .mgTracking(0.12, size: 9)
                .foregroundStyle(MGWeb.textFaint)
                .padding(.horizontal, 14)
                .padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceInset))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
                .accessibilityIdentifier("mg.heimdall.hold")
        }
    }
}

/// Lays its children out left to right, wrapping to a new line when the width runs
/// out (the web's `flex-wrap: wrap`).
struct FlowRow: Layout {
    var spacing: CGFloat = 5

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? .infinity
        var x: CGFloat = 0, y: CGFloat = 0, line: CGFloat = 0, widest: CGFloat = 0
        for s in subviews {
            let size = s.sizeThatFits(.unspecified)
            if x > 0 && x + size.width > width {
                y += line + spacing
                x = 0
                line = 0
            }
            x += size.width + spacing
            line = max(line, size.height)
            widest = max(widest, x - spacing)
        }
        return CGSize(width: min(widest, width), height: y + line)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        var x = bounds.minX, y = bounds.minY, line: CGFloat = 0
        for s in subviews {
            let size = s.sizeThatFits(.unspecified)
            if x > bounds.minX && x + size.width > bounds.maxX {
                y += line + spacing
                x = bounds.minX
                line = 0
            }
            s.place(at: CGPoint(x: x, y: y), proposal: ProposedViewSize(size))
            x += size.width + spacing
            line = max(line, size.height)
        }
    }
}

extension View {
    /// A readout over the picture on its own sunken plate, as the web's ring labels
    /// are: over the grid and the accent glow, small mono text failed the contrast
    /// audit.
    func readoutPlate() -> some View {
        padding(.horizontal, 3).background(MGWeb.bgSunken)
    }
}

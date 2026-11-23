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
    @Environment(\.dynamicTypeSize) private var typeSize
    let compact: Bool
    let context: String
    let trailing: Trailing
    private let accent = MGWebAccent.heimdall

    public init(compact: Bool = false, context: String = HeimdallChrome.context, @ViewBuilder trailing: () -> Trailing) {
        self.compact = compact
        self.context = context
        self.trailing = trailing()
    }

    public var body: some View {
        (typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(spacing: compact ? 8 : 14))) {
            WebProductMark(name: "Heimdall", accent: accent)
            if !compact {
                Text(context)
                    .font(MGWebFont.mono(11))
                    .mgTracking(0.14, size: MGWebFont.monoSize(11))
                    .foregroundStyle(MGWeb.textFaint)
                    .lineLimit(1)
                    .layoutPriority(-1)
            }
            if !typeSize.isAccessibilitySize { Spacer(minLength: 8) }
            ForEach(Array((compact ? Array(HeimdallChrome.seals.prefix(1)) : HeimdallChrome.seals).enumerated()),
                    id: \.offset) { _, seal in
                WebStatusPill(seal.0, seal.1)
            }
            trailing
        }
        .padding(.horizontal, compact ? 10 : 16)
        .padding(.vertical, 6)
        .frame(maxWidth: .infinity, minHeight: 52, alignment: .leading)
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
                .mgTracking(0.14, size: MGWebFont.monoSize(11))
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityAddTraits(.isHeader)
            ForEach(model.layers, id: \.id) { layer in
                WebSwitch(layer.label,
                          isOn: Binding(get: { model.isOn(layer.id) }, set: { model.setLayer(layer.id, $0) }),
                          accent: .heimdall, identifier: "mg.heimdall.layer.\(layer.id)")
            }
            if !model.userLayers.isEmpty {
                Text("YOUR LAYERS · NOT FUSED")
                    .font(MGWebFont.mono(11))
                    .mgTracking(0.14, size: MGWebFont.monoSize(11))
                    .foregroundStyle(MGWeb.textFaint)
                    .accessibilityAddTraits(.isHeader)
                    .padding(.top, 4)
                ForEach(model.userLayers) { layer in
                    WebSwitch("\(layer.label) · \(layer.points.count)/\(layer.total)",
                              isOn: Binding(get: { model.userLayerOn[layer.id] != false },
                                            set: { model.setUserLayer(layer.id, $0) }),
                              accent: .heimdall, identifier: "mg.heimdall.userlayer.\(layer.name)")
                }
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
/// the hover card. With `readoutsInPicture` false (the iPhone) the readouts are not
/// drawn over the picture: the app shows them as `HeimdallPictureReadoutStrip` beside it.
public struct HeimdallPictureCanvas: View {
    @ObservedObject var model: HeimdallPictureModel
    let showsFooter: Bool
    let readoutsInPicture: Bool
    let onSelect: (String) -> Void
    @Environment(\.dynamicTypeSize) private var typeSize
    /// Each track label's drawn size, as laid out: the track's button reaches over it.
    @State private var labelSizes: [String: CGSize] = [:]
    private let accent = MGWebAccent.heimdall

    /// At accessibility text sizes the picture draws no text of its own (it would cover
    /// the picture and be cut by it): the symbols stay, and the app lists the tracks
    /// under the picture (`HeimdallTrackList`).
    private var large: Bool { typeSize.isAccessibilitySize }

    public init(model: HeimdallPictureModel, showsFooter: Bool = true, readoutsInPicture: Bool = true,
                onSelect: @escaping (String) -> Void = { _ in }) {
        self.model = model
        self.showsFooter = showsFooter
        self.readoutsInPicture = readoutsInPicture
        self.onSelect = onSelect
    }

    public var body: some View {
        GeometryReader { geo in
            let size = geo.size
            ZStack(alignment: .topLeading) {
                background(size)
                rings(size)
                if !large && readoutsInPicture {
                    // Side by side where they fit (the Mac); where they would overlap the
                    // focus readout starts under the left one.
                    PictureReadoutsLayout(spacing: 6) {
                        readout
                        if let focus = model.selectedTrack { focusReadout(focus) }
                    }
                    .padding(.top, 12)
                    .frame(width: size.width, alignment: .topLeading)
                }
                if model.isOn("feeds") { coverage(size) }
                if !model.visibleUserLayers.isEmpty { HeimdallUserPoints(layers: model.visibleUserLayers, size: size) }
                ForEach(model.visibleTracks, id: \.id) { track in
                    let at = HeimdallPictureGeometry.point(x: track.x, y: track.y, in: size)
                    let label = large ? nil : labelSizes[track.id]
                    let frame = HeimdallPictureGeometry.trackFrame(at: at, label: label, in: size)
                    trackButton(track, symbolAt: CGPoint(x: at.x - frame.minX, y: at.y - frame.minY), size: frame.size)
                        .position(x: frame.midX, y: frame.midY)
                    if !large { trackLabel(track, at: at, in: size) }
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

    /// Where a ring's label sits: at its top, as the web draws it; on the iPhone at its
    /// east side, where no track's label falls on it. A label outside the picture is not
    /// drawn.
    private func ringLabelPoint(_ r: CGFloat, in size: CGSize) -> CGPoint {
        #if os(iOS)
        return CGPoint(x: size.width / 2 + r, y: size.height / 2)
        #else
        return CGPoint(x: size.width / 2, y: size.height / 2 - r)
        #endif
    }

    private func rings(_ size: CGSize) -> some View {
        ZStack {
            ForEach([(160.0, "20 NM"), (320.0, "40 NM"), (480.0, "60 NM")], id: \.1) { r, label in
                Circle()
                    .strokeBorder(MGWeb.divider, lineWidth: 1)
                    .frame(width: r * 2, height: r * 2)
                    .position(x: size.width / 2, y: size.height / 2)
                    .accessibilityHidden(true)
                let at = ringLabelPoint(r, in: size)
                if !large, at.x < size.width - 24, at.y > 12 {
                    // Kept inside the picture: at an iPhone SE's width and large text the
                    // 20 NM label centred on the ring ran past the right edge.
                    PinnedLabelLayout(point: at, centredVertically: true) {
                        Text(label)
                            .font(MGWebFont.mono(8))
                            .mgTracking(0.12, size: MGWebFont.monoSize(8))
                            .foregroundStyle(MGWeb.textFaint)
                            .lineLimit(1)
                            .fixedSize()
                            .padding(.horizontal, 5)
                            .background(MGWeb.bgSunken)
                            .accessibilityHidden(MGWeb.readoutsDecorative)
                    }
                    .frame(width: size.width, height: size.height)
                    .allowsHitTesting(false)
                }
            }
        }
        .frame(width: size.width, height: size.height)
    }

    private var readout: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("N ↑").accessibilityHidden(MGWeb.readoutsDecorative)
                .accessibilityLabel(Text("North up"))
                .font(MGWebFont.mono(11))
                .mgTracking(0.14, size: MGWebFont.monoSize(11))
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
                Text("10 NM").accessibilityHidden(MGWeb.readoutsDecorative)
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: MGWebFont.monoSize(8))
                    .foregroundStyle(MGWeb.textFaint)
                .readoutPlate()
            }
            Text("VECTOR · 1 MIN PROJECTION").accessibilityHidden(MGWeb.readoutsDecorative)
                .font(MGWebFont.mono(8))
                .mgTracking(0.12, size: MGWebFont.monoSize(8))
                .foregroundStyle(MGWeb.textFaint)
                .readoutPlate()
            if model.withheld > 0 {
                Text("\(model.withheld) TRACK\(model.withheld == 1 ? "" : "S") WITHHELD · ABOVE YOUR CLEARANCE")
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: MGWebFont.monoSize(8))
                    .foregroundStyle(MGWeb.warnText)
                .readoutPlate()
                    .accessibilityIdentifier("mg.heimdall.withheld")
            }
        }
        .padding(.leading, 14)
    }

    /// What the readouts say, for VoiceOver, where they are not readable text: on the
    /// Mac (7–9 pt mono, as the web draws them, below what the contrast audit can read,
    /// so decoration) and at accessibility text sizes (not drawn). Where they are text
    /// (the iPhone, in the readout strip) the picture's label does not repeat them.
    private var pictureLabel: String {
        var count = "Operating picture, \(model.visibleTracks.count) tracks shown."
        let user = model.visibleUserLayers
        if !user.isEmpty {
            let placed = user.reduce(0) { n, l in
                n + l.points.filter { HeimdallPictureModel.referencePoint(lat: $0.lat, lon: $0.lon) != nil }.count
            }
            count += " \(placed) points of your own layers on this view: \(user.map(\.label).joined(separator: ", "))."
        }
        guard MGWeb.readoutsDecorative || large else { return count }
        var text = count + " North up, scale 10 nautical miles, vectors show a one minute projection."
        if let t = model.selectedTrack { text += " " + HeimdallPictureReadoutStrip.focusSentence(t) }
        return text
    }

    private func focusReadout(_ t: HeimdallTrack) -> some View {
        VStack(alignment: .trailing, spacing: 3) {
            Text("FOCUS · \(t.id)")
                .font(MGWebFont.mono(8))
                .mgTracking(0.14, size: MGWebFont.monoSize(8))
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
        .padding(.trailing, 14)
        .accessibilityHidden(MGWeb.readoutsDecorative)
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
                .mgTracking(0.12, size: MGWebFont.monoSize(8))
                .foregroundStyle(accent.text)
                .padding(.horizontal, 6)
                .padding(.vertical, 1)
                .background(MGWeb.bgSunken)
                .position(x: size.width * 0.39, y: size.height * 0.08)
        }
        .frame(width: size.width, height: size.height, alignment: .topLeading)
    }

    /// A track: one button, 44 pt round its symbol (centred on the track's point) and
    /// reaching over its label (`trackLabel`, drawn apart and not read by VoiceOver), so
    /// the label's text lies inside the element that speaks for it and VoiceOver says
    /// the track once. `symbolAt` is the symbol's centre in the button's `size`.
    private func trackButton(_ t: HeimdallTrack, symbolAt: CGPoint, size: CGSize) -> some View {
        let anomalous = t.track == "anomalous"
        let selected = t.id == model.selected
        return Button {
            model.select(t.id)
            onSelect(t.id)
        } label: {
            TrackGlyph(track: t, selected: selected, accent: accent)
                .frame(width: HeimdallPictureGeometry.hitSize, height: HeimdallPictureGeometry.hitSize)
                .position(symbolAt)
                .frame(width: size.width, height: size.height)
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

    /// A track's label, where the web puts it: 5 pt under the 14 pt dot, centred on it,
    /// and kept inside the picture (on an iPhone SE's width the rightmost labels would
    /// otherwise run past the edge at large text). VoiceOver does not read it: the
    /// track's button, which reaches over it, says the same. It stays in the
    /// accessibility tree as an empty container named `mg.heimdall.tracklabel.<id>`, so
    /// the UI tests can find its frame.
    private func trackLabel(_ t: HeimdallTrack, at point: CGPoint, in size: CGSize) -> some View {
        let anomalous = t.track == "anomalous"
        let selected = t.id == model.selected
        return PinnedLabelLayout(point: CGPoint(x: point.x, y: point.y + HeimdallPictureGeometry.labelGap)) {
            ZStack {
                Text("\(t.id)\(anomalous ? " ⚠\u{FE0E}" : "") · \(Int(t.hdg))°")
                    .font(MGWebFont.mono(9))
                    .mgTracking(0.08, size: MGWebFont.monoSize(9))
                    .foregroundStyle(selected ? MGWeb.grey100 : MGWeb.textFaint)
                    .lineLimit(1)
                    .fixedSize()
                    .padding(.horizontal, 2)
                    .background(MGWeb.bgPage)
                    .onGeometryChange(for: CGSize.self) { $0.size } action: { labelSizes[t.id] = $0 }
                    .accessibilityHidden(true)
            }
            .accessibilityElement(children: .contain)
            .accessibilityIdentifier("mg.heimdall.tracklabel.\(t.id)")
        }
        .frame(width: size.width, height: size.height)
        .allowsHitTesting(false)
    }


    private func hoverCard(_ t: HeimdallTrack, in size: CGSize) -> some View {
        let x = min(max(size.width * t.x / 100, 110), size.width - 110)
        let y = size.height * t.y / 100
        let flip = t.y < 36
        return HeimdallTrackCard(track: t, selected: t.id == model.selected)
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
    let honesty: String

    /// The note under the reference layout; the geographic views say the positions are
    /// not real ones (`notRealPositions`), as the web does.
    public static let referenceOnly = "SYNTHETIC PICTURE · REFERENCE LAYOUT ONLY"
    public static let notRealPositions = "SYNTHETIC PICTURE · REFERENCE LAYOUT · NOT REAL POSITIONS"

    public init(honesty: String = HeimdallStatsFooter.referenceOnly) { self.honesty = honesty }

    public var body: some View {
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
            : AnyLayout(HStackLayout(alignment: .bottom, spacing: 18))
        layout {
            WebStatBlock(value: "1,204", label: "TRACKED ENTITIES")
            WebStatBlock(value: "0.4s", label: "FUSION LATENCY")
            WebStatBlock(value: "1", label: "ANOMALIES", accent: accent)
            Text(honesty)
                .font(MGWebFont.mono(9))
                .mgTracking(0.14, size: MGWebFont.monoSize(9))
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
    @Environment(\.dynamicTypeSize) private var typeSize
    private let accent = MGWebAccent.heimdall

    public init(model: HeimdallPictureModel) { self.model = model }

    public var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            if let t = model.selectedTrack {
                header(t)
                WebCard {
                    (typeSize.isAccessibilitySize
                        ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12))
                        : AnyLayout(HStackLayout(alignment: .top, spacing: 12))) {
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
                FlowRow(spacing: 6) {
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
                .mgTracking(0.12, size: MGWebFont.monoSize(9))
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
            // A point's grace: the bounds can come back a fraction narrower than the
            // width measured (rounding to pixels), and a line that fitted when measured
            // must not wrap when placed, over what follows it.
            if x > bounds.minX && x + size.width > bounds.maxX + 1 {
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

/// The picture's two readout blocks: the first (north, scale, vector) at the top left,
/// the second (the focus track) at the top right, as the web draws them, when both fit
/// across the width with `spacing` between them; otherwise the second starts `spacing`
/// under the first, still at the right, so the two never overlap. [FR-H-4] [DD-7]
struct PictureReadoutsLayout: Layout {
    var spacing: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let sizes = subviews.map { $0.sizeThatFits(.unspecified) }
        let width = proposal.width ?? sizes.map(\.width).reduce(spacing, +)
        let frames = frames(width: width, sizes: sizes)
        return CGSize(width: width, height: frames.map(\.maxY).max() ?? 0)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let frames = frames(width: bounds.width, sizes: subviews.map { $0.sizeThatFits(.unspecified) })
        for (subview, frame) in zip(subviews, frames) {
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                          proposal: ProposedViewSize(frame.size))
        }
    }

    func frames(width: CGFloat, sizes: [CGSize]) -> [CGRect] {
        guard let left = sizes.first else { return [] }
        var frames = [CGRect(origin: .zero, size: left)]
        if sizes.count > 1 {
            let right = CGSize(width: min(sizes[1].width, width), height: sizes[1].height)
            let beside = left.width + spacing + right.width <= width
            frames.append(CGRect(x: width - right.width, y: beside ? 0 : left.height + spacing,
                                 width: right.width, height: right.height))
        }
        return frames
    }
}

/// The tracks as a list, for accessibility text sizes, where the picture draws no
/// labels: each opens its inspector, as its symbol does.
public struct HeimdallTrackList: View {
    @ObservedObject var model: HeimdallPictureModel
    let onSelect: (String) -> Void

    public init(model: HeimdallPictureModel, onSelect: @escaping (String) -> Void) {
        self.model = model
        self.onSelect = onSelect
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("TRACKS")
                .font(MGWebFont.mono(11))
                .mgTracking(0.14, size: MGWebFont.monoSize(11))
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityAddTraits(.isHeader)
            ForEach(model.visibleTracks, id: \.id) { t in
                Button {
                    model.select(t.id)
                    onSelect(t.id)
                } label: {
                    Text("\(t.id) · \(t.cls) · \(Int(t.hdg))°\(t.track == "anomalous" ? " · ANOMALOUS" : "")")
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(t.track == "anomalous" ? MGWeb.warnText : MGWeb.textBody)
                        .multilineTextAlignment(.leading)
                        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("mg.heimdall.list.\(t.id)")
            }
        }
    }
}

/// Where the picture puts a track: its point, its symbol's 44 pt hit frame centred on
/// the point, and its label 5 pt under the 14 pt dot, kept inside the picture. The
/// views and the geometry tests use the same rules. [FR-H-4] [DD-7] [RC-49]
enum HeimdallPictureGeometry {
    /// The symbol's hit frame (the 14 pt dot at its centre).
    static let hitSize: CGFloat = 44
    /// From the dot's centre to its label's top: half the 14 pt dot and the web's 5 pt.
    static let labelGap: CGFloat = 12
    /// How close a label may come to the picture's edge.
    static let edgeInset: CGFloat = 2

    /// A track's point: `x` and `y` in percent of the picture, as the web places it.
    static func point(x: Double, y: Double, in size: CGSize) -> CGPoint {
        CGPoint(x: size.width * x / 100, y: size.height * y / 100)
    }

    static func symbolFrame(at point: CGPoint) -> CGRect {
        CGRect(x: point.x - hitSize / 2, y: point.y - hitSize / 2, width: hitSize, height: hitSize)
    }

    static func labelFrame(at point: CGPoint, label: CGSize, in size: CGSize) -> CGRect {
        pinned(CGPoint(x: point.x, y: point.y + labelGap), size: label, centredVertically: false, in: size)
    }

    /// The track's button: its symbol's hit frame and, once drawn, its label's frame.
    static func trackFrame(at point: CGPoint, label: CGSize?, in size: CGSize) -> CGRect {
        let symbol = symbolFrame(at: point)
        guard let label else { return symbol }
        return symbol.union(labelFrame(at: point, label: label, in: size))
    }

    /// A label of `size` centred across `point` (and on it vertically, or with its top
    /// there), moved the least it takes to keep `edgeInset` from every edge of `bounds`.
    static func pinned(_ point: CGPoint, size: CGSize, centredVertically: Bool, in bounds: CGSize) -> CGRect {
        var x = point.x - size.width / 2
        var y = centredVertically ? point.y - size.height / 2 : point.y
        x = max(edgeInset, min(x, bounds.width - edgeInset - size.width))
        y = max(edgeInset, min(y, bounds.height - edgeInset - size.height))
        return CGRect(x: x, y: y, width: size.width, height: size.height)
    }
}

/// One label over the picture, at its ideal size, placed by
/// `HeimdallPictureGeometry.pinned` in the whole picture's frame.
struct PinnedLabelLayout: Layout {
    var point: CGPoint
    var centredVertically = false

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        proposal.replacingUnspecifiedDimensions()
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        for subview in subviews {
            let size = subview.sizeThatFits(.unspecified)
            let frame = HeimdallPictureGeometry.pinned(point, size: size, centredVertically: centredVertically,
                                                       in: bounds.size)
            subview.place(at: CGPoint(x: bounds.minX + frame.minX, y: bounds.minY + frame.minY),
                          proposal: ProposedViewSize(size))
        }
    }
}

/// The iPhone's picture readouts, as a strip directly above the picture instead of over
/// it: north, scale and vector, the withheld count, and the focus track. Over the
/// picture at an iPhone's width they came over the tracks and their labels (the vector
/// line over E-2231's symbol even at the default text size, the focus lines over
/// E-2231's label), more so with larger text. Readable text, each line its own element;
/// the focus track's lines are one element that says them as a sentence. Not shown at
/// accessibility text sizes, where the picture's label says them. [FR-H-4] [DD-7]
/// [RC-49] [AC-C-11]
public struct HeimdallPictureReadoutStrip: View {
    @ObservedObject var model: HeimdallPictureModel
    private let accent = MGWebAccent.heimdall

    public init(model: HeimdallPictureModel) { self.model = model }

    static func focusSentence(_ t: HeimdallTrack) -> String {
        "Focus \(t.id): heading \(Int(t.hdg)) degrees, \(Int(t.spd)) knots, \(t.alt), "
            + "\(HeimdallPictureModel.range(t)) nautical miles, \(Int(t.feeds)) feeds, \(t.track)."
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            FlowRow(spacing: 12) {
                Text("N ↑")
                    .font(MGWebFont.mono(11))
                    .mgTracking(0.14, size: MGWebFont.monoSize(11))
                    .foregroundStyle(MGWeb.textMuted)
                    .accessibilityLabel(Text("North up"))
                    .accessibilityIdentifier("mg.heimdall.readout.north")
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
                    Text("10 NM")
                        .font(MGWebFont.mono(8))
                        .mgTracking(0.12, size: MGWebFont.monoSize(8))
                        .foregroundStyle(MGWeb.textFaint)
                        .accessibilityLabel(Text("Scale 10 nautical miles"))
                        .accessibilityIdentifier("mg.heimdall.readout.scale")
                }
                Text("VECTOR · 1 MIN PROJECTION")
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: MGWebFont.monoSize(8))
                    .foregroundStyle(MGWeb.textFaint)
                    .accessibilityIdentifier("mg.heimdall.readout.vector")
            }
            if model.withheld > 0 {
                Text("\(model.withheld) TRACK\(model.withheld == 1 ? "" : "S") WITHHELD · ABOVE YOUR CLEARANCE")
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.12, size: MGWebFont.monoSize(8))
                    .foregroundStyle(MGWeb.warnText)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("mg.heimdall.withheld")
            }
            if let t = model.selectedTrack {
                FlowRow(spacing: 12) {
                    Text("FOCUS · \(t.id)")
                        .font(MGWebFont.mono(8))
                        .mgTracking(0.14, size: MGWebFont.monoSize(8))
                        .foregroundStyle(accent.text)
                    Text("\(HeimdallPictureModel.heading(t.hdg))° · \(Int(t.spd)) KT · \(t.alt)")
                        .font(MGWebFont.mono(9))
                        .foregroundStyle(MGWeb.textMuted)
                    Text("\(HeimdallPictureModel.range(t)) NM · \(Int(t.feeds)) FEEDS · \(t.track.uppercased())")
                        .font(MGWebFont.mono(9))
                        .foregroundStyle(MGWeb.textFaint)
                }
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(Self.focusSentence(t)))
                .accessibilityIdentifier("mg.heimdall.readout.focus")
            }
        }
        .lineLimit(1)
        .padding(.horizontal, 12)
        .padding(.vertical, 8)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(MGWeb.bgSunken)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.border).frame(height: 1).accessibilityHidden(true) }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.heimdall.readouts")
    }
}

/// The iPhone's readout strip and picture, stacked: the strip at its height, the picture
/// under it with what is left of `total` (520 pt, the picture's height before the strip),
/// but never less than `minPicture`. The picture's lowest track label then stays about
/// where it was above the console bar. A single subview is the picture alone, at
/// `total`. [FR-H-4] [DD-7] [RC-49]
struct PictureStripLayout: Layout {
    var total: CGFloat = 520
    var minPicture: CGFloat = 440

    func pictureHeight(strip: CGFloat) -> CGFloat { max(minPicture, total - strip) }

    private func heights(width: CGFloat, subviews: Subviews) -> (strip: CGFloat, picture: CGFloat) {
        guard subviews.count > 1 else { return (0, total) }
        let strip = subviews[0].sizeThatFits(ProposedViewSize(width: width, height: nil)).height
        return (strip, pictureHeight(strip: strip))
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = proposal.width ?? 390
        let h = heights(width: width, subviews: subviews)
        return CGSize(width: width, height: h.strip + h.picture)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let h = heights(width: bounds.width, subviews: subviews)
        if subviews.count > 1 {
            subviews[0].place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: h.strip))
        }
        if let picture = subviews.last {
            picture.place(at: CGPoint(x: bounds.minX, y: bounds.minY + h.strip),
                          proposal: ProposedViewSize(width: bounds.width, height: h.picture))
        }
    }
}

/// The user's own layers on the 2D reference layout: an open diamond in the warning tone
/// for each observation of an UNVERIFIED FEED, a small square in the accent for each
/// imported object, at `HeimdallPictureModel.referencePoint`. Drawn, not interactive:
/// the layer list names each layer, and the picture's label counts the points.
struct HeimdallUserPoints: View {
    let layers: [UserMapLayer]
    let size: CGSize

    var body: some View {
        Canvas { ctx, size in
            for layer in layers {
                for p in layer.points {
                    guard let at = HeimdallPictureModel.referencePoint(lat: p.lat, lon: p.lon) else { continue }
                    let c = HeimdallPictureGeometry.point(x: at.x, y: at.y, in: size)
                    if layer.unverified {
                        var d = Path()
                        d.move(to: CGPoint(x: c.x, y: c.y - 5))
                        d.addLine(to: CGPoint(x: c.x + 5, y: c.y))
                        d.addLine(to: CGPoint(x: c.x, y: c.y + 5))
                        d.addLine(to: CGPoint(x: c.x - 5, y: c.y))
                        d.closeSubpath()
                        ctx.stroke(d, with: .color(MGWeb.warn), lineWidth: 1.5)
                    } else {
                        ctx.fill(Path(CGRect(x: c.x - 3.5, y: c.y - 3.5, width: 7, height: 7)),
                                 with: .color(MGWebAccent.heimdall.fill))
                    }
                }
            }
        }
        .frame(width: size.width, height: size.height)
        .allowsHitTesting(false)
        .accessibilityHidden(true)
    }
}

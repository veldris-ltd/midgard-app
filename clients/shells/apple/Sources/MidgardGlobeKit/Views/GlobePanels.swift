import MidgardCore
import MidgardKit
import SwiftUI

/// The tracks over the globe: the altitude stems and ground shadows, the markers (one
/// button each, which selects the track), their labels, the selection ring and, on the
/// Mac, the hovered track's card. The marker is 13 pt as the web's sprite. [FR-H-1]
struct GlobeMarkers: View {
    @ObservedObject var controller: GlobeController
    @ObservedObject var picture: HeimdallPictureModel
    let onSelect: (String) -> Void

    static let markerPx: CGFloat = 13

    var body: some View {
        let markers = controller.markers
        ZStack(alignment: .topLeading) {
            Canvas { ctx, _ in
                for m in markers where m.labelled {
                    let t = track(m.id)
                    let warn = t?.track == "anomalous"
                    let text = "\(m.id)\(warn ? " ⚠\u{FE0E}" : "")\(t.map { " · \(Int($0.hdg))°" } ?? "")"
                    drawTrackLabel(&ctx, text, at: CGPoint(x: m.point.x, y: m.point.y + Self.markerPx / 2 + 3),
                                   selected: m.id == picture.selected)
                }
                for m in markers where m.visible {
                    let tone = Tone.of(m.tone)
                    if let g = m.ground {
                        var stem = Path()
                        stem.move(to: g)
                        stem.addLine(to: m.point)
                        ctx.stroke(stem, with: .color(tone.opacity(0.7)), lineWidth: 1)
                        if m.groundVisible {
                            let d = Self.markerPx * 0.55 * 0.8
                            ctx.fill(Path(ellipseIn: CGRect(x: g.x - d / 2, y: g.y - d / 2, width: d, height: d)),
                                     with: .color(tone.opacity(0.5)))
                        }
                    }
                    if m.id == picture.selected {
                        let r = Self.markerPx * 0.84
                        ctx.stroke(Path(ellipseIn: CGRect(x: m.point.x - r, y: m.point.y - r, width: 2 * r, height: 2 * r)),
                                   with: .color(tone), lineWidth: 2)
                    }
                }
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
            ForEach(markers.filter(\.visible)) { m in
                marker(m)
            }
            if let id = controller.hovered, let m = markers.first(where: { $0.id == id && $0.visible }),
               let t = picture.tracks.first(where: { $0.id == id }) {
                GeometryReader { geo in
                    let below = m.point.y < 150
                    HeimdallTrackCard(track: t, selected: t.id == picture.selected)
                        .fixedSize()
                        .position(x: min(geo.size.width - 110, max(110, m.point.x)), y: m.point.y + (below ? 90 : -90))
                }
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
        }
    }

    private func track(_ id: String) -> HeimdallTrack? { picture.tracks.first { $0.id == id } }

    private func marker(_ m: MarkerScreen) -> some View {
        let t = track(m.id)
        let selected = m.id == picture.selected
        let big: CGFloat = m.id == controller.hovered ? 1.35 : 1
        let hit = max(minTarget, 22)
        return Button {
            onSelect(m.id)
        } label: {
            MarkerShape(layer: m.layer, pointed: m.rotation != nil)
                .fill(Tone.of(m.tone))
                .frame(width: Self.markerPx * big, height: Self.markerPx * big)
                .rotationEffect(.radians(m.rotation ?? 0))
                .frame(width: hit, height: hit)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .position(m.point)
        .onHover { inside in
            if inside { controller.hovered = m.id } else if controller.hovered == m.id { controller.hovered = nil }
        }
        .accessibilityLabel(Text(t.map { TrackRow.label($0, placed: true) } ?? m.id))
        .accessibilityAddTraits(selected ? [.isSelected] : [])
        .accessibilityIdentifier("mg.heimdall.3d.marker.\(m.id)")
    }
}

/// The left panel: the REGION list and the TRACKS list (`.hg-globe-nav`).
struct GlobeNavPanel: View {
    @ObservedObject var c: GlobeController
    @ObservedObject var picture: HeimdallPictureModel
    let regions: [Region]
    let current: Region?
    let flyToRegion: (Region) -> Void
    let chooseTrack: (String) -> Void

    var body: some View {
        Panel {
            VStack(alignment: .leading, spacing: 6) {
                PanelHeading(text: "REGION")
                VStack(spacing: 2) {
                    ForEach(regions) { r in
                        PanelButton(current: current?.id == r.id, identifier: "mg.heimdall.region.\(r.id)",
                                    accessibility: r.label, action: { flyToRegion(r) }) { Text(r.label) }
                    }
                }
            }
            .accessibilityElement(children: .contain)
            .accessibilityLabel(Text("Regions"))
            TrackList(picture: picture, count: c.markers.count, suffix: "ON GLOBE", idPrefix: "mg.heimdall.3d.track",
                      choose: chooseTrack)
        }
    }
}

/// The TRACKS list: each track the layers show, with its altitude; a track with no
/// position is listed and disabled.
struct TrackList: View {
    @ObservedObject var picture: HeimdallPictureModel
    let count: Int
    let suffix: String
    let idPrefix: String
    let choose: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            PanelHeading(text: "TRACKS", trailing: "\(count) \(suffix)")
            ScrollView {
                VStack(spacing: 2) {
                    ForEach(picture.visibleTracks, id: \.id) { t in
                        let placed = SamplePositions.position(t.id) != nil
                        PanelButton(current: t.id == picture.selected, disabled: !placed, identifier: "\(idPrefix).\(t.id)",
                                    accessibility: TrackRow.label(t, placed: placed), action: { choose(t.id) }) {
                            TrackRow(track: t, placed: placed)
                        }
                    }
                }
            }
            .frame(maxHeight: 260)
            .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Tracks"))
    }
}

/// The right panel: the CAMERA readout and controls, STYLE, HEIGHTS and the credits
/// (`.hg-globe-side`).
struct GlobeSidePanel: View {
    @ObservedObject var c: GlobeController
    let current: Region?
    let credits: [Credit]
    let style: GlobeStyle
    let offline: Bool
    @Binding var heights: Double
    let chooseStyle: (GlobeStyle) -> Void
    let reducedMotion: Bool
    let summary: String

    var body: some View {
        Panel {
            VStack(alignment: .leading, spacing: 6) {
                PanelHeading(text: "CAMERA")
                CameraReadout(camera: c.settled, current: current, summary: summary)
                HStack(spacing: 4) {
                    ControlButton(accessibility: "Zoom in", identifier: "mg.heimdall.3d.zoomin", action: { c.nudge(dx: 0, dy: 0, zoom: 0.6) }) {
                        Text("+").font(MGWebFont.mono(13))
                    }
                    ControlButton(accessibility: "Zoom out", identifier: "mg.heimdall.3d.zoomout", action: { c.nudge(dx: 0, dy: 0, zoom: 1 / 0.6) }) {
                        Text("−").font(MGWebFont.mono(13))
                    }
                }
                HStack(spacing: 4) {
                    ControlButton(accessibility: "Tilt towards the horizon", identifier: "mg.heimdall.3d.tiltup", action: { c.orbit(tilt: 10) }) {
                        Text("TILT +").font(MGWebFont.mono(9))
                    }
                    ControlButton(accessibility: "Tilt towards straight down", identifier: "mg.heimdall.3d.tiltdown", action: { c.orbit(tilt: -10) }) {
                        Text("TILT −").font(MGWebFont.mono(9))
                    }
                    Compass(heading: c.settled?.heading ?? 0) { c.resetOrientation(reducedMotion: reducedMotion) }
                        .frame(width: max(30, minTarget))
                }
            }
            StylePicker(style: style, offline: offline, choose: chooseStyle)
            HeightsSlider(heights: $heights)
            CreditsControl(credits: credits)
            CreditsStrip(credits: credits)
        }
    }
}

/// ALT, position, TILT and HDG, and the region in view; VoiceOver reads the summary.
struct CameraReadout: View {
    let camera: ViewState?
    let current: Region?
    let summary: String

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if let c = camera {
                Text("ALT \(Presets.altitudeText(c.altKm)) KM")
                Text("\(Presets.latText(c.lat)) · \(Presets.lonText(c.lon))")
                Text("TILT \(Int(c.pitch.rounded()))° · HDG \(Presets.bearingText(c.heading))")
                Text("\(c.pitch >= 1 ? "TOWARDS" : "OVER") \(current?.label ?? "OPEN GLOBE")")
            } else {
                Text("—")
            }
        }
        .font(MGWebFont.mono(9))
        .tracked(0.08, 9)
        .foregroundStyle(MGWeb.textBody)
        .lineSpacing(2)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .ignore)
        .accessibilityAddTraits(.isStaticText)
        .accessibilityLabel(Text(summary.isEmpty ? "Camera" : summary))
        .accessibilityIdentifier("mg.heimdall.3d.summary")
    }
}

/// The compass: turns with the heading; resets to north up, looking straight down.
struct Compass: View {
    let heading: Double
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Canvas { ctx, size in
                let s = min(size.width, size.height) / 24 * 0.75
                ctx.translateBy(x: size.width / 2, y: size.height / 2)
                ctx.rotate(by: .degrees(-heading))
                var north = Path()
                north.move(to: CGPoint(x: 0, y: -10 * s)); north.addLine(to: CGPoint(x: 4 * s, y: 0))
                north.addLine(to: CGPoint(x: 0, y: -2 * s)); north.addLine(to: CGPoint(x: -4 * s, y: 0)); north.closeSubpath()
                var south = Path()
                south.move(to: CGPoint(x: 0, y: 10 * s)); south.addLine(to: CGPoint(x: -4 * s, y: 0))
                south.addLine(to: CGPoint(x: 0, y: 2 * s)); south.addLine(to: CGPoint(x: 4 * s, y: 0)); south.closeSubpath()
                ctx.fill(north, with: .color(MGWeb.warn))
                ctx.fill(south, with: .color(MGWeb.textMuted.opacity(0.6)))
            }
            .frame(width: 24, height: 24)
            .frame(maxWidth: .infinity, minHeight: max(minTarget, 26))
            .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(MGWeb.border))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("Heading \(Presets.bearingText(heading)). Reset to north up, looking straight down"))
        .accessibilityIdentifier("mg.heimdall.3d.compass")
    }
}

/// STYLE: CLEAN, TOPOGRAPHIC, SATELLITE; offline they cannot load, and say so.
struct StylePicker: View {
    let style: GlobeStyle
    let offline: Bool
    let choose: (GlobeStyle) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            PanelHeading(text: "STYLE")
            VStack(spacing: 2) {
                ForEach(GlobeStyle.allCases, id: \.self) { s in
                    let on = s == style
                    Button {
                        if !offline { choose(s) }
                    } label: {
                        Text(s.label)
                            .font(MGWebFont.mono(10))
                            .tracked(0.08, 10)
                            .foregroundStyle(offline ? MGWeb.textMuted : on ? MGWebAccent.heimdall.text : MGWeb.textBody)
                            .padding(.horizontal, 8)
                            .frame(maxWidth: .infinity, minHeight: max(minTarget, 24), alignment: .leading)
                            .background(on ? MGWebAccent.heimdall.tint : .clear)
                            .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(on ? MGWebAccent.heimdall.fill : .clear))
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(Text("\(s.label) style"))
                    .accessibilityHint(Text(offline ? "Styles need map tiles; offline" : ""))
                    .accessibilityAddTraits(on ? [.isSelected] : [])
                    .accessibilityIdentifier("mg.heimdall.3d.style.\(s.rawValue)")
                }
            }
            if offline {
                Text("STYLES NEED MAP TILES · OFFLINE").font(MGWebFont.mono(9)).tracked(0.1, 9).foregroundStyle(MGWeb.textMuted)
            }
        }
    }
}

/// HEIGHTS: altitude exaggeration, ×1 to ×20.
struct HeightsSlider: View {
    @Binding var heights: Double

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            PanelHeading(text: "HEIGHTS", trailing: "×\(Int(heights))")
            Slider(value: $heights, in: GlobeItems.minExaggeration...GlobeItems.maxExaggeration, step: 1)
                .tint(MGWebAccent.heimdall.fill)
                .accessibilityLabel(Text("Heights"))
                .accessibilityValue(Text("Heights exaggerated \(Int(heights)) times"))
                .accessibilityIdentifier("mg.heimdall.3d.heights")
        }
    }
}

/// The iPhone's controls: the camera readout at the top; along the bottom the REGION,
/// TRACKS and STYLE menus with the credits, the zoom, tilt and compass buttons, and the
/// credits strip. Every control is 44 pt. [FR-H-1] [AC-C-11]
struct GlobeCompactControls: View {
    @ObservedObject var c: GlobeController
    @ObservedObject var picture: HeimdallPictureModel
    let regions: [Region]
    let current: Region?
    let credits: [Credit]
    let style: GlobeStyle
    let offline: Bool
    @Binding var heights: Double
    let chooseStyle: (GlobeStyle) -> Void
    let flyToRegion: (Region) -> Void
    let chooseTrack: (String) -> Void
    let reducedMotion: Bool
    let summary: String
    @State private var showingCredits = false
    @State private var showingHeights = false
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let large = typeSize.isAccessibilitySize
        // At accessibility text sizes the controls stack, wrap and scroll, so no label is cut.
        let row = large ? AnyLayout(VStackLayout(spacing: 6)) : AnyLayout(HStackLayout(spacing: 6))
        VStack(spacing: 0) {
            if !large {
                HStack(alignment: .top) {
                    Panel { CameraReadout(camera: c.settled, current: current, summary: summary) }.frame(maxWidth: 200)
                    Spacer(minLength: 0)
                }
                .padding(8)
            }
            Spacer(minLength: 0)
            CompactControlArea(large: large) {
                if large { CameraReadout(camera: c.settled, current: current, summary: summary) }
                row {
                    RegionMenu(regions: regions, current: current, choose: flyToRegion)
                    TrackMenu(picture: picture, idPrefix: "mg.heimdall.3d.track", choose: chooseTrack)
                    Menu {
                        Picker("Style", selection: Binding(get: { style }, set: { chooseStyle($0) })) {
                            ForEach(GlobeStyle.allCases, id: \.self) { s in Text(s.label).tag(s) }
                        }
                        .disabled(offline)
                        Button("Heights ×\(Int(heights))…") { showingHeights = true }
                    } label: {
                        MenuLabel(text: style.label)
                    }
                    .accessibilityLabel(Text("Style, \(style.label)"))
                    .accessibilityIdentifier("mg.heimdall.3d.style")
                    CreditsButton(showing: $showingCredits)
                }
                row {
                    ControlButton(accessibility: "Zoom in", identifier: "mg.heimdall.3d.zoomin", action: { c.nudge(dx: 0, dy: 0, zoom: 0.6) }) {
                        Text("+").font(MGWebFont.mono(15))
                    }
                    ControlButton(accessibility: "Zoom out", identifier: "mg.heimdall.3d.zoomout", action: { c.nudge(dx: 0, dy: 0, zoom: 1 / 0.6) }) {
                        Text("−").font(MGWebFont.mono(15))
                    }
                    ControlButton(accessibility: "Tilt towards the horizon", identifier: "mg.heimdall.3d.tiltup", action: { c.orbit(tilt: 10) }) {
                        Text("TILT +")
                    }
                    ControlButton(accessibility: "Tilt towards straight down", identifier: "mg.heimdall.3d.tiltdown", action: { c.orbit(tilt: -10) }) {
                        Text("TILT −")
                    }
                    Compass(heading: c.settled?.heading ?? 0) { c.resetOrientation(reducedMotion: reducedMotion) }
                }
                CreditsStrip(credits: credits).frame(maxWidth: .infinity, alignment: .trailing)
            }
        }
        .sheet(isPresented: $showingCredits) { CreditsSheet(credits: credits) }
        .sheet(isPresented: $showingHeights) {
            VStack(alignment: .leading, spacing: 12) {
                HeightsSlider(heights: $heights)
                Button("Done") { showingHeights = false }
                    .frame(minHeight: 44)
                    .accessibilityIdentifier("mg.heimdall.3d.heights.done")
            }
            .padding(20)
            .presentationDetents([.height(180)])
        }
    }
}

/// The iPhone's bottom control area: a translucent band; at accessibility text sizes it
/// scrolls, up to half the view, so stacked controls never run off the screen.
struct CompactControlArea<Content: View>: View {
    let large: Bool
    @ViewBuilder let content: () -> Content

    var body: some View {
        Group {
            if large {
                ViewThatFits(in: .vertical) {
                    VStack(spacing: 6) { content() }
                    ScrollView { VStack(spacing: 6) { content() } }
                }
                .containerRelativeFrame(.vertical, alignment: .bottom) { h, _ in h * 0.55 }
                .fixedSize(horizontal: false, vertical: false)
            } else {
                VStack(spacing: 6) { content() }
            }
        }
        .padding(8)
        .background(MGWeb.bgSunken)
    }
}

struct MenuLabel: View {
    let text: String
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        let large = typeSize.isAccessibilitySize
        HStack(spacing: 4) {
            Text(text).lineLimit(large ? nil : 1).minimumScaleFactor(large ? 1 : 0.7)
                .fixedSize(horizontal: false, vertical: large)
            Image(systemName: "chevron.up.chevron.down").font(.system(size: 9)).accessibilityHidden(true)
        }
        .font(MGWebFont.mono(10))
        .foregroundStyle(MGWeb.textBody)
        .padding(.horizontal, 6)
        .frame(maxWidth: .infinity, minHeight: minTarget)
        .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(MGWeb.border))
        .contentShape(Rectangle())
    }
}

/// The REGION menu (the iPhone's REGION list).
struct RegionMenu: View {
    let regions: [Region]
    let current: Region?
    let choose: (Region) -> Void

    var body: some View {
        Menu {
            ForEach(regions) { r in
                Button { choose(r) } label: {
                    if current?.id == r.id { Label(r.label, systemImage: "checkmark") } else { Text(r.label) }
                }
                .accessibilityIdentifier("mg.heimdall.region.\(r.id)")
            }
        } label: {
            MenuLabel(text: "REGION")
        }
        .accessibilityLabel(Text("Region\(current.map { ", \($0.label)" } ?? "")"))
        .accessibilityIdentifier("mg.heimdall.region")
    }
}

/// The TRACKS menu: the LAYERS switches, then each track the layers show.
struct TrackMenu: View {
    @ObservedObject var picture: HeimdallPictureModel
    let idPrefix: String
    let choose: (String) -> Void

    var body: some View {
        Menu {
            Section("LAYERS") {
                ForEach(picture.layers, id: \.id) { layer in
                    Toggle(layer.label, isOn: Binding(get: { picture.isOn(layer.id) }, set: { picture.setLayer(layer.id, $0) }))
                }
            }
            Section("TRACKS") {
                ForEach(picture.visibleTracks, id: \.id) { t in
                    let placed = SamplePositions.position(t.id) != nil
                    Button { choose(t.id) } label: {
                        Text("\(t.id) · \(placed ? t.alt : "NO POSITION")")
                    }
                    .disabled(!placed)
                    .accessibilityLabel(Text(TrackRow.label(t, placed: placed)))
                    .accessibilityIdentifier("\(idPrefix).\(t.id)")
                }
            }
        } label: {
            MenuLabel(text: "TRACKS")
        }
        .accessibilityLabel(Text("Tracks and layers"))
        .accessibilityIdentifier("mg.heimdall.tracks")
    }
}

struct CreditsButton: View {
    @Binding var showing: Bool

    var body: some View {
        Button {
            showing = true
        } label: {
            Text("ⓘ")
                .font(MGWebFont.mono(14))
                .foregroundStyle(MGWeb.textBody)
                .frame(width: minTarget, height: minTarget)
                .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(MGWeb.border))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("Credits"))
        .accessibilityIdentifier("mg.heimdall.credits")
    }
}

struct CreditsSheet: View {
    let credits: [Credit]
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("CREDITS").font(MGWebFont.mono(11)).tracked(0.14, 11).foregroundStyle(MGWeb.textMuted)
                    .accessibilityAddTraits(.isHeader)
                Spacer()
                Button("Done") { dismiss() }.frame(minWidth: 44, minHeight: 44)
            }
            CreditsList(credits: credits)
        }
        .padding(16)
        .background(MGWeb.bgSunken)
        .presentationDetents([.medium, .large])
    }
}

import MidgardCore
import MidgardKit
import SwiftUI

/// The Operating Picture on a real map, the second 2D mode, as the web's `map/MapView.tsx`:
/// VersaTiles vector tiles (OpenStreetMap data) over the bundled Natural Earth land, with
/// the tracks at their positions, the REGION and track lists, the MAP readout, zoom and
/// the credits. It shares the picture's layer switches and selection. Tiles are fetched
/// only while it is on screen. [FR-H-1] [AC-H-1] [NFR-PRV-1]
struct HeimdallMapView: View {
    @ObservedObject var picture: HeimdallPictureModel
    let context: HeimdallPictureViewContext
    @StateObject private var c = MapController()
    @State private var fitted = false
    @State private var interacted = false
    @State private var started = false
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion

    static let identifier = "mg.heimdall.map.view"

    private var reducedMotion: Bool { MotionPolicy.reduced(system: systemReduceMotion) }
    private var points: [GeoPoint] { picture.tracks.compactMap { SamplePositions.position($0.id) } }
    private var regions: [Region] { Presets.regions(points) }
    private var placed: [HeimdallTrack] { picture.visibleTracks.filter { SamplePositions.position($0.id) != nil } }
    private var offline: Bool { context.offline || c.tileState == .offline }
    private var credits: [Credit] { offline ? [.naturalEarth] : [.vector, .naturalEarth] }
    private var current: Region? { c.settled.flatMap { Presets.region(at: $0.lat, $0.lon, in: regions) } }

    private var summary: String {
        guard let m = c.settled else { return "" }
        return "Map at zoom \(String(format: "%.1f", m.zoom)), centred on \(Presets.latText(m.lat)), \(Presets.lonText(m.lon)), "
            + "over \(current?.label ?? "open map")."
    }

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .topLeading) {
                MapSurface(controller: c)
                    .accessibilityElement()
                    .accessibilityAddTraits(.isImage)
                    .accessibilityLabel(Text("Map"))
                    .accessibilityValue(Text(summary))
                    .accessibilityHint(Text(isCompactPlatform ? "Drag to pan, pinch to zoom. The track list reaches each track."
                                                              : "Arrow keys pan, plus and minus zoom. The track list reaches each track."))
                    .accessibilityIdentifier("mg.heimdall.map.surface")
                markers.frame(width: geo.size.width, height: geo.size.height)
                notices.frame(width: geo.size.width).padding(.top, context.compact ? 64 : 12)
                if context.compact {
                    compact.frame(width: geo.size.width, height: geo.size.height)
                } else {
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
                        TrackList(picture: picture, count: placed.count, suffix: "ON MAP", idPrefix: "mg.heimdall.map.track",
                                  choose: { select($0, fly: true) })
                    }
                    .frame(width: 196)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(12)
                    Panel {
                        VStack(alignment: .leading, spacing: 6) {
                            PanelHeading(text: "MAP")
                            readout
                            zoomButtons
                        }
                        CreditsControl(credits: credits)
                        CreditsStrip(credits: credits)
                    }
                    .frame(width: 212)
                    .padding(12)
                    .frame(width: geo.size.width, alignment: .topTrailing)
                }
            }
        }
        .background(MGWeb.bgPage)
        .clipped()
        .onAppear(perform: start)
        .onDisappear { c.stop() }
        .onChange(of: picture.tracks.map(\.id)) { refit() }
        .onChange(of: scheme) { c.setDark(scheme == .dark) }
        .onChange(of: context.offline) { c.start(online: !context.offline) }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(Self.identifier)
    }

    private var readout: some View {
        VStack(alignment: .leading, spacing: 2) {
            if let m = c.settled {
                Text("ZOOM \(String(format: "%.1f", m.zoom))")
                Text("\(Presets.latText(m.lat)) · \(Presets.lonText(m.lon))")
                Text("OVER \(current?.label ?? "OPEN MAP")")
            } else {
                Text("—")
            }
        }
        .font(MGWebFont.mono(9))
        .tracked(0.08, 9)
        .foregroundStyle(MGWeb.textBody)
        .fixedSize(horizontal: false, vertical: true)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(summary.isEmpty ? "Map" : summary))
        .accessibilityIdentifier("mg.heimdall.map.summary")
    }

    private var zoomButtons: some View {
        HStack(spacing: 4) {
            ControlButton(accessibility: "Zoom in", identifier: "mg.heimdall.map.zoomin", action: { c.nudge(dx: 0, dy: 0, dZoom: 1) }) {
                Text("+").font(MGWebFont.mono(13))
            }
            ControlButton(accessibility: "Zoom out", identifier: "mg.heimdall.map.zoomout", action: { c.nudge(dx: 0, dy: 0, dZoom: -1) }) {
                Text("−").font(MGWebFont.mono(13))
            }
        }
    }

    @State private var showingCredits = false

    @Environment(\.dynamicTypeSize) private var typeSize

    private var compact: some View {
        let large = typeSize.isAccessibilitySize
        let row = large ? AnyLayout(VStackLayout(spacing: 6)) : AnyLayout(HStackLayout(spacing: 6))
        return VStack(spacing: 0) {
            if !large {
                HStack {
                    Panel { readout }.frame(maxWidth: 200)
                    Spacer(minLength: 0)
                }
                .padding(8)
            }
            Spacer(minLength: 0)
            CompactControlArea(large: large) {
                if large { readout }
                row {
                    RegionMenu(regions: regions, current: current, choose: flyToRegion)
                    TrackMenu(picture: picture, idPrefix: "mg.heimdall.map.track", choose: { select($0, fly: true) })
                    zoomButtons
                    CreditsButton(showing: $showingCredits)
                }
                CreditsStrip(credits: credits).frame(maxWidth: .infinity, alignment: .trailing)
            }
        }
        .sheet(isPresented: $showingCredits) { CreditsSheet(credits: credits) }
    }

    @ViewBuilder private var notices: some View {
        VStack {
            if offline {
                Notice(text: TilePause.offlineNotice)
            } else if c.paused {
                Notice(text: TilePause.notice)
            }
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: markers

    private var markers: some View {
        let cam = c.camera
        _ = cam
        return ZStack(alignment: .topLeading) {
            // The labels, drawn (not text elements over the map), under the markers.
            Canvas { ctx, size in
                for t in placed {
                    guard let pos = SamplePositions.position(t.id) else { continue }
                    let p = c.project(pos.lat, pos.lon)
                    guard p.x > -40, p.y > -40, p.x < size.width + 40, p.y < size.height + 40 else { continue }
                    let warn = t.track == "anomalous"
                    drawTrackLabel(&ctx, "\(t.id)\(warn ? " ⚠\u{FE0E}" : "") · \(Int(t.hdg))°", at: CGPoint(x: p.x, y: p.y + 10),
                                   selected: t.id == picture.selected)
                }
            }
            .allowsHitTesting(false)
            .accessibilityHidden(true)
            ForEach(placed, id: \.id) { t in
                if let pos = SamplePositions.position(t.id) {
                    marker(t, at: c.project(pos.lat, pos.lon))
                }
            }
            if let id = c.hovered, let t = placed.first(where: { $0.id == id }), let pos = SamplePositions.position(id) {
                GeometryReader { geo in
                    let p = c.project(pos.lat, pos.lon)
                    HeimdallTrackCard(track: t, selected: t.id == picture.selected)
                        .fixedSize()
                        .position(x: min(geo.size.width - 110, max(110, p.x)), y: p.y + (p.y < 150 ? 90 : -90))
                }
                .allowsHitTesting(false)
                .accessibilityHidden(true)
            }
        }
    }

    private func marker(_ t: HeimdallTrack, at p: CGPoint) -> some View {
        let tone = Tone.of(t)
        let selected = t.id == picture.selected
        let side: CGFloat = t.layer == "ground" ? 10 : 12
        let corner: CGFloat = t.layer == "maritime" ? 2 : t.layer == "ground" ? 1 : 6
        let hit = max(minTarget, 24)
        return ZStack {
            // The heading vector: 22 pt, fading away from the dot.
            Rectangle()
                .fill(LinearGradient(colors: [tone, tone.opacity(0)], startPoint: .bottom, endPoint: .top))
                .frame(width: 2, height: 22)
                .offset(y: -17)
                .rotationEffect(.degrees(t.hdg))
                .allowsHitTesting(false)
            Button {
                select(t.id, fly: false)
            } label: {
                RoundedRectangle(cornerRadius: corner)
                    .fill(tone)
                    .frame(width: side, height: side)
                    .rotationEffect(.degrees(t.layer == "ground" ? 45 : 0))
                    .background(RoundedRectangle(cornerRadius: corner + 2).fill(MGWeb.bgSunken.opacity(0.7)).frame(width: side + 4, height: side + 4))
                    .overlay {
                        if selected {
                            Circle().strokeBorder(tone, lineWidth: 1).frame(width: side + 10, height: side + 10)
                        }
                    }
                    .frame(width: hit, height: hit)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { inside in
                if inside { c.hovered = t.id } else if c.hovered == t.id { c.hovered = nil }
            }
            .accessibilityLabel(Text(TrackRow.label(t, placed: true)))
            .accessibilityAddTraits(selected ? [.isSelected] : [])
            .accessibilityIdentifier("mg.heimdall.map.marker.\(t.id)")
        }
        .position(p)
        .opacity(p.x < -40 || p.y < -40 || p.x > c.viewport.width + 40 || p.y > c.viewport.height + 40 ? 0 : 1)
    }

    // MARK: life cycle and moves

    private func start() {
        guard !started else { return }
        started = true
        let ctl = c
        c.onUserMove = { [weak ctl] in
            interacted = true
            ctl?.moved(.user)
        }
        c.setDark(scheme == .dark)
        // The first view, fitted to the tracks: the console chose it, so tiles start paused.
        let sector = regions[regions.count - 1]
        fitted = sector.fittedToTracks
        c.moved(.track)
        c.start(online: !context.offline)
        c.flyTo(MapView2D.regionCamera(sector, viewportPx: max(c.viewport.height, 600)), reducedMotion: true)
    }

    private func refit() {
        let sector = regions[regions.count - 1]
        guard started, !fitted, sector.fittedToTracks else { return }
        fitted = true
        if interacted { return }
        c.moved(.track)
        c.flyTo(MapView2D.regionCamera(sector, viewportPx: max(c.viewport.height, 600)), reducedMotion: true)
    }

    private func flyToRegion(_ r: Region) {
        interacted = true
        c.moved(TilePause.move(for: r))
        c.flyTo(MapView2D.regionCamera(r, viewportPx: max(c.viewport.height, 200)), reducedMotion: reducedMotion)
    }

    private func select(_ id: String, fly: Bool) {
        picture.select(id)
        context.onInspect(id)
        guard fly, let p = SamplePositions.position(id) else { return }
        interacted = true
        c.moved(.track)
        c.flyTo(MapCamera(lat: p.lat, lon: p.lon, zoom: max(c.camera.zoom, 8)), reducedMotion: reducedMotion)
    }
}

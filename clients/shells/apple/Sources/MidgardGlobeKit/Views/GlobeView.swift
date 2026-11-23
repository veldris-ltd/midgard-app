import Metal
import MidgardCore
import MidgardKit
import SwiftUI

/// The Operating Picture as a 3D globe, as the web's `globe/GlobeView.tsx`: the globe
/// (Metal), the tracks over it at their positions with altitude stems, the REGION list
/// and the track list, the CAMERA readout and controls, the STYLE and HEIGHTS settings
/// and the credits. It shares the picture's layer switches and selection, so a track
/// chosen here is the inspector's. Map tiles are fetched only while this view is on
/// screen, and the loader is destroyed (every request cancelled) when it closes.
/// [FR-H-1] [AC-H-1] [NFR-PRV-1]
struct HeimdallGlobeView: View {
    @ObservedObject var picture: HeimdallPictureModel
    let context: HeimdallPictureViewContext
    @StateObject private var c = GlobeController()
    @AppStorage("mg-heimdall-globe-style") private var styleRaw = GlobeStyle.clean.rawValue
    @State private var heights = GlobeItems.defaultExaggeration
    @State private var failed = false
    @State private var fitted = false
    @State private var interacted = false
    @State private var started = false
    @State private var outlines = GlobeOutlines.shared()
    @Environment(\.colorScheme) private var scheme
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion

    static let identifier = "mg.heimdall.3d.view"

    private var reducedMotion: Bool { MotionPolicy.reduced(system: systemReduceMotion) }
    private var style: GlobeStyle { GlobeStyle(rawValue: styleRaw) ?? .clean }
    private var online: Bool { c.tileState == .online }
    private var offline: Bool { context.offline || c.tileState == .offline }
    private var points: [GeoPoint] { picture.tracks.compactMap { SamplePositions.position($0.id) } }
    private var regions: [Region] { Presets.regions(points, closeUps: online) }
    private var listed: [HeimdallTrack] { picture.visibleTracks }

    private var credits: [Credit] {
        var out = [Credit.naturalEarth]
        if online {
            out.append(.terrain)
            if style == .satellite { out.append(.imagery) }
        }
        return out
    }

    var body: some View {
        Group {
            if failed || outlines == nil {
                unavailable
            } else if let outlines {
                globe(outlines)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier(Self.identifier)
    }

    private var unavailable: some View {
        VStack(spacing: 8) {
            Text("3D VIEW UNAVAILABLE · METAL").font(MGWebFont.mono(12).weight(.semibold)).foregroundStyle(MGWeb.warnText)
            Text("THIS DEVICE GAVE THE GLOBE NO METAL DEVICE · THE 2D VIEW STILL WORKS")
                .font(MGWebFont.mono(10)).foregroundStyle(MGWeb.textMuted)
        }
        .multilineTextAlignment(.center)
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(MGWeb.bgPage)
    }

    private func globe(_ outlines: GlobeOutlines) -> some View {
        GeometryReader { geo in
            ZStack(alignment: .topLeading) {
                GlobeSurface(controller: c, outlines: outlines) { failed = true }
                    .accessibilityElement()
                    .accessibilityAddTraits(.isImage)
                    .accessibilityLabel(Text("Globe"))
                    .accessibilityValue(Text(summary))
                    .accessibilityHint(Text(isCompactPlatform
                        ? "Drag to pan, pinch to zoom, twist to turn, two fingers up or down to tilt. The track list reaches each track."
                        : "Arrow keys pan, Shift and arrows tilt and turn, plus and minus zoom. The track list reaches each track."))
                    .accessibilityIdentifier("mg.heimdall.3d.surface")
                GlobeMarkers(controller: c, picture: picture) { id in
                    select(id, fly: false)
                }
                .frame(width: geo.size.width, height: geo.size.height)
                notices.frame(width: geo.size.width).padding(.top, context.compact ? 120 : 12)
                if context.compact {
                    GlobeCompactControls(c: c, picture: picture, regions: regions, current: current, credits: credits,
                                         style: style, offline: offline, heights: $heights,
                                         chooseStyle: { styleRaw = $0.rawValue }, flyToRegion: flyToRegion,
                                         chooseTrack: { select($0, fly: true) }, reducedMotion: reducedMotion,
                                         summary: summary)
                        .frame(width: geo.size.width, height: geo.size.height)
                } else {
                    GlobeNavPanel(c: c, picture: picture, regions: regions, current: current, flyToRegion: flyToRegion,
                                  chooseTrack: { select($0, fly: true) })
                        .frame(width: 196)
                        .frame(maxHeight: geo.size.height - 96, alignment: .top)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(12)
                    GlobeSidePanel(c: c, current: current, credits: credits, style: style, offline: offline,
                                   heights: $heights, chooseStyle: { styleRaw = $0.rawValue }, reducedMotion: reducedMotion,
                                   summary: summary)
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
        .onChange(of: itemsKey) { pushItems() }
        .onChange(of: picture.selected) { c.setSelected(picture.selected) }
        .onChange(of: styleRaw) { c.setStyle(style) }
        .onChange(of: scheme) { applyScheme() }
        .onChange(of: context.offline) { attachTiles() }
    }

    private var itemsKey: String {
        picture.visibleTracks.map(\.id).joined(separator: ",") + "|\(heights)"
    }

    private var current: Region? { c.settled.flatMap { Presets.regionInView($0, regions) } }
    private var summary: String { c.settled.map { Presets.describe($0, regions) } ?? "" }

    @ViewBuilder private var notices: some View {
        VStack(spacing: 6) {
            if offline {
                Notice(text: TilePause.offlineNotice)
            } else if c.paused && online {
                Notice(text: TilePause.notice)
            }
        }
        .frame(maxWidth: .infinity)
    }

    // MARK: life cycle

    private func start() {
        guard !started else { return }
        started = true
        let ctl = c
        c.onUserMove = { [weak ctl] in
            interacted = true
            ctl?.moved(.user)
        }
        c.setStyle(style)
        applyScheme()
        pushItems()
        c.setSelected(picture.selected)
        // The first view, fitted to the tracks, or a fixed one until they arrive: either
        // way the console chose it, not the user, so tile loading starts paused.
        let sector = regions[regions.count - 1]
        fitted = sector.fittedToTracks
        c.moved(.track)
        c.flyTo(sector.camera, reducedMotion: true)
        attachTiles()
    }

    /// The tile layer, only while online: the session's OFFLINE chip keeps the plain globe.
    private func attachTiles() {
        guard started else { return }
        if context.offline {
            c.attachTiles(nil)
        } else if c.tiles == nil, let device = MTLCreateSystemDefaultDevice() {
            c.attachTiles(GlobeTiles(device: device, tiles: .publicHosts, fetcher: TileNetwork.fetcher,
                                     disk: TileNetwork.disk, online: { NetworkPath.shared.online }))
        }
    }

    private func applyScheme() {
        c.setDark(scheme == .dark)
    }

    private func pushItems() {
        let items = GlobeItems.items(picture.tracks, layerOn: picture.isOn, exaggeration: heights)
        c.setItems(items, heights: GlobeItems.clampExaggeration(heights))
    }

    /// Tracks that arrive after start-up: fit the first view to them, once.
    private func refit() {
        pushItems()
        let sector = regions[regions.count - 1]
        guard started, !fitted, sector.fittedToTracks else { return }
        fitted = true
        if interacted { return }
        c.moved(.track)
        c.flyTo(sector.camera, reducedMotion: true)
    }

    // MARK: moves

    private func flyToRegion(_ r: Region) {
        interacted = true
        c.moved(TilePause.move(for: r))
        c.flyTo(r.camera, reducedMotion: reducedMotion)
    }

    /// Selects a track (the inspector's), and with `fly` flies to it from behind, keeping
    /// the tilt and heading, never below its drawn marker.
    private func select(_ id: String, fly: Bool) {
        picture.select(id)
        context.onInspect(id)
        guard fly, let t = picture.tracks.first(where: { $0.id == id }), let p = SamplePositions.position(id) else { return }
        let km = (Altitude.parse(t.alt)?.metres ?? 0) / 1000
        let k = GlobeItems.clampExaggeration(heights)
        let now = c.camera
        let alt = max(GlobeCamera.trackFlyAltitudeKm(km, heights: k, current: now.altKm), km * k * 3)
        interacted = true
        c.moved(.track)
        c.flyTo(GlobeCamera.lookingAt(p, altKm: alt, pitch: now.pitch, heading: now.heading), reducedMotion: reducedMotion)
    }
}

/// The tile network for both views: one client (the fixed hosts) and one disk cache.
enum TileNetwork {
    static let fetcher = TileHTTPClient(hosts: TileSet.publicHosts.hosts)
    static let disk = DiskTileCache.appDefault()
}

import CoreGraphics
import CoreText
import Foundation
import QuartzCore
import MidgardKit
import SwiftUI

/// The MAP view's colours from the design tokens, per mode, as the web's
/// `map/mapRenderer.ts` mixes them.
struct MapPalette {
    let sea, land, green, built, bare, building, coast, road, roadMajor, roadCasing, rail, boundary, label, halo: CGColor

    static func make(dark: Bool) -> MapPalette {
        typealias C = SIMD4<Double>
        let p = dark ? GlobePalette.dark : GlobePalette.light
        let muted: C = dark ? GlobePalette.rgba(0x9199A3) : GlobePalette.rgba(0x5C636B)
        let faint: C = dark ? GlobePalette.rgba(0x6B7279) : GlobePalette.rgba(0x8A9099)
        let page: C = dark ? GlobePalette.rgba(0x0A0B0D) : GlobePalette.rgba(0xEFF0F3)
        func mix(_ a: C, _ b: C, _ t: Double) -> C { C(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, a.z + (b.z - a.z) * t, 1) }
        func cg(_ c: C, _ a: Double = 1) -> CGColor {
            CGColor(colorSpace: CGColorSpace(name: CGColorSpace.sRGB)!, components: [c.x, c.y, c.z, a])!
        }
        let water = mix(p.sea, C(0.2, 0.4, 0.6, 1), dark ? 0.12 : 0.18)
        return MapPalette(sea: cg(water), land: cg(p.land), green: cg(mix(p.land, C(0.24, 0.55, 0.3, 1), dark ? 0.14 : 0.2)),
                          built: cg(mix(p.land, faint, 0.12)), bare: cg(mix(p.land, muted, 0.08)),
                          building: cg(mix(p.land, p.border, 0.6)), coast: cg(p.coast), road: cg(mix(p.border, muted, 0.25)),
                          roadMajor: cg(mix(p.coast, muted, 0.45)), roadCasing: cg(p.land), rail: cg(mix(p.border, faint, 0.5)),
                          boundary: cg(mix(p.coast, muted, 0.3)), label: cg(muted), halo: cg(page, 0.85))
    }

    func fill(_ c: FillClass) -> CGColor {
        switch c {
        case .green: return green
        case .built: return built
        case .bare: return bare
        case .water: return sea
        case .building: return building
        }
    }

    func line(_ c: LineClass) -> (width: CGFloat, colour: CGColor, dash: [CGFloat]) {
        switch c {
        case .water: return (0.9, sea, [])
        case .region: return (0.8, boundary, [4, 3])
        case .country: return (1.3, boundary, [7, 3, 2, 3])
        case .ferry: return (0.8, boundary, [3, 4])
        case .rail: return (0.9, rail, [5, 3])
        case .local: return (0.7, road, [])
        case .minor: return (1.0, road, [])
        case .major: return (1.6, roadMajor, [])
        case .motorway: return (2.2, roadMajor, [])
        }
    }
}

/// The MAP view's state, as the web's `map/mapRenderer.ts` keeps it: the camera, its
/// eased flights, the vector tile loader (VersaTiles OpenStreetMap tiles, decoded off the
/// main thread within the limits, at most 96 asked for at once, nearest the centre
/// first, a 64 MB cache besides the 128 MB on-screen cap), the tile pause, and the
/// drawing: the bundled Natural Earth land wherever no tile is drawn, the tiles' fills
/// and lines, and up to 60 place labels a frame from the best 400 candidates. [FR-H-1]
@MainActor
final class MapController: ObservableObject {
    static let maxRequested = 96
    static let cacheEntries = 192
    static let cacheBytes = 64 * 1024 * 1024
    static let ancestorLevels = 5

    @Published private(set) var camera = MapCamera(lat: 57, lon: -2, zoom: 6)
    @Published private(set) var settled: MapCamera?
    @Published private(set) var tileState: TileLoader.State?
    @Published private(set) var paused = false
    @Published var hovered: String?

    private(set) var viewport = CGSize(width: 1, height: 1)
    private(set) var dark = true
    private var palette = MapPalette.make(dark: true)
    private var loader: TileLoader?
    private var inUse: Set<String> = []
    private var flight: (from: MapCamera, to: MapCamera, start: CFTimeInterval)?
    private var settleWork: DispatchWorkItem?
    private var land: CGPath?
    private let tiles = TileSet.publicHosts
    var requestRender: () -> Void = {}
    var onUserMove: () -> Void = {}

    init() {
        if let close = GlobeOutlines.shared()?.close { land = Self.landPath(close) }
    }

    /// The bundled land, as one path in zoom-0 world points.
    static func landPath(_ file: OutlineFile) -> CGPath {
        let p = CGMutablePath()
        for poly in file.land ?? [] {
            for ring in poly {
                let pts = Outlines.decodeLine(ring, quantum: file.quantum)
                var i = 0
                while i + 1 < pts.count {
                    let pt = CGPoint(x: TileMath.lonToTileX(pts[i], 0) * 256, y: TileMath.latToTileY(pts[i + 1], 0) * 256)
                    if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
                    i += 2
                }
                p.closeSubpath()
            }
        }
        return p
    }

    func start(online: Bool) {
        if online, loader == nil {
            var o = TileLoader.Options()
            o.cacheEntries = Self.cacheEntries
            o.cacheBytes = Self.cacheBytes
            let l = TileLoader(tiles: tiles, fetcher: TileNetwork.fetcher, disk: TileNetwork.disk, options: o,
                               weigh: { ($0 as? MapTileData)?.bytes ?? 0 }, online: { NetworkPath.shared.online })
            l.protect = { [unowned self] in self.inUse.contains($0) }
            l.onChange = { [weak self, weak l] in
                guard let self, let l else { return }
                if self.tileState != l.state { self.tileState = l.state }
                self.requestRender()
            }
            l.setPaused(paused)
            loader = l
            tileState = l.state
        } else if !online {
            loader?.destroy()
            loader = nil
            tileState = .offline
        }
        requestRender()
    }

    func stop() {
        loader?.destroy()
        loader = nil
        flight = nil
        settleWork?.cancel()
    }

    func setDark(_ d: Bool) {
        guard d != dark else { return }
        dark = d
        palette = MapPalette.make(dark: d)
        requestRender()
    }

    /// The drawing view's size, from its layout. The markers over the map are placed
    /// with it, so a change is published (on the next turn of the main loop: it is made
    /// during layout): until then they were placed for a 1 × 1 viewport, off screen and
    /// hidden, and stayed so until the camera next moved (the MAP view opened with no
    /// track markers).
    func setViewport(_ size: CGSize) {
        guard size.width > 1, size.height > 1, size != viewport else { return }
        viewport = size
        requestRender()
        DispatchQueue.main.async { [weak self] in self?.objectWillChange.send() }
    }

    func moved(_ move: TilePause.Move) {
        paused = TilePause.paused(after: move)
        loader?.setPaused(paused)
        requestRender()
    }

    // MARK: moves

    func flyTo(_ target: MapCamera, reducedMotion: Bool) {
        let to = MapView2D.clamp(target)
        if reducedMotion {
            flight = nil
            camera = to
            settled = to
            requestRender()
            return
        }
        flight = (camera, to, CACurrentMediaTime())
        requestRender()
    }

    /// Keyboard and button pan and zoom, about the centre.
    func nudge(dx: Double, dy: Double, dZoom: Double = 0) {
        onUserMove()
        flight = nil
        let w = viewport.width, h = viewport.height
        var c = MapView2D.pan(camera, dx, dy, w, h)
        if dZoom != 0 { c = MapView2D.zoom(c, to: c.zoom + dZoom, at: w / 2, h / 2, w, h) }
        camera = c
        requestRender()
        settleSoon()
    }

    func beginGesture() { flight = nil }

    func drag(dx: Double, dy: Double) {
        onUserMove()
        camera = MapView2D.pan(camera, dx, dy, viewport.width, viewport.height)
        requestRender()
    }

    /// Zoom by `dZoom` levels about (px, py).
    func zoom(by dZoom: Double, at p: CGPoint) {
        onUserMove()
        flight = nil
        camera = MapView2D.zoom(camera, to: camera.zoom + dZoom, at: p.x, p.y, viewport.width, viewport.height)
        requestRender()
        settleSoon()
    }

    func endGesture() { settled = camera }

    private func settleSoon() {
        settleWork?.cancel()
        let work = DispatchWorkItem { [weak self] in
            guard let self else { return }
            self.settled = self.camera
        }
        settleWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: work)
    }

    /// A position on the map, at the copy of the world nearest the viewport.
    func project(_ lat: Double, _ lon: Double) -> CGPoint {
        let p = MapView2D.project(lat, lon, camera, viewport.width, viewport.height)
        return CGPoint(x: MapView2D.nearestCopyX(p.x, viewport.width, camera.zoom), y: p.y)
    }

    // MARK: per frame

    /// Advances a flight; returns whether to keep drawing.
    func tick(now: CFTimeInterval) -> Bool {
        guard let f = flight else { return false }
        let t = min(1, (now - f.start) / 0.7)
        let e = GlobeCamera.easeInOutCubic(t)
        camera = MapView2D.clamp(MapCamera(lat: f.from.lat + (f.to.lat - f.from.lat) * e,
                                           lon: f.from.lon + (f.to.lon - f.from.lon) * e,
                                           zoom: f.from.zoom + (f.to.zoom - f.from.zoom) * e))
        if t >= 1 {
            flight = nil
            camera = f.to
            settled = f.to
            return false
        }
        return true
    }

    func draw(_ g: CGContext) {
        let w = viewport.width, h = viewport.height
        let pal = palette
        g.setFillColor(pal.sea)
        g.fill(CGRect(x: 0, y: 0, width: w, height: h))

        let vectorMax = tiles.sources[.vector]?.maxZoom ?? 14
        let view = MapView2D.visibleTiles(camera, w, h, maxZoom: vectorMax)
        let url = { (t: Tile) in self.tiles.url(.vector, t) ?? "" }
        let plan: MapOnScreen.Plan<MapTileData>
        if let loader {
            plan = MapOnScreen.plan(view.tiles.map(\.tile), get: { loader.get(url($0), as: MapTileData.self) }, bytes: \.bytes,
                                    maxRequested: Self.maxRequested, maxBytes: MapOnScreen.maxBytes,
                                    ancestorLevels: Self.ancestorLevels)
        } else {
            var p = MapOnScreen.Plan<MapTileData>()
            p.draw = Array(repeating: nil, count: view.tiles.count)
            plan = p
        }

        // The bundled land wherever a tile is not drawn: what shows with no tiles.
        if plan.draw.contains(where: { $0 == nil }), let land {
            let s = pow(2, camera.zoom)
            let cx = TileMath.lonToTileX(camera.lon, 0) * 256, cy = TileMath.latToTileY(camera.lat, 0) * 256
            let world = MapView2D.worldWidthPx(camera.zoom)
            let x0 = w / 2 - cx * s
            var k = Int(floor(-x0 / world))
            while x0 + Double(k) * world < w {
                g.saveGState()
                g.translateBy(x: x0 + Double(k) * world, y: h / 2 - cy * s)
                g.scaleBy(x: s, y: s)
                g.addPath(land)
                g.setFillColor(pal.land)
                g.fillPath()
                g.addPath(land)
                g.setStrokeColor(pal.coast)
                g.setLineWidth(0.8 / s)
                g.strokePath()
                g.restoreGState()
                k += 1
            }
        }

        if let loader {
            var keep = Set<String>()
            for (i, t) in plan.request.enumerated() {
                let u = url(t)
                keep.insert(u)
                let st = loader.status(u)
                if st == .none || st == .failed {
                    let z = t.z
                    loader.request(u, priority: i) { try MapFeatures.decode($0, z: z) }
                }
            }
            for t in plan.used { keep.insert(url(t)) }
            inUse = keep
            loader.retain(keep)
        }

        var labelled: [(data: MapTileData, left: Double, top: Double, size: Double)] = []
        for (i, v) in view.tiles.enumerated() {
            guard let d = plan.draw[i] else { continue }
            let k = Double(1 << d.up)
            let size = view.size * k
            let left = v.left - (Double(v.tile.x) - Double(d.from.x) * k) * view.size
            let top = v.top - (Double(v.tile.y) - Double(d.from.y) * k) * view.size
            g.saveGState()
            g.clip(to: CGRect(x: v.left, y: v.top, width: view.size, height: view.size).insetBy(dx: -0.5, dy: -0.5))
            drawTile(g, d.data, left: left, top: top, size: size, palette: pal)
            g.restoreGState()
            if d.up == 0 { labelled.append((d.data, left, top, size)) }
        }
        drawLabels(g, labelled, palette: pal)
    }

    private func drawTile(_ g: CGContext, _ d: MapTileData, left: Double, top: Double, size: Double, palette pal: MapPalette) {
        let k = size / d.extent
        g.saveGState()
        g.translateBy(x: left, y: top)
        g.scaleBy(x: k, y: k)
        g.setFillColor(pal.land)
        g.fill(CGRect(x: 0, y: 0, width: d.extent, height: d.extent))
        for c in FillClass.allCases {
            guard let p = d.fills[c] else { continue }
            g.addPath(p)
            g.setFillColor(pal.fill(c))
            g.fillPath()
        }
        g.setLineCap(.round)
        g.setLineJoin(.round)
        for c in LineClass.allCases {
            guard let p = d.lines[c] else { continue }
            let st = pal.line(c)
            if c == .major || c == .motorway {
                g.setLineDash(phase: 0, lengths: [])
                g.addPath(p)
                g.setStrokeColor(pal.roadCasing)
                g.setLineWidth((st.width + 1.6) / k)
                g.strokePath()
            }
            g.setLineDash(phase: 0, lengths: st.dash.map { $0 / k })
            g.addPath(p)
            g.setStrokeColor(st.colour)
            g.setLineWidth(st.width / k)
            g.strokePath()
        }
        g.setLineDash(phase: 0, lengths: [])
        g.restoreGState()
    }

    private func drawLabels(_ g: CGContext, _ tiles: [(data: MapTileData, left: Double, top: Double, size: Double)],
                            palette pal: MapPalette) {
        // Only the best few hundred near the view are measured, however many tiles show.
        let all = MapFeatures.labelCandidates(tiles, width: viewport.width, height: viewport.height)
        var placed: [CGRect] = []
        for c in all {
            let rank = c.label.rank
            let size: CGFloat = rank == 0 ? 11 : rank == 1 ? 10 : 9
            let text = rank == 0 ? c.label.text.uppercased() : c.label.text
            let font = labelFont(size: size, bold: rank == 0)
            let attr = NSAttributedString(string: text, attributes: [
                NSAttributedString.Key(kCTFontAttributeName as String): font,
                NSAttributedString.Key(kCTForegroundColorAttributeName as String): pal.label,
            ])
            let line = CTLineCreateWithAttributedString(attr)
            let width = CTLineGetTypographicBounds(line, nil, nil, nil)
            let box = CGRect(x: c.x - width / 2 - 4, y: c.y - size, width: width + 8, height: 2 * size)
            if placed.contains(where: { $0.intersects(box) }) { continue }
            placed.append(box)
            g.saveGState()
            // Text draws with y up: flip about the label's baseline.
            g.textMatrix = .identity
            g.translateBy(x: c.x - width / 2, y: c.y + size * 0.35)
            g.scaleBy(x: 1, y: -1)
            g.setLineWidth(3)
            g.setLineJoin(.round)
            g.setStrokeColor(pal.halo)
            g.setTextDrawingMode(.stroke)
            let halo = CTLineCreateWithAttributedString(NSAttributedString(string: text, attributes: [
                NSAttributedString.Key(kCTFontAttributeName as String): font,
                NSAttributedString.Key(kCTForegroundColorFromContextAttributeName as String): true,
            ]))
            g.textPosition = .zero
            CTLineDraw(halo, g)
            g.setTextDrawingMode(.fill)
            g.textPosition = .zero
            CTLineDraw(line, g)
            g.restoreGState()
            if placed.count > 60 { break }
        }
    }

    private var fonts: [String: CTFont] = [:]

    /// The web's mono face (registered by the app at launch), 600 for rank 0 as the web's.
    private func labelFont(size: CGFloat, bold: Bool) -> CTFont {
        let key = "\(size)-\(bold)"
        if let f = fonts[key] { return f }
        let traits: [CFString: Any] = [kCTFontWeightTrait: bold ? 0.3 : 0.0]
        let d = CTFontDescriptorCreateWithAttributes([kCTFontFamilyNameAttribute: MGFonts.monoFamily,
                                                      kCTFontTraitsAttribute: traits] as CFDictionary)
        let f = CTFontCreateWithFontDescriptor(d, size, nil)
        fonts[key] = f
        return f
    }
}

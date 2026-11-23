import Foundation
import MidgardCore
import simd
import SwiftUI

/// A track's marker as placed on screen this frame.
struct MarkerScreen: Equatable, Identifiable {
    let id: String
    let layer: String
    let tone: GlobeItem.Tone
    let point: CGPoint
    /// The ground under an airborne track, for its stem and shadow (nil on the surface).
    let ground: CGPoint?
    let groundVisible: Bool
    /// The marker's heading as it looks on screen, radians clockwise from up; nil unpointed.
    let rotation: Double?
    let visible: Bool
    let labelled: Bool
}

/// The globe's camera and what it draws, as the web's `globe/renderer.ts` keeps them:
/// the view, flights, the floors (50 km, the markers', and with tiles the close-up over
/// the terrain), the sight-line tilt limit, the tile layer and the tile pause, and the
/// markers placed on screen. The Metal renderer draws from it each frame; the SwiftUI
/// view reads its published state. [FR-H-1] [AC-H-1]
@MainActor
final class GlobeController: ObservableObject {
    /// Below this altitude the 1:50m outlines replace the 1:110m ones.
    static let detailBelowKm = 3000.0
    /// Height of the atmosphere shell (1.035 Earth radii): below it the camera sees a sky.
    static let atmosphereKm = 0.035 * Geo.earthRadiusKm
    /// Above this altitude only the selected and hovered tracks keep their labels.
    static let labelsBelowKm = 6000.0
    /// With tiles drawn, the sphere sits this far under sea level (radii; about 640 m).
    static let sphereUnderTiles = 0.9999
    /// With tiles drawn, the outlines are hidden below this altitude.
    static let linesHiddenBelowKm = 150.0
    /// Degrees per point of an orbit drag: tilt (vertical) and turn (horizontal).
    static let tiltPerPx = 0.25
    static let turnPerPx = 0.3

    @Published private(set) var camera = ViewState(lat: 54.6, lon: -4, altKm: 2200)
    /// The camera once a move has settled: the readout and the spoken summary.
    @Published private(set) var settled: ViewState?
    @Published private(set) var markers: [MarkerScreen] = []
    /// nil until the tile layer exists (it never does when the session is offline).
    @Published private(set) var tileState: TileLoader.State?
    @Published private(set) var paused = false
    @Published private(set) var tilesShown = false
    @Published var hovered: String?

    private(set) var items: [GlobeItem] = []
    private(set) var selected: String?
    private(set) var heights = GlobeItems.defaultExaggeration
    private(set) var style: GlobeStyle = .clean
    private(set) var dark = true
    private(set) var tiles: GlobeTiles?
    /// The Metal renderer, once the surface exists.
    weak var renderer: GlobeRenderer?
    private var tilesSeen = false
    private var floorKm = GlobeCamera.minAltitudeKm
    private var flight: (from: ViewState, to: ViewState, start: CFTimeInterval, duration: Double)?
    /// A close-up asked for before any tile was drawn, flown to again once tiles show.
    private var wanted: (target: ViewState, reducedMotion: Bool)?
    private var settleWork: DispatchWorkItem?
    private(set) var viewport = CGSize(width: 1, height: 1)
    /// Asks the view to draw a frame (or keep drawing while something moves).
    var requestRender: () -> Void = {}
    /// The user moved the camera (drag, zoom, tilt, turn, key): resumes paused tiles.
    var onUserMove: () -> Void = {}

    /// What the renderer draws this frame.
    private(set) var viewProj = simd_double4x4(1)
    private(set) var viewRot = simd_double4x4(1)
    private(set) var eye = Vec3(0, 0, 1)
    private(set) var near = 1e-3
    private(set) var showFarLines = true
    private(set) var showNearLines = false
    private(set) var atmosphereInside = false
    private(set) var atmosphereStrength = 0.5
    var sphereScale: Double { tilesShown ? Self.sphereUnderTiles : 1 }

    init() {
        applyCamera()
    }

    // MARK: inputs from the view

    func setViewport(_ size: CGSize) {
        guard size.width > 1, size.height > 1, size != viewport else { return }
        viewport = size
        applyCamera()
    }

    func setItems(_ new: [GlobeItem], heights k: Double) {
        let heightsChanged = k != heights
        heights = k
        if heightsChanged { tiles?.setExaggeration(k) }
        items = new
        // No marker may stand above the camera or inside its near plane.
        let highest = new.map(\.altitudeKm).max().map { $0 * k } ?? 0
        floorKm = max(GlobeCamera.minAltitudeKm, GlobeCamera.cameraFloorKm(highest))
        if camera.altKm < floorKm { camera = zoomed(camera, 1) }
        applyCamera()
    }

    func setSelected(_ id: String?) {
        selected = id
        placeMarkers()
    }

    func setStyle(_ s: GlobeStyle) {
        style = s
        tiles?.setStyle(s)
        requestRender()
    }

    func setDark(_ d: Bool) {
        dark = d
        tiles?.setDark(d)
        renderer?.paintLand(dark: d)
        requestRender()
    }

    /// Starts the tile layer (online only); the renderer gives the Metal device.
    func attachTiles(_ layer: GlobeTiles?) {
        tiles?.destroy()
        tiles = layer
        tilesSeen = false
        tilesShown = false
        if let layer {
            layer.setExaggeration(heights)
            layer.setStyle(style)
            layer.setDark(dark)
            layer.setPaused(paused)
            layer.onChange = { [weak self] in
                guard let self else { return }
                if self.tileState != layer.loader.state { self.tileState = layer.loader.state }
                self.requestRender()
            }
            tileState = layer.loader.state
        } else {
            tileState = nil
        }
        applyCamera()
    }

    func stop() {
        tiles?.destroy()
        tiles = nil
        tileState = nil
        flight = nil
        settleWork?.cancel()
    }

    /// Tile loading paused after a move the tracks decided, resumed by the user's own.
    func moved(_ move: TilePause.Move) {
        let p = TilePause.paused(after: move)
        paused = p
        tiles?.setPaused(p)
        requestRender()
    }

    // MARK: floors

    /// Tiles are drawn, or online and between two sets of them: the close-up stays allowed.
    var closeUpAllowed: Bool { tilesShown || (tilesSeen && tiles?.online == true) }

    var minKm: Double { closeUpAllowed ? GlobeCamera.closeUpAltitudeKm : GlobeCamera.minAltitudeKm }

    /// The terrain's height at the point in the middle of the view, when tiles are drawn.
    func pivotGround(_ v: ViewState) -> Double {
        guard closeUpAllowed, let t = tiles else { return 0 }
        var g = 0.0
        for _ in 0..<2 {
            guard let at = GlobeCamera.lookTarget(v, groundKm: g) else { return g }
            g = t.groundKm(at.lat, at.lon) ?? g
        }
        return g
    }

    private func groundUnderCamera() -> Double {
        closeUpAllowed ? (tiles?.groundKm(camera.lat, camera.lon) ?? 0) : 0
    }

    private func currentFloor() -> Double {
        closeUpAllowed ? GlobeCamera.terrainFloorKm(groundUnderCamera()) : floorKm
    }

    // MARK: moves

    func flyTo(_ target: ViewState, reducedMotion: Bool, retry: Bool = false) {
        let floor = currentFloor()
        wanted = !retry && !tilesShown && tiles != nil && target.altKm < floor ? (target, reducedMotion) : nil
        var t = target
        t.altKm = max(target.altKm, floor)
        let to = GlobeCamera.normalise(t, min: minKm)
        let duration = GlobeCamera.flyDuration(camera, to, reducedMotion: reducedMotion)
        if duration == 0 {
            flight = nil
            camera = to
            applyCamera()
            settle()
            return
        }
        flight = (camera, to, CACurrentMediaTime(), duration)
        requestRender()
    }

    private func userMove() {
        wanted = nil
        onUserMove()
    }

    /// Zoom about the point in view, stopping at the floor.
    private func zoomed(_ v: ViewState, _ factor: Double) -> ViewState {
        let want = max(v.altKm * factor, currentFloor())
        return GlobeCamera.zoom(v, by: want / v.altKm, min: minKm, groundKm: pivotGround(v))
    }

    /// Keyboard and button pan and zoom.
    func nudge(dx: Double, dy: Double, zoom: Double = 1) {
        userMove()
        flight = nil
        camera = zoomed(GlobeCamera.pan(camera, dx: dx, dy: dy, viewportPx: viewport.height, min: minKm), zoom)
        applyCamera()
        settleSoon()
    }

    /// Tilt by `tilt` and turn by `turn` degrees about the point in view.
    func orbit(tilt: Double, turn: Double = 0) {
        userMove()
        flight = nil
        let g = pivotGround(camera)
        camera = GlobeCamera.rotate(GlobeCamera.tilt(camera, by: tilt, min: minKm, groundKm: g), by: turn, min: minKm, groundKm: g)
        applyCamera()
        settleSoon()
    }

    /// Back to north up and straight down, over the point in view.
    func resetOrientation(reducedMotion: Bool) {
        userMove()
        let g = pivotGround(camera)
        let flat = GlobeCamera.rotate(GlobeCamera.tilt(camera, by: -90, min: minKm, groundKm: g), by: -camera.heading,
                                      min: minKm, groundKm: g)
        flyTo(flat, reducedMotion: reducedMotion)
    }

    /// A drag began: any flight stops.
    func beginGesture() {
        flight = nil
        wanted = nil
    }

    /// A one-finger or mouse drag by (dx, dy) points: the ground follows it.
    func drag(dx: Double, dy: Double) {
        userMove()
        camera = GlobeCamera.pan(camera, dx: dx, dy: dy, viewportPx: viewport.height, min: minKm)
        applyCamera()
    }

    /// A right-button, ⌥ or ⌃ drag: tilt (vertical) and turn (horizontal).
    func orbitDrag(dx: Double, dy: Double) {
        userMove()
        let g = pivotGround(camera)
        camera = GlobeCamera.rotate(GlobeCamera.tilt(camera, by: -dy * Self.tiltPerPx, min: minKm, groundKm: g),
                                    by: -dx * Self.turnPerPx, min: minKm, groundKm: g)
        applyCamera()
    }

    /// Pinch (factor < 1 is closer), twist (degrees) and two-finger vertical move (points).
    func pinch(zoom factor: Double, twistDeg: Double = 0, liftPx: Double = 0) {
        userMove()
        if factor != 1, factor > 0 { camera = zoomed(camera, factor) }
        if twistDeg != 0 || liftPx != 0 {
            let g = pivotGround(camera)
            camera = GlobeCamera.rotate(GlobeCamera.tilt(camera, by: liftPx * Self.tiltPerPx, min: minKm, groundKm: g),
                                        by: -twistDeg, min: minKm, groundKm: g)
        }
        applyCamera()
    }

    /// The scroll wheel (or a trackpad's scroll), as the web's `wheel`.
    func wheel(deltaY: Double) {
        userMove()
        flight = nil
        camera = zoomed(camera, exp(deltaY * 0.0015))
        applyCamera()
        settleSoon()
    }

    func endGesture() {
        settle()
    }

    // MARK: per frame

    /// One frame: the flight, the tiles, the markers. Returns whether to keep drawing.
    func frame(now: CFTimeInterval) -> Bool {
        if let f = flight {
            let t = min(1, (now - f.start) / f.duration)
            camera = GlobeCamera.interpolate(f.from, f.to, t, min: minKm)
            if t >= 1 {
                flight = nil
                applyCamera()
                settle()
            } else {
                applyCamera()
            }
        }
        updateTiles()
        placeMarkers()
        return flight != nil
    }

    private func updateTiles() {
        guard let t = tiles else { return }
        t.update(camera, viewportPx: viewport.height, aspect: viewport.width / max(1, viewport.height))
        let shown = t.active
        if shown { tilesSeen = true }
        if shown != tilesShown {
            tilesShown = shown
            applyCamera()
            settle()
            // A close-up held at the floor until now: on down to it.
            let w = wanted
            wanted = nil
            if shown, let w { flyTo(w.target, reducedMotion: w.reducedMotion, retry: true) }
        } else if shown && camera.altKm < currentFloor() {
            applyCamera()
            settle()
        }
    }

    func applyCamera() {
        let floor = currentFloor()
        if camera.altKm < floor { camera.altKm = floor }
        camera = GlobeCamera.normalise(camera, min: minKm)
        if closeUpAllowed, let t = tiles, camera.pitch > 0 {
            let limit = GlobeCamera.sightPitchLimitDeg(camera) { t.groundKm($0, $1) ?? 0 }
            if limit < camera.pitch { camera.pitch = limit }
        }
        let clearance = closeUpAllowed ? camera.altKm - groundUnderCamera() : .infinity
        let planes = GlobeCamera.clipPlanes(camera.altKm, clearanceKm: clearance, markerKm: nearestMarkerKm())
        near = planes.near
        let f = GlobeCamera.frame(camera)
        eye = f.position
        viewRot = Self.lookRotation(forward: f.forward, up: f.up)
        let aspect = viewport.width / max(1, viewport.height)
        viewProj = Self.perspective(fovDeg: GlobeCamera.fovDeg, aspect: aspect, near: near) * viewRot * Self.translation(-eye)
        let closeIn = camera.altKm < Self.detailBelowKm
        let linesOff = tilesShown && camera.altKm < Self.linesHiddenBelowKm
        showNearLines = !linesOff && closeIn
        showFarLines = !linesOff && !closeIn
        atmosphereInside = camera.altKm < Self.atmosphereKm
        atmosphereStrength = atmosphereInside ? 0.42 : min(0.55, max(0.2, (camera.altKm - Self.atmosphereKm) / 600))
        placeMarkers()
        requestRender()
    }

    private func nearestMarkerKm() -> Double {
        let cam = Geo.toXyz(camera.lat, camera.lon, camera.altKm)
        var best = Double.infinity
        for i in items {
            let p = markerPosition(i)
            if Geo.isOccluded(cam: cam, p) { continue }
            best = min(best, simd_length(p - cam))
        }
        return best * Geo.earthRadiusKm
    }

    /// Where a marker is drawn: its own position, or on the terrain when that is higher.
    private func markerPosition(_ i: GlobeItem) -> Vec3 {
        let ground = groundKm(i)
        let own = i.altitudeKm * heights
        return own > ground ? i.position : Geo.toXyz(i.lat, i.lon, ground)
    }

    private func groundKm(_ i: GlobeItem) -> Double {
        tilesShown ? (tiles?.groundKm(i.lat, i.lon) ?? 0) : 0
    }

    // MARK: projection

    static func translation(_ t: Vec3) -> simd_double4x4 {
        var m = simd_double4x4(1)
        m.columns.3 = SIMD4(t.x, t.y, t.z, 1)
        return m
    }

    /// A camera rotation looking along `forward` with `up` (right-handed, -z forward).
    static func lookRotation(forward: Vec3, up: Vec3) -> simd_double4x4 {
        let z = -simd_normalize(forward)
        let x = simd_normalize(simd_cross(up, z))
        let y = simd_cross(z, x)
        return simd_double4x4(rows: [SIMD4(x.x, x.y, x.z, 0), SIMD4(y.x, y.y, y.z, 0), SIMD4(z.x, z.y, z.z, 0),
                                     SIMD4(0, 0, 0, 1)])
    }

    /// A perspective projection with reversed depth and no far plane (depth 1 at `near`,
    /// 0 at infinity), for Metal's 0–1 clip depth.
    static func perspective(fovDeg: Double, aspect: Double, near: Double) -> simd_double4x4 {
        let f = 1 / tan(fovDeg * Geo.rad / 2)
        return simd_double4x4(columns: (SIMD4(f / aspect, 0, 0, 0), SIMD4(0, f, 0, 0), SIMD4(0, 0, 0, -1),
                                        SIMD4(0, 0, near, 0)))
    }

    /// A point on the ground to screen points, or nil behind the globe or the camera
    /// (the user's layers).
    func surfacePoint(_ lat: Double, _ lon: Double) -> CGPoint? {
        let pos = Geo.toXyz(lat, lon, 0)
        guard !Geo.isOccluded(cam: eye, pos) else { return nil }
        return project(pos)
    }

    /// A point on the globe to screen points, or nil behind the camera.
    func project(_ p: Vec3) -> CGPoint? {
        let c = viewProj * SIMD4(p.x, p.y, p.z, 1)
        guard c.w > 1e-12 else { return nil }
        return CGPoint(x: (c.x / c.w + 1) / 2 * viewport.width, y: (1 - c.y / c.w) / 2 * viewport.height)
    }

    private func placeMarkers() {
        let camPos = eye
        let w = viewport.width, h = viewport.height
        markers = items.map { i in
            let pos = markerPosition(i)
            let hidden = Geo.isOccluded(cam: camPos, pos)
            let p = project(pos)
            var rotation: Double?
            if let heading = i.heading, let p {
                let ahead = pos + Geo.headingTangent(i.lat, i.lon, heading) * 1e-3
                if let a = project(ahead) {
                    let sx = a.x - p.x, sy = a.y - p.y
                    if hypot(sx, sy) > 1e-9 { rotation = atan2(Double(sx), Double(-sy)) }
                }
            }
            var ground: CGPoint?
            var groundVisible = false
            if i.hasStem && i.altitudeKm * heights > groundKm(i) {
                let g = Geo.toXyz(i.lat, i.lon, groundKm(i))
                ground = project(g)
                groundVisible = ground != nil && !Geo.isOccluded(cam: camPos, g)
            }
            let point = p ?? CGPoint(x: -100, y: -100)
            let onScreen = !hidden && p != nil && point.x > -40 && point.x < w + 40 && point.y > -40 && point.y < h + 40
            let named = camera.altKm < Self.labelsBelowKm || i.id == selected || i.id == hovered
            return MarkerScreen(id: i.id, layer: i.layer, tone: i.tone, point: point, ground: ground,
                                groundVisible: groundVisible, rotation: rotation, visible: onScreen, labelled: onScreen && named)
        }
    }

    // MARK: settling

    private func settleSoon() {
        settleWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.settle() }
        settleWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: work)
    }

    private func settle() {
        settleWork?.cancel()
        settled = camera
    }
}

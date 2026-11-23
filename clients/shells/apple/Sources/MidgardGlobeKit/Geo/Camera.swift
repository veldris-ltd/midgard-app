import Foundation
import simd

// The globe camera as data, as the web's `globe/camera.ts` and `globe/view.ts`: where it
// looks from, its limits, drag and zoom, tilt and turn, and the eased fly-to. Pure.
// [FR-H-1]
//
// `lat`, `lon` and `altKm` are the point under the camera and its height, so the
// altitude limits and the floor apply to the camera itself. `pitch` is the angle between
// the view and straight down (0° looks at the Earth's centre, 75° towards the horizon)
// and `heading` the bearing the view faces, clockwise from north. Tilt, turn and zoom
// orbit the point in the middle of the view.

public struct ViewState: Equatable, Sendable {
    public var lat: Double
    public var lon: Double
    public var altKm: Double
    public var pitch: Double
    public var heading: Double

    public init(lat: Double, lon: Double, altKm: Double, pitch: Double = 0, heading: Double = 0) {
        self.lat = lat
        self.lon = lon
        self.altKm = altKm
        self.pitch = pitch
        self.heading = heading
    }

    var point: GeoPoint { GeoPoint(lat: lat, lon: lon) }
}

public enum GlobeCamera {
    /// The floor without map tiles: no closer than this to the surface.
    public static let minAltitudeKm = 50.0
    /// With terrain tiles the camera may come down to this, over the sea or low ground.
    public static let closeUpAltitudeKm = 1.5
    /// ...and stays at least this far above the terrain under it.
    public static let terrainClearanceKm = 0.15
    public static let maxAltitudeKm = 40000.0
    /// Vertical field of view, in degrees.
    public static let fovDeg = 45.0
    /// The most the camera tilts, close in.
    public static let maxPitchDeg = 75.0
    /// The camera keeps clear of the poles, where "north" stops meaning anything.
    static let maxCameraLat = 89.0
    /// Far out, the view's centre stays this far below the horizon.
    static let horizonMarginDeg = 6.0
    /// Clearance kept between the camera and the highest marker below it.
    static let markerClearanceKm = 25.0

    static let rad = Double.pi / 180
    static let kmPerDeg = Double.pi * Geo.earthRadiusKm / 180

    // MARK: limits

    /// Altitude within the limits: 50 km (or `minKm`) up to 40,000 km.
    public static func clampAltitude(_ altKm: Double, min minKm: Double = minAltitudeKm) -> Double {
        if altKm.isNaN { return maxAltitudeKm }
        return Swift.min(maxAltitudeKm, Swift.max(minKm, altKm))
    }

    /// The floor over terrain: the 1.5 km close-up, or 150 m above the ground under the
    /// camera (km times HEIGHTS), whichever is higher.
    public static func terrainFloorKm(_ groundKm: Double) -> Double {
        let g = groundKm.isFinite ? Swift.max(0, groundKm) : 0
        return clampAltitude(Swift.max(closeUpAltitudeKm, g + terrainClearanceKm), min: closeUpAltitudeKm)
    }

    /// The lowest the camera may go: 50 km, or higher when HEIGHTS lifts a marker near it.
    public static func cameraFloorKm(_ highestMarkerKm: Double) -> Double {
        guard highestMarkerKm > 0 else { return minAltitudeKm }
        return clampAltitude(Swift.max(minAltitudeKm, highestMarkerKm + markerClearanceKm))
    }

    /// How high a fly-to a track goes: never below the floor for its drawn marker, and
    /// otherwise the current altitude up to 800 km.
    public static func trackFlyAltitudeKm(_ trackAltitudeKm: Double, heights: Double, current: Double) -> Double {
        let marker = Swift.max(0, trackAltitudeKm) * heights
        let want = current.isFinite ? Swift.min(current, 800) : 800
        return Swift.max(cameraFloorKm(marker), want)
    }

    /// Near and far clip planes, in globe radii, for a camera `altKm` up.
    public static func clipPlanes(_ altKm: Double, clearanceKm: Double = .infinity, markerKm: Double = .infinity)
        -> (near: Double, far: Double) {
        let alt = clampAltitude(altKm, min: closeUpAltitudeKm)
        let nearKm = Swift.min(alt * 0.02, Swift.max(0, clearanceKm) / 2, Swift.max(0, markerKm) / 2)
        return (Swift.max(1e-6, nearKm / Geo.earthRadiusKm), alt / Geo.earthRadiusKm + 2.5)
    }

    public static func wrapHeading(_ h: Double) -> Double {
        guard h.isFinite else { return 0 }
        let w = (h.truncatingRemainder(dividingBy: 360) + 360).truncatingRemainder(dividingBy: 360)
        return w == 360 ? 0 : w
    }

    /// The tilt allowed at an altitude: 75° close in, less far out.
    public static func maxPitchDeg(_ altKm: Double, min minKm: Double = minAltitudeKm) -> Double {
        let h = clampAltitude(altKm, min: minKm) / Geo.earthRadiusKm
        let tangent = asin(1 / (1 + h)) / rad
        return Swift.min(maxPitchDeg, Swift.max(0, tangent - horizonMarginDeg))
    }

    public static func normalise(_ v: ViewState, min minKm: Double = minAltitudeKm) -> ViewState {
        let lat = v.lat.isFinite ? Swift.max(-maxCameraLat, Swift.min(maxCameraLat, v.lat)) : 0
        let lon = v.lon.isFinite ? Geo.wrapLongitude(v.lon) : 0
        let alt = clampAltitude(v.altKm, min: minKm)
        let pitch = v.pitch.isFinite ? Swift.min(maxPitchDeg(alt, min: minKm), Swift.max(0, v.pitch)) : 0
        return ViewState(lat: lat, lon: lon, altKm: alt, pitch: pitch, heading: wrapHeading(v.heading))
    }

    // MARK: the view's frame

    /// Where the camera is (globe radii), which way it looks, and which way is up.
    public static func frame(_ v: ViewState) -> (position: Vec3, forward: Vec3, up: Vec3) {
        let position = Geo.toXyz(v.lat, v.lon, v.altKm)
        let radial = simd_normalize(position)
        let t = Geo.tangents(v.lat, v.lon)
        let h = v.heading * rad, p = v.pitch * rad
        let dir = cos(h) * t.north + sin(h) * t.east
        return (position, -cos(p) * radial + sin(p) * dir, sin(p) * radial + cos(p) * dir)
    }

    /// The pivot's height: the ground there, held below the camera so the view still meets it.
    static func pivotGround(_ altKm: Double, _ groundKm: Double) -> Double {
        groundKm.isFinite ? Swift.min(Swift.max(0, groundKm), 0.8 * altKm) : 0
    }

    /// The point in the middle of the view, on ground `groundKm` high, or nil if the view
    /// misses the globe.
    public static func lookTarget(_ v: ViewState, groundKm: Double = 0) -> GeoPoint? {
        let f = frame(v)
        let o = f.position, d = f.forward
        let r = 1 + pivotGround(v.altKm, groundKm) / Geo.earthRadiusKm
        let b = simd_dot(o, d)
        let c = simd_dot(o, o) - r * r
        let disc = b * b - c
        if disc < 0 { return nil }
        let t = -b - disc.squareRoot()
        if t < 0 { return nil }
        let g = Geo.fromXyz(o + t * d)
        return GeoPoint(lat: g.lat, lon: g.lon)
    }

    static func destination(_ lat: Double, _ lon: Double, distDeg: Double, bearingDeg: Double) -> GeoPoint {
        let p1 = lat * rad, l1 = lon * rad, d = distDeg * rad, b = bearingDeg * rad
        let p2 = asin(sin(p1) * cos(d) + cos(p1) * sin(d) * cos(b))
        let l2 = l1 + atan2(sin(b) * sin(d) * cos(p1), cos(d) - sin(p1) * sin(p2))
        return GeoPoint(lat: p2 / rad, lon: Geo.wrapLongitude(l2 / rad))
    }

    static func bearing(_ a: GeoPoint, _ b: GeoPoint) -> Double {
        let p1 = a.lat * rad, p2 = b.lat * rad, dl = (b.lon - a.lon) * rad
        let y = sin(dl) * cos(p2)
        let x = cos(p1) * sin(p2) - sin(p1) * cos(p2) * cos(dl)
        return wrapHeading(atan2(y, x) / rad)
    }

    /// The camera `altKm` up, tilted `pitch` and facing `heading`, placed so the middle of
    /// its view is `target`. The pitch is held to the limit for the altitude.
    public static func lookingAt(_ target: GeoPoint, altKm: Double, pitch: Double, heading: Double,
                                 min minKm: Double = minAltitudeKm, groundKm: Double = 0) -> ViewState {
        let alt = clampAltitude(altKm, min: minKm)
        let p = Swift.min(maxPitchDeg(alt, min: minKm), Swift.max(0, pitch.isFinite ? pitch : 0))
        let h = wrapHeading(heading)
        let k = (1 + alt / Geo.earthRadiusKm) * sin(p * rad) / (1 + pivotGround(alt, groundKm) / Geo.earthRadiusKm)
        let theta = (asin(Swift.min(1, k)) - p * rad) / rad
        if theta < 1e-12 {
            return normalise(ViewState(lat: target.lat, lon: target.lon, altKm: alt, pitch: p, heading: h), min: minKm)
        }
        var back = h + 180
        var n = destination(target.lat, target.lon, distDeg: theta, bearingDeg: back)
        for _ in 0..<8 {
            let err = Geo.wrapLongitude(h - bearing(n, target))
            if abs(err) < 1e-10 { break }
            back += err
            n = destination(target.lat, target.lon, distDeg: theta, bearingDeg: back)
        }
        return normalise(ViewState(lat: n.lat, lon: n.lon, altKm: alt, pitch: p, heading: h), min: minKm)
    }

    static func pivot(_ v: ViewState, groundKm: Double = 0) -> GeoPoint {
        lookTarget(v, groundKm: groundKm) ?? v.point
    }

    // MARK: moves

    /// Tilt by `dDeg` about the point in view, at the same altitude.
    public static func tilt(_ v: ViewState, by dDeg: Double, min minKm: Double = minAltitudeKm, groundKm: Double = 0) -> ViewState {
        let pitch = Swift.min(maxPitchDeg(v.altKm, min: minKm), Swift.max(0, v.pitch + dDeg))
        return lookingAt(pivot(v, groundKm: groundKm), altKm: v.altKm, pitch: pitch, heading: v.heading,
                         min: minKm, groundKm: groundKm)
    }

    /// Turn by `dDeg` (clockwise) about the point in view.
    public static func rotate(_ v: ViewState, by dDeg: Double, min minKm: Double = minAltitudeKm, groundKm: Double = 0) -> ViewState {
        lookingAt(pivot(v, groundKm: groundKm), altKm: v.altKm, pitch: v.pitch, heading: v.heading + dDeg,
                  min: minKm, groundKm: groundKm)
    }

    /// Zoom by a factor of the altitude, keeping the point in view where it is.
    public static func zoom(_ v: ViewState, by factor: Double, min minKm: Double = minAltitudeKm, groundKm: Double = 0) -> ViewState {
        let alt = clampAltitude(v.altKm * factor, min: minKm)
        guard v.pitch > 0 else {
            var c = v
            c.altKm = alt
            return normalise(c, min: minKm)
        }
        return lookingAt(pivot(v, groundKm: groundKm), altKm: alt, pitch: Swift.min(v.pitch, maxPitchDeg(alt, min: minKm)),
                         heading: v.heading, min: minKm, groundKm: groundKm)
    }

    /// Drag by (dx, dy) points in a viewport `viewportPx` tall, straight down: the ground
    /// under the pointer follows it, and the rate scales with altitude.
    static func panStraight(_ c: ViewState, _ dxPx: Double, _ dyPx: Double, viewportPx: Double, fovDeg: Double,
                            min minKm: Double) -> ViewState {
        let viewKm = 2 * c.altKm * tan(fovDeg * rad / 2)
        let degPerPx = Swift.min(180, viewKm / kmPerDeg) / Swift.max(1, viewportPx)
        let lonScale = Swift.max(0.2, cos(c.lat * rad))
        var out = c
        out.lat = c.lat + dyPx * degPerPx
        out.lon = c.lon - dxPx * degPerPx / lonScale
        return normalise(out, min: minKm)
    }

    /// Drag by (dx, dy) points: the ground follows the pointer in the turned, tilted view.
    public static func pan(_ v: ViewState, dx dxPx: Double, dy dyPx: Double, viewportPx: Double,
                           fovDeg: Double = fovDeg, min minKm: Double = minAltitudeKm) -> ViewState {
        let h = v.heading * rad
        let scale = Swift.min(3, 1 / cos(v.pitch * rad))
        let dx = (dxPx * cos(h) - dyPx * sin(h)) * scale
        let dy = (dxPx * sin(h) + dyPx * cos(h)) * scale
        var c = panStraight(v, dx, dy, viewportPx: viewportPx, fovDeg: fovDeg, min: minKm)
        c.pitch = v.pitch
        c.heading = v.heading
        return normalise(c, min: minKm)
    }

    // MARK: flights

    public static func easeInOutCubic(_ t: Double) -> Double {
        if !(t > 0) { return 0 }
        if t >= 1 { return 1 }
        return t < 0.5 ? 4 * t * t * t : 1 - pow(-2 * t + 2, 3) / 2
    }

    /// The view `t` (0–1, linear time) of the way from `a` to `b`, eased: longitude the
    /// short way round, altitude geometrically, a long flight rising mid-way.
    public static func interpolate(_ a: ViewState, _ b: ViewState, _ t: Double, min minKm: Double = minAltitudeKm) -> ViewState {
        if t <= 0 { return a }
        if t >= 1 { return b }
        let e = easeInOutCubic(t)
        let dLon = Geo.wrapLongitude(b.lon - a.lon)
        let base = exp(log(a.altKm) + (log(b.altKm) - log(a.altKm)) * e)
        let peak = clampAltitude(Geo.angularDistanceDeg(a.point, b.point) * kmPerDeg * 0.6, min: minKm)
        let lift = Swift.max(0, peak - Swift.max(a.altKm, b.altKm)) * sin(Double.pi * e)
        return normalise(ViewState(lat: a.lat + (b.lat - a.lat) * e, lon: a.lon + dLon * e, altKm: base + lift,
                                   pitch: a.pitch + (b.pitch - a.pitch) * e,
                                   heading: a.heading + Geo.wrapLongitude(b.heading - a.heading) * e), min: minKm)
    }

    /// How long a fly-to lasts, in seconds: none under reduced motion, otherwise
    /// 0.6–2.4 s by distance, and a turn or tilt alone still animated.
    public static func flyDuration(_ a: ViewState, _ b: ViewState, reducedMotion: Bool) -> Double {
        if reducedMotion { return 0 }
        let angle = Geo.angularDistanceDeg(a.point, b.point)
        let zoom = abs(log(clampAltitude(b.altKm, min: closeUpAltitudeKm) / clampAltitude(a.altKm, min: closeUpAltitudeKm)))
        var ms = 0.0
        if !(angle < 1e-6 && zoom < 1e-6) {
            let effort = Swift.min(1, angle / 120 + zoom / log(maxAltitudeKm / minAltitudeKm))
            ms = (600 + 1800 * effort).rounded()
        }
        let turn = abs(b.pitch - a.pitch) + abs(Geo.wrapLongitude(b.heading - a.heading))
        if ms == 0 && turn > 0.5 { ms = (600 + 900 * Swift.min(1, turn / 180)).rounded() }
        return ms / 1000
    }

    // MARK: terrain

    /// Whether the sight line from the camera to the point in view clears the terrain.
    static func sightClear(_ v: ViewState, groundAt: (Double, Double) -> Double) -> Bool {
        guard let sea = lookTarget(v) else { return true }
        let g = pivotGround(v.altKm, groundAt(sea.lat, sea.lon))
        let at = lookTarget(v, groundKm: g) ?? sea
        let p = Geo.toXyz(v.lat, v.lon, v.altKm)
        let t = Geo.toXyz(at.lat, at.lon, g)
        let samples = 24
        for i in 1..<samples {
            let f = Double(i) / Double(samples)
            let s = Geo.fromXyz(p + (t - p) * f)
            if groundAt(s.lat, s.lon) + terrainClearanceKm * (1 - f) > s.altKm { return false }
        }
        return true
    }

    /// The most the camera may tilt where it stands without its line of sight to the
    /// point in view dropping below the terrain.
    public static func sightPitchLimitDeg(_ v: ViewState, groundAt: (Double, Double) -> Double) -> Double {
        if !(v.pitch > 0) || sightClear(v, groundAt: groundAt) { return v.pitch }
        var lo = 0.0, hi = v.pitch
        for _ in 0..<14 {
            let mid = (lo + hi) / 2
            var probe = v
            probe.pitch = mid
            if sightClear(probe, groundAt: groundAt) { lo = mid } else { hi = mid }
        }
        return lo
    }
}

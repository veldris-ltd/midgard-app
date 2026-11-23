import Foundation
import simd

// The TOPOGRAPHIC style (`tiles/topo.ts`), the globe's terrain patch for one tile
// (`tiles/mesh.ts`) and which tiles the globe draws and fetches (`tiles/select.ts`).
// Pure. [FR-H-1]

public struct TopoPalette: Sendable {
    public let sea: SIMD3<Double>
    public let seaDeep: SIMD3<Double>
    public let land: [(Double, SIMD3<Double>)]
    public let contour: SIMD3<Double>
    public let contourAlpha: Double
    public let indexAlpha: Double

    public static let light = TopoPalette(
        sea: [196, 216, 228], seaDeep: [150, 182, 204],
        land: [(0, [214, 226, 200]), (200, [205, 218, 178]), (500, [222, 212, 168]), (1000, [204, 180, 140]),
               (1500, [184, 156, 126]), (2500, [214, 206, 198]), (4000, [246, 246, 246])],
        contour: [120, 92, 60], contourAlpha: 0.35, indexAlpha: 0.65)

    public static let dark = TopoPalette(
        sea: [16, 28, 38], seaDeep: [8, 16, 24],
        land: [(0, [34, 42, 34]), (200, [40, 48, 34]), (500, [52, 52, 38]), (1000, [64, 56, 42]),
               (1500, [76, 66, 52]), (2500, [98, 94, 88]), (4000, [130, 128, 124])],
        contour: [222, 196, 140], contourAlpha: 0.22, indexAlpha: 0.45)
}

public enum Topo {
    /// Metres between contours at a tile zoom.
    public static func contourInterval(_ z: Int) -> Double {
        switch z {
        case ...4: return 1000
        case 5...6: return 500
        case 7...8: return 200
        case 9...10: return 100
        case 11...12: return 50
        default: return 20
        }
    }

    /// Vertical exaggeration of the hillshade: relief stays visible from far out.
    public static func shadeZFactor(_ z: Int) -> Double {
        min(8, max(1.5, pow(2, Double(12 - z) / 3)))
    }

    /// Horn's hillshade, 0–1, lit from `azimuth` at `altitude` degrees.
    public static func hillshade(_ h: [Float], _ w: Int, _ rows: Int, metresPerPixel: Double, zFactor: Double,
                                 azimuth: Double = 315, altitude: Double = 45) -> [Float] {
        var out = [Float](repeating: 0, count: w * rows)
        let zenith = (90 - altitude) * Geo.rad
        let az = (360 - azimuth + 90).truncatingRemainder(dividingBy: 360) * Geo.rad
        let cz = cos(zenith), sz = sin(zenith)
        h.withUnsafeBufferPointer { hp in
            @inline(__always) func at(_ x: Int, _ y: Int) -> Double {
                Double(hp[min(rows - 1, max(0, y)) * w + min(w - 1, max(0, x))])
            }
            for y in 0..<rows {
                for x in 0..<w {
                    let a = at(x - 1, y - 1), b = at(x, y - 1), c = at(x + 1, y - 1)
                    let d = at(x - 1, y), f = at(x + 1, y)
                    let g = at(x - 1, y + 1), hh = at(x, y + 1), i = at(x + 1, y + 1)
                    let dzdx = (c + 2 * f + i - (a + 2 * d + g)) / (8 * metresPerPixel)
                    let dzdy = (g + 2 * hh + i - (a + 2 * b + c)) / (8 * metresPerPixel)
                    let slope = atan(zFactor * hypot(dzdx, dzdy))
                    let aspect = atan2(dzdy, -dzdx)
                    let v = cz * cos(slope) + sz * sin(slope) * cos(az - aspect)
                    out[y * w + x] = Float(max(0, v))
                }
            }
        }
        return out
    }

    static func lerp(_ a: SIMD3<Double>, _ b: SIMD3<Double>, _ t: Double) -> SIMD3<Double> {
        (a + (b - a) * t).rounded(.toNearestOrAwayFromZero)
    }

    /// The colour for a height: the sea deepening to 4,000 m, the land by its stops.
    public static func tint(_ p: TopoPalette, _ h: Double) -> SIMD3<Double> {
        if h <= 0 { return lerp(p.sea, p.seaDeep, min(1, -h / 4000)) }
        let s = p.land
        if h <= s[0].0 { return s[0].1 }
        for k in 1..<s.count where h <= s[k].0 {
            return lerp(s[k - 1].1, s[k].1, (h - s[k - 1].0) / (s[k].0 - s[k - 1].0))
        }
        return s[s.count - 1].1
    }

    /// The style's RGBA pixels for a grid of heights, row-major from the north-west.
    public static func pixels(_ h: [Float], _ w: Int, _ rows: Int, metresPerPixel: Double, interval: Double,
                              palette p: TopoPalette, zFactor: Double) -> [UInt8] {
        let shade = hillshade(h, w, rows, metresPerPixel: metresPerPixel, zFactor: zFactor)
        let flat = sin(45 * Geo.rad)
        var out = [UInt8](repeating: 255, count: w * rows * 4)
        for y in 0..<rows {
            for x in 0..<w {
                let k = y * w + x
                let v = Double(h[k])
                var c = tint(p, v)
                if v > 0 {
                    c *= 1 + 1.1 * (Double(shade[k]) - flat)
                    let band = floor(v / interval)
                    let right = x < w - 1 ? floor(max(0, Double(h[k + 1])) / interval) : band
                    let down = y < rows - 1 ? floor(max(0, Double(h[k + w])) / interval) : band
                    if band != right || band != down {
                        let top = max(band, right, down)
                        let a = Int(top) % 5 == 0 ? p.indexAlpha : p.contourAlpha
                        c += (p.contour - c) * a
                    }
                }
                let o = k * 4
                // As a clamped byte array stores them: rounded, held to 0–255.
                out[o] = UInt8(min(255, max(0, c.x.rounded())))
                out[o + 1] = UInt8(min(255, max(0, c.y.rounded())))
                out[o + 2] = UInt8(min(255, max(0, c.z.rounded())))
            }
        }
        return out
    }
}

/// A terrain patch: an n × n grid evenly spaced in Web Mercator, raised by the heights
/// times HEIGHTS (below sea level stays at the sea surface), with a skirt hanging from
/// its edges so neighbouring tiles never show a crack. Positions are relative to the
/// patch's centre, so float stays precise at a 1.5 km close-up.
public struct TerrainPatch {
    public let center: Vec3
    /// Interleaved per vertex: position (3), normal (3), tile uv (2), world uv (2).
    public let vertices: [Float]
    public let indices: [UInt16]
    public let vertexCount: Int
    /// The highest vertex, exaggerated, in km.
    public let maxKm: Double

    public static let floatsPerVertex = 10

    /// How far the skirt hangs below the edge: more for bigger tiles.
    public static func skirtDepthKm(_ t: Tile) -> Double {
        max(0.5, TileMath.equatorKm / pow(2, Double(t.z)) * 0.02)
    }

    public static func build(_ t: Tile, heights: [Float]?, n: Int, exaggeration: Double) -> TerrainPatch {
        let side = n + 1
        let grid = side * side
        precondition(heights == nil || heights!.count == grid, "mesh: expected \(grid) heights")
        let skirt = 4 * n
        let count = grid + skirt
        var lat = [Double](repeating: 0, count: count)
        var lon = [Double](repeating: 0, count: count)
        var alt = [Double](repeating: 0, count: count)
        var u = [Float](repeating: 0, count: count)
        var v = [Float](repeating: 0, count: count)
        var maxKm = 0.0
        let z = Double(t.z)
        for j in 0...n {
            let la = TileMath.tileYToLat(Double(t.y) + Double(j) / Double(n), z)
            for i in 0...n {
                let k = j * side + i
                lat[k] = la
                lon[k] = TileMath.tileXToLon(Double(t.x) + Double(i) / Double(n), z)
                let h = heights.map { max(0, Double($0[k])) } ?? 0
                alt[k] = h / 1000 * exaggeration
                maxKm = max(maxKm, alt[k])
                u[k] = Float(i) / Float(n)
                v[k] = Float(j) / Float(n)
            }
        }
        var ring: [Int] = []
        for i in 0..<n { ring.append(i) }
        for j in 0..<n { ring.append(j * side + n) }
        for i in stride(from: n, to: 0, by: -1) { ring.append(n * side + i) }
        for j in stride(from: n, to: 0, by: -1) { ring.append(j * side) }
        let depth = skirtDepthKm(t)
        for (s, e) in ring.enumerated() {
            let k = grid + s
            lat[k] = lat[e]; lon[k] = lon[e]; alt[k] = alt[e] - depth; u[k] = u[e]; v[k] = v[e]
        }
        let centre = Geo.toXyz(TileMath.tileYToLat(Double(t.y) + 0.5, z), TileMath.tileXToLon(Double(t.x) + 0.5, z))
        var vertices = [Float](repeating: 0, count: count * floatsPerVertex)
        for k in 0..<count {
            let p = Geo.toXyz(lat[k], lon[k], alt[k])
            let r = 1 + alt[k] / Geo.earthRadiusKm
            let o = k * floatsPerVertex
            vertices[o] = Float(p.x - centre.x)
            vertices[o + 1] = Float(p.y - centre.y)
            vertices[o + 2] = Float(p.z - centre.z)
            vertices[o + 3] = Float(p.x / r)
            vertices[o + 4] = Float(p.y / r)
            vertices[o + 5] = Float(p.z / r)
            vertices[o + 6] = u[k]
            vertices[o + 7] = v[k]
            // World uv for the clean style's equirectangular land: north at the top.
            vertices[o + 8] = Float((lon[k] + 180) / 360)
            vertices[o + 9] = Float((90 - lat[k]) / 180)
        }
        var tris: [UInt16] = []
        tris.reserveCapacity(n * n * 6 + skirt * 6)
        for j in 0..<n {
            for i in 0..<n {
                let a = j * side + i, b = a + 1, c = a + side, d = c + 1
                tris += [UInt16(a), UInt16(c), UInt16(b), UInt16(b), UInt16(c), UInt16(d)]
            }
        }
        for s in 0..<skirt {
            let e0 = ring[s], e1 = ring[(s + 1) % skirt]
            let s0 = grid + s, s1 = grid + (s + 1) % skirt
            tris += [UInt16(e0), UInt16(s0), UInt16(e1), UInt16(e1), UInt16(s0), UInt16(s1)]
        }
        return TerrainPatch(center: centre, vertices: vertices, indices: tris, vertexCount: count, maxKm: maxKm)
    }

    /// The ground, in metres, at (u, v) of a drawn patch: at least the surface the patch
    /// draws there between its vertices, and at least the terrain itself. Below sea level
    /// is the sea surface; a patch drawn flat (`grid` nil) is at sea level.
    public static func drawnGroundM(_ grid: HeightGrid?, _ rect: TileRect, n: Int, _ u: Double, _ v: Double) -> Double {
        guard let grid else { return 0 }
        let cu = min(1, max(0, u)), cv = min(1, max(0, v))
        let i = min(n - 1, Int(floor(cu * Double(n)))), j = min(n - 1, Int(floor(cv * Double(n))))
        func at(_ a: Double, _ b: Double) -> Double { Terrarium.heightAt(grid, rect.ox + rect.size * a, rect.oy + rect.size * b) }
        let dn = Double(n)
        return max(0, at(cu, cv), at(Double(i) / dn, Double(j) / dn), at(Double(i + 1) / dn, Double(j) / dn),
                   at(Double(i) / dn, Double(j + 1) / dn), at(Double(i + 1) / dn, Double(j + 1) / dn))
    }
}

public enum TileSelect {
    public struct Options {
        public var minZoom: Int
        public var maxZoom: Int
        /// Most tiles drawn plus wanted.
        public var budget: Int
        public var viewportPx: Double
        public var aspect: Double
        public var fovDeg: Double = GlobeCamera.fovDeg
        public var tilePx: Double = 256
        public var texelPx: Double = 1
    }

    /// The angle at the Earth's centre between the point below the camera and the
    /// farthest ground in view, in degrees.
    public static func groundViewAngleDeg(_ altKm: Double, fovDeg: Double, aspect: Double) -> Double {
        let d = 1 + altKm / Geo.earthRadiusKm
        let half = atan(tan(fovDeg * Geo.rad / 2) * (1 + aspect * aspect).squareRoot())
        let s = d * sin(half)
        if s >= 1 { return acos(1 / d) / Geo.rad }
        return (asin(s) - half) / Geo.rad
    }

    struct Node {
        let t: Tile
        let centreDeg: Double
        let radiusDeg: Double
        let centre: Vec3
    }

    static func node(_ t: Tile, _ cam: GeoPoint) -> Node {
        let b = TileMath.bounds(t)
        let z = Double(t.z)
        let c = GeoPoint(lat: TileMath.tileYToLat(Double(t.y) + 0.5, z), lon: TileMath.tileXToLon(Double(t.x) + 0.5, z))
        let midLon = (b.west + b.east) / 2
        let edge = [GeoPoint(lat: b.north, lon: b.west), GeoPoint(lat: b.north, lon: b.east),
                    GeoPoint(lat: b.south, lon: b.west), GeoPoint(lat: b.south, lon: b.east),
                    GeoPoint(lat: b.north, lon: midLon), GeoPoint(lat: b.south, lon: midLon),
                    GeoPoint(lat: c.lat, lon: b.west), GeoPoint(lat: c.lat, lon: b.east)]
        let radius = edge.map { Geo.angularDistanceDeg(c, $0) }.max() ?? 0
        return Node(t: t, centreDeg: Geo.angularDistanceDeg(cam, c), radiusDeg: radius, centre: Geo.toXyz(c.lat, c.lon))
    }

    /// A quadtree walk from `minZoom`: a tile splits while its texels would look larger
    /// than a screen pixel and all its visible children are ready; until then the parent
    /// is drawn and the children are wanted, within the budget.
    public static func select(_ cam: ViewState, _ o: Options, ready: (Tile) -> Bool) -> (draw: [Tile], want: [Tile]) {
        let d = 1 + cam.altKm / Geo.earthRadiusKm
        let tilted = cam.pitch > 0
        let view = tilted ? acos(1 / d) / Geo.rad : groundViewAngleDeg(cam.altKm, fovDeg: o.fovDeg, aspect: o.aspect)
        let kmPerPxPerKm = 2 * tan(o.fovDeg * Geo.rad / 2) / max(1, o.viewportPx)
        let frame = tilted ? GlobeCamera.frame(cam) : nil
        let halfDiag = atan(tan(o.fovDeg * Geo.rad / 2) * (1 + o.aspect * o.aspect).squareRoot()) + 2 * Geo.rad
        let camPoint = cam.point
        func inCone(_ n: Node) -> Bool {
            guard let frame else { return true }
            let v = n.centre - frame.position
            let dist = simd_length(v)
            let r = n.radiusDeg * Geo.rad
            if dist <= r { return true }
            let angle = acos(max(-1, min(1, simd_dot(v, frame.forward) / dist)))
            return angle - asin(min(1, r / dist)) < halfDiag
        }
        func visible(_ n: Node) -> Bool { n.centreDeg - n.radiusDeg < view && inCone(n) }
        func wantsSplit(_ n: Node) -> Bool {
            if n.t.z >= o.maxZoom { return false }
            let a = max(0, n.centreDeg - n.radiusDeg) * Geo.rad
            let distKm = (d * d + 1 - 2 * d * cos(a)).squareRoot() * Geo.earthRadiusKm
            let lat = min(85, abs(TileMath.tileYToLat(Double(n.t.y) + 0.5, Double(n.t.z))))
            let texelKm = TileMath.equatorKm * cos(lat * Geo.rad) / pow(2, Double(n.t.z)) / o.tilePx
            return texelKm > o.texelPx * max(distKm, 1e-3) * kmPerPxPerKm
        }
        let n0 = 1 << o.minZoom
        var queue: [Node] = []
        for y in 0..<n0 {
            for x in 0..<n0 {
                let n = node(Tile(z: o.minZoom, x: x, y: y), camPoint)
                if visible(n) { queue.append(n) }
            }
        }
        var draw: [Node] = []
        var want: [Node] = []
        var used = queue.count
        while !queue.isEmpty {
            queue.sort { $0.t.z != $1.t.z ? $0.t.z < $1.t.z : $0.centreDeg < $1.centreDeg }
            var next: [Node] = []
            for n in queue {
                let isReady = ready(n.t)
                if !isReady || !wantsSplit(n) {
                    if isReady { draw.append(n) } else { want.append(n) }
                    continue
                }
                let kids = TileMath.children(n.t).map { node($0, camPoint) }.filter(visible)
                if kids.isEmpty {
                    draw.append(n)
                    continue
                }
                if kids.allSatisfy({ ready($0.t) }) {
                    if used + kids.count - 1 > o.budget {
                        draw.append(n)
                    } else {
                        used += kids.count - 1
                        next += kids
                    }
                    continue
                }
                draw.append(n)
                let missing = kids.filter { !ready($0.t) }
                if used + missing.count <= o.budget {
                    used += missing.count
                    want += missing
                }
            }
            queue = next
        }
        want.sort { $0.centreDeg != $1.centreDeg ? $0.centreDeg < $1.centreDeg : $0.t.z < $1.t.z }
        return (draw.map(\.t), want.map(\.t))
    }
}

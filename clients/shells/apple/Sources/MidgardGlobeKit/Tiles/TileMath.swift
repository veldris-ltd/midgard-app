import Foundation

// Web Mercator tile maths ("slippy map" tiles, EPSG:3857, 256 px), shared by the globe's
// terrain and imagery tiles and the MAP view, as the web's `tiles/tileMath.ts`. x runs
// west to east and y north to south, each from 0 to 2^z at zoom z. Pure. [FR-H-1]

public struct Tile: Hashable, Sendable, CustomStringConvertible {
    public let z: Int
    public let x: Int
    public let y: Int

    public init(z: Int, x: Int, y: Int) {
        self.z = z
        self.x = x
        self.y = y
    }

    public var description: String { "\(z)/\(x)/\(y)" }
}

public struct TileBounds: Equatable, Sendable {
    public let west: Double
    public let east: Double
    public let north: Double
    public let south: Double
}

public enum TileMath {
    /// The latitude at which Web Mercator's square world ends.
    public static let maxMercatorLat = 85.0511287798066
    /// Equatorial circumference of the WGS 84 ellipsoid, the Mercator scale, in km.
    public static let equatorKm = 40075.016685578485
    /// The deepest zoom any tile source here serves.
    static let maxTileZoom = 22

    public static func lonToTileX(_ lon: Double, _ z: Double) -> Double { (lon + 180) / 360 * pow(2, z) }

    public static func latToTileY(_ lat: Double, _ z: Double) -> Double {
        let l = max(-maxMercatorLat, min(maxMercatorLat, lat)) * Geo.rad
        return (1 - log(tan(l) + 1 / cos(l)) / Double.pi) / 2 * pow(2, z)
    }

    public static func tileXToLon(_ x: Double, _ z: Double) -> Double { x / pow(2, z) * 360 - 180 }

    public static func tileYToLat(_ y: Double, _ z: Double) -> Double {
        let n = Double.pi - 2 * Double.pi * y / pow(2, z)
        return atan(sinh(n)) / Geo.rad
    }

    /// The tile at zoom `z` holding a point; edges and poles land in the last row or column.
    public static func tileAt(_ lat: Double, _ lon: Double, _ z: Int) -> Tile {
        let n = 1 << z
        let x = min(n - 1, max(0, Int(floor(lonToTileX(lon, Double(z))))))
        let y = min(n - 1, max(0, Int(floor(latToTileY(lat, Double(z))))))
        return Tile(z: z, x: x, y: y)
    }

    public static func bounds(_ t: Tile) -> TileBounds {
        let z = Double(t.z)
        return TileBounds(west: tileXToLon(Double(t.x), z), east: tileXToLon(Double(t.x + 1), z),
                          north: tileYToLat(Double(t.y), z), south: tileYToLat(Double(t.y + 1), z))
    }

    public static func isValid(_ t: Tile) -> Bool {
        guard t.z >= 0, t.z <= maxTileZoom else { return false }
        let n = 1 << t.z
        return t.x >= 0 && t.y >= 0 && t.x < n && t.y < n
    }

    public static func parent(_ t: Tile) -> Tile? {
        t.z == 0 ? nil : Tile(z: t.z - 1, x: t.x >> 1, y: t.y >> 1)
    }

    /// The four children, north-west, north-east, south-west, south-east.
    public static func children(_ t: Tile) -> [Tile] {
        let z = t.z + 1, x = t.x * 2, y = t.y * 2
        return [Tile(z: z, x: x, y: y), Tile(z: z, x: x + 1, y: y), Tile(z: z, x: x, y: y + 1), Tile(z: z, x: x + 1, y: y + 1)]
    }

    /// The tile's ancestor at zoom `z`, or the tile itself when `z` is not above it.
    public static func ancestor(_ t: Tile, at z: Int) -> Tile {
        if z >= t.z { return t }
        let d = t.z - z
        return Tile(z: z, x: t.x >> d, y: t.y >> d)
    }

    /// Where `t` sits inside its ancestor at zoom `z`, in fractions of that ancestor.
    public static func subRect(_ t: Tile, _ z: Int) -> TileRect {
        if z >= t.z { return TileRect(ox: 0, oy: 0, size: 1) }
        let n = Double(1 << (t.z - z))
        let a = ancestor(t, at: z)
        return TileRect(ox: Double(t.x) / n - Double(a.x), oy: Double(t.y) / n - Double(a.y), size: 1 / n)
    }

    /// Ground metres per pixel of a tile at zoom `z` and latitude `lat`.
    public static func metresPerPixel(_ lat: Double, _ z: Int, tilePx: Double = 256) -> Double {
        equatorKm * 1000 * cos(lat * Geo.rad) / (tilePx * pow(2, Double(z)))
    }

    /// The (fractional) zoom at which `spanKm` of ground at `lat` fills `viewportPx` pixels.
    public static func zoomForSpan(_ spanKm: Double, lat: Double, viewportPx: Double, tilePx: Double = 256) -> Double {
        log2(equatorKm * cos(lat * Geo.rad) * viewportPx / (tilePx * spanKm))
    }
}

/// The part of a tile to sample, in fractions of it.
public struct TileRect: Equatable, Sendable {
    public let ox: Double
    public let oy: Double
    public let size: Double
}

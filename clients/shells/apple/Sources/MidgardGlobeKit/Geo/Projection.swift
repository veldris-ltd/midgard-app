import Foundation
import simd

// Projection between geographic positions and the globe's xyz frame, as the web's
// `globe/projection.ts`. [FR-H-1]
//
// The globe is a sphere of radius 1 centred on the origin, right-handed and y-up: the
// north pole on +y, the equator on the xz plane, the prime meridian on +x, and east
// towards -z. Altitude raises the radius in proportion to the Earth's mean radius.

public typealias Vec3 = SIMD3<Double>

/// A position on the globe: latitude and longitude in degrees.
public struct GeoPoint: Equatable, Sendable {
    public var lat: Double
    public var lon: Double

    public init(lat: Double, lon: Double) {
        self.lat = lat
        self.lon = lon
    }
}

public enum Geo {
    /// The Earth's mean radius (IUGG), in kilometres.
    public static let earthRadiusKm = 6371.0088
    static let rad = Double.pi / 180

    /// Latitude and longitude in degrees, altitude in km above the sphere, to xyz.
    public static func toXyz(_ lat: Double, _ lon: Double, _ altKm: Double = 0, radius: Double = 1) -> Vec3 {
        let r = radius * (1 + altKm / earthRadiusKm)
        let phi = lat * rad
        let lambda = lon * rad
        let c = cos(phi)
        return Vec3(r * c * cos(lambda), r * sin(phi), -r * c * sin(lambda))
    }

    /// xyz back to latitude, longitude in (-180, 180] and altitude in km.
    public static func fromXyz(_ p: Vec3, radius: Double = 1) -> (lat: Double, lon: Double, altKm: Double) {
        let horizontal = hypot(p.x, p.z)
        let r = hypot(horizontal, p.y)
        let lat = atan2(p.y, horizontal) / rad
        var lon = atan2(-p.z, p.x) / rad
        if lon <= -180 { lon += 360 }
        return (lat, lon, (r / radius - 1) * earthRadiusKm)
    }

    /// Wraps a longitude into (-180, 180].
    public static func wrapLongitude(_ lon: Double) -> Double {
        let w = (((lon + 180).truncatingRemainder(dividingBy: 360)) + 360).truncatingRemainder(dividingBy: 360) - 180
        return w == -180 ? 180 : w
    }

    /// Whether the globe (radius `radius`) hides `p` from a camera at `cam`: the straight
    /// line between them enters the sphere before it reaches `p`.
    public static func isOccluded(cam: Vec3, _ p: Vec3, radius: Double = 1) -> Bool {
        let d = p - cam
        let a = simd_dot(d, d)
        if a == 0 { return false }
        let b = 2 * simd_dot(cam, d)
        let c = simd_dot(cam, cam) - radius * radius
        let disc = b * b - 4 * a * c
        if disc <= 0 { return false }
        let t = (-b - disc.squareRoot()) / (2 * a)
        return t > 0 && t < 1 - 1e-7
    }

    /// The unit tangent at (lat, lon) pointing along `bearingDeg` (clockwise from north).
    public static func headingTangent(_ lat: Double, _ lon: Double, _ bearingDeg: Double) -> Vec3 {
        let t = tangents(lat, lon)
        let b = bearingDeg * rad
        return cos(b) * t.north + sin(b) * t.east
    }

    /// North and east at a point, as unit tangents in the globe's frame.
    static func tangents(_ lat: Double, _ lon: Double) -> (north: Vec3, east: Vec3) {
        let p = lat * rad, l = lon * rad
        return (Vec3(-sin(p) * cos(l), cos(p), sin(p) * sin(l)), Vec3(-sin(l), 0, -cos(l)))
    }

    /// Great-circle angle between two positions, in degrees.
    public static func angularDistanceDeg(_ a: GeoPoint, _ b: GeoPoint) -> Double {
        let p1 = a.lat * rad
        let p2 = b.lat * rad
        let dp = p2 - p1
        let dl = (b.lon - a.lon) * rad
        let h = pow(sin(dp / 2), 2) + cos(p1) * cos(p2) * pow(sin(dl / 2), 2)
        return 2 * asin(min(1, h.squareRoot())) / rad
    }
}

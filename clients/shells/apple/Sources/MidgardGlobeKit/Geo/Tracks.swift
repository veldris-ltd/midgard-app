import Foundation
import MidgardCore

// The sun, the altitude parser and tracks to globe items, as the web's `globe/sun.ts`,
// `globe/altitude.ts` and `globe/items.ts`. Pure. [FR-H-1] [AC-H-1]

public enum Sun {
    /// The subsolar point: the low-precision solar position from the Astronomical Almanac,
    /// with Greenwich mean sidereal time for the longitude.
    public static func subsolarPoint(_ date: Date) -> GeoPoint {
        let j2000 = 946_728_000.0 // 2000-01-01T12:00:00Z
        let n = (date.timeIntervalSince1970 - j2000) / 86400
        let l = 280.46 + 0.9856474 * n
        let g = (357.528 + 0.9856003 * n) * Geo.rad
        let lambda = (l + 1.915 * sin(g) + 0.02 * sin(2 * g)) * Geo.rad
        let epsilon = (23.439 - 0.0000004 * n) * Geo.rad
        let decl = asin(sin(epsilon) * sin(lambda))
        let ra = atan2(cos(epsilon) * sin(lambda), cos(lambda)) / Geo.rad
        let gmst = 280.46061837 + 360.98564736629 * n
        return GeoPoint(lat: decl / Geo.rad, lon: Geo.wrapLongitude(ra - gmst))
    }
}

public struct Altitude: Equatable, Sendable {
    public enum Trend: Sendable { case level, climbing, descending }
    public let feet: Double
    public let metres: Double
    public let trend: Trend

    public static let feetToMetres = 0.3048

    /// `FL340` is a flight level; `SURFACE`, `GND` and `SFC` are zero; `2500 FT` and
    /// `800 M` are plain heights; a trailing ↑ or ↓ is the trend. Anything else is
    /// unknown (nil), never silently zero.
    public static func parse(_ raw: String) -> Altitude? {
        var s = raw.trimmingCharacters(in: .whitespaces).uppercased()
        var trend = Trend.level
        if let last = s.last, last == "↑" || last == "↓" {
            trend = last == "↑" ? .climbing : .descending
            s = String(s.dropLast()).trimmingCharacters(in: .whitespaces)
        }
        if ["SURFACE", "GND", "SFC"].contains(s) { return Altitude(feet: 0, metres: 0, trend: trend) }
        if s.hasPrefix("FL") {
            let digits = s.dropFirst(2).trimmingCharacters(in: .whitespaces)
            if (1...3).contains(digits.count), digits.allSatisfy(\.isASCII), digits.allSatisfy(\.isNumber),
               let n = Double(digits) {
                return Altitude(feet: n * 100, metres: n * 100 * feetToMetres, trend: trend)
            }
            return nil
        }
        for (unit, isFeet) in [("FT", true), ("M", false)] where s.hasSuffix(unit) {
            let number = s.dropLast(unit.count).trimmingCharacters(in: .whitespaces)
            guard !number.isEmpty, number.allSatisfy({ $0.isASCII && ($0.isNumber || $0 == ".") }),
                  number.first != ".", let n = Double(number) else { return nil }
            return isFeet ? Altitude(feet: n, metres: n * feetToMetres, trend: trend)
                          : Altitude(feet: n / feetToMetres, metres: n, trend: trend)
        }
        return nil
    }
}

/// The sample tracks' positions. The signed release's sample tracks carry only the 2D
/// reference layout's x/y; the web console's Stage 1 build gives the same five tracks
/// `lat`/`lon` over the UK and its seas (`sdk/ts/src/mock/heimdall.ts` on
/// `heimdall_3dmaps`), and these are those values. A track not listed is not placed.
public enum SamplePositions {
    public static let byTrack: [String: GeoPoint] = [
        "E-2214": GeoPoint(lat: 57.35, lon: -4.2),
        "E-2231": GeoPoint(lat: 58.25, lon: -2.3),
        "E-0442": GeoPoint(lat: 56.05, lon: 0.45),
        "E-0871": GeoPoint(lat: 57.55, lon: 1.35),
        "E-1108": GeoPoint(lat: 55.75, lon: -3.05),
    ]

    public static func position(_ id: String) -> GeoPoint? { byTrack[id] }
}

/// What the globe draws for one track. [FR-H-1] [AC-H-1]
public struct GlobeItem: Equatable, Sendable, Identifiable {
    public enum Tone: Sendable { case accent, warn }
    public let id: String
    public let layer: String
    public let tone: Tone
    public let lat: Double
    public let lon: Double
    /// True altitude in km, unexaggerated; 0 when unknown.
    public let altitudeKm: Double
    public let altitudeKnown: Bool
    /// Marker position on the unit globe, at the exaggerated altitude.
    public let position: Vec3
    /// The point on the surface below it.
    public let ground: Vec3
    /// Whether a stem stands from the ground to the marker.
    public let hasStem: Bool
    /// Bearing the marker points along (aircraft and vessels), or nil.
    public let heading: Double?
}

public enum GlobeItems {
    /// HEIGHTS: altitude exaggeration, ×4 by default.
    public static let defaultExaggeration = 4.0
    public static let minExaggeration = 1.0
    public static let maxExaggeration = 20.0

    public static func clampExaggeration(_ x: Double) -> Double {
        guard x.isFinite else { return x > 0 ? maxExaggeration : defaultExaggeration }
        return min(maxExaggeration, max(minExaggeration, x))
    }

    public static func items(_ tracks: [HeimdallTrack], layerOn: (String) -> Bool, exaggeration: Double) -> [GlobeItem] {
        let k = clampExaggeration(exaggeration)
        return tracks.compactMap { t in
            guard let p = SamplePositions.position(t.id), abs(p.lat) <= 90, abs(p.lon) <= 180, layerOn(t.layer) else {
                return nil
            }
            let alt = Altitude.parse(t.alt)
            let altitudeKm = (alt?.metres ?? 0) / 1000
            let position = Geo.toXyz(p.lat, p.lon, altitudeKm * k)
            let pointed = (t.layer == "air" || t.layer == "maritime") && t.hdg.isFinite
            return GlobeItem(id: t.id, layer: t.layer, tone: t.track == "anomalous" ? .warn : .accent,
                             lat: p.lat, lon: p.lon, altitudeKm: altitudeKm, altitudeKnown: alt != nil,
                             position: position, ground: Geo.toXyz(p.lat, p.lon, 0), hasStem: altitudeKm > 0,
                             heading: pointed ? (t.hdg.truncatingRemainder(dividingBy: 360) + 360).truncatingRemainder(dividingBy: 360) : nil)
        }
    }
}

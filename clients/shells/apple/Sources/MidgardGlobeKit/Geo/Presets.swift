import Foundation

// Camera presets, the REGION list and the camera summary for screen readers, as the
// web's `globe/presets.ts`. [FR-H-1]
//
// WORLD, UK & IRELAND, NORTH SEA and CHANNEL are fixed. SECTOR NORTH is fitted to the
// positioned tracks of the picture. WORLD looks straight down; the others open tilted
// 45–60°. With map tiles drawn, SNOWDONIA and SOUTHERN UPLANDS are listed too.

public struct GeoBounds: Equatable, Sendable {
    public let south: Double
    public let north: Double
    public let west: Double
    public let east: Double

    public init(south: Double, north: Double, west: Double, east: Double) {
        self.south = south
        self.north = north
        self.west = west
        self.east = east
    }

    func contains(_ lat: Double, _ lon: Double) -> Bool {
        lat >= south && lat <= north && lon >= west && lon <= east
    }

    var area: Double { (north - south) * (east - west) * cos((north + south) / 2 * Geo.rad) }
}

public struct Region: Equatable, Sendable, Identifiable {
    public let id: String
    public let label: String
    public let camera: ViewState
    /// The area the region names; WORLD has none and never counts as "over" it.
    public let bounds: GeoBounds?
    /// Framed on the tracks' positions (SECTOR NORTH with tracks), not a fixed place:
    /// flying there pauses map tiles (`TilePause`).
    public let fittedToTracks: Bool

    public init(id: String, label: String, camera: ViewState, bounds: GeoBounds?, fittedToTracks: Bool = false) {
        self.id = id
        self.label = label
        self.camera = camera
        self.bounds = bounds
        self.fittedToTracks = fittedToTracks
    }
}

public enum Presets {
    static let kmPerDeg = Double.pi * Geo.earthRadiusKm / 180

    public static let fixed: [Region] = [
        Region(id: "world", label: "WORLD", camera: ViewState(lat: 30, lon: -10, altKm: 20000, pitch: 0, heading: 0), bounds: nil),
        Region(id: "uk-ireland", label: "UK & IRELAND",
               camera: GlobeCamera.lookingAt(GeoPoint(lat: 54.3, lon: -4.5), altKm: 1100, pitch: 45, heading: 0),
               bounds: GeoBounds(south: 49.5, north: 61.0, west: -11.0, east: 2.5)),
        Region(id: "north-sea", label: "NORTH SEA",
               camera: GlobeCamera.lookingAt(GeoPoint(lat: 56.5, lon: 2.5), altKm: 800, pitch: 50, heading: 10),
               bounds: GeoBounds(south: 51.0, north: 61.5, west: -3.5, east: 9.0)),
        Region(id: "channel", label: "CHANNEL",
               camera: GlobeCamera.lookingAt(GeoPoint(lat: 50.2, lon: -1.5), altKm: 350, pitch: 55, heading: 75),
               bounds: GeoBounds(south: 48.5, north: 51.2, west: -6.0, east: 2.0)),
    ]

    /// Close, tilted views of relief, listed only while map tiles are drawn.
    public static let closeUps: [Region] = [
        // Yr Wyddfa from the north-east, up the Llanberis pass.
        Region(id: "snowdonia", label: "SNOWDONIA",
               camera: GlobeCamera.lookingAt(GeoPoint(lat: 53.068, lon: -4.076), altKm: 12, pitch: 60, heading: 215,
                                             min: GlobeCamera.closeUpAltitudeKm),
               bounds: GeoBounds(south: 52.8, north: 53.25, west: -4.35, east: -3.7)),
        // The Moffat hills from the south-west, past the Devil's Beef Tub.
        Region(id: "southern-uplands", label: "SOUTHERN UPLANDS",
               camera: GlobeCamera.lookingAt(GeoPoint(lat: 55.42, lon: -3.38), altKm: 15, pitch: 58, heading: 40,
                                             min: GlobeCamera.closeUpAltitudeKm),
               bounds: GeoBounds(south: 55.1, north: 55.9, west: -4.6, east: -2.3)),
    ]

    static let sectorNorthFallback = Region(
        id: "sector-north", label: "SECTOR NORTH",
        camera: GlobeCamera.lookingAt(GeoPoint(lat: 57.0, lon: -1.5), altKm: 700, pitch: 50, heading: 0),
        bounds: GeoBounds(south: 55.0, north: 59.0, west: -5.5, east: 2.5))

    /// SECTOR NORTH fitted to the tracks that carry a position; a fixed view when none do.
    public static func sectorNorth(_ points: [GeoPoint]) -> Region {
        let pts = points.filter { $0.lat.isFinite && $0.lon.isFinite }
        guard !pts.isEmpty else { return sectorNorthFallback }
        let lats = pts.map(\.lat), lons = pts.map(\.lon)
        let south = lats.min()! - 0.5, north = lats.max()! + 0.5
        let west = lons.min()! - 1, east = lons.max()! + 1
        let lat = (south + north) / 2, lon = (west + east) / 2
        let spanKm = max((north - south) * kmPerDeg, (east - west) * kmPerDeg * cos(lat * Geo.rad))
        let altKm = GlobeCamera.clampAltitude(spanKm * 1.15 / (2 * tan(GlobeCamera.fovDeg * Geo.rad / 2)))
        let camera = GlobeCamera.lookingAt(GeoPoint(lat: lat, lon: lon), altKm: altKm * 0.9, pitch: 50, heading: 0)
        return Region(id: "sector-north", label: "SECTOR NORTH", camera: camera,
                      bounds: GeoBounds(south: south, north: north, west: west, east: east), fittedToTracks: true)
    }

    /// The REGION list: the fixed presets, the close-ups while tiles are drawn, then SECTOR NORTH.
    public static func regions(_ points: [GeoPoint], closeUps withCloseUps: Bool = false) -> [Region] {
        fixed + (withCloseUps ? closeUps : []) + [sectorNorth(points)]
    }

    /// The smallest region whose bounds hold the point, or nil over open globe.
    public static func region(at lat: Double, _ lon: Double, in regions: [Region]) -> Region? {
        var best: Region?
        for r in regions {
            guard let b = r.bounds, b.contains(lat, lon) else { continue }
            if best == nil || b.area < best!.bounds!.area { best = r }
        }
        return best
    }

    /// The region in the middle of the view: below the camera, or ahead of it when tilted.
    public static func regionInView(_ c: ViewState, _ regions: [Region]) -> Region? {
        let t = c.pitch > 0 ? (GlobeCamera.lookTarget(c) ?? c.point) : c.point
        return region(at: t.lat, t.lon, in: regions)
    }

    static func grouped(_ n: Double) -> String {
        let f = NumberFormatter()
        f.numberStyle = .decimal
        f.groupingSeparator = ","
        f.usesGroupingSeparator = true
        f.maximumFractionDigits = 0
        f.locale = Locale(identifier: "en_GB")
        return f.string(from: NSNumber(value: n.rounded())) ?? String(Int(n.rounded()))
    }

    /// The heading as a three-digit bearing.
    public static func bearingText(_ heading: Double) -> String {
        let v = Int(heading.rounded()) % 360
        return String(format: "%03d°", v)
    }

    /// The altitude as the readout shows it: a decimal under 10 km, grouped above.
    public static func altitudeText(_ altKm: Double) -> String {
        altKm < 10 ? String(format: "%.1f", altKm) : grouped(altKm)
    }

    public static func latText(_ lat: Double) -> String { String(format: "%.2f° %@", abs(lat), lat < 0 ? "S" : "N") }
    public static func lonText(_ lon: Double) -> String { String(format: "%.2f° %@", abs(lon), lon < 0 ? "W" : "E") }

    /// One sentence for a screen reader: altitude, position, tilt, heading and region.
    public static func describe(_ c: ViewState, _ regions: [Region]) -> String {
        let region = regionInView(c, regions)?.label ?? "open globe"
        let tilted = Int(c.pitch.rounded()) >= 1
        let look = tilted ? "tilted \(Int(c.pitch.rounded()))°" : "looking straight down"
        let whereText = tilted ? "looking towards \(region)" : "over \(region)"
        return "Camera \(altitudeText(c.altKm)) km above \(latText(c.lat)), \(lonText(c.lon)), \(look), "
            + "heading \(bearingText(c.heading)), \(whereText)."
    }
}

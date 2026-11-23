import CoreGraphics
import Foundation

// What the MAP view draws from one VersaTiles vector tile (OpenStreetMap data in the
// Shortbread schema): fills, lines and place labels, by class, as the web's
// `map/features.ts`. Pure: it returns plain paths in tile units. [FR-H-1]

public enum FillClass: String, CaseIterable, Sendable {
    // Drawn in this order.
    case green, built, bare, water, building
}

public enum LineClass: String, CaseIterable, Sendable {
    // Drawn in this order.
    case water, region, country, ferry, rail, local, minor, major, motorway
}

public struct MapLabel: Equatable, Sendable {
    public let text: String
    public let x: Double
    public let y: Double
    /// Lower draws first and wins collisions.
    public let rank: Int
    public let population: Double
}

public struct MapTileData: @unchecked Sendable {
    public let extent: Double
    /// Each class's polygons as one path, in tile units.
    public let fills: [FillClass: CGPath]
    public let fillCounts: [FillClass: Int]
    public let lines: [LineClass: CGPath]
    public let lineCounts: [LineClass: Int]
    public let labels: [MapLabel]
    /// About how many bytes this holds, for the map's cache budget.
    public let bytes: Int
}

public enum MapFeatures {
    /// The most place labels kept from one tile, highest ranked first.
    public static let maxTileLabels = 500
    /// The most characters of a label's text kept (UTF-16 units, never half a pair).
    public static let maxLabelChars = 200
    /// The most label candidates measured for placement in one frame.
    public static let maxLabelCandidates = 400

    /// A label's text cut to `maxLabelChars` UTF-16 units, without splitting a pair.
    public static func labelText(_ text: String) -> String {
        let units = Array(text.utf16)
        guard units.count > maxLabelChars else { return text }
        var cut = maxLabelChars
        if (0xD800...0xDBFF).contains(units[cut - 1]) { cut -= 1 }
        return String(decoding: units[0..<cut], as: UTF16.self)
    }

    static let green: Set<String> = ["forest", "wood", "park", "grass", "meadow", "heath", "scrub", "nature_reserve",
                                     "golf_course", "village_green", "garden", "recreation_ground", "allotments",
                                     "cemetery", "grave_yard", "wetland", "marsh", "bog", "swamp"]
    static let built: Set<String> = ["residential", "commercial", "industrial", "retail", "railway", "brownfield",
                                     "garages", "military", "quarry", "landfill"]
    static let bare: Set<String> = ["bare_rock", "scree", "shingle", "sand", "beach", "glacier", "mud"]

    public static func classifyFill(_ layer: String, _ p: [String: PropertyValue]) -> FillClass? {
        if layer == "ocean" || layer == "water_polygons" || layer == "dam_polygons" { return .water }
        if layer == "buildings" { return .building }
        if layer == "land" || layer == "sites" {
            let k = p["kind"]?.text ?? ""
            if green.contains(k) { return .green }
            if built.contains(k) { return .built }
            if bare.contains(k) { return .bare }
        }
        return nil
    }

    /// The line class, or nil when the feature is not drawn at zoom `z`.
    public static func classifyLine(_ layer: String, _ p: [String: PropertyValue], z: Int) -> LineClass? {
        let k = p["kind"]?.text ?? ""
        switch layer {
        case "water_lines": return k == "river" || k == "canal" || z >= 12 ? .water : nil
        case "ferries": return .ferry
        case "boundaries":
            if p["maritime"] == .bool(true) || p["maritime"] == .string("true") { return nil }
            let level = p["admin_level"]?.number
            if level == 2 { return .country }
            if level == 4 { return .region }
            return nil
        case "streets":
            if k == "rail" || p["rail"] == .bool(true) { return k == "rail" ? .rail : nil }
            if k == "motorway" { return .motorway }
            if k == "trunk" || k == "primary" { return .major }
            if k == "secondary" || k == "tertiary" { return z >= 9 ? .minor : nil }
            if ["residential", "unclassified", "living_street", "service", "pedestrian"].contains(k) { return z >= 12 ? .local : nil }
            return nil
        default: return nil
        }
    }

    /// A place's label rank at zoom `z`, or nil when it is not labelled there.
    public static func placeRank(_ kind: String, z: Int) -> Int? {
        switch kind {
        case "capital", "state_capital", "city": return 0
        case "town": return 1
        case "village": return z >= 11 ? 2 : nil
        case "suburb", "hamlet": return z >= 13 ? 3 : nil
        default: return nil
        }
    }

    /// A vector tile's fills, lines and labels for the map at zoom `z`, within the limits.
    public static func decode(_ bytes: [UInt8], z: Int, limits: VectorLimits = .standard) throws -> MapTileData {
        let tile = try VectorTile(bytes)
        try tile.check(limits)
        return try extract(tile, z: z)
    }

    public static func extract(_ tile: VectorTile, z: Int) throws -> MapTileData {
        var fills: [FillClass: CGMutablePath] = [:]
        var fillCounts: [FillClass: Int] = [:]
        var lines: [LineClass: CGMutablePath] = [:]
        var lineCounts: [LineClass: Int] = [:]
        var labels: [MapLabel] = []
        var extent = 4096.0
        var bytes = 0
        for layer in tile.layers {
            if layer.extent > 0 { extent = Double(layer.extent) }
            for i in 0..<layer.count {
                let f = try layer.feature(i)
                if f.type == 3, let c = classifyFill(layer.name, f.properties) {
                    let path = fills[c] ?? CGMutablePath()
                    for ring in f.loadGeometry() where ring.count > 1 {
                        path.move(to: CGPoint(x: ring[0].x, y: ring[0].y))
                        for p in ring.dropFirst() { path.addLine(to: CGPoint(x: p.x, y: p.y)) }
                        path.closeSubpath()
                        bytes += 16 * ring.count + 32
                    }
                    fills[c] = path
                    fillCounts[c, default: 0] += 1
                } else if f.type == 2, let c = classifyLine(layer.name, f.properties, z: z) {
                    let path = lines[c] ?? CGMutablePath()
                    for l in f.loadGeometry() where l.count > 1 {
                        path.move(to: CGPoint(x: l[0].x, y: l[0].y))
                        for p in l.dropFirst() { path.addLine(to: CGPoint(x: p.x, y: p.y)) }
                        bytes += 16 * l.count + 32
                        lineCounts[c, default: 0] += 1
                    }
                    lines[c] = path
                } else if f.type == 1, layer.name == "place_labels" {
                    guard let rank = placeRank(f.properties["kind"]?.text ?? "", z: z) else { continue }
                    // Cut here, so no long name ever reaches the text layout.
                    let text = labelText(f.properties["name_en"]?.text ?? f.properties["name"]?.text ?? "")
                    guard !text.isEmpty, let pt = f.loadGeometry().first?.first,
                          pt.x >= 0, pt.y >= 0, pt.x < Double(layer.extent), pt.y < Double(layer.extent) else { continue }
                    labels.append(MapLabel(text: text, x: pt.x, y: pt.y, rank: rank,
                                           population: f.properties["population"]?.number ?? 0))
                }
            }
        }
        labels.sort { $0.rank != $1.rank ? $0.rank < $1.rank : $0.population > $1.population }
        if labels.count > maxTileLabels { labels.removeLast(labels.count - maxTileLabels) }
        for l in labels { bytes += 64 + 2 * l.text.utf16.count }
        return MapTileData(extent: extent, fills: fills.mapValues { $0.copy()! }, fillCounts: fillCounts,
                           lines: lines.mapValues { $0.copy()! }, lineCounts: lineCounts, labels: labels, bytes: bytes)
    }

    public struct PlacedLabel {
        public let label: MapLabel
        public let x: Double
        public let y: Double
    }

    /// The labels worth measuring this frame: those near the viewport from the tiles
    /// drawn, best ranked first, at most `max`. Positions in points.
    public static func labelCandidates(_ tiles: [(data: MapTileData, left: Double, top: Double, size: Double)],
                                       width: Double, height: Double, max: Int = maxLabelCandidates) -> [PlacedLabel] {
        var all: [PlacedLabel] = []
        for t in tiles {
            for l in t.data.labels {
                let x = t.left + l.x / t.data.extent * t.size
                let y = t.top + l.y / t.data.extent * t.size
                if x < -50 || y < -20 || x > width + 50 || y > height + 20 { continue }
                all.append(PlacedLabel(label: l, x: x, y: y))
            }
        }
        all.sort { $0.label.rank != $1.label.rank ? $0.label.rank < $1.label.rank : $0.label.population > $1.label.population }
        if all.count > max { all.removeLast(all.count - max) }
        return all
    }
}

import Foundation

// The tile sources and the allow-list built from them, as the web's `tiles/hosts.ts` and
// `tiles/publicHosts.ts` (the `public` demo build: the three public hosts). Every tile
// request goes only to a URL `allowed` accepts: HTTPS, the exact origin (no other host,
// port or credentials), the source's path, a tile in range, no query or fragment, and
// only the one spelling `url` builds. Pure. [FR-H-1] [NFR-PRV-1]

public enum TileSourceId: String, CaseIterable, Sendable {
    case terrain, imagery, vector
}

public struct TileSource: Sendable {
    public let id: TileSourceId
    /// `https://host`, as a URL's origin writes it.
    public let origin: String
    /// Path up to the tile's own segments, with leading and trailing slashes.
    public let pathPrefix: String
    public enum Order: Sendable { case zxy, zyx }
    public let order: Order
    /// File extension after the last segment, or "" for none.
    public let ext: String
    public let minZoom: Int
    public let maxZoom: Int
}

public enum TileLimits {
    static let mb = 1024 * 1024
    /// The most bytes a tile's body may have (the web's `MAX_TILE_BYTES`); a larger one is
    /// a permanent "missing" tile.
    public static let maxBytes: [TileSourceId: Int] = [.terrain: 1 * mb, .imagery: 2 * mb, .vector: 2 * mb]
    /// Pixel sizes a raster tile may decode to (square); anything else is refused.
    public static let rasterSizes: [TileSourceId: [Int]] = [.terrain: [256, 512], .imagery: [256]]
    /// How long one request, body included, may take.
    public static let fetchTimeout: TimeInterval = 15
    /// The decode time limit per tile.
    public static let decodeTimeout: TimeInterval = 2
}

public struct TileSet: Sendable {
    public let sources: [TileSourceId: TileSource]

    /// The three public hosts: Mapzen terrain on AWS Open Data (terrarium), EOX
    /// Sentinel-2 cloudless 2016 (`s2cloudless_3857`, CC BY 4.0, the only layer used),
    /// and VersaTiles OpenStreetMap vector tiles (Shortbread).
    public static let publicHosts = TileSet(sources: [
        .terrain: TileSource(id: .terrain, origin: "https://s3.amazonaws.com", pathPrefix: "/elevation-tiles-prod/terrarium/",
                             order: .zxy, ext: ".png", minZoom: 0, maxZoom: 15),
        .imagery: TileSource(id: .imagery, origin: "https://tiles.maps.eox.at",
                             pathPrefix: "/wmts/1.0.0/s2cloudless_3857/default/g/", order: .zyx, ext: ".jpg",
                             minZoom: 0, maxZoom: 14),
        .vector: TileSource(id: .vector, origin: "https://tiles.versatiles.org", pathPrefix: "/tiles/osm/",
                            order: .zxy, ext: "", minZoom: 0, maxZoom: 14),
    ])

    /// The hosts requests may go to.
    public var hosts: Set<String> {
        Set(sources.values.compactMap { URL(string: $0.origin)?.host })
    }

    /// The tile's URL. A tile out of range is a programming error: nil.
    public func url(_ id: TileSourceId, _ t: Tile) -> String? {
        guard let s = sources[id], TileMath.isValid(t), t.z >= s.minZoom, t.z <= s.maxZoom else { return nil }
        let path = s.order == .zxy ? "\(t.z)/\(t.x)/\(t.y)" : "\(t.z)/\(t.y)/\(t.x)"
        return "\(s.origin)\(s.pathPrefix)\(path)\(s.ext)"
    }

    /// The source and tile a URL is, when it is exactly an allowed tile request; else nil.
    public func allowed(_ raw: String) -> (TileSourceId, Tile)? {
        guard let comps = URLComponents(string: raw), comps.scheme == "https", comps.user == nil,
              comps.password == nil, comps.query == nil, comps.fragment == nil, comps.port == nil,
              let host = comps.host else { return nil }
        for s in sources.values {
            guard s.origin == "https://\(host)", comps.path.hasPrefix(s.pathPrefix) else { continue }
            var rest = String(comps.path.dropFirst(s.pathPrefix.count))
            if !s.ext.isEmpty {
                guard rest.hasSuffix(s.ext) else { continue }
                rest = String(rest.dropLast(s.ext.count))
            }
            let parts = rest.split(separator: "/", omittingEmptySubsequences: false)
            guard parts.count == 3, parts.allSatisfy({ (1...7).contains($0.count) && $0.allSatisfy { $0.isASCII && $0.isNumber } }),
                  parts[0].count <= 2, let z = Int(parts[0]), let a = Int(parts[1]), let b = Int(parts[2]) else { return nil }
            let t = s.order == .zxy ? Tile(z: z, x: a, y: b) : Tile(z: z, x: b, y: a)
            // Only the one spelling `url` builds: padded numbers, case, dot segments,
            // backslashes and the like do not get through.
            guard let canonical = url(s.id, t), raw == canonical else { return nil }
            return (s.id, t)
        }
        return nil
    }
}

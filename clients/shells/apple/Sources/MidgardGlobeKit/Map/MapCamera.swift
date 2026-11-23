import CoreGraphics
import Foundation

// The MAP view's camera as data, as the web's `map/mapCamera.ts`: Web Mercator at a
// fractional zoom, pan, zoom about a point, and the vector tiles in view; and which tiles
// it draws within a byte cap on what is on screen, as `map/onScreen.ts`. Pure. [FR-H-1]

public struct MapCamera: Equatable, Sendable {
    public var lat: Double
    public var lon: Double
    public var zoom: Double

    public init(lat: Double, lon: Double, zoom: Double) {
        self.lat = lat
        self.lon = lon
        self.zoom = zoom
    }
}

public enum MapView2D {
    public static let minZoom = 3.0
    /// Two levels past the vector tiles' deepest, drawn larger.
    public static let maxZoom = 16.0
    static let tilePx = 256.0

    public static func clamp(_ c: MapCamera) -> MapCamera {
        let zoom = c.zoom.isFinite ? min(maxZoom, max(minZoom, c.zoom)) : minZoom
        let lat = c.lat.isFinite ? max(-TileMath.maxMercatorLat, min(TileMath.maxMercatorLat, c.lat)) : 0
        let lon = c.lon.isFinite ? Geo.wrapLongitude(c.lon) : 0
        return MapCamera(lat: lat, lon: lon, zoom: zoom)
    }

    static func world(_ lat: Double, _ lon: Double, _ zoom: Double) -> CGPoint {
        CGPoint(x: TileMath.lonToTileX(lon, zoom) * tilePx, y: TileMath.latToTileY(lat, zoom) * tilePx)
    }

    public static func project(_ lat: Double, _ lon: Double, _ c: MapCamera, _ w: Double, _ h: Double) -> CGPoint {
        let p = world(lat, lon, c.zoom), o = world(c.lat, c.lon, c.zoom)
        return CGPoint(x: w / 2 + (p.x - o.x), y: h / 2 + (p.y - o.y))
    }

    public static func unproject(_ x: Double, _ y: Double, _ c: MapCamera, _ w: Double, _ h: Double) -> GeoPoint {
        let o = world(c.lat, c.lon, c.zoom)
        return GeoPoint(lat: TileMath.tileYToLat((o.y + y - h / 2) / tilePx, c.zoom),
                        lon: TileMath.tileXToLon((o.x + x - w / 2) / tilePx, c.zoom))
    }

    /// Drag by (dx, dy) points: the ground follows the pointer.
    public static func pan(_ c: MapCamera, _ dx: Double, _ dy: Double, _ w: Double, _ h: Double) -> MapCamera {
        let centre = unproject(w / 2 - dx, h / 2 - dy, c, w, h)
        return clamp(MapCamera(lat: centre.lat, lon: centre.lon, zoom: c.zoom))
    }

    /// Zoom to `zoom` keeping the ground under (px, py) where it is.
    public static func zoom(_ c: MapCamera, to zoom: Double, at px: Double, _ py: Double, _ w: Double, _ h: Double) -> MapCamera {
        let z = clamp(MapCamera(lat: c.lat, lon: c.lon, zoom: zoom)).zoom
        let at = unproject(px, py, c, w, h)
        let p = world(at.lat, at.lon, z)
        let centre = CGPoint(x: p.x - (px - w / 2), y: p.y - (py - h / 2))
        return clamp(MapCamera(lat: TileMath.tileYToLat(centre.y / tilePx, z), lon: TileMath.tileXToLon(centre.x / tilePx, z), zoom: z))
    }

    public struct VisibleTile: Equatable {
        public let tile: Tile
        public let left: Double
        public let top: Double
    }

    /// The tiles covering the viewport at the zoom below the camera's (at most the
    /// source's `maxZoom`), each `size` points across, nearest the centre first.
    public static func visibleTiles(_ c: MapCamera, _ w: Double, _ h: Double, maxZoom: Int) -> (z: Int, size: Double, tiles: [VisibleTile]) {
        let z = max(0, min(maxZoom, Int(floor(c.zoom))))
        let size = tilePx * pow(2, c.zoom - Double(z))
        let n = 1 << z
        let cx = TileMath.lonToTileX(c.lon, Double(z)), cy = TileMath.latToTileY(c.lat, Double(z))
        let x0 = Int(floor(cx - w / 2 / size)), x1 = Int(floor(cx + w / 2 / size))
        let y0 = max(0, Int(floor(cy - h / 2 / size))), y1 = min(n - 1, Int(floor(cy + h / 2 / size)))
        var tiles: [(VisibleTile, Double)] = []
        if y0 <= y1, x0 <= x1 {
            for y in y0...y1 {
                for x in x0...x1 {
                    let left = w / 2 + (Double(x) - cx) * size
                    let top = h / 2 + (Double(y) - cy) * size
                    let d = hypot(left + size / 2 - w / 2, top + size / 2 - h / 2)
                    tiles.append((VisibleTile(tile: Tile(z: z, x: ((x % n) + n) % n, y: y), left: left, top: top), d))
                }
            }
        }
        tiles.sort { $0.1 < $1.1 }
        return (z, size, tiles.map(\.0))
    }

    public static func worldWidthPx(_ zoom: Double) -> Double { tilePx * pow(2, zoom) }

    /// A projected x moved by whole worlds to the copy nearest the viewport's centre.
    public static func nearestCopyX(_ x: Double, _ w: Double, _ zoom: Double) -> Double {
        let world = worldWidthPx(zoom)
        return x - ((x - w / 2) / world).rounded() * world
    }

    /// The map zoom that shows what the globe camera at `altKm` shows, `viewportPx` tall.
    public static func zoomForAltitude(_ altKm: Double, lat: Double, viewportPx: Double) -> Double {
        TileMath.zoomForSpan(2 * altKm * tan(GlobeCamera.fovDeg * Geo.rad / 2), lat: lat, viewportPx: viewportPx)
    }

    /// A REGION's camera on the map: centred on the point the region's globe view looks at.
    /// (The web centres the map on the tilted globe camera's own position, which for UK &
    /// IRELAND lands on northern Spain; the point in view is the region's centre.)
    public static func regionCamera(_ r: Region, viewportPx: Double) -> MapCamera {
        let centre = GlobeCamera.lookTarget(r.camera) ?? r.camera.point
        let zoom = zoomForAltitude(r.camera.altKm, lat: centre.lat, viewportPx: max(200, viewportPx))
        return MapCamera(lat: centre.lat, lon: centre.lon, zoom: min(maxZoom, max(minZoom, zoom)))
    }
}

/// Which vector tiles the map draws and asks for this frame, within a byte cap on what is
/// on screen (the web's `map/onScreen.ts`): each visible tile is drawn from itself or its
/// nearest loaded ancestor; a tile that would take the total over the cap is not used,
/// and once the cap is reached nothing more is requested. [FR-H-1]
public enum MapOnScreen {
    /// The most bytes of decoded vector tiles on screen at once.
    public static let maxBytes = 128 * 1024 * 1024

    public struct Plan<T> {
        public var request: [Tile] = []
        public var draw: [(from: Tile, data: T, up: Int)?] = []
        public var used: [Tile] = []
        public var bytes = 0
        public var capped = false
    }

    /// `get` gives a tile's data, `.some(nil)` when the source has none, nil when not loaded.
    public static func plan<T>(_ tiles: [Tile], get: (Tile) -> T??, bytes: (T) -> Int, maxRequested: Int,
                               maxBytes: Int, ancestorLevels: Int) -> Plan<T> {
        var counted = Set<Tile>()
        var plan = Plan<T>()
        for (i, tile) in tiles.enumerated() {
            var drawn: (from: Tile, data: T, up: Int)?
            var t: Tile? = tile
            var up = 0
            while let cur = t, up <= ancestorLevels {
                defer { up += 1; t = TileMath.parent(cur) }
                guard let loaded = get(cur) else {
                    if up == 0 && !plan.capped && i < maxRequested { plan.request.append(cur) }
                    continue
                }
                guard let d = loaded else {
                    plan.used.append(cur)
                    break
                }
                if !counted.contains(cur) {
                    if plan.bytes + bytes(d) > maxBytes {
                        plan.capped = true
                        continue
                    }
                    counted.insert(cur)
                    plan.bytes += bytes(d)
                    plan.used.append(cur)
                }
                drawn = (cur, d, up)
                break
            }
            plan.draw.append(drawn)
        }
        return plan
    }
}

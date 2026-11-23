import Foundation
import XCTest

@testable import MidgardGlobeKit

/// Tile maths, the terrarium sampler, raster and vector decoding with their limits, the
/// map's features and caps, and the caches, on the web console's three recorded tiles
/// (`Fixtures/CREDITS.md`); no network. Values from the web's `tiles/*.test.ts` and
/// `map/*.test.ts`. [FR-H-1]
final class TileTests: XCTestCase {
    static func fixture(_ name: String) throws -> [UInt8] {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: nil, subdirectory: "Fixtures"))
        return [UInt8](try Data(contentsOf: url))
    }

    private func close(_ a: Double, _ b: Double, _ eps: Double = 1e-6, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertLessThanOrEqual(abs(a - b), eps, "\(a) != \(b)", file: file, line: line)
    }

    // MARK: tile maths

    func testTileMaths() {
        XCTAssertEqual(TileMath.tileAt(0, 0, 1), Tile(z: 1, x: 1, y: 1))
        XCTAssertEqual(TileMath.tileAt(57.35, -4.2, 6), Tile(z: 6, x: 31, y: 19))
        XCTAssertEqual(TileMath.tileAt(90, 180, 3), Tile(z: 3, x: 7, y: 0))
        let b = TileMath.bounds(Tile(z: 0, x: 0, y: 0))
        close(b.west, -180); close(b.east, 180); close(b.north, TileMath.maxMercatorLat)
        for (lat, lon) in [(57.35, -4.2), (-33.9, 151.2), (0.0, 0.0)] {
            close(TileMath.tileYToLat(TileMath.latToTileY(lat, 9), 9), lat, 1e-9)
            close(TileMath.tileXToLon(TileMath.lonToTileX(lon, 9), 9), lon, 1e-9)
        }
        XCTAssertEqual(TileMath.parent(Tile(z: 6, x: 31, y: 19)), Tile(z: 5, x: 15, y: 9))
        XCTAssertNil(TileMath.parent(Tile(z: 0, x: 0, y: 0)))
        XCTAssertEqual(TileMath.children(Tile(z: 1, x: 1, y: 0)).map(\.description), ["2/2/0", "2/3/0", "2/2/1", "2/3/1"])
        XCTAssertEqual(TileMath.ancestor(Tile(z: 8, x: 125, y: 78), at: 6), Tile(z: 6, x: 31, y: 19))
        XCTAssertEqual(TileMath.subRect(Tile(z: 8, x: 125, y: 78), 6), TileRect(ox: 0.25, oy: 0.5, size: 0.25))
        XCTAssertFalse(TileMath.isValid(Tile(z: 2, x: 4, y: 0)))
        XCTAssertFalse(TileMath.isValid(Tile(z: 23, x: 0, y: 0)))
        close(TileMath.metresPerPixel(0, 0), 156_543.033_928, 1e-3)
    }

    // MARK: terrarium (terrarium.test.ts)

    func testTheTerrariumEncodingAndBilinearSampler() throws {
        let px: [UInt8] = [128, 0, 0, 255, 128, 100, 128, 255, 127, 255, 0, 255, 0, 0, 0, 255]
        let g = try Terrarium.decode(px, width: 2, height: 2)
        XCTAssertEqual(g.data, [0, 100.5, -1, -32768])
        XCTAssertThrowsError(try Terrarium.decode([UInt8](repeating: 0, count: 12), width: 2, height: 2))
        let grid = HeightGrid(width: 2, height: 2, data: [0, 100, 200, 300])
        close(Terrarium.heightAt(grid, 0.25, 0.25), 0)
        close(Terrarium.heightAt(grid, 0.75, 0.25), 100)
        close(Terrarium.heightAt(grid, 0.5, 0.5), 150)
        close(Terrarium.heightAt(grid, 0, 0), 0)
        close(Terrarium.heightAt(grid, 1, 1), 300)
        close(Terrarium.heightAt(grid, 0.5, 0.25), 50)
        let whole = Terrarium.resample(grid, TileRect(ox: 0, oy: 0, size: 1), 2)
        XCTAssertEqual(whole.count, 9); XCTAssertEqual(whole[0], 0); XCTAssertEqual(whole[4], 150); XCTAssertEqual(whole[8], 300)
        let quarter = Terrarium.resample(grid, TileRect(ox: 0.5, oy: 0.5, size: 0.5), 1)
        XCTAssertEqual(quarter, [150, 200, 250, 300])
    }

    func testTheRecordedTerrainTileOverTheHighlands() throws {
        let g = try RasterTiles.decodeTerrain(try Self.fixture("terrarium-6-31-19.png"))
        XCTAssertEqual(g.width, 256); XCTAssertEqual(g.height, 256)
        let r = Terrarium.range(g)
        XCTAssertTrue(r.min < -50 && r.min > -3000, "min \(r.min)")
        XCTAssertTrue(r.max > 900 && r.max < 1400, "max \(r.max)")
        // The North Sea off Caithness, at sea; the Cairngorm plateau and Ben Nevis, high.
        XCTAssertLessThan(Terrarium.heightAt(g, 0.822, 0.175), 0)
        XCTAssertGreaterThan(Terrarium.heightAt(g, 0.348, 0.584), 700)
        XCTAssertGreaterThan(Terrarium.heightAt(g, 0.11, 0.673), 700)
        // The drawn ground is never below the terrain, nor below sea level.
        let rect = TileRect(ox: 0, oy: 0, size: 1)
        XCTAssertGreaterThanOrEqual(TerrainPatch.drawnGroundM(g, rect, n: 32, 0.11, 0.673), Terrarium.heightAt(g, 0.11, 0.673))
        XCTAssertEqual(TerrainPatch.drawnGroundM(g, rect, n: 32, 0.822, 0.175), 0)
        XCTAssertEqual(TerrainPatch.drawnGroundM(nil, rect, n: 32, 0.5, 0.5), 0)
    }

    func testAPatchHasItsSkirtAndStaysCentred() {
        let p = TerrainPatch.build(Tile(z: 6, x: 31, y: 19), heights: [Float](repeating: 1000, count: 33 * 33), n: 32, exaggeration: 4)
        XCTAssertEqual(p.vertexCount, 33 * 33 + 4 * 32)
        XCTAssertEqual(p.indices.count, 32 * 32 * 6 + 4 * 32 * 6)
        close(p.maxKm, 4)
        XCTAssertLessThan(abs(Double(p.vertices[0])), 0.1, "positions are relative to the patch's centre")
        XCTAssertEqual(TerrainPatch.skirtDepthKm(Tile(z: 14, x: 0, y: 0)), 0.5)
    }

    func testTopographicColours() {
        XCTAssertEqual(Topo.contourInterval(4), 1000)
        XCTAssertEqual(Topo.contourInterval(12), 50)
        XCTAssertEqual(Topo.contourInterval(14), 20)
        XCTAssertEqual(Topo.shadeZFactor(12), 1.5)
        XCTAssertEqual(Topo.shadeZFactor(0), 8)
        XCTAssertEqual(Topo.tint(.dark, 0), TopoPalette.dark.sea)
        XCTAssertEqual(Topo.tint(.light, 5000), SIMD3(246, 246, 246))
        let flat = Topo.pixels([Float](repeating: 100, count: 16), 4, 4, metresPerPixel: 30, interval: 50, palette: .dark, zFactor: 1.5)
        XCTAssertEqual(flat.count, 64)
        XCTAssertEqual(flat[3], 255)
    }

    // MARK: raster limits (decode.test.ts)

    func testRasterHeadersAreReadAndSizesChecked() throws {
        let png = try Self.fixture("terrarium-6-31-19.png"), jpg = try Self.fixture("s2cloudless-2016-6-31-19.jpg")
        XCTAssertEqual(RasterTiles.header(png), RasterHeader(format: .png, width: 256, height: 256))
        XCTAssertEqual(RasterTiles.header(jpg), RasterHeader(format: .jpeg, width: 256, height: 256))
        XCTAssertNil(RasterTiles.header([0, 1, 2, 3]))
        XCTAssertNoThrow(try RasterTiles.decodeImagery(jpg))
        // A JPEG is not terrain; a 300 px tile is refused before decoding.
        XCTAssertThrowsError(try RasterTiles.checkHeader(.terrain, RasterTiles.header(jpg)))
        var big = png
        big[19] = 0x2C // width 300
        XCTAssertThrowsError(try RasterTiles.checkHeader(.terrain, RasterTiles.header(big))) {
            XCTAssertTrue(String(describing: $0).contains("300 × 256"))
        }
        XCTAssertNoThrow(try RasterTiles.checkSize(.terrain, 512, 512))
        XCTAssertThrowsError(try RasterTiles.checkSize(.imagery, 512, 512))
        XCTAssertThrowsError(try RasterTiles.decodeTerrain(Array(png.prefix(40))))
    }

    // MARK: vector tiles and the map's caps

    func testTheRecordedVectorTileOverTayside() throws {
        let d = try MapFeatures.decode(try Self.fixture("versatiles-osm-8-125-78.mvt"), z: 8)
        XCTAssertEqual(d.extent, 4096)
        XCTAssertGreaterThanOrEqual(d.fillCounts[.water] ?? 0, 80)
        XCTAssertEqual(d.fillCounts[.green], 1)
        XCTAssertGreaterThanOrEqual(d.lineCounts[.major] ?? 0, 3)
        XCTAssertGreaterThanOrEqual(d.lineCounts[.rail] ?? 0, 1)
        XCTAssertNil(d.lineCounts[.minor])
        XCTAssertTrue(d.labels.allSatisfy { $0.x >= 0 && $0.x < 4096 && $0.y >= 0 && $0.y < 4096 })
        XCTAssertEqual(d.labels.prefix(2).map(\.text), ["Forfar", "Blairgowrie and Rattray"])
        XCTAssertFalse(d.labels.contains { ["Dundee", "Perth", "Inverness", "Montrose"].contains($0.text) })
        XCTAssertTrue(d.labels.contains { $0.text == "Pitlochry" })
        XCTAssertGreaterThan(d.bytes, 0)
    }

    func testVectorLimitsRefuseBeforeBuilding() throws {
        let bytes = try Self.fixture("versatiles-osm-8-125-78.mvt")
        XCTAssertThrowsError(try MapFeatures.decode(bytes, z: 8, limits: VectorLimits(featuresPerLayer: 10, features: 100_000, points: 1_000_000)))
        XCTAssertThrowsError(try MapFeatures.decode(bytes, z: 8, limits: VectorLimits(featuresPerLayer: 50_000, features: 50, points: 1_000_000)))
        XCTAssertThrowsError(try MapFeatures.decode(bytes, z: 8, limits: VectorLimits(featuresPerLayer: 50_000, features: 100_000, points: 1000)))
        // A ClosePath repeated a million times in five bytes, and an unknown command.
        XCTAssertThrowsError(try VectorTile.geometryPoints([9, 0, 0, 0xFF, 0xFF, 0x3D][...], budget: 1_000_000))
        XCTAssertThrowsError(try VectorTile.geometryPoints([9, 0, 0, 11, 0, 0][...], budget: 100), "unknown command 3")
        XCTAssertThrowsError(try VectorTile.geometryPoints([0xF9, 0x01, 0, 0][...], budget: 100), "MoveTo count past the bytes")
        XCTAssertEqual(try VectorTile.geometryPoints([9, 0, 0, 18, 2, 2, 2, 2, 15][...], budget: 100), 4)
        XCTAssertThrowsError(try VectorTile([0x1A, 0x05, 0x01]))
    }

    func testLabelCaps() {
        XCTAssertEqual(MapFeatures.maxTileLabels, 500)
        XCTAssertEqual(MapFeatures.maxLabelCandidates, 400)
        XCTAssertEqual(MapFeatures.maxLabelChars, 200)
        XCTAssertEqual(MapFeatures.labelText(String(repeating: "a", count: 300)).count, 200)
        let pairs = String(repeating: "a", count: 199) + "😀😀"
        let cut = MapFeatures.labelText(pairs)
        XCTAssertEqual(cut.utf16.count, 199, "never half a surrogate pair")
        let towns = (0..<1000).map { MapLabel(text: "T\($0)", x: Double($0 % 256), y: 10, rank: 1, population: Double($0)) }
        let data = MapTileData(extent: 256, fills: [:], fillCounts: [:], lines: [:], lineCounts: [:], labels: towns, bytes: 0)
        let picked = MapFeatures.labelCandidates([(data, 0, 0, 256)], width: 256, height: 256)
        XCTAssertEqual(picked.count, 400)
        XCTAssertEqual(picked.first?.label.text, "T999", "the most populous first")
        XCTAssertEqual(MapFeatures.placeRank("city", z: 8), 0)
        XCTAssertNil(MapFeatures.placeRank("village", z: 8))
        XCTAssertEqual(MapFeatures.classifyLine("streets", ["kind": .string("secondary")], z: 9), .minor)
        XCTAssertNil(MapFeatures.classifyLine("boundaries", ["admin_level": .number(2), "maritime": .bool(true)], z: 9))
    }

    func testTheMapsOnScreenCapAndCamera() {
        let tiles = [Tile(z: 8, x: 1, y: 1), Tile(z: 8, x: 2, y: 1), Tile(z: 8, x: 3, y: 1)]
        let plan = MapOnScreen.plan(tiles, get: { t -> Int?? in t.x == 3 ? nil : .some(100) }, bytes: { $0 },
                                    maxRequested: 96, maxBytes: 150, ancestorLevels: 5)
        XCTAssertEqual(plan.bytes, 100)
        XCTAssertTrue(plan.capped)
        XCTAssertNotNil(plan.draw[0]); XCTAssertNil(plan.draw[1])
        XCTAssertTrue(plan.request.isEmpty, "past the cap nothing more is asked for")
        XCTAssertEqual(MapOnScreen.maxBytes, 128 * 1024 * 1024)
        let c = MapCamera(lat: 57, lon: -2, zoom: 6)
        let p = MapView2D.project(57, -2, c, 800, 600)
        close(p.x, 400); close(p.y, 300)
        let back = MapView2D.unproject(500, 200, c, 800, 600)
        let again = MapView2D.project(back.lat, back.lon, c, 800, 600)
        close(again.x, 500, 1e-6); close(again.y, 200, 1e-6)
        XCTAssertEqual(MapView2D.clamp(MapCamera(lat: 0, lon: 0, zoom: 30)).zoom, 16)
        XCTAssertEqual(MapView2D.clamp(MapCamera(lat: 0, lon: 0, zoom: 0)).zoom, 3)
        XCTAssertLessThan(MapView2D.pan(c, 100, 0, 800, 600).lon, -2, "drag right: the view goes west")
        let uk = MapView2D.regionCamera(Presets.fixed[1], viewportPx: 600)
        XCTAssertTrue(uk.lat > 53 && uk.lat < 55.5, "UK & IRELAND is centred on the point it looks at: \(uk.lat)")
    }

    // MARK: caches

    func testTheLruCacheEvictsOldestUnprotectedOnce() {
        var evicted: [String] = []
        let c = LruCache<Int>(capacity: 2, onEvict: { k, _ in evicted.append(k) })
        c.set("a", 1); c.set("b", 2)
        _ = c.get("a")
        c.set("c", 3)
        XCTAssertEqual(evicted, ["b"])
        c.protect = { $0 == "a" }
        c.set("d", 4)
        XCTAssertEqual(evicted, ["b", "c"])
        let w = LruCache<Int>(capacity: 10, maxWeight: 10, weigh: { $0 })
        w.set("x", 6); w.set("y", 6)
        XCTAssertEqual(w.keys, ["y"])
        XCTAssertEqual(w.weight, 6)
    }

    func testTheDiskCacheStaysUnderItsCapLeastRecentlyUsedOut() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("tiles-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: dir) }
        let cache = DiskTileCache(directory: dir, capacity: 3000)
        let t0 = Date(timeIntervalSince1970: 1_000_000)
        cache.write("https://tiles.versatiles.org/tiles/osm/1/0/0", Data(count: 1000), now: t0)
        cache.write("https://tiles.versatiles.org/tiles/osm/1/0/1", Data(count: 1000), now: t0 + 1)
        cache.write("https://tiles.versatiles.org/tiles/osm/1/1/0", Data(count: 1000), now: t0 + 2)
        XCTAssertNotNil(cache.read("https://tiles.versatiles.org/tiles/osm/1/0/0", now: t0 + 3), "used again: now the newest")
        cache.write("https://tiles.versatiles.org/tiles/osm/1/1/1", Data(count: 1000), now: t0 + 4)
        XCTAssertLessThanOrEqual(cache.totalBytes, 3000)
        XCTAssertNil(cache.read("https://tiles.versatiles.org/tiles/osm/1/0/1"), "the least recently used went first")
        XCTAssertNotNil(cache.read("https://tiles.versatiles.org/tiles/osm/1/0/0"))
        // A fresh instance reads the same files and the same cap.
        let again = DiskTileCache(directory: dir, capacity: 3000)
        XCTAssertEqual(again.totalBytes, 3000)
        XCTAssertEqual(DiskTileCache.defaultCapacity, 500 * 1024 * 1024)
        again.clear()
        XCTAssertEqual(again.totalBytes, 0)
    }
}

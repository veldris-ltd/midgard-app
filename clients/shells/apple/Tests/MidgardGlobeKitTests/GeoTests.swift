import Foundation
import XCTest

@testable import MidgardGlobeKit

/// The globe's pure geometry against the web console's own tests and values
/// (`globe/projection.test.ts`, `camera.test.ts`, `view.test.ts`, `presets.test.ts`,
/// `sun.test.ts`, `altitude.test.ts`). [FR-H-1]
final class GeoTests: XCTestCase {
    private func close(_ a: Double, _ b: Double, _ eps: Double = 1e-9, file: StaticString = #filePath, line: UInt = #line) {
        XCTAssertLessThanOrEqual(abs(a - b), eps, "\(a) != \(b)", file: file, line: line)
    }

    // MARK: projection

    func testProjectionFrameAndRoundTrip() {
        let p0 = Geo.toXyz(0, 0)
        close(p0.x, 1); close(p0.y, 0); close(p0.z, 0)
        close(Geo.toXyz(90, 0).y, 1)
        // East is towards -z.
        close(Geo.toXyz(0, 90).z, -1)
        close(Geo.toXyz(0, 0, Geo.earthRadiusKm).x, 2)
        for (lat, lon, alt) in [(57.35, -4.2, 10.4), (-33.9, 151.2, 0.0), (0.0, 180.0, 500.0), (89.0, -179.5, 3.0)] {
            let g = Geo.fromXyz(Geo.toXyz(lat, lon, alt))
            close(g.lat, lat, 1e-9); close(g.lon, lon, 1e-9); close(g.altKm, alt, 1e-6)
        }
        XCTAssertEqual(Geo.wrapLongitude(190), -170)
        XCTAssertEqual(Geo.wrapLongitude(-180), 180)
        XCTAssertEqual(Geo.wrapLongitude(-540), 180)
        close(Geo.angularDistanceDeg(GeoPoint(lat: 0, lon: 0), GeoPoint(lat: 0, lon: 90)), 90)
    }

    func testTheGlobeHidesTheFarSide() {
        let cam = Geo.toXyz(0, 0, 1000)
        XCTAssertFalse(Geo.isOccluded(cam: cam, Geo.toXyz(0, 0)))
        XCTAssertTrue(Geo.isOccluded(cam: cam, Geo.toXyz(0, 180)))
        XCTAssertFalse(Geo.isOccluded(cam: cam, Geo.toXyz(0, 5, 10)))
    }

    // MARK: camera limits (camera.test.ts)

    func testAltitudeLimits() {
        XCTAssertEqual(GlobeCamera.minAltitudeKm, 50)
        XCTAssertEqual(GlobeCamera.closeUpAltitudeKm, 1.5)
        XCTAssertEqual(GlobeCamera.maxAltitudeKm, 40000)
        XCTAssertEqual(GlobeCamera.fovDeg, 45)
        XCTAssertEqual(GlobeCamera.clampAltitude(1), 50)
        XCTAssertEqual(GlobeCamera.clampAltitude(1, min: 1.5), 1.5)
        XCTAssertEqual(GlobeCamera.clampAltitude(1e9), 40000)
        XCTAssertEqual(GlobeCamera.clampAltitude(.nan), 40000)
        let c = GlobeCamera.normalise(ViewState(lat: 95, lon: 190, altKm: 1))
        XCTAssertEqual(c.lat, 89); XCTAssertEqual(c.lon, -170); XCTAssertEqual(c.altKm, 50)
    }

    func testFloors() {
        XCTAssertEqual(GlobeCamera.terrainClearanceKm, 0.15)
        XCTAssertEqual(GlobeCamera.terrainFloorKm(0), 1.5)
        XCTAssertEqual(GlobeCamera.terrainFloorKm(1.2), 1.5)
        close(GlobeCamera.terrainFloorKm(5.38), 5.53)
        XCTAssertEqual(GlobeCamera.terrainFloorKm(.nan), 1.5)
        XCTAssertEqual(GlobeCamera.terrainFloorKm(-3), 1.5)
        XCTAssertEqual(GlobeCamera.cameraFloorKm(0), 50)
        XCTAssertEqual(GlobeCamera.cameraFloorKm(10), 50)
        XCTAssertGreaterThan(GlobeCamera.cameraFloorKm(274), 274)
        XCTAssertLessThanOrEqual(GlobeCamera.cameraFloorKm(1e9), 40000)
        // The near plane stays inside the 150 m clearance close in.
        for ground in [0, 0.5, 1.3, 5.4, 26.9, 177] {
            let alt = GlobeCamera.terrainFloorKm(ground)
            XCTAssertLessThan(GlobeCamera.clipPlanes(alt, clearanceKm: alt - ground).near * Geo.earthRadiusKm, alt - ground)
        }
    }

    func testPanZoomAndTilt() {
        let c = ViewState(lat: 50, lon: 0, altKm: 1000)
        let right = GlobeCamera.pan(c, dx: 100, dy: 0, viewportPx: 800)
        XCTAssertLessThan(right.lon, 0, "drag right: camera goes west")
        close(right.lat, 50)
        XCTAssertGreaterThan(GlobeCamera.pan(c, dx: 0, dy: 100, viewportPx: 800).lat, 50, "drag down: camera goes north")
        XCTAssertEqual(GlobeCamera.zoom(c, by: 0.5).altKm, 500)
        XCTAssertEqual(GlobeCamera.zoom(c, by: 1e-6).altKm, 50)
        XCTAssertEqual(GlobeCamera.zoom(c, by: 1e-6, min: 1.5).altKm, 1.5)
        // The tilt limit: 75° close in, less far out.
        XCTAssertEqual(GlobeCamera.maxPitchDeg(60), 75)
        XCTAssertLessThan(GlobeCamera.maxPitchDeg(20000), 20)
        // Tilting and turning keep the point in view.
        let flat = GlobeCamera.lookingAt(GeoPoint(lat: 56, lon: -3), altKm: 300, pitch: 0, heading: 0)
        let tilted = GlobeCamera.rotate(GlobeCamera.tilt(flat, by: 40), by: 30)
        let t = GlobeCamera.lookTarget(tilted)!
        close(t.lat, 56, 1e-6); close(t.lon, -3, 1e-6)
        close(tilted.pitch, 40, 1e-9); close(tilted.heading, 30, 1e-9)
    }

    func testEasedFlights() {
        XCTAssertEqual(GlobeCamera.easeInOutCubic(0), 0)
        XCTAssertEqual(GlobeCamera.easeInOutCubic(0.5), 0.5)
        XCTAssertEqual(GlobeCamera.easeInOutCubic(1), 1)
        let a = ViewState(lat: 10, lon: 170, altKm: 1000), b = ViewState(lat: 20, lon: -170, altKm: 2000)
        XCTAssertEqual(GlobeCamera.interpolate(a, b, 1), b)
        XCTAssertGreaterThan(abs(GlobeCamera.interpolate(a, b, 0.5).lon), 170, "the short way round")
        let far = GlobeCamera.interpolate(ViewState(lat: 54, lon: -4, altKm: 500), ViewState(lat: -30, lon: 140, altKm: 500), 0.5)
        XCTAssertGreaterThan(far.altKm, 5000, "a long flight rises")
        let x = ViewState(lat: 54, lon: -4, altKm: 500), y = ViewState(lat: 20, lon: 0, altKm: 20000)
        XCTAssertEqual(GlobeCamera.flyDuration(x, y, reducedMotion: true), 0, "reduced motion: no easing")
        let d = GlobeCamera.flyDuration(x, y, reducedMotion: false)
        XCTAssertTrue((0.6...2.4).contains(d), "\(d)")
        XCTAssertEqual(GlobeCamera.flyDuration(x, x, reducedMotion: false), 0)
    }

    // MARK: presets (presets.test.ts)

    private let points = [GeoPoint(lat: 57.35, lon: -4.2), GeoPoint(lat: 58.25, lon: -2.3), GeoPoint(lat: 56.05, lon: 0.45),
                          GeoPoint(lat: 57.55, lon: 1.35), GeoPoint(lat: 55.75, lon: -3.05)]

    func testThePresetsAreTheWebs() {
        XCTAssertEqual(Presets.fixed.map(\.label), ["WORLD", "UK & IRELAND", "NORTH SEA", "CHANNEL"])
        XCTAssertEqual(Presets.regions(points).map(\.label), ["WORLD", "UK & IRELAND", "NORTH SEA", "CHANNEL", "SECTOR NORTH"])
        XCTAssertEqual(Presets.regions(points, closeUps: true).map(\.label),
                       ["WORLD", "UK & IRELAND", "NORTH SEA", "CHANNEL", "SNOWDONIA", "SOUTHERN UPLANDS", "SECTOR NORTH"])
        XCTAssertEqual(Presets.fixed[0].camera, ViewState(lat: 30, lon: -10, altKm: 20000))
        let expected: [String: (Double, Double, Double, Double, Double)] = [
            "uk-ireland": (54.3, -4.5, 1100, 45, 0), "north-sea": (56.5, 2.5, 800, 50, 10), "channel": (50.2, -1.5, 350, 55, 75),
            "snowdonia": (53.068, -4.076, 12, 60, 215), "southern-uplands": (55.42, -3.38, 15, 58, 40),
        ]
        for r in Presets.regions(points, closeUps: true) where r.id != "world" {
            XCTAssertTrue((45...60).contains(r.camera.pitch), "\(r.label): \(r.camera.pitch)")
            let t = GlobeCamera.lookTarget(r.camera)!
            let b = r.bounds!
            XCTAssertTrue(t.lat > b.south && t.lat < b.north && t.lon > b.west && t.lon < b.east, r.label)
            if let e = expected[r.id] {
                close(t.lat, e.0, 1e-6); close(t.lon, e.1, 1e-6)
                XCTAssertEqual(r.camera.altKm, e.2); XCTAssertEqual(r.camera.pitch, e.3); close(r.camera.heading, e.4, 1e-9)
            }
        }
        let s = Presets.sectorNorth(points)
        XCTAssertTrue(s.fittedToTracks)
        let t = GlobeCamera.lookTarget(s.camera)!
        XCTAssertTrue(t.lat > 55.75 && t.lat < 58.25 && t.lon > -4.2 && t.lon < 1.35)
        XCTAssertTrue(s.camera.altKm >= 50 && s.camera.altKm < 2000)
        XCTAssertFalse(Presets.sectorNorth([]).fittedToTracks)
        let regions = Presets.regions(points)
        XCTAssertEqual(Presets.region(at: 57.5, 0.5, in: regions)?.label, "SECTOR NORTH")
        XCTAssertEqual(Presets.region(at: 50.2, -1.5, in: regions)?.label, "CHANNEL")
        XCTAssertEqual(Presets.region(at: 53.0, -8.0, in: regions)?.label, "UK & IRELAND")
        XCTAssertEqual(Presets.region(at: 54.0, 4.5, in: regions)?.label, "NORTH SEA")
        XCTAssertNil(Presets.region(at: -30, 140, in: regions))
    }

    func testTheCameraSummaryReadsAsTheWebs() {
        let regions = Presets.regions(points)
        XCTAssertEqual(Presets.describe(ViewState(lat: 50.25, lon: -1.5, altKm: 600), regions),
                       "Camera 600 km above 50.25° N, 1.50° W, looking straight down, heading 000°, over CHANNEL.")
        XCTAssertEqual(Presets.describe(ViewState(lat: -30, lon: 140, altKm: 20000), regions),
                       "Camera 20,000 km above 30.00° S, 140.00° E, looking straight down, heading 000°, over open globe.")
        XCTAssertEqual(Presets.describe(ViewState(lat: 56.8, lon: -5, altKm: 1.54), regions),
                       "Camera 1.5 km above 56.80° N, 5.00° W, looking straight down, heading 000°, over SECTOR NORTH.")
        let channel = Presets.describe(regions.first { $0.id == "channel" }!.camera, regions)
        XCTAssertTrue(channel.hasPrefix("Camera 350 km above "), channel)
        XCTAssertTrue(channel.hasSuffix(", tilted 55°, heading 075°, looking towards CHANNEL."), channel)
    }

    // MARK: sun, altitude, items

    func testTheSubsolarPoint() {
        // The June solstice of 2026 (21 June, 08:24 UTC): the sun over the Tropic of Cancer.
        let solstice = Sun.subsolarPoint(Date(timeIntervalSince1970: 1_782_030_240))
        close(solstice.lat, 23.44, 0.05)
        // Noon at Greenwich on an equinox: the sun near 0° longitude and the equator.
        let equinox = Sun.subsolarPoint(Date(timeIntervalSince1970: 1_774_094_400)) // 2026-03-21 12:00 UTC
        close(equinox.lat, 0, 0.5)
        close(equinox.lon, 0, 2.5)
    }

    func testAltitudesParseAsTheWebs() {
        XCTAssertEqual(Altitude.parse("FL340")?.feet, 34000)
        XCTAssertEqual(Altitude.parse("FL212 ↓")?.trend, .descending)
        XCTAssertEqual(Altitude.parse("FL212 ↓")?.feet, 21200)
        XCTAssertEqual(Altitude.parse("SURFACE")?.metres, 0)
        XCTAssertEqual(Altitude.parse("GND")?.metres, 0)
        XCTAssertEqual(Altitude.parse("800 M")?.metres, 800)
        XCTAssertEqual(Altitude.parse("2500 FT")?.metres, 762)
        XCTAssertNil(Altitude.parse("HIGH"))
        XCTAssertNil(Altitude.parse("FL"))
        XCTAssertEqual(GlobeItems.clampExaggeration(0), 1)
        XCTAssertEqual(GlobeItems.clampExaggeration(99), 20)
        XCTAssertEqual(GlobeItems.defaultExaggeration, 4)
    }
}

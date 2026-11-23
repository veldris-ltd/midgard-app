import Foundation

// The bundled Natural Earth outlines (v5.1.2, public domain), decoded, as the web's
// `globe/outlines.ts` and `globe/geometry.ts`. The two files are the web console's own
// (`data/ne-110m.json`, `data/ne-50m.json`, built by `tools/heimdall-outlines/build.mjs`
// from pinned downloads): rings and lines quantised and delta-encoded as
// [lon0, lat0, dlon1, dlat1, ...]. Decoded lines are flat [lon, lat, ...] in degrees.
// [FR-H-1]

public struct OutlineFile: Decodable, Sendable {
    public let format: String
    public let quantum: Double
    public let scale: String
    public let coast: [[Int]]?
    public let borders: [[Int]]?
    /// Polygons, each a list of rings (outer first).
    public let land: [[[Int]]]?

    public static let expectedFormat = "midgard-heimdall-outlines/1"
}

public struct GlobeOutlines: Sendable {
    /// 1:110m coastlines and borders.
    public let world: OutlineFile
    /// 1:50m land and borders.
    public let close: OutlineFile

    public enum LoadError: Error { case missing(String), format(String) }

    /// The two files from this library's resources: no network.
    public static func bundled() throws -> GlobeOutlines {
        GlobeOutlines(world: try load("ne-110m"), close: try load("ne-50m"))
    }

    static func load(_ name: String) throws -> OutlineFile {
        guard let url = Bundle.module.url(forResource: name, withExtension: "json", subdirectory: "Outlines")
            ?? Bundle.module.url(forResource: name, withExtension: "json") else { throw LoadError.missing(name) }
        let file = try JSONDecoder().decode(OutlineFile.self, from: Data(contentsOf: url))
        guard file.format == OutlineFile.expectedFormat else { throw LoadError.format(name) }
        return file
    }

    /// Loaded once per process, off the main thread on first use.
    nonisolated(unsafe) static var cached: GlobeOutlines?
    static let lock = NSLock()
    public static func shared() -> GlobeOutlines? {
        lock.lock()
        defer { lock.unlock() }
        if cached == nil { cached = try? bundled() }
        return cached
    }
}

public enum Outlines {
    static func steps(_ quantum: Double) -> Double { (1 / quantum).rounded() }

    static func integers(_ enc: [Int]) -> [Int] {
        var out = [Int](repeating: 0, count: enc.count)
        var x = 0, y = 0
        var i = 0
        while i + 1 < enc.count {
            x += enc[i]
            y += enc[i + 1]
            out[i] = x
            out[i + 1] = y
            i += 2
        }
        return out
    }

    public static func decodeLine(_ enc: [Int], quantum: Double) -> [Double] {
        let n = steps(quantum)
        return integers(enc).map { Double($0) / n }
    }

    /// Coastlines from land polygons: each ring, less the edges Natural Earth adds where a
    /// polygon is cut along the antimeridian or closed along the south pole.
    public static func coastFromLand(_ polygons: [[[Int]]], quantum: Double) -> [[Double]] {
        let n = steps(quantum)
        let meridian = Int(180 * n)
        let pole = Int(-90 * n) + 1
        var out: [[Double]] = []
        for poly in polygons {
            for enc in poly {
                let p = integers(enc)
                let count = p.count / 2
                guard count > 0 else { continue }
                func cut(_ i: Int) -> Bool {
                    let ax = p[2 * i], ay = p[2 * i + 1], bx = p[2 * i + 2], by = p[2 * i + 3]
                    return (abs(ax) == meridian && bx == ax) || (ay <= pole && by <= pole)
                }
                var runs: [[Int]] = []
                var run = [0]
                for i in 0..<max(0, count - 1) {
                    if cut(i) {
                        if run.count > 1 { runs.append(run) }
                        run = [i + 1]
                    } else {
                        run.append(i + 1)
                    }
                }
                if run.count > 1 { runs.append(run) }
                let closed = count > 2 && p[0] == p[2 * count - 2] && p[1] == p[2 * count - 1]
                if closed, runs.count > 1, runs[0][0] == 0, runs[runs.count - 1].last == count - 1 {
                    let last = runs.removeLast()
                    runs[0] = last + runs[0].dropFirst()
                }
                for r in runs {
                    var line = [Double](repeating: 0, count: r.count * 2)
                    for (k, idx) in r.enumerated() {
                        line[2 * k] = Double(p[2 * idx]) / n
                        line[2 * k + 1] = Double(p[2 * idx + 1]) / n
                    }
                    out.append(line)
                }
            }
        }
        return out
    }

    /// Flat [lon, lat, ...] lines to segment pairs on a sphere of `radius`, a segment
    /// longer than `maxStepDeg` split so it follows the curve.
    public static func lineSegments(_ lines: [[Double]], radius: Double, maxStepDeg: Double) -> [Float] {
        var out: [Float] = []
        func push(_ lon: Double, _ lat: Double) {
            let p = Geo.toXyz(lat, lon, 0, radius: radius)
            out.append(Float(p.x)); out.append(Float(p.y)); out.append(Float(p.z))
        }
        for l in lines {
            var i = 0
            while i + 3 < l.count {
                let lon0 = l[i], lat0 = l[i + 1], lon1 = l[i + 2], lat1 = l[i + 3]
                let steps = max(1, Int((max(abs(lon1 - lon0), abs(lat1 - lat0)) / maxStepDeg).rounded(.up)))
                for s in 0..<steps {
                    let a = Double(s) / Double(steps), b = Double(s + 1) / Double(steps)
                    push(lon0 + (lon1 - lon0) * a, lat0 + (lat1 - lat0) * a)
                    push(lon0 + (lon1 - lon0) * b, lat0 + (lat1 - lat0) * b)
                }
                i += 2
            }
        }
        return out
    }

    /// Meridians and parallels every `everyDeg`, sampled every `stepDeg`; none at the poles.
    public static func graticule(everyDeg: Double, stepDeg: Double) -> [[Double]] {
        var lines: [[Double]] = []
        var lon = -180.0
        while lon < 180 {
            var pts: [Double] = []
            var lat = -90 + everyDeg
            while lat <= 90 - everyDeg + 1e-9 {
                pts.append(lon); pts.append(lat)
                lat += stepDeg
            }
            pts.append(lon); pts.append(90 - everyDeg)
            lines.append(pts)
            lon += everyDeg
        }
        var lat = -90 + everyDeg
        while lat < 90 - 1e-9 {
            var pts: [Double] = []
            var l = -180.0
            while l <= 180 {
                pts.append(l); pts.append(lat)
                l += stepDeg
            }
            lines.append(pts)
            lat += everyDeg
        }
        return lines
    }
}

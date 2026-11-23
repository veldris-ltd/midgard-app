import Foundation

// Mapbox vector tiles (MVT 2.1, protocol buffers), read with limits, as the web's
// `tiles/decode.ts` does with `@mapbox/vector-tile`: the features in each layer and in
// the tile, and the points of every geometry, are counted from the encoded bytes before
// any geometry is built (a repeated ClosePath can claim millions of points in a few
// bytes); past a limit, or with a malformed geometry, the tile is refused. [FR-H-1]

public struct VectorLimits: Sendable {
    public let featuresPerLayer: Int
    public let features: Int
    public let points: Int

    /// The web's `VECTOR_LIMITS`: generous for real OpenStreetMap tiles, far below what
    /// would stall the decoder.
    public static let standard = VectorLimits(featuresPerLayer: 50_000, features: 100_000, points: 1_000_000)
}

public enum PropertyValue: Equatable, Sendable {
    case string(String), number(Double), bool(Bool)

    var text: String {
        switch self {
        case let .string(s): return s
        case let .number(n): return n == n.rounded() && abs(n) < 1e15 ? String(Int(n)) : String(n)
        case let .bool(b): return b ? "true" : "false"
        }
    }

    var number: Double? {
        switch self {
        case let .number(n): return n
        case let .string(s): return Double(s)
        case let .bool(b): return b ? 1 : 0
        }
    }
}

public struct VTFeature {
    public let type: Int
    public let properties: [String: PropertyValue]
    let geometry: ArraySlice<UInt8>

    /// The geometry as rings or lines of (x, y) tile units, as `loadGeometry` builds it.
    /// Called only for geometry already counted and checked.
    public func loadGeometry() -> [[(x: Double, y: Double)]] {
        var r = ProtoReader(geometry)
        var lines: [[(x: Double, y: Double)]] = []
        var line: [(x: Double, y: Double)]?
        var x = 0, y = 0
        while !r.atEnd {
            guard let cmdLen = try? r.varint() else { break }
            let cmd = Int(cmdLen & 7)
            let count = Int(cmdLen >> 3)
            if cmd == 1 || cmd == 2 {
                for _ in 0..<count {
                    guard let dx = try? r.svarint(), let dy = try? r.svarint() else { return lines + (line.map { [$0] } ?? []) }
                    x += dx
                    y += dy
                    if cmd == 1 {
                        if let l = line { lines.append(l) }
                        line = []
                    }
                    line?.append((Double(x), Double(y)))
                }
            } else if cmd == 7 {
                if let l = line, let first = l.first { line?.append(first) }
            } else {
                break
            }
        }
        if let l = line { lines.append(l) }
        return lines
    }
}

public struct VTLayer {
    public let name: String
    public let extent: Int
    let featureBytes: [ArraySlice<UInt8>]
    let keys: [String]
    let values: [PropertyValue?]

    public var count: Int { featureBytes.count }

    func rawFeature(_ i: Int) throws -> (type: Int, tags: [Int], geometry: ArraySlice<UInt8>) {
        var r = ProtoReader(featureBytes[i])
        var type = 0
        var tags: [Int] = []
        var geometry = ArraySlice<UInt8>()
        while !r.atEnd {
            let (field, wire) = try r.key()
            switch (field, wire) {
            case (2, 2):
                var p = ProtoReader(try r.bytes())
                while !p.atEnd { tags.append(Int(try p.varint())) }
            case (3, 0): type = Int(try r.varint())
            case (4, 2): geometry = try r.bytes()
            default: try r.skip(wire)
            }
        }
        return (type, tags, geometry)
    }

    public func feature(_ i: Int) throws -> VTFeature {
        let raw = try rawFeature(i)
        var props: [String: PropertyValue] = [:]
        var k = 0
        while k + 1 < raw.tags.count {
            let ki = raw.tags[k], vi = raw.tags[k + 1]
            if ki < keys.count, vi < values.count, let v = values[vi] { props[keys[ki]] = v }
            k += 2
        }
        return VTFeature(type: raw.type, properties: props, geometry: raw.geometry)
    }
}

public struct VectorTile {
    public let layers: [VTLayer]

    public init(_ bytes: [UInt8]) throws {
        var r = ProtoReader(bytes[...])
        var layers: [VTLayer] = []
        while !r.atEnd {
            let (field, wire) = try r.key()
            if field == 3, wire == 2 {
                layers.append(try Self.layer(try r.bytes()))
            } else {
                try r.skip(wire)
            }
        }
        self.layers = layers
    }

    static func layer(_ bytes: ArraySlice<UInt8>) throws -> VTLayer {
        var r = ProtoReader(bytes)
        var name = ""
        var extent = 4096
        var features: [ArraySlice<UInt8>] = []
        var keys: [String] = []
        var values: [PropertyValue?] = []
        while !r.atEnd {
            let (field, wire) = try r.key()
            switch (field, wire) {
            case (1, 2): name = String(decoding: try r.bytes(), as: UTF8.self)
            case (2, 2): features.append(try r.bytes())
            case (3, 2): keys.append(String(decoding: try r.bytes(), as: UTF8.self))
            case (4, 2): values.append(try value(try r.bytes()))
            case (5, 0): extent = Int(try r.varint())
            default: try r.skip(wire)
            }
        }
        return VTLayer(name: name, extent: extent, featureBytes: features, keys: keys, values: values)
    }

    static func value(_ bytes: ArraySlice<UInt8>) throws -> PropertyValue? {
        var r = ProtoReader(bytes)
        var out: PropertyValue?
        while !r.atEnd {
            let (field, wire) = try r.key()
            switch (field, wire) {
            case (1, 2): out = .string(String(decoding: try r.bytes(), as: UTF8.self))
            case (2, 5): out = .number(Double(Float(bitPattern: try r.fixed32())))
            case (3, 1): out = .number(Double(bitPattern: try r.fixed64()))
            case (4, 0): out = .number(Double(Int64(bitPattern: try r.varint())))
            case (5, 0): out = .number(Double(try r.varint()))
            case (6, 0): out = .number(Double(try r.svarint()))
            case (7, 0): out = .bool(try r.varint() != 0)
            default: try r.skip(wire)
            }
        }
        return out
    }

    /// The points a geometry will build, counted from its commands without building any,
    /// stopping once past `budget`; a geometry that is not plainly valid is refused: a
    /// command integer of 2^31 or more, a ClosePath with a count other than 1, a MoveTo or
    /// LineTo with more repeats than bytes left, a parameter past the geometry's end, or
    /// an unknown command.
    public static func geometryPoints(_ geometry: ArraySlice<UInt8>, budget: Int) throws -> Int {
        var r = ProtoReader(geometry)
        var points = 0
        var line = false
        while !r.atEnd {
            let cmdLen: UInt64
            do { cmdLen = try r.varint() } catch { throw TileRefused("vector tile malformed: a command runs past the geometry") }
            if cmdLen > 0x7FFF_FFFF { throw TileRefused("vector tile malformed: command integer \(cmdLen)") }
            let cmd = Int(cmdLen % 8)
            let count = Int(cmdLen / 8)
            if count == 0 { continue }
            if cmd == 7 {
                if count != 1 { throw TileRefused("vector tile malformed: ClosePath count \(count)") }
                if line { points += 1 }
            } else if cmd == 1 || cmd == 2 {
                if 2 * count > r.remaining {
                    throw TileRefused("vector tile malformed: \(cmd == 1 ? "MoveTo" : "LineTo") count \(count) with \(r.remaining) bytes left")
                }
                for _ in 0..<count {
                    do {
                        _ = try r.svarint()
                        _ = try r.svarint()
                    } catch {
                        throw TileRefused("vector tile malformed: a parameter runs past the geometry")
                    }
                    if cmd == 1 {
                        line = true
                        points += 1
                    } else if line {
                        points += 1
                    }
                    if points > budget { return points }
                }
            } else {
                throw TileRefused("vector tile malformed: unknown geometry command \(cmd)")
            }
            if points > budget { return points }
        }
        return points
    }

    /// Refuses a tile past the limits, before any geometry is built.
    public func check(_ limits: VectorLimits = .standard) throws {
        var features = 0
        for layer in layers {
            if layer.count > limits.featuresPerLayer {
                throw TileRefused("vector tile too complex: \(layer.count) features in one layer")
            }
            features += layer.count
            if features > limits.features { throw TileRefused("vector tile too complex: over \(limits.features) features") }
        }
        var points = 0
        for layer in layers {
            for i in 0..<layer.count {
                let raw: (type: Int, tags: [Int], geometry: ArraySlice<UInt8>)
                do { raw = try layer.rawFeature(i) } catch { throw TileRefused("vector tile malformed: a feature does not parse") }
                points += try Self.geometryPoints(raw.geometry, budget: limits.points - points)
                if points > limits.points { throw TileRefused("vector tile too complex: over \(limits.points) points") }
            }
        }
    }
}

/// A protocol-buffer reader over a byte slice; every read is bounds-checked.
struct ProtoReader {
    struct Truncated: Error {}

    private let buf: ArraySlice<UInt8>
    private var pos: Int

    init(_ bytes: ArraySlice<UInt8>) {
        self.buf = bytes
        self.pos = bytes.startIndex
    }

    var atEnd: Bool { pos >= buf.endIndex }
    var remaining: Int { buf.endIndex - pos }

    mutating func varint() throws -> UInt64 {
        var result: UInt64 = 0
        var shift: UInt64 = 0
        while true {
            guard pos < buf.endIndex, shift < 64 else { throw Truncated() }
            let b = buf[pos]
            pos += 1
            result |= UInt64(b & 0x7F) << shift
            if b & 0x80 == 0 { return result }
            shift += 7
        }
    }

    mutating func svarint() throws -> Int {
        let v = try varint()
        return Int(Int64(bitPattern: (v >> 1) ^ (0 &- (v & 1))))
    }

    mutating func key() throws -> (Int, Int) {
        let k = try varint()
        return (Int(k >> 3), Int(k & 7))
    }

    mutating func bytes() throws -> ArraySlice<UInt8> {
        let n = try varint()
        guard n <= UInt64(remaining) else { throw Truncated() }
        let start = pos
        pos += Int(n)
        return buf[start..<pos]
    }

    mutating func fixed32() throws -> UInt32 {
        guard remaining >= 4 else { throw Truncated() }
        var v: UInt32 = 0
        for i in 0..<4 { v |= UInt32(buf[pos + i]) << (8 * UInt32(i)) }
        pos += 4
        return v
    }

    mutating func fixed64() throws -> UInt64 {
        guard remaining >= 8 else { throw Truncated() }
        var v: UInt64 = 0
        for i in 0..<8 { v |= UInt64(buf[pos + i]) << (8 * UInt64(i)) }
        pos += 8
        return v
    }

    mutating func skip(_ wire: Int) throws {
        switch wire {
        case 0: _ = try varint()
        case 1: guard remaining >= 8 else { throw Truncated() }; pos += 8
        case 2: _ = try bytes()
        case 5: guard remaining >= 4 else { throw Truncated() }; pos += 4
        default: throw Truncated()
        }
    }
}

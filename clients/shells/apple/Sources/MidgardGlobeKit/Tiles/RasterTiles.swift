import CoreGraphics
import Foundation
import ImageIO

// Raster tiles: the terrarium encoding (`tiles/terrarium.ts`) and decoding untrusted PNG
// and JPEG tiles with limits (`tiles/decode.ts`): the header's format and size are read
// first, and anything that is not exactly an allowed square size is refused before the
// image is decoded; the decoded image is checked again. Pure but for ImageIO. [FR-H-1]

/// A tile refused by a limit: remembered as missing, never retried.
public struct TileRefused: Error, Equatable, CustomStringConvertible {
    public let description: String
    public init(_ description: String) { self.description = description }
}

/// Terrain heights in metres, row-major, north row first.
public struct HeightGrid: Sendable {
    public let width: Int
    public let height: Int
    public let data: [Float]

    public init(width: Int, height: Int, data: [Float]) {
        self.width = width
        self.height = height
        self.data = data
    }

    /// About how many bytes this holds, for the cache.
    public var bytes: Int { data.count * 4 }
}

public enum Terrarium {
    /// height = R × 256 + G + B / 256 − 32768, from RGBA (or RGB with `channels` 3) pixels.
    public static func decode(_ pixels: [UInt8], width: Int, height: Int, channels: Int = 4) throws -> HeightGrid {
        let n = width * height
        guard pixels.count >= n * channels else {
            throw TileRefused("terrarium: expected \(n * channels) pixels' bytes, got \(pixels.count)")
        }
        var data = [Float](repeating: 0, count: n)
        for i in 0..<n {
            let o = i * channels
            data[i] = Float(Double(pixels[o]) * 256 + Double(pixels[o + 1]) + Double(pixels[o + 2]) / 256 - 32768)
        }
        return HeightGrid(width: width, height: height, data: data)
    }

    /// The height at (u, v), fractions of the tile from its north-west corner: pixels are
    /// areas, so values interpolate between pixel centres and hold flat in the outer half
    /// pixel (the web's bilinear sampler).
    public static func heightAt(_ g: HeightGrid, _ u: Double, _ v: Double) -> Double {
        let x = min(Double(g.width - 1), max(0, u * Double(g.width) - 0.5))
        let y = min(Double(g.height - 1), max(0, v * Double(g.height) - 0.5))
        let x0 = Int(floor(x)), y0 = Int(floor(y))
        let x1 = min(g.width - 1, x0 + 1), y1 = min(g.height - 1, y0 + 1)
        let fx = x - Double(x0), fy = y - Double(y0)
        let w = g.width
        let d = g.data
        let top = Double(d[y0 * w + x0]) * (1 - fx) + Double(d[y0 * w + x1]) * fx
        let bottom = Double(d[y1 * w + x0]) * (1 - fx) + Double(d[y1 * w + x1]) * fx
        return top * (1 - fy) + bottom * fy
    }

    /// Heights at the (n+1)² vertices of an n × n grid laid over `rect` of the tile.
    public static func resample(_ g: HeightGrid, _ rect: TileRect, _ n: Int) -> [Float] {
        var out = [Float](repeating: 0, count: (n + 1) * (n + 1))
        for j in 0...n {
            let v = rect.oy + rect.size * Double(j) / Double(n)
            for i in 0...n {
                out[j * (n + 1) + i] = Float(heightAt(g, rect.ox + rect.size * Double(i) / Double(n), v))
            }
        }
        return out
    }

    public static func range(_ g: HeightGrid) -> (min: Float, max: Float) {
        (g.data.min() ?? 0, g.data.max() ?? 0)
    }
}

public struct RasterHeader: Equatable, Sendable {
    public enum Format: Sendable { case png, jpeg }
    public let format: Format
    public let width: Int
    public let height: Int
}

public enum RasterTiles {
    static let pngSignature: [UInt8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]

    /// The image's format and size from its header alone, or nil when it is neither a
    /// PNG nor a baseline or progressive JPEG.
    public static func header(_ b: [UInt8]) -> RasterHeader? {
        if b.count >= 24, Array(b[0..<8]) == pngSignature {
            guard b[12] == 0x49, b[13] == 0x48, b[14] == 0x44, b[15] == 0x52 else { return nil } // IHDR
            func u32(_ o: Int) -> Int { Int(b[o]) << 24 | Int(b[o + 1]) << 16 | Int(b[o + 2]) << 8 | Int(b[o + 3]) }
            return RasterHeader(format: .png, width: u32(16), height: u32(20))
        }
        if b.count >= 4, b[0] == 0xFF, b[1] == 0xD8 {
            var i = 2
            while i + 3 < b.count {
                if b[i] != 0xFF { return nil }
                let m = b[i + 1]
                if m == 0xFF { i += 1; continue }
                if m == 0x01 || (0xD0...0xD7).contains(m) { i += 2; continue }
                if m == 0xD9 || m == 0xDA { return nil }
                let len = Int(b[i + 2]) << 8 | Int(b[i + 3])
                if len < 2 { return nil }
                if (0xC0...0xCF).contains(m), m != 0xC4, m != 0xC8, m != 0xCC {
                    if i + 9 > b.count { return nil }
                    return RasterHeader(format: .jpeg, width: Int(b[i + 7]) << 8 | Int(b[i + 8]),
                                        height: Int(b[i + 5]) << 8 | Int(b[i + 6]))
                }
                i += 2 + len
            }
        }
        return nil
    }

    /// Refuses a raster tile whose header is not an allowed format and size for its source.
    @discardableResult
    public static func checkHeader(_ source: TileSourceId, _ h: RasterHeader?) throws -> RasterHeader {
        guard let h else { throw TileRefused("\(source): not a PNG or JPEG tile") }
        if source == .terrain, h.format != .png { throw TileRefused("terrain: not a PNG tile") }
        try checkSize(source, h.width, h.height)
        return h
    }

    public static func checkSize(_ source: TileSourceId, _ width: Int, _ height: Int) throws {
        let sizes = TileLimits.rasterSizes[source] ?? []
        guard width == height, sizes.contains(width) else {
            throw TileRefused("\(source): a \(width) × \(height) tile; only \(sizes.map { "\($0) × \($0)" }.joined(separator: " or "))")
        }
    }

    /// Decodes a checked image; the decoder's own idea of the size is checked again.
    static func image(_ bytes: [UInt8], _ source: TileSourceId) throws -> CGImage {
        try checkHeader(source, header(bytes))
        guard let src = CGImageSourceCreateWithData(Data(bytes) as CFData, [kCGImageSourceShouldCache: false] as CFDictionary),
              let img = CGImageSourceCreateImageAtIndex(src, 0, [kCGImageSourceShouldCache: false] as CFDictionary)
        else { throw TileRefused("\(source): the image did not decode") }
        try checkSize(source, img.width, img.height)
        return img
    }

    /// An imagery tile, ready for a texture.
    public static func decodeImagery(_ bytes: [UInt8]) throws -> CGImage {
        try image(bytes, .imagery)
    }

    /// A terrarium tile's heights, from its PNG's pixel values with no colour management:
    /// the decoded samples are read as they are.
    public static func decodeTerrain(_ bytes: [UInt8]) throws -> HeightGrid {
        let img = try image(bytes, .terrain)
        let w = img.width, h = img.height
        var pixels: [UInt8]
        if img.bitsPerComponent == 8, let data = img.dataProvider?.data, let base = CFDataGetBytePtr(data),
           [24, 32].contains(img.bitsPerPixel), img.colorSpace?.model == .rgb {
            let bpp = img.bitsPerPixel / 8
            let alphaFirst: Bool
            switch img.alphaInfo {
            case .first, .premultipliedFirst, .noneSkipFirst: alphaFirst = bpp == 4
            default: alphaFirst = false
            }
            let littleEndian = img.byteOrderInfo == .order32Little
            pixels = [UInt8](repeating: 0, count: w * h * 4)
            for y in 0..<h {
                let row = base + y * img.bytesPerRow
                for x in 0..<w {
                    let p = row + x * bpp
                    var r = p[0], g = p[1], b = p[2]
                    if bpp == 4 {
                        if littleEndian {
                            // BGRA or ABGR in memory.
                            if alphaFirst { b = p[0]; g = p[1]; r = p[2] } else { b = p[1]; g = p[2]; r = p[3] }
                        } else if alphaFirst {
                            r = p[1]; g = p[2]; b = p[3]
                        }
                    }
                    let o = (y * w + x) * 4
                    pixels[o] = r; pixels[o + 1] = g; pixels[o + 2] = b; pixels[o + 3] = 255
                }
            }
        } else {
            // An unusual layout (a palette, 16 bits): drawn into RGBA in the image's own space.
            pixels = [UInt8](repeating: 0, count: w * h * 4)
            let space = img.colorSpace ?? CGColorSpaceCreateDeviceRGB()
            let rgbSpace = space.model == .rgb ? space : CGColorSpaceCreateDeviceRGB()
            let drawn = pixels.withUnsafeMutableBytes { buffer -> Bool in
                guard let ctx = CGContext(data: buffer.baseAddress, width: w, height: h, bitsPerComponent: 8,
                                          bytesPerRow: w * 4, space: rgbSpace,
                                          bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return false }
                ctx.interpolationQuality = .none
                ctx.draw(img, in: CGRect(x: 0, y: 0, width: w, height: h))
                return true
            }
            guard drawn else { throw TileRefused("terrain: no bitmap for the tile") }
        }
        return try Terrarium.decode(pixels, width: w, height: h)
    }
}

import AVFoundation
import CoreGraphics
import CoreVideo
import Foundation
import ImageIO

// Decoding for the camera analytics (heimdall/analytics/analytics.py) without ffmpeg:
// what `ffprobe` and `ffmpeg -vf fps=F,scale=W:H:flags=bilinear -pix_fmt bgr24` give it,
// with AVFoundation (clips) and ImageIO (stills). The output is the same raw stream:
// frames of H x W x 3 bytes, B G R, row after row, no padding, frame after frame.

public enum FramesError: Error, CustomStringConvertible {
    case unreadable(String)
    case noVideoTrack
    case noFrames

    public var description: String {
        switch self {
        case .unreadable(let why): return why
        case .noVideoTrack: return "no video track"
        case .noFrames: return "no frames decoded"
        }
    }
}

/// Which decoded frames ffmpeg's `fps` filter (its default `round=near`) keeps: every input
/// frame belongs to output slot round(t x fps), and each slot shows the LAST frame of its
/// slot; a slot no frame falls in repeats the frame before. Slots count from the first
/// frame's and end before the slot of the clip's end (the last frame's time plus its
/// duration), so the last frame's own slot is often not shown. (Checked against ffmpeg 8 on TfL JamCams clips, 25 fps in, 4 fps out: input
/// frames 3, 9, 15, 21, 28, … are kept.)
public struct FrameSelector {
    public let fps: Int
    public let maxFrames: Int
    private var held: (slot: Int64, index: Int)?
    private var nextSlot: Int64?
    public private(set) var emitted = 0

    public init(fps: Int, maxFrames: Int) {
        self.fps = max(1, fps)
        self.maxFrames = max(0, maxFrames)
    }

    /// The slot of a frame at `value / timescale` seconds: floor(t x fps + 1/2), exactly.
    public func slot(value: Int64, timescale: Int32) -> Int64 {
        let num = 2 * value * Int64(fps) + Int64(timescale)
        let den = 2 * Int64(timescale)
        return num >= 0 ? num / den : -((-num + den - 1) / den)
    }

    public var done: Bool { emitted >= maxFrames }

    /// Feeds the next decoded frame (in presentation order) and holds it; returns the frames
    /// to output now, in order: the previously held one, repeated to fill any empty slots.
    public mutating func push(index: Int, slot: Int64) -> [Int] {
        var out: [Int] = []
        if let h = held, slot > h.slot {
            out = flush(through: slot - 1)
        }
        held = (slot, index)
        if nextSlot == nil { nextSlot = slot }
        return out
    }

    /// At the end of the input, whose end (the last frame's time plus its duration) is in
    /// slot `endSlot`: the held frame for the slots before it (nil: for its own slot).
    public mutating func finish(endSlot: Int64? = nil) -> [Int] {
        guard let h = held else { return [] }
        return flush(through: endSlot.map { $0 - 1 } ?? h.slot)
    }

    private mutating func flush(through last: Int64) -> [Int] {
        guard let h = held, var s = nextSlot else { return [] }
        var out: [Int] = []
        while s <= last && emitted < maxFrames {
            out.append(h.index)
            emitted += 1
            s += 1
        }
        nextSlot = s
        return out
    }
}

/// The letterbox size analytics.py asks for: r = min(S/W, S/H), int(W r) x int(H r).
public func letterboxSize(width: Int, height: Int, target: Int) -> (width: Int, height: Int) {
    let r = min(Double(target) / Double(width), Double(target) / Double(height))
    return (max(1, Int(Double(width) * r)), max(1, Int(Double(height) * r)))
}

/// Bilinear resize of a BGRA (or BGRX) image to packed BGR, sampling at pixel centres
/// (src = (dst + 0.5) x scale - 0.5, clamped at the edges), as ffmpeg's bilinear scaler.
public func resizeBGRAtoBGR(_ src: UnsafePointer<UInt8>, width: Int, height: Int, rowBytes: Int,
                            toWidth ow: Int, toHeight oh: Int) -> [UInt8] {
    var out = [UInt8](repeating: 0, count: ow * oh * 3)
    let sx = Double(width) / Double(ow), sy = Double(height) / Double(oh)
    // Per column: left index and weight (shared by every row).
    var x0 = [Int](repeating: 0, count: ow), x1 = [Int](repeating: 0, count: ow)
    var wx = [Double](repeating: 0, count: ow)
    for x in 0..<ow {
        let fx = min(max((Double(x) + 0.5) * sx - 0.5, 0), Double(width - 1))
        let i = Int(fx)
        x0[x] = i * 4
        x1[x] = min(i + 1, width - 1) * 4
        wx[x] = fx - Double(i)
    }
    out.withUnsafeMutableBufferPointer { o in
        for y in 0..<oh {
            let fy = min(max((Double(y) + 0.5) * sy - 0.5, 0), Double(height - 1))
            let j = Int(fy)
            let wy = fy - Double(j)
            let r0 = src + j * rowBytes, r1 = src + min(j + 1, height - 1) * rowBytes
            var p = y * ow * 3
            for x in 0..<ow {
                let a = x0[x], b = x1[x], w = wx[x]
                for c in 0..<3 {
                    let top = Double(r0[a + c]) * (1 - w) + Double(r0[b + c]) * w
                    let bottom = Double(r1[a + c]) * (1 - w) + Double(r1[b + c]) * w
                    o[p + c] = UInt8(min(255, max(0, (top * (1 - wy) + bottom * wy).rounded())))
                }
                p += 3
            }
        }
    }
    return out
}

/// Width and height as ffprobe reports them (the coded size, no rotation): a clip's first
/// video track, else an image.
public func probe(_ url: URL) async throws -> (width: Int, height: Int) {
    if let track = await videoTrack(AVURLAsset(url: url)) {
        let size = try await track.load(.naturalSize)
        return (Int(size.width.rounded()), Int(size.height.rounded()))
    }
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let props = CGImageSourceCopyPropertiesAtIndex(source, 0, nil) as? [CFString: Any],
          let w = props[kCGImagePropertyPixelWidth] as? Int, let h = props[kCGImagePropertyPixelHeight] as? Int
    else { throw FramesError.unreadable("not a clip or image: \(url.lastPathComponent)") }
    return (w, h)
}

/// The first video track, nil if there is none or the file is not a movie.
func videoTrack(_ asset: AVURLAsset) async -> AVAssetTrack? {
    (try? await asset.loadTracks(withMediaType: .video))?.first
}

/// Decodes a clip at `fps` (ffmpeg's `fps` filter selection), at most `maxFrames` frames,
/// each resized to `width` x `height` BGR and handed to `write` as it is ready.
public func decodeClip(_ url: URL, width: Int, height: Int, fps: Int, maxFrames: Int,
                       write: ([UInt8]) throws -> Void) async throws -> Int {
    let asset = AVURLAsset(url: url)
    guard let track = await videoTrack(asset) else { throw FramesError.noVideoTrack }
    let reader: AVAssetReader
    do { reader = try AVAssetReader(asset: asset) } catch {
        throw FramesError.unreadable("cannot read \(url.lastPathComponent): \(error.localizedDescription)")
    }
    let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
        kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
    ])
    output.alwaysCopiesSampleData = false
    reader.add(output)
    guard reader.startReading() else {
        throw FramesError.unreadable("cannot decode \(url.lastPathComponent): \(reader.error?.localizedDescription ?? "unknown")")
    }
    defer { if reader.status == .reading { reader.cancelReading() } }
    var selector = FrameSelector(fps: fps, maxFrames: maxFrames)
    // The frame the selector holds (the last of its slot so far); resized only if output.
    var held: CMSampleBuffer?
    var heldBGR: [UInt8]?
    var index = 0
    var written = 0
    var end: CMTime?  // the last frame's time plus its duration
    var lastPTS: CMTime?
    func emit(_ count: Int) throws {
        guard count > 0, let sample = held, let image = CMSampleBufferGetImageBuffer(sample) else { return }
        if heldBGR == nil { heldBGR = try bgr(image, width: width, height: height) }
        for _ in 0..<count {
            try write(heldBGR!)
            written += 1
        }
    }
    while !selector.done, let sample = output.copyNextSampleBuffer() {
        guard CMSampleBufferGetImageBuffer(sample) != nil else { continue }
        let pts = CMSampleBufferGetPresentationTimeStamp(sample)
        var duration = CMSampleBufferGetDuration(sample)
        if !duration.isValid || duration.value <= 0, let last = lastPTS { duration = CMTimeSubtract(pts, last) }
        end = duration.isValid && duration.value > 0 ? CMTimeAdd(pts, duration) : nil
        lastPTS = pts
        // What the selector releases is always the frame it held until now.
        try emit(selector.push(index: index, slot: selector.slot(value: pts.value, timescale: pts.timescale)).count)
        held = sample
        heldBGR = nil
        index += 1
    }
    if reader.status == .failed {
        throw FramesError.unreadable("decoding \(url.lastPathComponent) failed: \(reader.error?.localizedDescription ?? "unknown")")
    }
    if !selector.done {
        let endSlot = end.map { selector.slot(value: $0.value, timescale: $0.timescale) }
        try emit(selector.finish(endSlot: endSlot).count)
    }
    guard written > 0 else { throw FramesError.noFrames }
    return written
}

func bgr(_ buffer: CVPixelBuffer, width: Int, height: Int) throws -> [UInt8] {
    CVPixelBufferLockBaseAddress(buffer, .readOnly)
    defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
    guard let base = CVPixelBufferGetBaseAddress(buffer) else { throw FramesError.unreadable("empty frame") }
    return resizeBGRAtoBGR(base.assumingMemoryBound(to: UInt8.self),
                           width: CVPixelBufferGetWidth(buffer), height: CVPixelBufferGetHeight(buffer),
                           rowBytes: CVPixelBufferGetBytesPerRow(buffer), toWidth: width, toHeight: height)
}

/// Decodes one image (JPEG, PNG …) resized to `width` x `height` BGR.
public func decodeStill(_ url: URL, width: Int, height: Int) throws -> [UInt8] {
    guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil) else {
        throw FramesError.unreadable("not an image: \(url.lastPathComponent)")
    }
    let w = image.width, h = image.height
    let rowBytes = w * 4
    var pixels = [UInt8](repeating: 0, count: rowBytes * h)
    // BGRX in memory: 32-bit little-endian with the alpha byte skipped first.
    let info = CGBitmapInfo.byteOrder32Little.rawValue | CGImageAlphaInfo.noneSkipFirst.rawValue
    let drawn: Bool = pixels.withUnsafeMutableBytes { raw in
        guard let ctx = CGContext(data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8,
                                  bytesPerRow: rowBytes, space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: info) else { return false }
        ctx.interpolationQuality = .none
        ctx.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
        return true
    }
    guard drawn else { throw FramesError.unreadable("cannot draw \(url.lastPathComponent)") }
    return pixels.withUnsafeBufferPointer {
        resizeBGRAtoBGR($0.baseAddress!, width: w, height: h, rowBytes: rowBytes, toWidth: width, toHeight: height)
    }
}

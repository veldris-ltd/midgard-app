import AVFoundation
import CoreVideo
import Foundation
import XCTest
@testable import MidgardFrames

final class FramesTests: XCTestCase {
    // MARK: frame selection (ffmpeg's fps filter)

    /// 25 fps in, 4 fps out: ffmpeg 8 keeps input frames 3, 9, 15, 21, 28, … (measured on
    /// TfL clips by matching its output to every decoded frame).
    func testSelectionMatchesFfmpegAt25To4() {
        var s = FrameSelector(fps: 4, maxFrames: 40)
        var kept: [Int] = []
        for i in 0..<271 where !s.done {
            kept += s.push(index: i, slot: s.slot(value: Int64(i), timescale: 25))
        }
        kept += s.finish()
        XCTAssertEqual(kept.count, 40)
        XCTAssertEqual(Array(kept.prefix(12)), [3, 9, 15, 21, 28, 34, 40, 46, 53, 59, 65, 71])
        XCTAssertEqual(kept.last, 246)
    }

    func testSlotRoundsHalfUp() {
        let s = FrameSelector(fps: 4, maxFrames: 1)
        XCTAssertEqual(s.slot(value: 12, timescale: 100), 0)   // 0.48
        XCTAssertEqual(s.slot(value: 125, timescale: 1000), 1) // 0.5
        XCTAssertEqual(s.slot(value: 16, timescale: 100), 1)   // 0.64
        XCTAssertEqual(s.slot(value: 0, timescale: 600), 0)
    }

    /// A slot with no frame repeats the frame before (a 2 fps input asked for 4 fps).
    func testEmptySlotsRepeatTheFrameBefore() {
        var s = FrameSelector(fps: 4, maxFrames: 10)
        var kept: [Int] = []
        for i in 0..<3 { kept += s.push(index: i, slot: s.slot(value: Int64(i), timescale: 2)) }
        kept += s.finish()
        XCTAssertEqual(kept, [0, 0, 1, 1, 2])
    }

    /// The clip's end bounds the last slots, as ffmpeg (4 s at 25 fps: 16 frames, slots 0-15;
    /// 0.32 s: 1 frame, measured with ffmpeg 8 on testsrc clips).
    func testTheEndOfTheClipEndsTheSlots() {
        func run(frames: Int) -> [Int] {
            var s = FrameSelector(fps: 4, maxFrames: 40)
            var kept: [Int] = []
            for i in 0..<frames { kept += s.push(index: i, slot: s.slot(value: Int64(i), timescale: 25)) }
            return kept + s.finish(endSlot: s.slot(value: Int64(frames), timescale: 25))
        }
        XCTAssertEqual(run(frames: 100).count, 16)
        XCTAssertEqual(run(frames: 63).count, 10)
        XCTAssertEqual(run(frames: 8), [3])
    }

    func testFinishWithNothingGivesNothing() {
        var s = FrameSelector(fps: 4, maxFrames: 10)
        XCTAssertEqual(s.finish(), [])
    }

    // MARK: sizes and resizing

    func testLetterboxSizeAsAnalyticsPy() {
        XCTAssertTrue(letterboxSize(width: 352, height: 288, target: 640) == (640, 523))
        XCTAssertTrue(letterboxSize(width: 1920, height: 1080, target: 640) == (640, 360))
        XCTAssertTrue(letterboxSize(width: 640, height: 640, target: 640) == (640, 640))
    }

    func testResizeKeepsAFlatColourAndDropsAlpha() {
        let src: [UInt8] = Array(repeating: [10, 20, 30, 255], count: 4 * 3).flatMap { $0 }
        let out = src.withUnsafeBufferPointer {
            resizeBGRAtoBGR($0.baseAddress!, width: 4, height: 3, rowBytes: 16, toWidth: 7, toHeight: 5)
        }
        XCTAssertEqual(out.count, 7 * 5 * 3)
        XCTAssertEqual(Array(out.prefix(6)), [10, 20, 30, 10, 20, 30])
        XCTAssertEqual(Set(out), [10, 20, 30])
    }

    /// Doubling a 2-pixel row: centres at 0.25 and 0.75 interpolate (src -0.25 clamps).
    func testResizeInterpolatesBetweenPixelCentres() {
        let src: [UInt8] = [0, 0, 0, 0, 200, 100, 40, 0]
        let out = src.withUnsafeBufferPointer {
            resizeBGRAtoBGR($0.baseAddress!, width: 2, height: 1, rowBytes: 8, toWidth: 4, toHeight: 1)
        }
        XCTAssertEqual(out, [0, 0, 0, 50, 25, 10, 150, 75, 30, 200, 100, 40])
    }

    // MARK: decoding

    var tmp: URL!

    override func setUpWithError() throws {
        tmp = FileManager.default.temporaryDirectory.appendingPathComponent("midgard-frames-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: tmp, withIntermediateDirectories: true)
    }

    override func tearDownWithError() throws {
        try? FileManager.default.removeItem(at: tmp)
    }

    /// An H.264 clip of `frames` frames at 25 fps, 352 x 288 like the TfL JamCams, each
    /// frame one flat grey (its index x 2), written with AVAssetWriter.
    func writeClip(frames: Int) async throws -> URL {
        let url = tmp.appendingPathComponent("clip.mp4")
        let writer = try AVAssetWriter(outputURL: url, fileType: .mp4)
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: 352, AVVideoHeightKey: 288,
        ])
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: 352, kCVPixelBufferHeightKey as String: 288,
        ])
        writer.add(input)
        XCTAssertTrue(writer.startWriting())
        writer.startSession(atSourceTime: .zero)
        for i in 0..<frames {
            while !input.isReadyForMoreMediaData { usleep(1000) }
            var buffer: CVPixelBuffer?
            CVPixelBufferPoolCreatePixelBuffer(nil, adaptor.pixelBufferPool!, &buffer)
            let b = try XCTUnwrap(buffer)
            CVPixelBufferLockBaseAddress(b, [])
            memset(CVPixelBufferGetBaseAddress(b), Int32(i * 2), CVPixelBufferGetDataSize(b))
            CVPixelBufferUnlockBaseAddress(b, [])
            XCTAssertTrue(adaptor.append(b, withPresentationTime: CMTime(value: CMTimeValue(i), timescale: 25)))
        }
        input.markAsFinished()
        let done = expectation(description: "written")
        writer.finishWriting { done.fulfill() }
        await fulfillment(of: [done], timeout: 30)
        XCTAssertEqual(writer.status, .completed, "\(String(describing: writer.error))")
        return url
    }

    func testDecodesASyntheticClipAtFourFps() async throws {
        let url = try await writeClip(frames: 100)  // 4 s
        let size = try await probe(url)
        XCTAssertTrue(size == (352, 288))
        var frames: [[UInt8]] = []
        let n = try await decodeClip(url, width: 640, height: 523, fps: 4, maxFrames: 40) { frames.append($0) }
        XCTAssertEqual(n, frames.count)
        XCTAssertEqual(n, 16)  // slots 0 … 15 of a 4 s clip, as ffmpeg
        XCTAssertTrue(frames.allSatisfy { $0.count == 640 * 523 * 3 })
        // Slot 0 shows frame 3 (grey 6), slot 1 frame 9 (grey 18): allow for H.264 and YUV.
        XCTAssertEqual(Double(frames[0][1000]), 6, accuracy: 4)
        XCTAssertEqual(Double(frames[1][1000]), 18, accuracy: 4)
        // max frames is honoured
        var count = 0
        let five = try await decodeClip(url, width: 64, height: 52, fps: 4, maxFrames: 5) { _ in count += 1 }
        XCTAssertEqual(five, 5)
        XCTAssertEqual(count, 5)
    }

    func testRefusesSomethingThatIsNotMedia() async throws {
        let url = tmp.appendingPathComponent("x.mp4")
        try Data("not a clip".utf8).write(to: url)
        do { _ = try await probe(url); XCTFail("probe accepted it") } catch {}
        do { _ = try await decodeClip(url, width: 10, height: 10, fps: 4, maxFrames: 4) { _ in }; XCTFail("decoded it") } catch {}
        XCTAssertThrowsError(try decodeStill(url, width: 10, height: 10))
    }

    /// A stored TfL clip and still from the recorded hour, when this Mac has it: the output
    /// shape analytics.py reads (40 frames of 640 x 523 x 3 for a ~10 s 352 x 288 clip).
    func testStoredTflClipShape() async throws {
        let media = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Midgard-dev/recordings/hour-1/media")
        let clip = media.appendingPathComponent("JamCams_00001.07450.mp4")
        let still = media.appendingPathComponent("JamCams_00001.07450.jpg")
        guard FileManager.default.fileExists(atPath: clip.path) else {
            throw XCTSkip("no stored TfL clip at \(clip.path)")
        }
        let (w, h) = try await probe(clip)
        XCTAssertTrue((w, h) == (352, 288))
        let size = letterboxSize(width: w, height: h, target: 640)
        var bytes = 0, frames = 0
        _ = try await decodeClip(clip, width: size.width, height: size.height, fps: 4, maxFrames: 40) {
            bytes += $0.count; frames += 1
        }
        XCTAssertEqual(frames, 40)
        XCTAssertEqual(bytes, 40 * 640 * 523 * 3)
        if FileManager.default.fileExists(atPath: still.path) {
            let (sw, sh) = try await probe(still)
            let s = letterboxSize(width: sw, height: sh, target: 640)
            XCTAssertEqual(try decodeStill(still, width: s.width, height: s.height).count, s.width * s.height * 3)
        }
    }
}

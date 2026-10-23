import Darwin
import Foundation
import MidgardFrames

// midgard-frames: the camera analytics' clip and image decoder in Midgard.app, in place of
// ffprobe and ffmpeg (heimdall/analytics/analytics.py, --frames-helper).
//
//   midgard-frames probe PATH
//       prints "W,H" (as `ffprobe -show_entries stream=width,height -of csv=p=0`)
//   midgard-frames decode PATH --width W --height H (--fps F --max-frames N | --still)
//       writes raw BGR frames of W x H to stdout (as `ffmpeg -vf fps=F,scale=W:H:flags=bilinear
//       -frames:v N -f rawvideo -pix_fmt bgr24 pipe:1`; --still: one image, scaled only)
//
// Exit 0 on success; 1 with a one-line message on stderr otherwise; 2 for bad usage.

func fail(_ message: String, code: Int32 = 1) -> Never {
    FileHandle.standardError.write(Data("midgard-frames: \(message)\n".utf8))
    exit(code)
}

func usage() -> Never {
    fail("usage: midgard-frames probe PATH | decode PATH --width W --height H (--fps F --max-frames N | --still)", code: 2)
}

/// Writes all of `bytes` to stdout (a pipe takes it in pieces).
func writeOut(_ bytes: [UInt8]) throws {
    try bytes.withUnsafeBytes { raw in
        var offset = 0
        while offset < raw.count {
            let n = write(1, raw.baseAddress! + offset, raw.count - offset)
            if n < 0 {
                if errno == EINTR { continue }
                throw FramesError.unreadable("writing to stdout: \(String(cString: strerror(errno)))")
            }
            offset += n
        }
    }
}

func run() async {
    var args = Array(CommandLine.arguments.dropFirst())
    guard args.count >= 2 else { usage() }
    let command = args.removeFirst()
    let url = URL(fileURLWithPath: args.removeFirst())
    var options: [String: String] = [:]
    var still = false
    while !args.isEmpty {
        let a = args.removeFirst()
        switch a {
        case "--still": still = true
        case "--width", "--height", "--fps", "--max-frames":
            guard !args.isEmpty else { usage() }
            options[a] = args.removeFirst()
        default: usage()
        }
    }
    func number(_ key: String, max limit: Int) -> Int {
        guard let v = options[key].flatMap({ Int($0) }), v > 0, v <= limit else { usage() }
        return v
    }
    signal(SIGPIPE, SIG_IGN)
    do {
        switch command {
        case "probe":
            guard options.isEmpty, !still else { usage() }
            let (w, h) = try await probe(url)
            print("\(w),\(h)")
        case "decode":
            let w = number("--width", max: 8192), h = number("--height", max: 8192)
            if still {
                guard options["--fps"] == nil, options["--max-frames"] == nil else { usage() }
                try writeOut(try decodeStill(url, width: w, height: h))
            } else {
                let fps = number("--fps", max: 60), n = number("--max-frames", max: 10_000)
                _ = try await decodeClip(url, width: w, height: h, fps: fps, maxFrames: n, write: writeOut)
            }
        default:
            usage()
        }
    } catch {
        fail("\(url.lastPathComponent): \(error)")
    }
}

await run()

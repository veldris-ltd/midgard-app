// Draws the Midgard app icon as vectors (CoreGraphics) and writes every PNG the two apps'
// asset catalogues need, plus the 1024 × 1024 masters. Nothing is traced from a bitmap:
// the hexagon, the plate and the texture are paths, so every size is drawn crisp.
//
// The design: a teal hexagon outline (pointy top and bottom, rounded joins) centred on a
// very dark blue-green ground with a faint dot texture.
//
//   swift clients/shells/apple/tools/make-app-icon.swift            # write everything
//   swift clients/shells/apple/tools/make-app-icon.swift --check    # compare, write nothing
//
// Run it from the repository root. It writes:
//   Apps/Icon/AppIcon-1024.png          the master (full-bleed square, no alpha)
//   Apps/Icon/AppIcon-macOS-1024.png    the macOS master (plate on the macOS grid)
//   Apps/MidgardiOS/Assets.xcassets/AppIcon.appiconset/   iOS: 1024 any, dark, tinted
//   Apps/MidgardMac/Assets.xcassets/AppIcon.appiconset/   macOS: 16 … 512 @1x and @2x
// (paths under clients/shells/apple/). Colours and proportions are the constants below.

import AppKit
import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

// MARK: design constants (fractions of the icon's side)

struct RGB { let r, g, b: CGFloat
    init(_ hex: UInt32) {
        r = CGFloat((hex >> 16) & 0xFF) / 255; g = CGFloat((hex >> 8) & 0xFF) / 255; b = CGFloat(hex & 0xFF) / 255
    }
    func cg(_ a: CGFloat = 1) -> CGColor { CGColor(srgbRed: r, green: g, blue: b, alpha: a) }
}

enum Design {
    /// The ground: very dark blue-green, a touch lighter at the centre.
    static let groundCentre = RGB(0x0D2328)
    static let groundEdge = RGB(0x061215)
    /// The hexagon's teal.
    static let teal = RGB(0x2FC6BA)
    /// The hexagon's circumradius (centre to a vertex) and stroke width.
    static let hexRadius: CGFloat = 0.30
    static let hexStroke: CGFloat = 0.062
    /// The dot texture: spacing, dot radius and opacity.
    static let dotPitch: CGFloat = 1.0 / 24
    static let dotRadius: CGFloat = 0.0042
    static let dotAlpha: CGFloat = 0.10
}

/// macOS icon grid (the current macOS app icon template, 1024 canvas): the plate is
/// 824 × 824, centred, corner radius 185.4, with a soft drop shadow.
enum MacGrid {
    static let plate: CGFloat = 824.0 / 1024
    static let corner: CGFloat = 185.4 / 1024
    static let shadowBlur: CGFloat = 28.0 / 1024
    static let shadowOffset: CGFloat = -12.0 / 1024
}

enum Variant { case light, dark, tinted }

// MARK: drawing

func hexagonPath(centre c: CGPoint, radius r: CGFloat) -> CGPath {
    let p = CGMutablePath()
    for i in 0..<6 {
        // Pointy top and bottom: vertices at 90°, 150°, … 30°.
        let a = (90.0 + Double(i) * 60.0) * .pi / 180
        let pt = CGPoint(x: c.x + r * CGFloat(cos(a)), y: c.y + r * CGFloat(sin(a)))
        if i == 0 { p.move(to: pt) } else { p.addLine(to: pt) }
    }
    p.closeSubpath()
    return p
}

/// The artwork in a square of side `s` at `origin` (the full-bleed design).
func drawArtwork(_ ctx: CGContext, origin: CGPoint, side s: CGFloat, variant: Variant) {
    let rect = CGRect(origin: origin, size: CGSize(width: s, height: s))
    let centre = CGPoint(x: rect.midX, y: rect.midY)
    let space = CGColorSpace(name: CGColorSpace.sRGB)!

    // Ground.
    switch variant {
    case .tinted:
        ctx.setFillColor(CGColor(gray: 0, alpha: 1)); ctx.fill(rect)
    case .light, .dark:
        let colours = [Design.groundCentre.cg(), Design.groundEdge.cg()] as CFArray
        let g = CGGradient(colorsSpace: space, colors: colours, locations: [0, 1])!
        ctx.saveGState(); ctx.clip(to: rect)
        ctx.drawRadialGradient(g, startCenter: centre, startRadius: 0, endCenter: centre,
                               endRadius: s * 0.75, options: [.drawsAfterEndLocation])
        ctx.restoreGState()
    }

    // Dot texture, on a grid centred on the hexagon.
    if variant != .tinted && s >= 64 {
        ctx.saveGState(); ctx.clip(to: rect)
        ctx.setFillColor(Design.teal.cg(Design.dotAlpha))
        let pitch = Design.dotPitch * s, r = max(Design.dotRadius * s, 0.35)
        let n = Int(ceil(0.5 / Design.dotPitch)) + 1
        for i in -n...n { for j in -n...n {
            let x = centre.x + CGFloat(i) * pitch, y = centre.y + CGFloat(j) * pitch
            ctx.fillEllipse(in: CGRect(x: x - r, y: y - r, width: 2 * r, height: 2 * r))
        } }
        ctx.restoreGState()
    }

    // The hexagon outline: rounded joins and caps.
    let hex = hexagonPath(centre: centre, radius: Design.hexRadius * s)
    ctx.saveGState()
    ctx.addPath(hex)
    ctx.setLineJoin(.round); ctx.setLineCap(.round)
    ctx.setLineWidth(max(Design.hexStroke * s, 1))
    switch variant {
    case .tinted: ctx.setStrokeColor(CGColor(gray: 1, alpha: 1))
    case .light, .dark:
        // A faint glow under the stroke, then the stroke.
        ctx.setShadow(offset: .zero, blur: s * 0.03, color: Design.teal.cg(0.45))
        ctx.setStrokeColor(Design.teal.cg())
    }
    ctx.strokePath()
    ctx.restoreGState()
}

func makeContext(_ px: Int, alpha: Bool) -> CGContext {
    let space = CGColorSpace(name: CGColorSpace.sRGB)!
    let info = alpha ? CGImageAlphaInfo.premultipliedLast.rawValue : CGImageAlphaInfo.noneSkipLast.rawValue
    let ctx = CGContext(data: nil, width: px, height: px, bitsPerComponent: 8, bytesPerRow: 0, space: space, bitmapInfo: info)!
    ctx.setShouldAntialias(true); ctx.setAllowsAntialiasing(true); ctx.interpolationQuality = .high
    return ctx
}

/// iOS: the full-bleed square, opaque (the system applies the mask).
func iosImage(_ px: Int, _ variant: Variant) -> CGImage {
    let ctx = makeContext(px, alpha: false)
    drawArtwork(ctx, origin: .zero, side: CGFloat(px), variant: variant)
    return ctx.makeImage()!
}

/// macOS: the artwork clipped to the rounded plate on the macOS grid, with its shadow,
/// on a transparent canvas.
func macImage(_ px: Int) -> CGImage {
    let s = CGFloat(px)
    let ctx = makeContext(px, alpha: true)
    ctx.clear(CGRect(x: 0, y: 0, width: s, height: s))
    let side = MacGrid.plate * s
    let origin = CGPoint(x: (s - side) / 2, y: (s - side) / 2)
    let plate = CGPath(roundedRect: CGRect(origin: origin, size: CGSize(width: side, height: side)),
                       cornerWidth: MacGrid.corner * s, cornerHeight: MacGrid.corner * s, transform: nil)
    if px >= 64 {
        ctx.saveGState()
        ctx.setShadow(offset: CGSize(width: 0, height: MacGrid.shadowOffset * s), blur: MacGrid.shadowBlur * s,
                      color: CGColor(gray: 0, alpha: 0.35))
        ctx.addPath(plate); ctx.setFillColor(Design.groundEdge.cg()); ctx.fillPath()
        ctx.restoreGState()
    }
    ctx.saveGState()
    ctx.addPath(plate); ctx.clip()
    drawArtwork(ctx, origin: origin, side: side, variant: .light)
    // A faint inner rim, so the dark plate keeps its edge on a dark Dock.
    ctx.addPath(plate)
    ctx.setLineWidth(max(4.0 / 1024 * s, 0.5))
    ctx.setStrokeColor(Design.teal.cg(0.16))
    ctx.strokePath()
    ctx.restoreGState()
    return ctx.makeImage()!
}

func png(_ image: CGImage) -> Data {
    let data = NSMutableData()
    let dest = CGImageDestinationCreateWithData(data, UTType.png.identifier as CFString, 1, nil)!
    CGImageDestinationAddImage(dest, image, nil)
    CGImageDestinationFinalize(dest)
    return data as Data
}

// MARK: outputs

let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
    .appendingPathComponent("clients/shells/apple")
guard FileManager.default.fileExists(atPath: root.appendingPathComponent("Package.swift").path) else {
    FileHandle.standardError.write("run from the repository root\n".data(using: .utf8)!)
    exit(2)
}
let check = CommandLine.arguments.contains("--check")
var outputs: [(String, Data)] = []

outputs.append(("Apps/Icon/AppIcon-1024.png", png(iosImage(1024, .light))))
outputs.append(("Apps/Icon/AppIcon-macOS-1024.png", png(macImage(1024))))

let iosSet = "Apps/MidgardiOS/Assets.xcassets/AppIcon.appiconset"
outputs.append(("\(iosSet)/AppIcon-1024.png", png(iosImage(1024, .light))))
outputs.append(("\(iosSet)/AppIcon-1024-dark.png", png(iosImage(1024, .dark))))
outputs.append(("\(iosSet)/AppIcon-1024-tinted.png", png(iosImage(1024, .tinted))))
outputs.append(("\(iosSet)/Contents.json", Data("""
{
  "images" : [
    { "filename" : "AppIcon-1024.png", "idiom" : "universal", "platform" : "ios", "size" : "1024x1024" },
    { "appearances" : [ { "appearance" : "luminosity", "value" : "dark" } ],
      "filename" : "AppIcon-1024-dark.png", "idiom" : "universal", "platform" : "ios", "size" : "1024x1024" },
    { "appearances" : [ { "appearance" : "luminosity", "value" : "tinted" } ],
      "filename" : "AppIcon-1024-tinted.png", "idiom" : "universal", "platform" : "ios", "size" : "1024x1024" }
  ],
  "info" : { "author" : "xcode", "version" : 1 }
}

""".utf8)))

let macSet = "Apps/MidgardMac/Assets.xcassets/AppIcon.appiconset"
var macImages: [String] = []
for (pt, scales) in [(16, [1, 2]), (32, [1, 2]), (128, [1, 2]), (256, [1, 2]), (512, [1, 2])] {
    for scale in scales {
        let px = pt * scale
        let name = "AppIcon-\(pt)@\(scale)x.png"
        outputs.append(("\(macSet)/\(name)", png(macImage(px))))
        macImages.append("    { \"filename\" : \"\(name)\", \"idiom\" : \"mac\", \"scale\" : \"\(scale)x\", \"size\" : \"\(pt)x\(pt)\" }")
    }
}
outputs.append(("\(macSet)/Contents.json", Data("""
{
  "images" : [
\(macImages.joined(separator: ",\n"))
  ],
  "info" : { "author" : "xcode", "version" : 1 }
}

""".utf8)))
for app in ["MidgardiOS", "MidgardMac"] {
    outputs.append(("Apps/\(app)/Assets.xcassets/Contents.json",
                    Data("{\n  \"info\" : { \"author\" : \"xcode\", \"version\" : 1 }\n}\n".utf8)))
}

var stale: [String] = []
for (path, data) in outputs {
    let url = root.appendingPathComponent(path)
    if check {
        if (try? Data(contentsOf: url)) != data { stale.append(path) }
    } else {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try data.write(to: url)
        print("wrote \(path)")
    }
}
if check {
    if stale.isEmpty { print("app icon: up to date (\(outputs.count) files)") } else {
        print("app icon: differs from a fresh drawing:\n  " + stale.joined(separator: "\n  "))
        exit(1)
    }
}

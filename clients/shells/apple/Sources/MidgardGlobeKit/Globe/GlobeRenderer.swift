import CoreGraphics
import Foundation
import Metal
import MetalKit
import simd

/// The globe's colours from the design tokens, per mode, as the web's `globe/colours.ts`
/// reads them: sea `--bg-sunken`, land `--surface-raised`, coast `--border-strong`,
/// borders `--border`, graticule `--divider`, atmosphere `--text-muted`. [FR-H-1]
struct GlobePalette {
    var sea: SIMD4<Double>
    var land: SIMD4<Double>
    var coast: SIMD4<Double>
    var border: SIMD4<Double>
    var graticule: SIMD4<Double>
    var atmosphere: SIMD4<Double>

    static func rgba(_ hex: UInt32, _ a: Double = 1) -> SIMD4<Double> {
        SIMD4(Double((hex >> 16) & 0xFF) / 255, Double((hex >> 8) & 0xFF) / 255, Double(hex & 0xFF) / 255, a)
    }

    static let dark = GlobePalette(sea: rgba(0x050506), land: rgba(0x1E2126), coast: rgba(0x3A3E46), border: rgba(0x2A2D34),
                                   graticule: rgba(0xFFFFFF, 0.07), atmosphere: rgba(0x9199A3))
    static let light = GlobePalette(sea: rgba(0xE6E8EC), land: rgba(0xF6F7F9), coast: rgba(0xBFC4CB), border: rgba(0xD6D9DE),
                                    graticule: rgba(0x0A0B0D, 0.08), atmosphere: rgba(0x5C636B))
}

/// The globe, drawn with Metal on demand, as the web's `globe/renderer.ts` draws it with
/// three.js: one sphere carrying the clean style (land and sea painted once into an
/// equirectangular texture from the 1:50m land) with day and night shading from the
/// subsolar point; the tile layer's terrain patches; the graticule, coastlines and land
/// borders as lines (1:110m far out, 1:50m close in); and a soft atmosphere edge that
/// becomes a sky inside its shell. Reversed depth with no far plane keeps a 1.5 km
/// close-up and the whole globe in one view. The markers and labels are SwiftUI, over
/// it. Nothing here makes a request: the outlines are bundled. [FR-H-1]
@MainActor
final class GlobeRenderer: NSObject, MTKViewDelegate {
    nonisolated static let textureWidth = 4096
    nonisolated static let night: Float = 0.5

    let device: MTLDevice
    private let queue: MTLCommandQueue
    private let surface: MTLRenderPipelineState
    private let lines: MTLRenderPipelineState
    private let atmosphere: MTLRenderPipelineState
    private let opaqueDepth: MTLDepthStencilState
    private let overlayDepth: MTLDepthStencilState
    private let sampler: MTLSamplerState
    private let sphereVertices: MTLBuffer
    private let sphereIndices: MTLBuffer
    private let sphereIndexCount: Int
    private let atmosphereIndices: MTLBuffer
    private let atmosphereVertices: MTLBuffer
    private let atmosphereIndexCount: Int
    private var landTexture: MTLTexture?
    private let blank: MTLTexture
    private var paintedDark: Bool?
    private var lineSets: [(kind: LineKind, detail: Detail, buffer: MTLBuffer, count: Int)] = []
    private let controller: GlobeController
    private let outlines: GlobeOutlines
    private var sunDir = SIMD3<Float>(1, 0, 0)
    private var sunAt = Date.distantPast
    weak var view: MTKView?

    enum LineKind { case graticule, coast, border }
    enum Detail { case far, near, all }

    init?(view: MTKView, controller: GlobeController, outlines: GlobeOutlines) {
        guard let device = view.device ?? MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else { return nil }
        self.device = device
        self.queue = queue
        self.controller = controller
        self.outlines = outlines
        view.device = device
        view.colorPixelFormat = .bgra8Unorm
        view.depthStencilPixelFormat = .depth32Float
        view.clearDepth = 0
        view.sampleCount = 4
        view.clearColor = MTLClearColor(red: 0, green: 0, blue: 0, alpha: 0)
        #if os(macOS)
        view.layer?.isOpaque = false
        #else
        view.isOpaque = false
        view.backgroundColor = .clear
        #endif
        do {
            let library = try device.makeLibrary(source: Self.shaders, options: nil)
            func pipeline(_ v: String, _ f: String, blend: Bool, cull: Bool = false) throws -> MTLRenderPipelineState {
                let d = MTLRenderPipelineDescriptor()
                d.vertexFunction = library.makeFunction(name: v)
                d.fragmentFunction = library.makeFunction(name: f)
                d.colorAttachments[0].pixelFormat = view.colorPixelFormat
                d.depthAttachmentPixelFormat = view.depthStencilPixelFormat
                d.rasterSampleCount = view.sampleCount
                if blend {
                    let a = d.colorAttachments[0]!
                    a.isBlendingEnabled = true
                    a.sourceRGBBlendFactor = .sourceAlpha
                    a.destinationRGBBlendFactor = .oneMinusSourceAlpha
                    a.sourceAlphaBlendFactor = .one
                    a.destinationAlphaBlendFactor = .oneMinusSourceAlpha
                }
                return try device.makeRenderPipelineState(descriptor: d)
            }
            surface = try pipeline("surfaceVertex", "surfaceFragment", blend: false)
            lines = try pipeline("lineVertex", "lineFragment", blend: true)
            atmosphere = try pipeline("atmosphereVertex", "atmosphereFragment", blend: true)
        } catch {
            return nil
        }
        let opaque = MTLDepthStencilDescriptor()
        opaque.depthCompareFunction = .greater
        opaque.isDepthWriteEnabled = true
        let overlay = MTLDepthStencilDescriptor()
        overlay.depthCompareFunction = .greater
        overlay.isDepthWriteEnabled = false
        guard let od = device.makeDepthStencilState(descriptor: opaque),
              let ov = device.makeDepthStencilState(descriptor: overlay) else { return nil }
        opaqueDepth = od
        overlayDepth = ov
        let s = MTLSamplerDescriptor()
        s.minFilter = .linear
        s.magFilter = .linear
        s.mipFilter = .linear
        s.maxAnisotropy = 8
        s.sAddressMode = .clampToEdge
        s.tAddressMode = .clampToEdge
        guard let sampler = device.makeSamplerState(descriptor: s) else { return nil }
        self.sampler = sampler

        let sphere = Self.sphere(radius: 1, segments: 160, rings: 80)
        let atmos = Self.sphere(radius: 1.035, segments: 96, rings: 48)
        guard let sv = device.makeBuffer(bytes: sphere.vertices, length: sphere.vertices.count * 4),
              let si = device.makeBuffer(bytes: sphere.indices, length: sphere.indices.count * 4),
              let av = device.makeBuffer(bytes: atmos.vertices, length: atmos.vertices.count * 4),
              let ai = device.makeBuffer(bytes: atmos.indices, length: atmos.indices.count * 4) else { return nil }
        sphereVertices = sv
        sphereIndices = si
        sphereIndexCount = sphere.indices.count
        atmosphereVertices = av
        atmosphereIndices = ai
        atmosphereIndexCount = atmos.indices.count

        let bd = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: 1, height: 1, mipmapped: false)
        guard let blank = device.makeTexture(descriptor: bd) else { return nil }
        blank.replace(region: MTLRegionMake2D(0, 0, 1, 1), mipmapLevel: 0, withBytes: [UInt8(0), 0, 0, 255], bytesPerRow: 4)
        self.blank = blank
        super.init()
        self.view = view
        buildLines()
        controller.renderer = self
        paintLand(dark: controller.dark)
    }

    // MARK: geometry

    /// A UV sphere in the surface layout (position, normal, tile uv, world uv), with the
    /// world uv north at the top, as the land texture is painted.
    static func sphere(radius: Double, segments: Int, rings: Int) -> (vertices: [Float], indices: [UInt32]) {
        var v: [Float] = []
        var idx: [UInt32] = []
        for j in 0...rings {
            let lat = 90 - 180 * Double(j) / Double(rings)
            for i in 0...segments {
                let lon = -180 + 360 * Double(i) / Double(segments)
                let p = Geo.toXyz(lat, lon, 0, radius: radius)
                let n = simd_normalize(p)
                let u = Float(i) / Float(segments), w = Float(j) / Float(rings)
                v += [Float(p.x), Float(p.y), Float(p.z), Float(n.x), Float(n.y), Float(n.z), u, w, u, w]
            }
        }
        let row = segments + 1
        for j in 0..<rings {
            for i in 0..<segments {
                let a = UInt32(j * row + i), b = a + 1, c = a + UInt32(row), d = c + 1
                idx += [a, c, b, b, c, d]
            }
        }
        return (v, idx)
    }

    private func buildLines() {
        let w = outlines.world, c = outlines.close
        func add(_ kind: LineKind, _ detail: Detail, _ l: [[Double]], _ radius: Double) {
            let segs = Outlines.lineSegments(l, radius: radius, maxStepDeg: 1)
            guard !segs.isEmpty, let b = device.makeBuffer(bytes: segs, length: segs.count * 4) else { return }
            lineSets.append((kind, detail, b, segs.count / 3))
        }
        add(.graticule, .all, Outlines.graticule(everyDeg: 15, stepDeg: 2), 1.0004)
        add(.coast, .far, (w.coast ?? []).map { Outlines.decodeLine($0, quantum: w.quantum) }, 1.0008)
        add(.border, .far, (w.borders ?? []).map { Outlines.decodeLine($0, quantum: w.quantum) }, 1.0007)
        add(.coast, .near, Outlines.coastFromLand(c.land ?? [], quantum: c.quantum), 1.0002)
        add(.border, .near, (c.borders ?? []).map { Outlines.decodeLine($0, quantum: c.quantum) }, 1.0002)
    }

    /// The clean style's land and sea, painted from the 1:50m land polygons into an
    /// equirectangular texture (north row first), off the main thread.
    func paintLand(dark: Bool) {
        guard paintedDark != dark else { return }
        paintedDark = dark
        let palette = dark ? GlobePalette.dark : GlobePalette.light
        let close = outlines.close
        let device = self.device
        let queue = self.queue
        TileLoader.decodeQueue.async {
            let texture = Self.landTexture(close, palette: palette, device: device, queue: queue)
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated {
                    guard let self, self.paintedDark == dark else { return }
                    self.landTexture = texture
                    self.controller.requestRender()
                }
            }
        }
    }

    nonisolated static func landTexture(_ close: OutlineFile, palette: GlobePalette, device: MTLDevice,
                                        queue: MTLCommandQueue) -> MTLTexture? {
        let w = textureWidth, h = textureWidth / 2
        var pixels = [UInt8](repeating: 0, count: w * h * 4)
        let ok = pixels.withUnsafeMutableBytes { buffer -> Bool in
            guard let ctx = CGContext(data: buffer.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                                      space: CGColorSpace(name: CGColorSpace.sRGB)!,
                                      bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else { return false }
            ctx.setFillColor(red: palette.sea.x, green: palette.sea.y, blue: palette.sea.z, alpha: 1)
            ctx.fill(CGRect(x: 0, y: 0, width: w, height: h))
            ctx.setFillColor(red: palette.land.x, green: palette.land.y, blue: palette.land.z, alpha: 1)
            // In the bitmap's own space y runs up, and its first row is the top: north.
            for poly in close.land ?? [] {
                let path = CGMutablePath()
                for ring in poly {
                    let p = Outlines.decodeLine(ring, quantum: close.quantum)
                    var i = 0
                    while i + 1 < p.count {
                        let pt = CGPoint(x: (p[i] + 180) / 360 * Double(w), y: (p[i + 1] + 90) / 180 * Double(h))
                        if i == 0 { path.move(to: pt) } else { path.addLine(to: pt) }
                        i += 2
                    }
                    path.closeSubpath()
                }
                ctx.addPath(path)
                ctx.fillPath(using: .evenOdd)
            }
            return true
        }
        guard ok else { return nil }
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: w, height: h, mipmapped: true)
        d.usage = .shaderRead
        guard let t = device.makeTexture(descriptor: d) else { return nil }
        t.replace(region: MTLRegionMake2D(0, 0, w, h), mipmapLevel: 0, withBytes: pixels, bytesPerRow: w * 4)
        if let cb = queue.makeCommandBuffer(), let blit = cb.makeBlitCommandEncoder() {
            blit.generateMipmaps(for: t)
            blit.endEncoding()
            cb.commit()
            cb.waitUntilCompleted()
        }
        return t
    }

    // MARK: drawing

    func mtkView(_ view: MTKView, drawableSizeWillChange size: CGSize) {
        controller.requestRender()
    }

    func draw(in view: MTKView) {
        let animating = controller.frame(now: CACurrentMediaTime())
        // Keep drawing while the camera flies; otherwise draw on demand.
        view.isPaused = !animating
        view.enableSetNeedsDisplay = !animating
        if Date().timeIntervalSince(sunAt) > 60 { updateSun() }
        guard let pass = view.currentRenderPassDescriptor, let drawable = view.currentDrawable,
              let cb = queue.makeCommandBuffer(), let enc = cb.makeRenderCommandEncoder(descriptor: pass) else { return }
        let c = controller
        let night = Self.night
        let viewProj = c.viewProj

        // The sphere, just under sea level while tiles are drawn.
        enc.setRenderPipelineState(surface)
        enc.setDepthStencilState(opaqueDepth)
        enc.setCullMode(.back)
        enc.setFrontFacing(.counterClockwise)
        enc.setFragmentSamplerState(sampler, index: 0)
        let land = landTexture ?? blank
        let scale = c.sphereScale
        var sphereU = SurfaceUniforms(mvp: Self.float4x4(viewProj * Self.scaleMatrix(scale)), sun: SIMD4(sunDir, night), useLand: 1)
        enc.setVertexBuffer(sphereVertices, offset: 0, index: 0)
        enc.setVertexBytes(&sphereU, length: MemoryLayout<SurfaceUniforms>.stride, index: 1)
        enc.setFragmentBytes(&sphereU, length: MemoryLayout<SurfaceUniforms>.stride, index: 1)
        enc.setFragmentTexture(blank, index: 0)
        enc.setFragmentTexture(land, index: 1)
        enc.drawIndexedPrimitives(type: .triangle, indexCount: sphereIndexCount, indexType: .uint32,
                                  indexBuffer: sphereIndices, indexBufferOffset: 0)

        // The tile layer's patches, each relative to its own centre.
        if let tiles = c.tiles, c.tilesShown || !tiles.drawList.isEmpty {
            enc.setCullMode(.none)
            for m in tiles.drawList {
                var u = SurfaceUniforms(mvp: Self.float4x4(viewProj * GlobeController.translation(m.center)),
                                        sun: SIMD4(sunDir, night), useLand: m.texture == nil ? 1 : 0)
                enc.setVertexBuffer(m.vertices, offset: 0, index: 0)
                enc.setVertexBytes(&u, length: MemoryLayout<SurfaceUniforms>.stride, index: 1)
                enc.setFragmentBytes(&u, length: MemoryLayout<SurfaceUniforms>.stride, index: 1)
                enc.setFragmentTexture(m.texture ?? blank, index: 0)
                enc.setFragmentTexture(land, index: 1)
                enc.drawIndexedPrimitives(type: .triangle, indexCount: m.indexCount, indexType: .uint16,
                                          indexBuffer: m.indices, indexBufferOffset: 0)
            }
        }

        // The graticule, coastlines and borders.
        enc.setRenderPipelineState(lines)
        enc.setDepthStencilState(overlayDepth)
        enc.setCullMode(.none)
        let palette = c.dark ? GlobePalette.dark : GlobePalette.light
        let mvp = Self.float4x4(viewProj)
        for set in lineSets {
            let shown: Bool
            switch set.detail {
            case .all: shown = c.showFarLines || c.showNearLines
            case .far: shown = c.showFarLines
            case .near: shown = c.showNearLines
            }
            guard shown else { continue }
            var colour: SIMD4<Double>
            switch set.kind {
            case .graticule:
                colour = palette.graticule
                // The graticule token is a faint overlay colour; keep it faint on the globe.
                colour.w = max(0.12, min(0.35, colour.w * 3))
            case .coast:
                colour = palette.coast
                colour.w = 0.95
            case .border:
                colour = palette.border
                colour.w = 0.7
            }
            var u = LineUniforms(mvp: mvp, colour: SIMD4<Float>(colour))
            enc.setVertexBuffer(set.buffer, offset: 0, index: 0)
            enc.setVertexBytes(&u, length: MemoryLayout<LineUniforms>.stride, index: 1)
            enc.setFragmentBytes(&u, length: MemoryLayout<LineUniforms>.stride, index: 1)
            enc.drawPrimitives(type: .line, vertexStart: 0, vertexCount: set.count)
        }

        // The atmosphere: its far wall, a rim round the globe or a sky from inside.
        enc.setRenderPipelineState(atmosphere)
        enc.setCullMode(.front)
        let modelView = c.viewRot * GlobeController.translation(-c.eye)
        let up = simd_normalize(c.eye)
        var au = AtmosphereUniforms(mvp: mvp, modelView: Self.float4x4(modelView),
                                    glow: SIMD4<Float>(palette.atmosphere), camPos: SIMD4(Float(c.eye.x), Float(c.eye.y), Float(c.eye.z), 1),
                                    camUp: SIMD4(Float(up.x), Float(up.y), Float(up.z), 0),
                                    params: SIMD4(Float(c.atmosphereStrength), c.atmosphereInside ? 1 : 0, 0, 0))
        enc.setVertexBuffer(atmosphereVertices, offset: 0, index: 0)
        enc.setVertexBytes(&au, length: MemoryLayout<AtmosphereUniforms>.stride, index: 1)
        enc.setFragmentBytes(&au, length: MemoryLayout<AtmosphereUniforms>.stride, index: 1)
        enc.drawIndexedPrimitives(type: .triangle, indexCount: atmosphereIndexCount, indexType: .uint32,
                                  indexBuffer: atmosphereIndices, indexBufferOffset: 0)
        enc.endEncoding()
        cb.present(drawable)
        cb.commit()
    }

    private func updateSun() {
        sunAt = Date()
        let s = Sun.subsolarPoint(sunAt)
        let p = simd_normalize(Geo.toXyz(s.lat, s.lon))
        sunDir = SIMD3<Float>(Float(p.x), Float(p.y), Float(p.z))
    }

    static func scaleMatrix(_ s: Double) -> simd_double4x4 {
        simd_double4x4(diagonal: SIMD4(s, s, s, 1))
    }

    static func float4x4(_ m: simd_double4x4) -> simd_float4x4 {
        simd_float4x4(columns: (SIMD4<Float>(m.columns.0), SIMD4<Float>(m.columns.1), SIMD4<Float>(m.columns.2),
                                SIMD4<Float>(m.columns.3)))
    }

    struct SurfaceUniforms {
        var mvp: simd_float4x4
        var sun: SIMD4<Float>
        var useLand: Float
        var pad = SIMD3<Float>(0, 0, 0)
    }

    struct LineUniforms {
        var mvp: simd_float4x4
        var colour: SIMD4<Float>
    }

    struct AtmosphereUniforms {
        var mvp: simd_float4x4
        var modelView: simd_float4x4
        var glow: SIMD4<Float>
        var camPos: SIMD4<Float>
        var camUp: SIMD4<Float>
        /// strength, inside.
        var params: SIMD4<Float>
    }

    // The shaders, compiled at start-up. Textures are sampled from sRGB formats (so in
    // linear light), lit, and written back as sRGB, as three.js does; the lines and the
    // atmosphere carry sRGB colours and blend as the web's canvas does.
    static let shaders = """
    #include <metal_stdlib>
    using namespace metal;

    struct SurfaceU { float4x4 mvp; float4 sun; float useLand; float3 pad; };
    struct SurfaceOut { float4 position [[position]]; float3 normal; float2 uv; float2 geo; };

    vertex SurfaceOut surfaceVertex(uint vid [[vertex_id]], const device float *v [[buffer(0)]],
                                    constant SurfaceU &u [[buffer(1)]]) {
        const device float *p = v + vid * 10;
        SurfaceOut o;
        o.position = u.mvp * float4(p[0], p[1], p[2], 1.0);
        o.normal = float3(p[3], p[4], p[5]);
        o.uv = float2(p[6], p[7]);
        o.geo = float2(p[8], p[9]);
        return o;
    }

    float3 toSRGB(float3 c) {
        c = saturate(c);
        return select(1.055 * pow(c, 1.0 / 2.4) - 0.055, c * 12.92, c <= 0.0031308);
    }

    fragment float4 surfaceFragment(SurfaceOut in [[stage_in]], constant SurfaceU &u [[buffer(1)]],
                                    texture2d<float> map [[texture(0)]], texture2d<float> land [[texture(1)]],
                                    sampler s [[sampler(0)]]) {
        float3 base = mix(map.sample(s, in.uv).rgb, land.sample(s, in.geo).rgb, u.useLand);
        float d = dot(normalize(in.normal), u.sun.xyz);
        float light = mix(u.sun.w, 1.0, smoothstep(-0.12, 0.12, d));
        return float4(toSRGB(base * light), 1.0);
    }

    struct LineU { float4x4 mvp; float4 colour; };
    struct LineOut { float4 position [[position]]; };

    vertex LineOut lineVertex(uint vid [[vertex_id]], const device float *v [[buffer(0)]], constant LineU &u [[buffer(1)]]) {
        const device float *p = v + vid * 3;
        LineOut o;
        o.position = u.mvp * float4(p[0], p[1], p[2], 1.0);
        return o;
    }

    fragment float4 lineFragment(LineOut in [[stage_in]], constant LineU &u [[buffer(1)]]) {
        return u.colour;
    }

    struct AtmosphereU { float4x4 mvp; float4x4 modelView; float4 glow; float4 camPos; float4 camUp; float4 params; };
    struct AtmosphereOut { float4 position [[position]]; float3 normalV; float3 posV; float3 world; };

    vertex AtmosphereOut atmosphereVertex(uint vid [[vertex_id]], const device float *v [[buffer(0)]],
                                          constant AtmosphereU &u [[buffer(1)]]) {
        const device float *p = v + vid * 10;
        float3 pos = float3(p[0], p[1], p[2]);
        AtmosphereOut o;
        o.position = u.mvp * float4(pos, 1.0);
        o.normalV = (u.modelView * float4(p[3], p[4], p[5], 0.0)).xyz;
        o.posV = (u.modelView * float4(pos, 1.0)).xyz;
        o.world = pos;
        return o;
    }

    fragment float4 atmosphereFragment(AtmosphereOut in [[stage_in]], constant AtmosphereU &u [[buffer(1)]]) {
        // From outside, the far wall faces away: brightest at the limb, fading outwards.
        float facing = -dot(normalize(in.normalV), normalize(-in.posV));
        float rim = pow(clamp(facing / 0.3, 0.0, 1.0), 1.6);
        // From inside, a sky: brightest along the horizon, thinning overhead.
        float elevation = dot(normalize(in.world - u.camPos.xyz), u.camUp.xyz);
        float sky = 1.0 - smoothstep(-0.02, 0.45, elevation);
        return float4(u.glow.rgb, mix(rim, sky, u.params.y) * u.params.x);
    }
    """
}

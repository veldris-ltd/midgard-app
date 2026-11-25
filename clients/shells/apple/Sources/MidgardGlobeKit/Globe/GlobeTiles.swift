import CoreGraphics
import Foundation
import Metal
@preconcurrency import MetalKit

/// The globe's styles: CLEAN (flat land and sea from the design tokens), TOPOGRAPHIC
/// (tints, hillshade and contours from the terrain tiles) and SATELLITE (EOX Sentinel-2
/// cloudless 2016). [FR-H-1]
public enum GlobeStyle: String, CaseIterable, Sendable {
    case clean, topographic, satellite

    public var label: String {
        switch self {
        case .clean: return "CLEAN"
        case .topographic: return "TOPOGRAPHIC"
        case .satellite: return "SATELLITE"
        }
    }
}

/// One terrain patch on the GPU.
final class PatchMesh {
    let tile: Tile
    let center: Vec3
    let vertices: MTLBuffer
    let indices: MTLBuffer
    let indexCount: Int
    let texture: MTLTexture?

    init(tile: Tile, center: Vec3, vertices: MTLBuffer, indices: MTLBuffer, indexCount: Int, texture: MTLTexture?) {
        self.tile = tile
        self.center = center
        self.vertices = vertices
        self.indices = indices
        self.indexCount = indexCount
        self.texture = texture
    }
}

/// The globe's map-tile layer, as the web's `tiles/globeTiles.ts`: terrain patches for the
/// area in view, in one of three styles. Each frame it picks tiles for the camera
/// (`TileSelect`, within the budget), asks the loader for their data, cancels what the
/// view no longer needs, and swaps the patches in. A patch is built off the main thread
/// once its data has arrived: terrain heights (terrarium, from zoom 6 down; coarser tiles
/// stay flat) raised by HEIGHTS, and the style's colours. When the loader goes offline
/// the layer draws nothing and the globe falls back to its own sphere. [FR-H-1]
@MainActor
final class GlobeTiles {
    /// Terrain heights are fetched to this zoom; finer patches sample their ancestor.
    nonisolated static let terrainMaxZoom = 13
    /// Patches coarser than this are flat: relief does not show from that far out.
    nonisolated static let terrainMinZoom = 6
    nonisolated static let minZoom = 2
    nonisolated static let maxZoom = 14
    /// Most tiles drawn plus wanted at once.
    nonisolated static let budget = 160
    nonisolated static let segments = 32
    nonisolated static let topoPx = 256
    /// Patches built at once, off the main thread.
    nonisolated static let maxBuilding = 4

    let loader: TileLoader
    private let tiles: TileSet
    private let device: MTLDevice
    private let textureLoader: MTKTextureLoader
    private let queue: MTLCommandQueue?
    private let meshes: LruCache<PatchMesh>
    private var building: Set<String> = []
    private var generation = 0
    private(set) var style: GlobeStyle = .clean
    private(set) var exaggeration = GlobeItems.defaultExaggeration
    private var dark = true
    private var drawn: Set<String> = []
    private(set) var drawnTiles: [Tile] = []
    private(set) var drawList: [PatchMesh] = []
    private var inUse: Set<String> = []
    /// Something arrived or was built: draw again.
    var onChange: (() -> Void)?

    init(device: MTLDevice, tiles: TileSet, fetcher: TileFetching, disk: DiskTileCache?, online: @escaping () -> Bool) {
        self.device = device
        self.tiles = tiles
        textureLoader = MTKTextureLoader(device: device)
        queue = device.makeCommandQueue()
        loader = TileLoader(tiles: tiles, fetcher: fetcher, disk: disk, online: online)
        meshes = LruCache(capacity: Self.budget * 2)
        meshes.protect = { [unowned self] in self.drawn.contains($0) }
        loader.protect = { [unowned self] in self.inUse.contains($0) }
        loader.onChange = { [weak self] in self?.onChange?() }
    }

    var active: Bool { loader.state == .online && !drawn.isEmpty }
    var online: Bool { loader.state == .online }

    func setPaused(_ paused: Bool) { loader.setPaused(paused) }

    func setStyle(_ s: GlobeStyle) {
        guard s != style else { return }
        style = s
        rebuild()
    }

    func setDark(_ d: Bool) {
        guard d != dark else { return }
        dark = d
        if style == .topographic { rebuild() }
    }

    func setExaggeration(_ k: Double) {
        guard k != exaggeration else { return }
        exaggeration = k
        rebuild()
    }

    func destroy() {
        loader.destroy()
        meshes.protect = { _ in false }
        meshes.clear()
        drawList = []
        generation += 1
    }

    /// The ground under a point as drawn, km above sea level times HEIGHTS; nil where no
    /// patch is drawn.
    func groundKm(_ lat: Double, _ lon: Double) -> Double? {
        for t in drawnTiles {
            let u = TileMath.lonToTileX(lon, Double(t.z)) - Double(t.x)
            let v = TileMath.latToTileY(lat, Double(t.z)) - Double(t.y)
            if u < 0 || u > 1 || v < 0 || v > 1 { continue }
            if t.z < Self.terrainMinZoom { return 0 }
            let tt = terrainTile(t)
            guard let loaded = terrain(tt) else { return nil }
            return TerrainPatch.drawnGroundM(loaded, TileMath.subRect(t, tt.z), n: Self.segments, u, v) / 1000 * exaggeration
        }
        return nil
    }

    func update(_ cam: ViewState, viewportPx: Double, aspect: Double) {
        if loader.state == .offline {
            // Keep asking for the tile under the camera, so the loader can probe for the
            // network coming back; draw nothing until it does.
            drawn = []
            drawnTiles = []
            drawList = []
            inUse = []
            loader.retain(Set(requestData(TileMath.tileAt(cam.lat, cam.lon, Self.terrainMinZoom), priority: 0)))
            return
        }
        var memo: [Tile: Bool] = [:]
        let sel = TileSelect.select(cam, TileSelect.Options(minZoom: Self.minZoom, maxZoom: Self.maxZoom, budget: Self.budget,
                                                             viewportPx: viewportPx, aspect: aspect)) { t in
            if let r = memo[t] { return r }
            let r = ensureMesh(t)
            memo[t] = r
            return r
        }
        drawn = Set(sel.draw.map(\.description))
        drawnTiles = sel.draw
        drawList = sel.draw.compactMap { meshes.get($0.description) }
        var keep = Set<String>()
        inUse = []
        for t in sel.draw { for u in urls(t) { inUse.insert(u) } }
        for (i, t) in sel.want.enumerated() { for u in requestData(t, priority: i) { keep.insert(u) } }
        keep.formUnion(inUse)
        loader.retain(keep)
    }

    // MARK: data

    private func needsTerrain(_ t: Tile) -> Bool { t.z >= Self.terrainMinZoom || style == .topographic }
    private var needsImagery: Bool { style == .satellite }
    private func terrainTile(_ t: Tile) -> Tile { TileMath.ancestor(t, at: min(t.z, Self.terrainMaxZoom)) }

    private func urls(_ t: Tile) -> [String] {
        var out: [String] = []
        if needsTerrain(t), let u = tiles.url(.terrain, terrainTile(t)) { out.append(u) }
        if needsImagery, let u = tiles.url(.imagery, t) { out.append(u) }
        return out
    }

    /// The decoded terrain tile: `.some(nil)` when the source has none, nil when not loaded.
    private func terrain(_ t: Tile) -> HeightGrid?? {
        guard let u = tiles.url(.terrain, t) else { return .some(nil) }
        return loader.get(u, as: HeightGrid.self)
    }

    /// Asks for what a tile needs; returns the URLs.
    private func requestData(_ t: Tile, priority: Int) -> [String] {
        let list = urls(t)
        let terrainURL = tiles.url(.terrain, terrainTile(t))
        for u in list {
            let st = loader.status(u)
            guard st == .none || st == .failed else { continue }
            if u == terrainURL {
                loader.request(u, priority: priority) { try RasterTiles.decodeTerrain($0) }
            } else {
                loader.request(u, priority: priority) { try RasterTiles.decodeImagery($0) }
            }
        }
        return list
    }

    // MARK: patches

    /// True when the tile's patch exists; otherwise starts building it once its data is in.
    private func ensureMesh(_ t: Tile) -> Bool {
        let key = t.description
        if meshes.has(key) { return true }
        if building.contains(key) { return false }
        var grid: HeightGrid?
        if needsTerrain(t) {
            guard let g = terrain(terrainTile(t)) else { return false }
            grid = g
        }
        var image: CGImage?
        if needsImagery {
            guard let u = tiles.url(.imagery, t), let b = loader.get(u, as: CGImage.self) else { return false }
            image = b
        }
        guard building.count < Self.maxBuilding else { return false }
        building.insert(key)
        let job = BuildJob(tile: t, grid: grid, image: image, style: style, dark: dark, exaggeration: exaggeration,
                           terrainZoom: terrainTile(t).z)
        let gen = generation
        let device = self.device
        let textureLoader = self.textureLoader
        let queue = self.queue
        TileLoader.decodeQueue.async { [weak self] in
            let mesh = job.build(device: device, textureLoader: textureLoader, queue: queue)
            DispatchQueue.main.async { [weak self] in
                MainActor.assumeIsolated {
                    guard let self else { return }
                    self.building.remove(key)
                    guard gen == self.generation, let mesh else { return }
                    self.meshes.set(key, mesh)
                    self.onChange?()
                }
            }
        }
        return false
    }

    private func rebuild() {
        generation += 1
        building = []
        drawn = []
        drawnTiles = []
        drawList = []
        meshes.protect = { _ in false }
        meshes.clear()
        meshes.protect = { [unowned self] in self.drawn.contains($0) }
        onChange?()
    }
}

/// What one patch is built from, off the main thread.
struct BuildJob: @unchecked Sendable {
    let tile: Tile
    let grid: HeightGrid?
    let image: CGImage?
    let style: GlobeStyle
    let dark: Bool
    let exaggeration: Double
    let terrainZoom: Int

    func build(device: MTLDevice, textureLoader: MTKTextureLoader, queue: MTLCommandQueue?) -> PatchMesh? {
        let n = GlobeTiles.segments
        let rect = TileMath.subRect(tile, terrainZoom)
        let heights = grid.flatMap { tile.z >= GlobeTiles.terrainMinZoom ? Terrarium.resample($0, rect, n) : nil }
        let patch = TerrainPatch.build(tile, heights: heights, n: n, exaggeration: exaggeration)
        var texture: MTLTexture?
        if style == .topographic, let grid {
            texture = topoTexture(grid, rect, device: device, queue: queue)
        } else if style == .satellite, let image {
            texture = try? textureLoader.newTexture(cgImage: image, options: Self.textureOptions)
        }
        guard let vb = device.makeBuffer(bytes: patch.vertices, length: patch.vertices.count * 4, options: .storageModeShared),
              let ib = device.makeBuffer(bytes: patch.indices, length: patch.indices.count * 2, options: .storageModeShared)
        else { return nil }
        return PatchMesh(tile: tile, center: patch.center, vertices: vb, indices: ib, indexCount: patch.indices.count,
                         texture: texture)
    }

    static let textureOptions: [MTKTextureLoader.Option: Any] = [
        .SRGB: true, .generateMipmaps: true, .textureUsage: MTLTextureUsage.shaderRead.rawValue,
        .textureStorageMode: MTLStorageMode.private.rawValue, .origin: MTKTextureLoader.Origin.topLeft.rawValue,
    ]

    /// The TOPOGRAPHIC colours as an sRGB texture with mipmaps: sampled in linear light
    /// and lit, as the web's `DataTexture` in the sRGB colour space.
    private func topoTexture(_ grid: HeightGrid, _ rect: TileRect, device: MTLDevice, queue: MTLCommandQueue?) -> MTLTexture? {
        let n = GlobeTiles.topoPx
        let h: [Float]
        if rect.size == 1, grid.width == n, grid.height == n {
            h = grid.data
        } else {
            // Pixel centres of the finer tile, sampled from its ancestor.
            let inner = TileRect(ox: rect.ox + rect.size / Double(2 * n), oy: rect.oy + rect.size / Double(2 * n),
                                 size: rect.size * (1 - 1 / Double(n)))
            h = Terrarium.resample(grid, inner, n - 1)
        }
        let lat = TileMath.tileYToLat(Double(tile.y) + 0.5, Double(tile.z))
        let px = Topo.pixels(h, n, n, metresPerPixel: TileMath.metresPerPixel(lat, tile.z, tilePx: Double(n)),
                             interval: Topo.contourInterval(tile.z), palette: dark ? .dark : .light,
                             zFactor: Topo.shadeZFactor(tile.z))
        let d = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm_srgb, width: n, height: n, mipmapped: true)
        d.usage = .shaderRead
        guard let t = device.makeTexture(descriptor: d) else { return nil }
        t.replace(region: MTLRegionMake2D(0, 0, n, n), mipmapLevel: 0, withBytes: px, bytesPerRow: n * 4)
        if let cb = queue?.makeCommandBuffer(), let blit = cb.makeBlitCommandEncoder() {
            blit.generateMipmaps(for: t)
            blit.endEncoding()
            cb.commit()
            cb.waitUntilCompleted()
        }
        return t
    }
}

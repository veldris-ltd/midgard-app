import Foundation
import XCTest

@testable import MidgardGlobeKit

/// The tile network's guards, as the web's `tiles/hosts.test.ts` and `loader.test.ts`:
/// the fixed hosts and the one URL spelling each tile has; HTTPS only, no cookies, a
/// generic User-Agent, no redirect followed, the byte caps; and the loader's cache,
/// cancel, pause and offline states, on the recorded tiles through a stand-in network.
/// No request leaves the machine. [FR-H-1] [NFR-PRV-1]
@MainActor
final class LoaderTests: XCTestCase {
    let tiles = TileSet.publicHosts

    // MARK: the allow-list

    func testTheHostsAndTheirOneSpelling() {
        XCTAssertEqual(tiles.hosts, ["s3.amazonaws.com", "tiles.maps.eox.at", "tiles.versatiles.org"])
        let t = Tile(z: 6, x: 31, y: 19)
        XCTAssertEqual(tiles.url(.terrain, t), "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/6/31/19.png")
        XCTAssertEqual(tiles.url(.imagery, t), "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless_3857/default/g/6/19/31.jpg")
        XCTAssertEqual(tiles.url(.vector, Tile(z: 8, x: 125, y: 78)), "https://tiles.versatiles.org/tiles/osm/8/125/78")
        XCTAssertNil(tiles.url(.vector, Tile(z: 15, x: 0, y: 0)), "past the source's deepest zoom")
        for id in TileSourceId.allCases {
            let u = tiles.url(id, t)!
            XCTAssertEqual(tiles.allowed(u)?.0, id)
            XCTAssertEqual(tiles.allowed(u)?.1, t)
        }
        let refused = [
            "http://tiles.versatiles.org/tiles/osm/8/125/78",
            "https://tiles.versatiles.org.evil.example/tiles/osm/8/125/78",
            "https://example.com/tiles/osm/8/125/78",
            "https://tiles.versatiles.org:443/tiles/osm/8/125/78",
            "https://user@tiles.versatiles.org/tiles/osm/8/125/78",
            "https://tiles.versatiles.org/tiles/osm/8/125/78?x=1",
            "https://tiles.versatiles.org/tiles/osm/8/125/78#f",
            "https://tiles.versatiles.org/tiles/osm/08/125/78",
            "https://tiles.versatiles.org/tiles/osm/8/125/078",
            "https://TILES.versatiles.org/tiles/osm/8/125/78",
            "https://tiles.versatiles.org./tiles/osm/8/125/78",
            "https://tiles.versatiles.org/tiles/osm/../osm/8/125/78",
            "https://tiles.versatiles.org/tiles/osm/8/999/78",
            "https://tiles.versatiles.org/tiles/osm/8/125/78.pbf",
            "https://s3.amazonaws.com/other-bucket/6/31/19.png",
            "https://tiles.maps.eox.at/wmts/1.0.0/s2cloudless-2020_3857/default/g/6/19/31.jpg",
        ]
        for u in refused { XCTAssertNil(tiles.allowed(u), u) }
    }

    // MARK: the HTTP client

    func testRequestsCarryNoCookiesAndAGenericUserAgent() throws {
        let client = TileHTTPClient(hosts: tiles.hosts)
        let req = try XCTUnwrap(client.request(URL(string: tiles.url(.vector, Tile(z: 8, x: 125, y: 78))!)!))
        XCTAssertFalse(req.httpShouldHandleCookies)
        XCTAssertEqual(req.value(forHTTPHeaderField: "User-Agent"), "MidgardMaps/0.1")
        XCTAssertEqual(req.httpMethod, "GET")
        let c = TileHTTPClient.configuration()
        XCTAssertEqual(c.httpCookieAcceptPolicy, .never)
        XCTAssertFalse(c.httpShouldSetCookies)
        XCTAssertNil(c.httpCookieStorage)
        XCTAssertNil(c.urlCache)
        XCTAssertNil(c.urlCredentialStorage)
        XCTAssertEqual(c.timeoutIntervalForRequest, 15)
        XCTAssertEqual(c.timeoutIntervalForResource, 15)
        XCTAssertEqual(c.httpAdditionalHeaders?["User-Agent"] as? String, TileHTTPClient.userAgent)
        // The User-Agent names no user, machine, system or address.
        for word in [NSUserName(), Host.current().localizedName ?? "-", "Darwin", "CFNetwork", "@"] where !word.isEmpty {
            XCTAssertFalse(TileHTTPClient.userAgent.contains(word), word)
        }
        // HTTP, other hosts, ports and credentials are never sent.
        for u in ["http://tiles.versatiles.org/tiles/osm/8/125/78", "https://example.com/x",
                  "https://tiles.versatiles.org:8443/tiles/osm/8/125/78", "https://a:b@tiles.versatiles.org/tiles/osm/8/125/78"] {
            XCTAssertNil(client.request(URL(string: u)!), u)
        }
    }

    func testRedirectsAreRefusedAndBodiesCapped() async throws {
        let client = TileHTTPClient(hosts: tiles.hosts, protocolClasses: [StubNetwork.self])
        let base = "https://tiles.versatiles.org/tiles/osm/8/125/"
        StubNetwork.set([
            base + "1": .init(status: 200, body: Data(count: 100)),
            base + "2": .init(status: 302, headers: ["Location": "https://elsewhere.example/tile"], body: Data()),
            base + "3": .init(status: 200, body: Data(count: 2 * 1024 * 1024 + 1)),
            base + "4": .init(status: 200, headers: ["Content-Length": "3000000"], body: Data(count: 10)),
            base + "5": .init(status: 404, body: Data()),
        ])
        defer { StubNetwork.set([:]) }
        let cap = TileLimits.maxBytes[.vector]!
        XCTAssertEqual(cap, 2 * 1024 * 1024)
        func get(_ u: String) async -> FetchOutcome { await client.fetch(URL(string: u)!, maxBytes: cap, timeout: 5) }
        let ok = await get(base + "1"), redirected = await get(base + "2"), big = await get(base + "3")
        let declared = await get(base + "4"), missing = await get(base + "5")
        let insecure = await get("http://tiles.versatiles.org/tiles/osm/8/125/1")
        XCTAssertEqual(ok, .body(Data(count: 100)))
        XCTAssertEqual(redirected, .network, "a redirect is not followed")
        XCTAssertFalse(StubNetwork.requested.contains("https://elsewhere.example/tile"))
        XCTAssertEqual(big, .oversize)
        XCTAssertEqual(declared, .oversize)
        XCTAssertEqual(missing, .status(404))
        XCTAssertEqual(insecure, .refused)
        XCTAssertEqual(TileLimits.maxBytes[.terrain], 1024 * 1024)
        XCTAssertEqual(TileLimits.maxBytes[.imagery], 2 * 1024 * 1024)
        XCTAssertEqual(TileLimits.fetchTimeout, 15)
        XCTAssertEqual(TileLimits.decodeTimeout, 2)
    }

    // MARK: the loader

    private func until(_ timeout: TimeInterval = 5, _ condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() && Date() < deadline { try? await Task.sleep(nanoseconds: 10_000_000) }
    }

    func testRecordedTilesLoadAndOffListURLsAreRefused() async throws {
        let net = FakeFetcher()
        let terrain = tiles.url(.terrain, Tile(z: 6, x: 31, y: 19))!
        let vector = tiles.url(.vector, Tile(z: 8, x: 125, y: 78))!
        net.outcomes[terrain] = .body(Data(try TileTests.fixture("terrarium-6-31-19.png")))
        net.outcomes[vector] = .body(Data(try TileTests.fixture("versatiles-osm-8-125-78.mvt")))
        let loader = TileLoader(tiles: tiles, fetcher: net)
        XCTAssertTrue(loader.request(terrain, priority: 0) { try RasterTiles.decodeTerrain($0) })
        XCTAssertTrue(loader.request(vector, priority: 1) { try MapFeatures.decode($0, z: 8) })
        await until { loader.status(terrain) == .ready && loader.status(vector) == .ready }
        XCTAssertEqual(loader.get(terrain, as: HeightGrid.self)??.width, 256)
        XCTAssertEqual(loader.get(vector, as: MapTileData.self)??.labels.first?.text, "Forfar")
        XCTAssertFalse(loader.request("https://example.com/tiles/osm/8/125/78", priority: 0) { $0 })
        XCTAssertFalse(loader.request("http://tiles.versatiles.org/tiles/osm/8/125/78", priority: 0) { $0 })
        XCTAssertEqual(net.calls.count, 2)
        XCTAssertEqual(loader.state, .online)
    }

    func testMissingServerErrorsOversizeAndUndecodable() async {
        let net = FakeFetcher()
        let u = { (y: Int) in self.tiles.url(.vector, Tile(z: 8, x: 125, y: y))! }
        net.outcomes[u(1)] = .status(404)
        net.outcomes[u(2)] = .status(503)
        net.outcomes[u(3)] = .oversize
        net.outcomes[u(4)] = .body(Data([1, 2, 3]))
        let loader = TileLoader(tiles: tiles, fetcher: net)
        for y in 1...4 { loader.request(u(y), priority: y) { try MapFeatures.decode($0, z: 8) } }
        await until { (1...4).allSatisfy { loader.status(u($0)) != .pending } }
        XCTAssertEqual(loader.status(u(1)), .missing)
        XCTAssertEqual(loader.status(u(2)), .failed, "a 5xx is retried later")
        XCTAssertEqual(loader.status(u(3)), .missing)
        XCTAssertEqual(loader.stats.oversize, 1)
        XCTAssertEqual(loader.status(u(4)), .missing, "a tile that will not decode")
        XCTAssertEqual(loader.state, .online)
    }

    func testCancelPauseAndOffline() async {
        let held = FakeFetcher()
        held.hold = true
        let loader = TileLoader(tiles: tiles, fetcher: held)
        let a = tiles.url(.vector, Tile(z: 8, x: 125, y: 78))!
        loader.request(a, priority: 0) { $0 }
        await until { held.calls.count == 1 }
        loader.retain([])
        await until { held.cancelled == 1 }
        XCTAssertEqual(loader.stats.cancelled, 1)
        XCTAssertEqual(held.cancelled, 1, "the request in flight was cancelled")
        XCTAssertEqual(loader.status(a), .none)

        loader.setPaused(true)
        loader.request(a, priority: 0) { $0 }
        try? await Task.sleep(nanoseconds: 100_000_000)
        XCTAssertEqual(held.calls.count, 1, "paused, nothing is sent")
        XCTAssertEqual(loader.status(a), .none)

        let down = FakeFetcher()
        let offline = TileLoader(tiles: tiles, fetcher: down)
        for y in 0..<3 { offline.request(tiles.url(.vector, Tile(z: 8, x: 125, y: y))!, priority: y) { $0 } }
        await until { offline.state == .offline }
        XCTAssertEqual(offline.state, .offline, "network failures in a row: offline")
        let device = TileLoader(tiles: tiles, fetcher: FakeFetcher(), online: { false })
        device.request(a, priority: 0) { $0 }
        XCTAssertEqual(device.state, .offline, "the device has no network")
        offline.destroy()
        device.destroy()
    }
}

/// A stand-in network for the loader: canned outcomes (no network failure unless none
/// is given), or every request held until cancelled.
final class FakeFetcher: TileFetching, @unchecked Sendable {
    var outcomes: [String: FetchOutcome] = [:]
    var hold = false
    private(set) var calls: [String] = []
    private(set) var cancelled = 0
    private let lock = NSLock()

    private func record(_ url: URL) -> (FetchOutcome, Bool) {
        lock.withLock {
            calls.append(url.absoluteString)
            return (outcomes[url.absoluteString] ?? .network, hold)
        }
    }

    private func noteCancelled() { lock.withLock { cancelled += 1 } }

    func fetch(_ url: URL, maxBytes: Int, timeout: TimeInterval) async -> FetchOutcome {
        let (outcome, holding) = record(url)
        guard holding else { return outcome }
        while !Task.isCancelled { try? await Task.sleep(nanoseconds: 5_000_000) }
        noteCancelled()
        return .network
    }
}

/// A URL protocol answering from a table, so the HTTP client is tested with no network.
final class StubNetwork: URLProtocol {
    struct Answer {
        var status: Int
        var headers: [String: String] = [:]
        var body: Data
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var table: [String: Answer] = [:]
    nonisolated(unsafe) private static var seen: [String] = []

    static func set(_ t: [String: Answer]) {
        lock.lock(); defer { lock.unlock() }
        table = t
        seen = []
    }

    static var requested: [String] {
        lock.lock(); defer { lock.unlock() }
        return seen
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url!
        Self.lock.lock()
        Self.seen.append(url.absoluteString)
        let answer = Self.table[url.absoluteString] ?? Answer(status: 404, body: Data())
        Self.lock.unlock()
        let response = HTTPURLResponse(url: url, statusCode: answer.status, httpVersion: "HTTP/1.1", headerFields: answer.headers)!
        if (300..<400).contains(answer.status), let to = answer.headers["Location"].flatMap(URL.init(string:)) {
            client?.urlProtocol(self, wasRedirectedTo: URLRequest(url: to), redirectResponse: response)
        }
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        var at = 0
        while at < answer.body.count {
            let end = min(answer.body.count, at + 256 * 1024)
            client?.urlProtocol(self, didLoad: answer.body.subdata(in: at..<end))
            at = end
        }
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

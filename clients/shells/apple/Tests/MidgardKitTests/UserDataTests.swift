import Foundation
@preconcurrency import MidgardCore
@testable import MidgardKit
import Network
import XCTest

/// A local HTTP/1.1 stub on the loopback interface: canned answers by method and path,
/// and every request it was sent, for the checks. Nothing leaves the machine.
final class LoopbackStub: @unchecked Sendable {
    struct Seen: Sendable {
        let method: String
        let path: String
        let headers: [String: String]
        let body: Data
    }

    typealias Answer = (status: Int, headers: [String: String], body: Data)
    private let listener: NWListener
    private let queue = DispatchQueue(label: "loopback-stub")
    private let lock = NSLock()
    private var seen: [Seen] = []
    private let answer: @Sendable (Seen) -> Answer
    private(set) var port: UInt16 = 0

    init(answer: @escaping @Sendable (Seen) -> Answer) throws {
        let params = NWParameters.tcp
        params.requiredInterfaceType = .loopback
        params.allowLocalEndpointReuse = true
        listener = try NWListener(using: params, on: .any)
        self.answer = answer
        let ready = DispatchSemaphore(value: 0)
        listener.stateUpdateHandler = { state in
            if case .ready = state { ready.signal() }
        }
        listener.newConnectionHandler = { [weak self] c in self?.serve(c) }
        listener.start(queue: queue)
        _ = ready.wait(timeout: .now() + 5)
        port = listener.port?.rawValue ?? 0
    }

    deinit { listener.cancel() }

    var requests: [Seen] {
        lock.lock(); defer { lock.unlock() }
        return seen
    }

    private func serve(_ c: NWConnection) {
        c.start(queue: queue)
        read(c, buffer: Data())
    }

    private func read(_ c: NWConnection, buffer: Data) {
        c.receive(minimumIncompleteLength: 1, maximumLength: 65536) { [weak self] data, _, done, error in
            guard let self else { return }
            var buffer = buffer
            if let data { buffer.append(data) }
            if let request = Self.parse(buffer) {
                self.lock.lock()
                self.seen.append(request)
                self.lock.unlock()
                let a = self.answer(request)
                var head = "HTTP/1.1 \(a.status) X\r\nContent-Length: \(a.body.count)\r\nConnection: close\r\n"
                for (k, v) in a.headers { head += "\(k): \(v)\r\n" }
                head += "\r\n"
                c.send(content: Data(head.utf8) + a.body, completion: .contentProcessed { _ in c.cancel() })
            } else if done || error != nil {
                c.cancel()
            } else {
                self.read(c, buffer: buffer)
            }
        }
    }

    static func parse(_ data: Data) -> Seen? {
        guard let end = data.range(of: Data("\r\n\r\n".utf8)) else { return nil }
        let head = String(decoding: data[..<end.lowerBound], as: UTF8.self).components(separatedBy: "\r\n")
        let first = head[0].split(separator: " ")
        guard first.count >= 2 else { return nil }
        var headers: [String: String] = [:]
        for line in head.dropFirst() {
            if let colon = line.firstIndex(of: ":") {
                headers[line[..<colon].lowercased()] = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            }
        }
        let length = Int(headers["content-length"] ?? "0") ?? 0
        let body = data[end.upperBound...]
        guard body.count >= length else { return nil }
        return Seen(method: String(first[0]), path: String(first[1]), headers: headers, body: Data(body.prefix(length)))
    }
}

/// Your Data against a local stub: the gateway's sign-in and a query over the hardened
/// client, the address rule, redirects and hosts off the user's list refused, and a
/// feed poll parsed by the core. [RC-24] [NFR-PRV-1] [FR-H-1]
@MainActor
final class UserDataTests: XCTestCase {
    /// A random test password, made here and never printed.
    private let password = UUID().uuidString + UUID().uuidString
    private let token = "tok-" + UUID().uuidString

    private func stub() throws -> LoopbackStub {
        let password = password
        let token = token
        return try LoopbackStub { req in
            let json = { (s: String) in (200, ["Content-Type": "application/json"], Data(s.utf8)) }
            switch (req.method, req.path) {
            case ("POST", "/api/v1/login"):
                let body = (try? JSONSerialization.jsonObject(with: req.body) as? [String: String]) ?? [:]
                guard body["username"] == "sam", body["password"] == password else {
                    return (401, [:], Data(#"{"error":"invalid credentials"}"#.utf8))
                }
                return json(#"{"token":"\#(token)","expiresAt":9999999999}"#)
            case ("GET", "/api/v1/whoami"):
                guard req.headers["authorization"] == "Bearer \(token)" else { return (401, [:], Data()) }
                return json(#"{"subject":"sam","roles":["operator"],"clearance":"internal","compartments":[],"purpose":"operations","expiresAt":1}"#)
            case ("GET", "/api/v1/object-types"):
                guard req.headers["authorization"] == "Bearer \(token)" else { return (401, [:], Data()) }
                return json(#"[{"name":"Transformer","apiName":"transformer","propertyCount":2,"properties":[{"name":"serial","type":"text"},{"name":"rating_kva","type":"int"}],"instanceCount":"1","marking":"internal","compartment":null,"links":[{"label":"connected to → Feeder"}],"actions":[],"updated":"just now"}]"#)
            case ("POST", "/api/v1/query"):
                guard req.headers["authorization"] == "Bearer \(token)" else { return (401, [:], Data()) }
                return json(#"{"results":[{"id":"transformer/TX-40117","typeId":"transformer","propertyValues":{"serial":"TX-40117","rating_kva":"500"},"markings":[{"id":"mk-internal","name":"INTERNAL","level":"internal","compartments":[]}]}],"total":1,"offset":0,"limit":5,"auditId":"AUD-14"}"#)
            case ("GET", "/redirect"):
                return (302, ["Location": "http://127.0.0.1:1/elsewhere"], Data())
            case ("GET", "/feed.json"):
                return json(#"{"feed":"grid","sample":[{"entity":"GB-GRID","kind":"utility-power-grid","observedAt":1792691220,"attrs":{"lat":54.3,"lon":-2.3,"name":"GB electricity grid"}},{"entity":"X","kind":"k","observedAt":1}]}"#)
            default:
                return (404, [:], Data(#"{"error":"not found"}"#.utf8))
            }
        }
    }

    func testSignInAndQueryAGatewayOverTheHardenedClient() async throws {
        let s = try stub()
        let http = UserHTTPClient()
        let base = try LiveGatewayClient.base("http://127.0.0.1:\(s.port)")
        XCTAssertEqual(base.absoluteString, "http://127.0.0.1:\(s.port)/api/v1")
        let client = LiveGatewayClient(http: http)

        // Not on the user's list yet: nothing is sent.
        do {
            _ = try await client.login(base: base, username: "sam", password: password, code: nil)
            XCTFail("sent to a host not on the list")
        } catch let e as LiveGatewayError {
            XCTAssertTrue(e.message.contains("not on the list"), e.message)
        }
        XCTAssertTrue(s.requests.isEmpty)

        http.setAllowed([base])
        do {
            _ = try await client.login(base: base, username: "sam", password: "wrong", code: nil)
            XCTFail("a wrong password signed in")
        } catch let e as LiveGatewayError {
            XCTAssertEqual(e, .refused(401, "invalid credentials"))
        }
        let session = try await client.login(base: base, username: "sam", password: password, code: nil)
        XCTAssertEqual(session.subject, "sam")
        XCTAssertEqual(session.clearance, "internal")
        XCTAssertEqual(session.host, "127.0.0.1:\(s.port)")

        // The list answer decodes strictly as the console's read model.
        let types = try await client.get(session, "/object-types")
        let rows = try gatewayRows(rowType: "MidgardObjectType", json: String(decoding: types, as: UTF8.self))
        guard case let .midgardObjectType(t)? = rows.first?.item else { return XCTFail("no type row") }
        XCTAssertEqual(t.apiName, "transformer")
        XCTAssertEqual(rows.first?.marking.level, .internal, "the marking the item states")

        // A query, as Data Search sends it.
        let call = try XCTUnwrap(LiveRoutes.call("midgard.query", target: "midgard.objectTypes/transformer",
                                                 params: [("maxResults", "50"), ("where", #"[{"property":"rating_kva","op":"eq","value":"500"}]"#)],
                                                 session: session))
        XCTAssertEqual(call.path, "/query")
        let answer = try await client.post(session, call.path, call.body)
        let decoded = try XCTUnwrap(try JSONSerialization.jsonObject(with: answer) as? [String: Any])
        XCTAssertEqual(decoded["auditId"] as? String, "AUD-14")
        let sent = try XCTUnwrap(s.requests.last { $0.path == "/api/v1/query" })
        let body = try XCTUnwrap(try JSONSerialization.jsonObject(with: sent.body) as? [String: Any])
        XCTAssertEqual(body["objectType"] as? String, "transformer")
        XCTAssertNotNil(body["where"] as? [Any])

        // Every request: the generic User-Agent, no cookie; the token only after sign-in.
        for r in s.requests {
            XCTAssertEqual(r.headers["user-agent"], "MidgardMaps/0.1")
            XCTAssertNil(r.headers["cookie"])
        }
        XCTAssertNil(s.requests.first { $0.path == "/api/v1/login" }?.headers["authorization"])
        // The password went in the body of the sign-in only.
        let others = s.requests.filter { $0.path != "/api/v1/login" }
        XCTAssertFalse(others.contains { String(decoding: $0.body, as: UTF8.self).contains(password) })
    }

    func testTheAddressRuleRedirectsAndTheByteCap() async throws {
        let s = try stub()
        for plaintext in ["http://gw.example.org", "http://192.168.1.5:7710"] {
            XCTAssertThrowsError(try LiveGatewayClient.base(plaintext)) { e in
                XCTAssertEqual((e as? LiveGatewayError)?.message,
                               "Refusing a plaintext gateway URL; use https:// unless the edge is on loopback")
            }
        }
        XCTAssertThrowsError(try LiveGatewayClient.base("https://user:pw@gw.example.org"))
        let http = UserHTTPClient()
        let base = try XCTUnwrap(URL(string: "http://127.0.0.1:\(s.port)/"))
        http.setAllowed([base])
        // A redirect is refused, never followed.
        let redirected = await http.send("GET", try XCTUnwrap(URL(string: "http://127.0.0.1:\(s.port)/redirect")))
        XCTAssertEqual(redirected, .network)
        XCTAssertFalse(s.requests.contains { $0.path == "/elsewhere" })
        // Another port of the same host is another address: not sent.
        let other = await http.send("GET", try XCTUnwrap(URL(string: "http://127.0.0.1:1/x")))
        XCTAssertEqual(other, .refused)
        // The session keeps nothing.
        let c = UserHTTPClient.configuration()
        XCTAssertNil(c.httpCookieStorage)
        XCTAssertNil(c.urlCache)
        XCTAssertNil(c.urlCredentialStorage)
        XCTAssertEqual(c.timeoutIntervalForRequest, 15)
        XCTAssertEqual(UserHTTPClient.maxBytes, 8 * 1024 * 1024)
    }

    func testAFeedPollIsParsedAsAnUnverifiedLayer() async throws {
        let s = try stub()
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "mg-test-\(UUID().uuidString)"))
        let data = UserDataModel(app: AppModel(), http: UserHTTPClient(), defaults: defaults)
        XCTAssertEqual(data.saveFeed(name: "Grid", url: "http://feeds.example.org/x", interval: 60),
                       "Refusing a plaintext feed URL; use https:// unless the feed is on loopback")
        XCTAssertNil(data.saveFeed(name: "Grid", url: "http://127.0.0.1:\(s.port)/feed.json", interval: 5))
        let feed = try XCTUnwrap(data.feeds.first)
        XCTAssertEqual(feed.interval, 30, "the interval is at least 30 s")
        XCTAssertEqual(data.allowedOrigins, ["http://127.0.0.1:\(s.port)"])
        // Nothing is fetched until the picture asks.
        XCTAssertTrue(s.requests.isEmpty)
        await data.poll(feed)
        let state = try XCTUnwrap(data.feedStates[feed.id])
        XCTAssertNil(state.error)
        XCTAssertEqual(state.layers.first?.feed, "grid")
        XCTAssertEqual(state.layers.first?.total, 2)
        let layer = try XCTUnwrap(data.observationLayers.first)
        XCTAssertEqual(layer.label, "Grid · UNVERIFIED FEED")
        XCTAssertEqual(layer.points.map(\.label), ["GB electricity grid"])
        XCTAssertTrue(layer.unverified)
        // On the 2D reference layout a UK position lands inside, as the sample tracks do.
        XCTAssertNotNil(HeimdallPictureModel.referencePoint(lat: 57.35, lon: -4.2))
        XCTAssertNil(HeimdallPictureModel.referencePoint(lat: -33.9, lon: 151.2))
        data.removeFeed(feed.id)
        XCTAssertTrue(data.allowedOrigins.isEmpty)
    }

    func testOntologyFilesAreTold() {
        XCTAssertTrue(UserDataModel.isOntology(name: "ontology.json", text: "{}"))
        XCTAssertTrue(UserDataModel.isOntology(name: "grid.json", text: #"{"version":1,"objectTypes":[]}"#))
        XCTAssertFalse(UserDataModel.isOntology(name: "network.jsonl", text: #"{"object":"x","key":"k"}"#))
        XCTAssertFalse(UserDataModel.isOntology(name: "obs.json", text: #"{"feed":"x","sample":[]}"#))
    }
}

import JavaScriptCore
import XCTest
@testable import MidgardShellCore

/// Answers requests for one host with a canned response and records what it was sent.
/// Each test uses its own host, so tests run in parallel without sharing a stub.
final class StubProtocol: URLProtocol, @unchecked Sendable {
    struct Canned: Sendable {
        var status: Int
        var body: Data
        var failure: URLError.Code?
    }
    struct Seen: Sendable {
        var method: String?
        var url: URL?
        var contentType: String?
        var body: Data
    }

    private static let lock = NSLock()
    nonisolated(unsafe) private static var canned: [String: Canned] = [:]
    nonisolated(unsafe) private static var seen: [String: Seen] = [:]

    static func register(host: String, _ response: Canned) {
        lock.withLock { canned[host] = response }
    }
    static func request(host: String) -> Seen? {
        lock.withLock { seen[host] }
    }
    static func forget(host: String) {
        lock.withLock { canned[host] = nil; seen[host] = nil }
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let host = request.url?.host ?? ""
        var body = request.httpBody ?? Data()
        if body.isEmpty, let stream = request.httpBodyStream {
            stream.open()
            var buffer = [UInt8](repeating: 0, count: 4096)
            while case let n = stream.read(&buffer, maxLength: buffer.count), n > 0 {
                body.append(buffer, count: n)
            }
            stream.close()
        }
        let seen = Seen(method: request.httpMethod, url: request.url,
                        contentType: request.value(forHTTPHeaderField: "Content-Type"), body: body)
        let canned = Self.lock.withLock { () -> Canned? in
            Self.seen[host] = seen
            return Self.canned[host]
        }
        guard let canned else {
            client?.urlProtocol(self, didFailWithError: URLError(.cannotConnectToHost))
            return
        }
        if let failure = canned.failure {
            client?.urlProtocol(self, didFailWithError: URLError(failure))
            return
        }
        let response = HTTPURLResponse(url: request.url!, statusCode: canned.status,
                                       httpVersion: "HTTP/1.1",
                                       headerFields: ["Content-Type": "application/json"])!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: canned.body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}

final class SessionTests: XCTestCase {
    var host: String!
    var gateway: URL!
    var session: URLSession!
    let user = DemoUser(username: "analyst", password: "pa\"ss<word>", label: "Analyst")

    override func setUp() {
        host = "stub-\(UUID().uuidString.lowercased()).test"
        gateway = URL(string: "http://\(host!):61234/api/v1")!
        let config = URLSessionConfiguration.ephemeral
        config.protocolClasses = [StubProtocol.self]
        session = URLSession(configuration: config)
    }

    override func tearDown() {
        StubProtocol.forget(host: host)
        session.invalidateAndCancel()
    }

    private func respond(_ status: Int, _ json: String) {
        StubProtocol.register(host: host, .init(status: status, body: Data(json.utf8), failure: nil))
    }

    // MARK: Demo users

    func testLoadsDemoUsers() throws {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("demo-users-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: file) }
        try Data(#"[{"username":"a","password":"p1","label":"Analyst"},{"username":"b","password":"p2","label":"Boss"}]"#.utf8)
            .write(to: file)
        let users = try DemoUsers.load(from: file)
        XCTAssertEqual(users, [DemoUser(username: "a", password: "p1", label: "Analyst"),
                               DemoUser(username: "b", password: "p2", label: "Boss")])
    }

    func testMalformedDemoUsersThrows() throws {
        let file = FileManager.default.temporaryDirectory
            .appendingPathComponent("demo-users-\(UUID().uuidString).json")
        defer { try? FileManager.default.removeItem(at: file) }
        try Data(#"[{"username":"a"}]"#.utf8).write(to: file)
        XCTAssertThrowsError(try DemoUsers.load(from: file))
    }

    func testPasswordIsNeverDescribed() {
        let secret = "hunter2-secret"
        let u = DemoUser(username: "a", password: secret, label: "Analyst")
        XCTAssertFalse(String(describing: u).contains(secret))
        XCTAssertFalse(String(reflecting: u).contains(secret))
        XCTAssertFalse("\(u)".contains(secret))
        var dumped = ""
        dump(u, to: &dumped)
        XCTAssertFalse(dumped.contains(secret))
        XCTAssertTrue(String(describing: u).contains("a"))
    }

    // MARK: Login

    func testSuccessfulLogin() async throws {
        respond(200, #"{"token":"abc123","subject":"analyst","clearance":"restricted","roles":["analyst","viewer"],"expiresAt":"1792000000"}"#)
        let info = try await Session.login(gateway: gateway, user: user, session: session)
        XCTAssertEqual(info.token, "abc123")
        XCTAssertEqual(info.subject, "analyst")
        XCTAssertEqual(info.clearance, "restricted")
        XCTAssertEqual(info.roles, ["analyst", "viewer"])
        XCTAssertEqual(info.expiresAt, Date(timeIntervalSince1970: 1_792_000_000))
    }

    func testLoginToleratesNullClaimsAndNumericExpiry() async throws {
        respond(200, #"{"token":"t","subject":"s","clearance":null,"roles":null,"expiresAt":1792000000}"#)
        let info = try await Session.login(gateway: gateway, user: user, session: session)
        XCTAssertNil(info.clearance)
        XCTAssertEqual(info.roles, [])
        XCTAssertEqual(info.expiresAt, Date(timeIntervalSince1970: 1_792_000_000))
    }

    func testLoginPostsJSONCredentials() async throws {
        respond(200, #"{"token":"t","subject":"s","clearance":"open","roles":[],"expiresAt":"1"}"#)
        _ = try await Session.login(gateway: gateway, user: user, session: session)
        let seen = try XCTUnwrap(StubProtocol.request(host: host))
        XCTAssertEqual(seen.method, "POST")
        XCTAssertEqual(seen.url?.path, "/api/v1/login")
        XCTAssertEqual(seen.contentType, "application/json")
        let body = try XCTUnwrap(JSONSerialization.jsonObject(with: seen.body) as? [String: String])
        XCTAssertEqual(body, ["username": "analyst", "password": "pa\"ss<word>"])
    }

    func testTrailingSlashOnGatewayIsTolerated() async throws {
        respond(200, #"{"token":"t","subject":"s","clearance":"open","roles":[],"expiresAt":"1"}"#)
        let slashed = URL(string: gateway.absoluteString + "/")!
        _ = try await Session.login(gateway: slashed, user: user, session: session)
        XCTAssertEqual(StubProtocol.request(host: host)?.url?.path, "/api/v1/login")
    }

    func test401IsInvalidCredentials() async {
        respond(401, #"{"error":"invalid credentials"}"#)
        do {
            _ = try await Session.login(gateway: gateway, user: user, session: session)
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? SessionError, .invalidCredentials)
        }
    }

    func test500IsHTTPError() async {
        respond(500, #"{"error":"idp unavailable"}"#)
        do {
            _ = try await Session.login(gateway: gateway, user: user, session: session)
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? SessionError, .http(status: 500, message: "idp unavailable"))
        }
    }

    func testTransportErrorIsWrapped() async {
        StubProtocol.register(host: host, .init(status: 0, body: Data(), failure: .cannotConnectToHost))
        do {
            _ = try await Session.login(gateway: gateway, user: user, session: session)
            XCTFail("expected an error")
        } catch {
            guard case .transport = error as? SessionError else {
                return XCTFail("expected transport, got \(error)")
            }
        }
    }

    func testMalformedBodyIsBadResponse() async {
        respond(200, #"{"subject":"s"}"#)
        do {
            _ = try await Session.login(gateway: gateway, user: user, session: session)
            XCTFail("expected an error")
        } catch {
            guard case .badResponse = error as? SessionError else {
                return XCTFail("expected badResponse, got \(error)")
            }
        }
    }

    func testErrorsNeverCarryThePassword() async {
        respond(500, #"{"error":"idp unavailable"}"#)
        do {
            _ = try await Session.login(gateway: gateway, user: user, session: session)
            XCTFail("expected an error")
        } catch {
            XCTAssertFalse(String(describing: error).contains(user.password))
        }
    }

    func testDefaultSessionBypassesProxies() {
        let config = Session.loopback.configuration
        XCTAssertEqual(config.connectionProxyDictionary?.count, 0)
        XCTAssertEqual(config.timeoutIntervalForRequest, 10)
        XCTAssertNil(config.urlCache)
    }

    // MARK: Refresh

    func testRefreshTenMinutesBeforeExpiry() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [],
                               expiresAt: now.addingTimeInterval(12 * 3600))
        XCTAssertEqual(Session.refreshDate(for: info, now: now), now.addingTimeInterval(12 * 3600 - 600))
    }

    func testRefreshAtHalfLifeWhenShort() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [],
                               expiresAt: now.addingTimeInterval(600))
        XCTAssertEqual(Session.refreshDate(for: info, now: now), now.addingTimeInterval(300))
    }

    func testRefreshSoonButNotImmediatelyWhenExpired() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [],
                               expiresAt: now.addingTimeInterval(-5))
        XCTAssertEqual(Session.refreshDate(for: info, now: now), now.addingTimeInterval(30))
    }

    func testRefreshNeverSoonerThanThirtySeconds() {
        let now = Date(timeIntervalSince1970: 1_000_000)
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [],
                               expiresAt: now.addingTimeInterval(40))
        XCTAssertEqual(Session.refreshDate(for: info, now: now), now.addingTimeInterval(30))
    }

    func testSessionInfoNeverShowsToken() {
        let info = SessionInfo(token: "tok-SECRET-123", subject: "s", clearance: "open", roles: [],
                               expiresAt: Date())
        XCTAssertFalse(String(describing: info).contains("SECRET"))
        XCTAssertFalse(String(reflecting: info).contains("SECRET"))
        var dumped = ""
        dump(info, to: &dumped)
        XCTAssertFalse(dumped.contains("SECRET"))
    }

    // MARK: Injection

    private struct Injected {
        var gateway: String?
        var console: String?
        var token: String?
        var who: [String: Any]?
    }

    static let origin = "http://127.0.0.1:61235"

    private func evaluate(_ script: String, origin: String = SessionTests.origin) throws -> Injected {
        let context = try XCTUnwrap(JSContext())
        var exception: String?
        context.exceptionHandler = { _, value in exception = value?.toString() }
        context.evaluateScript("""
            var window = {};
            var location = { origin: \(Session.literal(origin)) };
            var sessionStorage = { store: {}, setItem: function (k, v) { this.store[k] = String(v); } };
            """)
        context.evaluateScript(script)
        XCTAssertNil(exception)
        let window = context.objectForKeyedSubscript("window")
        let store = context.objectForKeyedSubscript("sessionStorage").objectForKeyedSubscript("store")
        func string(_ v: JSValue?) -> String? { (v?.isString ?? false) ? v?.toString() : nil }
        var who: [String: Any]?
        if let text = string(store?.objectForKeyedSubscript("midgard.session.who")) {
            who = try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any]
        }
        return Injected(gateway: string(window?.objectForKeyedSubscript("__MIDGARD_GATEWAY__")),
                        console: string(window?.objectForKeyedSubscript("__MIDGARD_CONSOLE__")),
                        token: string(store?.objectForKeyedSubscript("midgard.session.token")),
                        who: who)
    }

    func testInjectionScriptRoundTrips() throws {
        let info = SessionInfo(token: "deadbeef", subject: "analyst", clearance: "restricted",
                               roles: ["analyst", "viewer"], expiresAt: Date())
        let got = try evaluate(Session.injectionScript(
            origin: Self.origin, gatewayURL: "http://127.0.0.1:61234/api/v1",
            midgardConsoleURL: "http://127.0.0.1:61235/", session: info))
        XCTAssertEqual(got.gateway, "http://127.0.0.1:61234/api/v1")
        XCTAssertEqual(got.console, "http://127.0.0.1:61235/")
        XCTAssertEqual(got.token, "deadbeef")
        XCTAssertEqual(got.who?["subject"] as? String, "analyst")
        XCTAssertEqual(got.who?["clearance"] as? String, "restricted")
        XCTAssertEqual(got.who?["roles"] as? [String], ["analyst", "viewer"])
    }

    func testInjectionScriptEscapesHostileValues() throws {
        let nasty = "a'b\"c\\d</script><script>alert(1)</script>\u{2028}e\u{2029}f\n\u{0}g${x}`h"
        let info = SessionInfo(token: nasty, subject: nasty, clearance: nasty,
                               roles: [nasty, "plain"], expiresAt: Date())
        let script = Session.injectionScript(origin: Self.origin, gatewayURL: nasty, midgardConsoleURL: nasty, session: info)
        XCTAssertFalse(script.contains("</script"))
        XCTAssertFalse(script.contains("\u{2028}"))
        XCTAssertFalse(script.contains("\u{2029}"))
        let got = try evaluate(script)
        XCTAssertEqual(got.gateway, nasty)
        XCTAssertEqual(got.console, nasty)
        XCTAssertEqual(got.token, nasty)
        XCTAssertEqual(got.who?["subject"] as? String, nasty)
        XCTAssertEqual(got.who?["clearance"] as? String, nasty)
        XCTAssertEqual(got.who?["roles"] as? [String], [nasty, "plain"])
    }

    func testInjectionOmitsMissingClearance() throws {
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [], expiresAt: Date())
        let got = try evaluate(Session.injectionScript(origin: Self.origin, gatewayURL: "g", midgardConsoleURL: "c", session: info))
        XCTAssertNil(got.who?["clearance"])
        XCTAssertEqual(got.who?["roles"] as? [String], [])
    }

    func testInjectionOnlyRunsOnItsOrigin() throws {
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [], expiresAt: Date())
        let script = Session.injectionScript(origin: Self.origin, gatewayURL: "g",
                                             midgardConsoleURL: "c", session: info)
        let matching = try evaluate(script, origin: Self.origin)
        XCTAssertEqual(matching.token, "t")
        XCTAssertEqual(matching.gateway, "g")
        for other in ["http://127.0.0.1:9999", "https://evil.example", "null", ""] {
            let got = try evaluate(script, origin: other)
            XCTAssertNil(got.token, other)
            XCTAssertNil(got.who, other)
            XCTAssertNil(got.gateway, other)
            XCTAssertNil(got.console, other)
        }
    }

    func testInjectionOriginIsEscaped() throws {
        let info = SessionInfo(token: "t", subject: "s", clearance: nil, roles: [], expiresAt: Date())
        let odd = "http://a'b\"c</script>\u{2028}"
        let script = Session.injectionScript(origin: odd, gatewayURL: "g", midgardConsoleURL: "c", session: info)
        XCTAssertFalse(script.contains("</script"))
        XCTAssertEqual(try evaluate(script, origin: odd).token, "t")
    }
}

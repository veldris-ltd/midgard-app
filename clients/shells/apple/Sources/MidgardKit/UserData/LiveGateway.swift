import Foundation
@preconcurrency import MidgardCore

/// A signed-in session with an enclosure gateway (`services/http-gateway`), over its
/// documented HTTP API, as the SDK's `HttpGateway` calls it. The token is held in
/// memory only, for as long as the app runs connected: it is never written to disk or
/// the keychain, never logged, and sent only to the gateway's own address.
/// [RC-24] [RC-37] [VLD-ANB-MIDGARD-0014]
public struct LiveSession: Sendable {
    /// The API base, `…/api/v1`.
    public let base: URL
    /// The gateway's host (and port), for the LIVE chip.
    public let host: String
    let token: String
    /// `whoami`: who the gateway signed in, at what clearance.
    public let subject: String
    public let clearance: String
    public let roles: [String]
    public let compartments: [String]
    public let purpose: String
}

/// Why a gateway call did not give an answer.
public enum LiveGatewayError: Error, Equatable, Sendable {
    /// The address breaks the address rule (the SDK's words).
    case address(String)
    /// The gateway answered with an error status and its message.
    case refused(Int, String)
    /// No usable answer (connection, time limit, a redirect, too large).
    case unreachable(String)
    /// The token expired or no longer verifies: the session is over.
    case signedOut
    /// An answer that is not what the API documents.
    case malformed(String)

    public var message: String {
        switch self {
        case let .address(why): return why
        case let .refused(_, why): return why
        case let .unreachable(why): return why
        case .signedOut: return "The gateway ended the session: sign in again."
        case let .malformed(why): return why
        }
    }
}

/// The calls, over `UserHTTPClient`. [RC-24]
public struct LiveGatewayClient: Sendable {
    let http: UserHTTPClient

    public init(http: UserHTTPClient) {
        self.http = http
    }

    /// The API base for `address`, by the SDK's rule (`checkGatewayAddress`).
    public static func base(_ address: String) throws -> URL {
        do {
            guard let url = URL(string: try checkGatewayAddress(url: address)) else {
                throw LiveGatewayError.address("Refusing a gateway URL that does not parse: \(address)")
            }
            return url
        } catch let ClientError.Refused(reason) {
            throw LiveGatewayError.address(reason)
        }
    }

    static func describe(_ outcome: UserHTTPOutcome, host: String) -> LiveGatewayError {
        switch outcome {
        case let .status(code, body):
            if code == 401 { return .signedOut }
            let message = (try? JSONSerialization.jsonObject(with: body) as? [String: Any])?["error"] as? String
            return .refused(code, message ?? "\(host) answered \(code)")
        case .oversize: return .unreachable("\(host) sent more than 8 MiB; nothing was read")
        case .refused: return .unreachable("\(host) is not on the list of addresses this app may reach")
        case .network, .ok: return .unreachable("\(host) could not be reached (no answer, a redirect, or the 15 s limit)")
        }
    }

    /// `POST /login`, then `GET /whoami`. A refused login says the gateway's words
    /// ("invalid credentials"); a code is sent only when given.
    public func login(base: URL, username: String, password: String, code: String?) async throws -> LiveSession {
        let host = base.host.map { h in base.port.map { "\(h):\($0)" } ?? h } ?? base.absoluteString
        var body: [String: String] = ["username": username, "password": password]
        if let code, !code.isEmpty { body["totp"] = code }
        let data = try JSONSerialization.data(withJSONObject: body)
        let outcome = await http.send("POST", base.appendingPathComponent("login"), body: data)
        guard case let .ok(answer) = outcome else {
            if case .status(401, _) = outcome {
                let message = (try? JSONSerialization.jsonObject(with: Self.body(outcome)) as? [String: Any])?["error"] as? String
                throw LiveGatewayError.refused(401, message ?? "invalid credentials")
            }
            throw Self.describe(outcome, host: host)
        }
        guard let token = (try? JSONSerialization.jsonObject(with: answer) as? [String: Any])?["token"] as? String,
              !token.isEmpty else {
            throw LiveGatewayError.malformed("\(host)'s login answer carried no token")
        }
        let who = await http.send("GET", base.appendingPathComponent("whoami"), bearer: token)
        guard case let .ok(whoData) = who,
              let me = try? JSONSerialization.jsonObject(with: whoData) as? [String: Any] else {
            throw Self.describe(who, host: host)
        }
        return LiveSession(base: base, host: host, token: token, subject: me["subject"] as? String ?? username,
                           clearance: me["clearance"] as? String ?? "open", roles: me["roles"] as? [String] ?? [],
                           compartments: me["compartments"] as? [String] ?? [],
                           purpose: me["purpose"] as? String ?? "operations")
    }

    private static func body(_ o: UserHTTPOutcome) -> Data {
        if case let .status(_, d) = o { return d }
        return Data()
    }

    /// `POST /logout`: revokes the token. Best effort; the token is dropped either way.
    public func logout(_ s: LiveSession) async {
        _ = await http.send("POST", s.base.appendingPathComponent("logout"), body: Data("{}".utf8), bearer: s.token)
    }

    /// A path under the base (`/object-types`, `/audit?limit=50`).
    static func url(_ s: LiveSession, _ path: String) -> URL? {
        URL(string: s.base.absoluteString + path)
    }

    public func get(_ s: LiveSession, _ path: String) async throws -> Data {
        guard let url = Self.url(s, path) else { throw LiveGatewayError.malformed("bad path \(path)") }
        let outcome = await http.send("GET", url, bearer: s.token)
        guard case let .ok(data) = outcome else { throw Self.describe(outcome, host: s.host) }
        return data
    }

    public func post(_ s: LiveSession, _ path: String, _ body: [String: Any]) async throws -> Data {
        guard let url = Self.url(s, path) else { throw LiveGatewayError.malformed("bad path \(path)") }
        let data = try JSONSerialization.data(withJSONObject: body, options: [.sortedKeys])
        let outcome = await http.send("POST", url, body: data, bearer: s.token)
        guard case let .ok(answer) = outcome else { throw Self.describe(outcome, host: s.host) }
        return answer
    }
}

/// Which of the consoles' collections a gateway serves, and how. Midgard's fabric,
/// governance and platform read models; nothing of Admin, Heimdall, Forge, Mimir or
/// Bifrost (the gateway's `/admin/*` routes are passkey enrolment, not Admin's read
/// models). [AC-M-4]
public enum LiveRoutes {
    /// Collection → `GET` path.
    public static let lists: [String: String] = [
        "midgard.objectTypes": "/object-types",
        "midgard.approvals": "/approvals",
        "midgard.audit": "/audit?limit=50",
        "midgard.policies": "/policies",
        "midgard.identities": "/identities",
        "midgard.keys": "/keys",
        "midgard.computeJobs": "/compute",
        "midgard.observability": "/observability",
        "midgard.resilience": "/resilience",
        "midgard.contractSurface": "/sdk-surface",
    ]
    /// Read by querying each type (`POST /query`), as the console's object list is.
    public static let objects = "midgard.objects"
    /// The computed reads and actions the gateway runs.
    public static let verbs: Set<String> = [
        "midgard.query", "midgard.searchAround", "midgard.simulatePolicy", "midgard.requestApproval",
        "midgard.decideApproval", "midgard.applyAction",
    ]

    public static func serves(_ collection: String) -> Bool {
        lists[collection] != nil || collection == objects
    }

    /// Midgard's screens, by the collection each one is built on.
    public static func serves(screen: String) -> Bool {
        switch screen {
        case "Object Fabric", "Data Search", "Search Around", "Policies", "Identity", "Keys", "Audit", "Compute",
             "Observability", "Resilience", "SDK":
            return true
        default:
            return false
        }
    }

    /// The JSON body of a parameter (`where`), or the text.
    static func value(_ text: String) -> Any {
        (try? JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])) ?? text
    }

    /// One of the console's actions as the gateway's call: its path and body.
    public static func call(_ verb: String, target: String, params: [(String, String)],
                            session: LiveSession) -> (path: String, body: [String: Any])? {
        let p = Dictionary(params, uniquingKeysWith: { _, last in last })
        let (collection, id) = target.split(separator: "/", maxSplits: 1).map(String.init).pair
        switch verb {
        case "midgard.query":
            guard collection == "midgard.objectTypes" else { return nil }
            var body: [String: Any] = ["objectType": id, "maxResults": Int(p["maxResults"] ?? "") ?? 5]
            if let w = p["where"] { body["where"] = value(w) }
            if let o = p["orderBy"], !o.isEmpty { body["orderBy"] = o }
            if p["descending"] == "true" { body["descending"] = true }
            if let off = Int(p["offset"] ?? "") { body["offset"] = off }
            return ("/query", body)
        case "midgard.searchAround":
            guard collection == "midgard.objectTypes" else { return nil }
            var body: [String: Any] = ["seedId": "\(id)/\(p["seedId"].flatMap { $0.isEmpty ? nil : $0 } ?? "*")",
                                       "hops": Int(p["hops"] ?? "") ?? 2]
            if let l = Int(p["limit"] ?? "") { body["limit"] = l }
            return ("/search-around", body)
        case "midgard.simulatePolicy":
            return ("/policy/simulate", ["subject": session.subject, "purpose": p["purpose"] ?? session.purpose,
                                         "clearance": session.clearance, "roles": session.roles,
                                         "marking": p["marking"] ?? "open"])
        case "midgard.requestApproval":
            return ("/approvals", ["actionType": p["actionType"] ?? "", "target": id])
        case "midgard.decideApproval":
            let verb = p["approve"] == "true" ? "approve" : "reject"
            let escaped = id.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed.subtracting(["/"])) ?? id
            return ("/approvals/\(escaped)/\(verb)", [:])
        case "midgard.applyAction":
            var body: [String: Any] = ["actionType": p["actionType"] ?? "", "params": ["target": id]]
            if let t = p["humanGateToken"], !t.isEmpty { body["humanGateToken"] = t }
            return ("/actions", body)
        default:
            return nil
        }
    }
}

private extension Array where Element == String {
    var pair: (String, String) { (first ?? "", count > 1 ? self[1] : "") }
}

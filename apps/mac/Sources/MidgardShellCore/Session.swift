import Foundation

/// One of the presenter's demo accounts, from `demo-users.json`. Its password is never
/// shown: every description of a `DemoUser` (print, string interpolation, `dump`, the
/// debugger's summary) redacts it.
public struct DemoUser: Sendable, Equatable, Codable, Identifiable {
    public var username: String
    public var password: String
    /// What the sign-in menu shows ("Analyst", "Supervisor" …).
    public var label: String

    public var id: String { username }

    public init(username: String, password: String, label: String) {
        self.username = username
        self.password = password
        self.label = label
    }
}

extension DemoUser: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    public var description: String { "DemoUser(\(username), \(label), password: <redacted>)" }
    public var debugDescription: String { description }
    public var customMirror: Mirror {
        Mirror(self, children: ["username": username, "label": label, "password": "<redacted>"],
               displayStyle: .struct)
    }
}

public enum DemoUsers {
    /// Reads `[{"username","password","label"}]`.
    public static func load(from file: URL) throws -> [DemoUser] {
        try JSONDecoder().decode([DemoUser].self, from: Data(contentsOf: file))
    }
}

/// A signed-in presenter session, as the gateway's `/login` returns it.
public struct SessionInfo: Sendable, Equatable {
    /// The hex bearer token.
    public var token: String
    public var subject: String
    /// The clearance band the token grants, for display; absent if the gateway gave none.
    public var clearance: String?
    public var roles: [String]
    public var expiresAt: Date

    public init(token: String, subject: String, clearance: String?, roles: [String], expiresAt: Date) {
        self.token = token
        self.subject = subject
        self.clearance = clearance
        self.roles = roles
        self.expiresAt = expiresAt
    }
}

extension SessionInfo: CustomStringConvertible, CustomDebugStringConvertible, CustomReflectable {
    /// Without the token.
    public var description: String {
        "SessionInfo(\(subject), clearance: \(clearance ?? "none"), roles: \(roles), expires: \(expiresAt))"
    }
    public var debugDescription: String { description }
    public var customMirror: Mirror {
        Mirror(self, children: ["token": "<redacted>", "subject": subject, "clearance": clearance as Any,
                                "roles": roles, "expiresAt": expiresAt],
               displayStyle: .struct)
    }
}

public enum SessionError: Error, Equatable, CustomStringConvertible {
    /// The gateway answered 401.
    case invalidCredentials
    /// Any other answer than 200, with the gateway's `error` message (or the start of the body).
    case http(status: Int, message: String)
    /// The gateway could not be reached.
    case transport(String)
    /// A 200 whose body was not a login result.
    case badResponse(String)

    public var description: String {
        switch self {
        case .invalidCredentials: return "invalid credentials"
        case .http(let status, let message): return "login failed: HTTP \(status): \(message)"
        case .transport(let reason): return "gateway unreachable: \(reason)"
        case .badResponse(let reason): return "unexpected login response: \(reason)"
        }
    }
}

public enum Session {
    /// Loopback only: no proxies, no cache, no cookies kept, 10 s timeout.
    public static let loopback: URLSession = {
        let config = URLSessionConfiguration.ephemeral
        config.connectionProxyDictionary = [:]
        config.timeoutIntervalForRequest = 10
        config.timeoutIntervalForResource = 10
        config.urlCache = nil
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: config)
    }()

    /// `POST <gateway>/login {username, password}`. `gateway` is the API base
    /// (`http://127.0.0.1:<port>/api/v1`).
    public static func login(gateway: URL, user: DemoUser,
                             session: URLSession = Session.loopback) async throws -> SessionInfo {
        var request = URLRequest(url: gateway.appendingPathComponent("login"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.timeoutInterval = 10
        request.httpBody = try JSONSerialization.data(
            withJSONObject: ["username": user.username, "password": user.password])

        let data: Data
        let response: URLResponse
        do {
            (data, response) = try await session.data(for: request)
        } catch {
            throw SessionError.transport(error.localizedDescription)
        }
        guard let http = response as? HTTPURLResponse else {
            throw SessionError.badResponse("not an HTTP response")
        }
        switch http.statusCode {
        case 200: return try parse(data)
        case 401: throw SessionError.invalidCredentials
        default: throw SessionError.http(status: http.statusCode, message: message(in: data))
        }
    }

    /// The soonest a refresh is scheduled, so a short-lived or already-expired token (a
    /// misconfigured TTL, clock skew) cannot drive a tight sign-in loop.
    public static let minimumRefreshDelay: TimeInterval = 30

    /// When to sign in again: ten minutes before expiry, or halfway through the remaining
    /// life if that is sooner, but never sooner than `minimumRefreshDelay` from now.
    public static func refreshDate(for info: SessionInfo, now: Date = Date()) -> Date {
        let floor = now.addingTimeInterval(minimumRefreshDelay)
        let remaining = info.expiresAt.timeIntervalSince(now)
        guard remaining > 0 else { return floor }
        return max(floor, info.expiresAt.addingTimeInterval(-min(600, remaining / 2)))
    }

    /// JavaScript to run at document start in every console page: where the gateway and the
    /// Midgard console are, and the signed-in session as `AuthGate` reads it. It does
    /// nothing unless the page's `location.origin` is exactly `origin` (the app's own web
    /// server, e.g. `http://127.0.0.1:61235`), so the token never reaches another page.
    /// Every value is embedded as an escaped JSON literal.
    public static func injectionScript(origin: String, gatewayURL: String, midgardConsoleURL: String,
                                       session: SessionInfo) -> String {
        var who: [String: Any] = ["subject": session.subject, "roles": session.roles]
        if let clearance = session.clearance { who["clearance"] = clearance }
        return """
            (function () {
              if (typeof location === 'undefined' || location.origin !== \(literal(origin))) { return; }
              window.__MIDGARD_GATEWAY__ = \(literal(gatewayURL));
              window.__MIDGARD_CONSOLE__ = \(literal(midgardConsoleURL));
              try {
                sessionStorage.setItem('midgard.session.token', \(literal(session.token)));
                sessionStorage.setItem('midgard.session.who', JSON.stringify(\(literal(who))));
              } catch (e) {}
            })();
            """
    }

    /// `value` as JSON text that is also a safe JavaScript literal inside a page: `<` is
    /// escaped (no `</script>` or `<!--`), and U+2028/U+2029, which JSON allows raw but
    /// older JavaScript does not, are escaped too. Only the inside of strings can hold
    /// these characters, where `\uXXXX` means the same thing.
    static func literal(_ value: Any) -> String {
        guard let data = try? JSONSerialization.data(
                withJSONObject: value, options: [.fragmentsAllowed, .sortedKeys]),
              let json = String(data: data, encoding: .utf8)
        else { return "null" }
        return json
            .replacingOccurrences(of: "<", with: "\\u003c")
            .replacingOccurrences(of: ">", with: "\\u003e")
            .replacingOccurrences(of: "\u{2028}", with: "\\u2028")
            .replacingOccurrences(of: "\u{2029}", with: "\\u2029")
    }

    private struct Wire: Decodable {
        var token: String
        var subject: String
        var clearance: String?
        var roles: [String]?
        var expiresAt: Seconds

        /// The gateway sends unix seconds as a string; a number is accepted too.
        struct Seconds: Decodable {
            var value: TimeInterval
            init(from decoder: Decoder) throws {
                let c = try decoder.singleValueContainer()
                if let n = try? c.decode(Double.self) {
                    value = n
                } else if let n = TimeInterval(try c.decode(String.self)) {
                    value = n
                } else {
                    throw DecodingError.dataCorruptedError(in: c, debugDescription: "expiresAt is not unix seconds")
                }
            }
        }
    }

    private static func parse(_ data: Data) throws -> SessionInfo {
        do {
            let wire = try JSONDecoder().decode(Wire.self, from: data)
            return SessionInfo(token: wire.token, subject: wire.subject, clearance: wire.clearance,
                               roles: wire.roles ?? [],
                               expiresAt: Date(timeIntervalSince1970: wire.expiresAt.value))
        } catch {
            throw SessionError.badResponse(String(describing: error))
        }
    }

    private static func message(in data: Data) -> String {
        if let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let message = object["error"] as? String {
            return message
        }
        return String(decoding: data.prefix(200), as: UTF8.self)
    }
}

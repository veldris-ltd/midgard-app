import Foundation
@preconcurrency import MidgardCore

/// One point of a user-added layer on Heimdall's picture.
public struct UserMapPoint: Identifiable, Equatable, Sendable {
    public let id: String
    public let label: String
    public let kind: String
    public let lat: Double
    public let lon: Double
}

/// A layer the user added to the operating picture: an imported observations file, a
/// feed URL's last answer, or imported objects that carry a position. None is signed
/// by a certified mesh identity, so none is fused into a signed track: observation
/// layers are labelled UNVERIFIED FEED wherever they are named. [FR-H-1]
public struct UserMapLayer: Identifiable, Equatable, Sendable {
    public enum Origin: Equatable, Sendable { case file, feed, objects }
    public let id: String
    public let name: String
    public let origin: Origin
    public let points: [UserMapPoint]
    /// Observations or objects in it, placed or not.
    public let total: Int

    /// Its label on the layer list, the legend and the feeds rows.
    public var label: String {
        switch origin {
        case .file, .feed: return "\(name) · UNVERIFIED FEED"
        case .objects: return "\(name) · IMPORTED"
        }
    }

    public var unverified: Bool { origin != .objects }
}

/// A feed URL the user added: polled while Heimdall's picture is open.
public struct UserFeedConfig: Codable, Identifiable, Equatable, Sendable {
    public static let minimumInterval = 30
    public static let defaultInterval = 60
    public var id: String
    public var name: String
    public var url: String
    public var interval: Int
}

/// The state of one feed URL.
public struct UserFeedState: Equatable, Sendable {
    public var lastFetch: Date?
    public var error: String?
    public var layers: [UserLayerInfo] = []
}

/// Your Data: what the user adds to the app — files imported (checked as `mg-admin
/// import` checks them, in the core), an enclosure gateway to read live, and feed URLs
/// for the picture — and the list of addresses the app may reach for them. The files
/// are kept sealed under the device root (`user-data.sealed`); the gateway's token in
/// memory only; the feeds' names, addresses and intervals in the app's preferences.
/// Nothing is fetched unless the gateway is connected or Heimdall's picture is open.
/// [DD-1] [RC-10] [RC-24] [NFR-PRV-1] [AC-C-13]
@MainActor
public final class UserDataModel: ObservableObject {
    public struct Notice: Equatable {
        public let ok: Bool
        public let text: String
    }

    public enum GatewayState: Equatable {
        case disconnected, connecting, connected, failed(String)
    }

    static let feedsKey = "mg-user-feeds"
    /// The largest file read for an import (the core's cap).
    public static let maxFileBytes = 16 * 1024 * 1024

    @Published public private(set) var summary: UserDataSummary?
    @Published public private(set) var notice: Notice?
    @Published public private(set) var busy = false
    @Published public private(set) var live: LiveSession?
    @Published public private(set) var gatewayState: GatewayState = .disconnected
    @Published public private(set) var gatewayAddress = ""
    @Published public private(set) var feeds: [UserFeedConfig] = []
    @Published public private(set) var feedStates: [String: UserFeedState] = [:]
    @Published public private(set) var feedNotice: Notice?

    public let http: UserHTTPClient
    private weak var app: AppModel?
    private let defaults: UserDefaults
    private var pollers: [String: Task<Void, Never>] = [:]
    private var pollingClients = 0

    init(app: AppModel, http: UserHTTPClient = UserHTTPClient(), defaults: UserDefaults = .standard) {
        self.app = app
        self.http = http
        self.defaults = defaults
        if let data = defaults.data(forKey: Self.feedsKey),
           let saved = try? JSONDecoder().decode([UserFeedConfig].self, from: data) {
            feeds = saved
        }
        updateAllowed()
    }

    private var client: LiveGatewayClient { LiveGatewayClient(http: http) }

    /// The addresses the app may reach for the user: the gateway and each feed.
    public var allowedOrigins: [String] { http.allowedOrigins.sorted() }

    private func updateAllowed() {
        var urls = feeds.compactMap { URL(string: $0.url) }
        if let live { urls.append(live.base) } else if let base = try? LiveGatewayClient.base(gatewayAddress),
                                                    gatewayState == .connecting {
            urls.append(base)
        }
        http.setAllowed(urls)
    }

    // MARK: imported files

    /// What the picker gave, read as files: the one `ontology.json` (by name, or an
    /// object with `objectTypes`) and the data files.
    static func read(_ urls: [URL]) throws -> (ontology: NamedFile?, files: [NamedFile]) {
        var ontology: NamedFile?
        var files: [NamedFile] = []
        for url in urls {
            let scoped = url.startAccessingSecurityScopedResource()
            defer { if scoped { url.stopAccessingSecurityScopedResource() } }
            let name = url.lastPathComponent
            let size = (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
            guard size <= maxFileBytes else {
                throw ImportProblem("\(name): larger than 16 MiB; split it into several files")
            }
            let data: Data
            do { data = try Data(contentsOf: url) } catch { throw ImportProblem("\(name): could not be read") }
            guard data.count <= maxFileBytes else { throw ImportProblem("\(name): larger than 16 MiB; split it into several files") }
            guard let text = String(data: data, encoding: .utf8) else { throw ImportProblem("\(name): not UTF-8 text") }
            let file = NamedFile(name: name, text: text)
            if isOntology(name: name, text: text) {
                if ontology != nil { throw ImportProblem("pick one ontology.json, not two") }
                ontology = file
            } else {
                files.append(file)
            }
        }
        return (ontology, files)
    }

    struct ImportProblem: Error { let text: String; init(_ t: String) { text = t } }

    static func isOntology(name: String, text: String) -> Bool {
        if name.lowercased() == "ontology.json" { return true }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard trimmed.hasPrefix("{"),
              let obj = try? JSONSerialization.jsonObject(with: Data(trimmed.utf8)) as? [String: Any] else { return false }
        return obj["objectTypes"] != nil
    }

    /// Import the picked files: checked and installed in the core (audited), then the
    /// sealed copy written. A refusal names the file and line and changes nothing.
    public func importFiles(_ urls: [URL]) async {
        guard let app, !busy else { return }
        busy = true
        notice = nil
        defer { busy = false }
        let picked: (ontology: NamedFile?, files: [NamedFile])
        do { picked = try Self.read(urls) } catch let p as ImportProblem {
            notice = Notice(ok: false, text: "Import refused: \(p.text)")
            return
        } catch { return }
        do {
            let outcome = try await app.core { try $0.importData(ontology: picked.ontology, files: picked.files) }
            summary = outcome.summary
            let added = picked.files.map(\.name).joined(separator: ", ")
            notice = Notice(ok: true, text: Self.describeImport(outcome.summary, added: added, audit: outcome.auditId))
            await pushFeedRows()
        } catch let ClientError.Refused(reason) {
            notice = Notice(ok: false, text: "Import refused: \(reason)")
        } catch {
            notice = Notice(ok: false, text: "Import refused: \(ErrorText.describe(error))")
        }
    }

    static func describeImport(_ s: UserDataSummary, added: String, audit: String) -> String {
        var text = "Imported \(added): now \(s.objects) object(s) and \(s.links) link(s)"
        let observations = s.layers.reduce(0) { $0 + Int($1.total) }
        if observations > 0 { text += ", \(observations) observation(s)" }
        text += ". Recorded as \(audit)."
        if s.unreadable > 0 {
            text += " \(s.unreadable) object(s) lack the purpose \"operations\": nobody can read them, as through a gateway."
        }
        return text
    }

    /// Take the imported data out (audited) and delete its sealed copy.
    public func removeImported() async {
        guard let app, !busy else { return }
        busy = true
        defer { busy = false }
        do {
            let audit = try await app.core { try $0.removeImportedData() }
            summary = try? await app.core { $0.client.userDataSummary() }
            notice = Notice(ok: true, text: "Imported data removed. Recorded as \(audit).")
            await pushFeedRows()
        } catch let ClientError.Refused(reason) {
            notice = Notice(ok: false, text: reason)
        } catch {
            notice = Notice(ok: false, text: ErrorText.describe(error))
        }
    }

    /// After a sign-in: install the sealed copy, if there is one.
    func restore() async {
        guard let app else { return }
        summary = try? await app.core { try $0.restoreUserData() }
        await pushFeedRows()
    }

    /// The imported object types.
    public var importedTypes: Set<String> { Set(summary?.typeIds ?? []) }

    // MARK: gateway

    /// Connect: check the address (the SDK's rule), sign in, ask who the gateway signed
    /// in. Midgard's screens then read the gateway. The password is not kept.
    public func connect(address: String, username: String, password: String, code: String) async {
        guard gatewayState != .connecting else { return }
        let base: URL
        do { base = try LiveGatewayClient.base(address) } catch let e as LiveGatewayError {
            gatewayState = .failed(e.message)
            return
        } catch { return }
        gatewayAddress = address
        gatewayState = .connecting
        updateAllowed()
        do {
            let session = try await client.login(base: base, username: username, password: password,
                                                 code: code.isEmpty ? nil : code)
            live = session
            gatewayState = .connected
        } catch let e as LiveGatewayError {
            gatewayState = .failed(e.message)
        } catch {
            gatewayState = .failed("The sign-in could not be sent.")
        }
        updateAllowed()
        app?.objectWillChange.send()
    }

    /// Sign out of the gateway (the token is revoked and dropped): the screens go back to
    /// the sample and imported data.
    public func disconnect() async {
        if let live { await client.logout(live) }
        live = nil
        gatewayState = .disconnected
        updateAllowed()
        app?.objectWillChange.send()
    }

    private func ended(_ e: Error) {
        if case LiveGatewayError.signedOut = e {
            live = nil
            gatewayState = .failed(LiveGatewayError.signedOut.message)
            updateAllowed()
            app?.objectWillChange.send()
        }
    }

    /// A collection from the gateway, as rows (nothing withheld: the gateway scoped
    /// them already); an empty list, with the reason as the message, otherwise.
    func liveRows(_ collection: String, rowType: String?) async -> [ConsoleRow] {
        guard let live, let rowType else { return [] }
        do {
            if let path = LiveRoutes.lists[collection] {
                let data = try await client.get(live, path)
                return try gatewayRows(rowType: rowType, json: String(decoding: data, as: UTF8.self))
            }
            guard collection == LiveRoutes.objects else { return [] }
            let types = try JSONSerialization.jsonObject(with: try await client.get(live, "/object-types")) as? [[String: Any]] ?? []
            var all: [Any] = []
            for t in types {
                guard let api = t["apiName"] as? String else { continue }
                let answer = try await client.post(live, "/query", ["objectType": api, "maxResults": 500])
                let results = (try JSONSerialization.jsonObject(with: answer) as? [String: Any])?["results"] as? [Any] ?? []
                all.append(contentsOf: results)
            }
            let json = String(decoding: try JSONSerialization.data(withJSONObject: all), as: UTF8.self)
            return try gatewayRows(rowType: rowType, json: json)
        } catch let e as LiveGatewayError {
            ended(e)
            app?.message = "\(live.host): \(e.message)"
        } catch let ClientError.Malformed(detail) {
            app?.message = "\(live.host): \(detail)"
        } catch {
            app?.message = "\(live.host): the answer could not be read."
        }
        return []
    }

    /// One of Midgard's actions on the gateway: its answer, or its refusal in the
    /// gateway's words. The gateway audits it (its `auditId`).
    func liveRun(_ verb: String, _ target: String, _ params: [(String, String)]) async -> MidgardOutcome {
        guard let live else { return .failed("Not connected.") }
        guard let call = LiveRoutes.call(verb, target: target, params: params, session: live) else {
            return .refused("Not served by \(live.host): this runs only on the sample data.")
        }
        do {
            let data = try await client.post(live, call.path, call.body)
            let audit = (try? JSONSerialization.jsonObject(with: data) as? [String: Any])?["auditId"] as? String
            return .applied(auditId: audit, result: String(decoding: data, as: UTF8.self))
        } catch let e as LiveGatewayError {
            ended(e)
            return .refused(e.message)
        } catch {
            return .failed("The request could not be sent.")
        }
    }

    // MARK: feeds

    private func saveFeeds() {
        if let data = try? JSONEncoder().encode(feeds) { defaults.set(data, forKey: Self.feedsKey) }
        updateAllowed()
    }

    /// Add (or, with an id, change) a feed: a name, an address by the address rule, an
    /// interval of at least 30 s. Returns the refusal, if any.
    @discardableResult
    public func saveFeed(id: String? = nil, name: String, url: String, interval: Int) -> String? {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return "Give the feed a name: it names its layer on the picture." }
        let checked: String
        do { checked = try checkFeedAddress(url: url.trimmingCharacters(in: .whitespacesAndNewlines)) } catch let ClientError.Refused(reason) {
            return reason
        } catch { return "Refusing a feed URL that does not parse: \(url)" }
        let feed = UserFeedConfig(id: id ?? UUID().uuidString, name: name, url: checked,
                                  interval: max(UserFeedConfig.minimumInterval, interval))
        if let i = feeds.firstIndex(where: { $0.id == feed.id }) { feeds[i] = feed } else { feeds.append(feed) }
        saveFeeds()
        if pollingClients > 0 { restartPoller(feed) }
        Task { await pushFeedRows() }
        return nil
    }

    public func removeFeed(_ id: String) {
        pollers.removeValue(forKey: id)?.cancel()
        feeds.removeAll { $0.id == id }
        feedStates[id] = nil
        saveFeeds()
        Task { await pushFeedRows() }
    }

    /// Heimdall's picture opened: poll every feed while it stays open.
    public func startPolling() {
        pollingClients += 1
        guard pollingClients == 1 else { return }
        for feed in feeds { restartPoller(feed) }
    }

    /// Heimdall's picture closed: stop, cancelling any fetch in flight.
    public func stopPolling() {
        pollingClients = max(0, pollingClients - 1)
        guard pollingClients == 0 else { return }
        for (_, task) in pollers { task.cancel() }
        pollers = [:]
    }

    public var polling: Bool { pollingClients > 0 }

    private func restartPoller(_ feed: UserFeedConfig) {
        pollers[feed.id]?.cancel()
        pollers[feed.id] = Task { [weak self] in
            while !Task.isCancelled {
                await self?.poll(feed)
                try? await Task.sleep(for: .seconds(max(UserFeedConfig.minimumInterval, feed.interval)))
            }
        }
    }

    /// Fetch a feed once and parse its observations (the core's parser).
    func poll(_ feed: UserFeedConfig) async {
        guard let url = URL(string: feed.url) else { return }
        let host = url.host ?? feed.url
        var state = feedStates[feed.id] ?? UserFeedState()
        let outcome = await http.send("GET", url)
        if Task.isCancelled { return }
        state.lastFetch = Date()
        switch outcome {
        case let .ok(data):
            do {
                state.layers = try parseObservations(source: feed.name, text: String(decoding: data, as: UTF8.self))
                state.error = nil
            } catch let ClientError.Refused(reason) {
                state.error = reason
            } catch {
                state.error = "\(host): the answer could not be read"
            }
        default:
            state.error = LiveGatewayClient.describe(outcome, host: host).message
        }
        feedStates[feed.id] = state
        await pushFeedRows()
    }

    /// Heimdall's feeds rows for the user's own: each imported observations layer and
    /// each feed URL, never authenticated.
    private func pushFeedRows() async {
        guard let app, app.isUnlocked else { return }
        var rows: [UserFeed] = []
        for layer in summary?.layers ?? [] {
            rows.append(UserFeed(id: "file:\(layer.source):\(layer.feed)", name: layer.feed, kind: "observations file",
                                 rate: "file", statusLabel: "UNVERIFIED FEED", failing: false))
        }
        for feed in feeds {
            let error = feedStates[feed.id]?.error
            rows.append(UserFeed(id: "url:\(feed.id)", name: feed.name, kind: "feed URL", rate: "every \(feed.interval) s",
                                 statusLabel: error.map { "UNVERIFIED FEED · \($0)" } ?? "UNVERIFIED FEED",
                                 failing: error != nil))
        }
        _ = try? await app.core { try $0.client.setUserFeeds(feeds: rows) }
    }

    // MARK: the picture's layers

    /// The observation layers on the picture: imported files' and feeds' last answers.
    public var observationLayers: [UserMapLayer] {
        var out: [UserMapLayer] = []
        for layer in summary?.layers ?? [] {
            out.append(Self.layer(id: "file:\(layer.source):\(layer.feed)", name: layer.feed, origin: .file, layer))
        }
        for feed in feeds {
            let layers = feedStates[feed.id]?.layers ?? []
            let points = layers.flatMap(\.points)
            out.append(UserMapLayer(id: "url:\(feed.id)", name: feed.name, origin: .feed,
                                    points: points.map(Self.point), total: layers.reduce(0) { $0 + Int($1.total) }))
        }
        return out
    }

    static func point(_ p: UserPoint) -> UserMapPoint {
        UserMapPoint(id: p.id, label: p.label, kind: p.kind, lat: p.lat, lon: p.lon)
    }

    static func layer(id: String, name: String, origin: UserMapLayer.Origin, _ l: UserLayerInfo) -> UserMapLayer {
        UserMapLayer(id: id, name: name, origin: origin, points: l.points.map(point), total: Int(l.total))
    }
}

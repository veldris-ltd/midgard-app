import Foundation
@preconcurrency import MidgardCore
import SwiftUI

/// The app's session state: enrol, sign in as a persona, switch, lock (also on idle),
/// chips, offline toggle, sync, reset. Every core call runs through `CoreGateway`, off
/// the main thread. Errors become screen states and messages, never crashes.
/// [FR-C-1] [FR-C-4] [FR-C-9] [AC-C-13]
@MainActor
public final class AppModel: ObservableObject {
    public enum Phase: Equatable {
        case loading, needsEnrolment, locked, unlocked(PersonaInfo), failed(String)
    }

    /// Messages about saved state the user should know of.
    public enum Notice {
        public static let staleDiscarded =
            "Saved data on this device was older than expected and was discarded. It will be loaded again."
        public static let unreadableDiscarded =
            "Saved data on this device could not be opened and was discarded. It will be loaded again."
        public static let pendingDropped =
            "Changes you had not yet sent could not be restored at your current clearance and were discarded."
    }

    @Published public private(set) var phase: Phase = .loading
    @Published public private(set) var personas: [PersonaInfo] = []
    @Published public private(set) var collections: [CollectionInfo] = []
    @Published public private(set) var chips: StatusChips?
    /// The persona last signed in during this run (kept in memory only).
    @Published public private(set) var lastPersona: PersonaInfo?
    @Published public var message: String?
    /// What the client runs on, once the environment is known.
    @Published public private(set) var dataSourceKind: DataSourceKind?
    /// The banner above everything: the demonstration notice while the source is the
    /// release's sample data or the demo dataset (the web consoles' own words in mock
    /// mode), "No data source connected" with none. [AC-C-13] [FR-C-5]
    public var banner: SourceBanner? {
        switch dataSourceKind {
        case DataSourceKind.demo?, DataSourceKind.sample?: return .demo
        case DataSourceKind.none?: return .noSource
        case nil: return nil
        }
    }
    public private(set) var custodyLabel: String?
    /// Which app this is, once the environment is known (Task B7).
    @Published public private(set) var edition: AppEdition?
    /// The edition's banner under the source banner, its About screen and Heimdall's 3D
    /// view: each from the linked edition provider (`EditionHook`), so nil in a release
    /// app, which links none. [AC-C-13]
    public var editionBanner: AnyView? { edition.flatMap { EditionHook.registered?.banner(for: $0) } }
    public var about: AnyView? { edition.flatMap { EditionHook.registered?.about(for: $0) } }
    public var threeDEntry: HeimdallThreeDProvider? { edition.flatMap { EditionHook.registered?.threeD(for: $0) } }

    public var chipsPresentation: StatusChipsPresentation? { chips.map(StatusChipsPresentation.init) }

    /// Whether a persona is signed in (the console shell is shown).
    public var isUnlocked: Bool {
        if case .unlocked = phase { return true }
        return false
    }

    /// Whether the session is offline, from the sync chip.
    public var isOffline: Bool { chips?.sync.phase == .offline }

    private var gateway: CoreGateway<SessionController>?
    private let idleTimeout: TimeInterval
    private lazy var idle = IdleLock(timeout: idleTimeout) { [weak self] in
        Task { await self?.idleExpired() }
    }

    public init(idleTimeout: TimeInterval = 300) {
        self.idleTimeout = idleTimeout
    }

    /// Make the environment off the main thread (it probes the keychain), then start.
    /// A failure is a screen state.
    public func start(making: @escaping () throws -> AppEnvironment) async {
        let made: Result<AppEnvironment, Error> = await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async { continuation.resume(returning: Result { try making() }) }
        }
        switch made {
        case let .success(env): await start(env)
        case let .failure(error): phase = .failed(ErrorText.describe(error))
        }
    }

    public func start(_ env: AppEnvironment) async {
        do {
            // The banner follows the client, never the request: a bundled dataset that
            // is missing or fails its signature check claims no source. [AC-C-13]
            dataSourceKind = nil
            edition = env.edition
            let config = ClientConfig(source: env.dataSource.coreChoice, replicaId: env.replicaId, device: env.device)
            let gateway = try await Self.open(env: env, config: config)
            self.gateway = gateway
            custodyLabel = env.keyRoot.custodyLabel
            let (kind, personas, collections, enrolled) = try await gateway.run {
                ($0.client.dataSourceKind(), $0.client.personas(), $0.client.collections(), try $0.isEnrolled())
            }
            dataSourceKind = kind
            self.personas = personas
            self.collections = collections
            phase = enrolled ? .locked : .needsEnrolment
        } catch {
            phase = .failed(ErrorText.describe(error))
        }
    }

    /// Opening the dataset checks its signature: off the main thread as well.
    private static func open(env: AppEnvironment, config: ClientConfig) async throws -> CoreGateway<SessionController> {
        try await withCheckedThrowingContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(with: Result {
                    let client = try env.makeClient(config, env.keyRoot)
                    let store = PersonaStateStore(directory: env.storeDirectory, secrets: env.secrets)
                    return CoreGateway(SessionController(client: client, store: store, keyRoot: env.keyRoot))
                })
            }
        }
    }

    public func enrol() async {
        guard await call({ try $0.enrol() }) != nil else { return }
        phase = .locked
    }

    public func signIn(_ persona: PersonaInfo) async {
        message = nil
        guard let (info, outcome) = await call({ try $0.signIn(persona.id) }) else { return }
        enter(info, outcome)
        idle.touch()
    }

    public func switchPersona(_ persona: PersonaInfo) async {
        message = nil
        guard let (info, outcome) = await call({ try $0.switchPersona(persona.id) }) else { return }
        enter(info, outcome)
    }

    private func enter(_ info: SessionInfo, _ outcome: RestoreOutcome) {
        chips = info.chips
        lastPersona = info.persona
        phase = .unlocked(info.persona)
        switch outcome {
        case .fresh, .restored: break
        case .restoredPendingDropped: message = Notice.pendingDropped
        case .discardedStale: message = Notice.staleDiscarded
        case .discardedUnreadable: message = Notice.unreadableDiscarded
        }
    }

    public func lock() async {
        idle.stop()
        guard gateway != nil else { return }
        _ = await call { try $0.lock() }
        chips = nil
        phase = .locked
    }

    /// The idle timer ran out: save and lock.
    func idleExpired() async {
        guard case .unlocked = phase else { return }
        await lock()
    }

    public func setOffline(_ offline: Bool) async {
        if let chips = await call({ try $0.setOffline(offline) }) { self.chips = chips }
    }

    public func syncNow() async {
        if let report = await call({ try $0.syncNow() }) { chips = report.chips }
    }

    /// A collection's rows (withheld ones by level only); empty, with a message, on error.
    public func rows(_ collection: String) async -> [ConsoleRow] {
        await call { try $0.rows(collection) } ?? []
    }

    /// Queue an action; returns its id.
    public func requestAction(_ action: ActionInput) async -> String? {
        guard let (id, chips) = await call({ try $0.requestAction(action) }) else { return nil }
        self.chips = chips
        return id
    }

    /// An action's state as this device knows it (queued, applied with its audit id,
    /// refused with the reason); nil, with a message, on error.
    public func actionStatus(_ id: String) async -> ActionStatus? {
        await call { try $0.actionStatus(id) }
    }

    public func resetDemo() async {
        idle.stop()
        guard await call({ try $0.resetDemo() }) != nil else { return }
        chips = nil
        lastPersona = nil
        phase = .needsEnrolment
    }

    /// Any user interaction postpones the idle lock.
    public func touch() {
        if case .unlocked = phase { idle.touch() }
    }

    /// Run `body` on the gateway; map an error to a message (and `Locked` to the
    /// locked screen).
    private func call<T>(_ body: @escaping (SessionController) throws -> T) async -> T? {
        guard let gateway else { return nil }
        do {
            return try await gateway.run(body)
        } catch ClientError.Locked {
            if case .unlocked = phase { phase = .locked }
            chips = nil
        } catch {
            message = ErrorText.describe(error)
        }
        return nil
    }
}

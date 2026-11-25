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
    /// The banner above everything: none over the sample workspace (the status row's
    /// data chip names it, owner decision 25 Nov 2026), the LIVE banner while a gateway is
    /// connected, "No data source connected" with none. [AC-C-13] [FR-C-5]
    public var banner: SourceBanner? {
        switch dataSourceKind {
        case DataSourceKind.demo?, DataSourceKind.sample?:
            if let live = userData.live { return .live(live.host) }
            return nil
        case DataSourceKind.none?: return .noSource
        case nil: return nil
        }
    }

    /// The status row's data chip: "SAMPLE WORKSPACE" (with "+ IMPORTED" once the user has
    /// imported files), "LIVE · <host>" while a gateway is connected, "NO DATA SOURCE"
    /// with none. Sample data is never called live. [AC-C-13]
    public var sourceChip: ChipText? {
        DataSourceChip.make(kind: dataSourceKind, liveHost: userData.live?.host,
                            imported: !(userData.summary?.files.isEmpty ?? true))
    }

    /// Your Data is open (the iPhone's sheet: the session menu's "Your data" and the
    /// invitation's "Connect your data" both set it).
    @Published public var showingYourData = false

    /// Whether the user dismissed the sample-workspace invitation. Remembered in the app's
    /// preferences; a UI-testing launch reads only its launch arguments
    /// (`-MidgardSampleInviteDismissed YES`) and writes nothing.
    @Published public private(set) var sampleInviteDismissed: Bool = AppModel.readInviteDismissed()

    /// The invitation shows over the sample workspace, signed in, until it is dismissed
    /// or the user's own data is there (a gateway, imported files or a feed).
    public var showsSampleInvite: Bool {
        guard isUnlocked, !sampleInviteDismissed else { return false }
        guard dataSourceKind == .sample || dataSourceKind == .demo else { return false }
        return userData.live == nil && (userData.summary?.files.isEmpty ?? true) && userData.feeds.isEmpty
    }

    public func dismissSampleInvite() {
        sampleInviteDismissed = true
        if !Self.uiTesting { UserDefaults.standard.set(true, forKey: SampleWorkspaceInvite.dismissedKey) }
    }

    private static var uiTesting: Bool {
        #if DEBUG
        return UITestingEnvironment.requested()
        #else
        return false
        #endif
    }

    private static func readInviteDismissed() -> Bool {
        if uiTesting {
            let args = UserDefaults.standard.volatileDomain(forName: UserDefaults.argumentDomain)
            return (args[SampleWorkspaceInvite.dismissedKey] as? String).map { ["YES", "1", "true"].contains($0) } ?? false
        }
        return UserDefaults.standard.bool(forKey: SampleWorkspaceInvite.dismissedKey)
    }
    public private(set) var custodyLabel: String?
    /// Which app this is, once the environment is known (Task B7).
    @Published public private(set) var edition: AppEdition?
    /// The edition's About screen and Heimdall's
    /// further picture views (MAP and 3D): each from the linked edition provider
    /// (`EditionHook`), so nil in a release app, which links none. [AC-C-13]
    public var about: AnyView? { edition.flatMap { EditionHook.registered?.about(for: $0) } }
    public var pictureViews: HeimdallPictureViewsProvider? {
        edition.flatMap { EditionHook.registered?.pictureViews(for: $0) }
    }

    public var chipsPresentation: StatusChipsPresentation? { chips.map(StatusChipsPresentation.init) }

    /// The name the persona chip shows for `persona`: connected to a gateway, the
    /// subject the gateway signed in (its `/whoami`), never a sample persona's name;
    /// otherwise the sample persona's. [AC-C-13]
    public func signedInName(_ persona: PersonaInfo) -> String {
        userData.live?.subject ?? persona.display
    }

    /// Whether a persona is signed in (the console shell is shown).
    public var isUnlocked: Bool {
        if case .unlocked = phase { return true }
        return false
    }

    /// Whether the session is offline, from the sync chip.
    public var isOffline: Bool { chips?.sync.phase == .offline }

    private var gateway: CoreGateway<SessionController>?
    /// Your Data: imported files, the gateway connection and the feeds.
    public private(set) lazy var userData = UserDataModel(app: self)
    private let idleTimeout: TimeInterval
    private lazy var idle = IdleLock(timeout: idleTimeout) { [weak self] in
        Task { await self?.idleExpired() }
    }

    public init(idleTimeout: TimeInterval = 300) {
        self.idleTimeout = idleTimeout
        // The house type (Oxanium, Space Grotesk, JetBrains Mono), bundled in MidgardKit:
        // without it every `MGFonts` style falls back to the system font.
        MGFonts.registerAll()
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
        Task { await userData.restore() }
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
    /// Connected to a gateway, Midgard's and Admin's collections are the gateway's: the
    /// ones it serves read live, the others are empty (the screen says it is not
    /// served), never the sample data under a LIVE chip.
    public func rows(_ collection: String) async -> [ConsoleRow] {
        if userData.live != nil, collection.hasPrefix("midgard.") || collection.hasPrefix("admin.") {
            guard LiveRoutes.serves(collection) else { return [] }
            let rowType = collections.first { $0.name == collection }?.rowType
            return await userData.liveRows(collection, rowType: rowType)
        }
        return await call { try $0.rows(collection) } ?? []
    }

    /// Run `body` on the core's queue, with its error (Your Data shows the core's own
    /// refusal, naming the file and line).
    func core<T>(_ body: @escaping (SessionController) throws -> T) async throws -> T {
        guard let gateway else { throw ClientError.Locked }
        return try await gateway.run(body)
    }

    /// Queue an action; returns its id. Connected to a gateway, Admin's actions are not
    /// made (the gateway does not serve Admin; Midgard's go to the gateway).
    public func requestAction(_ action: ActionInput) async -> String? {
        if let live = userData.live, action.verb.hasPrefix("admin.") || action.verb.hasPrefix("midgard.") {
            message = "Not served by \(live.host): disconnect to act on the sample data."
            return nil
        }
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
        await userData.disconnect()
        guard await call({ try $0.resetDemo() }) != nil else { return }
        // Starting again from the beginning: the invitation shows again.
        sampleInviteDismissed = false
        if !Self.uiTesting { UserDefaults.standard.removeObject(forKey: SampleWorkspaceInvite.dismissedKey) }
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

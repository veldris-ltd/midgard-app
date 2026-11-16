import Foundation
import MidgardCore

/// What happened to a persona's saved state at sign-in.
public enum RestoreOutcome: Equatable, Sendable {
    /// No file for this persona: the cache starts empty and reads fetch.
    case fresh
    case restored(kept: UInt32)
    /// Restored, but the queued changes in it could not be read at this clearance.
    case restoredPendingDropped(kept: UInt32)
    /// The file was older than the newest kept for this persona (a rollback): deleted.
    case discardedStale
    /// The file did not open or parse: deleted.
    case discardedUnreadable
}

/// The session's steps over the core, its sealed files and the platform key, in the
/// order they must happen. Synchronous, and only ever called on `CoreGateway`'s queue,
/// so a file write and the core call it follows cannot interleave with another call.
/// It never runs inside the key root. [FR-C-1] [FR-C-4] [FR-C-9] [AC-C-13]
public final class SessionController {
    public enum Failure: Error, Equatable {
        case notEnrolled
    }

    public let client: MidgardClientProtocol
    public let store: PersonaStateStore
    public let keyRoot: PlatformKeyRoot

    public init(client: MidgardClientProtocol, store: PersonaStateStore, keyRoot: PlatformKeyRoot) {
        self.client = client
        self.store = store
        self.keyRoot = keyRoot
    }

    public func isEnrolled() throws -> Bool { try store.loadKeys() != nil }

    /// First use: make and store the sealed keys (no prompt).
    public func enrol() throws {
        try store.saveKeys(try client.enrol())
    }

    /// Unlock (one prompt) as `persona`, then load that persona's state file.
    public func signIn(_ persona: String) throws -> (SessionInfo, RestoreOutcome) {
        guard let keys = try store.loadKeys() else { throw Failure.notEnrolled }
        let info = try client.unlock(keys: keys, personaId: persona)
        let outcome = restore(persona)
        return ((try? client.statusChips()).map { SessionInfo(persona: info.persona, chips: $0) } ?? info, outcome)
    }

    /// Load `persona`'s file with the generation kept for that persona. On `Stale` or
    /// any other import error the file is deleted (the next read refetches); the
    /// generation stays. With no file but a kept generation, the core is still told the
    /// generation, so its next export is numbered above every file seen.
    func restore(_ persona: String) -> RestoreOutcome {
        let minimum = (try? store.minGeneration(for: persona)) ?? 0
        let file: Data?
        do { file = try store.load(persona: persona) } catch { file = nil }
        guard let file else {
            if minimum > 0 { _ = try? client.importState(file: Data(), minGeneration: minimum) }
            return .fresh
        }
        do {
            let report = try client.importState(file: file, minGeneration: minimum)
            return report.pendingDropped ? .restoredPendingDropped(kept: report.kept) : .restored(kept: report.kept)
        } catch {
            try? store.discard(persona: persona)
            if case ClientError.Stale = error { return .discardedStale }
            return .discardedUnreadable
        }
    }

    /// Export the signed-in persona's state: the file first, then its generation.
    public func persist() throws {
        let persona = try client.sessionPersona()
        try store.save(try client.exportState(), persona: persona.id)
    }

    /// Save, then lock. A failed save does not keep the session open.
    public func lock() throws {
        let saved = Result { try persist() }
        try client.lock()
        try saved.get()
    }

    /// Save the current persona, switch (refused while changes are unsent), then load
    /// the new persona's file.
    public func switchPersona(_ persona: String) throws -> (SessionInfo, RestoreOutcome) {
        try persist()
        let info = try client.switchPersona(personaId: persona)
        let outcome = restore(persona)
        return ((try? client.statusChips()).map { SessionInfo(persona: info.persona, chips: $0) } ?? info, outcome)
    }

    public func rows(_ collection: String) throws -> [ConsoleRow] {
        let rows = try client.collectionRows(collection: collection)
        try persist()
        return rows
    }

    public func requestAction(_ action: ActionInput) throws -> (String, StatusChips) {
        let id = try client.requestAction(action: action)
        try persist()
        return (id, try client.statusChips())
    }

    public func setOffline(_ offline: Bool) throws -> StatusChips {
        try client.setOffline(offline: offline)
    }

    public func syncNow() throws -> SyncReport {
        let report = try client.syncNow()
        try persist()
        return report
    }

    /// Reset demo: lock and reload the dataset, delete every file and keychain item
    /// this app keeps, and the platform key.
    public func resetDemo() throws {
        try client.resetDemo()
        try store.reset()
        try keyRoot.destroyKey()
    }
}

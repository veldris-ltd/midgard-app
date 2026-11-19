import MidgardCore

/// The demonstration notice, word for word the web consoles' (`DEMONSTRATION_NOTICE`
/// in ui/shared/src/DemonstrationBanner.tsx): shown above everything while the app
/// runs on the bundled demo dataset (owner decision 10). [AC-C-13]
public enum DemoBanner {
    public static let notice =
        "Demonstration data. This console runs on built-in sample data and is not connected to an enclosure."
}

/// With no data source (the shipped apps, owner decision 19 Nov 2026), this replaces
/// the demonstration notice. [FR-C-5] [AC-C-13]
public enum NoSourceBanner {
    public static let notice = "No data source connected"
}

/// Which banner the session shows above everything, and its accessibility identifier.
public enum SourceBanner: Equatable, Sendable {
    case demo, noSource

    public var text: String {
        switch self {
        case .demo: return DemoBanner.notice
        case .noSource: return NoSourceBanner.notice
        }
    }

    public var identifier: String {
        switch self {
        case .demo: return "mg.banner.demo"
        case .noSource: return "mg.banner.nosource"
        }
    }
}

/// Third-party notices shown in Settings → Acknowledgements and listed in the release
/// notes. The UniFFI line is checked by the release checklist (A15). [DD-6] [DD-7]
public enum Acknowledgements {
    public struct Entry: Identifiable, Equatable, Sendable {
        public let id: String
        public let text: String
    }

    public static let entries: [Entry] = [
        Entry(id: "uniffi",
              text: "UniFFI 0.28.3, Mozilla Public License 2.0, source: https://github.com/mozilla/uniffi-rs/tree/v0.28.3"),
        Entry(id: "oxanium", text: "Oxanium, SIL Open Font License 1.1 (Oxanium-OFL.txt)"),
        Entry(id: "space-grotesk", text: "Space Grotesk, SIL Open Font License 1.1 (SpaceGrotesk-OFL.txt)"),
        Entry(id: "jetbrains-mono", text: "JetBrains Mono, SIL Open Font License 1.1 (JetBrainsMono-OFL.txt)"),
    ]
}

/// Screen text for the core's errors. Platform details (status codes, the core's
/// internal detail strings) stay out of it. [AC-C-13] [FR-C-9]
public enum ErrorText {
    public static func describe(_ error: Error) -> String {
        switch error {
        case KeyRootFactory.Failure.unsignedBuild:
            return "This build is not signed for the keychain, so it cannot keep keys. Use a signed build."
        case KeyRootFactory.Failure.keychainUnavailable, SessionController.Failure.keychainUnavailable:
            return "The keychain is not available right now. Unlock the device and try again."
        case KeyRootFactory.Failure.mismatch:
            return "This device's saved keys do not match its key store. Reset the demo to start again."
        case PersonaStateStore.Failure.namingKeyDamaged:
            return "This device's saved data cannot be found again. Reset the demo to start again."
        case SessionController.Failure.stateFileUnavailable:
            return "Saved data on this device cannot be read right now. Unlock the device and try again."
        case SessionController.Failure.notEnrolled:
            return "This device is not set up yet."
        case AppEnvironment.Failure.misconfigured:
            return "This app is misconfigured: MidgardDataSource is missing or not recognised. Install a correctly built app."
        default:
            break
        }
        guard let error = error as? ClientError else {
            return "Something went wrong on this device. Try again."
        }
        switch error {
        case .AboveClearance: return "Withheld: above your clearance."
        case .Offline: return "Offline. Changes are queued and will sync when you reconnect."
        case .NothingCached: return "Nothing is saved on this device for this view yet. Go online to load it."
        case let .PendingOps(count):
            return "Sync \(count) queued \(count == 1 ? "change" : "changes") before switching persona."
        case .BadTarget: return "That item cannot be acted on."
        case .Stale: return "Saved data on this device was older than expected and was discarded."
        case .Reserved: return "That name is reserved."
        case .DeviceKey: return "This device's audit key could not be opened. Reset the demo to start again."
        case .SignatureFailure: return "The demo data failed its signature check and was not loaded."
        case .Integrity: return "Saved data on this device could not be opened."
        case .Malformed: return "The data could not be read."
        case .Locked: return "Locked."
        case .AlreadyUnlocked: return "Already signed in."
        case .UnlockInProgress: return "Unlock is already in progress."
        case .UnknownPersona: return "That persona is not part of this demo."
        case .UnknownCollection: return "That view is not part of this demo."
        case .Cancelled: return "Unlock cancelled."
        case .KeyRootUnavailable: return "This device's key is not available. Check Face ID, Touch ID or the passcode."
        case .SealedRootMalformed: return "The saved keys are damaged. Reset the demo to start again."
        case .UnsealFailed: return "This device's key could not open the saved keys. Reset the demo to start again."
        case let .Refused(reason): return "Not available: \(reason)."
        case .NotFound: return "The demo data is missing from the app."
        case .Internal: return "Something went wrong on this device. Try again."
        }
    }
}

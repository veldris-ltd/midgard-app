import Foundation
import MidgardCore

/// What the app runs on, from its Info.plist key `MidgardDataSource` (owner decision
/// 19 Nov 2026): `none` (the shipped apps: no data, one local user) or `bundled` (the
/// signed demo dataset in the app's resources). [FR-C-5] [AC-C-13]
public enum DataSourceChoice: Equatable {
    case none
    case bundled(URL)
}

/// Everything the app model needs from the device, injectable for tests. [FR-C-1]
public struct AppEnvironment {
    public typealias ClientFactory = (ClientConfig, KeyRoot) throws -> MidgardClientProtocol

    /// The app was built without a recognised `MidgardDataSource`: a failed state,
    /// never a silent choice of either mode.
    public enum Failure: Error, Equatable {
        case misconfigured
    }

    /// The Info.plist key naming the data source.
    public static let dataSourceKey = "MidgardDataSource"

    public var dataSource: DataSourceChoice
    public var storeDirectory: URL
    public var secrets: SecretStore
    public var keyRoot: PlatformKeyRoot
    public var device: DeviceInfo
    public var replicaId: String
    public var makeClient: ClientFactory = { config, root in try MidgardClient(config: config, keyRoot: root) }

    public init(dataSource: DataSourceChoice, storeDirectory: URL, secrets: SecretStore, keyRoot: PlatformKeyRoot,
                device: DeviceInfo, replicaId: String) {
        self.dataSource = dataSource
        self.storeDirectory = storeDirectory
        self.secrets = secrets
        self.keyRoot = keyRoot
        self.device = device
        self.replicaId = replicaId
    }

    /// The app's own: the data source its Info.plist names, Application Support, the
    /// keychain, and the key root chosen by `KeyRootFactory` (at enrolment by a keychain
    /// probe; once enrolled, by the recorded choice). It touches the keychain and the
    /// disk: call it off the main thread, through `AppModel.start(making:)`. `reason`
    /// is the text of the unlock prompt. Throws `Failure.misconfigured` (before touching
    /// the keychain) for a missing or unknown `MidgardDataSource`, and
    /// `KeyRootFactory.Failure.unsignedBuild` for a build that cannot keep keys.
    public static func live(reason: String, device: DeviceInfo, bundle: Bundle = .main) throws -> AppEnvironment {
        let dataSource = try dataSourceChoice(in: bundle)
        let secrets = KeychainSecretStore()
        let directory = try StorePaths.storeDirectory()
        let sealed = try PersonaStateStore(directory: directory, secrets: secrets).loadKeys()
        return AppEnvironment(dataSource: dataSource, storeDirectory: directory,
                              secrets: secrets,
                              keyRoot: try KeyRootFactory.make(reason: reason, secrets: secrets, sealedRoot: sealed?.root),
                              device: device, replicaId: replicaId())
    }

    /// The data source `bundle`'s Info.plist names: `none`, or `bundled` (its
    /// `<release>.mgdemo`, which must be there: `ClientError.NotFound` otherwise). Any
    /// other value, or none, is `Failure.misconfigured`.
    public static func dataSourceChoice(in bundle: Bundle) throws -> DataSourceChoice {
        // The raw Info.plist value: `object(forInfoDictionaryKey:)` may return a
        // localised one.
        switch bundle.infoDictionary?[dataSourceKey] as? String {
        case "none"?:
            return .none
        case "bundled"?:
            guard let url = StorePaths.bundledDataset(in: bundle) else {
                throw ClientError.NotFound(what: "bundled dataset")
            }
            return .bundled(url)
        default:
            throw Failure.misconfigured
        }
    }

    /// A random id for this install's CRDT replica (not secret, not a user name).
    static func replicaId(defaults: UserDefaults = .standard) -> String {
        let key = "midgard.replica-id"
        if let id = defaults.string(forKey: key) { return id }
        let id = UUID().uuidString
        defaults.set(id, forKey: key)
        return id
    }
}

/// Handed once to the queue that opens the client, and only read there.
extension AppEnvironment: @unchecked Sendable {}

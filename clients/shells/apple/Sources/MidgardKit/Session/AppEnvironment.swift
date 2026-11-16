import Foundation
import MidgardCore

/// Everything the app model needs from the device, injectable for tests. [FR-C-1]
public struct AppEnvironment {
    public typealias ClientFactory = (ClientConfig, KeyRoot) throws -> MidgardClientProtocol

    public var dataset: URL?
    public var storeDirectory: URL
    public var secrets: SecretStore
    public var keyRoot: PlatformKeyRoot
    public var device: DeviceInfo
    public var replicaId: String
    public var makeClient: ClientFactory = { config, root in try MidgardClient(config: config, keyRoot: root) }

    public init(dataset: URL?, storeDirectory: URL, secrets: SecretStore, keyRoot: PlatformKeyRoot,
                device: DeviceInfo, replicaId: String) {
        self.dataset = dataset
        self.storeDirectory = storeDirectory
        self.secrets = secrets
        self.keyRoot = keyRoot
        self.device = device
        self.replicaId = replicaId
    }

    /// The app's own: the bundled dataset, Application Support, the keychain, and the
    /// key root chosen by `KeyRootFactory` (at enrolment by a keychain probe; once
    /// enrolled, by the recorded choice). It touches the keychain and the disk: call it
    /// off the main thread, through `AppModel.start(making:)`. `reason` is the text of
    /// the unlock prompt. Throws `KeyRootFactory.Failure.unsignedBuild` for a build
    /// that cannot keep keys.
    public static func live(reason: String, device: DeviceInfo, bundle: Bundle = .main) throws -> AppEnvironment {
        let secrets = KeychainSecretStore()
        let directory = try StorePaths.storeDirectory()
        let sealed = try PersonaStateStore(directory: directory, secrets: secrets).loadKeys()
        return AppEnvironment(dataset: StorePaths.bundledDataset(in: bundle), storeDirectory: directory,
                              secrets: secrets,
                              keyRoot: try KeyRootFactory.make(reason: reason, secrets: secrets, sealedRoot: sealed?.root),
                              device: device, replicaId: replicaId())
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

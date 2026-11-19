import CryptoKit
import Foundation
import MidgardCore

/// A software key root for tests: AES-GCM under an in-memory key, no prompt, no
/// keychain. Never in an app build. It counts its unseals (each one is a prompt on a
/// device) and can be told to answer as a cancelled prompt. [FR-C-9] [DD-6]
final class TestKeyRoot: KeyRoot {
    private let key = SymmetricKey(size: .bits256)
    private let lock = NSLock()
    private var _prompts = 0
    private var _cancel = false

    var prompts: Int { lock.lock(); defer { lock.unlock() }; return _prompts }
    var cancelNext: Bool {
        get { lock.lock(); defer { lock.unlock() }; return _cancel }
        set { lock.lock(); _cancel = newValue; lock.unlock() }
    }

    func seal(dataKey: Data) throws -> Data {
        do {
            return try AES.GCM.seal(dataKey, using: key).combined ?? Data()
        } catch {
            throw KeyRootError.PlatformUnavailable(detail: "test seal")
        }
    }

    func unseal(sealed: Data) throws -> Data {
        lock.lock()
        _prompts += 1
        let cancel = _cancel
        lock.unlock()
        if cancel { throw KeyRootError.UserCancelled }
        guard sealed.count == 60 else { throw KeyRootError.Malformed(detail: "length \(sealed.count)") }
        do {
            return try AES.GCM.open(AES.GCM.SealedBox(combined: sealed), using: key)
        } catch {
            throw KeyRootError.Unseal
        }
    }

    func hardwareBacked() -> Bool { false }
}

/// The repository root, found from this file's path.
func repositoryRoot(file: String = #filePath) -> URL {
    var url = URL(fileURLWithPath: file)
    for _ in 0..<6 { url.deleteLastPathComponent() }  // the file, MidgardKitTests, Tests, apple, shells, clients
    return url
}

/// The dataset committed for this release.
func releaseDataset() -> URL {
    repositoryRoot().appendingPathComponent("clients/demo/datasets/\(coreVersion()).mgdemo")
}

/// A fresh temporary directory, deleted by the caller.
func temporaryDirectory() throws -> URL {
    let url = FileManager.default.temporaryDirectory
        .appendingPathComponent("midgardkit-tests-\(UUID().uuidString)", isDirectory: true)
    try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
    return url
}

func makeConfig(dataset: URL? = releaseDataset(), replica: String = "contract") -> ClientConfig {
    ClientConfig(datasetPath: dataset?.path, replicaId: replica,
                 device: DeviceInfo(ramMb: 8192, hasAccelerator: true))
}

import Foundation
import LocalAuthentication
import MidgardCore

/// A key root the shell owns: the core's `KeyRoot` callback plus what the shell needs
/// around it (a label for the custody chip's detail, and deleting the platform key on
/// "Reset demo"). Implementations never call the `MidgardClient`. [FR-C-9] [DD-6]
public protocol PlatformKeyRoot: KeyRoot {
    /// What holds the platform key, for Settings and the custody chip's detail.
    var custodyLabel: String { get }
    /// Delete the platform key (Reset demo). The sealed root is useless afterwards.
    func destroyKey() throws
}

/// The constant key id of the device root (one per app install).
public let deviceRootKeyId = "device-root"

/// Asks for user presence (Face ID / Touch ID / passcode) before the software key
/// root unseals. Errors are LocalAuthentication's; the caller maps them.
public protocol UserPresence {
    func confirm(reason: String) throws
}

/// The device owner's authentication (biometry with passcode fallback). Called from
/// the core's `unseal` callback, which runs off the main thread.
public struct DeviceOwnerPresence: UserPresence {
    public init() {}

    public func confirm(reason: String) throws {
        let context = LAContext()
        var policyError: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthentication, error: &policyError) else {
            throw policyError ?? NSError(domain: LAError.errorDomain, code: LAError.Code.passcodeNotSet.rawValue)
        }
        let done = DispatchSemaphore(value: 0)
        var failure: Error?
        context.evaluatePolicy(.deviceOwnerAuthentication, localizedReason: reason) { ok, error in
            if !ok { failure = error ?? NSError(domain: LAError.errorDomain, code: LAError.Code.authenticationFailed.rawValue) }
            done.signal()
        }
        done.wait()
        if let failure { throw failure }
    }
}

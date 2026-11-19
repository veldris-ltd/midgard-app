import Foundation
import LocalAuthentication
import MidgardCore
import Security

/// Which callback a platform error came from.
public enum KeyRootOperation: Sendable {
    case seal, unseal
}

/// Maps Security and LocalAuthentication errors onto the core's `KeyRootError`, so a
/// screen can tell a cancelled prompt from a missing key from a sealed root that does
/// not open. Details name only the error domain and code: never key material.
/// [FR-C-9] [DD-6]
public enum KeyRootErrors {
    /// LocalAuthentication codes that mean the prompt was dismissed, not refused.
    private static let cancelCodes: Set<Int> = [LAError.Code.userCancel.rawValue,     // -2
                                                LAError.Code.systemCancel.rawValue,   // -4
                                                LAError.Code.appCancel.rawValue]      // -9
    /// Security statuses that mean the platform key is not usable here.
    private static let unavailableStatuses: Set<OSStatus> = [errSecMissingEntitlement, errSecItemNotFound,
                                                             errSecInteractionNotAllowed, errSecNotAvailable,
                                                             errSecNoSuchKeychain, errSecAuthFailed]

    public static func map(domain: String, code: Int, during operation: KeyRootOperation) -> KeyRootError {
        let detail = "\(domain) \(code)"
        if domain == NSOSStatusErrorDomain, code == Int(errSecUserCanceled) { return .UserCancelled }  // -128
        if domain == LAError.errorDomain {
            return cancelCodes.contains(code) ? .UserCancelled : .PlatformUnavailable(detail: detail)
        }
        if domain == NSOSStatusErrorDomain, unavailableStatuses.contains(OSStatus(truncatingIfNeeded: code)) {
            return .PlatformUnavailable(detail: detail)
        }
        switch operation {
        case .seal: return .PlatformUnavailable(detail: detail)
        case .unseal: return .Unseal
        }
    }

    public static func map(status: OSStatus, during operation: KeyRootOperation) -> KeyRootError {
        map(domain: NSOSStatusErrorDomain, code: Int(status), during: operation)
    }

    public static func map(_ error: Error, during operation: KeyRootOperation) -> KeyRootError {
        if let error = error as? KeyRootError { return error }
        if let error = error as? SecretStoreError, case let .keychain(status) = error {
            return map(status: status, during: operation)
        }
        let ns = error as NSError
        return map(domain: ns.domain, code: ns.code, during: operation)
    }

    static func map(_ error: Unmanaged<CFError>?, during operation: KeyRootOperation) -> KeyRootError {
        guard let error = error?.takeRetainedValue() else {
            return operation == .seal ? .PlatformUnavailable(detail: "unknown") : .Unseal
        }
        return map(error as Error, during: operation)
    }
}

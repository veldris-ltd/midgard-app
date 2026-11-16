import Foundation

/// Runs every call into the core, and every file and keychain step that goes with it,
/// one at a time on a private serial queue: never on the main thread, and never on the
/// Swift concurrency pool (`unlock` blocks while the platform prompts, which would
/// starve the pool). [FR-C-1] [FR-C-9]
public final class CoreGateway<Controller> {
    private let queue = DispatchQueue(label: "io.veldris.midgard.core", qos: .userInitiated)
    private let controller: Controller

    public init(_ controller: Controller) {
        self.controller = controller
    }

    public func run<T>(_ body: @escaping (Controller) throws -> T) async throws -> T {
        try await withCheckedThrowingContinuation { continuation in
            queue.async { [controller] in
                continuation.resume(with: Result { try body(controller) })
            }
        }
    }
}

import Foundation

/// Locks after `timeout` seconds without user activity. Any interaction calls
/// `touch()`; the app also locks when it leaves the foreground. [FR-C-9]
@MainActor
public final class IdleLock {
    private let timeout: TimeInterval
    private let onExpire: @MainActor () -> Void
    private var pending: Task<Void, Never>?

    public init(timeout: TimeInterval = 300, onExpire: @escaping @MainActor () -> Void) {
        self.timeout = timeout
        self.onExpire = onExpire
    }

    public func touch() {
        pending?.cancel()
        let nanoseconds = UInt64(max(timeout, 0) * 1_000_000_000)
        pending = Task { @MainActor [weak self] in
            try? await Task.sleep(nanoseconds: nanoseconds)
            guard !Task.isCancelled, let self else { return }
            self.pending = nil
            self.onExpire()
        }
    }

    public func stop() {
        pending?.cancel()
        pending = nil
    }
}

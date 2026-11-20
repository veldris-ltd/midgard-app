import Foundation

/// Locks after `timeout` seconds without user activity. Any interaction calls
/// `touch()`; the app also locks when it leaves the foreground. The timeout is measured
/// on the continuous clock, which keeps counting while the device sleeps (the uptime
/// clock behind `Task.sleep(nanoseconds:)` stops): a Mac closed for an hour with five
/// minutes left locks as soon as it wakes, not five minutes later. [FR-C-9]
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
        let timeout = Duration.seconds(max(timeout, 0))
        pending = Task { @MainActor [weak self] in
            try? await Task.sleep(for: timeout, clock: .continuous)
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

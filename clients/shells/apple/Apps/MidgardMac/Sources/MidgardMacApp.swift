import AppKit
@preconcurrency import MidgardCore
import MidgardKit
import SwiftUI

/// The Midgard macOS app: one session (`AppModel`) shared by every window, each window
/// showing its own console route. [FR-C-1] [FR-C-9] [DD-6]
@main
struct MidgardMacApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        WindowGroup(for: ConsoleRoute.self) { $route in
            RootView(model: delegate.model, route: $route)
                .frame(minWidth: 900, minHeight: 600)
        } defaultValue: {
            ConsoleRoute()
        }
        .commands { MidgardCommands(model: delegate.model) }

        Settings {
            SettingsView(model: delegate.model)
        }
    }
}

/// Owns the session and what is app-wide about it: starting it off the main thread,
/// postponing the idle lock on input, and locking after the app has been in the
/// background for the grace period. [FR-C-9]
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// How long the app may stay inactive before it locks.
    static let backgroundGrace: TimeInterval = 30

    let model = AppModel()
    private var monitor: Any?
    private var observers: [NSObjectProtocol] = []
    private var pendingLock: Task<Void, Never>?

    func applicationDidFinishLaunching(_ notification: Notification) {
        monitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .scrollWheel]) { [weak self] event in
            MainActor.assumeIsolated { self?.model.touch() }
            return event
        }
        let center = NotificationCenter.default
        observers.append(center.addObserver(forName: NSApplication.willResignActiveNotification, object: nil,
                                            queue: .main) { [weak self] _ in
            MainActor.assumeIsolated { self?.scheduleBackgroundLock() }
        })
        observers.append(center.addObserver(forName: NSApplication.didBecomeActiveNotification, object: nil,
                                            queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.pendingLock?.cancel()
                self?.pendingLock = nil
            }
        })
        let model = model
        Task { await model.start(making: Launch.environment) }
    }

    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool { false }

    func applicationWillTerminate(_ notification: Notification) {
        #if DEBUG
        UITesting.removeStore()
        #endif
    }

    private func scheduleBackgroundLock() {
        pendingLock?.cancel()
        let model = model
        pendingLock = Task { @MainActor in
            try? await Task.sleep(nanoseconds: UInt64(Self.backgroundGrace * 1_000_000_000))
            guard !Task.isCancelled, case .unlocked = model.phase else { return }
            await model.lock()
        }
    }
}

/// How the session's environment is made: the live one (the keychain, Application
/// Support, the key root `KeyRootFactory` chooses, the data source Info.plist names),
/// or in Debug builds launched with `-MidgardUITesting` a non-prompting test key root
/// and a fresh temporary store. Called off the main thread by `AppModel.start(making:)`.
enum Launch {
    static let reason = "Unlock Midgard"

    static var device: DeviceInfo {
        let mib = ProcessInfo.processInfo.physicalMemory / 1_048_576
        return DeviceInfo(ramMb: UInt32(min(mib, UInt64(UInt32.max))), hasAccelerator: true)
    }

    @Sendable static func environment() throws -> AppEnvironment {
        #if DEBUG
        if UITesting.isActive { return try UITesting.environment(device: device) }
        #endif
        return try AppEnvironment.live(reason: reason, device: device)
    }
}

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
                // Tall enough for Midgard's eleven screens under the six consoles: a
                // row cut by the window's bottom edge fails the contrast audit.
                .frame(minWidth: 900, minHeight: 760)
        } defaultValue: {
            ConsoleRoute()
        }
        .defaultSize(width: 1100, height: 820)
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
        #if DEBUG
        if UITesting.isActive { NSApp.activate(ignoringOtherApps: true) }
        #endif
        // SwiftUI sometimes opens no window at launch (seen under XCUITest, launched in
        // the background): open one, as File ▸ New Window would.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { MainActor.assumeIsolated { Self.ensureWindow() } }
    }

    func applicationShouldHandleReopen(_ sender: NSApplication, hasVisibleWindows flag: Bool) -> Bool {
        if !flag { Self.ensureWindow() }
        return false
    }

    /// Open a console window when none is visible, through the window group's own
    /// File ▸ New Window command (⌘N). It depends on that menu item: it finds it by its
    /// ⌘N key equivalent, so if Commands.swift replaces or rebinds New Window (or the
    /// scene stops being a WindowGroup) this silently opens nothing, and a launch under
    /// XCUITest can come up with no window.
    static func ensureWindow() {
        guard !NSApp.windows.contains(where: { $0.isVisible && $0.canBecomeMain }) else { return }
        guard let file = NSApp.mainMenu?.items.first(where: { $0.submenu?.items.contains { $0.keyEquivalent == "n" } ?? false }),
              let menu = file.submenu,
              let index = menu.items.firstIndex(where: { $0.keyEquivalent == "n" && $0.keyEquivalentModifierMask == .command })
        else { return }
        menu.performActionForItem(at: index)
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

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
        // The web console's reference width (Heimdall's frame needs it), and the height
        // its inspector needs under the app's banners and status line.
        .defaultSize(width: 1400, height: 1000)
        .commands { MidgardCommands(model: delegate.model) }

        // Your Data: imported files, the gateway, the feeds (Session ▸ Your Data…, ⇧⌘D).
        Window("Your Data", id: YourDataWindow.id) {
            YourDataView(app: delegate.model, compact: false)
                .frame(minWidth: 560, minHeight: 560)
        }
        .defaultSize(width: 720, height: 860)

        Settings {
            SettingsView(model: delegate.model)
        }
    }
}

/// Owns the session and what is app-wide about it: starting it off the main thread,
/// postponing the idle lock on input, locking at once when the Mac sleeps, its screens
/// sleep or the user switches away (fast user switching), and locking after the app has
/// been in the background for the grace period. [FR-C-9]
@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    /// How long the app may stay inactive before it locks.
    static let backgroundGrace: TimeInterval = 30

    let model = AppModel()
    private var monitor: Any?
    /// Each observer with the centre it was added to.
    private var observers: [(NotificationCenter, NSObjectProtocol)] = []
    private var pendingLock: Task<Void, Never>?

    func applicationDidFinishLaunching(_ notification: Notification) {
        monitor = NSEvent.addLocalMonitorForEvents(
            matching: [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown, .scrollWheel]
        ) { [weak self] event in
            MainActor.assumeIsolated { self?.model.touch() }
            return event
        }
        let app = NotificationCenter.default
        observe(app, NSApplication.willResignActiveNotification) { $0.scheduleBackgroundLock() }
        observe(app, NSApplication.didBecomeActiveNotification) { $0.cancelBackgroundLock() }
        // Workspace notifications come only through the workspace's own centre.
        let workspace = NSWorkspace.shared.notificationCenter
        for name in [NSWorkspace.willSleepNotification, NSWorkspace.screensDidSleepNotification,
                     NSWorkspace.sessionDidResignActiveNotification] {
            observe(workspace, name) { $0.lockNow() }
        }
        let model = model
        Task { await model.start(making: Launch.environment) }
        #if DEBUG
        if UITestingEnvironment.requested() {
            NSApp.activate(ignoringOtherApps: true)
            // SwiftUI sometimes opens no window at launch under XCUITest (the app is
            // launched in the background): open one, as File ▸ New Window would.
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { MainActor.assumeIsolated { Self.ensureWindow() } }
        }
        #endif
    }

    private func observe(_ center: NotificationCenter, _ name: Notification.Name,
                         _ action: @escaping @MainActor (AppDelegate) -> Void) {
        let token = center.addObserver(forName: name, object: nil, queue: .main) { [weak self] _ in
            MainActor.assumeIsolated {
                if let self { action(self) }
            }
        }
        observers.append((center, token))
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
        if let monitor { NSEvent.removeMonitor(monitor) }
        monitor = nil
        for (center, token) in observers { center.removeObserver(token) }
        observers = []
        cancelBackgroundLock()
        #if DEBUG
        if UITestingEnvironment.requested() { UITestingEnvironment.removeCurrentStore() }
        #endif
    }

    private func scheduleBackgroundLock() {
        pendingLock?.cancel()
        let model = model
        pendingLock = Task { @MainActor in
            // The continuous clock keeps counting while the Mac sleeps (the uptime clock
            // would not), so the grace period is wall time.
            try? await Task.sleep(for: .seconds(Self.backgroundGrace), clock: .continuous)
            guard !Task.isCancelled, model.isUnlocked else { return }
            await model.lock()
        }
    }

    private func cancelBackgroundLock() {
        pendingLock?.cancel()
        pendingLock = nil
    }

    /// The Mac is going to sleep, its screens slept, or another user took the screen:
    /// lock now, without waiting for the grace period.
    private func lockNow() {
        cancelBackgroundLock()
        guard model.isUnlocked else { return }
        let model = model
        Task { await model.lock() }
    }
}

/// How the session's environment is made: the live one (the keychain, Application
/// Support, the key root `KeyRootFactory` chooses, the data source Info.plist names),
/// or in Debug builds launched with `-MidgardUITesting` MidgardKit's
/// `UITestingEnvironment` (a key root that never prompts, secrets in memory, a fresh
/// temporary store). Called off the main thread by `AppModel.start(making:)`.
enum Launch {
    static let reason = "Unlock Midgard"

    static var device: DeviceInfo {
        let mib = ProcessInfo.processInfo.physicalMemory / 1_048_576
        return DeviceInfo(ramMb: UInt32(min(mib, UInt64(UInt32.max))), hasAccelerator: true)
    }

    @Sendable static func environment() throws -> AppEnvironment {
        #if DEBUG
        if UITestingEnvironment.requested() { return try UITestingEnvironment.make(reason: reason, device: device) }
        #endif
        return try AppEnvironment.live(reason: reason, device: device)
    }
}

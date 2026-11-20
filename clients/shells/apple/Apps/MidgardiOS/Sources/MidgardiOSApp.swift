import MidgardCore
import MidgardKit
import SwiftUI
import UIKit

/// The iPhone app (Task B3): one window, the session's phases, the six consoles.
/// It ships with no data (owner decision 19 Nov 2026): Info.plist `MidgardDataSource` is
/// `none`, so the core signs in its one local user and every screen is empty.
/// [FR-C-1] [FR-C-5]
@main
struct MidgardiOSApp: App {
    @UIApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    init() {
        _ = MGFonts.registerAll()
    }

    var body: some Scene {
        WindowGroup {
            RootView(model: delegate.model, shortcuts: delegate.shortcuts)
        }
    }
}

/// Owns the session and the ⌘1…⌘6 key commands. The commands are in the app's main
/// menu (`buildMenu(with:)`) and their action is handled here, at the end of the
/// responder chain: UIKit offers a key press to the first responder and everything
/// after it, so the commands fire whatever holds focus (the search field being edited
/// as well) and with no first responder at all. [FR-C-1] [RC-49]
///
/// This relies on undocumented behaviour: with `@UIApplicationDelegateAdaptor`, the
/// application's delegate is SwiftUI's own, which forwards the actions it does not
/// handle (here `selectConsole(_:)` and `canPerformAction(_:withSender:)`) to this
/// object. Should a SwiftUI release stop forwarding them, ⌘1…⌘6 would do nothing:
/// `SessionFlowTests.testCommandDigitsWorkWhileSearching` presses ⌘2 while the search
/// field is being edited and fails if that happens.
@MainActor
final class AppDelegate: UIResponder, UIApplicationDelegate {
    let model = AppModel()
    let shortcuts = ConsoleShortcuts()

    static let consoleMenu = UIMenu.Identifier("io.veldris.midgard.consoles")

    override func buildMenu(with builder: UIMenuBuilder) {
        super.buildMenu(with: builder)
        guard builder.system == .main else { return }
        let commands = ConsoleCatalog.consoles.map { console in
            let command = UIKeyCommand(title: "Show \(console.name)", action: #selector(selectConsole(_:)),
                                       input: "\(console.number)", modifierFlags: .command,
                                       propertyList: console.number)
            command.wantsPriorityOverSystemBehavior = true
            return command
        }
        let menu = UIMenu(title: "Console", identifier: Self.consoleMenu, options: .displayInline, children: commands)
        if builder.menu(for: .view) != nil {
            builder.insertChild(menu, atStartOfMenu: .view)
        } else {
            builder.insertSibling(menu, afterMenu: .application)
        }
    }

    /// ⌘1…⌘6: select that console, once signed in.
    @objc func selectConsole(_ command: UIKeyCommand) {
        guard model.isUnlocked, let number = command.propertyList as? Int else { return }
        model.touch()
        shortcuts.select(number)
    }

    override func canPerformAction(_ action: Selector, withSender sender: Any?) -> Bool {
        if action == #selector(selectConsole(_:)) { return model.isUnlocked }
        return super.canPerformAction(action, withSender: sender)
    }
}

/// The console a ⌘-digit asked for, handed from the app delegate to `RootView`. Each
/// request carries a serial number, so pressing the same digit twice is still a change.
@MainActor
final class ConsoleShortcuts: ObservableObject {
    struct Request: Equatable {
        let number: Int
        let serial: Int
    }

    @Published private(set) var request: Request?

    func select(_ number: Int) {
        request = Request(number: number, serial: (request?.serial ?? 0) + 1)
    }
}

/// How the app builds its environment. Called off the main thread by
/// `AppModel.start(making:)`: it reads the keychain and the disk. In Debug builds
/// launched with `-MidgardUITesting`, MidgardKit's `UITestingEnvironment` instead.
enum LaunchEnvironment {
    static let unlockReason = "Unlock Midgard"

    static func make(arguments: [String] = ProcessInfo.processInfo.arguments) throws -> AppEnvironment {
        let device = DeviceInfo(ramMb: UInt32(clamping: ProcessInfo.processInfo.physicalMemory / (1024 * 1024)),
                                hasAccelerator: true)
        #if DEBUG
        if UITestingEnvironment.requested(arguments) {
            return try UITestingEnvironment.make(reason: unlockReason, device: device)
        }
        #endif
        return try AppEnvironment.live(reason: unlockReason, device: device)
    }
}

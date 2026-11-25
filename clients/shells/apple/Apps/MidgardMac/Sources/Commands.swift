import MidgardKit
import SwiftUI

/// The key window's console route, for the menu commands.
struct ConsoleRouteKey: FocusedValueKey {
    typealias Value = Binding<ConsoleRoute>
}

extension FocusedValues {
    var consoleRoute: Binding<ConsoleRoute>? {
        get { self[ConsoleRouteKey.self] }
        set { self[ConsoleRouteKey.self] = newValue }
    }
}

/// Menu commands: ⌘1…⌘6 select a console in the key window; ⌘R syncs now, ⌘L locks,
/// ⇧⌘O toggles offline. ⌘N (File ▸ New Window) comes with the route window group.
/// [FR-C-1] [FR-C-9] [RC-49]
enum YourDataWindow {
    static let id = "your-data"
}

struct MidgardCommands: Commands {
    @ObservedObject var model: AppModel
    @FocusedBinding(\.consoleRoute) private var route: ConsoleRoute?
    @Environment(\.openWindow) private var openWindow

    private var unlocked: Bool { model.isUnlocked }

    var body: some Commands {
        CommandMenu("Console") {
            ForEach(Array(ConsoleCatalog.consoles.enumerated()), id: \.element.id) { index, console in
                Button(console.name) { route?.select(console: index) }
                    .keyboardShortcut(KeyEquivalent(Character(String(index + 1))), modifiers: .command)
                    .disabled(route == nil || !unlocked)
            }
        }
        CommandMenu("Session") {
            Button("Sync Now") { Task { await model.syncNow() } }
                .keyboardShortcut("r", modifiers: .command)
                .disabled(!unlocked)
            Button(model.isOffline ? "Go Online" : "Go Offline") {
                let offline = model.isOffline
                Task { await model.setOffline(!offline) }
            }
            .keyboardShortcut("o", modifiers: [.command, .shift])
            .disabled(!unlocked)
            Divider()
            Button("Your Data…") { openWindow(id: YourDataWindow.id) }
                .keyboardShortcut("d", modifiers: [.command, .shift])
            Divider()
            Button("Lock") { Task { await model.lock() } }
                .keyboardShortcut("l", modifiers: .command)
                .disabled(!unlocked)
        }
    }
}

/// Settings: Reset this device (behind a confirmation) and the acknowledgements; in the
/// 3D and maps editions only, About, from the edition provider (`AppModel.about`). The
/// release app has no provider, so its Settings are unchanged. With no data source, the
/// reset empties the local store and forgets this device's key. [FR-C-1] [FR-C-9] [DD-6] [DD-7]
struct SettingsView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        TabView {
            GeneralSettings(model: model)
                .tabItem { Label("General", systemImage: "gearshape") }
            YourDataView(app: model, compact: true)
                .tabItem { Label("Your Data", systemImage: "tray.and.arrow.down") }
            if let about = model.about {
                NavigationStack { about }
                    .tabItem { Label("About", systemImage: "info.circle") }
            }
            NavigationStack { AcknowledgementsView() }
                .tabItem { Label("Acknowledgements", systemImage: "doc.text") }
        }
        .frame(width: 560, height: 560)
    }
}

struct GeneralSettings: View {
    @ObservedObject var model: AppModel
    @State private var confirming = false

    private var explanation: String {
        model.banner == .noSource ? ResetText.noSource : ResetText.explanation
    }

    var body: some View {
        Form {
            Section {
                Text(explanation).font(MGFonts.sans(.body))
                Button("Reset this device", role: .destructive) { confirming = true }
                    .frame(minHeight: 44)
                    .accessibilityIdentifier("mg.settings.reset")
                    .confirmationDialog("Reset this device?", isPresented: $confirming) {
                        Button("Reset this device", role: .destructive) { Task { await model.resetDemo() } }
                    } message: {
                        Text(explanation)
                    }
            }
        }
        .padding(MGSpace.s5)
    }
}

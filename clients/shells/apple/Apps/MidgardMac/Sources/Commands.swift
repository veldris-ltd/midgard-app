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
struct MidgardCommands: Commands {
    @ObservedObject var model: AppModel
    @FocusedBinding(\.consoleRoute) private var route: ConsoleRoute?

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
            Button("Lock") { Task { await model.lock() } }
                .keyboardShortcut("l", modifiers: .command)
                .disabled(!unlocked)
        }
    }
}

/// Settings: Reset demo (behind a confirmation) and the acknowledgements; in the 3D and
/// maps editions only, About, from the edition provider (`AppModel.about`). The release
/// app has no provider, so its Settings are unchanged. With no data source, Reset demo empties the local store
/// and forgets this device's key. [FR-C-1] [FR-C-9] [DD-6] [DD-7]
struct SettingsView: View {
    @ObservedObject var model: AppModel

    var body: some View {
        TabView {
            GeneralSettings(model: model)
                .tabItem { Label("General", systemImage: "gearshape") }
            if let about = model.about {
                NavigationStack { about }
                    .tabItem { Label("About", systemImage: "info.circle") }
            }
            NavigationStack { AcknowledgementsView() }
                .tabItem { Label("Acknowledgements", systemImage: "doc.text") }
        }
        .frame(width: 520, height: 340)
    }
}

struct GeneralSettings: View {
    @ObservedObject var model: AppModel
    @State private var confirming = false

    private var explanation: String {
        model.banner == .noSource
            ? "Deletes the local store on this Mac and this device's key. You will set up this device again."
            : "Deletes this device's saved data and key. Signing in again starts the demo from the beginning."
    }

    var body: some View {
        Form {
            Section {
                Text(explanation).font(MGFonts.sans(.body))
                Button("Reset demo", role: .destructive) { confirming = true }
                    .frame(minHeight: 44)
                    .accessibilityIdentifier("mg.settings.reset")
                    .confirmationDialog("Reset the demo?", isPresented: $confirming) {
                        Button("Reset demo", role: .destructive) { Task { await model.resetDemo() } }
                    } message: {
                        Text(explanation)
                    }
            }
        }
        .padding(MGSpace.s5)
    }
}

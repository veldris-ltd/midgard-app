import AppKit
import MidgardShellCore
import SwiftUI

@main
struct MidgardApp: App {
    @NSApplicationDelegateAdaptor(AppDelegate.self) private var delegate

    var body: some Scene {
        Window("Midgard", id: "main") {
            RootView()
                .environmentObject(delegate.model)
        }
        .defaultSize(Self.largeSize)
        .windowResizability(.contentMinSize)
        .commands {
            CommandGroup(replacing: .newItem) {}
        }
    }

    /// Opens large: the whole visible screen, at least the minimum size.
    static var largeSize: CGSize {
        let visible = NSScreen.main?.visibleFrame.size ?? CGSize(width: 1440, height: 900)
        return CGSize(width: max(1280, visible.width), height: max(800, visible.height))
    }
}

@MainActor
final class AppDelegate: NSObject, NSApplicationDelegate {
    let model = AppModel()
    private var signals: [DispatchSourceSignal] = []
    private var terminating = false

    func applicationWillFinishLaunching(_ notification: Notification) {
        // Before any window or service: a second shell hands over to the first.
        if !model.claimSingleInstance() { exit(0) }
    }

    func applicationDidFinishLaunching(_ notification: Notification) {
        // A `swift run` binary has no bundle: make it a normal app in the Dock and in front.
        NSApp.setActivationPolicy(.regular)
        NSApp.activate(ignoringOtherApps: true)
        for sig in [SIGTERM, SIGINT] {
            signal(sig, SIG_IGN)
            let source = DispatchSource.makeSignalSource(signal: sig, queue: .main)
            source.setEventHandler {
                // Through the run loop, not from inside this main-queue block: while
                // `.terminateLater` waits, the main queue must be free to run the shutdown.
                MainActor.assumeIsolated {
                    self.model.log("received signal \(sig); quitting")
                    NSApp.perform(#selector(NSApplication.terminate(_:)), with: nil, afterDelay: 0)
                }
            }
            source.resume()
            signals.append(source)
        }
        model.launch()
    }

    /// Closing the window quits Midgard and stops every service (a presenter app).
    func applicationShouldTerminateAfterLastWindowClosed(_ sender: NSApplication) -> Bool {
        model.log("window closed")
        return true
    }

    func applicationShouldTerminate(_ sender: NSApplication) -> NSApplication.TerminateReply {
        if !terminating {
            terminating = true
            model.log("quit requested")
            Task {
                await self.model.shutdown()
                sender.reply(toApplicationShouldTerminate: true)
            }
        }
        return .terminateLater
    }
}

/// The starting screen until everything is up, then the toolbar and the web view.
struct RootView: View {
    @EnvironmentObject var model: AppModel
    @State private var confirmingReset = false

    var body: some View {
        Group {
            if model.phase == .ready {
                MainView()
            } else {
                StartingView()
            }
        }
        .frame(minWidth: 1280, minHeight: 800)
        .background(Palette.page)
        .toolbar { toolbar }
        .alert("Reset the demo?", isPresented: $confirmingReset) {
            Button("Reset", role: .destructive) { model.resetDemo() }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This restores the starting data and removes changes made during the demo. It takes about a minute.")
        }
    }

    @ToolbarContentBuilder private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .principal) {
            Picker("Show", selection: Binding(get: { model.section }, set: { model.go($0) })) {
                ForEach(Destination.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .frame(width: 440)
            .disabled(model.phase != .ready)
        }
        ToolbarItemGroup(placement: .primaryAction) {
            Menu {
                Text("Switch user")
                ForEach(model.users) { user in
                    Button {
                        model.switchUser(user)
                    } label: {
                        if user == model.currentUser {
                            Label(user.label, systemImage: "checkmark")
                        } else {
                            Text(user.label)
                        }
                    }
                    .disabled(user == model.currentUser)
                }
            } label: {
                Label(model.currentUser?.label ?? "Not signed in", systemImage: "person.crop.circle")
                    .labelStyle(.titleAndIcon)
            }
            .help("Switch user")
            .disabled(model.phase != .ready || model.switchingUser)

            Button {
                confirmingReset = true
            } label: {
                Label("Reset demo…", systemImage: "arrow.counterclockwise")
                    .labelStyle(.titleAndIcon)
            }
            .help("Restore the starting data")
            .disabled(model.phase != .ready)
        }
    }
}

struct MainView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        WebView(controller: model.web)
            .overlay(alignment: .bottom) {
                if let note = model.note {
                    HStack(alignment: .top, spacing: 12) {
                        Image(systemName: "info.circle")
                        Text(note).fixedSize(horizontal: false, vertical: true)
                        Button("Dismiss") { model.note = nil }
                    }
                    .padding(12)
                    .background(.regularMaterial)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .padding(16)
                    .frame(maxWidth: 720)
                }
            }
    }
}

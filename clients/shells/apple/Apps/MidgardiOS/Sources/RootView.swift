import MidgardCore
import MidgardKit
import SwiftUI

/// The app's root: the session's phases (enrol, sign in, locked, the consoles) under the
/// source banner, from MidgardKit's `SessionRootView`, over a status line that carries
/// the audit's hooks. It starts the model off the main thread, locks when the app goes
/// to the background, and covers the screen whenever the scene is not active (so the app
/// switcher's snapshot shows nothing). The accent is set here to design tokens that meet
/// AA in both modes (the system blue does not, on the sign-in screen's persona button).
/// [FR-C-1] [FR-C-9] [AC-C-11] [RC-49]
struct RootView: View {
    @ObservedObject var model: AppModel
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    @State private var started = false
    /// The console shown once signed in (1…6): kept here so the status line can say it.
    @State private var console = 1

    private var reducedMotion: Bool { MotionPolicy.reduced(system: systemReduceMotion) }

    var body: some View {
        VStack(spacing: 0) {
            SessionRootView(model: model) { persona in
                ConsoleShell(model: model, persona: persona, selection: $console)
                    // ⌘1…⌘6 while signed in (ConsoleKeyCommands: a new responder takes over
                    // from the one it replaces when SwiftUI rebuilds it).
                    .background {
                        ConsoleKeyCommands { console = $0 }.accessibilityHidden(true)
                    }
            }
            // The banner at the top of the screen, whatever the phase's content.
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            StatusLine(reduced: reducedMotion, console: model.isUnlocked ? console : nil)
        }
        .tint(MidgardStyle.accent)
        .overlay {
            if scenePhase != .active { PrivacyCover() }
        }
        .transaction { transaction in
            if reducedMotion {
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        }
        .task {
            guard !started else { return }
            started = true
            await model.start(making: { try LaunchEnvironment.make() })
        }
        .onChange(of: scenePhase) { _, phase in
            // Lock on leaving the app; only a signed-in session has anything to lock.
            guard phase == .background, model.isUnlocked else { return }
            Task { await model.lock() }
        }
    }
}

/// Colours the app sets over the system's, from the design tokens.
enum MidgardStyle {
    /// Buttons and links: the accent, pressed shade on light, hover shade on dark
    /// (each above 4.5:1 on the page).
    static let accent = Color(mgLight: (MGToken.accentPress, 1), dark: (MGToken.accentHover, 1))
    /// Secondary text: the muted text token of each mode (above 4.5:1 on the page).
    static let secondaryText = Color(mgLight: MGToken.Light.textMuted, dark: (MGToken.textMuted, 1))
}

/// The status line at the bottom of the screen: which console is shown ("Console 1 of
/// 6: Midgard", `mg.console.selected` = 1…6, once signed in) and whether animations run
/// ("Animations off (Reduce Motion)", `mg.motion` = reduced/full, in every phase).
/// Visible text, as on the Mac, so VoiceOver and the contrast audit see what the UI
/// tests read (a hidden one-point marker failed the contrast audit). [RC-49] [AC-C-11]
struct StatusLine: View {
    let reduced: Bool
    let console: Int?
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        // One line, or, at accessibility text sizes, one statement under the other.
        let layout = typeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: MGSpace.s1))
            : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: MGSpace.s3))
        layout {
            if let console {
                Text("Console \(console) of \(Console.all.count): \(Console.all[console - 1].name)")
                    .accessibilityValue(Text("\(console)"))
                    .accessibilityIdentifier("mg.console.selected")
            }
            if !typeSize.isAccessibilitySize { Spacer(minLength: MGSpace.s2) }
            Text(reduced ? "Animations off (Reduce Motion)" : "Animations on")
                .accessibilityValue(Text(reduced ? "reduced" : "full"))
                .accessibilityIdentifier("mg.motion")
        }
        .font(MGFonts.sans(.caption))
        .foregroundStyle(Color.primary)
        .padding(.horizontal, MGSpace.s4)
        .padding(.vertical, MGSpace.s1)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(Color(uiColor: .systemBackground))
        .overlay(alignment: .top) { Divider() }
    }
}

extension AppModel {
    var isUnlocked: Bool {
        if case .unlocked = phase { return true }
        return false
    }
}

/// What the app switcher shows instead of the screen.
struct PrivacyCover: View {
    var body: some View {
        ZStack {
            Color(uiColor: .systemBackground)
            VStack(spacing: MGSpace.s3) {
                Image(systemName: "lock.fill").font(.largeTitle).accessibilityHidden(true)
                Text("Midgard").font(MGFonts.display(.title))
            }
        }
        .ignoresSafeArea()
        .accessibilityElement(children: .combine)
        .accessibilityIdentifier("mg.privacy")
    }
}

/// Signed in: the six consoles, in the order Midgard, Admin, Heimdall, Forge, Mimir,
/// Bifrost, switched by a bar of six at the bottom (⌘1…⌘6 on a hardware keyboard).
/// Not a system `TabView`: an iPhone tab bar shows five, and the sixth console would go
/// under UIKit's More list, which wraps the console's own navigation stack in its own
/// (and whose rows did not respond in the simulator). Only the selected console is in
/// the view tree, so VoiceOver and the audit see one; each console's open screen and
/// search are kept here, so switching back returns to where it was.
struct ConsoleShell: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @Binding var selection: Int
    @State private var paths: [Int: [String]] = [:]
    @State private var queries: [Int: String] = [:]

    private var console: Console { Console.all[min(max(selection, 1), Console.all.count) - 1] }

    var body: some View {
        ConsoleTab(model: model, console: console, persona: persona,
                   path: Binding(get: { paths[console.number] ?? [] }, set: { paths[console.number] = $0 }),
                   query: Binding(get: { queries[console.number] ?? "" }, set: { queries[console.number] = $0 }))
            .id(console.number)
            .safeAreaInset(edge: .bottom, spacing: 0) { ConsoleBar(selection: $selection) }
    }
}

/// ⌘1…⌘6 on a hardware keyboard, as UIKit key commands on a view that takes first
/// responder whenever nothing else holds it. (`.keyboardShortcut` on the console bar's
/// buttons, or on hidden buttons, did not fire in the simulator: a screen of buttons
/// has no first responder for the key commands to start from.)
struct ConsoleKeyCommands: UIViewRepresentable {
    let select: (Int) -> Void

    func makeUIView(context: Context) -> KeyCommandView {
        let view = KeyCommandView()
        updateUIView(view, context: context)
        return view
    }

    func updateUIView(_ view: KeyCommandView, context: Context) { view.select = select }

    final class KeyCommandView: UIView {
        var select: ((Int) -> Void)?
        private var observers: [NSObjectProtocol] = []

        override var canBecomeFirstResponder: Bool { true }

        override var keyCommands: [UIKeyCommand]? {
            Console.all.map { console in
                let command = UIKeyCommand(title: "Show \(console.name)", action: #selector(pick(_:)),
                                           input: "\(console.number)", modifierFlags: .command,
                                           propertyList: console.number)
                command.wantsPriorityOverSystemBehavior = true
                return command
            }
        }

        @objc private func pick(_ command: UIKeyCommand) {
            if let number = command.propertyList as? Int { select?(number) }
        }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            observers.forEach(NotificationCenter.default.removeObserver)
            observers = []
            guard window != nil else { return }
            claim()
            // Back to first responder when a text field (the search) lets go of it.
            for name in [UITextField.textDidEndEditingNotification, UITextView.textDidEndEditingNotification] {
                observers.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) {
                    [weak self] _ in MainActor.assumeIsolated { self?.claim() }
                })
            }
        }

        override func resignFirstResponder() -> Bool {
            let resigned = super.resignFirstResponder()
            // Take it back at once when nothing took it over, else once a menu or sheet
            // lets go.
            if resigned {
                claim()
                claim(after: 0.5)
            }
            return resigned
        }

        /// Become first responder, unless another responder (a text field being
        /// edited, a menu) holds it.
        private func claim(after delay: TimeInterval = 0) {
            DispatchQueue.main.asyncAfter(deadline: .now() + delay) { [weak self] in
                guard let self, self.window != nil, !self.isFirstResponder else { return }
                // Free, or held by the responder this one replaces (SwiftUI makes a new one
                // when the console changes, before the old one leaves the window).
                let holder = FirstResponder.current()
                guard holder == nil || holder is KeyCommandView else { return }
                self.becomeFirstResponder()
            }
        }
    }
}

/// The current first responder, found the documented way: an action sent to a nil
/// target goes along the responder chain, from the first responder if there is one.
enum FirstResponder {
    nonisolated(unsafe) private static weak var found: UIResponder?

    @MainActor static func current() -> UIResponder? {
        found = nil
        UIApplication.shared.sendAction(#selector(UIResponder.mg_reportFirstResponder), to: nil, from: nil, for: nil)
        // With no first responder the action still reaches the key window or the
        // application, which are not holding it.
        return found.flatMap { $0.isFirstResponder ? $0 : nil }
    }

    fileprivate static func report(_ responder: UIResponder) { found = responder }
}

extension UIResponder {
    @objc fileprivate func mg_reportFirstResponder() { FirstResponder.report(self) }
}

/// The six consoles' buttons. At accessibility text sizes the
/// names give way to the symbols (a long press shows the large-content viewer).
struct ConsoleBar: View {
    @Binding var selection: Int
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        HStack(spacing: 0) {
            ForEach(Console.all) { console in
                let selected = console.number == selection
                Button {
                    selection = console.number
                } label: {
                    VStack(spacing: 2) {
                        // Capped like a system tab bar's: six symbols at accessibility
                        // sizes would not fit the screen's width.
                        Image(systemName: console.symbol)
                            .font(.title3)
                            .dynamicTypeSize(...DynamicTypeSize.xxxLarge)
                            .accessibilityHidden(true)
                        if !typeSize.isAccessibilitySize {
                            Text(console.name)
                                .font(MGFonts.sans(.caption2).weight(selected ? .semibold : .regular))
                                .lineLimit(1)
                                .minimumScaleFactor(0.6)
                        }
                    }
                    .foregroundStyle(Color.primary)
                    .frame(maxWidth: .infinity, minHeight: 48)
                    .background {
                        if selected {
                            RoundedRectangle(cornerRadius: 10).fill(Color(uiColor: .tertiarySystemFill))
                        }
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text(console.name))
                .accessibilityAddTraits(selected ? [.isSelected] : [])
                .accessibilityShowsLargeContentViewer {
                    Image(systemName: console.symbol)
                    Text(console.name)
                }
                .accessibilityIdentifier("mg.console.tab.\(console.slug)")
            }
        }
        .padding(.horizontal, MGSpace.s2)
        .padding(.vertical, MGSpace.s1)
        .background(Color(uiColor: .systemBackground))
        .overlay(alignment: .top) { Divider() }
    }
}

/// One console: its screens, searchable, pull to refresh (sync now), each opening its
/// placeholder.
struct ConsoleTab: View {
    @ObservedObject var model: AppModel
    let console: Console
    let persona: PersonaInfo
    @Binding var path: [String]
    @Binding var query: String

    private var screens: [String] {
        let q = query.trimmingCharacters(in: .whitespaces)
        return q.isEmpty ? console.screens : console.screens.filter { $0.localizedCaseInsensitiveContains(q) }
    }

    var body: some View {
        NavigationStack(path: $path) {
            // A scroll view of buttons rather than a List of NavigationLinks: List rows
            // did not respond to taps under the session's tap gesture (MidgardKit's
            // SessionRootView touches the idle timer with a simultaneous TapGesture).
            ScrollView {
                LazyVStack(spacing: 0) {
                    ScreenSearchField(console: console, query: $query)
                        .padding(.horizontal, MGSpace.s4)
                        .padding(.vertical, MGSpace.s2)
                    // mg.0 is the session menu above the list; the rows follow in order.
                    ForEach(Array(screens.enumerated()), id: \.element) { index, screen in
                        Button {
                            path.append(screen)
                        } label: {
                            HStack {
                                Text(screen).font(MGFonts.sans(.body)).multilineTextAlignment(.leading)
                                Spacer(minLength: MGSpace.s2)
                                Image(systemName: "chevron.right").font(.footnote.weight(.semibold))
                                    .accessibilityHidden(true)
                            }
                            .foregroundStyle(Color.primary)
                            .padding(.horizontal, MGSpace.s4)
                            .padding(.vertical, MGSpace.s3)
                            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(Text(screen))
                        .accessibilityHint(Text("Opens the screen"))
                        .accessibilityIdentifier("mg.\(index + 1).\(console.slug).\(Console.slug(screen))")
                        Divider().padding(.leading, MGSpace.s4)
                    }
                }
            }
            .overlay {
                if screens.isEmpty {
                    ContentUnavailableView.search(text: query)
                }
            }
            .navigationTitle(console.name)
            // Inline: a large title leaves Midgard's eleventh screen cut by the console bar
            // on an iPhone 17, and a row cut by an edge fails the contrast audit.
            .navigationBarTitleDisplayMode(.inline)
            .navigationDestination(for: String.self) { screen in
                ScreenPlaceholder(model: model, console: console, screen: screen, persona: persona)
            }
            .refreshable { await model.syncNow() }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) { SessionMenu(model: model, persona: persona) }
            }
        }
    }
}

/// The search over a console's screens, at the top of its list. Not `.searchable`: the
/// system search bar's grey placeholder fails the contrast audit and its field clips at
/// accessibility text sizes. This one uses the app's fonts, a short prompt lettered in
/// the secondary text token, and a field at least 44 pt tall (its own frame is the hit
/// region the audit measures, not the capsule around it).
struct ScreenSearchField: View {
    let console: Console
    @Binding var query: String
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: MGSpace.s2) {
            Image(systemName: "magnifyingglass").accessibilityHidden(true)
            TextField(text: $query, prompt: Text("Search").foregroundStyle(MidgardStyle.secondaryText)) {
                Text("Search \(console.name) screens")
            }
            .font(MGFonts.sans(.body))
            .frame(minHeight: 44)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .submitLabel(.search)
            .focused($focused)
            .accessibilityLabel(Text("Search \(console.name) screens"))
            .accessibilityIdentifier("mg.search.\(console.slug)")
            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: "xmark.circle.fill").frame(minWidth: 44, minHeight: 44)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text("Clear search"))
                .accessibilityIdentifier("mg.search.clear")
            }
        }
        .foregroundStyle(Color.primary)
        .padding(.horizontal, MGSpace.s3)
        .background(RoundedRectangle(cornerRadius: 12).fill(Color(uiColor: .secondarySystemBackground)))
        .contentShape(Rectangle())
        .onTapGesture { focused = true }
    }
}

/// A screen until its phase: what it is and when it arrives.
struct ScreenPlaceholder: View {
    @ObservedObject var model: AppModel
    let console: Console
    let screen: String
    let persona: PersonaInfo

    var body: some View {
        ScrollView {
            VStack(spacing: MGSpace.s3) {
                Image(systemName: console.symbol).font(.largeTitle).accessibilityHidden(true)
                Text(screen).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
                Text("This \(console.name) screen arrives in Phase \(console.phase). With no data source connected it will show its empty state.")
                    .font(MGFonts.sans(.body))
                    .fixedSize(horizontal: false, vertical: true)
            }
            .multilineTextAlignment(.center)
            .foregroundStyle(Color.primary)
            .padding(MGSpace.s5)
            .frame(maxWidth: .infinity)
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("mg.placeholder.\(console.slug).\(Console.slug(screen))")
        }
        .navigationTitle(screen)
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) { SessionMenu(model: model, persona: persona) }
        }
    }
}

/// The session menu: who is signed in (and switching persona), sync now, offline, lock.
struct SessionMenu: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo

    private var offline: Bool { model.chips?.sync.phase == .offline }

    var body: some View {
        Menu {
            Section("Signed in as \(persona.display)") {
                ForEach(model.personas, id: \.id) { other in
                    Button {
                        Task { await model.switchPersona(other) }
                    } label: {
                        if other.id == persona.id {
                            Label(other.display, systemImage: "checkmark")
                        } else {
                            Text(other.display)
                        }
                    }
                    .disabled(other.id == persona.id)
                }
            }
            Button {
                Task { await model.syncNow() }
            } label: {
                Label("Sync now", systemImage: "arrow.triangle.2.circlepath")
            }
            .accessibilityIdentifier("mg.session.sync")
            Button {
                Task { await model.setOffline(!offline) }
            } label: {
                Label(offline ? "Go online" : "Go offline", systemImage: offline ? "wifi" : "wifi.slash")
            }
            .accessibilityIdentifier("mg.session.offline")
            Button {
                Task { await model.lock() }
            } label: {
                Label("Lock", systemImage: "lock")
            }
            .accessibilityIdentifier("mg.session.lock")
        } label: {
            Label("Session", systemImage: "person.crop.circle")
                .frame(minWidth: 44, minHeight: 44)
        }
        .accessibilityLabel(Text("Session: \(persona.display)"))
        .accessibilityIdentifier("mg.0.session-menu")

    }
}

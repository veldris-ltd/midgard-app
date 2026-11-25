import MidgardCore
import SwiftUI

/// The locked screen: the persona last used, Unlock (one prompt), and Reset behind
/// a confirmation. Buttons in the token accent (`MGStyle`), whatever the app's accent.
/// [FR-C-9] [RC-49]
public struct LockView: View {
    @ObservedObject var model: AppModel
    let persona: PersonaInfo
    @State private var confirmingReset = false

    public init(model: AppModel, persona: PersonaInfo) {
        self.model = model
        self.persona = persona
    }

    public var body: some View {
        VStack(spacing: MGSpace.s4) {
            Text("Locked").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
            Text(persona.display).font(MGFonts.sans(.body))
            Button { Task { await model.signIn(persona) } } label: { HitTarget("Unlock") }
                .accessibilityIdentifier("mg.0.unlock")
            Button(role: .destructive) { confirmingReset = true } label: { HitTarget("Reset this device") }
                .accessibilityIdentifier("mg.1.reset")
                .confirmationDialog("Reset this device?", isPresented: $confirmingReset) {
                    Button("Reset this device", role: .destructive) { Task { await model.resetDemo() } }
                } message: {
                    Text(ResetText.explanation)
                }
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
        .tint(MGStyle.accent)
    }
}

/// First launch: set up this device's key (no prompt).
public struct EnrolView: View {
    @ObservedObject var model: AppModel
    public init(model: AppModel) { self.model = model }

    public var body: some View {
        VStack(spacing: MGSpace.s4) {
            Text("Set up this device").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
            Text(model.custodyLabel.map { "Keys are protected by: \($0)" } ?? "")
                .font(MGFonts.sans(.footnote))
            Button { Task { await model.enrol() } } label: { HitTarget("Set up") }
                .accessibilityIdentifier("mg.0.enrol")
            MessageView(message: model.message)
        }
        .padding(MGSpace.s5)
        .tint(MGStyle.accent)
    }
}

/// The session's screens by phase, under the source banner (none over the sample
/// workspace) with, signed in, the status chips (the data chip first) and the
/// sample-workspace invitation; `content` is the signed-in
/// app (a console). Any touch postpones the idle lock: on iOS a touch anywhere in the
/// window (`IdleTouchWatcher`); on macOS the app watches mouse and key events itself.
/// Not a tap gesture over the screen: SwiftUI then reports every element under it as
/// tappable, and the hit-region audit failed the status chips. [RC-49]
public struct SessionRootView<Content: View>: View {
    @ObservedObject var model: AppModel
    let openYourData: () -> Void
    let content: (PersonaInfo) -> Content

    /// `openYourData` opens Your Data (the invitation's "Connect your data").
    public init(model: AppModel, openYourData: @escaping () -> Void,
                @ViewBuilder content: @escaping (PersonaInfo) -> Content) {
        self.model = model
        self.openYourData = openYourData
        self.content = content
    }

    #if os(iOS)
    @Environment(\.dynamicTypeSize) private var typeSize
    @State private var chromeOpen = false
    #endif

    /// On the iPhone at accessibility text sizes, signed in: the source and edition
    /// banners and the status chips fold into one header row (opened by a tap), so a
    /// console's list keeps room above the console bar (Phase D's Policies finding).
    private var foldsChrome: Bool {
        #if os(iOS)
        return typeSize.isAccessibilitySize && model.isUnlocked
        #else
        return false
        #endif
    }

    public var body: some View {
        VStack(spacing: 0) {
            #if os(iOS)
            if foldsChrome {
                FoldedChromeHeader(model: model, open: $chromeOpen)
                if chromeOpen {
                    if let banner = model.banner { SourceBannerView(banner) }
                }
            } else {
                if let banner = model.banner { SourceBannerView(banner) }
            }
            #else
            if let banner = model.banner { SourceBannerView(banner) }
            #endif
            switch model.phase {
            case .loading:
                LoadingView().frame(maxWidth: .infinity, maxHeight: .infinity)
            case .needsEnrolment:
                EnrolView(model: model)
            case .locked:
                if let persona = model.lastPersona {
                    LockView(model: model, persona: persona)
                } else {
                    PersonaPickerView(model: model)
                }
            case let .unlocked(persona):
                VStack(spacing: 0) {
                    if let chips = model.chipsPresentation, !foldsChrome || chromeOpenNow {
                        // On the iPhone a narrower band, so the consoles' lists keep the
                        // room they need above the console bar.
                        #if os(iOS)
                        SessionStatusChips(model: model, chips: chips)
                            .padding(.horizontal, MGSpace.s2).padding(.vertical, MGSpace.s1)
                        #else
                        SessionStatusChips(model: model, chips: chips).padding(MGSpace.s2)
                        #endif
                    }
                    if !foldsChrome || chromeOpenNow {
                        SampleWorkspaceInviteView(model: model, openYourData: openYourData)
                    }
                    MessageView(message: model.message)
                    content(persona)
                }
                .background { IdleTouchWatcher { model.touch() } }
            case let .failed(text):
                Text(text).font(MGFonts.sans(.body)).padding(MGSpace.s5).accessibilityIdentifier("mg.failed")
            }
        }
    }
}

extension SessionRootView {
    fileprivate var chromeOpenNow: Bool {
        #if os(iOS)
        return chromeOpen
        #else
        return true
        #endif
    }
}

#if os(iOS)
/// The source banner, status chips and invitation folded into one row at accessibility
/// text sizes: what the data is (the data chip's words, or the LIVE banner's short form),
/// and a button (44 pt) that shows or hides the rest. The full text is its spoken label.
/// [RC-49] [AC-C-11]
struct FoldedChromeHeader: View {
    @ObservedObject var model: AppModel
    @Binding var open: Bool

    var body: some View {
        Button {
            open.toggle()
        } label: {
            HStack(spacing: MGSpace.s2) {
                Text(foldedText)
                    .font(MGFonts.sans(.caption).weight(.semibold))
                    .lineLimit(2)
                    .minimumScaleFactor(0.7)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: MGSpace.s1)
                Image(systemName: open ? "chevron.up" : "chevron.down").accessibilityHidden(true)
            }
            .foregroundStyle(foreground)
            .padding(.horizontal, MGSpace.s3)
            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
            .background(background)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(model.banner?.text ?? model.sourceChip?.accessibilityLabel ?? "Midgard"))
        .accessibilityValue(Text(open ? "Details shown" : "Details hidden"))
        .accessibilityHint(Text(open ? "Hides the banners and status" : "Shows the banners and status"))
        .accessibilityIdentifier("mg.chrome.folded")
    }

    /// A banner (no source, or LIVE) keeps its warning colours; the sample workspace is
    /// lettered as its data chip, on the chip's tint.
    private var foldedText: String {
        model.banner?.compactText ?? model.sourceChip?.text ?? "Midgard"
    }

    private var foreground: Color {
        if model.banner != nil { return Color(mgHex: MGContrast.ink(on: MGToken.warn)) }
        let tone = model.sourceChip?.tone ?? .info
        return Color(mgLight: (tone.text.light, 1), dark: (tone.text.dark, 1))
    }

    private var background: Color {
        if model.banner != nil { return Color(mgHex: MGToken.warn) }
        let tone = model.sourceChip?.tone ?? .info
        return Color(mgHex: tone.tint.0, opacity: tone.tint.1)
    }
}
#endif

/// A button label at least 44 × 44 pt where it can be hit: the frame sits inside the
/// button, so it is the button's hit region (a `.frame` outside a button only pads it).
/// [RC-49] [AC-C-11]
struct HitTarget: View {
    let title: String
    init(_ title: String) { self.title = title }

    var body: some View {
        Text(title).frame(minWidth: 44, minHeight: 44).contentShape(Rectangle())
    }
}

/// What resetting this device does, for its confirmation (the lock screen and the Mac's
/// Settings).
public enum ResetText {
    public static let explanation =
        "Deletes this device's saved data and key. Signing in again starts the sample workspace from the beginning."
    public static let noSource =
        "Deletes the local store on this Mac and this device's key. You will set up this device again."
}

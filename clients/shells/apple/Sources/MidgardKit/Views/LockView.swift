import MidgardCore
import SwiftUI

/// The locked screen: the persona last used, Unlock (one prompt), and Reset demo behind
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
            Button(role: .destructive) { confirmingReset = true } label: { HitTarget("Reset demo") }
                .accessibilityIdentifier("mg.1.reset")
                .confirmationDialog("Reset the demo?", isPresented: $confirmingReset) {
                    Button("Reset demo", role: .destructive) { Task { await model.resetDemo() } }
                } message: {
                    Text("Deletes this device's saved data and key. Signing in again starts the demo from the beginning.")
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

/// The session's screens by phase, under the demo banner; `content` is the signed-in
/// app (a console). Any touch postpones the idle lock: on iOS a touch anywhere in the
/// window (`IdleTouchWatcher`); on macOS the app watches mouse and key events itself.
/// Not a tap gesture over the screen: SwiftUI then reports every element under it as
/// tappable, and the hit-region audit failed the status chips. [RC-49]
public struct SessionRootView<Content: View>: View {
    @ObservedObject var model: AppModel
    let content: (PersonaInfo) -> Content

    public init(model: AppModel, @ViewBuilder content: @escaping (PersonaInfo) -> Content) {
        self.model = model
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
                    if let banner = model.banner { DemoBannerView(banner) }
                    if let banner = model.editionBanner { banner }
                }
            } else {
                if let banner = model.banner { DemoBannerView(banner) }
                if let banner = model.editionBanner { banner }
            }
            #else
            if let banner = model.banner { DemoBannerView(banner) }
            if let banner = model.editionBanner { banner }
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
                        StatusChipsView(chips).padding(.horizontal, MGSpace.s2).padding(.vertical, MGSpace.s1)
                        #else
                        StatusChipsView(chips).padding(MGSpace.s2)
                        #endif
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
/// The source banner, edition and status chips folded into one row at accessibility
/// text sizes: what the source is, in its short form, and a button (44 pt) that shows
/// or hides the rest. The full notice is its spoken label. [RC-49] [AC-C-11]
struct FoldedChromeHeader: View {
    @ObservedObject var model: AppModel
    @Binding var open: Bool

    var body: some View {
        Button {
            open.toggle()
        } label: {
            HStack(spacing: MGSpace.s2) {
                Text(model.banner?.compactText ?? "Midgard")
                    .font(MGFonts.sans(.caption).weight(.semibold))
                    .lineLimit(2)
                    .minimumScaleFactor(0.7)
                    .multilineTextAlignment(.leading)
                Spacer(minLength: MGSpace.s1)
                Image(systemName: open ? "chevron.up" : "chevron.down").accessibilityHidden(true)
            }
            .foregroundStyle(Color(mgHex: MGContrast.ink(on: MGToken.warn)))
            .padding(.horizontal, MGSpace.s3)
            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
            .background(Color(mgHex: MGToken.warn))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(model.banner?.text ?? "Midgard"))
        .accessibilityValue(Text(open ? "Details shown" : "Details hidden"))
        .accessibilityHint(Text(open ? "Hides the banners and status" : "Shows the banners and status"))
        .accessibilityIdentifier("mg.chrome.folded")
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

import MidgardCore
import SwiftUI

/// Sign in as one of the demo personas. Signing in prompts for Face ID / Touch ID (or
/// the passcode) through the key root. [FR-C-9]
///
/// Accessibility [RC-49] [AC-C-11]: colours from the design tokens (`MGStyle`), not the
/// system's secondary grey or the app's accent, so the screen meets AA in every app and
/// mode; each persona is a bordered button (keyboard-focusable wherever the platform
/// lets buttons take focus: iOS with Full Keyboard Access, macOS with Keyboard
/// navigation on) at least 44 pt tall; at accessibility text sizes its clearance goes
/// under the name instead of beside it, where it was cut off.
public struct PersonaPickerView: View {
    @ObservedObject var model: AppModel
    @Environment(\.dynamicTypeSize) private var typeSize
    public init(model: AppModel) { self.model = model }

    public var body: some View {
        // Four personas at accessibility text sizes are taller than the iPhone's screen:
        // always a scroll view, which bounces only when the list does not fit. A plain
        // VStack inside it, not a ViewThatFits choosing between the list and a scroll
        // view of a LazyVStack: the iOS Dynamic Type audit reported every text that
        // version showed as clipped.
        ScrollView {
            VStack(alignment: .leading, spacing: MGSpace.s4) {
                Text("Sign in").font(MGFonts.display(.title)).accessibilityAddTraits(.isHeader)
                Text("Demo personas").font(MGFonts.mono(.caption)).foregroundStyle(MGStyle.secondaryText)
                ForEach(Array(model.personas.enumerated()), id: \.element.id) { index, persona in
                    Button {
                        Task { await model.signIn(persona) }
                    } label: {
                        PersonaLabel(persona: persona, stacked: typeSize.isAccessibilitySize)
                    }
                    .buttonStyle(.bordered)
                    // A rounded rectangle: the default capsule cut into the stacked lines at
                    // large text sizes.
                    .buttonBorderShape(.roundedRectangle)
                    .accessibilityLabel(Text("\(persona.display), \(persona.role), clearance \(persona.clearanceLabel)"))
                    .accessibilityIdentifier("mg.\(index).persona.\(persona.id)")
                }
                MessageView(message: model.message)
            }
            .padding(MGSpace.s5)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .scrollBounceBehavior(.basedOnSize)
        .tint(MGStyle.accent)
    }
}

/// A persona button's label: name and role, and the clearance beside them (or under
/// them when `stacked`). Every line in the token text colour (`MGStyle.text`).
/// `AccessibilityPrecheckTests` checks that colour on the page backgrounds and the
/// theme's surfaces, not on the bordered button's fill, which the system composites
/// (a translucent fill over the page) and which has no token to check against: the
/// fill's contrast is measured by each app's contrast audit.
struct PersonaLabel: View {
    let persona: PersonaInfo
    let stacked: Bool

    var body: some View {
        let layout = stacked
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: MGSpace.s1))
            : AnyLayout(HStackLayout(alignment: .center, spacing: MGSpace.s3))
        layout {
            VStack(alignment: .leading) {
                Text(persona.display).font(MGFonts.sans(.body))
                Text(persona.role.uppercased()).font(MGFonts.mono(.caption))
            }
            if !stacked { Spacer(minLength: 0) }
            // Caption, not caption2: the iOS Dynamic Type audit reported the caption2
            // clearance as not following the text size ("partially unsupported").
            Text(persona.clearanceLabel).font(MGFonts.mono(.caption))
        }
        .multilineTextAlignment(.leading)
        .foregroundStyle(MGStyle.text)
        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
        .contentShape(Rectangle())
    }
}

/// The model's latest message, if any.
struct MessageView: View {
    let message: String?
    var body: some View {
        if let message {
            Text(message).font(MGFonts.sans(.footnote)).accessibilityIdentifier("mg.message")
        }
    }
}

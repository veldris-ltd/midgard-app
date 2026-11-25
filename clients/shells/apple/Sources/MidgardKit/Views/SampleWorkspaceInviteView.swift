import SwiftUI

/// The invitation over the sample workspace (owner decision 25 Nov 2026): what the user is
/// looking at, and the way to their own data, in the consoles' own card style rather than
/// a warning. "Connect your data" opens Your Data (the Mac's window, ⇧⌘D; the iPhone's
/// sheet, as the session menu's "Your data"); the close button dismisses it for good
/// (`AppModel.dismissSampleInvite`). It goes by itself once the user's own data is there.
/// Hit targets are 44 pt at least; the text scales with Dynamic Type. [AC-C-13] [RC-49]
public struct SampleWorkspaceInviteView: View {
    @ObservedObject var model: AppModel
    @ObservedObject var data: UserDataModel
    let openYourData: () -> Void
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(model: AppModel, openYourData: @escaping () -> Void) {
        self.model = model
        data = model.userData
        self.openYourData = openYourData
    }

    public var body: some View {
        if model.showsSampleInvite {
            card
                .padding(.horizontal, MGSpace.s2)
                .padding(.bottom, MGSpace.s2)
        }
    }

    private var card: some View {
        ViewThatFits(in: .horizontal) {
            if !typeSize.isAccessibilitySize {
                HStack(spacing: MGSpace.s3) {
                    icon
                    message.lineLimit(1)
                    Spacer(minLength: MGSpace.s2)
                    connect(inline: true)
                    dismiss
                }
            }
            // Narrow: the words, and the action under them; the close button at the top.
            HStack(alignment: .top, spacing: MGSpace.s3) {
                icon.padding(.top, MGSpace.s3)
                VStack(alignment: .leading, spacing: 0) {
                    message.padding(.top, MGSpace.s3)
                    connect(inline: false)
                }
                Spacer(minLength: 0)
                dismiss
            }
        }
        .padding(.leading, MGSpace.s3)
        .padding(.trailing, MGSpace.s1)
        .padding(.vertical, MGSpace.s1)
        .background(RoundedRectangle(cornerRadius: 10, style: .continuous).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: 10, style: .continuous).strokeBorder(MGWeb.border, lineWidth: 1))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Sample workspace"))
        .accessibilityIdentifier("mg.invite")
    }

    private var icon: some View {
        Image(systemName: "square.stack.3d.up").accessibilityHidden(true)
            .font(MGFonts.sans(.body))
            .foregroundStyle(MGWeb.infoText)
    }

    private var message: some View {
        Text(SampleWorkspaceInvite.text)
            .font(MGFonts.sans(.subheadline))
            .foregroundStyle(MGStyle.text)
            .multilineTextAlignment(.leading)
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.invite.text")
    }

    private func connect(inline: Bool) -> some View {
        Button(action: openYourData) {
            Text(SampleWorkspaceInvite.action)
                .font(MGFonts.sans(.subheadline).weight(.semibold))
                .padding(.horizontal, inline ? MGSpace.s3 : 0)
                .frame(minHeight: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .foregroundStyle(MGStyle.accent)
        .accessibilityHint(Text("Opens Your Data, to connect a data server or import JSON files"))
        .accessibilityIdentifier("mg.invite.connect")
    }

    private var dismiss: some View {
        Button {
            model.dismissSampleInvite()
        } label: {
            Image(systemName: "xmark").accessibilityHidden(true)
                .font(MGFonts.sans(.footnote).weight(.semibold))
                .foregroundStyle(MGStyle.secondaryText)
                .frame(minWidth: 44, minHeight: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("Dismiss"))
        .accessibilityHint(Text("Hides this invitation. The data chip still names the sample workspace."))
        .accessibilityIdentifier("mg.invite.dismiss")
    }
}

/// The status chips with the data chip first, kept current as the user's data changes
/// (imports and the gateway live in `UserDataModel`). [AC-C-13]
struct SessionStatusChips: View {
    @ObservedObject var model: AppModel
    @ObservedObject var data: UserDataModel
    let chips: StatusChipsPresentation

    init(model: AppModel, chips: StatusChipsPresentation) {
        self.model = model
        data = model.userData
        self.chips = chips
    }

    var body: some View {
        StatusChipsView(chips, source: model.sourceChip)
    }
}

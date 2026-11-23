import MidgardCore
import SwiftUI

/// The web console's frame (`ConsoleShell`): the classification banner, the header, then
/// the navigation, the main area and the inspector side by side, with 1 pt rules
/// between them. The Mac app fills it; the navigation it passes holds the app's own
/// routes. [DD-7]
public struct HeimdallConsoleFrame<Nav: View, Main: View, Aside: View, Trailing: View>: View {
    let nav: Nav
    let main: Main
    let aside: Aside
    let trailing: Trailing
    let asideWidth: CGFloat

    public init(asideWidth: CGFloat = 360, @ViewBuilder nav: () -> Nav, @ViewBuilder main: () -> Main,
                @ViewBuilder aside: () -> Aside, @ViewBuilder trailing: () -> Trailing) {
        self.nav = nav()
        self.main = main()
        self.aside = aside()
        self.trailing = trailing()
        self.asideWidth = asideWidth
    }

    public var body: some View {
        VStack(spacing: 0) {
            WebClassificationBanner(level: HeimdallChrome.banner.level, text: HeimdallChrome.banner.text)
            HeimdallHeader { trailing }
            HStack(spacing: 0) {
                nav
                    .frame(width: 232)
                    .accessibilityElement(children: .contain)
                    .accessibilityLabel(Text("Navigation"))
                Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true)
                main.frame(maxWidth: .infinity, maxHeight: .infinity)
                Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true)
                ScrollView { aside }
                    .frame(width: asideWidth)
                    .background(MGWeb.bgSunken)
            }
            .frame(maxHeight: .infinity)
        }
        .background(MGWeb.bgPage)
    }
}

/// The Operating Picture on the iPhone: the classification banner and the header's
/// first seal, then the picture's readouts as a strip and the picture, the stats and the
/// layer switches under it; the inspector opens as a sheet when a track is chosen (or
/// from the inspect button). [FR-H-4] [FR-H-5] [DD-7] [RC-49] [AC-C-11]
public struct HeimdallOperatingPictureCompact: View {
    @StateObject private var model: HeimdallPictureModel
    @ObservedObject private var app: AppModel
    @State private var inspecting = false
    @Environment(\.dynamicTypeSize) private var typeSize
    let personaId: String
    /// The view chosen in the navigation bar's switch: the reference layout, or one an
    /// edition library adds (MAP, 3D), shown in the picture's place.
    @Binding var view: String

    public init(app: AppModel, personaId: String, view: Binding<String> = .constant(HeimdallPictureView.reference.id)) {
        _model = StateObject(wrappedValue: HeimdallPictureModel(app: app))
        self.app = app
        self.personaId = personaId
        self._view = view
    }

    /// The edition's view chosen in the switch, when it is not the reference layout.
    private var chosen: (HeimdallPictureViewsProvider, HeimdallPictureView)? {
        guard view != HeimdallPictureView.reference.id, let provider = app.pictureViews,
              let v = provider.views.first(where: { $0.id == view }) else { return nil }
        return (provider, v)
    }

    public var body: some View {
        Group {
            if let pair = chosen {
                let (provider, v) = pair
                VStack(spacing: 0) {
                    // The page colour runs up under the navigation bar, as on the reference
                    // layout; the banner's own colour would otherwise extend up there.
                    Rectangle().fill(MGWeb.bgPage).frame(height: 1).accessibilityHidden(true)
                    WebClassificationBanner(level: HeimdallChrome.banner.level, text: HeimdallChrome.banner.text, compact: true)
                    provider.makeView(v, picture: model,
                                      context: HeimdallPictureViewContext(offline: app.isOffline, compact: true) { _ in
                                          inspecting = true
                                      })
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
                .background(MGWeb.bgPage)
            } else {
                reference
            }
        }
        .sheet(isPresented: $inspecting) {
            ScrollView { HeimdallInspector(model: model) }
                .background(MGWeb.bgSunken)
                .presentationDetents([.medium, .large])
                .presentationDragIndicator(.visible)
                .accessibilityIdentifier("mg.heimdall.inspector")
        }
        .task(id: personaId) { await model.load() }
        // The user's feeds are read only while the picture is open.
        .task { await model.followUserData() }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.content.heimdall.operating-picture")
    }

    private var reference: some View {
        ScrollView {
            VStack(spacing: 0) {
                WebClassificationBanner(level: HeimdallChrome.banner.level, text: HeimdallChrome.banner.text, compact: true)
                HeimdallHeader(compact: true) { EmptyView() }
                // The readouts as a strip directly above the picture, not over it, at
                // an iPhone's width (except at accessibility sizes, where the picture's
                // label says them).
                PictureStripLayout {
                    if !typeSize.isAccessibilitySize { HeimdallPictureReadoutStrip(model: model) }
                    HeimdallPictureCanvas(model: model, showsFooter: false, readoutsInPicture: false) { _ in
                        inspecting = true
                    }
                }
                VStack(alignment: .leading, spacing: 14) {
                    if typeSize.isAccessibilitySize {
                        HeimdallTrackList(model: model) { _ in inspecting = true }
                    }
                    HeimdallStatsFooter()
                    if let t = model.selectedTrack {
                        Button {
                            inspecting = true
                        } label: {
                            HStack(spacing: 8) {
                                Image(systemName: "sidebar.right").accessibilityHidden(true)
                                Text("Inspect \(t.id)")
                                    .font(MGWebFont.sans(14).weight(.semibold))
                            }
                            .foregroundStyle(MGWeb.textHeading)
                            .padding(.horizontal, 14)
                            .frame(maxWidth: .infinity, minHeight: 44)
                            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceCard))
                            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.borderStrong))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityIdentifier("mg.heimdall.inspect")
                    }
                    HeimdallLayersList(model: model)
                }
                .padding(12)
            }
        }
        .background(MGWeb.bgPage)
    }
}

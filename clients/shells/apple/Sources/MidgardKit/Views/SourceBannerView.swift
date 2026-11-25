import SwiftUI

/// The source banner, above everything, only where there is something to warn of: "No
/// data source connected" (owner decision 19 Nov 2026), or the LIVE banner while a
/// gateway is connected, which says that only Midgard's screens read it. The sample
/// workspace has no banner (owner decision 25 Nov 2026): the status row's data chip names
/// it. [AC-C-13] [FR-C-5]
public struct SourceBannerView: View {
    let banner: SourceBanner
    @Environment(\.dynamicTypeSize) private var typeSize
    public init(_ banner: SourceBanner) { self.banner = banner }

    /// On the Mac the banner stops at the title bar: drawn on under it, its element took
    /// the title bar in, and the light-mode contrast audit measured the notice against
    /// the bar and its window buttons. The iPhone's runs under the status bar.
    #if os(macOS)
    static let underBars: Edge.Set = []
    #else
    static let underBars: Edge.Set = .all
    #endif

    public var body: some View {
        // At accessibility text sizes the short form; the full notice is always the
        // spoken label.
        Text(typeSize.isAccessibilitySize ? banner.compactText : banner.text)
            .font(MGFonts.sans(.caption).weight(.semibold))
            .multilineTextAlignment(.center)
            .foregroundStyle(Color(mgHex: MGContrast.ink(on: MGToken.warn)))
            .padding(.horizontal, MGSpace.s4)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity)
            .background(Color(mgHex: MGToken.warn), ignoresSafeAreaEdges: Self.underBars)
            .accessibilityLabel(Text(banner.text))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier(banner.identifier)
    }
}

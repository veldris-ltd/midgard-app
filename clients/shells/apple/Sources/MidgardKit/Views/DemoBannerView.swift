import SwiftUI

/// The source banner, above everything: the demonstration notice, as in the web
/// consoles' mock mode (owner decision 10), or "No data source connected" (owner
/// decision 19 Nov 2026). [AC-C-13] [FR-C-5]
public struct DemoBannerView: View {
    let banner: SourceBanner
    public init(_ banner: SourceBanner = .demo) { self.banner = banner }

    public var body: some View {
        Text(banner.text)
            .font(MGFonts.sans(.caption).weight(.semibold))
            .multilineTextAlignment(.center)
            .foregroundStyle(Color(mgHex: MGContrast.ink(on: MGToken.warn)))
            .padding(.horizontal, MGSpace.s4)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity)
            .background(Color(mgHex: MGToken.warn))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier(banner.identifier)
    }
}

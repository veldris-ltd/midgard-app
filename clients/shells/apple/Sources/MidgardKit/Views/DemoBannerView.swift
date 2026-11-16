import SwiftUI

/// The demonstration notice, above everything, as in the web consoles' mock mode
/// (owner decision 10). [AC-C-13]
public struct DemoBannerView: View {
    let text: String
    public init(_ text: String = DemoBanner.notice) { self.text = text }

    public var body: some View {
        Text(text)
            .font(MGFonts.sans(.caption).weight(.semibold))
            .multilineTextAlignment(.center)
            .foregroundStyle(Color(mgHex: MGToken.ink950))
            .padding(.horizontal, MGSpace.s4)
            .padding(.vertical, 6)
            .frame(maxWidth: .infinity)
            .background(Color(mgHex: MGToken.warn))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.banner.demo")
    }
}

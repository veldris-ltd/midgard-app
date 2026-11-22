import SwiftUI

/// A 3D edition's About screen: the edition, and that it is not the signed release.
/// The release edition has none. [FR-C-1] [AC-C-13]
public struct AboutView: View {
    let about: EditionAbout

    public init(_ about: EditionAbout) { self.about = about }

    public var body: some View {
        VStack(alignment: .leading, spacing: MGSpace.s3) {
            Text(about.name).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("mg.about.name")
            Text(about.editionLine).font(MGFonts.sans(.body))
                .accessibilityIdentifier("mg.about.edition")
            Text(about.releaseLine).font(MGFonts.sans(.body))
                .textSelection(.enabled)
                .accessibilityIdentifier("mg.about.release")
        }
        .padding(MGSpace.s5)
        .frame(maxWidth: .infinity, alignment: .leading)
        .navigationTitle("About")
    }
}

/// A 3D edition's banner, under the source banner, styled as it is.
public struct EditionBannerView: View {
    let banner: EditionBanner
    public init(_ banner: EditionBanner) { self.banner = banner }

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

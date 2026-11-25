import MidgardKit
import SwiftUI

/// An edition's About screen: the edition, its version and what it does.
/// [FR-C-1] [AC-C-13]
public struct AboutView: View {
    let about: EditionAbout

    public init(_ about: EditionAbout) { self.about = about }

    /// "Version 0.1.0 (1)", from the app's Info.plist.
    static var versionLine: String {
        let info = Bundle.main.infoDictionary ?? [:]
        let short = info["CFBundleShortVersionString"] as? String ?? "?"
        let build = info["CFBundleVersion"] as? String ?? "?"
        return "Version \(short) (\(build))"
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: MGSpace.s3) {
            Text(about.name).font(MGFonts.display(.title2)).accessibilityAddTraits(.isHeader)
                .accessibilityIdentifier("mg.about.name")
            Text(about.editionLine).font(MGFonts.sans(.body))
                .accessibilityIdentifier("mg.about.edition")
            Text(about.releaseLine).font(MGFonts.sans(.body))
                .textSelection(.enabled)
                .accessibilityIdentifier("mg.about.release")
            Text(Self.versionLine).font(MGFonts.sans(.body))
                .foregroundStyle(MGStyle.secondaryText)
                .textSelection(.enabled)
                .accessibilityIdentifier("mg.about.version")
        }
        .padding(MGSpace.s5)
        .frame(maxWidth: .infinity, alignment: .leading)
        .navigationTitle("About")
    }
}

import SwiftUI

/// Settings → Acknowledgements: the third-party notices (UniFFI's MPL-2.0 line, the
/// fonts' OFL-1.1 lines). [DD-6] [DD-7]
public struct AcknowledgementsView: View {
    public init() {}

    public var body: some View {
        List(Acknowledgements.entries) { entry in
            Text(entry.text)
                .font(MGFonts.sans(.footnote))
                .textSelection(.enabled)
                .accessibilityIdentifier("mg.ack.\(entry.id)")
        }
        .navigationTitle("Acknowledgements")
    }
}

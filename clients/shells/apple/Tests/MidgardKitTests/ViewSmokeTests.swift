import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// Every shared view renders with real data from the core (macOS host; the iOS slices
/// are typechecked by tools/typecheck-ios.sh). [AC-C-13] [DD-7]
@MainActor
final class ViewSmokeTests: XCTestCase {
    private func renders<V: View>(_ view: V, file: StaticString = #filePath, line: UInt = #line) {
        let renderer = ImageRenderer(content: view.frame(width: 390, height: 600))
        XCTAssertNotNil(renderer.cgImage, file: file, line: line)
    }

    func testSharedViewsRender() throws {
        MGFonts.registerAll()
        let client = try MidgardClient(config: makeConfig(), keyRoot: TestKeyRoot())
        _ = try client.unlock(keys: try client.enrol(), personaId: "k.rao")
        let rows = try client.collectionRows(collection: "midgard.objectTypes")
            + client.collectionRows(collection: "midgard.audit")
        XCTAssertTrue(rows.contains(where: \.withheld))
        renders(VStack { ForEach(Array(rows.enumerated()), id: \.offset) { ConsoleRowView($0.element) } })
        renders(StatusChipsView(StatusChipsPresentation(try client.statusChips())))
        renders(SignatureBadgeView(SignatureBadge(provenance: "verified")))
        renders(DemoBannerView())
        renders(DemoBannerView(.noSource))
        renders(NavigationStack { AcknowledgementsView() })

        let model = AppModel()
        renders(SessionRootView(model: model) { _ in Text("console") })
        renders(PersonaPickerView(model: model))
        renders(EnrolView(model: model))
        renders(LockView(model: model, persona: try client.sessionPersona()))
    }
}

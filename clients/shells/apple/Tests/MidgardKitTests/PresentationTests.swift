import Foundation
import MidgardCore
import XCTest

@testable import MidgardKit

/// What the screens say: chips, withheld rows, signature status, the demo banner, the
/// acknowledgements and error messages. Chip and row inputs come from the real core.
/// [AC-C-13] [FR-C-9] [DD-7]
final class PresentationTests: XCTestCase {
    private func signedIn(_ persona: String = "k.rao") throws -> MidgardClient {
        let client = try MidgardClient(config: makeConfig(), keyRoot: TestKeyRoot())
        _ = try client.unlock(keys: try client.enrol(), personaId: persona)
        return client
    }

    // MARK: chips

    func testOnlineChips() throws {
        let chips = StatusChipsPresentation(try signedIn().statusChips())
        XCTAssertEqual(chips.sync.text, "SYNCED")
        XCTAssertEqual(chips.sync.accessibilityLabel, "Sync: synced")
        XCTAssertEqual(chips.tier.text, "REMOTE")
        XCTAssertEqual(chips.custody.text, "SOFTWARE KEY")
        XCTAssertEqual(chips.custody.accessibilityLabel, "Key custody: software key, NonExtractable")
        XCTAssertEqual(chips.custody.tone, .warn)
    }

    func testOfflineIsAIOff() throws {
        let client = try signedIn()
        let target = try XCTUnwrap(try client.collectionRows(collection: "midgard.objectTypes").first { !$0.withheld }?.id)
        _ = try client.setOffline(offline: true)
        _ = try client.requestAction(action: ActionInput(verb: "demo.ping", target: "midgard.objectTypes/\(target)", params: []))
        let chips = StatusChipsPresentation(try client.statusChips())
        XCTAssertEqual(chips.tier.text, "AI OFF")
        XCTAssertEqual(chips.tier.accessibilityLabel, "AI: off (AI off)")
        XCTAssertEqual(chips.sync.text, "OFFLINE · 4 QUEUED")
        XCTAssertEqual(chips.sync.accessibilityLabel, "Sync: offline, 4 changes queued")
        XCTAssertEqual(chips.sync.tone, .warn)
    }

    func testEverySyncPhaseAndTierHasText() {
        let custody = CustodyChip(floor: "NonExtractable", hardwareWrapped: true)
        let tier = TierChip(tier: .hybrid, reason: "test")
        for (phase, text) in [(SyncPhase.synced, "SYNCED"), (.syncing, "SYNCING · 2"), (.offline, "OFFLINE · 2 QUEUED"),
                              (.conflict, "CONFLICT")] {
            let chips = StatusChipsPresentation(StatusChips(sync: SyncChip(phase: phase, pending: 2, lastSync: nil),
                                                            tier: tier, custody: custody))
            XCTAssertEqual(chips.sync.text, text)
            XCTAssertEqual(chips.custody.text, "SECURE ENCLAVE")
            XCTAssertEqual(chips.custody.tone, .secure)
        }
        for (t, text) in [(InferenceTier.onDevice, "ON-DEVICE"), (.remote, "REMOTE"), (.hybrid, "HYBRID"), (.disabled, "AI OFF")] {
            XCTAssertEqual(StatusChipsPresentation.tierText(t), text)
        }
    }

    // MARK: withheld rows

    func testWithheldRowsShowTheWordAndTheLevelOnly() throws {
        let rows = try signedIn().collectionRows(collection: "midgard.objectTypes")
        let withheld = rows.filter(\.withheld)
        XCTAssertFalse(withheld.isEmpty)
        for row in withheld {
            let shown = RowPresentation(row)
            XCTAssertTrue(shown.isWithheld)
            XCTAssertEqual(shown.title, "Withheld")
            XCTAssertEqual(shown.detail, "above your clearance")
            XCTAssertEqual(shown.marking.label, row.marking.label)
            XCTAssertEqual(shown.accessibilityLabel, "Withheld row, \(row.marking.label)")
            XCTAssertNil(shown.signature)
        }
        let visible = try XCTUnwrap(rows.first { !$0.withheld })
        XCTAssertFalse(RowPresentation(visible).isWithheld)
        XCTAssertEqual(RowPresentation(visible).title, visible.id)
    }

    // MARK: signature status

    func testSignatureStatusRendering() {
        let verified = SignatureBadge(.verified)
        XCTAssertEqual(verified.text, "VERIFIED")
        XCTAssertTrue(verified.showsCheck)
        let device = SignatureBadge(.device)
        XCTAssertEqual(device.text, "DEVICE-SIGNED")
        XCTAssertTrue(device.showsCheck)
        let none = SignatureBadge(.unsigned)
        XCTAssertEqual(none.text, "UNSIGNED")
        XCTAssertFalse(none.showsCheck)
    }

    func testMockClaimsAreNotCheckedAndNeverACheckMark() {
        let claim = SignatureBadge(claim: "not checked (demo)")
        XCTAssertEqual(claim.text, "NOT CHECKED (DEMO)")
        XCTAssertFalse(claim.showsCheck)
        // Any check mark left in free text is removed: only Verified/Device get one.
        for text in ["ML-DSA \u{2713}", "\u{2714} signed", "Verified", "ML-DSA-65"] {
            let badge = SignatureBadge(claim: text)
            XCTAssertFalse(badge.showsCheck)
            XCTAssertFalse(badge.text.contains("\u{2713}") || badge.text.contains("\u{2714}"), badge.text)
        }
    }

    func testMimirProvenanceVerifiedCarriesADemoQualifier() {
        let prov = SignatureBadge(provenance: "verified")
        XCTAssertEqual(prov.text, "verified (demo label)")
        XCTAssertFalse(prov.showsCheck)
        XCTAssertEqual(prov.accessibilityLabel, "Provenance: verified, a demo label, not a checked signature")
    }

    func testAuditRowsFromTheCoreRenderTheirStatus() throws {
        let rows = try signedIn().collectionRows(collection: "midgard.audit")
        let badges = rows.compactMap { RowPresentation($0).signature }
        XCTAssertFalse(badges.isEmpty)
        XCTAssertTrue(badges.allSatisfy { $0.showsCheck == ($0.status == .verified || $0.status == .device) })
    }

    func testOnlyVerifiedAndDeviceEverShowACheck() {
        let all = [SignatureStatus.verified, .device, .unsigned].map { SignatureBadge($0) }
            + ["not checked (demo)", "ok", "\u{2713}"].map { SignatureBadge(claim: $0) }
            + ["verified", "user"].map { SignatureBadge(provenance: $0) }
        for badge in all where badge.showsCheck {
            XCTAssertTrue(badge.status == .verified || badge.status == .device, badge.text)
        }
    }

    // MARK: banner, acknowledgements, errors

    func testDemoBannerMatchesTheWebConsoles() {
        XCTAssertEqual(DemoBanner.notice,
                       "Demonstration data. This console runs on built-in sample data and is not connected to an enclosure.")
    }

    func testAcknowledgementsNameUniFFI() {
        XCTAssertTrue(Acknowledgements.entries.contains {
            $0.text == "UniFFI 0.28.3, Mozilla Public License 2.0, source: https://github.com/mozilla/uniffi-rs/tree/v0.28.3"
        })
        for font in ["Oxanium", "Space Grotesk", "JetBrains Mono"] {
            XCTAssertTrue(Acknowledgements.entries.contains { $0.text.hasPrefix(font) && $0.text.contains("SIL Open Font License 1.1") })
        }
    }

    func testErrorsAreScreenTextWithoutPlatformDetail() {
        XCTAssertEqual(ErrorText.describe(ClientError.Cancelled), "Unlock cancelled.")
        XCTAssertEqual(ErrorText.describe(ClientError.PendingOps(count: 4)),
                       "Sync 4 queued changes before switching persona.")
        XCTAssertEqual(ErrorText.describe(ClientError.Offline),
                       "Offline. Changes are queued and will sync when you reconnect.")
        let detail = ErrorText.describe(ClientError.KeyRootUnavailable(detail: "NSOSStatusErrorDomain -34018"))
        XCTAssertFalse(detail.contains("-34018"), "platform details stay out of the screen text")
        XCTAssertEqual(ErrorText.describe(ClientError.UnsealFailed),
                       "This device's key could not open the saved keys. Reset the demo to start again.")
    }
}

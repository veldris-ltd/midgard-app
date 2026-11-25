import Foundation
import MidgardCore
import XCTest

@testable import MidgardKit

/// What the screens say: chips, withheld rows, signature status, the data chip and invitation, the
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
        let claim = SignatureBadge(claim: "not checked (sample)")
        XCTAssertEqual(claim.text, "NOT CHECKED (SAMPLE)")
        XCTAssertFalse(claim.showsCheck)
        // Any check mark left in free text is removed: only Verified/Device get one.
        let marks: Set<UInt32> = [0x2713, 0x2714, 0x2705, 0x2611, 0x1F5F8, 0x1F5F9, 0xFE0F]
        for text in ["ML-DSA \u{2713}", "\u{2714} signed", "Verified", "ML-DSA-65", "ML-DSA \u{2714}\u{FE0F}",
                     "\u{2611}\u{FE0F} signed", "\u{1F5F8} ok", "\u{1F5F9} ok", "\u{2705} done"] {
            for badge in [SignatureBadge(claim: text), SignatureBadge(provenance: text)] {
                XCTAssertFalse(badge.showsCheck)
                XCTAssertFalse(badge.text.unicodeScalars.contains { marks.contains($0.value) }, badge.text)
                XCTAssertFalse(badge.text.isEmpty, text)
            }
        }
    }

    func testMimirProvenanceVerifiedCarriesASampleQualifier() {
        let prov = SignatureBadge(provenance: "verified")
        XCTAssertEqual(prov.text, "verified (sample label)")
        XCTAssertFalse(prov.showsCheck)
        XCTAssertEqual(prov.accessibilityLabel, "Provenance: verified, a sample label, not a checked signature")
    }

    func testAuditRowsFromTheCoreRenderTheirStatus() throws {
        let rows = try signedIn().collectionRows(collection: "midgard.audit")
        let badges = rows.compactMap { RowPresentation($0).signature }
        XCTAssertFalse(badges.isEmpty)
        XCTAssertTrue(badges.allSatisfy { $0.showsCheck == ($0.status == .verified || $0.status == .device) })
    }

    func testOnlyVerifiedAndDeviceEverShowACheck() {
        let all = [SignatureStatus.verified, .device, .unsigned].map { SignatureBadge($0) }
            + ["not checked (sample)", "ok", "\u{2713}"].map { SignatureBadge(claim: $0) }
            + ["verified", "user"].map { SignatureBadge(provenance: $0) }
        for badge in all where badge.showsCheck {
            XCTAssertTrue(badge.status == .verified || badge.status == .device, badge.text)
        }
    }

    // MARK: banner, acknowledgements, errors

    func testTheSampleWorkspaceHasAChipAndAnInvitationNotABanner() {
        XCTAssertEqual(SampleWorkspaceInvite.text,
                       "Exploring the sample workspace. Connect your own data server or import JSON to see your data.")
        let sample = try! XCTUnwrap(DataSourceChip.make(kind: .sample, liveHost: nil, imported: false))
        XCTAssertEqual(sample.text, "SAMPLE WORKSPACE")
        XCTAssertEqual(sample.tone, .info)
        XCTAssertEqual(DataSourceChip.make(kind: .sample, liveHost: nil, imported: true)?.text, "SAMPLE WORKSPACE + IMPORTED")
        let live = try! XCTUnwrap(DataSourceChip.make(kind: .sample, liveHost: "gw.example", imported: true))
        XCTAssertEqual(live.text, "LIVE · gw.example")
        XCTAssertEqual(DataSourceChip.make(kind: DataSourceKind.none, liveHost: nil, imported: false)?.text, "NO DATA SOURCE")
        XCTAssertNil(DataSourceChip.make(kind: nil, liveHost: nil, imported: false))
        // The sample workspace is never called live, and no user-facing text calls the
        // app a demonstration, a trial or a beta.
        for chip in [sample, DataSourceChip.make(kind: .sample, liveHost: nil, imported: true)!] {
            XCTAssertFalse(chip.text.uppercased().contains("LIVE"), chip.text)
            XCTAssertFalse(chip.accessibilityLabel.lowercased().contains("live"), chip.accessibilityLabel)
        }
        let texts = [SampleWorkspaceInvite.text, SampleWorkspaceInvite.action, sample.text, sample.accessibilityLabel,
                     SourceBanner.live("h").text, SourceBanner.live("h").compactText, ResetText.explanation]
        for text in texts {
            for word in ["demo", "demonstration", "trial", "beta"] {
                XCTAssertNil(text.range(of: "\\b\(word)\\b", options: [.regularExpression, .caseInsensitive]), text)
            }
        }
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
                       "This device's key could not open the saved keys. Reset this device to start again.")
    }
}

import AppKit
import MidgardCore
import SwiftUI
import XCTest

@testable import MidgardKit

/// What `swift test` can check of the seven affordances before an app exists. These are
/// prechecks on the shared views, not the audit: the per-release record comes only from
/// the XCUITest run on a real runner (clients/shells/apple/audit.sh, AUDIT_SCHEMA.md).
///
/// - contrast_aa: measured from the tokens (WCAG 2.x ratio, AA 4.5:1 for text), for every
///   pair the shared views draw themselves — chip text on the tone's tint composited over
///   every surface, in both modes — and the theme's text tokens on its surfaces.
/// - hit_target_size: measured on macOS from the laid-out controls of the shared screens.
/// - reduced_motion: the loading view measured on macOS (no spinner when reduced), plus
///   source rules.
/// - dynamic_type, screen_reader_labels: source rules on MidgardKit (macOS has no Dynamic
///   Type, so scaling itself is measured only by the iOS audit). The rules read one line
///   or one file at a time: a modifier split across lines or files is not seen.
/// - focus_order, keyboard_navigation: not measurable here; the XCUITest audit only.
/// [RC-49] [AC-C-11] [DD-7]
@MainActor
final class AccessibilityPrecheckTests: XCTestCase {
    private let aa = MGContrast.aaText

    // MARK: contrast_aa

    func testContrastRatioFollowsWCAG() {
        XCTAssertEqual(MGContrast.ratio(0xFFFFFF, 0x000000), 21, accuracy: 0.001)
        XCTAssertEqual(MGContrast.ratio(0x3D8BFF, 0x3D8BFF), 1, accuracy: 0.001)
        XCTAssertEqual(MGContrast.ratio(0xFFFFFF, 0xCA1D25), 5.65, accuracy: 0.01)
        XCTAssertEqual(MGContrast.ratio(0xCA1D25, 0xFFFFFF), MGContrast.ratio(0xFFFFFF, 0xCA1D25), "symmetric")
    }

    func testMarkingChipsMeetAAInEveryLevel() {
        for level in [MarkingLevelChip.open, .internal, .restricted, .confidential] {
            let fill = MGToken.markingHex(level)
            XCTAssertGreaterThanOrEqual(MGContrast.ratio(MGContrast.ink(on: fill), fill), aa, "\(level)")
        }
        // The dark ink on the confidential red is 3.6:1, below AA: the chip uses white there.
        XCTAssertEqual(MGContrast.ink(on: MGToken.markingConfidential), MGToken.white)
        XCTAssertEqual(MGContrast.ink(on: MGToken.markingRestricted), MGToken.ink950)
    }

    private let tones: [ChipTone] = [.secure, .warn, .danger, .info]
    private let darkSurfaces = [MGToken.bgPage, MGToken.bgSunken, MGToken.surfaceCard, MGToken.surfaceRaised,
                                MGToken.surfaceInset]
    private let lightSurfaces = [MGToken.Light.bgPage.0, MGToken.Light.bgSunken.0, MGToken.Light.surfaceCard.0,
                                 MGToken.Light.surfaceRaised.0, MGToken.Light.surfaceInset.0]

    /// The lowest contrast of `text` on the tone's tint composited over each surface.
    private func worst(_ text: UInt32, on tone: ChipTone, over surfaces: [UInt32]) -> Double {
        surfaces.map { MGContrast.ratio(text, MGContrast.over(tone.tint, $0)) }.min() ?? 0
    }

    func testCompositingFollowsSourceOver() {
        XCTAssertEqual(MGContrast.over((0xFFFFFF, 1), 0x000000), 0xFFFFFF)
        XCTAssertEqual(MGContrast.over((0xFFFFFF, 0), 0x123456), 0x123456)
        XCTAssertEqual(MGContrast.over((0xFFFFFF, 0.5), 0x000000), 0x808080)
    }

    /// Status chips and signature badges are tinted and outlined (owner, 16 Nov): text in
    /// the tone's per-mode text token on the tone's tint, composited over every surface
    /// of that mode, meets AA.
    func testToneTextMeetsAAOnTheTintOverEverySurfaceInBothModes() {
        for tone in tones {
            XCTAssertGreaterThanOrEqual(worst(tone.text.dark, on: tone, over: darkSurfaces), aa, "\(tone) dark")
            XCTAssertGreaterThanOrEqual(worst(tone.text.light, on: tone, over: lightSurfaces), aa, "\(tone) light")
        }
    }

    /// Why the text tokens exist: the tone colour itself as text on its own tint fails AA
    /// somewhere in each mode (danger and info in dark; every tone in light).
    func testToneColouredTextFailsAAOnTheTint() {
        let darkFailing = tones.filter { worst($0.hex, on: $0, over: darkSurfaces) < aa }
        let lightFailing = tones.filter { worst($0.hex, on: $0, over: lightSurfaces) < aa }
        XCTAssertEqual(darkFailing, [.danger, .info])
        XCTAssertEqual(lightFailing, [.secure, .warn, .danger, .info])
    }

    func testDemoBannerMeetsAA() {
        XCTAssertGreaterThanOrEqual(MGContrast.ratio(MGContrast.ink(on: MGToken.warn), MGToken.warn), aa)
    }

    func testThemeTextTokensMeetAAOnEverySurfaceInBothModes() {
        let dark: [(String, UInt32)] = [("heading", MGToken.textHeading), ("body", MGToken.textBody),
                                        ("muted", MGToken.textMuted)]
        for (name, text) in dark {
            for surface in darkSurfaces {
                XCTAssertGreaterThanOrEqual(MGContrast.ratio(text, surface), aa, "dark \(name) on \(surface)")
            }
        }
        let light = [("heading", MGToken.Light.textHeading.0), ("body", MGToken.Light.textBody.0),
                     ("muted", MGToken.Light.textMuted.0)]
        for (name, text) in light {
            for surface in lightSurfaces {
                XCTAssertGreaterThanOrEqual(MGContrast.ratio(text, surface), aa, "light \(name) on \(surface)")
            }
        }
    }

    /// Text in a token colour drawn on a background the host chooses cannot be checked
    /// here, so the shared views draw token-coloured text only on their own fill, in
    /// the ink `MGContrast.ink(on:)` picks. (`text-faint`, 4.0:1 dark and 2.8:1 light, is
    /// for decoration only and is not used.)
    func testViewsDrawTokenColouredTextOnlyOnTheirOwnFill() throws {
        let fixedInk = try NSRegularExpression(pattern: #"foregroundStyle\(Color\(mgHex: (?!MGContrast\.ink\(on: )"#)
        for (file, text) in try viewSources() {
            XCTAssertEqual(fixedInk.numberOfMatches(in: text, range: NSRange(text.startIndex..., in: text)), 0,
                           "\(file): token-coloured text not on its own fill")
            XCTAssertFalse(text.contains("textFaint"), "\(file): text-faint is below AA")
        }
    }

    /// The chips draw their tone's text token over their tint, outlined in the tone.
    func testToneChipsAreTintedAndOutlined() throws {
        let sources = Dictionary(uniqueKeysWithValues: try viewSources())
        for file in ["StatusChipsView.swift", "SignatureBadgeView.swift"] {
            let text = try XCTUnwrap(sources[file])
            XCTAssertTrue(text.contains(".tone.text.light") && text.contains(".tone.text.dark"), "\(file): text token")
            XCTAssertTrue(text.contains(".tone.tint"), "\(file): tint fill")
            XCTAssertTrue(text.contains("strokeBorder(Color(mgHex:") && text.contains(".tone.hex"), "\(file): outline")
            XCTAssertFalse(text.contains("MGContrast.ink(on:"), "\(file): filled-chip ink")
        }
    }

    // MARK: hit_target_size

    /// Every control on the shared screens is at least 44 × 44 pt where it can be hit.
    /// On macOS a SwiftUI button's hit region is its bezel, drawn with a focus ring view
    /// of the same frame; a `.frame` outside the button does not enlarge it.
    func testSharedScreensControlsAreAtLeast44Points() async throws {
        MGFonts.registerAll()
        let model = try await startedModel()
        XCTAssertEqual(model.phase, .needsEnrolment)
        try assertControlsAtLeast44(EnrolView(model: model), expected: 1, "EnrolView")
        await model.enrol()
        try assertControlsAtLeast44(PersonaPickerView(model: model), expected: model.personas.count, "PersonaPickerView")
        let persona = try XCTUnwrap(model.personas.first)
        try assertControlsAtLeast44(LockView(model: model, persona: persona), expected: 2, "LockView")
    }

    // MARK: dynamic_type

    /// Every font in the shared views is an `MGFonts` style, and those scale with the
    /// text style (`relativeTo:`); no fixed point size and no `.fixedSize`, which would
    /// clip at the largest sizes.
    func testViewsUseOnlyScalingFonts() throws {
        let fonts = try String(contentsOf: sourceRoot().appendingPathComponent("DesignSystem/Fonts.swift"), encoding: .utf8)
        XCTAssertEqual(fonts.components(separatedBy: "relativeTo: style)").count - 1, 3)
        let fontCall = try NSRegularExpression(pattern: #"\.font\(\s*([A-Za-z.]+)"#)
        for (file, text) in try viewSources() {
            for match in fontCall.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                let callee = String(text[Range(match.range(at: 1), in: text)!])
                XCTAssertTrue(callee.hasPrefix("MGFonts."), "\(file): .font(\(callee)…) does not scale")
            }
            XCTAssertFalse(text.contains(".fixedSize("), "\(file): fixed size")
            XCTAssertFalse(text.contains(".system(size:"), "\(file): fixed point size")
        }
    }

    // MARK: reduced_motion

    /// Nothing in MidgardKit animates, so Reduce Motion has nothing to stop. An animation
    /// added later must go through the app's motion policy (Phase B) and carry an
    /// `mg.anim.` identifier for the audit; this test then has to change with it.
    func testMidgardKitHasNoAnimation() throws {
        let animation = try NSRegularExpression(
            pattern: #"withAnimation|\.animation\(|\.transition\(|repeatForever|\.symbolEffect\(|TimelineView|matchedGeometryEffect"#)
        for (file, text) in try kitSources() {
            XCTAssertEqual(animation.numberOfMatches(in: text, range: NSRange(text.startIndex..., in: text)), 0,
                           "\(file) animates")
        }
    }

    /// A spinner is motion too: every `ProgressView(` carries an `mg.anim.` identifier on
    /// its line, so the audit can see it, and only runs when motion is not reduced.
    func testEverySpinnerIsTaggedAsAnAnimation() throws {
        var spinners = 0
        for (file, text) in try kitSources() {
            for line in text.split(separator: "\n") where line.contains("ProgressView(") {
                spinners += 1
                XCTAssertTrue(line.contains("\"mg.anim."), "\(file): an untagged spinner: \(line)")
            }
        }
        XCTAssertGreaterThan(spinners, 0)
    }

    func testReduceMotionFollowsTheSystemOrTheAuditEnvironment() {
        XCTAssertFalse(MotionPolicy.reduced(system: false, environment: [:]))
        XCTAssertTrue(MotionPolicy.reduced(system: true, environment: [:]))
        XCTAssertTrue(MotionPolicy.reduced(system: false, environment: ["MIDGARD_REDUCE_MOTION": "1"]))
        XCTAssertFalse(MotionPolicy.reduced(system: false, environment: ["MIDGARD_REDUCE_MOTION": "0"]))
    }

    /// Loading under Reduce Motion is static text; otherwise a spinner.
    func testLoadingIsStaticUnderReduceMotion() {
        XCTAssertEqual(spinners(in: LoadingView(reduced: true)), 0)
        XCTAssertEqual(spinners(in: LoadingView(reduced: false)), 1)
    }

    // MARK: screen_reader_labels

    /// Every image is hidden from VoiceOver or labelled, and every chip and row speaks a
    /// label of its own rather than its abbreviated text.
    func testImagesAreLabelledOrHiddenAndChipsCarryLabels() throws {
        for (file, text) in try viewSources() {
            for line in text.split(separator: "\n") where line.contains("Image(") {
                XCTAssertTrue(line.contains("accessibilityHidden(true)") || line.contains("accessibilityLabel("),
                              "\(file): an image without a label: \(line)")
            }
            if text.contains("accessibilityElement(children: .ignore)") {
                XCTAssertTrue(text.contains("accessibilityLabel("), "\(file): a combined element without a label")
            }
        }
    }

    // MARK: helpers

    private func sourceRoot() -> URL {
        repositoryRoot().appendingPathComponent("clients/shells/apple/Sources/MidgardKit")
    }

    private func kitSources() throws -> [(String, String)] {
        let files = try XCTUnwrap(FileManager.default.enumerator(at: sourceRoot(), includingPropertiesForKeys: nil))
            .compactMap { $0 as? URL }.filter { $0.pathExtension == "swift" }
        XCTAssertGreaterThan(files.count, 10)
        return try files.map { ($0.lastPathComponent, try String(contentsOf: $0, encoding: .utf8)) }
    }

    private func viewSources() throws -> [(String, String)] {
        let views = try FileManager.default.contentsOfDirectory(
            at: sourceRoot().appendingPathComponent("Views"), includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "swift" }
        XCTAssertGreaterThan(views.count, 5)
        return try views.map { ($0.lastPathComponent, try String(contentsOf: $0, encoding: .utf8)) }
    }

    private func startedModel() async throws -> AppModel {
        let dir = try temporaryDirectory()
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let secrets = InMemorySecretStore()
        let keyRoot = SoftwareKeyRoot(reason: "test", label: "Software key (test)", secrets: secrets,
                                      presence: AlwaysPresent())
        let model = AppModel(idleTimeout: 3600)
        await model.start(AppEnvironment(dataset: releaseDataset(), storeDirectory: dir, secrets: secrets,
                                         keyRoot: keyRoot, device: DeviceInfo(ramMb: 8192, hasAccelerator: true),
                                         replicaId: "a11y-test"))
        return model
    }

    /// The progress indicators AppKit draws for a laid-out view.
    private func spinners<V: View>(in view: V) -> Int {
        let (host, window) = laidOut(view)
        defer { window.close() }
        var count = 0
        func walk(_ view: NSView) {
            if view is NSProgressIndicator { count += 1 }
            view.subviews.forEach(walk)
        }
        walk(host)
        return count
    }

    private func laidOut<V: View>(_ view: V) -> (NSView, NSWindow) {
        let host = NSHostingView(rootView: view.frame(width: 390, height: 700))
        let window = NSWindow(contentRect: NSRect(x: 0, y: 0, width: 390, height: 700), styleMask: [.titled],
                              backing: .buffered, defer: false)
        window.isReleasedWhenClosed = false
        window.contentView = host
        host.layoutSubtreeIfNeeded()
        RunLoop.main.run(until: Date().addingTimeInterval(0.2))
        return (host, window)
    }

    /// Lays the view out in a window and measures each control's focus ring (the frame
    /// AppKit hit-tests). Fails when it finds fewer controls than `expected`, so a
    /// change in how SwiftUI draws controls cannot turn this into a vacuous pass.
    private func assertControlsAtLeast44<V: View>(_ view: V, expected: Int, _ name: String,
                                                  file: StaticString = #filePath, line: UInt = #line) throws {
        let (host, window) = laidOut(view)
        var controls: [NSRect] = []
        func walk(_ view: NSView) {
            if String(describing: type(of: view)).contains("FocusRing") || view is NSControl { controls.append(view.frame) }
            view.subviews.forEach(walk)
        }
        walk(host)
        window.close()
        XCTAssertGreaterThanOrEqual(controls.count, expected, "\(name): controls found \(controls)", file: file, line: line)
        for frame in controls {
            XCTAssertGreaterThanOrEqual(frame.width, 44, "\(name): \(frame)", file: file, line: line)
            XCTAssertGreaterThanOrEqual(frame.height, 44, "\(name): \(frame)", file: file, line: line)
        }
    }
}

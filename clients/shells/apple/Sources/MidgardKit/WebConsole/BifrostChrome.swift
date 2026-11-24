import MidgardCore
import SwiftUI

/// Bifrost's console frame as the web suite draws it (`ui/shared/src/nav.ts`, product
/// `bifrost`, at the signed tag): the INTERNAL · RELEASE CONTROL PLANE banner, the
/// header's one seal, and the navigation's two sections in the web's order, with the
/// web's labels, icons and badges, each opening one of the app's screens
/// (`ConsoleCatalog`'s Bifrost screens), and each screen's context line and inspector
/// width. These are the web console's fixed chrome, not data. [FR-C-1] [DD-7]
public enum BifrostChrome {
    /// `NAV.bifrost.banner`.
    public static let banner = (level: MarkingLevelChip.internal, text: "INTERNAL · RELEASE CONTROL PLANE")
    public static let seals: [(WebStatusPill.Status, String)] = [(.secure, "ARTEFACTS ML-DSA ✓")]
    public static let accent = MGWebAccent.bifrost

    /// One navigation item: the web's id, label, icon and badge, and the screen it opens.
    public struct Item: Identifiable {
        public let id: String
        public let label: String
        public let symbol: String
        public let badge: String?
        public let screen: String
        /// `BIFROST / …`, the header's context line on this screen.
        public let context: String
        /// The screen's inspector rail on the Mac (`asideWidth`: 340 on every screen).
        public let asideWidth: CGFloat
    }

    public static let sections: [(title: String, items: [Item])] = [
        ("DELIVERY", [
            Item(id: "releases", label: "Releases", symbol: WebIcon.symbol("rocket"), badge: nil,
                 screen: "Release Console", context: "BIFROST / RELEASES", asideWidth: 340),
            Item(id: "artefacts", label: "Artefacts", symbol: WebIcon.symbol("package"), badge: "58",
                 screen: "Artefacts", context: "BIFROST / ARTEFACTS", asideWidth: 340),
            Item(id: "channels", label: "Channels", symbol: WebIcon.symbol("git-branch"), badge: "3",
                 screen: "Channels", context: "BIFROST / CHANNELS", asideWidth: 340),
            Item(id: "bundles", label: "Offline bundles", symbol: WebIcon.symbol("download"), badge: "2",
                 screen: "Bundles", context: "BIFROST / OFFLINE BUNDLES", asideWidth: 340),
        ]),
        ("FLEET", [
            Item(id: "fleet", label: "Fleet & drift", symbol: WebIcon.symbol("server"), badge: nil,
                 screen: "Fleet", context: "BIFROST / FLEET", asideWidth: 340),
            Item(id: "rollback", label: "Rollback centre", symbol: WebIcon.symbol("undo-2"), badge: nil,
                 screen: "Rollback", context: "BIFROST / ROLLBACK CENTRE", asideWidth: 340),
        ]),
    ]

    /// The navigation item that opens `screen` (a `ConsoleCatalog` name).
    public static func item(for screen: String) -> Item? {
        sections.flatMap(\.items).first { $0.screen == screen }
    }

    /// The screens in the web's navigation order.
    public static var screens: [String] { sections.flatMap(\.items).map(\.screen) }
}

/// A Bifrost row's `state` (`BifrostState`) as a pill: `online` reads as the web's, a
/// live seal in the accent's live colour.
func bifrostPill(_ state: String, _ label: String) -> WebStatusPill {
    let live = MGWebAccent.bifrost.text
    switch state {
    case "secure": return WebStatusPill(.secure, label, live: live)
    case "online": return WebStatusPill(.online, label, live: live)
    case "healthy": return WebStatusPill(.healthy, label, live: live)
    case "warn": return WebStatusPill(.warn, label, live: live)
    case "danger": return WebStatusPill(.danger, label, live: live)
    case "gated": return WebStatusPill(.gated, label, live: live)
    default: return WebStatusPill(.offline, label, live: live)
    }
}

/// The release console's health-gate pill (`GATE_PILL`): PASS healthy, WATCH warn, FAIL
/// danger.
public func bifrostGatePill(_ gate: String) -> WebStatusPill {
    switch gate {
    case "pass": return WebStatusPill(.healthy, "GATE PASS")
    case "fail": return WebStatusPill(.danger, "GATE FAIL")
    default: return WebStatusPill(.warn, "GATE WATCH")
    }
}

import MidgardCore
import SwiftUI

/// Admin's console frame as the web suite draws it (`ui/shared/src/nav.ts`, product
/// `admin`, at the signed tag): the management-zone classification banner, the header's
/// seals, the grey ADMIN product mark, and the navigation's four sections in the web's
/// order, with the web's labels, icons and badges, each opening one of the app's screens
/// (`ConsoleCatalog`'s Admin screens), and each screen's context line and inspector
/// width. Admin has no accent of its own: it renders under Midgard's (the plan's Phase
/// D). These are the web console's fixed chrome, not data. [FR-AD-1] [DD-7]
public enum AdminChrome {
    /// `NAV.admin.banner`.
    public static let banner = (level: MarkingLevelChip.confidential,
                                text: "CONFIDENTIAL · MANAGEMENT ZONE · ADMIN TRAFFIC SEGREGATED")
    public static let seals: [(WebStatusPill.Status, String)] = [(.secure, "SELF-GOVERNED"), (.online, "7 SUBSYSTEMS")]
    public static let accent = MGWebAccent.midgard
    public static let tile = WebProductMark.Tile.admin

    /// One navigation item: the web's id, label, icon and badge, and the screen it opens.
    public struct Item: Identifiable {
        public let id: String
        public let label: String
        public let symbol: String
        public let badge: String?
        public let screen: String
        /// `ADMIN / …`, the header's context line on this screen.
        public let context: String
        /// The screen's inspector rail on the Mac (`asideWidth`, 330 by default).
        public let asideWidth: CGFloat
    }

    public static let sections: [(title: String, items: [Item])] = [
        ("OVERSEE", [
            Item(id: "overview", label: "Overview", symbol: WebIcon.symbol("gauge"), badge: nil, screen: "Overview",
                 context: "ADMIN / OVERVIEW", asideWidth: 330),
            Item(id: "approvals", label: "Approvals", symbol: WebIcon.symbol("users"), badge: "3", screen: "Approvals",
                 context: "ADMIN / DUAL-CONTROL", asideWidth: 330),
        ]),
        ("ADMINISTER", [
            Item(id: "iam", label: "Identity & roles", symbol: WebIcon.symbol("fingerprint"), badge: nil, screen: "IAM",
                 context: "ADMIN / IDENTITY & ROLES", asideWidth: 330),
            Item(id: "config", label: "Config & policy", symbol: WebIcon.symbol("git-branch"), badge: nil, screen: "Config",
                 context: "ADMIN / CONFIG & POLICY", asideWidth: 360),
            Item(id: "registry", label: "Registry & governance", symbol: WebIcon.symbol("cpu"), badge: nil,
                 screen: "Registry", context: "ADMIN / REGISTRY & SUITE GOVERNANCE", asideWidth: 330),
            Item(id: "ledger", label: "Audit ledger", symbol: WebIcon.symbol("scroll-text"), badge: nil, screen: "Ledger",
                 context: "ADMIN / AUDIT LEDGER", asideWidth: 360),
        ]),
        ("EMERGENCY", [
            Item(id: "breakglass", label: "Break-glass", symbol: WebIcon.symbol("alert-triangle"), badge: "1",
                 screen: "Break-Glass", context: "ADMIN / BREAK-GLASS", asideWidth: 340),
        ]),
        ("ASSIST", [
            Item(id: "copilot", label: "Admin copilot", symbol: WebIcon.symbol("sparkles"), badge: nil, screen: "Copilot",
                 context: "ADMIN / COPILOT · ADVISORY ONLY", asideWidth: 330),
        ]),
    ]

    /// The navigation item that opens `screen` (a `ConsoleCatalog` name).
    public static func item(for screen: String) -> Item? {
        sections.flatMap(\.items).first { $0.screen == screen }
    }

    /// The screens in the web's navigation order.
    public static var screens: [String] { sections.flatMap(\.items).map(\.screen) }
}

/// The `admin status` terminal under the navigation (Overview's `navFooter`).
public struct AdminStatusFooter: View {
    public init() {}

    public var body: some View {
        WebTerminal(lines: ["> admin status", "# 7 SUBSYSTEMS · SELF-GOVERNED"], small: true)
    }
}

import XCTest

/// The app MidgardMacUITests runs against: Midgard, the release edition (MidgardEdition
/// `release`), the signed release v0.1.0-rc.3 only, with no visible change. [FR-C-1]
enum Edition {
    static let isThreeD = false
    static let banner: (id: String, text: String)? = nil
    static let aboutName: String? = nil
    /// Screens only this edition has, for AccessibilityAudit: none.
    static let auditedScreens: [(XCUIApplication) -> Void] = []
}

// swift-tools-version:5.10
// MidgardKit: the shared SwiftUI layer of the Apple shell (SAD_06 §5.1). Presentation
// only (KDD-1): crypto, keys, sync merge and clearance are calls into MidgardCore,
// the UniFFI bindings over midgard-client-core. The key root implements only the
// core's seal/unseal callback. Build the XCFramework and the bindings first:
//   bash tools/build-xcframework.sh
// Swift 5 language mode: UniFFI 0.28's generated Swift predates strict concurrency.
// [FR-C-1] [DD-6]
import PackageDescription

let package = Package(
    name: "MidgardKit",
    defaultLocalization: "en",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "MidgardKit", targets: ["MidgardKit"]),
        // The accessibility audit's base class; each app's UI-test target subclasses it
        // (Task A14, clients/shells/apple/audit.sh). Links XCTest: UI-test targets only.
        .library(name: "MidgardAuditKit", targets: ["MidgardAuditKit"]),
        // The edition library (Task B7; owner decisions 22 and 23 Nov 2026), linked by
        // the two apps, MidgardMacMaps and MidgardiOSMaps (tools/check-edition.sh). It
        // holds everything the edition shows (banner, About, Heimdall's MAP and 3D views
        // with their tile loader) and registers it at link time as MidgardKit's
        // EditionProvider: its strong `midgard_edition_hook` replaces MidgardHooks' weak
        // default.
        .library(name: "MidgardGlobeKit", targets: ["MidgardGlobeKit"]),
    ],
    targets: [
        .binaryTarget(name: "MidgardCoreFFI", path: "Frameworks/MidgardCoreFFI.xcframework"),
        .target(name: "MidgardCore", dependencies: ["MidgardCoreFFI"], path: "Sources/MidgardCore",
                exclude: [".gitkeep"]),
        // The link-time hooks MidgardKit reads: weak C defaults that a linked library's
        // strong definitions replace. No runtime switch, no lookup by name.
        .target(name: "MidgardHooks", path: "Sources/MidgardHooks"),
        .target(name: "MidgardKit", dependencies: ["MidgardCore", "MidgardHooks"], path: "Sources/MidgardKit",
                resources: [.copy("Resources/Fonts")]),
        .target(name: "MidgardAuditKit", path: "Sources/MidgardAuditKit"),
        // The Maps edition's library: Heimdall's MAP and 3D views (Phase I), the map tile
        // loader and the bundled Natural Earth outlines (public domain).
        .target(name: "MidgardGlobeKit", dependencies: ["MidgardKit"], path: "Sources/MidgardGlobeKit",
                resources: [.copy("Resources/Outlines")]),
        .testTarget(name: "MidgardKitTests", dependencies: ["MidgardKit", "MidgardCore", "MidgardAuditKit"],
                    path: "Tests/MidgardKitTests"),
        .testTarget(name: "MidgardGlobeKitTests", dependencies: ["MidgardGlobeKit", "MidgardKit"],
                    path: "Tests/MidgardGlobeKitTests", exclude: ["Fixtures"]),
    ]
)

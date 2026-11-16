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
    ],
    targets: [
        .binaryTarget(name: "MidgardCoreFFI", path: "Frameworks/MidgardCoreFFI.xcframework"),
        .target(name: "MidgardCore", dependencies: ["MidgardCoreFFI"], path: "Sources/MidgardCore",
                exclude: [".gitkeep"]),
        .target(name: "MidgardKit", dependencies: ["MidgardCore"], path: "Sources/MidgardKit",
                resources: [.copy("Resources/Fonts")]),
        .target(name: "MidgardAuditKit", path: "Sources/MidgardAuditKit"),
        .testTarget(name: "MidgardKitTests", dependencies: ["MidgardKit", "MidgardCore", "MidgardAuditKit"],
                    path: "Tests/MidgardKitTests"),
    ]
)

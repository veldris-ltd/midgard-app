// swift-tools-version:6.0
// Midgard.app: the native macOS presentation shell. No third-party dependencies.
// Build outside the suite folder (storage budget):
//   swift build --package-path apps/mac --scratch-path ~/Library/Caches/midgard-build/swift
import PackageDescription

let package = Package(
    name: "Midgard",
    platforms: [.macOS(.v13)],
    products: [
        .executable(name: "Midgard", targets: ["Midgard"]),
        .library(name: "MidgardShellCore", targets: ["MidgardShellCore"]),
    ],
    targets: [
        // Everything testable without a window: ports, paths, checks, the web server.
        .target(name: "MidgardShellCore"),
        .executableTarget(name: "Midgard", dependencies: ["MidgardShellCore"]),
        .testTarget(name: "MidgardShellCoreTests", dependencies: ["MidgardShellCore"]),
    ],
    swiftLanguageModes: [.v6]
)

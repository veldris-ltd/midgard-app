import Foundation

/// The minimum Mac the demo runs well on, checked before anything starts so the
/// presenter gets one clear sentence rather than services failing one by one.
public enum SystemCheck {
    /// 13.3: what llama.cpp's macOS builds (the bundled model server) need.
    static let minimumMajor = 13
    static let minimumMinor = 3
    /// 8 GB Macs report exactly 8 × 1024³ bytes.
    static let minimumMemory: UInt64 = 8 * 1024 * 1024 * 1024

    /// nil when this Mac is supported, else a short message for the presenter.
    public static func problem(osVersion: OperatingSystemVersion, memoryBytes: UInt64) -> String? {
        if (osVersion.majorVersion, osVersion.minorVersion) < (minimumMajor, minimumMinor) {
            var version = "\(osVersion.majorVersion).\(osVersion.minorVersion)"
            if osVersion.patchVersion > 0 { version += ".\(osVersion.patchVersion)" }
            return "Midgard needs macOS \(minimumMajor).\(minimumMinor) or later; this Mac runs \(version)."
        }
        if memoryBytes < minimumMemory {
            let gb = Double(memoryBytes) / Double(1024 * 1024 * 1024)
            // Nearest tenth, but never shown as 8 when it is below 8.
            var shown = String(format: "%.1f", min((gb * 10).rounded() / 10, 7.9))
            if shown.hasSuffix(".0") { shown.removeLast(2) }
            return "Midgard needs at least 8 GB of memory; this Mac has \(shown) GB."
        }
        return nil
    }

    /// The check against this Mac's real version and memory.
    public static func problemOnThisMac() -> String? {
        let info = ProcessInfo.processInfo
        return problem(osVersion: info.operatingSystemVersion, memoryBytes: info.physicalMemory)
    }
}

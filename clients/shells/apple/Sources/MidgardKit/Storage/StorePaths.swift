import Foundation
import MidgardCore

/// Where the sealed files live: Application Support/Midgard/<release>, excluded from
/// backup; 0700 on macOS, complete file protection on iOS. [FR-C-4] [AC-C-2]
public enum StorePaths {
    public static func storeDirectory(base: URL? = nil) throws -> URL {
        let root = try base ?? FileManager.default.url(for: .applicationSupportDirectory, in: .userDomainMask,
                                                       appropriateFor: nil, create: true)
        var url = root.appendingPathComponent("Midgard", isDirectory: true)
            .appendingPathComponent(coreVersion(), isDirectory: true)
        #if os(iOS)
        let attributes: [FileAttributeKey: Any] = [.protectionKey: FileProtectionType.complete]
        #else
        let attributes: [FileAttributeKey: Any] = [.posixPermissions: 0o700]
        #endif
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true, attributes: attributes)
        try FileManager.default.setAttributes(attributes, ofItemAtPath: url.path)
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try url.setResourceValues(values)
        return url
    }

    /// The dataset bundled in the app as `<release>.mgdemo`.
    public static func bundledDataset(in bundle: Bundle = .main) -> URL? {
        bundle.url(forResource: coreVersion(), withExtension: "mgdemo")
    }
}

import Foundation
import SwiftUI

/// Whether motion is reduced: the system's Reduce Motion, or, in Debug builds only,
/// `MIDGARD_REDUCE_MOTION=1` in the launch environment (how the accessibility audit asks
/// for it; a Release build reads no test variable). [RC-49] [AC-C-11]
public enum MotionPolicy {
    public static func reduced(system: Bool,
                               environment: [String: String] = ProcessInfo.processInfo.environment) -> Bool {
        #if DEBUG
        return system || environment["MIDGARD_REDUCE_MOTION"] == "1"
        #else
        return system
        #endif
    }
}

/// Loading: a spinner, tagged `mg.anim.loading` for the audit, or static text when motion
/// is reduced. [RC-49] [AC-C-11]
public struct LoadingView: View {
    @Environment(\.accessibilityReduceMotion) private var systemReduceMotion
    let reduced: Bool?

    /// `reduced` overrides the policy (tests); by default it follows `MotionPolicy`.
    public init(reduced: Bool? = nil) { self.reduced = reduced }

    public var body: some View {
        if reduced ?? MotionPolicy.reduced(system: systemReduceMotion) {
            Text("Loading…").font(MGFonts.sans(.body)).accessibilityIdentifier("mg.loading")
        } else {
            ProgressView().accessibilityLabel(Text("Loading")).accessibilityIdentifier("mg.anim.loading")
        }
    }
}

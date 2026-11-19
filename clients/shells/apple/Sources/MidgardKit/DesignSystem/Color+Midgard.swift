import SwiftUI
#if canImport(UIKit)
import UIKit
#else
import AppKit
#endif

public extension Color {
    /// A colour from a 0xRRGGBB token. [DD-7]
    init(mgHex hex: UInt32, opacity: Double = 1) {
        self.init(.sRGB, red: Double((hex >> 16) & 0xFF) / 255, green: Double((hex >> 8) & 0xFF) / 255,
                  blue: Double(hex & 0xFF) / 255, opacity: opacity)
    }

    /// A colour that follows light and dark mode (the console's `.mode-light` scope).
    init(mgLight light: (UInt32, Double), dark: (UInt32, Double)) {
        #if canImport(UIKit)
        self.init(UIColor { traits in
            traits.userInterfaceStyle == .dark ? UIColor(Color(mgHex: dark.0, opacity: dark.1))
                                               : UIColor(Color(mgHex: light.0, opacity: light.1))
        })
        #else
        self.init(NSColor(name: nil) { appearance in
            appearance.bestMatch(from: [.darkAqua, .aqua]) == .darkAqua
                ? NSColor(Color(mgHex: dark.0, opacity: dark.1)) : NSColor(Color(mgHex: light.0, opacity: light.1))
        })
        #endif
    }
}

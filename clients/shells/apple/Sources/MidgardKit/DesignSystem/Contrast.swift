import Foundation

/// WCAG 2.x contrast between token colours, and the text ink for a filled chip. A chip
/// draws its own fill, so its text contrast does not depend on the screen behind it.
/// [RC-49] [AC-C-11] [DD-7]
public enum MGContrast {
    /// WCAG 2.x AA for body-size text.
    public static let aaText = 4.5

    /// Relative luminance of an sRGB 0xRRGGBB colour.
    public static func luminance(_ hex: UInt32) -> Double {
        func channel(_ shift: UInt32) -> Double {
            let c = Double((hex >> shift) & 0xFF) / 255
            return c <= 0.04045 ? c / 12.92 : pow((c + 0.055) / 1.055, 2.4)
        }
        return 0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
    }

    /// The contrast ratio between two colours, 1…21, in either order.
    public static func ratio(_ a: UInt32, _ b: UInt32) -> Double {
        let (la, lb) = (luminance(a), luminance(b))
        return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
    }

    /// The darkest ink or white, whichever reads better on `fill`.
    public static func ink(on fill: UInt32) -> UInt32 {
        ratio(MGToken.ink950, fill) >= ratio(MGToken.white, fill) ? MGToken.ink950 : MGToken.white
    }
}

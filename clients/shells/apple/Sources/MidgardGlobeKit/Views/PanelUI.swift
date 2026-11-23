import MidgardCore
import MidgardKit
import SwiftUI

// The panels, lists, notices and credits the MAP and 3D views share, as the web's
// `globe/globe.css` and `app/Credits.tsx` draw them: translucent sunken panels with a
// divider border, mono headings, plain list buttons with an accent border on the current
// one, and the folding ⓘ CREDITS control. On the iPhone every control is at least 44 pt.
// [FR-H-1] [DD-7] [AC-C-11]

#if os(iOS)
let minTarget: CGFloat = 44
let isCompactPlatform = true
#else
/// The Mac's smallest targets, as the console's own (26–34 pt).
let minTarget: CGFloat = 28
let isCompactPlatform = false
#endif

extension View {
    /// Letter-spacing in ems of the drawn size, as the web writes it.
    func tracked(_ em: CGFloat, _ size: CGFloat) -> some View { tracking(em * MGWebFont.monoSize(size)) }
}

/// The accent and warning tones of the tracks (`--accent`, `--warn`).
enum Tone {
    static let accent = MGWebAccent.heimdall.fill
    static let warn = MGWeb.warn

    static func of(_ track: HeimdallTrack) -> Color { track.track == "anomalous" ? warn : accent }
    static func of(_ tone: GlobeItem.Tone) -> Color { tone == .warn ? warn : accent }
}

struct Panel<Content: View>: View {
    let content: Content
    init(@ViewBuilder content: () -> Content) { self.content = content() }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) { content }
            .padding(10)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.bgSunken.opacity(0.92)))
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(.ultraThinMaterial))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.divider))
    }
}

struct PanelHeading: View {
    let text: String
    var trailing: String?

    var body: some View {
        HStack(spacing: 8) {
            Text(text)
            Spacer(minLength: 4)
            if let trailing { Text(trailing) }
        }
        .font(MGWebFont.mono(9))
        .tracked(0.16, 9)
        .foregroundStyle(MGWeb.textMuted)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(.isHeader)
    }
}

/// A list row button: mono 10, an accent border and text when current.
struct PanelButton<Label: View>: View {
    let current: Bool
    var disabled = false
    let identifier: String
    let accessibility: String
    let action: () -> Void
    @ViewBuilder let label: () -> Label

    var body: some View {
        Button(action: action) {
            label()
                .font(MGWebFont.mono(10))
                .tracked(0.08, 10)
                .foregroundStyle(disabled ? MGWeb.textMuted : current ? MGWebAccent.heimdall.text : MGWeb.textBody)
                .padding(.horizontal, 8)
                .padding(.vertical, 5)
                .frame(maxWidth: .infinity, minHeight: minTarget, alignment: .leading)
                .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(current ? MGWebAccent.heimdall.fill : .clear))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(disabled)
        .accessibilityLabel(Text(accessibility))
        .accessibilityAddTraits(current ? [.isSelected] : [])
        .accessibilityIdentifier(identifier)
    }
}

/// A bordered square control (zoom, tilt, compass).
struct ControlButton<Label: View>: View {
    let accessibility: String
    let identifier: String
    let action: () -> Void
    @ViewBuilder let label: () -> Label

    var body: some View {
        Button(action: action) {
            label()
                .font(MGWebFont.mono(10))
                .tracked(0.1, 10)
                .foregroundStyle(MGWeb.textBody)
                .frame(maxWidth: .infinity, minHeight: max(minTarget, 26))
                .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(MGWeb.border))
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(accessibility))
        .accessibilityIdentifier(identifier)
    }
}

/// A notice over the top of the view (`.hg-globe-notice`).
struct Notice: View {
    let text: String

    var body: some View {
        Text(text)
            .font(MGWebFont.mono(10))
            .tracked(0.14, 10)
            .foregroundStyle(MGWeb.warnText)
            .multilineTextAlignment(.center)
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.bgSunken.opacity(0.9)))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.divider))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.heimdall.tiles.notice")
    }
}

/// The folding ⓘ CREDITS control: every source the view draws from, its licence and
/// its attribution, as plain text.
struct CreditsControl: View {
    let credits: [Credit]
    @State private var open = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Button {
                open.toggle()
            } label: {
                Text("ⓘ CREDITS")
                    .font(MGWebFont.mono(9))
                    .tracked(0.14, 9)
                    .foregroundStyle(open ? MGWeb.textHeading : MGWeb.textMuted)
                    .padding(.horizontal, 8)
                    .frame(minHeight: max(minTarget, 22))
                    .overlay(RoundedRectangle(cornerRadius: 3).strokeBorder(open ? MGWeb.borderStrong : MGWeb.border))
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(Text("Credits"))
            .accessibilityValue(Text(open ? "shown" : "hidden"))
            .accessibilityIdentifier("mg.heimdall.credits")
            if open { CreditsList(credits: credits).frame(maxHeight: 220) }
        }
    }
}

struct CreditsList: View {
    let credits: [Credit]

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(credits) { c in
                    VStack(alignment: .leading, spacing: 2) {
                        Text(c.name).font(MGWebFont.sans(10).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                        Text(c.licence).font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textMuted)
                        Text(c.attribution).font(MGWebFont.sans(10)).foregroundStyle(MGWeb.textMuted)
                    }
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityElement(children: .combine)
                    .accessibilityIdentifier("mg.heimdall.credit.\(c.id)")
                }
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .accessibilityIdentifier("mg.heimdall.credits.list")
    }
}

/// The credits strip: each source in use, by name, always shown along the view's edge.
struct CreditsStrip: View {
    let credits: [Credit]

    var body: some View {
        Text(credits.map(\.short).joined(separator: " · "))
            .font(MGWebFont.mono(8))
            .foregroundStyle(MGWeb.textMuted)
            .fixedSize(horizontal: false, vertical: true)
            .multilineTextAlignment(.leading)
            .padding(.horizontal, 6)
            .padding(.vertical, 3)
            .background(RoundedRectangle(cornerRadius: 3).fill(MGWeb.bgSunken.opacity(0.92)))
            .accessibilityLabel(Text("Map data: " + credits.map(\.short).joined(separator: ", ")))
            .accessibilityIdentifier("mg.heimdall.credits.strip")
    }
}

/// One track in the TRACKS list: its tone, id and altitude, or NO POSITION.
struct TrackRow: View {
    let track: HeimdallTrack
    let placed: Bool

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Circle().fill(Tone.of(track)).frame(width: 7, height: 7).alignmentGuide(.firstTextBaseline) { $0[.bottom] - 1 }
            Text(track.id)
            Spacer(minLength: 4)
            Text(placed ? track.alt : "NO POSITION").font(MGWebFont.mono(9)).foregroundStyle(MGWeb.textMuted)
        }
    }

    static func label(_ t: HeimdallTrack, placed: Bool) -> String {
        "\(t.id) · \(t.cls) · \(t.alt)\(t.track == "anomalous" ? " · anomalous" : "")\(placed ? "" : " · no position")"
    }
}

/// A track's map or globe marker: the layer's shape (a chevron for aircraft and a bow for
/// vessels with a heading, a circle, square or diamond otherwise), in its tone.
struct MarkerShape: Shape {
    let layer: String
    let pointed: Bool

    func path(in r: CGRect) -> Path {
        let m = r.width / 2
        func pt(_ x: Double, _ y: Double) -> CGPoint { CGPoint(x: r.minX + x * m, y: r.minY + y * m) }
        var p = Path()
        switch (layer, pointed) {
        case ("air", true):
            p.move(to: pt(1, 0.18)); p.addLine(to: pt(1.62, 1.7)); p.addLine(to: pt(1, 1.3)); p.addLine(to: pt(0.38, 1.7))
            p.closeSubpath()
        case ("maritime", true):
            p.move(to: pt(1, 0.22)); p.addLine(to: pt(1.5, 0.75)); p.addLine(to: pt(1.5, 1.6)); p.addLine(to: pt(0.5, 1.6))
            p.addLine(to: pt(0.5, 0.75)); p.closeSubpath()
        case ("maritime", false):
            p.addRect(CGRect(origin: pt(0.42, 0.42), size: CGSize(width: 1.16 * m, height: 1.16 * m)))
        case ("ground", _):
            p.move(to: pt(1, 0.24)); p.addLine(to: pt(1.76, 1)); p.addLine(to: pt(1, 1.76)); p.addLine(to: pt(0.24, 1))
            p.closeSubpath()
        default:
            p.addEllipse(in: CGRect(origin: pt(0.38, 0.38), size: CGSize(width: 1.24 * m, height: 1.24 * m)))
        }
        return p
    }
}

/// A track's label drawn into a canvas (not a text element over the imagery): mono 9,
/// `--text-faint` (the selected one `--text-heading`), with the web's sunken halo.
func drawTrackLabel(_ ctx: inout GraphicsContext, _ text: String, at p: CGPoint, selected: Bool) {
    let label = Text(text)
        .font(MGWebFont.mono(9))
        .tracking(0.08 * MGWebFont.monoSize(9))
        .foregroundColor(selected ? MGWeb.textHeading : MGWeb.textMuted)
    ctx.drawLayer { layer in
        layer.addFilter(.shadow(color: MGWeb.bgSunken, radius: 2))
        layer.addFilter(.shadow(color: MGWeb.bgSunken, radius: 1))
        layer.draw(label, at: p, anchor: .top)
    }
}

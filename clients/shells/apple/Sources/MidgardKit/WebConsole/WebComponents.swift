import MidgardCore
import SwiftUI

// The web design system's components (`ui/design-system/src/components/`), drawn
// natively at the web's sizes, for the screens matched to the web suite. Each names the
// component it follows. [DD-7] [RC-49] [AC-C-11]

/// `ClassificationBanner`: the console's marking across the top. 26 pt (20 compact),
/// mono 10 (9) semibold, tracked 0.18 em, upper case.
public struct WebClassificationBanner: View {
    let level: MarkingLevelChip
    let text: String
    let compact: Bool

    public init(level: MarkingLevelChip, text: String, compact: Bool = false) {
        self.level = level
        self.text = text
        self.compact = compact
    }

    @Environment(\.dynamicTypeSize) private var typeSize

    public var body: some View {
        let size: CGFloat = compact ? 9 : 10
        let fill = MGToken.markingHex(level)
        // Untracked at accessibility text sizes; on the iPhone 12 pt bold relative to
        // caption (caption2 does not scale fully), where 11 pt semibold on the marking
        // red failed the contrast audit.
        let large = typeSize.isAccessibilitySize
        #if os(iOS)
        let font = MGWebFont.mono(12, .caption).weight(.bold)
        let drawn: CGFloat = 12
        #else
        let font = MGWebFont.mono(size).weight(.semibold)
        let drawn = MGWebFont.monoSize(size)
        #endif
        // At accessibility text sizes the level alone (the whole line is the spoken
        // label): the full line was taller than the iPhone's visible screen. The line
        // wraps rather than truncates, and the label is the text's own, as on the demo
        // banner: as a container over a text child, the iOS Dynamic Type audit reported
        // the level as clipped.
        Text(large ? (text.components(separatedBy: " · ").first ?? text).uppercased() : text.uppercased())
            .font(font)
            .mgTracking(large ? 0 : 0.18, size: drawn)
            .multilineTextAlignment(.center)
            .fixedSize(horizontal: false, vertical: true)
            .foregroundStyle(Color(mgHex: MGContrast.ink(on: fill)))
            .padding(.horizontal, MGSpace.s2)
            .frame(maxWidth: .infinity, minHeight: compact ? 20 : 26)
            .background(Color(mgHex: fill))
            .accessibilityLabel(Text("Classification: \(text)"))
            .accessibilityAddTraits(.isStaticText)
            .accessibilityIdentifier("mg.classification")
    }
}

/// `ProductMark` (size sm, no descriptor): the initial tile in the product tint and the
/// Oxanium word mark.
public struct WebProductMark: View {
    /// A mark's own tile colours, where the product has no accent of its own (Admin's
    /// grey `--grey-100` initial on a neutral tint).
    public struct Tile {
        public let text: Color
        public let tint: Color

        public init(text: Color, tint: Color) {
            self.text = text
            self.tint = tint
        }

        /// `ProductMark product="admin"`.
        public static let admin = Tile(text: MGWeb.grey100, tint: Color(mgHex: 0x7F7F7F, opacity: 0.16))
    }

    let name: String
    let accent: MGWebAccent
    let tile: Tile?

    public init(name: String, accent: MGWebAccent, tile: Tile? = nil) {
        self.name = name
        self.accent = accent
        self.tile = tile
    }

    public var body: some View {
        HStack(spacing: 7) {
            Text(String(name.prefix(1)))
                .font(MGWebFont.display(13, .footnote).weight(.black))
                .foregroundStyle(tile?.text ?? accent.text)
                .frame(minWidth: 26, minHeight: 26)
                .background(RoundedRectangle(cornerRadius: 7).fill(tile?.tint ?? accent.tint))
                .overlay(RoundedRectangle(cornerRadius: 7).strokeBorder(MGWeb.borderStrong))
            Text(name.uppercased())
                .font(MGWebFont.display(13, .footnote).weight(.bold))
                .mgTracking(0.07, size: 13)
                .foregroundStyle(MGWeb.textHeading)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(name))
        .accessibilityAddTraits(.isHeader)
    }
}

/// `StatusPill`: a dot in the status colour and mono 12 text on an inset pill.
public struct WebStatusPill: View {
    public enum Status { case secure, online, healthy, warn, offline, danger, live, gated }
    let status: Status
    let label: String
    let live: Color

    /// `live` is the console's accent text (`--accent-hover`), the colour of a live dot.
    public init(_ status: Status, _ label: String, live: Color = MGWebAccent.midgard.text) {
        self.status = status
        self.label = label
        self.live = live
    }

    private var dot: Color {
        switch status {
        case .secure, .online, .healthy: return MGWeb.secure
        case .warn, .gated: return MGWeb.warn
        case .offline: return MGWeb.decorFaint
        case .danger: return MGWeb.dangerText
        case .live: return live
        }
    }

    public var body: some View {
        HStack(spacing: 7) {
            Circle().fill(dot).frame(width: 8, height: 8).accessibilityHidden(true)
            Text(label)
                .font(MGWebFont.mono(12, .caption).weight(.medium))
                .mgTracking(0.04, size: MGWebFont.monoSize(12))
                .foregroundStyle(MGWeb.textBody)
        }
        .padding(.leading, 8)
        .padding(.trailing, 10)
        .padding(.vertical, 3)
        .background(Capsule().fill(MGWeb.surfaceInset))
        .overlay(Capsule().strokeBorder(MGWeb.border))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(label))
        .accessibilityAddTraits(.isStaticText)
    }
}

/// `Badge`: mono 11, tracked 0.06 em, on its tone's tint (secure, warn, danger, info)
/// or a raised fill (neutral).
public struct WebBadge: View {
    public enum Tone { case neutral, secure, warn, danger, info }
    let text: String
    let tone: Tone

    public init(_ text: String, tone: Tone) {
        self.text = text
        self.tone = tone
    }

    private var colours: (fg: Color, bg: Color, border: Color) {
        switch tone {
        case .neutral: return (MGWeb.textBody, MGWeb.surfaceRaised, MGWeb.border)
        case .secure: return (MGWeb.secureText, MGWeb.secureTint, .clear)
        case .warn: return (MGWeb.warnText, MGWeb.warnTint, .clear)
        case .danger: return (MGWeb.dangerText, MGWeb.dangerTint, .clear)
        case .info: return (MGWeb.infoText, MGWeb.infoTint, .clear)
        }
    }

    public var body: some View {
        let c = colours
        Text(text)
            .font(MGWebFont.mono(11).weight(.medium))
            .mgTracking(0.06, size: MGWebFont.monoSize(11))
            .lineLimit(1)
            .foregroundStyle(c.fg)
            .padding(.horizontal, 8)
            .padding(.vertical, 2)
            .background(RoundedRectangle(cornerRadius: MGRadius.xs).fill(c.bg))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.xs).strokeBorder(c.border))
    }
}

/// `MarkingChip`: the level's initial on its fixed colour, the level and the
/// compartment, mono 11 on an inset pill.
public struct WebMarkingChip: View {
    let chip: MarkingChip

    public init(_ chip: MarkingChip) { self.chip = chip }

    private var levelWord: String {
        switch chip.level {
        case .open: return "OPEN"
        case .internal: return "INTERNAL"
        case .restricted: return "RESTRICTED"
        case .confidential: return "CONFIDENTIAL"
        }
    }

    /// The compartments of the core's label (`CONFIDENTIAL // OPS-N, SIG`), if any.
    private var compartments: String? {
        let parts = chip.label.components(separatedBy: " // ")
        return parts.count > 1 ? parts.dropFirst().joined(separator: " // ") : nil
    }

    public var body: some View {
        let fill = MGToken.markingHex(chip.level)
        HStack(spacing: 6) {
            Text(String(levelWord.prefix(1)))
                .font(MGWebFont.mono(8).weight(.bold))
                .foregroundStyle(Color(mgHex: MGContrast.ink(on: fill)))
                .frame(minWidth: 14, minHeight: 14)
                .background(Circle().fill(Color(mgHex: fill)))
            Text(levelWord).foregroundStyle(MGWeb.textBody)
            if let compartments {
                Text("// \(compartments)").foregroundStyle(MGWeb.textFaint)
            }
        }
        .font(MGWebFont.mono(11))
        .mgTracking(0.08, size: MGWebFont.monoSize(11))
        .padding(.leading, 4)
        .padding(.trailing, 8)
        .padding(.vertical, 2)
        .background(Capsule().fill(MGWeb.surfaceInset))
        .overlay(Capsule().strokeBorder(MGWeb.border))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Marking: \(chip.label)"))
        .accessibilityAddTraits(.isStaticText)
    }
}

/// `StatBlock` (size sm): the readout in Oxanium black 24, the label mono 11 tracked
/// 0.14 em.
public struct WebStatBlock: View {
    let value: String
    let label: String
    let accent: MGWebAccent?

    public init(value: String, label: String, accent: MGWebAccent? = nil) {
        self.value = value
        self.label = label
        self.accent = accent
    }

    /// On the Mac the figure's element says the label too and the label is not read
    /// twice; on the iPhone each says its own text (the iPhone's text-detection audit
    /// reports drawn text that no element says).
    static var labelSpoken: Bool {
        #if os(iOS)
        return true
        #else
        return false
        #endif
    }

    public var body: some View {
        // Two elements, each only its own text (one element over both measured the
        // readout's colour against the label's in the contrast audit).
        VStack(alignment: .leading, spacing: 4) {
            // The figure's element is a little larger than its glyphs, the layout
            // unchanged (`WholeElement` over the figure and a margin): cut to the glyphs
            // alone, a one-digit figure ("4", "8") was too small a sample for the Mac's
            // contrast audit to tell its colour from the page's.
            Text(value)
                .font(MGWebFont.display(24, .title).weight(.black))
                .mgTracking(-0.02, size: 24)
                .lineLimit(1)
                .minimumScaleFactor(0.6)
                .foregroundStyle(accent?.text ?? MGWeb.textHeading)
                .padding(.vertical, 2)
                .padding(.trailing, 10)
                .modifier(WholeElement(label: Self.labelSpoken ? value : "\(label.capitalized): \(value)",
                                       traits: .isStaticText, identifier: "", action: nil))
                .padding(.vertical, -2)
                .padding(.trailing, -10)
            Text(label)
                .font(MGWebFont.mono(11))
                .mgTracking(0.14, size: MGWebFont.monoSize(11))
                .foregroundStyle(MGWeb.textFaint)
                .accessibilityHidden(!Self.labelSpoken)
        }
    }
}

/// `SectionLabel`: "05 // TEXT", the numeral in the accent, mono 11 tracked 0.14 em.
public struct WebSectionLabel: View {
    let number: Int
    let text: String
    let accent: MGWebAccent
    let muted: Bool

    /// `muted`: the numeral in the faint colour too (the web's `muted` prop).
    public init(number: Int, _ text: String, accent: MGWebAccent, muted: Bool = false) {
        self.number = number
        self.text = text
        self.accent = accent
        self.muted = muted
    }

    public var body: some View {
        (Text(String(format: "%02d //  ", number)).foregroundStyle(muted ? MGWeb.textFaint : accent.baseText)
            + Text(text.uppercased()).foregroundStyle(MGWeb.textMuted))
            .font(MGWebFont.mono(11))
            .mgTracking(0.14, size: MGWebFont.monoSize(11))
            .accessibilityAddTraits(.isHeader)
    }
}

/// `ProvenanceTag`: "[FR-H-4]", mono 11 on an inset tag.
public struct WebProvenanceTag: View {
    let id: String
    let title: String

    public init(_ id: String, title: String) {
        self.id = id
        self.title = title
    }

    public var body: some View {
        Text("[\(id)]")
            .font(MGWebFont.mono(11))
            .mgTracking(0.05, size: MGWebFont.monoSize(11))
            .foregroundStyle(id.hasPrefix("NFR") ? MGWeb.infoText : MGWeb.textBody)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(RoundedRectangle(cornerRadius: MGRadius.xs).fill(MGWeb.surfaceInset))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.xs).strokeBorder(MGWeb.border))
            .help(title.isEmpty ? id : title)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(Text(title.isEmpty ? id : "\(id): \(title)"))
            .accessibilityAddTraits(.isStaticText)
    }
}

/// `HumanGate`: pending (amber outline, Authorise and Reject), authorised (green, with
/// the audit id) or rejected (neutral).
public struct WebHumanGate: View {
    public enum State: Equatable {
        case pending
        /// Waiting on the enclosure (queued offline, or in flight).
        case queued
        case authorised(auditId: String?)
        case rejected
    }

    let title: String
    let detail: String
    let state: State
    let accent: MGWebAccent
    let busy: Bool
    let onAuthorise: () -> Void
    let onReject: () -> Void
    /// The identifiers' prefix: `<prefix>.authorise`, `<prefix>.reject`, …
    let idPrefix: String
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(title: String, detail: String, state: State, accent: MGWebAccent, busy: Bool,
                onAuthorise: @escaping () -> Void, onReject: @escaping () -> Void, idPrefix: String = "mg.heimdall") {
        self.idPrefix = idPrefix
        self.title = title
        self.detail = detail
        self.state = state
        self.accent = accent
        self.busy = busy
        self.onAuthorise = onAuthorise
        self.onReject = onReject
    }

    private var border: Color {
        switch state {
        case .authorised: return MGWeb.secure
        case .rejected: return MGWeb.borderStrong
        case .pending, .queued: return MGWeb.warn
        }
    }

    public var body: some View {
        Group {
            switch state {
            case let .authorised(auditId):
                HStack(spacing: 9) {
                    Image(systemName: "checkmark.shield").foregroundStyle(MGWeb.secureText).accessibilityHidden(true)
                    Text("AUTHORISED · SIGNED" + (auditId.map { " · AUDIT #\($0)" } ?? ""))
                        .font(MGWebFont.mono(10))
                        .mgTracking(0.08, size: MGWebFont.monoSize(10))
                        .foregroundStyle(MGWeb.secureText)
                        .accessibilityIdentifier("\(idPrefix).gate.authorised")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            case .rejected:
                Text("REJECTED · RECORDED")
                    .font(MGWebFont.mono(10))
                    .mgTracking(0.08, size: MGWebFont.monoSize(10))
                    .foregroundStyle(MGWeb.textFaint)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityIdentifier("\(idPrefix).gate.rejected")
            case .pending, .queued:
                pending
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceInset))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(border))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Human gate"))
    }

    private var pending: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "bolt").foregroundStyle(MGWeb.warnText).accessibilityHidden(true)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(MGWeb.textHeading)
                    Text(state == .queued ? "QUEUED · SENT WHEN THE ENCLOSURE IS REACHABLE" : detail)
                        .font(MGWebFont.mono(9))
                        .mgTracking(0.1, size: MGWebFont.monoSize(9))
                        .foregroundStyle(MGWeb.warnText)
                }
            }
            (typeSize.isAccessibilitySize
                ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
                : AnyLayout(HStackLayout(spacing: 8))) {
                Button(action: onAuthorise) {
                    Text("Authorise")
                        .font(MGWebFont.sans(12, .caption).weight(.semibold))
                        .foregroundStyle(Color(mgHex: MGContrast.ink(on: accent.hex)))
                        .padding(.horizontal, 14)
                        .frame(minWidth: 44, minHeight: 32)
                        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(accent.fill))
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(busy || state == .queued)
                .accessibilityIdentifier("\(idPrefix).authorise")
                Button(action: onReject) {
                    Text("Reject")
                        .font(MGWebFont.sans(12, .caption).weight(.semibold))
                        .foregroundStyle(MGWeb.textMuted)
                        .padding(.horizontal, 10)
                        .frame(minWidth: 44, minHeight: 32)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .disabled(busy)
                .accessibilityIdentifier("\(idPrefix).reject")
            }
        }
    }
}

/// A `Card` (size sm): 14 pt padding on the card surface, 10 pt corners, 1 pt border.
struct WebCard<Content: View>: View {
    var borderColor: Color = MGWeb.border
    @ViewBuilder var content: Content

    var body: some View {
        content
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(borderColor))
    }
}

/// `SideNav`: the console's sections, mono 11 titles, 34 pt items with an icon, the
/// label (sans 14) and a mono badge; the selected item on the accent tint with a 2 pt
/// accent rule. Items carry the reading-order identifiers the audit counts.
public struct WebSideNavItem: Identifiable {
    public let id: String
    public let label: String
    public let symbol: String
    public let badge: String?
    public let identifier: String
    public let selected: Bool
    public let action: () -> Void

    public init(id: String, label: String, symbol: String, badge: String?, identifier: String, selected: Bool,
                action: @escaping () -> Void) {
        self.id = id
        self.label = label
        self.symbol = symbol
        self.badge = badge
        self.identifier = identifier
        self.selected = selected
        self.action = action
    }
}

public struct WebSideNavSection: Identifiable {
    public let title: String
    public let items: [WebSideNavItem]
    public var id: String { title }

    public init(title: String, items: [WebSideNavItem]) {
        self.title = title
        self.items = items
    }
}

public struct WebSideNav<Footer: View>: View {
    let rail: WebSideNavSection?
    let sections: [WebSideNavSection]
    let accent: MGWebAccent
    let footer: Footer

    /// `rail`, when given, is drawn first as one row of icon tiles (the web's collapsed
    /// navigation rail, `IconButton`s): the app's console switcher, which the web suite,
    /// one console per page, does not have.
    public init(rail: WebSideNavSection? = nil, sections: [WebSideNavSection], accent: MGWebAccent,
                @ViewBuilder footer: () -> Footer) {
        self.rail = rail
        self.sections = sections
        self.accent = accent
        self.footer = footer()
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            if let rail {
                VStack(alignment: .leading, spacing: 2) {
                    sectionTitle(rail.title)
                    HStack(spacing: 1) {
                        ForEach(rail.items) { item in tile(item) }
                    }
                }
            }
            ForEach(sections) { section in
                VStack(alignment: .leading, spacing: 2) {
                    sectionTitle(section.title)
                    ForEach(section.items) { item in row(item) }
                }
            }
            Spacer(minLength: 0)
            footer
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 12)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(MGWeb.bgSunken)
    }

    private func sectionTitle(_ title: String) -> some View {
        Text(title)
            .font(MGWebFont.mono(11))
            .mgTracking(0.14, size: MGWebFont.monoSize(11))
            .foregroundStyle(MGWeb.textFaint)
            .padding(.horizontal, 10)
            .padding(.bottom, 6)
            .accessibilityAddTraits(.isHeader)
    }

    private func tile(_ item: WebSideNavItem) -> some View {
        Button(action: item.action) {
            Image(systemName: item.symbol)
                .font(MGWebFont.sans(15))
                .foregroundStyle(item.selected ? accent.text : MGWeb.textMuted)
                .frame(width: 34, height: 34)
                .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(item.selected ? accent.tint : Color.clear))
                .contentShape(Rectangle())
                .accessibilityHidden(true)
        }
        .buttonStyle(.plain)
        .help(item.label)
        .accessibilityLabel(Text(item.label))
        .accessibilityAddTraits(item.selected ? [.isSelected] : [])
        .accessibilityIdentifier(item.identifier)
    }

    private func row(_ item: WebSideNavItem) -> some View {
        Button(action: item.action) {
            HStack(spacing: 10) {
                Image(systemName: item.symbol)
                    .font(MGWebFont.sans(14))
                    .frame(width: 16)
                    .accessibilityHidden(true)
                Text(item.label)
                    .font(MGWebFont.sans(14).weight(item.selected ? .semibold : .regular))
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                Spacer(minLength: 4)
                if let badge = item.badge {
                    Text(badge)
                        .font(MGWebFont.mono(11))
                        .foregroundStyle(item.selected ? accent.text : MGWeb.textFaint)
                }
            }
            .foregroundStyle(item.selected ? MGWeb.textHeading : MGWeb.textMuted)
            .padding(.horizontal, 10)
            .frame(maxWidth: .infinity, minHeight: 34, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(item.selected ? accent.tint : Color.clear))
            .overlay(alignment: .leading) {
                if item.selected { Rectangle().fill(accent.fill).frame(width: 2) }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text(item.badge.map { "\(item.label), \($0)" } ?? item.label))
        .accessibilityAddTraits(item.selected ? [.isSelected] : [])
        .accessibilityIdentifier(item.identifier)
    }
}

/// `Switch` with its label: the LAYERS toggles. A system toggle in the switch style,
/// tinted with the accent, so it is a real switch to VoiceOver and the keyboard.
public struct WebSwitch: View {
    let label: String
    @Binding var isOn: Bool
    let accent: MGWebAccent
    let identifier: String

    public init(_ label: String, isOn: Binding<Bool>, accent: MGWebAccent, identifier: String) {
        self.label = label
        self._isOn = isOn
        self.accent = accent
        self.identifier = identifier
    }

    public var body: some View {
        // The switch first, then its label, as the web lays it out; the label stays the
        // toggle's own (for VoiceOver), drawn beside it.
        HStack(spacing: 10) {
            Toggle(isOn: $isOn) { Text(label) }
                .labelsHidden()
                .toggleStyle(.switch)
                .tint(accent.fill)
                .accessibilityIdentifier(identifier)
            Text(label)
                .font(MGWebFont.sans(14))
                .foregroundStyle(MGWeb.textBody)
                .accessibilityHidden(true)
        }
    }
}

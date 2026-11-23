import MidgardCore
import SwiftUI

// More of the web design system's components (`ui/design-system/src/components/`),
// drawn natively at the web's sizes for the screens matched to the web suite: the data
// components (Table, Terminal, MeterBar, Card with its accent rule), the form controls
// (Button, Select, Input, Tabs, Tag) and the console's label styles. Each names the
// component it follows. On iOS every control is at least 44 pt tall and every mono size
// at least 11 pt (`MGWebFont.mono`). [DD-7] [RC-49] [AC-C-11]

/// The smallest target a control gets: the web's own height on the Mac, 44 pt on iOS.
func webTarget(_ web: CGFloat) -> CGFloat {
    #if os(iOS)
    return max(web, 44)
    #else
    return web
    #endif
}

/// `.mg-label`: mono 11, tracked 0.14 em, upper case, in the faint text colour.
public struct WebLabel: View {
    let text: String
    let colour: Color

    public init(_ text: String, colour: Color = MGWeb.textFaint) {
        self.text = text
        self.colour = colour
    }

    public var body: some View {
        Text(text.uppercased())
            .font(MGWebFont.mono(11))
            .mgTracking(0.14, size: MGWebFont.monoSize(11))
            .foregroundStyle(colour)
            .fixedSize(horizontal: false, vertical: true)
            .accessibilityAddTraits(.isHeader)
    }
}

/// The web's `Icon` names (lucide) as SF Symbols.
public enum WebIcon {
    public static func symbol(_ lucide: String) -> String {
        switch lucide {
        case "boxes": return "shippingbox"
        case "filter": return "line.3.horizontal.decrease"
        case "search": return "magnifyingglass"
        case "shield-check": return "checkmark.shield"
        case "scroll-text": return "scroll"
        case "fingerprint": return "touchid"
        case "key": return "key"
        case "server": return "server.rack"
        case "code": return "chevron.left.forwardslash.chevron.right"
        case "activity": return "waveform.path.ecg"
        case "refresh-cw": return "arrow.triangle.2.circlepath"
        case "network": return "point.3.connected.trianglepath.dotted"
        case "zap": return "bolt"
        case "plus": return "plus"
        case "arrow-right": return "arrow.right"
        case "x": return "xmark"
        case "check": return "checkmark"
        case "play": return "play"
        case "target": return "scope"
        case "download": return "arrow.down.to.line"
        case "user": return "person"
        case "bot": return "cpu"
        case "cpu": return "cpu"
        case "clock": return "clock"
        case "git-branch": return "arrow.triangle.branch"
        case "git-commit": return "smallcircle.filled.circle"
        case "gauge": return "gauge.with.dots.needle.33percent"
        case "users": return "person.2"
        case "alert-triangle": return "exclamationmark.triangle"
        case "sparkles": return "sparkles"
        case "undo-2": return "arrow.uturn.backward"
        default: return "square"
        }
    }
}

/// `Button`: primary (accent fill), secondary (card), outline (accent rule) or ghost;
/// sm 30 pt or md 38 pt tall (44 pt on iOS); a disabled button on the inset surface in
/// the faint colour, with its reason as the help text and the accessibility hint.
public struct WebButton: View {
    public enum Variant { case primary, secondary, outline, ghost }
    public enum Size { case sm, md }

    let title: String
    let symbol: String?
    let trailingSymbol: String?
    let variant: Variant
    let size: Size
    let fullWidth: Bool
    let disabledReason: String?
    let accent: MGWebAccent
    let identifier: String?
    let action: () -> Void

    public init(_ title: String, symbol: String? = nil, trailingSymbol: String? = nil, variant: Variant = .primary,
                size: Size = .md, fullWidth: Bool = false, disabledReason: String? = nil, accent: MGWebAccent = .midgard,
                identifier: String? = nil, action: @escaping () -> Void = {}) {
        self.title = title
        self.symbol = symbol
        self.trailingSymbol = trailingSymbol
        self.variant = variant
        self.size = size
        self.fullWidth = fullWidth
        self.disabledReason = disabledReason
        self.accent = accent
        self.identifier = identifier
        self.action = action
    }

    private var disabled: Bool { disabledReason != nil }

    private var colours: (fg: Color, bg: Color, border: Color) {
        if disabled { return (MGWeb.textFaint, MGWeb.surfaceInset, MGWeb.border) }
        switch variant {
        case .primary: return (Color(mgHex: MGContrast.ink(on: accent.hex)), accent.fill, .clear)
        case .secondary: return (MGWeb.textHeading, MGWeb.surfaceCard, MGWeb.border)
        case .outline: return (accent.text, .clear, accent.fill)
        case .ghost: return (MGWeb.textBody, .clear, .clear)
        }
    }

    public var body: some View {
        let c = colours
        let fontSize: CGFloat = size == .sm ? 12 : 14
        Button(action: action) {
            HStack(spacing: size == .sm ? 7 : 8) {
                if let symbol {
                    Image(systemName: symbol).font(MGWebFont.sans(fontSize + 1).weight(.semibold)).accessibilityHidden(true)
                }
                Text(title)
                    .font(MGWebFont.sans(fontSize, size == .sm ? .caption : .callout).weight(.semibold))
                    .lineLimit(1)
                    .minimumScaleFactor(0.8)
                if let trailingSymbol {
                    Image(systemName: trailingSymbol).font(MGWebFont.sans(fontSize).weight(.semibold)).accessibilityHidden(true)
                }
            }
            .foregroundStyle(c.fg)
            .padding(.horizontal, size == .sm ? 13 : 18)
            .frame(maxWidth: fullWidth ? .infinity : nil, minHeight: webTarget(size == .sm ? 30 : 38))
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(c.bg))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(c.border))
            .contentShape(Rectangle())
        }
        .buttonStyle(WebPressStyle())
        .disabled(disabled)
        .help(disabledReason ?? title)
        .accessibilityHint(Text(disabledReason ?? ""))
        .modifier(OptionalIdentifier(identifier))
    }
}

struct OptionalIdentifier: ViewModifier {
    let identifier: String?
    init(_ identifier: String?) { self.identifier = identifier }

    func body(content: Content) -> some View {
        if let identifier { content.accessibilityIdentifier(identifier) } else { content }
    }
}

/// `Select` with its label above: the label in `.mg-label` style, then the chosen
/// option on the card surface with a chevron, opening a menu of the options.
public struct WebSelect: View {
    let label: String?
    let options: [(value: String, label: String)]
    @Binding var selection: String
    let identifier: String
    let width: CGFloat?
    let fullWidth: Bool

    public init(_ label: String?, options: [(value: String, label: String)], selection: Binding<String>,
                identifier: String, width: CGFloat? = nil, fullWidth: Bool = false) {
        self.label = label
        self.options = options
        self._selection = selection
        self.identifier = identifier
        self.width = width
        self.fullWidth = fullWidth
    }

    private var current: String { options.first { $0.value == selection }?.label ?? options.first?.label ?? "" }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let label { WebLabel(label, colour: MGWeb.textMuted).accessibilityHidden(true) }
            Menu {
                ForEach(options, id: \.value) { option in
                    Button {
                        selection = option.value
                    } label: {
                        if option.value == selection {
                            Label(option.label, systemImage: "checkmark")
                        } else {
                            Text(option.label)
                        }
                    }
                }
            } label: {
                HStack(spacing: 8) {
                    Text(current)
                        .font(MGWebFont.sans(14))
                        .foregroundStyle(MGWeb.textHeading)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Image(systemName: "chevron.down")
                        .font(MGWebFont.sans(11).weight(.semibold))
                        .foregroundStyle(MGWeb.textMuted)
                        .accessibilityHidden(true)
                }
                .padding(.horizontal, 12)
                .frame(minWidth: width ?? 120, maxWidth: fullWidth ? .infinity : width, minHeight: webTarget(32))
                .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
                .contentShape(Rectangle())
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .menuIndicator(.hidden)
            .fixedSize(horizontal: width == nil && !fullWidth, vertical: false)
            .accessibilityLabel(Text(label.map { "\($0): \(current)" } ?? current))
            .accessibilityIdentifier(identifier)
        }
    }
}

/// `Input` (mono, sm) with its optional label above and an optional leading symbol.
public struct WebTextField: View {
    let label: String?
    let placeholder: String
    @Binding var text: String
    let symbol: String?
    let identifier: String
    let width: CGFloat?
    let onSubmit: () -> Void

    public init(_ label: String?, placeholder: String, text: Binding<String>, symbol: String? = nil,
                identifier: String, width: CGFloat? = nil, onSubmit: @escaping () -> Void = {}) {
        self.label = label
        self.placeholder = placeholder
        self._text = text
        self.symbol = symbol
        self.identifier = identifier
        self.width = width
        self.onSubmit = onSubmit
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            if let label { WebLabel(label, colour: MGWeb.textMuted).accessibilityHidden(true) }
            HStack(spacing: 8) {
                if let symbol {
                    Image(systemName: symbol).font(MGWebFont.sans(13)).foregroundStyle(MGWeb.textMuted)
                        .accessibilityHidden(true)
                }
                TextField(text: $text, prompt: Text(placeholder).foregroundStyle(MGWeb.textMuted)) {
                    Text(label ?? placeholder)
                }
                .textFieldStyle(.plain)
                .font(MGWebFont.mono(13, .callout))
                .foregroundStyle(MGWeb.textHeading)
                .autocorrectionDisabled()
                #if os(iOS)
                .textInputAutocapitalization(.never)
                #endif
                .onSubmit(onSubmit)
                .accessibilityLabel(Text(label ?? placeholder))
                .accessibilityIdentifier(identifier)
            }
            .padding(.horizontal, 12)
            .frame(width: width, alignment: .leading)
            .frame(maxWidth: width == nil ? .infinity : nil, minHeight: webTarget(32), alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
        }
    }
}

/// `Tabs`: underline tabs, sans 14 medium, a 2 pt accent rule under the active one,
/// with an optional mono badge in the accent.
public struct WebTabs: View {
    let tabs: [(label: String, badge: String?)]
    @Binding var selection: Int
    let accent: MGWebAccent
    let identifier: String

    public init(_ tabs: [(label: String, badge: String?)], selection: Binding<Int>, accent: MGWebAccent = .midgard,
                identifier: String) {
        self.tabs = tabs
        self._selection = selection
        self.accent = accent
        self.identifier = identifier
    }

    public var body: some View {
        HStack(alignment: .bottom, spacing: 4) {
            ForEach(Array(tabs.enumerated()), id: \.offset) { index, tab in
                let active = index == selection
                Button {
                    selection = index
                } label: {
                    HStack(spacing: 7) {
                        Text(tab.label)
                            .font(MGWebFont.sans(14).weight(.medium))
                            .foregroundStyle(active ? MGWeb.textHeading : MGWeb.textMuted)
                        if let badge = tab.badge {
                            Text(badge).font(MGWebFont.mono(12, .caption).weight(.semibold)).foregroundStyle(accent.text)
                        }
                    }
                    .padding(.horizontal, 16)
                    .frame(minHeight: webTarget(40))
                    .overlay(alignment: .bottom) {
                        Rectangle().fill(active ? accent.fill : .clear).frame(height: 2)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(Text(tab.badge.map { "\(tab.label), \($0)" } ?? tab.label))
                .accessibilityAddTraits(active ? [.isSelected] : [])
                .accessibilityIdentifier("\(identifier).\(index)")
            }
            Spacer(minLength: 0)
        }
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.border).frame(height: 1) }
    }
}

/// `Tag`: a mono 12 pill, the accent tint and rule when active.
public struct WebTag: View {
    let text: String
    let active: Bool
    let accent: MGWebAccent
    let identifier: String?
    let action: (() -> Void)?

    public init(_ text: String, active: Bool = false, accent: MGWebAccent = .midgard, identifier: String? = nil,
                action: (() -> Void)? = nil) {
        self.text = text
        self.active = active
        self.accent = accent
        self.identifier = identifier
        self.action = action
    }

    private var pill: some View {
        Text(text)
            .font(MGWebFont.mono(12, .caption))
            .mgTracking(0.03, size: MGWebFont.monoSize(12))
            .lineLimit(1)
            .foregroundStyle(active ? accent.text : MGWeb.textBody)
            .padding(.horizontal, 10)
            .padding(.vertical, 4)
            .background(Capsule().fill(active ? accent.tint : MGWeb.surfaceInset))
            .overlay(Capsule().strokeBorder(active ? accent.fill : MGWeb.border))
    }

    public var body: some View {
        if let action {
            Button(action: action) {
                pill.frame(minHeight: webTarget(26)).contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityAddTraits(active ? [.isSelected] : [])
            .modifier(OptionalIdentifier(identifier))
        } else {
            pill
        }
    }
}

/// `Terminal`: mono 12 lines on the sunken well, an optional title bar with the accent
/// dot; `> ` lines start with the accent prompt, `#` lines are faint.
public struct WebTerminal: View {
    let title: String?
    let lines: [String]
    let accent: MGWebAccent
    let small: Bool

    public init(_ title: String? = nil, lines: [String], accent: MGWebAccent = .midgard, small: Bool = false) {
        self.title = title
        self.lines = lines
        self.accent = accent
        self.small = small
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            if let title {
                HStack(spacing: 8) {
                    Circle().fill(accent.fill).frame(width: 8, height: 8).accessibilityHidden(true)
                    Text(title.uppercased())
                        .font(MGWebFont.mono(11))
                        .mgTracking(0.14, size: MGWebFont.monoSize(11))
                        .foregroundStyle(MGWeb.textFaint)
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(MGWeb.surfaceInset)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.border).frame(height: 1) }
            }
            VStack(alignment: .leading, spacing: small ? 2 : 5) {
                ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                    lineView(line)
                }
            }
            .font(MGWebFont.mono(small ? 10 : 12, .caption))
            .padding(.horizontal, small ? 12 : 16)
            .padding(.vertical, small ? 10 : 14)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(MGWeb.bgSunken)
        .clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(([title].compactMap { $0 } + lines).joined(separator: ". ")))
        .accessibilityAddTraits(.isStaticText)
    }

    @ViewBuilder private func lineView(_ line: String) -> some View {
        if line.hasPrefix("> ") {
            (Text(">").foregroundStyle(accent.text) + Text(line.dropFirst()).foregroundStyle(MGWeb.grey300))
                .fixedSize(horizontal: false, vertical: true)
        } else {
            Text(line.isEmpty ? " " : line)
                .foregroundStyle(line.hasPrefix("#") ? MGWeb.textFaint : MGWeb.grey300)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// `MeterBar`: a 5 pt bar on the inset surface, filled in the accent (or the warn
/// colour from `warnAt`), and the value in mono 12.
public struct WebMeterBar: View {
    let value: Double
    let width: CGFloat
    let warnAt: Double?
    let format: (Double) -> String
    let accent: MGWebAccent

    public init(_ value: Double, width: CGFloat = 70, warnAt: Double? = nil, accent: MGWebAccent = .midgard,
                format: @escaping (Double) -> String = { "\(Int($0.rounded()))%" }) {
        self.value = min(max(value, 0), 100)
        self.width = width
        self.warnAt = warnAt
        self.format = format
        self.accent = accent
    }

    public var body: some View {
        let colour: Color = {
            guard let warnAt else { return accent.fill }
            return value >= warnAt ? MGWeb.warn : MGWeb.secure
        }()
        HStack(spacing: 8) {
            ZStack(alignment: .leading) {
                Capsule().fill(MGWeb.surfaceInset)
                Capsule().fill(colour).frame(width: width * value / 100)
            }
            .frame(width: width, height: 5)
            .accessibilityHidden(true)
            Text(format(value))
                .font(MGWebFont.mono(12, .caption))
                .foregroundStyle(MGWeb.textBody)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text(format(value)))
    }
}

/// `Card`: the card surface, 10 pt corners and a 1 pt border, padded 14 (sm) or 18
/// (md); with `accentRule` a 2 pt accent rule along the top; `action` makes it a
/// button (the web's `interactive`).
public struct WebPanel<Content: View>: View {
    let padding: CGFloat
    let accentRule: Bool
    let accent: MGWebAccent
    let identifier: String?
    let label: String?
    let action: (() -> Void)?
    let content: Content

    public init(padding: CGFloat = 14, accentRule: Bool = false, accent: MGWebAccent = .midgard, identifier: String? = nil,
                label: String? = nil, action: (() -> Void)? = nil, @ViewBuilder content: () -> Content) {
        self.padding = padding
        self.accentRule = accentRule
        self.accent = accent
        self.identifier = identifier
        self.label = label
        self.action = action
        self.content = content()
    }

    private var card: some View {
        content
            .padding(padding)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
            .overlay(alignment: .top) {
                if accentRule {
                    UnevenRoundedRectangle(topLeadingRadius: MGRadius.md, topTrailingRadius: MGRadius.md)
                        .fill(accent.fill).frame(height: 2)
                        .accessibilityHidden(true)
                }
            }
            .clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
    }

    public var body: some View {
        if let action {
            Button(action: action) { card.contentShape(Rectangle()) }
                .buttonStyle(.plain)
                .accessibilityElement(children: .combine)
                .modifier(OptionalLabel(label))
                .accessibilityAddTraits(accentRule ? [.isSelected] : [])
                .modifier(OptionalIdentifier(identifier))
        } else {
            card.modifier(OptionalIdentifier(identifier))
        }
    }
}

struct OptionalLabel: ViewModifier {
    let label: String?
    init(_ label: String?) { self.label = label }

    func body(content: Content) -> some View {
        if let label { content.accessibilityLabel(Text(label)) } else { content }
    }
}

/// The page header every console screen opens with: `SectionLabel` and the `h1`
/// (Oxanium 24) on the left, then the stat blocks and the primary button, wrapping
/// as the web's `flex-wrap` does.
public struct WebPageHeader<Trailing: View>: View {
    let number: Int
    let section: String
    let title: String
    let sampleData: Bool
    let accent: MGWebAccent
    let trailing: Trailing

    public init(number: Int, section: String, title: String, sampleData: Bool = false, accent: MGWebAccent = .midgard,
                @ViewBuilder trailing: () -> Trailing) {
        self.number = number
        self.section = section
        self.title = title
        self.sampleData = sampleData
        self.accent = accent
        self.trailing = trailing()
    }

    private var heading: some View {
        VStack(alignment: .leading, spacing: 6) {
            WebSectionLabel(number: number, section, accent: accent)
            HStack(spacing: 10) {
                Text(title)
                    .font(MGWebFont.display(24, .title).weight(.bold))
                    .foregroundStyle(MGWeb.textHeading)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityAddTraits(.isHeader)
                if sampleData { WebSampleDataBadge() }
            }
        }
    }

    public var body: some View {
        // The web's `flex: 1` spacer between the heading and the figures, while they fit
        // on one line; wrapped under it when they do not.
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .bottom, spacing: 24) {
                heading.fixedSize()
                Spacer(minLength: 0)
                HStack(alignment: .bottom, spacing: 24) { trailing }.fixedSize()
            }
            FlowRow(spacing: 18) {
                heading.padding(.trailing, 24)
                trailing
            }
            .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}

/// `SampleData`: the warn badge a page of fixed example figures carries.
public struct WebSampleDataBadge: View {
    public init() {}

    public var body: some View {
        WebBadge("SAMPLE DATA", tone: .warn)
            .help("Fixed example data served by the gateway, not this enclosure's own. The subsystem behind this page is not connected yet.")
            .accessibilityLabel(Text("Sample data: fixed example figures, not this enclosure's own"))
    }
}

/// Key and value lines, mono 12, the key faint in a fixed column, a divider under each.
public struct WebKeyValues: View {
    let rows: [(String, String)]
    let keyWidth: CGFloat
    let upperKeys: Bool

    public init(_ rows: [(String, String)], keyWidth: CGFloat = 104, upperKeys: Bool = false) {
        self.rows = rows
        self.keyWidth = keyWidth
        self.upperKeys = upperKeys
    }

    @Environment(\.dynamicTypeSize) private var typeSize

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                let layout = typeSize.isAccessibilitySize
                    ? AnyLayout(VStackLayout(alignment: .leading, spacing: 2))
                    : AnyLayout(HStackLayout(alignment: .firstTextBaseline, spacing: 10))
                layout {
                    Text(upperKeys ? row.0.uppercased() : row.0)
                        .foregroundStyle(MGWeb.textFaint)
                        .frame(width: typeSize.isAccessibilitySize ? nil : keyWidth, alignment: .leading)
                    Text(row.1)
                        .foregroundStyle(MGWeb.grey100)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: 0)
                }
                .font(MGWebFont.mono(12, .caption))
                .padding(.vertical, 7)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
                .accessibilityElement(children: .combine)
            }
        }
    }
}

/// A list of checked statements (the inspectors' "EVALUATED AT", "EVERY CALL").
public struct WebCheckList: View {
    let items: [String]

    public init(_ items: [String]) { self.items = items }

    public var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(items, id: \.self) { item in
                HStack(spacing: 8) {
                    Image(systemName: "checkmark").font(MGWebFont.sans(11).weight(.bold)).foregroundStyle(MGWeb.secureText)
                        .accessibilityHidden(true)
                    // Its element a little larger than the glyphs (the Mac's contrast
                    // audit misread a line cut to its text) and in the brighter grey.
                    Text(item).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.grey100)
                        .padding(.vertical, 2)
                        .padding(.trailing, 6)
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel(Text(item))
                        .accessibilityAddTraits(.isStaticText)
                        .padding(.vertical, -2)
                        .padding(.trailing, -6)
                    Spacer(minLength: 0)
                }
                .padding(.vertical, 6)
                .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.divider).frame(height: 1) }
            }
        }
    }
}

/// A status line (the web's `role="status"` / `role="alert"` boxes): mono 11 on the
/// inset surface with a rule in the tone's colour.
public struct WebNotice: View {
    public enum Tone { case secure, danger, warn, neutral }
    let text: String
    let tone: Tone
    let identifier: String?

    public init(_ text: String, tone: Tone, identifier: String? = nil) {
        self.text = text
        self.tone = tone
        self.identifier = identifier
    }

    private var colour: Color {
        switch tone {
        case .secure: return MGWeb.secureText
        case .danger: return MGWeb.dangerText
        case .warn: return MGWeb.warnText
        case .neutral: return MGWeb.textMuted
        }
    }

    public var body: some View {
        Text(text)
            .font(MGWebFont.mono(11))
            .mgTracking(0.06, size: MGWebFont.monoSize(11))
            .foregroundStyle(colour)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 12)
            .padding(.vertical, 10)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceInset))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(tone == .neutral ? MGWeb.border : colour))
            .accessibilityAddTraits(.isStaticText)
            .modifier(OptionalIdentifier(identifier))
    }
}

/// A faint mono line: loading states and footers (`LOADING…`).
public struct WebMicro: View {
    let text: String
    let tracking: CGFloat

    public init(_ text: String, tracking: CGFloat = 0) {
        self.text = text
        self.tracking = tracking
    }

    public var body: some View {
        Text(text)
            .font(MGWebFont.mono(11))
            .mgTracking(tracking, size: MGWebFont.monoSize(11))
            .foregroundStyle(MGWeb.textFaint)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// One column of a `WebTable`.
public struct WebColumn<Row> {
    public enum Align { case leading, trailing }
    let header: String
    let width: CGFloat?
    let align: Align
    let mono: Bool
    let text: (Row) -> String
    let cell: ((Row) -> AnyView)?

    /// A text column (sans 14, or mono 12).
    public init(_ header: String, width: CGFloat? = nil, align: Align = .leading, mono: Bool = false,
                text: @escaping (Row) -> String) {
        self.header = header
        self.width = width
        self.align = align
        self.mono = mono
        self.text = text
        self.cell = nil
    }

    /// A column drawn by `cell`; `text` is what it says to VoiceOver and in the list form.
    public init<V: View>(_ header: String, width: CGFloat? = nil, align: Align = .leading, text: @escaping (Row) -> String,
                         @ViewBuilder cell: @escaping (Row) -> V) {
        self.header = header
        self.width = width
        self.align = align
        self.mono = false
        self.text = text
        self.cell = { AnyView(cell($0)) }
    }
}

/// `Table`: mono upper-case headers on the inset surface, 40 pt rows (32 compact), the
/// selected row on the accent tint with a 2 pt accent rule. As a list (`asList`: the
/// iPhone, and any width at accessibility text sizes) each row is one button: its first
/// column, then every other column under it as "HEADER value". Each row is one element
/// to VoiceOver, saying every column.
public struct WebTable<Row>: View {
    let columns: [WebColumn<Row>]
    let rows: [Row]
    let id: (Row) -> String
    let selected: Int?
    let compact: Bool
    let minWidth: CGFloat
    let asList: Bool
    let accent: MGWebAccent
    let identifier: String
    let onSelect: ((Int) -> Void)?

    @Environment(\.dynamicTypeSize) private var typeSize

    public init(_ columns: [WebColumn<Row>], rows: [Row], id: @escaping (Row) -> String, selected: Int? = nil,
                compact: Bool = false, minWidth: CGFloat = 0, asList: Bool = false, accent: MGWebAccent = .midgard,
                identifier: String, onSelect: ((Int) -> Void)? = nil) {
        self.columns = columns
        self.rows = rows
        self.id = id
        self.selected = selected
        self.compact = compact
        self.minWidth = minWidth
        self.asList = asList
        self.accent = accent
        self.identifier = identifier
        self.onSelect = onSelect
    }

    private var list: Bool { asList || typeSize.isAccessibilitySize }

    /// The least a column without a width gets: the web's widths are hints a browser
    /// grows a column past to fit its content.
    static var flexibleMinimum: CGFloat { 120 }

    /// The width the table needs: the web's `minWidth`, or its fixed columns and the
    /// least for each other column, whichever is more.
    private var needed: CGFloat {
        let fixed = columns.compactMap(\.width).reduce(0, +)
        let flexible = CGFloat(columns.filter { $0.width == nil }.count) * Self.flexibleMinimum
        return max(minWidth, fixed + flexible)
    }

    /// Scrolled sideways, the room the table takes: each column without a width gets
    /// enough for most content, as a browser's wider-than-its-hints table does.
    private var scrolled: CGFloat {
        let fixed = columns.compactMap(\.width).reduce(0, +)
        return max(needed, fixed + CGFloat(columns.filter { $0.width == nil }.count) * 180)
    }

    public var body: some View {
        if list {
            table
        } else {
            // The web's `overflowX: auto`: scrolled sideways only when the column is
            // narrower than the table needs.
            ViewThatFits(in: .horizontal) {
                table.frame(minWidth: needed, maxWidth: .infinity)
                ScrollView(.horizontal) { table.frame(width: scrolled) }
            }
        }
    }

    private var table: some View {
        LazyVStack(spacing: 0) {
            if !list { header }
            ForEach(Array(rows.enumerated()), id: \.offset) { index, row in
                rowView(index, row)
                if index < rows.count - 1 { Rectangle().fill(MGWeb.divider).frame(height: 1).accessibilityHidden(true) }
            }
        }
        .background(MGWeb.surfaceCard)
        .clipShape(RoundedRectangle(cornerRadius: MGRadius.md))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Table: " + columns.map(\.header).joined(separator: ", ")))
        .accessibilityIdentifier(identifier)
    }

    private var header: some View {
        HStack(spacing: 0) {
            ForEach(Array(columns.enumerated()), id: \.offset) { _, column in
                frame(column) {
                    Text(column.header.uppercased())
                        .font(MGWebFont.mono(11).weight(.medium))
                        .mgTracking(0.14, size: MGWebFont.monoSize(11))
                        .foregroundStyle(MGWeb.textFaint)
                        .lineLimit(1)
                }
            }
        }
        .frame(height: 32)
        .background(MGWeb.surfaceInset)
        .overlay(alignment: .bottom) { Rectangle().fill(MGWeb.border).frame(height: 1) }
        .accessibilityHidden(true)
    }

    @ViewBuilder private func frame<V: View>(_ column: WebColumn<Row>, @ViewBuilder _ content: () -> V) -> some View {
        let alignment: Alignment = column.align == .trailing ? .trailing : .leading
        if let width = column.width {
            content().padding(.horizontal, 14).frame(width: width, alignment: alignment).clipped()
        } else {
            content().padding(.horizontal, 14).frame(maxWidth: .infinity, alignment: alignment)
        }
    }

    private func spoken(_ row: Row) -> String {
        columns.map { "\($0.header): \($0.text(row))" }.joined(separator: ", ")
    }

    @ViewBuilder private func cellView(_ column: WebColumn<Row>, _ row: Row, first: Bool) -> some View {
        if let cell = column.cell {
            if column.width != nil {
                // Chips and badges keep their one line, as the web's `nowrap` cells do.
                cell(row).fixedSize(horizontal: true, vertical: false)
            } else {
                cell(row)
            }
        } else {
            Text(column.text(row))
                .font(column.mono ? MGWebFont.mono(12, .caption) : MGWebFont.sans(14))
                .foregroundStyle(first ? MGWeb.grey100 : MGWeb.textBody)
                .lineLimit(1)
                .truncationMode(.tail)
        }
    }

    @ViewBuilder private func rowView(_ index: Int, _ row: Row) -> some View {
        let isSelected = index == selected
        let content = Group {
            if list {
                VStack(alignment: .leading, spacing: 6) {
                    if let first = columns.first { cellView(first, row, first: true) }
                    FlowRow(spacing: 10) {
                        ForEach(Array(columns.dropFirst().enumerated()), id: \.offset) { _, column in
                            HStack(spacing: 6) {
                                Text(column.header.uppercased())
                                    .font(MGWebFont.mono(11))
                                    .foregroundStyle(MGWeb.textBody)
                                if column.cell != nil {
                                    cellView(column, row, first: false)
                                } else {
                                    Text(column.text(row))
                                        .font(column.mono ? MGWebFont.mono(12, .caption) : MGWebFont.sans(14))
                                        .foregroundStyle(MGWeb.textBody)
                                        .fixedSize(horizontal: false, vertical: true)
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, 14)
                .padding(.vertical, 10)
                .frame(maxWidth: .infinity, minHeight: webTarget(40), alignment: .leading)
            } else {
                HStack(spacing: 0) {
                    ForEach(Array(columns.enumerated()), id: \.offset) { ci, column in
                        frame(column) { cellView(column, row, first: ci == 0) }
                    }
                }
                .frame(height: compact ? 32 : 40)
            }
        }
        .background(isSelected ? accent.tint : Color.clear)
        .overlay(alignment: .leading) {
            if isSelected { Rectangle().fill(accent.fill).frame(width: 2) }
        }
        .contentShape(Rectangle())
        // As a list (the iPhone), the row's own texts are not separate elements: the row
        // says them all, and its frame covers them, so a row the list's edge cuts is
        // measured as one element rather than as cut-off pieces of text.
        let drawn = content.accessibilityHidden(list)
        if let onSelect {
            Button { onSelect(index) } label: { drawn }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(spoken(row)))
                .accessibilityAddTraits(isSelected ? [.isSelected, .isButton] : [.isButton])
                .accessibilityIdentifier("\(identifier).row.\(id(row))")
        } else if list {
            drawn
                .overlay {
                    Color.clear
                        .accessibilityElement(children: .ignore)
                        .accessibilityLabel(Text(spoken(row)))
                        .accessibilityAddTraits(.isStaticText)
                        .accessibilityIdentifier("\(identifier).row.\(id(row))")
                }
        } else {
            content
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(spoken(row)))
                .accessibilityAddTraits(.isStaticText)
                .accessibilityIdentifier("\(identifier).row.\(id(row))")
        }
    }
}

/// A form's fields and its action on one line, the action at the end (the web's
/// `flex: 1` spacer before the button), or wrapped where they do not fit.
public struct WebFormRow<Fields: View, Action: View>: View {
    let fields: Fields
    let action: Action

    public init(@ViewBuilder fields: () -> Fields, @ViewBuilder action: () -> Action) {
        self.fields = fields()
        self.action = action()
    }

    public var body: some View {
        ViewThatFits(in: .horizontal) {
            HStack(alignment: .bottom, spacing: 12) {
                fields
                Spacer(minLength: 12)
                action
            }
            FlowRow(spacing: 12) {
                fields
                action
            }
        }
    }
}

/// A plain button style that leaves a disabled button's colours as the web draws them
/// (the system's dimming put its text under AA on the inset surface).
struct WebPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label.opacity(configuration.isPressed ? 0.85 : 1)
    }
}

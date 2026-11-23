import SwiftUI

// The web design system's admin components (`ui/design-system/src/components/admin/`),
// drawn natively at the web's sizes: RiskBadge, GuardedAction and BreakGlass. Tone text
// uses the AA shades `MGWeb` derives; on iOS every control is at least 44 pt and every
// mono size at least 11 pt. [FR-AD-5] [FR-AD-8] [DD-7] [AC-C-11]

/// The five risk bars (3 pt wide, `3 + i × 1.6` tall), filled up to `level` in the
/// level's tone: danger from 4, warn at 3, secure below.
public struct WebRiskBars: View {
    let level: Int

    public init(_ level: Int) { self.level = min(max(level, 1), 5) }

    static func fill(_ level: Int) -> Color {
        level >= 4 ? MGWeb.danger : level == 3 ? MGWeb.warn : MGWeb.secure
    }

    public var body: some View {
        HStack(alignment: .bottom, spacing: 2) {
            ForEach(1...5, id: \.self) { i in
                RoundedRectangle(cornerRadius: 1)
                    .fill(i <= level ? Self.fill(level) : MGWeb.surfaceRaised)
                    .frame(width: 3, height: 3 + CGFloat(i) * 1.6)
            }
        }
        .accessibilityHidden(true)
    }
}

/// `RiskBadge`: the bars, "RISK n" in the level's tone and, from risk 4 (or when told),
/// "DUAL-CONTROL", mono 11 on an inset tag.
public struct WebRiskBadge: View {
    let level: Int
    let dualControl: Bool

    /// `dualControl` nil: inferred from the level (4 and up), as the web's `null`.
    public init(_ level: Int, dualControl: Bool? = nil) {
        self.level = min(max(level, 1), 5)
        self.dualControl = dualControl ?? (self.level >= 4)
    }

    static func text(_ level: Int) -> Color {
        level >= 4 ? MGWeb.dangerText : level == 3 ? MGWeb.warnText : MGWeb.secureText
    }

    public var body: some View {
        HStack(spacing: 7) {
            WebRiskBars(level)
            Text("RISK \(level)").foregroundStyle(Self.text(level)).fontWeight(.medium)
            if dualControl { Text("DUAL-CONTROL").foregroundStyle(MGWeb.textFaint) }
        }
        .font(MGWebFont.mono(11))
        .mgTracking(0.07, size: MGWebFont.monoSize(11))
        .lineLimit(1)
        .padding(.horizontal, 9)
        .padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: MGRadius.xs).fill(MGWeb.surfaceInset))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.xs).strokeBorder(MGWeb.border))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(Text("Risk \(level)" + (dualControl ? ", dual control" : "")))
        .accessibilityAddTraits(.isStaticText)
    }
}

/// `GuardedAction`: the dual-control approval card (FR-AD-5): the action, subsystem and
/// risk bars; the mandatory justification; requester and approver with the live
/// separation-of-duties check; the state line in its tone, and, while awaiting, Deny
/// and "Approve · step-up auth".
public struct WebGuardedAction: View {
    public enum State: String { case awaiting, approved, executed, denied }

    let action: String
    let subsystem: String
    let risk: Int
    let requester: String
    let approver: String
    let state: State
    let justification: String?
    let deniedReason: String?
    let tokenTtl: String
    let accent: MGWebAccent
    let busy: Bool
    let identifier: String
    let onApprove: (() -> Void)?
    let onDeny: (() -> Void)?
    @Environment(\.dynamicTypeSize) private var typeSize

    public init(action: String, subsystem: String, risk: Int, requester: String, approver: String, state: State,
                justification: String?, deniedReason: String? = nil, tokenTtl: String = "10m", accent: MGWebAccent = .midgard,
                busy: Bool = false, identifier: String, onApprove: (() -> Void)?, onDeny: (() -> Void)?) {
        self.action = action
        self.subsystem = subsystem
        self.risk = min(max(risk, 1), 5)
        self.requester = requester
        self.approver = approver
        self.state = state
        self.justification = justification
        self.deniedReason = deniedReason
        self.tokenTtl = tokenTtl
        self.accent = accent
        self.busy = busy
        self.identifier = identifier
        self.onApprove = onApprove
        self.onDeny = onDeny
    }

    private var palette: (border: Color, text: Color, label: String) {
        switch state {
        case .awaiting: return (MGWeb.warn, MGWeb.warnText, "AWAITING SECOND APPROVER")
        case .approved: return (MGWeb.secure, MGWeb.secureText, "APPROVED · TOKEN \(tokenTtl) · SINGLE-USE")
        case .executed: return (MGWeb.secure, MGWeb.secureText, "EXECUTED · AUDITED")
        case .denied: return (MGWeb.danger, MGWeb.dangerText, deniedReason ?? "DENIED")
        }
    }

    private var violated: Bool { !requester.isEmpty && requester == approver }

    public var body: some View {
        let p = palette
        VStack(alignment: .leading, spacing: 9) {
            FlowRow(spacing: 10) {
                Text(action)
                    .font(MGWebFont.sans(14).weight(.semibold))
                    .foregroundStyle(MGWeb.textHeading)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(minWidth: 160, maxWidth: .infinity, alignment: .leading)
                HStack(spacing: 10) {
                    Text(subsystem)
                        .font(MGWebFont.mono(11))
                        .mgTracking(0.08, size: MGWebFont.monoSize(11))
                        .foregroundStyle(MGWeb.textFaint)
                    WebRiskBars(risk)
                }
            }
            if let justification {
                VStack(alignment: .leading, spacing: 3) {
                    Text("JUSTIFICATION · MANDATORY")
                        .font(MGWebFont.mono(9))
                        .mgTracking(0.1, size: MGWebFont.monoSize(9))
                        .foregroundStyle(MGWeb.textFaint)
                    Text(justification)
                        .font(MGWebFont.sans(12, .caption))
                        .foregroundStyle(MGWeb.textBody)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 7)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
                .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
                .accessibilityElement(children: .combine)
            }
            FlowRow(spacing: 14) {
                (Text("REQUESTER ").foregroundStyle(MGWeb.textFaint) + Text(requester).foregroundStyle(MGWeb.textBody))
                (Text("APPROVER ").foregroundStyle(MGWeb.textFaint)
                    + Text(approver.isEmpty ? "unassigned" : approver)
                        .foregroundStyle(approver.isEmpty ? MGWeb.textFaint : MGWeb.textBody))
                Text(violated ? "SEPARATION VIOLATED" : "SEPARATION ✓")
                    .foregroundStyle(violated ? MGWeb.dangerText : MGWeb.secureText)
            }
            .font(MGWebFont.mono(11))
            (typeSize.isAccessibilitySize
                ? AnyLayout(VStackLayout(alignment: .leading, spacing: 8))
                : AnyLayout(HStackLayout(spacing: 10))) {
                Text(p.label)
                    .font(MGWebFont.mono(11).weight(.medium))
                    .mgTracking(0.1, size: MGWebFont.monoSize(11))
                    .foregroundStyle(p.text)
                    .fixedSize(horizontal: false, vertical: true)
                    .accessibilityIdentifier("\(identifier).state")
                if !typeSize.isAccessibilitySize { Spacer(minLength: 0) }
                if state == .awaiting, let onApprove {
                    HStack(spacing: 10) {
                        Button { onDeny?() } label: {
                            Text("Deny")
                                .font(MGWebFont.sans(12, .caption).weight(.semibold))
                                .foregroundStyle(MGWeb.textBody)
                                .padding(.horizontal, 13)
                                .frame(minWidth: 44, minHeight: webTarget(28))
                                .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(WebPressStyle())
                        .disabled(busy)
                        .accessibilityLabel(Text("Deny \(action)"))
                        .accessibilityIdentifier("\(identifier).deny")
                        Button(action: onApprove) {
                            Text("Approve · step-up auth")
                                .font(MGWebFont.sans(12, .caption).weight(.semibold))
                                .foregroundStyle(Color(mgHex: MGContrast.ink(on: accent.hex)))
                                .padding(.horizontal, 13)
                                .frame(minWidth: 44, minHeight: webTarget(28))
                                .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(accent.fill))
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(WebPressStyle())
                        .disabled(busy)
                        .accessibilityLabel(Text("Approve \(action), step-up auth"))
                        .accessibilityIdentifier("\(identifier).approve")
                    }
                }
            }
        }
        .padding(.horizontal, 15)
        .padding(.vertical, 13)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(p.border))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("\(action), risk \(risk)"))
        .accessibilityIdentifier(identifier)
    }
}

/// `BreakGlass`: the emergency-elevation strip (FR-AD-8). Sealed: a dashed chip;
/// active: red, with the time left; review: amber, the post-hoc review demand; with the
/// holder, the scope and the justification.
public struct WebBreakGlass: View {
    public enum State: String { case idle, active, review }

    let state: State
    let holder: String?
    let scope: String?
    let remaining: String
    let justification: String?

    public init(state: State, holder: String?, scope: String?, remaining: String = "00:41:12", justification: String?) {
        self.state = state
        self.holder = holder
        self.scope = scope
        self.remaining = remaining
        self.justification = justification
    }

    public var body: some View {
        if state == .idle {
            HStack(spacing: 8) {
                Circle().fill(MGWeb.decorFaint).frame(width: 7, height: 7).accessibilityHidden(true)
                Text("BREAK-GLASS · SEALED · DUAL-CONTROL TO OPEN")
                    .font(MGWebFont.mono(11))
                    .mgTracking(0.08, size: MGWebFont.monoSize(11))
                    .foregroundStyle(MGWeb.textFaint)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.horizontal, 11)
            .padding(.vertical, 5)
            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm)
                .strokeBorder(MGWeb.borderStrong, style: StrokeStyle(lineWidth: 1, dash: [3, 2])))
            .accessibilityElement(children: .combine)
            .accessibilityIdentifier("mg.admin.breakglass.strip")
        } else {
            strip
        }
    }

    private var strip: some View {
        let active = state == .active
        let tone = active ? MGWeb.danger : MGWeb.warn
        let toneText = active ? MGWeb.dangerText : MGWeb.warnText
        return VStack(alignment: .leading, spacing: 7) {
            FlowRow(spacing: 10) {
                HStack(spacing: 10) {
                    Circle().fill(tone).frame(width: 9, height: 9).accessibilityHidden(true)
                    Text(active ? "BREAK-GLASS ACTIVE" : "POST-HOC REVIEW REQUIRED")
                        .font(MGWebFont.mono(12, .caption).weight(.semibold))
                        .mgTracking(0.12, size: MGWebFont.monoSize(12))
                        .foregroundStyle(toneText)
                        .accessibilityIdentifier("mg.admin.breakglass.state")
                }
                if active {
                    Text("\(remaining) REMAINING")
                        .font(MGWebFont.mono(12, .caption))
                        .foregroundStyle(MGWeb.textHeading)
                        .accessibilityIdentifier("mg.admin.breakglass.remaining")
                }
            }
            FlowRow(spacing: 14) {
                if let holder {
                    (Text("HOLDER ").foregroundStyle(MGWeb.textFaint) + Text(holder).foregroundStyle(MGWeb.textBody))
                }
                if let scope {
                    (Text("SCOPE ").foregroundStyle(MGWeb.textFaint) + Text(scope).foregroundStyle(MGWeb.textBody))
                }
                Text("SECURITY OFFICERS ALERTED · REAL-TIME").foregroundStyle(MGWeb.textFaint)
            }
            .font(MGWebFont.mono(11))
            if let justification {
                Text(justification)
                    .font(MGWebFont.sans(12, .caption))
                    .foregroundStyle(MGWeb.textBody)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 11)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.md).fill(active ? MGWeb.dangerTint : MGWeb.warnTint))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.md).strokeBorder(tone))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(active ? "Break-glass active" : "Post-hoc review required"))
        .accessibilityIdentifier("mg.admin.breakglass.strip")
    }
}

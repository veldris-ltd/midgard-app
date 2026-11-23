import MidgardCore
import SwiftUI

/// The Operating Picture's view switch (the web's `ViewSwitch`, `.hg-view-switch`): the
/// 2D reference layout first, then the views an edition library adds, as one bordered
/// group of mono buttons; the chosen one has the accent tint and an accent rule under
/// it. On the iPhone each button is at least 44 pt square. [FR-H-1] [DD-7] [AC-C-11]
public struct HeimdallViewSwitch: View {
    let views: [HeimdallPictureView]
    @Binding var selection: String
    private let accent = MGWebAccent.heimdall

    public init(views: [HeimdallPictureView], selection: Binding<String>) {
        self.views = [HeimdallPictureView.reference] + views.filter { $0.id != HeimdallPictureView.reference.id }
        self._selection = selection
    }

    #if os(iOS)
    private let minSide: CGFloat = 44
    #else
    private let minSide: CGFloat = 30
    #endif

    public var body: some View {
        HStack(spacing: 0) {
            ForEach(Array(views.enumerated()), id: \.element.id) { index, view in
                if index > 0 { Rectangle().fill(MGWeb.border).frame(width: 1).accessibilityHidden(true) }
                button(view)
            }
        }
        .fixedSize()
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
        .clipShape(RoundedRectangle(cornerRadius: MGRadius.sm))
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text("Picture view"))
        .accessibilityIdentifier("mg.heimdall.view")
    }

    private func button(_ view: HeimdallPictureView) -> some View {
        let on = selection == view.id
        return Button {
            selection = view.id
        } label: {
            Text(view.label)
                .font(MGWebFont.mono(10).weight(on ? .semibold : .regular))
                .mgTracking(0.14, size: MGWebFont.monoSize(10))
                .foregroundStyle(on ? MGWeb.textHeading : MGWeb.textMuted)
                .padding(.horizontal, 10)
                .frame(minWidth: max(38, minSide), minHeight: minSide)
                .background(on ? accent.tint : Color.clear)
                .overlay(alignment: .bottom) {
                    if on { Rectangle().fill(accent.fill).frame(height: 2) }
                }
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("\(view.label) view"))
        .accessibilityAddTraits(on ? [.isSelected] : [])
        .accessibilityIdentifier("mg.heimdall.view.\(view.id)")
    }
}

/// The track card the 2D picture shows on hover and the geographic views show beside a
/// track (the web's `TrackTooltip`, shared by every view so all show the same rows).
/// [FR-H-1] [DD-7]
public struct HeimdallTrackCard: View {
    let track: HeimdallTrack
    let selected: Bool
    private let accent = MGWebAccent.heimdall

    public init(track: HeimdallTrack, selected: Bool) {
        self.track = track
        self.selected = selected
    }

    /// The card's rows, as the web's `tooltipRows`.
    public static func rows(_ t: HeimdallTrack) -> [(String, String)] {
        [
            ("HDG / SPD", "\(HeimdallPictureModel.heading(t.hdg))° · \(Int(t.spd)) KT"),
            ("ALT", t.alt),
            ("RNG", "\(HeimdallPictureModel.range(t)) NM FROM SITE"),
            ("CAP", HeimdallPictureModel.capability[t.cls] ?? "—"),
            ("TRACK", "\(t.track.uppercased()) · \(Int(t.feeds)) FEEDS"),
        ]
    }

    public var body: some View {
        let t = track
        VStack(alignment: .leading, spacing: 0) {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Text(t.id).font(MGWebFont.mono(11).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                Text(t.cls).font(MGWebFont.mono(9)).mgTracking(0.1, size: MGWebFont.monoSize(9)).foregroundStyle(MGWeb.textFaint)
                Spacer(minLength: 8)
                Text("\(Int((t.conf * 100).rounded()))%")
                    .font(MGWebFont.mono(10))
                    .foregroundStyle(t.conf < 0.7 ? MGWeb.warnText : MGWeb.secureText)
            }
            .padding(.bottom, 6)
            ForEach(Self.rows(t), id: \.0) { k, v in
                HStack(spacing: 10) {
                    Text(k).mgTracking(0.08, size: MGWebFont.monoSize(9)).foregroundStyle(MGWeb.textFaint).frame(width: 62, alignment: .leading)
                    Text(v).foregroundStyle(MGWeb.textBody)
                }
                .font(MGWebFont.mono(9))
                .padding(.vertical, 2)
            }
            if selected {
                Text("FOCUSED · OPTIONS IN INSPECTOR →")
                    .font(MGWebFont.mono(8))
                    .mgTracking(0.14, size: MGWebFont.monoSize(8))
                    .foregroundStyle(accent.text)
                    .padding(.top, 5)
            }
        }
        .padding(.horizontal, 11)
        .padding(.vertical, 9)
        .frame(minWidth: 200, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm)
            .strokeBorder(t.track == "anomalous" ? MGWeb.warn : MGWeb.borderStrong))
        .shadow(color: .black.opacity(0.45), radius: 10, y: 6)
    }
}

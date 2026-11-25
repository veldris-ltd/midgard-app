import MidgardCore
import SwiftUI

/// The Operating Picture's Alerts button (the web toolbar's bell, shown active): the
/// count of what needs attention, and a list of it (`HeimdallAlert`: anomalous tracks,
/// quarantined and stale feeds, from the rows the persona may see). Choosing a track's
/// alert selects the track for the inspector. The web's bell has no list behind it; the
/// list is the apps' own, from the same sample rows. [FR-H-1] [FR-H-3]
public struct HeimdallAlertsButton: View {
    @ObservedObject var model: HeimdallPictureModel
    @State private var showing = false
    /// The track whose alert was chosen, handed on once the list has gone.
    @State private var chosen: String?
    /// On the iPhone, choosing a track's alert opens the inspector. It is called after
    /// the list's sheet has been dismissed, so that one presentation follows the other.
    let onInspect: (String) -> Void

    public init(model: HeimdallPictureModel, onInspect: @escaping (String) -> Void = { _ in }) {
        self.model = model
        self.onInspect = onInspect
    }

    public var body: some View {
        let count = model.alerts.count
        Button {
            showing = true
        } label: {
            ZStack(alignment: .topTrailing) {
                Image(systemName: "bell")
                    .font(MGWebFont.sans(16))
                    .foregroundStyle(MGWebAccent.heimdall.text)
                    .frame(width: 32, height: 32)
                    .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWebAccent.heimdall.tint))
                if count > 0 {
                    Text("\(count)")
                        .font(MGWebFont.mono(10).weight(.bold))
                        .foregroundStyle(Color(mgHex: MGContrast.ink(on: MGToken.warn)))
                        .padding(.horizontal, 4)
                        .frame(minWidth: 16, minHeight: 16)
                        .background(Capsule().fill(MGWeb.warn))
                        .offset(x: 6, y: -6)
                }
            }
            .frame(minWidth: 44, minHeight: 44)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(Text("Alerts"))
        .accessibilityValue(Text("\(count)"))
        .accessibilityIdentifier("mg.heimdall.alerts")
        .popover(isPresented: $showing) {
            // On the iPhone the popover is a sheet: half height, dragged away.
            list.presentationDetents([.medium, .large]).presentationDragIndicator(.visible)
        }
    }

    private var list: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 10) {
                WebLabel("ALERTS · \(model.alerts.count)")
                if model.alerts.isEmpty {
                    Text("Nothing needs attention in what you may see.")
                        .font(MGWebFont.sans(14)).foregroundStyle(MGWeb.textMuted)
                }
                ForEach(model.alerts) { alert in
                    row(alert)
                }
            }
            .padding(16)
            .frame(minWidth: 300, maxWidth: 380, alignment: .leading)
        }
        .background(MGWeb.bgSunken)
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("mg.heimdall.alerts.list")
        .onDisappear {
            // The list's sheet has finished going: present the inspector now, not after a
            // guessed delay, which iOS 27's slower dismissal outlasted.
            guard let track = chosen else { return }
            chosen = nil
            onInspect(track)
        }
    }

    @ViewBuilder private func row(_ alert: HeimdallAlert) -> some View {
        if let track = alert.track {
            Button {
                model.select(track)
                chosen = track
                showing = false
            } label: {
                card(alert)
            }
            .buttonStyle(.plain)
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isButton)
            .accessibilityHint(Text("Selects the track"))
            .accessibilityIdentifier("mg.heimdall.alert.\(alert.id)")
        } else {
            card(alert)
                .accessibilityElement(children: .combine)
                .accessibilityIdentifier("mg.heimdall.alert.\(alert.id)")
        }
    }

    private func card(_ alert: HeimdallAlert) -> some View {
        let colour = alert.tone == .danger ? MGWeb.dangerText : MGWeb.warnText
        return HStack(alignment: .top, spacing: 10) {
            Image(systemName: alert.tone == .danger ? "xmark.octagon" : "exclamationmark.triangle")
                .foregroundStyle(colour)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: 3) {
                Text(alert.title).font(MGWebFont.mono(11).weight(.semibold)).foregroundStyle(colour)
                Text(alert.detail).font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textBody)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Spacer(minLength: 0)
        }
        .padding(10)
        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceCard))
        .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(alert.tone == .danger ? MGWeb.danger : MGWeb.warn))
        .contentShape(Rectangle())
    }
}

import MidgardShellCore
import SwiftUI

/// Design-system colours used by the native screens.
enum Palette {
    static let page = Color(red: 0x0a / 255, green: 0x0b / 255, blue: 0x0d / 255)
    static let card = Color(red: 0x16 / 255, green: 0x18 / 255, blue: 0x1c / 255)
    static let border = Color(red: 0x2a / 255, green: 0x2d / 255, blue: 0x34 / 255)
    static let text = Color(red: 0xe9 / 255, green: 0xeb / 255, blue: 0xee / 255)
    static let muted = Color(red: 0x91 / 255, green: 0x99 / 255, blue: 0xa3 / 255)
    static let secure = Color(red: 0x21 / 255, green: 0xc0 / 255, blue: 0x7a / 255)
    static let warn = Color(red: 0xf0 / 255, green: 0xa8 / 255, blue: 0x24 / 255)
    static let danger = Color(red: 0xf2 / 255, green: 0x6b / 255, blue: 0x70 / 255)
    static let accent = Color(red: 0xca / 255, green: 0x1d / 255, blue: 0x25 / 255)
}

/// "Starting Midgard…" with each service's progress, or what went wrong and "Try again".
struct StartingView: View {
    @EnvironmentObject var model: AppModel

    var body: some View {
        ZStack {
            Palette.page.ignoresSafeArea()
            VStack(alignment: .leading, spacing: 24) {
                switch model.phase {
                case .failed(let failure):
                    failureView(failure)
                case .stopping:
                    stoppingView
                default:
                    progressView
                }
            }
            .padding(40)
            .frame(maxWidth: 640)
            .background(Palette.card)
            .overlay(RoundedRectangle(cornerRadius: 10).stroke(Palette.border))
            .clipShape(RoundedRectangle(cornerRadius: 10))
        }
        .foregroundStyle(Palette.text)
    }

    private var progressView: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(model.startingTitle).font(.system(size: 28, weight: .semibold))
            if let step = model.startingStep {
                Text(step).foregroundStyle(Palette.muted)
            }
            let names = model.serviceOrder
            let ready = names.filter { model.statuses[$0] == .ready }.count
            ProgressView(value: Double(ready), total: Double(max(1, names.count)))
                .tint(Palette.accent)
            VStack(alignment: .leading, spacing: 10) {
                ForEach(names, id: \.self) { name in
                    ServiceRow(name: name, status: model.statuses[name])
                }
            }
            Text("This takes up to a minute.").font(.callout).foregroundStyle(Palette.muted)
        }
    }

    private var stoppingView: some View {
        VStack(alignment: .leading, spacing: 20) {
            Text(model.startingTitle).font(.system(size: 28, weight: .semibold))
            HStack(spacing: 10) {
                ProgressView().controlSize(.small)
                Text("Stopping the services…").foregroundStyle(Palette.muted)
            }
        }
    }

    private func failureView(_ failure: StartFailure) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            Label(failure.title, systemImage: "exclamationmark.triangle.fill")
                .font(.system(size: 22, weight: .semibold))
                .foregroundStyle(Palette.danger)
            let (summary, logLines) = Self.split(failure.details)
            Text(summary).fixedSize(horizontal: false, vertical: true)
            if let logLines {
                Text("Last lines of its log").font(.callout).foregroundStyle(Palette.muted)
                ScrollView {
                    Text(logLines)
                        .font(.system(size: 11, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(10)
                }
                .frame(maxHeight: 220)
                .background(Palette.page)
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
            if failure.canRetry {
                HStack {
                    Button("Try again") { model.retry() }
                        .keyboardShortcut(.defaultAction)
                    if failure.canReset {
                        Button("Reset demo…") { model.confirmingReset = true }
                    }
                    Spacer()
                    Text("Logs: \(model.paths.logs.path)").font(.caption).foregroundStyle(Palette.muted)
                        .textSelection(.enabled)
                }
            }
        }
    }

    /// The supervisor's reasons end with " Last log lines:\n…".
    static func split(_ details: String) -> (String, String?) {
        guard let range = details.range(of: " Last log lines:\n") else { return (details, nil) }
        return (String(details[..<range.lowerBound]), String(details[range.upperBound...]))
    }
}

private struct ServiceRow: View {
    let name: String
    let status: ServiceStatus?

    var body: some View {
        HStack(spacing: 10) {
            icon.frame(width: 18)
            Text(ServiceNames.friendly(name)).frame(width: 100, alignment: .leading)
            Text(label).foregroundStyle(Palette.muted)
            Spacer()
        }
    }

    @ViewBuilder private var icon: some View {
        switch status {
        case .ready:
            Image(systemName: "checkmark.circle.fill").foregroundStyle(Palette.secure)
        case .starting:
            ProgressView().controlSize(.small)
        case .restarting:
            Image(systemName: "arrow.clockwise.circle.fill").foregroundStyle(Palette.warn)
        case .failed:
            Image(systemName: "xmark.octagon.fill").foregroundStyle(Palette.danger)
        case .stopped, .none:
            Image(systemName: "circle").foregroundStyle(Palette.muted)
        }
    }

    private var label: String {
        switch status {
        case .ready: return "Ready"
        case .starting: return "Starting…"
        case .restarting(let attempt, _): return "Restarting (attempt \(attempt))…"
        case .failed(let reason): return reason == "port in use" ? "Port in use, retrying…" : "Failed"
        case .stopped, .none: return "Waiting"
        }
    }
}

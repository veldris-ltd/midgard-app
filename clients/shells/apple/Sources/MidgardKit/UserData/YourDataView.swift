import MidgardCore
import SwiftUI
import UniformTypeIdentifiers

/// Your Data, on both apps (the Mac's Session menu and Settings; the iPhone's session
/// menu): the sample data, the files imported, the enclosure gateway and the feed URLs,
/// each with what it is and whether it is live. Sample and imported data are never
/// called live. Built from the web console's components (`WebConsole/`). [DD-7] [RC-49]
public struct YourDataView: View {
    @ObservedObject var app: AppModel
    @ObservedObject var data: UserDataModel
    let compact: Bool
    @State private var importing = false
    @State private var confirmingRemove = false
    @State private var address = ""
    @State private var username = ""
    @State private var password = ""
    @State private var code = ""
    @State private var feedName = ""
    @State private var feedURL = ""
    @State private var feedInterval = UserFeedConfig.defaultInterval
    @State private var editing: String?
    @State private var feedError: String?

    public init(app: AppModel, compact: Bool) {
        self.app = app
        self.data = app.userData
        self.compact = compact
    }

    static var importTypes: [UTType] {
        [.json, UTType(filenameExtension: "jsonl") ?? .json, .plainText, .tabSeparatedText]
    }

    public var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                VStack(alignment: .leading, spacing: 6) {
                    Text("Your data")
                        .font(MGWebFont.display(compact ? 24 : 28, .title))
                        .foregroundStyle(MGWeb.textHeading)
                        .accessibilityAddTraits(.isHeader)
                    Text("Import your own JSON files, connect to an enclosure gateway, or add feeds to Heimdall's picture, as the guides describe.")
                        .font(MGWebFont.sans(14, .body))
                        .foregroundStyle(MGWeb.textBody)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !app.isUnlocked {
                    WebNotice("Sign in to import files, connect to a gateway or change feeds.", tone: .warn,
                              identifier: "mg.yourdata.locked")
                }
                sample
                imported
                gateway
                feeds
                addresses
            }
            .padding(compact ? 14 : 24)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(MGWeb.bgPage)
        .accessibilityIdentifier("mg.yourdata")
        .fileImporter(isPresented: $importing, allowedContentTypes: Self.importTypes, allowsMultipleSelection: true) { result in
            if case let .success(urls) = result, !urls.isEmpty {
                Task { await data.importFiles(urls) }
            }
        }
    }

    private func section(_ title: String) -> some View {
        Text(title)
            .font(MGWebFont.mono(11))
            .mgTracking(0.14, size: MGWebFont.monoSize(11))
            .foregroundStyle(MGWeb.textMuted)
            .accessibilityAddTraits(.isHeader)
    }

    // MARK: sample

    private var sample: some View {
        VStack(alignment: .leading, spacing: 8) {
            section("SAMPLE")
            WebPanel(identifier: "mg.yourdata.sample") {
                VStack(alignment: .leading, spacing: 8) {
                    FlowRow(spacing: 8) {
                        WebStatusPill(.secure, "RELEASE \(coreVersion().uppercased()) · SIGNED")
                        WebBadge("DEMONSTRATION DATA · NOT LIVE", tone: .warn)
                    }
                    Text("The signed release's built-in sample data, as its web consoles show it in mock mode. Always present; never live.")
                        .font(MGWebFont.sans(13, .callout)).foregroundStyle(MGWeb.textBody)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    // MARK: imported files

    private var imported: some View {
        VStack(alignment: .leading, spacing: 8) {
            section("IMPORTED FILES")
            WebPanel(identifier: "mg.yourdata.imported") {
                VStack(alignment: .leading, spacing: 10) {
                    if let s = data.summary, let ontology = s.ontologyName {
                        FlowRow(spacing: 8) {
                            WebBadge("IMPORTED · NOT LIVE", tone: .info)
                            WebBadge("\(s.objects) OBJECTS · \(s.links) LINKS", tone: .neutral)
                        }
                        Text("Ontology: \(ontology)")
                            .font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                        ForEach(Array(s.files.enumerated()), id: \.offset) { _, f in
                            Text(fileLine(f))
                                .font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                                .fixedSize(horizontal: false, vertical: true)
                                .accessibilityIdentifier("mg.yourdata.file.\(f.name)")
                        }
                        if s.unreadable > 0 {
                            Text("\(s.unreadable) object(s) lack the purpose \"operations\": nobody can read them.")
                                .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.warnText)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    } else {
                        Text("Nothing imported. Pick your ontology.json and one or more JSON Lines data files (.jsonl or .json), or Heimdall observation files, as \"Import your own data\" describes. All or nothing: the first bad line refuses the import and is named.")
                            .font(MGWebFont.sans(13, .callout)).foregroundStyle(MGWeb.textBody)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    FlowRow(spacing: 10) {
                        WebButton(data.busy ? "Importing…" : "Import files…", symbol: "square.and.arrow.down",
                                  disabledReason: app.isUnlocked ? (data.busy ? "An import is running" : nil) : "Sign in first",
                                  identifier: "mg.yourdata.import") { importing = true }
                        if data.summary?.ontologyName != nil {
                            WebButton("Remove imported data", symbol: "trash", variant: .secondary,
                                      disabledReason: app.isUnlocked ? nil : "Sign in first",
                                      identifier: "mg.yourdata.remove") { confirmingRemove = true }
                                .confirmationDialog("Remove the imported data?", isPresented: $confirmingRemove) {
                                    Button("Remove imported data", role: .destructive) { Task { await data.removeImported() } }
                                } message: {
                                    Text("Its objects, links and observation layers leave every screen. The sample data stays.")
                                }
                        }
                    }
                    #if os(macOS)
                    Text("Or drop the files on a Midgard window.")
                        .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                    #endif
                    if let n = data.notice {
                        WebNotice(n.text, tone: n.ok ? .secure : .danger, identifier: "mg.yourdata.notice")
                    }
                }
            }
        }
    }

    private func fileLine(_ f: ImportedFileInfo) -> String {
        switch f.kind {
        case .records: return "\(f.name) · \(f.objects) objects · \(f.links) links"
        case .observations: return "\(f.name) · \(f.observations) observations · UNVERIFIED FEED"
        }
    }

    // MARK: gateway

    private var gatewayPill: WebStatusPill {
        switch data.gatewayState {
        case .connected: return WebStatusPill(.live, "LIVE · \(data.live?.host ?? "")")
        case .connecting: return WebStatusPill(.warn, "CONNECTING…")
        case .failed: return WebStatusPill(.danger, "NOT CONNECTED")
        case .disconnected: return WebStatusPill(.offline, "NOT CONNECTED")
        }
    }

    private var gateway: some View {
        VStack(alignment: .leading, spacing: 8) {
            section("ENCLOSURE GATEWAY")
            WebPanel(identifier: "mg.yourdata.gateway") {
                VStack(alignment: .leading, spacing: 10) {
                    gatewayPill.accessibilityIdentifier("mg.yourdata.gateway.state")
                    if let live = data.live {
                        Text("Signed in to \(live.host) as \(live.subject) · clearance \(live.clearance)\(live.compartments.isEmpty ? "" : " // \(live.compartments.joined(separator: ", "))") · purpose \(live.purpose). Midgard's screens read this gateway; Admin's are not served by it and say so.")
                            .font(MGWebFont.sans(13, .callout)).foregroundStyle(MGWeb.textBody)
                            .fixedSize(horizontal: false, vertical: true)
                        WebButton("Disconnect", symbol: "bolt.horizontal.circle", variant: .secondary,
                                  identifier: "mg.yourdata.disconnect") { Task { await data.disconnect() } }
                    } else {
                        Text("An address (https://, or http:// on this device only), and the username and password an administrator enrolled with mg-admin user add. The token stays in memory; the password is not kept.")
                            .font(MGWebFont.sans(13, .callout)).foregroundStyle(MGWeb.textBody)
                            .fixedSize(horizontal: false, vertical: true)
                        WebTextField("ADDRESS", placeholder: "https://gateway.example.org", text: $address,
                                     identifier: "mg.yourdata.address")
                        WebTextField("USERNAME", placeholder: "sam", text: $username, identifier: "mg.yourdata.username")
                        VStack(alignment: .leading, spacing: 6) {
                            WebLabel("PASSWORD", colour: MGWeb.textMuted).accessibilityHidden(true)
                            SecureField(text: $password, prompt: Text("Password").foregroundStyle(MGWeb.textMuted)) {
                                Text("Password")
                            }
                            .textFieldStyle(.plain)
                            .font(MGWebFont.mono(13, .callout))
                            .foregroundStyle(MGWeb.textHeading)
                            .padding(.horizontal, 12)
                            .frame(maxWidth: .infinity, minHeight: webTarget(32), alignment: .leading)
                            .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
                            .overlay(RoundedRectangle(cornerRadius: MGRadius.sm).strokeBorder(MGWeb.border))
                            .accessibilityLabel(Text("Password"))
                            .accessibilityIdentifier("mg.yourdata.password")
                        }
                        WebTextField("AUTHENTICATOR CODE (IF YOUR ACCOUNT HAS ONE)", placeholder: "123456", text: $code,
                                     identifier: "mg.yourdata.code")
                        WebButton(data.gatewayState == .connecting ? "Connecting…" : "Connect", symbol: "bolt.horizontal",
                                  disabledReason: address.isEmpty || username.isEmpty || password.isEmpty
                                      ? "Give the address, username and password" : (app.isUnlocked ? nil : "Sign in first"),
                                  identifier: "mg.yourdata.connect") {
                            let (a, u, p, c) = (address, username, password, code)
                            password = ""
                            code = ""
                            Task { await data.connect(address: a, username: u, password: p, code: c) }
                        }
                        if case let .failed(why) = data.gatewayState {
                            WebNotice(why, tone: .danger, identifier: "mg.yourdata.gateway.error")
                        }
                    }
                }
            }
        }
    }

    // MARK: feeds

    private var feeds: some View {
        VStack(alignment: .leading, spacing: 8) {
            section("FEEDS")
            WebPanel(identifier: "mg.yourdata.feeds") {
                VStack(alignment: .leading, spacing: 10) {
                    Text("An https:// address serving Heimdall observations (JSON, or JSON Lines), read while Heimdall's picture is open. Not signed by a certified mesh identity, so shown as UNVERIFIED FEED and never fused into signed tracks.")
                        .font(MGWebFont.sans(13, .callout)).foregroundStyle(MGWeb.textBody)
                        .fixedSize(horizontal: false, vertical: true)
                    ForEach(data.feeds) { feed in feedRow(feed) }
                    WebTextField("NAME", placeholder: "Grid observations", text: $feedName, identifier: "mg.yourdata.feed.name")
                    WebTextField("ADDRESS", placeholder: "https://feeds.example.org/observations.json", text: $feedURL,
                                 identifier: "mg.yourdata.feed.url")
                    Stepper(value: $feedInterval, in: UserFeedConfig.minimumInterval...3600, step: 30) {
                        Text("Every \(feedInterval) s")
                            .font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                    }
                    .frame(minHeight: webTarget(32))
                    .accessibilityIdentifier("mg.yourdata.feed.interval")
                    FlowRow(spacing: 10) {
                        WebButton(editing == nil ? "Add feed" : "Save feed", symbol: "plus",
                                  disabledReason: feedName.isEmpty || feedURL.isEmpty ? "Give a name and an address" : nil,
                                  identifier: "mg.yourdata.feed.add") {
                            feedError = data.saveFeed(id: editing, name: feedName, url: feedURL, interval: feedInterval)
                            if feedError == nil {
                                feedName = ""
                                feedURL = ""
                                feedInterval = UserFeedConfig.defaultInterval
                                editing = nil
                            }
                        }
                        if editing != nil {
                            WebButton("Cancel", variant: .ghost, identifier: "mg.yourdata.feed.cancel") {
                                editing = nil
                                feedName = ""
                                feedURL = ""
                            }
                        }
                    }
                    if let feedError { WebNotice(feedError, tone: .danger, identifier: "mg.yourdata.feed.error") }
                }
            }
        }
    }

    private func feedRow(_ feed: UserFeedConfig) -> some View {
        let state = data.feedStates[feed.id]
        return VStack(alignment: .leading, spacing: 6) {
            FlowRow(spacing: 8) {
                Text(feed.name).font(MGWebFont.sans(14, .callout).weight(.semibold)).foregroundStyle(MGWeb.textHeading)
                WebBadge("UNVERIFIED FEED", tone: .warn)
            }
            Text("\(URL(string: feed.url)?.host ?? feed.url) · every \(feed.interval) s · \(lastFetch(state))")
                .font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                .fixedSize(horizontal: false, vertical: true)
            if let error = state?.error {
                Text(error).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.dangerText)
                    .fixedSize(horizontal: false, vertical: true)
            }
            FlowRow(spacing: 10) {
                WebButton("Edit", variant: .ghost, size: .sm, identifier: "mg.yourdata.feed.edit.\(feed.name)") {
                    editing = feed.id
                    feedName = feed.name
                    feedURL = feed.url
                    feedInterval = feed.interval
                }
                WebButton("Remove", variant: .ghost, size: .sm, identifier: "mg.yourdata.feed.remove.\(feed.name)") {
                    data.removeFeed(feed.id)
                }
            }
        }
        .padding(10)
        .background(RoundedRectangle(cornerRadius: MGRadius.sm).fill(MGWeb.surfaceInset))
        .accessibilityElement(children: .contain)
    }

    private func lastFetch(_ state: UserFeedState?) -> String {
        guard let at = state?.lastFetch else {
            return data.polling ? "fetching" : "read while Heimdall's picture is open"
        }
        let points = state?.layers.reduce(0) { $0 + $1.points.count } ?? 0
        return "last read \(at.formatted(date: .omitted, time: .standard)) · \(points) placed"
    }

    // MARK: addresses

    private var addresses: some View {
        VStack(alignment: .leading, spacing: 8) {
            section("ADDRESSES THIS APP MAY REACH FOR YOUR DATA")
            WebPanel(identifier: "mg.yourdata.addresses") {
                VStack(alignment: .leading, spacing: 6) {
                    let origins = data.allowedOrigins
                    if origins.isEmpty {
                        Text("None. The map tiles' three hosts are a separate list.")
                            .font(MGWebFont.sans(13, .callout)).foregroundStyle(MGWeb.textBody)
                    } else {
                        ForEach(origins, id: \.self) { o in
                            Text(o).font(MGWebFont.mono(12, .caption)).foregroundStyle(MGWeb.textBody)
                        }
                        Text("Only these, only while the gateway is connected or Heimdall's picture is open. The map tiles' three hosts are a separate list.")
                            .font(MGWebFont.sans(12, .caption)).foregroundStyle(MGWeb.textMuted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                }
            }
        }
    }
}

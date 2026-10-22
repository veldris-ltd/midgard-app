import AppKit
import MidgardShellCore
import SwiftUI
import WebKit

/// The one WKWebView the window shows, with the offline rules, the session scripts and
/// the navigation policy. Created once and kept for the life of the app.
@MainActor
final class WebController: NSObject, WKNavigationDelegate, WKUIDelegate {
    let webView: WKWebView
    private(set) var rulesInstalled = false
    /// Which URLs a navigation may go to; nothing is allowed until `configure`.
    private var isAllowed: (URL) -> Bool = { _ in false }
    var onPageLoaded: ((URL?) -> Void)?
    /// The web content process ended; the URL it showed, if known.
    var onContentCrash: ((URL?) -> Void)?
    var onLoadFailed: ((URL?, Error) -> Void)?

    override init() {
        let configuration = WKWebViewConfiguration()
        // Persistent, so the consoles' UI preferences survive a relaunch.
        configuration.websiteDataStore = .default()
        // Safe Browsing would look pages up online; everything here is loopback.
        configuration.preferences.isFraudulentWebsiteWarningEnabled = false
        webView = WKWebView(frame: .zero, configuration: configuration)
        super.init()
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.allowsBackForwardNavigationGestures = false
        webView.underPageBackgroundColor = NSColor(red: 0.04, green: 0.043, blue: 0.05, alpha: 1)
        if #available(macOS 13.3, *), Bundle.main.bundleIdentifier == nil {
            webView.isInspectable = true  // development runs only
        }
    }

    /// Compiles and installs the content rule list that blocks every non-loopback load.
    func installOfflineRules() async throws {
        guard let store = WKContentRuleListStore.default() else {
            throw CocoaError(.featureUnsupported)
        }
        let list: WKContentRuleList = try await withCheckedThrowingContinuation { continuation in
            store.compileContentRuleList(forIdentifier: OfflineRules.identifier,
                                         encodedContentRuleList: OfflineRules.json) { list, error in
                if let list {
                    continuation.resume(returning: list)
                } else {
                    continuation.resume(throwing: error ?? CocoaError(.featureUnsupported))
                }
            }
        }
        webView.configuration.userContentController.add(list)
        rulesInstalled = true
    }

    /// Replaces the document-start scripts (main frame only) and the navigation policy.
    func configure(scripts: [String], allowed: @escaping (URL) -> Bool) {
        let controller = webView.configuration.userContentController
        controller.removeAllUserScripts()
        for source in scripts {
            controller.addUserScript(WKUserScript(source: source, injectionTime: .atDocumentStart,
                                                  forMainFrameOnly: true))
        }
        isAllowed = allowed
    }

    /// Removes every page's stored data (local and session storage, IndexedDB, caches,
    /// cookies): after Reset demo the consoles start as on a fresh Mac.
    func clearWebsiteData() async {
        let store = webView.configuration.websiteDataStore
        await store.removeData(ofTypes: WKWebsiteDataStore.allWebsiteDataTypes(), modifiedSince: .distantPast)
    }

    /// The web content process's pid (development self-test only; private WebKit key).
    var contentProcessIdentifier: pid_t? {
        let key = "_webProcessIdentifier"
        guard webView.responds(to: NSSelectorFromString(key)) else { return nil }
        return (webView.value(forKey: key) as? NSNumber).map { pid_t($0.int32Value) }
    }

    func load(_ url: URL) {
        webView.load(URLRequest(url: url))
    }

    func reload() {
        webView.reload()
    }

    func evaluate(_ script: String) {
        webView.evaluateJavaScript(script, completionHandler: nil)
    }

    /// Runs an async function body in the page and reports its result as text.
    func callAsync(_ body: String, completion: @escaping (String) -> Void) {
        webView.callAsyncJavaScript(body, arguments: [:], in: nil, in: .page) { result in
            switch result {
            case .success(let value): completion("\(value)")
            case .failure(let error): completion("error: \(error.localizedDescription)")
            }
        }
    }

    // MARK: WKNavigationDelegate

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping @MainActor @Sendable (WKNavigationActionPolicy) -> Void) {
        guard let url = navigationAction.request.url, isAllowed(url) else {
            NSLog("Midgard: refused navigation to %@", navigationAction.request.url?.absoluteString ?? "?")
            return decisionHandler(.cancel)
        }
        decisionHandler(.allow)
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        onPageLoaded?(webView.url)
    }

    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!,
                 withError error: Error) {
        onLoadFailed?((error as NSError).userInfo[NSURLErrorFailingURLErrorKey] as? URL ?? webView.url, error)
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
        onLoadFailed?(webView.url, error)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        // The model decides: reload (within a budget), Heimdall in 2D, or go home.
        onContentCrash?(webView.url)
    }

    // MARK: WKUIDelegate

    /// `target=_blank` and `window.open`: the same view, never a new window.
    func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                 for navigationAction: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
        if let url = navigationAction.request.url, isAllowed(url) {
            webView.load(navigationAction.request)
        }
        return nil
    }
}

/// Puts the controller's web view in the SwiftUI hierarchy.
struct WebView: NSViewRepresentable {
    let controller: WebController

    func makeNSView(context: Context) -> WKWebView { controller.webView }
    func updateNSView(_ nsView: WKWebView, context: Context) {}
}

/// Development self-test only: a PNG of the app's own window (no Screen Recording
/// permission needed), with the web view's content drawn in from its own snapshot.
@MainActor
enum WindowSnapshot {
    static func save(webView: WKWebView, to file: URL) async -> String {
        guard let window = NSApp.windows.first(where: { $0.isVisible && $0.contentView != nil }),
              let frameView = window.contentView?.superview,
              let rep = frameView.bitmapImageRepForCachingDisplay(in: frameView.bounds)
        else { return "no window" }
        frameView.cacheDisplay(in: frameView.bounds, to: rep)
        if webView.window === window, let image = try? await webView.takeSnapshot(configuration: nil) {
            let rect = webView.convert(webView.bounds, to: frameView)
            NSGraphicsContext.saveGraphicsState()
            NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
            image.draw(in: rect)
            NSGraphicsContext.restoreGraphicsState()
        }
        guard let png = rep.representation(using: .png, properties: [:]) else { return "no image" }
        do {
            try FileManager.default.createDirectory(at: file.deletingLastPathComponent(),
                                                    withIntermediateDirectories: true)
            try png.write(to: file)
            return "saved \(file.path) (\(rep.pixelsWide)x\(rep.pixelsHigh))"
        } catch {
            return "failed: \(error.localizedDescription)"
        }
    }
}

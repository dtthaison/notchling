import AppKit
import ConsoleCore
import SwiftUI
import WebKit

enum VizBundleLocation {
    /// Contents/Resources/viz inside the app; from `swift run`, the repo's viz/ folder.
    static func indexURL() -> URL? {
        if let url = Bundle.main.resourceURL?.appendingPathComponent("viz/index.html"),
           FileManager.default.fileExists(atPath: url.path) { return url }
        let dev = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().deletingLastPathComponent().appendingPathComponent("viz/index.html")
        return FileManager.default.fileExists(atPath: dev.path) ? dev : nil
    }
}

/// The shared viz bundle in a WKWebView. Specs go in through `callAsyncJavaScript` arguments, so no
/// string of ours is ever spliced into script text. Navigation away from the bundle is refused.
struct VizWebView: NSViewRepresentable {
    let spec: VizSpec
    let indexURL: URL

    func makeCoordinator() -> Coordinator { Coordinator(indexURL: indexURL) }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.preferences.javaScriptCanOpenWindowsAutomatically = false
        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = context.coordinator
        webView.setValue(false, forKey: "drawsBackground")
        webView.underPageBackgroundColor = NSColor(Token.surface)
        webView.setAccessibilityLabel("Visualize")
        context.coordinator.webView = webView
        webView.loadFileURL(indexURL, allowingReadAccessTo: indexURL.deletingLastPathComponent())
        return webView
    }

    func updateNSView(_ webView: WKWebView, context: Context) {
        context.coordinator.show(spec)
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        let indexURL: URL
        weak var webView: WKWebView?
        private var loaded = false
        private var pending: String?
        private var lastShown: String?

        init(indexURL: URL) { self.indexURL = indexURL }

        func show(_ spec: VizSpec) {
            let json: String
            do { json = try spec.json() } catch {
                ConsoleLog.error("VizWebView.show", indexURL.path, "encode failed: \(error)")
                return
            }
            guard json != lastShown else { return }
            lastShown = json
            if loaded { run(json) } else { pending = json }
        }

        private func run(_ json: String) {
            webView?.callAsyncJavaScript("window.renderViz(JSON.parse(spec));", arguments: ["spec": json],
                                         in: nil, in: .page) { result in
                if case .failure(let error) = result {
                    ConsoleLog.error("VizWebView.run", self.indexURL.path, "renderViz failed: \(error)")
                }
            }
        }

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            loaded = true
            if let pending { run(pending); self.pending = nil }
        }

        func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) {
            ConsoleLog.error("VizWebView.didFail", indexURL.path, "\(error)")
        }

        func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) {
            ConsoleLog.error("VizWebView.didFailProvisional", indexURL.path, "\(error)")
        }

        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            let target = navigationAction.request.url
            let isBundle = target.map { $0.isFileURL && $0.resolvingSymlinksInPath().path == indexURL.resolvingSymlinksInPath().path } ?? false
            if isBundle || target?.absoluteString == "about:blank" {
                decisionHandler(.allow)
            } else {
                ConsoleLog.error("VizWebView.navigation", target?.absoluteString ?? "nil", "navigation away from the bundle refused")
                decisionHandler(.cancel)
            }
        }
    }
}

/// One of the four web-rendered tabs: skeleton while the engine reads, error with Thử lại, then the bundle.
struct VizContent: View {
    @Bindable var model: ConsoleModel
    let session: Session
    let kind: VizSpec.Kind

    var body: some View {
        switch model.viz.state {
        case .idle, .loading:
            SkeletonRows(count: 6).frame(maxHeight: .infinity, alignment: .top)
        case .failed(let message):
            ErrorState(message: message) { model.viz.retry() }
        case .ready(let bundle):
            if let index = VizBundleLocation.indexURL() {
                if kind == .entity { entityContent(index) } else {
                    VizWebView(spec: spec(from: bundle), indexURL: index).id(kind)
                }
            } else {
                ErrorState(message: "Không tìm thấy bundle viz trong app.") { model.viz.retry() }
            }
        }
    }

    @ViewBuilder
    private func entityContent(_ index: URL) -> some View {
        let cwd = model.vizCwdOverride ?? session.cwd
        Group {
            if let spec = model.viz.entitySpecs[cwd] {
                VizWebView(spec: spec, indexURL: index)
            } else {
                SkeletonRows(count: 6).frame(maxHeight: .infinity, alignment: .top)
            }
        }
        .task(id: cwd) { model.viz.requestEntity(cwd: cwd) }
    }

    private func spec(from bundle: VizBundle) -> VizSpec {
        switch kind {
        case .flow: return bundle.flow
        case .diagram: return bundle.diagram
        case .entity: return bundle.chart  // unreachable: entityContent handles it
        case .chart: return bundle.chart
        }
    }
}

import AppKit
import ConsoleCore
import SwiftUI
import WebKit

@MainActor
@Observable
final class WebTabModel {
    enum State: Equatable {
        case noURL
        case probing
        case down
        case ready(URL)
    }

    private(set) var state: State = .noURL
    var field = ""
    private(set) var reloadToken = 0
    @ObservationIgnored private var probeTask: Task<Void, Never>?

    /// Sets the address and probes it. Only http(s) is accepted.
    func open(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespaces)
        let withScheme = trimmed.contains("://") ? trimmed : "http://" + trimmed
        guard let url = URL(string: withScheme), let scheme = url.scheme, ["http", "https"].contains(scheme), url.host != nil else {
            ConsoleLog.error("WebTabModel.open", trimmed, "not an http(s) URL")
            state = .noURL
            return
        }
        field = url.absoluteString
        probe(url)
    }

    func reload() {
        guard case .ready = state else { if let url = URL(string: field) { probe(url) }; return }
        reloadToken += 1
    }

    func retry() {
        if let url = URL(string: field) { probe(url) }
    }

    private func probe(_ url: URL) {
        probeTask?.cancel()
        state = .probing
        probeTask = Task {
            let reachable = await WebTabModel.isReachable(url)
            guard !Task.isCancelled else { return }
            state = reachable ? .ready(url) : .down
        }
    }

    /// HEAD with a 1 s timeout; any HTTP answer, whatever its status, means a server is there.
    nonisolated static func isReachable(_ url: URL) async -> Bool {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 1)
        request.httpMethod = "HEAD"
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 1
        configuration.timeoutIntervalForResource = 1
        do {
            let (_, response) = try await URLSession(configuration: configuration).data(for: request)
            return response is HTTPURLResponse
        } catch {
            ConsoleLog.logger.debug("probe \(url.absoluteString, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return false
        }
    }
}

private struct SessionWebView: NSViewRepresentable {
    let url: URL
    let reloadToken: Int

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        let webView = WKWebView(frame: .zero, configuration: configuration)
        webView.navigationDelegate = context.coordinator
        webView.underPageBackgroundColor = NSColor(Token.surface)
        webView.setAccessibilityLabel("Trang web của session")
        webView.load(URLRequest(url: url))
        context.coordinator.loadedURL = url
        context.coordinator.token = reloadToken
        return webView
    }

    func updateNSView(_ webView: WKWebView, context: Context) {
        if context.coordinator.loadedURL != url {
            context.coordinator.loadedURL = url
            webView.load(URLRequest(url: url))
        } else if context.coordinator.token != reloadToken {
            webView.reload()
        }
        context.coordinator.token = reloadToken
    }

    final class Coordinator: NSObject, WKNavigationDelegate {
        var loadedURL: URL?
        var token = 0

        func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            let scheme = navigationAction.request.url?.scheme ?? ""
            if ["http", "https", "about"].contains(scheme) {
                decisionHandler(.allow)
            } else {
                ConsoleLog.error("SessionWebView.navigation", navigationAction.request.url?.absoluteString ?? "nil", "only http(s) is allowed")
                decisionHandler(.cancel)
            }
        }
    }
}

struct WebTab: View {
    @Bindable var model: ConsoleModel
    let session: Session
    @State private var web = WebTabModel()
    @FocusState private var fieldFocused: Bool

    private var detectedURL: URL? { model.viz.bundle?.webURL }

    var body: some View {
        VStack(spacing: 0) {
            addressBar
            Divider1()
            content
        }
        .onAppear { openDetected() }
        .onChange(of: detectedURL) { _, _ in if web.state == .noURL { openDetected() } }
        .onChange(of: session.id) { _, _ in web = WebTabModel(); openDetected() }
    }

    private func openDetected() {
        if let url = detectedURL, web.field.isEmpty { web.open(url.absoluteString) }
    }

    private var addressBar: some View {
        HStack(spacing: Space.sm) {
            TextField("http://localhost:3000", text: $web.field)
                .textFieldStyle(.plain).font(TypeToken.codeSm).foregroundStyle(Token.onSurface)
                .focused($fieldFocused)
                .onSubmit { web.open(web.field) }
                .padding(.horizontal, Space.sm).frame(height: 28)
                .background(Token.neutral, in: RoundedRectangle(cornerRadius: Radius.md))
                .overlay(RoundedRectangle(cornerRadius: Radius.md)
                    .stroke(fieldFocused ? Token.primary : Token.border, lineWidth: fieldFocused ? 2 : 1))
                .accessibilityLabel("Địa chỉ trang web")
            Button { web.reload() } label: { Image(systemName: "arrow.clockwise").font(.system(size: 12)) }
                .buttonStyle(IconButtonStyle()).accessibilityLabel("Tải lại").help("Tải lại")
                .keyboardShortcut("r", modifiers: .command)
            Button {
                if let url = URL(string: web.field), ["http", "https"].contains(url.scheme ?? "") { NSWorkspace.shared.open(url) }
            } label: { Image(systemName: "arrow.up.forward.app").font(.system(size: 12)) }
                .buttonStyle(IconButtonStyle()).accessibilityLabel("Mở trong trình duyệt").help("Mở trong trình duyệt")
        }
        .padding(.horizontal, Space.md).frame(height: 40)
    }

    @ViewBuilder
    private var content: some View {
        switch web.state {
        case .noURL:
            placeholder(symbol: "globe", title: "Chưa thấy trang web của session này",
                        detail: "Console tìm URL localhost trong chat, hoặc cấu hình trong .claude/launch.json. Nhập địa chỉ ở trên để mở.")
        case .probing:
            SkeletonRows(count: 4).frame(maxHeight: .infinity, alignment: .top)
        case .down:
            VStack(spacing: Space.md) {
                Image(systemName: "bolt.horizontal.circle").font(.system(size: 26, weight: .light)).foregroundStyle(Token.muted)
                    .accessibilityHidden(true)
                Text("Server chưa chạy").font(TypeToken.titleMd).foregroundStyle(Token.onSurface)
                Text("\(web.field) không trả lời trong 1 giây. Bật dev server của session rồi thử lại.")
                    .font(TypeToken.bodyMd).foregroundStyle(Token.muted).multilineTextAlignment(.center).frame(maxWidth: 380)
                Button("Thử lại") { web.retry() }.buttonStyle(SecondaryButtonStyle()).accessibilityLabel("Thử lại")
            }
            .padding(Space.xl).frame(maxWidth: .infinity, maxHeight: .infinity).panelEntrance()
        case .ready(let url):
            SessionWebView(url: url, reloadToken: web.reloadToken)
        }
    }

    private func placeholder(symbol: String, title: String, detail: String) -> some View {
        VStack(spacing: Space.md) {
            Image(systemName: symbol).font(.system(size: 26, weight: .light)).foregroundStyle(Token.muted).accessibilityHidden(true)
            Text(title).font(TypeToken.titleMd).foregroundStyle(Token.onSurface)
            Text(detail).font(TypeToken.bodyMd).foregroundStyle(Token.muted).multilineTextAlignment(.center).frame(maxWidth: 400)
        }
        .padding(Space.xl).frame(maxWidth: .infinity, maxHeight: .infinity).panelEntrance()
        .accessibilityElement(children: .combine)
    }
}

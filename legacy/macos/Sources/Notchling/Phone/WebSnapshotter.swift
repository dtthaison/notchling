import AppKit
import ConsoleCore
import WebKit

/// Renders a localhost page in an offscreen WKWebView and returns it as a JPEG, for phones (v1: a picture,
/// not an interactive page).
@MainActor
final class WebSnapshotter: NSObject, WKNavigationDelegate {
    private var continuation: CheckedContinuation<Void, Error>?

    static func snapshot(url: URL, width: Int) async throws -> Data {
        let snapshotter = WebSnapshotter()
        return try await snapshotter.render(url: url, width: width)
    }

    private func render(url: URL, width: Int) async throws -> Data {
        let size = NSSize(width: CGFloat(width), height: CGFloat(width) * 1.8)
        let configuration = WKWebViewConfiguration()
        configuration.websiteDataStore = .nonPersistent()
        let webView = WKWebView(frame: NSRect(origin: .zero, size: size), configuration: configuration)
        webView.navigationDelegate = self
        // A web view that is not in a window never paints, so it gets a borderless one far off screen.
        let window = NSWindow(contentRect: NSRect(origin: NSPoint(x: -20_000, y: -20_000), size: size),
                              styleMask: [.borderless], backing: .buffered, defer: false)
        window.contentView = webView
        window.isReleasedWhenClosed = false
        window.orderBack(nil)
        defer { window.close() }

        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask { @MainActor in
                try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                    self.continuation = continuation
                    webView.load(URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 8))
                }
            }
            group.addTask {
                try await Task.sleep(nanoseconds: 10_000_000_000)
                throw BridgeFailure(.notReachable, "Trang không tải xong trong 10 giây.")
            }
            _ = try await group.next()
            group.cancelAll()
        }
        try await Task.sleep(nanoseconds: 700_000_000)  // let late scripts and images paint

        let configurationSnapshot = WKSnapshotConfiguration()
        configurationSnapshot.afterScreenUpdates = true
        let image: NSImage = try await withCheckedThrowingContinuation { continuation in
            webView.takeSnapshot(with: configurationSnapshot) { image, error in
                if let image { continuation.resume(returning: image) } else {
                    continuation.resume(throwing: error ?? BridgeFailure(.internal, "Không chụp được trang."))
                }
            }
        }
        guard let tiff = image.tiffRepresentation, let bitmap = NSBitmapImageRep(data: tiff),
              let jpeg = bitmap.representation(using: .jpeg, properties: [.compressionFactor: 0.6]) else {
            throw BridgeFailure(.internal, "Không mã hoá được ảnh JPEG.")
        }
        return jpeg
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        continuation?.resume()
        continuation = nil
    }

    func webView(_ webView: WKWebView, didFail navigation: WKNavigation!, withError error: Error) { fail(error) }
    func webView(_ webView: WKWebView, didFailProvisionalNavigation navigation: WKNavigation!, withError error: Error) { fail(error) }

    private func fail(_ error: Error) {
        continuation?.resume(throwing: BridgeFailure(.notReachable, "Không tải được trang: \(error.localizedDescription)"))
        continuation = nil
    }

    func webView(_ webView: WKWebView, decidePolicyFor navigationAction: WKNavigationAction,
                 decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
        let scheme = navigationAction.request.url?.scheme ?? ""
        decisionHandler(["http", "https", "about"].contains(scheme) ? .allow : .cancel)
    }
}

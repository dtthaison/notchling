import AppKit
import SwiftUI
import WebKit

/// The window over `PageLibrary`: every saved Artifact in a searchable list, shown in place, whatever
/// account is active. Separate from the Accounts window because a page needs far more room.
final class PagesWindowController: NSObject, NSWindowDelegate {
    private let model: PagesModel
    private var window: NSWindow?
    private var timer: Timer?

    init(accounts: AccountStore) {
        model = PagesModel(accounts: accounts)
    }

    func show(select folder: String? = nil) {
        model.reload()
        if let folder { model.select(folder) } else if model.selected == nil { model.selection = model.shown.first?.folder }
        if window == nil {
            let window = EditingWindow(contentViewController: NSHostingController(rootView: PagesView(model: model)))
            window.title = "Artifacts"
            window.styleMask = [.titled, .closable, .miniaturizable, .resizable]
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.setContentSize(NSSize(width: 1080, height: 720))
            window.center()
            window.setFrameAutosaveName("Notchling Artifacts")
            self.window = window
            // A publish lands from the hook process, so look for it while the window is open.
            timer = Timer.scheduledTimer(withTimeInterval: 3, repeats: true) { [weak self] _ in self?.model.reload() }
        }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
    }

    func windowDidBecomeKey(_ notification: Notification) {
        model.reload()
    }

    func windowWillClose(_ notification: Notification) {
        timer?.invalidate()
        timer = nil
        // Let go of the web view and its process on the next pass; the window is still closing here.
        DispatchQueue.main.async { self.window = nil }
    }
}

/// The app has no main menu, so nothing else sends ⌘C, ⌘V and friends to the search field or the page.
private final class EditingWindow: NSWindow {
    override func performKeyEquivalent(with event: NSEvent) -> Bool {
        guard event.modifierFlags.intersection(.deviceIndependentFlagsMask) == .command else {
            return super.performKeyEquivalent(with: event)
        }
        let action: Selector?
        switch event.charactersIgnoringModifiers {
        case "c": action = #selector(NSText.copy(_:))
        case "v": action = #selector(NSText.paste(_:))
        case "x": action = #selector(NSText.cut(_:))
        case "a": action = #selector(NSText.selectAll(_:))
        case "w": performClose(nil); return true
        default: action = nil
        }
        if let action, NSApp.sendAction(action, to: nil, from: self) { return true }
        return super.performKeyEquivalent(with: event)
    }
}

extension PageLibrary.Entry: Identifiable {
    var id: String { folder }
}

final class PagesModel: ObservableObject {
    @Published private(set) var pages: [PageLibrary.Entry] = []
    @Published var selection: String?
    @Published var search = ""
    /// "" means every account.
    @Published var account = ""
    private let accounts: AccountStore
    private var stamp: Date?

    init(accounts: AccountStore) { self.accounts = accounts }

    /// Reads the list again only when the hook has rewritten it.
    func reload() {
        let changed = (try? FileManager.default.attributesOfItem(atPath: PageLibrary.listURL.path))?[.modificationDate] as? Date
        guard changed != stamp || pages.isEmpty else { return }
        stamp = changed
        pages = PageLibrary.entries()
        if selected == nil { selection = shown.first?.folder }
    }

    var shown: [PageLibrary.Entry] {
        let words = search.lowercased().split(separator: " ")
        return pages.filter { page in
            (account.isEmpty || page.account == account)
                && words.allSatisfy { "\(page.title) \(page.project) \(page.account)".lowercased().contains($0) }
        }
    }

    var selected: PageLibrary.Entry? { pages.first { $0.folder == selection } }

    var owners: [String] { Array(Set(pages.map(\.account))).filter { !$0.isEmpty }.sorted() }

    var activeEmail: String? { accounts.accounts.first { $0.id == accounts.activeID }?.email }

    func select(_ folder: String) {
        if !shown.contains(where: { $0.folder == folder }) {
            search = ""
            account = ""
        }
        selection = folder
    }

    func remove(_ page: PageLibrary.Entry) {
        PageLibrary.remove(page)
        let next = shown.drop { $0.folder != page.folder }.dropFirst().first
        stamp = nil
        pages = PageLibrary.entries()
        selection = next?.folder ?? shown.first?.folder
    }
}

// MARK: - Views

struct PagesView: View {
    @ObservedObject var model: PagesModel

    var body: some View {
        HSplitView {
            PageList(model: model)
                .frame(minWidth: 240, maxWidth: 340)
            Group {
                if let page = model.selected {
                    PageDetail(page: page, model: model)
                } else {
                    Text(model.pages.isEmpty ? "Artifacts you publish from now on appear here." : "Pick an artifact.")
                        .foregroundColor(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .frame(minWidth: 480)
        }
        .frame(minWidth: 760, minHeight: 480)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

private struct PageList: View {
    @ObservedObject var model: PagesModel

    var body: some View {
        VStack(spacing: 0) {
            VStack(spacing: 8) {
                TextField("Search", text: $model.search)
                    .textFieldStyle(.roundedBorder)
                if model.owners.count > 1 {
                    Picker("", selection: $model.account) {
                        Text("All Accounts").tag("")
                        ForEach(model.owners, id: \.self) { Text($0).tag($0) }
                    }
                    .labelsHidden()
                    .pointerCursor()
                }
            }
            .padding(12)
            Divider()
            List(model.shown, selection: $model.selection) { page in
                PageRow(page: page, inUse: page.account == model.activeEmail)
                    .tag(page.folder)
            }
            .listStyle(.inset)
            Divider()
            Text(model.shown.count == 1 ? "1 artifact" : "\(model.shown.count) artifacts")
                .font(.caption)
                .foregroundColor(.secondary)
                .padding(8)
        }
    }
}

private struct PageRow: View {
    let page: PageLibrary.Entry
    let inUse: Bool

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(page.title).lineLimit(2)
            Text([page.project, PageDates.short(page.updated)].filter { !$0.isEmpty }.joined(separator: " · "))
                .font(.caption)
                .foregroundColor(.secondary)
                .lineLimit(1)
            if !page.account.isEmpty {
                HStack(spacing: 4) {
                    Circle().fill(inUse ? Color.green : Color.secondary.opacity(0.4)).frame(width: 6, height: 6)
                    Text(page.account).font(.caption).foregroundColor(.secondary).lineLimit(1).truncationMode(.middle)
                }
            }
        }
        .padding(.vertical, 3)
    }
}

private struct PageDetail: View {
    let page: PageLibrary.Entry
    @ObservedObject var model: PagesModel
    @State private var removing = false

    private var exists: Bool { FileManager.default.fileExists(atPath: page.localURL.path) }

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 8) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(page.title).font(.headline).lineLimit(1)
                    Text(subtitle).font(.caption).foregroundColor(.secondary).lineLimit(1)
                }
                Spacer(minLength: 12)
                Button("Open in Browser") { NSWorkspace.shared.open(page.localURL) }
                    .disabled(!exists)
                    .pointerCursor()
                if let online {
                    Button("Open on claude.ai") { NSWorkspace.shared.open(online) }
                        .help(ownerNote ?? "Opens the published page.")
                        .pointerCursor()
                }
                Menu {
                    if !page.url.isEmpty {
                        Button("Copy Link") {
                            NSPasteboard.general.clearContents()
                            NSPasteboard.general.setString(page.url, forType: .string)
                        }
                    }
                    Button("Show in Finder") { NSWorkspace.shared.activateFileViewerSelecting([page.localURL]) }
                    Divider()
                    Button("Remove Local Copy…") { removing = true }
                } label: {
                    Image(systemName: "ellipsis.circle")
                }
                .menuStyle(.borderlessButton)
                .fixedSize()
                .pointerCursor()
            }
            .padding(12)
            if let note = ownerNote {
                Divider()
                Label(note, systemImage: "person.crop.circle.badge.exclamationmark")
                    .font(.caption)
                    .foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.vertical, 6)
            }
            Divider()
            if exists {
                PagePreview(page: page)
            } else {
                Text("The local copy is gone.")
                    .foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .alert("Remove \(page.title)?", isPresented: $removing) {
            Button("Remove", role: .destructive) { model.remove(page) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Only the copy on this Mac goes. The page on claude.ai stays.")
        }
    }

    private var online: URL? {
        page.url.hasPrefix("https://claude.ai/") ? URL(string: page.url) : nil
    }

    private var subtitle: String {
        [page.account, page.project, PageDates.long(page.updated)].filter { !$0.isEmpty }.joined(separator: " · ")
    }

    /// Only the account that published a page can open its link, so say so before a click ends in a 404.
    private var ownerNote: String? {
        guard online != nil, !page.account.isEmpty, let active = model.activeEmail, active != page.account else { return nil }
        return "On claude.ai this page opens only for \(page.account). You're using \(active)."
    }
}

private enum PageDates {
    private static let shortFormat: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "MMM d"
        return f
    }()
    private static let longFormat: DateFormatter = {
        let f = DateFormatter()
        f.dateFormat = "MMM d, yyyy HH:mm"
        return f
    }()

    static func short(_ time: Double) -> String { shortFormat.string(from: Date(timeIntervalSince1970: time)) }
    static func long(_ time: Double) -> String { longFormat.string(from: Date(timeIntervalSince1970: time)) }
}

/// Shows the saved file itself. Links leave for the default browser; the page can read only its own folder.
private struct PagePreview: NSViewRepresentable {
    let page: PageLibrary.Entry

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> WKWebView {
        let web = WKWebView(frame: .zero)
        web.navigationDelegate = context.coordinator
        web.uiDelegate = context.coordinator
        return web
    }

    func updateNSView(_ web: WKWebView, context: Context) {
        // A republish changes `updated`; a timer tick changes nothing and mustn't reload.
        let key = "\(page.folder)|\(page.updated)"
        guard context.coordinator.loaded != key else { return }
        let republished = context.coordinator.loaded?.hasPrefix(page.folder + "|") == true
        context.coordinator.loaded = key
        guard republished else {
            context.coordinator.scroll = nil
            load(in: web)
            return
        }
        // Reloading under the reader keeps their place.
        web.evaluateJavaScript("window.scrollY") { y, _ in
            context.coordinator.scroll = y as? Double
            load(in: web)
        }
    }

    private func load(in web: WKWebView) {
        if page.file.hasSuffix(".md") {
            let text = (try? String(contentsOf: page.localURL, encoding: .utf8)) ?? ""
            web.loadHTMLString("""
            <!doctype html><meta charset="utf-8"><style>:root{color-scheme:light dark}
            pre{margin:24px;white-space:pre-wrap;font:13px/1.5 ui-monospace,monospace}</style>
            <pre>\(PageLibrary.escape(text))</pre>
            """, baseURL: nil)
        } else {
            web.loadFileURL(page.localURL, allowingReadAccessTo: page.localURL.deletingLastPathComponent())
        }
    }

    final class Coordinator: NSObject, WKNavigationDelegate, WKUIDelegate {
        var loaded: String?
        var scroll: Double?

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            guard let scroll else { return }
            self.scroll = nil
            webView.evaluateJavaScript("window.scrollTo(0, \(scroll))")
        }

        func webView(_ webView: WKWebView, decidePolicyFor action: WKNavigationAction,
                     decisionHandler: @escaping (WKNavigationActionPolicy) -> Void) {
            guard action.targetFrame?.isMainFrame != false, let url = action.request.url,
                  !url.isFileURL, url.scheme != "about" else {
                decisionHandler(.allow)
                return
            }
            if action.navigationType == .linkActivated { NSWorkspace.shared.open(url) }
            decisionHandler(.cancel)
        }

        /// `target=_blank` links.
        func webView(_ webView: WKWebView, createWebViewWith configuration: WKWebViewConfiguration,
                     for action: WKNavigationAction, windowFeatures: WKWindowFeatures) -> WKWebView? {
            if let url = action.request.url, url.scheme == "https" || url.scheme == "http" {
                NSWorkspace.shared.open(url)
            }
            return nil
        }
    }
}

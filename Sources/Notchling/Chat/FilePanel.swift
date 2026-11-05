import SwiftUI
import Quartz
import PDFKit
import AVKit
import UniformTypeIdentifiers
import WebKit

/// How a file from a message is shown beside the chat.
enum FilePreviewKind: Equatable {
    /// Rendered like a message, so a README reads as it does on GitHub.
    case markdown
    /// HTML and SVG in a web view that can read only the file's folder.
    case web
    /// Code, logs, JSON and other text: monospaced and selectable.
    case text
    /// Fitted to the panel.
    case image
    /// PDFKit: scrolls, zooms, selects text.
    case pdf
    /// Video and audio with the system's player controls.
    case media
    /// Office and iWork files and anything else: Quick Look.
    case quickLook

    private static let markdownExtensions: Set<String> = ["md", "markdown", "mdx", "mdown"]
    private static let webExtensions: Set<String> = ["html", "htm", "xhtml", "svg"]
    /// Text Quick Look would show as a binary icon, or that has no registered type.
    private static let textExtensions: Set<String> = [
        "ts", "tsx", "js", "jsx", "mjs", "cjs", "json", "jsonl", "yaml", "yml", "toml", "ini", "env", "lock",
        "swift", "go", "rs", "py", "rb", "java", "kt", "c", "h", "cc", "cpp", "hpp", "m", "mm", "cs", "php",
        "sh", "zsh", "bash", "fish", "sql", "graphql", "proto", "css", "scss", "less", "vue", "svelte", "astro",
        "txt", "log", "csv", "tsv", "xml", "plist", "gradle", "dockerfile", "makefile", "gitignore", "conf", "cfg",
    ]

    static func of(_ url: URL, sniff: (URL) -> Data? = { try? FileHandle(forReadingFrom: $0).read(upToCount: 8192) }) -> Self {
        let ext = url.pathExtension.lowercased()
        if markdownExtensions.contains(ext) { return .markdown }
        if webExtensions.contains(ext) { return .web }
        if textExtensions.contains(ext) || textExtensions.contains(url.lastPathComponent.lowercased()) { return .text }
        if let type = UTType(filenameExtension: ext) {
            if type.conforms(to: .pdf) { return .pdf }
            if type.conforms(to: .image) { return .image }
            if type.conforms(to: .audiovisualContent) { return .media }
            if [.presentation, .spreadsheet, .rtf, .rtfd].contains(where: type.conforms) { return .quickLook }
            if type.conforms(to: .text) || type.conforms(to: .sourceCode) { return .text }
        }
        // No known type: text if the start of it reads as UTF-8 without NULs.
        if let head = sniff(url), !head.contains(0), String(data: head, encoding: .utf8) != nil { return .text }
        return .quickLook
    }
}

/// A file opened from a link in the conversation, beside it like a saved Artifact. Reloads when the
/// file changes on disk (the session editing it). Escape or × closes it.
struct FilePanel: View {
    @ObservedObject var model: ChatModel
    let url: URL
    let maxWidth: CGFloat
    @State private var version: Stamp
    private let tick = Timer.publish(every: 1.5, on: .main, in: .common).autoconnect()

    init(model: ChatModel, url: URL, maxWidth: CGFloat) {
        self.model = model
        self.url = url
        self.maxWidth = maxWidth
        _version = State(initialValue: Self.stamp(of: url))
    }

    var body: some View {
        let kind = FilePreviewKind.of(url)
        SidePanel(maxWidth: maxWidth) {
            HStack(spacing: 8) {
                Image(nsImage: NSWorkspace.shared.icon(forFile: url.path)).resizable().frame(width: 20, height: 20)
                VStack(alignment: .leading, spacing: 1) {
                    Text(url.lastPathComponent).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                    Text(details).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1).truncationMode(.head)
                }
                .help(url.path)
                Spacer(minLength: 4)
                Button { NSWorkspace.shared.open(url) } label: {
                    Image(systemName: "arrow.up.forward.app").font(.system(size: 12, weight: .medium))
                }
                .buttonStyle(.kit(.ghost)).help("Open in its app")
                Button { NSWorkspace.shared.activateFileViewerSelecting([url]) } label: {
                    Image(systemName: "folder").font(.system(size: 12, weight: .medium))
                }
                .buttonStyle(.kit(.ghost)).help("Show in Finder")
                Button { withAnimation(KitMotion.appear) { model.file = nil } } label: {
                    Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                }
                .buttonStyle(.kit(.ghost)).help("Close (Esc)")
            }
            .padding(.leading, 14).padding(.trailing, 8).frame(height: 48)
        } content: {
            Group {
                if version.size == nil {
                    // Deleted or moved since the message named it (a job's tmp folder goes with the job).
                    VStack(spacing: 6) {
                        Text("This file is gone").kitType(.heading).foregroundColor(KitInk.text)
                        Text("It was moved or deleted after the message named it.").kitType(.meta).foregroundColor(KitInk.textMuted)
                    }
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else {
                switch kind {
                case .markdown: MarkdownFileView(url: url, version: version)
                case .web: WebFileView(url: url, version: version)
                case .text: TextFileView(url: url, version: version)
                case .image: ImageFileView(url: url, version: version)
                case .pdf: PDFFileView(url: url, version: version)
                case .media: MediaFileView(url: url)
                case .quickLook: QuickLookView(url: url, version: version)
                }
                }
            }
            .id(url)
        }
        .onChange(of: url) { version = Self.stamp(of: $0) }
        .onReceive(tick) { _ in
            let now = Self.stamp(of: url)
            if now != version { version = now }
        }
    }

    private var details: String {
        let folder = (url.deletingLastPathComponent().path as NSString).abbreviatingWithTildeInPath
        guard let size = version.size else { return folder + " · not found" }
        return folder + " · " + ByteCountFormatter.string(fromByteCount: Int64(size), countStyle: .file)
    }

    struct Stamp: Equatable {
        var size: Int?
        var modified: Date?
    }

    static func stamp(of url: URL) -> Stamp {
        let values = try? url.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey])
        return Stamp(size: values?.fileSize, modified: values?.contentModificationDate)
    }
}

/// The right-hand panel's frame: a draggable left edge (width remembered), a header, the content.
struct SidePanel<Header: View, Content: View>: View {
    let maxWidth: CGFloat
    @ViewBuilder let header: () -> Header
    @ViewBuilder let content: () -> Content
    @AppStorage("chatPageWidth") private var width: Double = 520
    @State private var dragStart: Double?

    var body: some View {
        let shown = min(max(320, width), maxWidth)
        HStack(spacing: 0) {
            Rectangle().fill(KitInk.hairline).frame(width: 1)
                .overlay(Color.clear.frame(width: 9).contentShape(Rectangle())
                    .onHover { inside in if inside { NSCursor.resizeLeftRight.push() } else { NSCursor.pop() } }
                    .gesture(DragGesture(minimumDistance: 1, coordinateSpace: .global)
                        .onChanged { drag in
                            let start = dragStart ?? shown
                            dragStart = start
                            width = min(max(320, start - drag.translation.width), maxWidth)
                        }
                        .onEnded { _ in dragStart = nil }))
            VStack(spacing: 0) {
                header()
                Rectangle().fill(KitInk.hairline).frame(height: 1)
                content()
            }
        }
        .frame(width: shown)
    }
}

// MARK: - Viewers

/// Files are read whole; past this they show the start, so a huge log can't stall the window.
private let textLimit = 2_000_000

private func readText(_ url: URL) -> (text: String, cut: Bool) {
    guard let handle = try? FileHandle(forReadingFrom: url) else { return ("", false) }
    defer { try? handle.close() }
    let data = (try? handle.read(upToCount: textLimit + 1)) ?? Data()
    let cut = data.count > textLimit
    let text = String(decoding: cut ? data.prefix(textLimit) : data, as: UTF8.self)
    return (text, cut)
}

private struct MarkdownFileView: View {
    let url: URL
    let version: FilePanel.Stamp
    @State private var text = ""

    var body: some View {
        ScrollView {
            MarkdownView(text: text)
                .padding(20)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
        // Relative links in the file are read against its own folder.
        .environment(\.chatFolder, url.deletingLastPathComponent().path)
        .onAppear { text = readText(url).text }
        .onChange(of: version) { _ in text = readText(url).text }
    }
}

/// Monospaced, selectable, scrollable; one NSTextView, so a long file stays fast.
private struct TextFileView: NSViewRepresentable {
    let url: URL
    let version: FilePanel.Stamp

    func makeNSView(context: Context) -> NSScrollView {
        let scroll = NSTextView.scrollableTextView()
        scroll.drawsBackground = false
        guard let view = scroll.documentView as? NSTextView else { return scroll }
        view.isEditable = false
        view.isSelectable = true
        view.isRichText = false
        view.drawsBackground = false
        view.textContainerInset = NSSize(width: 14, height: 14)
        view.font = .monospacedSystemFont(ofSize: 12, weight: .regular)
        view.textColor = NSColor(KitInk.text)
        // Long lines scroll sideways rather than wrapping, as in an editor.
        view.isHorizontallyResizable = true
        view.textContainer?.widthTracksTextView = false
        view.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
        scroll.hasHorizontalScroller = true
        return scroll
    }

    func updateNSView(_ scroll: NSScrollView, context: Context) {
        guard context.coordinator.shown != version, let view = scroll.documentView as? NSTextView else { return }
        context.coordinator.shown = version
        let (text, cut) = readText(url)
        let place = scroll.contentView.bounds.origin
        view.string = cut ? text + "\n\n… (first 2 MB shown)" : text
        scroll.contentView.scroll(to: place)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }
    final class Coordinator { var shown: FilePanel.Stamp? }
}

private struct WebFileView: NSViewRepresentable {
    let url: URL
    let version: FilePanel.Stamp

    func makeNSView(context: Context) -> WKWebView { WKWebView(frame: .zero) }

    func updateNSView(_ web: WKWebView, context: Context) {
        guard context.coordinator.shown != version else { return }
        context.coordinator.shown = version
        web.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())
    }

    func makeCoordinator() -> Coordinator { Coordinator() }
    final class Coordinator { var shown: FilePanel.Stamp? }
}

private struct ImageFileView: NSViewRepresentable {
    let url: URL
    let version: FilePanel.Stamp

    func makeNSView(context: Context) -> NSImageView {
        let view = NSImageView()
        view.imageScaling = .scaleProportionallyDown
        view.imageAlignment = .alignCenter
        view.animates = true
        view.isEditable = false
        view.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        view.setContentCompressionResistancePriority(.defaultLow, for: .vertical)
        return view
    }

    func updateNSView(_ view: NSImageView, context: Context) {
        guard context.coordinator.shown != version else { return }
        context.coordinator.shown = version
        view.image = NSImage(contentsOf: url)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }
    final class Coordinator { var shown: FilePanel.Stamp? }
}

private struct PDFFileView: NSViewRepresentable {
    let url: URL
    let version: FilePanel.Stamp

    func makeNSView(context: Context) -> PDFView {
        let view = PDFView()
        view.autoScales = true
        view.displayMode = .singlePageContinuous
        view.backgroundColor = .clear
        return view
    }

    func updateNSView(_ view: PDFView, context: Context) {
        guard context.coordinator.shown != version else { return }
        context.coordinator.shown = version
        view.document = PDFDocument(url: url)
    }

    func makeCoordinator() -> Coordinator { Coordinator() }
    final class Coordinator { var shown: FilePanel.Stamp? }
}

/// Doesn't start by itself; reloading a playing video on every change would be worse than a stale one.
private struct MediaFileView: NSViewRepresentable {
    let url: URL

    func makeNSView(context: Context) -> AVPlayerView {
        let view = AVPlayerView()
        view.controlsStyle = .inline
        view.player = AVPlayer(url: url)
        return view
    }

    func updateNSView(_ view: AVPlayerView, context: Context) {}

    static func dismantleNSView(_ view: AVPlayerView, coordinator: ()) {
        view.player?.pause()
        view.player = nil
    }
}

private struct QuickLookView: NSViewRepresentable {
    let url: URL
    let version: FilePanel.Stamp

    func makeNSView(context: Context) -> QLPreviewView {
        let view = QLPreviewView(frame: .zero, style: .normal) ?? QLPreviewView()
        view.autostarts = false
        return view
    }

    func updateNSView(_ view: QLPreviewView, context: Context) {
        guard context.coordinator.shown != version else { return }
        let first = context.coordinator.shown == nil
        context.coordinator.shown = version
        if first { view.previewItem = url as NSURL } else { view.refreshPreviewItem() }
    }

    static func dismantleNSView(_ view: QLPreviewView, coordinator: Coordinator) {
        view.close()
    }

    func makeCoordinator() -> Coordinator { Coordinator() }
    final class Coordinator { var shown: FilePanel.Stamp? }
}

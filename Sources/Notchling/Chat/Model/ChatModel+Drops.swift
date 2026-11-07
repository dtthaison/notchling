import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    /// What the conversation takes as a drop: files (Finder), and images (the screenshot thumbnail,
    /// Photos, a browser), which come as file promises or data rather than a file link.
    static let dropTypes: [UTType] = [.fileURL, .image]

    /// Where dropped images and temporary files are kept, so the path still works when Claude reads it.
    static let dropsDir = HookBridge.dir.appendingPathComponent("drops")

    /// Attaches each dropped item to the reply: a thumbnail above the box, its path sent after the text.
    /// A file in a temporary folder (the screenshot thumbnail's goes away within seconds) and an image
    /// with no file are copied to `dropsDir` first.
    func receiveDrop(_ providers: [NSItemProvider], for id: String) -> Bool {
        var took = false
        for provider in providers {
            let insert: (URL) -> Void = { [weak self] url in
                DispatchQueue.main.async {
                    self?.attach(url, for: id)
                    self?.focusRequest += 1
                }
            }
            if provider.hasItemConformingToTypeIdentifier(UTType.fileURL.identifier) {
                took = true
                _ = provider.loadObject(ofClass: URL.self) { url, _ in
                    guard let url, url.isFileURL else { return }
                    insert(Self.isTemporary(url) ? (Self.keep(url) ?? url) : url)
                }
            } else if let type = provider.registeredTypeIdentifiers.first(where: { UTType($0)?.conforms(to: .image) == true }) {
                took = true
                // The file handed over here is deleted when this returns, so copy it now.
                provider.loadFileRepresentation(forTypeIdentifier: type) { url, _ in
                    guard let url, let kept = Self.keep(url, name: provider.suggestedName, type: UTType(type)) else { return }
                    insert(kept)
                }
            }
        }
        return took
    }

    private static func isTemporary(_ url: URL) -> Bool {
        let path = url.resolvingSymlinksInPath().path
        let temp = URL(fileURLWithPath: NSTemporaryDirectory()).resolvingSymlinksInPath().deletingLastPathComponent().path
        return path.hasPrefix(temp) || path.hasPrefix("/private/var/folders/") || path.contains("/TemporaryItems/")
    }

    /// Copies a file into `dropsDir` under its own name (made unique), dropping copies over a week old.
    private static func keep(_ url: URL, name: String? = nil, type: UTType? = nil) -> URL? {
        let fm = FileManager.default
        try? fm.createDirectory(at: dropsDir, withIntermediateDirectories: true)
        let week = Date().addingTimeInterval(-7 * 86400)
        for old in (try? fm.contentsOfDirectory(at: dropsDir, includingPropertiesForKeys: [.contentModificationDateKey])) ?? [] {
            if let date = (try? old.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate, date < week {
                try? fm.removeItem(at: old)
            }
        }
        var base = (name?.isEmpty == false ? name! : url.deletingPathExtension().lastPathComponent)
            .replacingOccurrences(of: "/", with: "-")
        var ext = url.pathExtension
        if ext.isEmpty { ext = type?.preferredFilenameExtension ?? "" }
        if base.hasSuffix("." + ext) { base = String(base.dropLast(ext.count + 1)) }
        var target = dropsDir.appendingPathComponent(ext.isEmpty ? base : "\(base).\(ext)")
        var n = 2
        while fm.fileExists(atPath: target.path) {
            target = dropsDir.appendingPathComponent(ext.isEmpty ? "\(base) \(n)" : "\(base) \(n).\(ext)")
            n += 1
        }
        do { try fm.copyItem(at: url, to: target) } catch { return nil }
        return target
    }

    func attach(_ url: URL, for id: String) {
        var list = attachments[id] ?? []
        guard !list.contains(url) else { return }
        list.append(url)
        attachments[id] = list
    }

    func detach(_ url: URL, for id: String) {
        attachments[id]?.removeAll { $0 == url }
        if attachments[id]?.isEmpty == true { attachments[id] = nil }
    }

    /// What Return sends: the typed text, then each attached file's path, the way Claude Code's terminal
    /// takes a dropped file.
    func composed(_ id: String) -> String {
        Self.compose(drafts[id] ?? "", paths: (attachments[id] ?? []).map(\.path))
    }

    func clearComposer(_ id: String) {
        drafts[id] = ""
        attachments[id] = nil
    }

    /// The text with each path after it, quoted when it has spaces or quotes.
    static func compose(_ text: String, paths: [String]) -> String {
        let quoted = paths.map { p in p.contains(where: { $0 == " " || $0 == "'" || $0 == "\"" }) ? "\"\(p)\"" : p }
        let body = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return ([body].filter { !$0.isEmpty } + quoted).joined(separator: " ")
    }
}

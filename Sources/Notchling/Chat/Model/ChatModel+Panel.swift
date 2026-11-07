import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    /// A link in the conversation: a saved Artifact opens in the panel beside the chat (readable from
    /// any account), anything else in the browser.
    func open(_ url: URL) -> OpenURLAction.Result {
        // A path from a message (`ChatPaths`): a folder opens in Finder, a file beside the chat.
        if url.isFileURL {
            if url.hasDirectoryPath || ChatPaths.isFolder(url.path) {
                NSWorkspace.shared.open(url)
            } else {
                withAnimation(KitMotion.appear) { file = url.standardizedFileURL }
            }
            return .handled
        }
        // A command in inline code: one click is easy to make by accident, so ask first.
        if let (command, folder) = ChatPaths.parseRunLink(url) {
            let alert = NSAlert()
            alert.messageText = "Run This Command?"
            let dir = folder ?? selected?.cwd ?? ""
            alert.informativeText = command + "\n\nIn " + (dir as NSString).abbreviatingWithTildeInPath
            alert.addButton(withTitle: "Run")
            alert.addButton(withTitle: "Cancel")
            if alert.runModal() == .alertFirstButtonReturn { runCommand(command, in: folder) }
            return .handled
        }
        if PageLibrary.artifactID(in: url.absoluteString) != nil,
           let found = PageLibrary.find(url.absoluteString).first {
            withAnimation(KitMotion.appear) { page = found }
            return .handled
        }
        return .systemAction
    }

    /// The panel's page again from the library, so a republish shows up.
    func refreshPage() {
        guard let shown = page, !shown.url.isEmpty else { return }
        let stamp = PageLibrary.listModified
        if let checked = pageChecked, checked == stamp { return }
        pageChecked = stamp
        guard let fresh = PageLibrary.find(shown.url).first(where: { $0.folder == shown.folder }),
              fresh != shown else { return }
        page = fresh
    }
}

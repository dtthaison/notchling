import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// The Chat window: every Claude Code session in a sidebar, its conversation, and a reply box for
/// background sessions. Replies take the notch's local path (`claude attach` in a pty), so nothing
/// listens on a port. Questions and permissions go through `NotchModel`, so the notch and this
/// window never answer the same prompt twice. Design notes: docs/chat-design.md.
final class ChatWindowController: NSObject, NSWindowDelegate {
    private let model: ChatModel
    private var window: EditingWindow?
    private var timer: Timer?
    private var watch: AnyCancellable?
    private var pageWatch: AnyCancellable?

    init(notch: NotchModel, accounts: AccountStore? = nil, codex: CodexStore? = nil, antigravity: AntigravityStore? = nil) {
        model = ChatModel(notch: notch, accounts: accounts, codex: codex, antigravity: antigravity)
    }

    /// A usage-limit switch, shown in the sessions it moved.
    func noteLimitSwitch(_ text: String) { model.noteLimitSwitch(text) }

    var onOpenArtifact: ((String) -> Bool)? {
        get { model.onOpenArtifact }
        set { model.onOpenArtifact = newValue }
    }

    /// Chat was open when Notchling last quit (an install relaunches it), and the session it showed.
    private static let openKey = "chatOpen", selectionKey = "chatSelection"

    /// At launch: reopens Chat if it was open when Notchling quit, on the same session, without taking focus.
    func restore() {
        guard UserDefaults.standard.bool(forKey: Self.openKey) else { return }
        show(select: UserDefaults.standard.string(forKey: Self.selectionKey), activate: false)
    }

    /// Opens Chat with a saved page in the panel beside the conversation (from search or the menu).
    func showPage(folder: String) {
        guard let entry = PageLibrary.entries().first(where: { $0.folder == folder }) else { return }
        show()
        withAnimation(KitMotion.appear) { model.page = entry }
    }

    /// Opens Chat on the session with the terminal sheet running the command (from the notch).
    func showTerminal(select id: String, command: String) {
        show(select: id)
        model.openTerminal(typing: command)
    }

    /// Opens Chat on a conversation found by search (Claude Code, Codex or Antigravity CLI).
    func showSession(_ session: SessionMeta) {
        show(select: model.prepareSession(session))
    }

    func show(select id: String? = nil, activate: Bool = true) {
        UserDefaults.standard.set(true, forKey: Self.openKey)
        if let id { model.selection = id } else if model.selected == nil { model.selection = model.notch.agents.first?.id }
        if window == nil {
            let window = EditingWindow(contentViewController: NSHostingController(rootView: ChatView(model: model, notch: model.notch)))
            window.styleMask = [.titled, .closable, .miniaturizable, .resizable, .fullSizeContentView]
            window.titlebarAppearsTransparent = true
            window.titleVisibility = .hidden
            window.isReleasedWhenClosed = false
            window.delegate = self
            window.setContentSize(NSSize(width: 1040, height: 720))
            window.contentMinSize = NSSize(width: 720, height: 460)
            window.center()
            window.setFrameAutosaveName("Notchling Chat")
            window.keyHandler = { [weak self] event in self?.model.handleKey(event.keyCode) ?? false }
            self.window = window
            // The title (hidden, but used by Mission Control and the Window menu) names the session.
            watch = model.$selection.sink { [weak self] id in
                UserDefaults.standard.set(id, forKey: Self.selectionKey)
                DispatchQueue.main.async { self?.window?.title = self?.model.selected?.name ?? "Chat" }
            }
            // A page needs room beside the chat: widen a narrow window, within the screen.
            pageWatch = model.$page.map { $0 != nil }.removeDuplicates().filter { $0 }.sink { [weak self] _ in
                guard let window = self?.window, let screen = window.screen?.visibleFrame else { return }
                var frame = window.frame
                let want = min(1360, screen.width)
                guard frame.width < want else { return }
                frame.size.width = want
                frame.origin.x = min(max(screen.minX, frame.origin.x - (want - window.frame.width) / 2), screen.maxX - want)
                window.setFrame(frame, display: true, animate: true)
            }
            // Transcripts change under us while sessions work; re-reading is cheap when nothing did.
            timer = Timer.scheduledTimer(withTimeInterval: 1, repeats: true) { [weak self] _ in self?.model.refresh() }
        }
        model.refresh()
        guard activate else { window?.orderFront(nil); return }
        NSApp.activate(ignoringOtherApps: true)
        window?.makeKeyAndOrderFront(nil)
        // Open calm: a background session's reply box takes focus; otherwise the sidebar does, so the
        // arrow keys work at once. Never the search field.
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.05) { [weak self] in
            guard let self else { return }
            if self.model.selected?.canType == true { self.model.focusRequest += 1 } else { self.model.sidebarFocusRequest += 1 }
        }
    }

    func windowWillClose(_ notification: Notification) {
        // Closed by you, not by quitting: don't bring it back at the next launch.
        UserDefaults.standard.set(false, forKey: Self.openKey)
        timer?.invalidate()
        timer = nil
        watch = nil
        pageWatch = nil
        DispatchQueue.main.async { self.window = nil }
    }
}

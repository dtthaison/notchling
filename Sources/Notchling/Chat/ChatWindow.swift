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

    init(notch: NotchModel, accounts: AccountStore? = nil) {
        model = ChatModel(notch: notch, accounts: accounts)
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
                if let id { UserDefaults.standard.set(id, forKey: Self.selectionKey) }
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
            if self.model.selected?.canReply == true { self.model.focusRequest += 1 } else { self.model.sidebarFocusRequest += 1 }
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

final class ChatModel: ObservableObject {
    /// A reply on its way: shown in the conversation until the transcript has it.
    struct Pending: Equatable {
        let text: String
        let at: Date
        var sending: Bool

        /// The transcript already shows this reply: the same words, or any message of yours written
        /// since it was sent (a long paste lands in another shape). Until then it shows as pending.
        func landed(in conversation: Conversation) -> Bool {
            let want = Self.words(text)
            return conversation.items.suffix(12).contains { item in
                guard case .user(let said, _) = item.kind else { return false }
                guard let date = item.date else { return Self.words(said) == want }
                return date >= at.addingTimeInterval(-2) || (Self.words(said) == want && date >= at.addingTimeInterval(-60))
            }
        }

        private static func words(_ text: String) -> String {
            text.split(whereSeparator: \.isWhitespace).joined(separator: " ")
        }
    }

    let notch: NotchModel
    @Published var selection: String? {
        didSet {
            guard selection != oldValue else { return }
            conversation = Conversation()
            completionIndex = 0
            refresh()
        }
    }
    @Published private(set) var conversation = Conversation()
    /// Which tool steps are open; shared so a refresh never folds them.
    let expansion = ChatExpansion()
    /// The session details panel on the right.
    @Published var showInspector = UserDefaults.standard.object(forKey: "chatInspector") as? Bool ?? false {
        didSet { UserDefaults.standard.set(showInspector, forKey: "chatInspector") }
    }
    /// Opens a saved Artifact in the Artifacts window; false when there's no local copy.
    var onOpenArtifact: ((String) -> Bool)?
    /// The Artifact open in the panel beside the chat.
    @Published var page: PageLibrary.Entry?
    @Published var drafts: [String: String] = [:]
    @Published private(set) var pending: [String: Pending] = [:]
    @Published var errors: [String: String] = [:]
    @Published var completionIndex = 0
    @Published var search = ""
    /// Bumped to put the cursor in the reply box (the window opening, a session picked from the notch).
    @Published var focusRequest = 0
    /// Bumped to give the sidebar keyboard focus.
    @Published var sidebarFocusRequest = 0
    var composerFocused = false
    /// Sessions seen, so one that drops off the list stays readable while it's open.
    private var seen: [String: Agent] = [:]
    private var watch: AnyCancellable?

    init(notch: NotchModel, accounts: AccountStore? = nil) {
        self.notch = notch
        self.accounts = accounts
        watch = notch.$agents.sink { [weak self] list in
            for agent in list { self?.seen[agent.id] = agent }
        }
        accountsWatch = accounts?.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async { self?.objectWillChange.send() }
        }
    }

    // MARK: Accounts (lanes)

    /// The account a session runs on: its lane's, or the main login's. Nil when unknown (an ended session).
    struct SessionAccount: Equatable {
        let email: String
        let lane: Lane?
    }

    let accounts: AccountStore?
    private var accountsWatch: AnyCancellable?
    /// Lanes by session id, from the lane folder each session runs in. Re-read every few seconds, since a
    /// switch changes a lane's account.
    @Published private(set) var lanes: [String: Lane] = [:]
    /// Lanes exist, so sessions can be on different accounts: the sidebar then names each one's account.
    @Published private(set) var lanesInUse = false
    private var lanesChecked = Date.distantPast
    /// Limit switches by lane name ("" for the main login), shown in the sessions they moved.
    @Published var accountNotes: [String: AccountNote] = [:]
    @Published var accountErrors: [String: String] = [:]

    struct AccountNote: Equatable {
        let text: String
        let at: Date
    }

    func account(for agent: Agent) -> SessionAccount? {
        if let lane = lanes[agent.id] { return SessionAccount(email: lane.email, lane: lane) }
        // In a lane folder the lanes list doesn't name (yet): don't guess.
        if agent.configDir != nil { return nil }
        return accounts?.active.map { SessionAccount(email: $0.email, lane: nil) }
    }

    /// Saved accounts no session uses: not signed out, not the main login, not in a lane.
    var freeAccounts: [ClaudeAccount] {
        guard let accounts else { return [] }
        let taken = Set(LaneStore.all().map(\.accountID))
        return accounts.accounts.filter { !$0.needsLogin && $0.id != accounts.activeID && !taken.contains($0.id) }
    }

    private func refreshLanes(force: Bool = false) {
        guard force || Date().timeIntervalSince(lanesChecked) > 5 else { return }
        lanesChecked = Date()
        let agents = notch.agents
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let all = LaneStore.all()
            let inUse = !all.isEmpty
            var found: [String: Lane] = [:]
            for agent in agents {
                if let dir = agent.configDir, let lane = all.first(where: { $0.path == dir }) { found[agent.id] = lane }
            }
            DispatchQueue.main.async {
                guard let self else { return }
                if self.lanes != found { self.lanes = found }
                if self.lanesInUse != inUse { self.lanesInUse = inUse }
            }
        }
    }

    /// Moves the session's lane (every session in it) or the main login (every session on it) to `accountID`.
    func switchAccount(of agent: Agent, to accountID: String) {
        guard let accounts, let current = account(for: agent) else { return }
        let id = agent.id
        accountErrors[id] = nil
        let finish = { [weak self] (error: Error?) in
            guard let self else { return }
            if let error { self.accountErrors[id] = error.localizedDescription }
            self.refreshLanes(force: true)
        }
        if let lane = current.lane {
            accounts.switchLane(lane.name, to: accountID) { result in
                if case .failure(let error) = result { finish(error) } else { finish(nil) }
            }
        } else {
            accounts.switchTo(accountID) { finish($0) }
        }
    }

    /// Opens Terminal in the session's folder with a new session on `accountID`, in that account's lane.
    func newTerminalSession(for agent: Agent, on accountID: String) {
        guard let accounts else { return }
        let id = agent.id
        let folder = agent.cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser.path : agent.cwd
        accountErrors[id] = nil
        accounts.openLane(for: accountID) { [weak self] result in
            do {
                try LaneStore.openTerminal(try result.get(), in: folder)
            } catch {
                self?.accountErrors[id] = error.localizedDescription
            }
            self?.refreshLanes(force: true)
        }
    }

    /// What the conversation takes as a drop: files (Finder), and images (the screenshot thumbnail,
    /// Photos, a browser), which come as file promises or data rather than a file link.
    static let dropTypes: [UTType] = [.fileURL, .image]

    /// Where dropped images and temporary files are kept, so the path still works when Claude reads it.
    static let dropsDir = HookBridge.dir.appendingPathComponent("drops")

    /// Adds each dropped item's path to the reply. A file in a temporary folder (the screenshot
    /// thumbnail's goes away within seconds) and an image with no file are copied to `dropsDir` first.
    func receiveDrop(_ providers: [NSItemProvider], for id: String) -> Bool {
        var took = false
        for provider in providers {
            let insert: (URL) -> Void = { [weak self] url in
                DispatchQueue.main.async {
                    self?.insertPath(url.path, for: id)
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

    /// Adds a dropped file's path to the reply, quoted when it has spaces.
    func insertPath(_ path: String, for id: String) {
        let quoted = path.contains(where: { $0 == " " || $0 == "'" || $0 == "\"" }) ? "\"\(path)\"" : path
        let draft = drafts[id] ?? ""
        drafts[id] = draft + (draft.isEmpty || draft.hasSuffix(" ") || draft.hasSuffix("\n") ? "" : " ") + quoted + " "
    }

    /// Where a new background session can run: the main login, each lane, then each free saved account
    /// (picking one makes a lane for it). `accountID` nil means the main login.
    struct SessionTarget { let title: String; let accountID: String? }

    var sessionTargets: [SessionTarget] {
        guard let accounts else { return [] }
        var out = [SessionTarget(title: "\(accounts.active?.email ?? "Main Login") · main", accountID: nil)]
        out += LaneStore.all().map { SessionTarget(title: "\($0.email) · \($0.name) lane", accountID: $0.accountID) }
        out += freeAccounts.map { SessionTarget(title: $0.email, accountID: $0.id) }
        return out
    }

    /// Project folders for a new session: those of the sessions in the list (newest first), then ones added
    /// with Add Project Folder…, that still exist.
    var projectFolders: [String] {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        var out: [String] = []
        let live = notch.agents.sorted { $0.since > $1.since }.map(\.cwd)
        for folder in live + (UserDefaults.standard.stringArray(forKey: "chatProjectFolders") ?? [])
        where !folder.isEmpty && folder != home && !out.contains(folder) && FileManager.default.fileExists(atPath: folder) {
            out.append(folder)
        }
        return out
    }

    func addProjectFolder(_ folder: String) {
        var saved = UserDefaults.standard.stringArray(forKey: "chatProjectFolders") ?? []
        saved.removeAll { $0 == folder }
        saved.insert(folder, at: 0)
        UserDefaults.standard.set(Array(saved.prefix(30)), forKey: "chatProjectFolders")
    }

    /// The folder's name, with its parent when another listed folder has the same name.
    static func folderTitle(_ folder: String, among all: [String]) -> String {
        let url = URL(fileURLWithPath: folder)
        let name = url.lastPathComponent
        let clash = all.contains { $0 != folder && URL(fileURLWithPath: $0).lastPathComponent == name }
        return clash ? url.deletingLastPathComponent().lastPathComponent + "/" + name : name
    }

    /// Starts an idle background session (`claude --bg`) in `folder` on the target's account, in its lane,
    /// and selects it once it shows up. Errors go to `errorKey`'s banner, or an alert without one.
    func newBackgroundSession(in folder: String, on accountID: String?, errorKey: String? = nil) {
        let fail: (String) -> Void = { [weak self] text in
            if let errorKey { self?.accountErrors[errorKey] = text; return }
            let alert = NSAlert()
            alert.messageText = "Couldn't start the session"
            alert.informativeText = text
            alert.runModal()
        }
        if let errorKey { accountErrors[errorKey] = nil }
        guard let accounts else { return }
        let start: (Lane?) -> Void = { [weak self] lane in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = Self.startBackground(in: folder, configDir: lane?.path)
                DispatchQueue.main.async {
                    switch result {
                    case .success(let shortID): self?.selectWhenListed(shortID)
                    case .failure(let error): fail(error.localizedDescription)
                    }
                    self?.refreshLanes(force: true)
                }
            }
        }
        guard let accountID, accountID != accounts.activeID else { return start(nil) }
        accounts.openLane(for: accountID) { result in
            switch result {
            case .success(let lane): start(lane)
            case .failure(let error): fail(error.localizedDescription)
            }
        }
    }

    /// Moves one background session to another account: stops it (`claude stop` keeps the conversation),
    /// then resumes the same conversation in the background with the target's config folder (`accountID`
    /// nil is the main login; a free account gets a lane). Every other session stays where it is. The
    /// prompt cache doesn't follow (it belongs to the old account), so the first reply re-reads it all.
    func moveSession(_ agent: Agent, to accountID: String?, title: String) {
        guard case .job(let shortID) = agent.target, let accounts, let transcript = agent.transcriptPath else { return }
        let conversation = URL(fileURLWithPath: transcript).deletingPathExtension().lastPathComponent
        let alert = NSAlert()
        alert.messageText = "Move “\(agent.name)” to \(title)?"
        alert.informativeText = (agent.group == .working ? "It's working now; the current step is cut off. " : "")
            + "It stops and resumes the same conversation on that account. Other sessions stay where they are. "
            + "The first reply there re-reads the whole conversation, since the prompt cache stays with the old account."
        alert.addButton(withTitle: "Move")
        alert.addButton(withTitle: "Cancel")
        NSApp.activate(ignoringOtherApps: true)
        guard alert.runModal() == .alertFirstButtonReturn else { return }

        let id = agent.id
        accountErrors[id] = nil
        let flags = Self.launchFlags(forJob: shortID, name: agent.name)
        let cwd = agent.cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser.path : agent.cwd
        let move: (Lane?) -> Void = { [weak self] lane in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = Self.stopJob(shortID).flatMap {
                    Self.startBackground(in: cwd, configDir: lane?.path, arguments: flags + ["--bg", "--resume", conversation])
                }
                DispatchQueue.main.async {
                    switch result {
                    case .success(let newID): self?.selectWhenListed(newID)
                    case .failure(let error): self?.accountErrors[id] = "Couldn't move it: \(error.localizedDescription)"
                    }
                    self?.refreshLanes(force: true)
                }
            }
        }
        guard let accountID, accountID != accounts.activeID else { return move(nil) }
        accounts.openLane(for: accountID) { [weak self] result in
            switch result {
            case .success(let lane): move(lane)
            case .failure(let error): self?.accountErrors[id] = error.localizedDescription
            }
        }
    }

    /// The flags the job was started with (name, model, permission mode), so the moved session keeps them.
    private static func launchFlags(forJob shortID: String, name: String) -> [String] {
        let dir = AgentMonitor.configDir(forJob: shortID).map { URL(fileURLWithPath: $0) } ?? LaneStore.mainDir
        let url = dir.appendingPathComponent("jobs/\(shortID)/state.json")
        guard let data = try? Data(contentsOf: url),
              let obj = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let flags = obj["respawnFlags"] as? [String] else { return ["--name", name] }
        // Never carry a resume or background flag twice.
        var out: [String] = []
        var skipNext = false
        for flag in flags {
            if skipNext { skipNext = false; continue }
            if flag == "--resume" || flag == "-r" { skipNext = true; continue }
            if flag == "--bg" || flag == "--background" { continue }
            out.append(flag)
        }
        return out
    }

    private static func stopJob(_ shortID: String) -> Result<Void, Error> {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
        p.arguments = ["stop", shortID]
        p.environment = AgentMonitor.environment(forJob: shortID)
        let out = Pipe()
        p.standardOutput = out
        p.standardError = out
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch { return .failure(error) }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else {
            let message = String(decoding: data, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
            return .failure(AccountError(message.isEmpty ? "claude stop exited with code \(p.terminationStatus)." : message))
        }
        // Let the host let go of the transcript before the resume opens it.
        Thread.sleep(forTimeInterval: 1.5)
        return .success(())
    }

    private static func startBackground(in folder: String, configDir: String?,
                                        arguments: [String] = ["--bg"]) -> Result<String, Error> {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: ReplySender.claudePath)
        p.arguments = arguments
        p.currentDirectoryURL = URL(fileURLWithPath: folder)
        var env = ProcessInfo.processInfo.environment
        env[LaneStore.variable] = configDir
        p.environment = env
        let out = Pipe()
        p.standardOutput = out
        p.standardError = out
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch { return .failure(error) }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        let text = String(decoding: data, as: UTF8.self)
        // "backgrounded · 6ce4c19c · name (idle — send a prompt to start)"; a resume may word the first
        // part differently, so take the first line whose second part is a short job id.
        for line in text.split(separator: "\n") {
            let parts = line.components(separatedBy: " · ")
            guard parts.count > 1 else { continue }
            let id = parts[1].trimmingCharacters(in: .whitespaces)
            if id.count == 8, id.allSatisfy(\.isHexDigit) { return .success(id) }
        }
        let message = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return .failure(AccountError(message.isEmpty ? "claude --bg exited with code \(p.terminationStatus)." : message))
    }

    /// Opens the new background session `shortID` in Chat once the session list has it (up to 30 s). Its
    /// id starts as the short id and becomes the session id once Claude Code writes it, so keep following
    /// it until then, unless you've picked another session meanwhile.
    private func selectWhenListed(_ shortID: String, tries: Int = 30, picked: String? = nil) {
        if let picked, selection != picked { return }
        var chosen = picked
        if let agent = notch.agents.first(where: { if case .job(let id) = $0.target { return id == shortID }; return false }) {
            if selection != agent.id {
                selection = agent.id
                focusRequest += 1
            }
            chosen = agent.id
            if agent.id != shortID { return }
        }
        guard tries > 0 else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) { [weak self] in
            self?.selectWhenListed(shortID, tries: tries - 1, picked: chosen)
        }
    }

    func noteLimitSwitch(_ text: String) {
        let key: String
        if text.hasPrefix("The "), let end = text.range(of: " lane switched from ") {
            key = String(text[text.index(text.startIndex, offsetBy: 4)..<end.lowerBound])
        } else if text.hasPrefix("Switched from ") {
            key = ""
        } else {
            return
        }
        accountNotes[key] = AccountNote(text: text, at: Date())
        refreshLanes(force: true)
    }

    /// The latest limit switch that moved this session, for an hour after it happened.
    func accountNote(for agent: Agent) -> AccountNote? {
        guard let current = account(for: agent),
              let note = accountNotes[current.lane?.name ?? ""], Date().timeIntervalSince(note.at) < 3600 else { return nil }
        return note
    }

    #if DEMO
    /// Made-up lanes for a probe render; only builds with -D DEMO.
    func demo(lanes: [String: Lane]) {
        self.lanes = lanes
        lanesInUse = true
        lanesChecked = .distantFuture
    }
    #endif

    func dismissAccountNote(for agent: Agent) {
        guard let current = account(for: agent) else { return }
        accountNotes[current.lane?.name ?? ""] = nil
    }

    var selected: Agent? {
        guard let id = selection else { return nil }
        return notch.agents.first { $0.id == id } ?? seen[id]
    }

    /// The sidebar list: the notch's sessions, filtered by the search words.
    var shown: [Agent] {
        let words = search.lowercased().split(separator: " ")
        var list = notch.agents.filter { agent in
            let text = "\(agent.name) \(agent.project) \(agent.host)".lowercased()
            return words.allSatisfy { text.contains($0) }
        }
        if let s = selected, !list.contains(where: { $0.id == s.id }), words.isEmpty { list.append(s) }
        return list
    }

    func refresh() {
        refreshPage()
        if accounts != nil { refreshLanes() }
        guard let agent = selected else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let path = TranscriptReader.path(for: agent) else { return }
            let fresh = ConversationReader.read(path)
            DispatchQueue.main.async {
                guard let self, self.selection == agent.id else { return }
                if fresh != self.conversation { self.conversation = fresh }
                self.dropDelivered(agent.id)
            }
        }
    }

    /// A sent reply leaves the pending row once the transcript shows it (or after a while, since a
    /// long paste can land in the transcript in another shape).
    private func dropDelivered(_ id: String) {
        guard let p = pending[id] else { return }
        // Landed even while the send is still finishing: otherwise it shows twice for a moment.
        if p.landed(in: conversation) || (!p.sending && Date().timeIntervalSince(p.at) > 30) { pending[id] = nil }
    }

    /// A link in the conversation: a saved Artifact opens in the panel beside the chat (readable from
    /// any account), anything else in the browser.
    func open(_ url: URL) -> OpenURLAction.Result {
        if PageLibrary.artifactID(in: url.absoluteString) != nil,
           let found = PageLibrary.find(url.absoluteString).first {
            withAnimation(KitMotion.appear) { page = found }
            return .handled
        }
        return .systemAction
    }

    /// The panel's page again from the library, so a republish shows up.
    private func refreshPage() {
        guard let shown = page, !shown.url.isEmpty,
              let fresh = PageLibrary.find(shown.url).first(where: { $0.folder == shown.folder }),
              fresh != shown else { return }
        page = fresh
    }

    // MARK: Replying

    func send(_ preset: String? = nil) {
        guard let agent = selected, case .job(let jobID) = agent.target, pending[agent.id]?.sending != true,
              notch.question(agent.id) == nil, notch.permission(agent.id) == nil else { return }
        let text = ShellCommand.normalize((preset ?? drafts[agent.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines))
        guard !text.isEmpty, text != "!" else { return }
        let id = agent.id
        if preset == nil { drafts[id] = "" }
        errors[id] = nil
        pending[id] = Pending(text: text, at: Date(), sending: true)
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var failure: Error?
            do { try ReplySender.send(text, toJob: jobID) } catch { failure = error }
            DispatchQueue.main.async {
                guard let self else { return }
                if let failure {
                    self.pending[id] = nil
                    self.errors[id] = failure.localizedDescription
                    if preset == nil, (self.drafts[id] ?? "").isEmpty { self.drafts[id] = text }
                } else {
                    self.pending[id]?.sending = false
                    self.refresh()
                }
            }
        }
    }

    func isSending(_ id: String) -> Bool { pending[id]?.sending == true }

    /// Commands matching a draft like "/sd:ap"; empty once there's a space after the command.
    func completions(for agent: Agent) -> [SlashCommand] {
        let draft = drafts[agent.id] ?? ""
        guard draft.hasPrefix("/"), !draft.contains(where: \.isWhitespace) else { return [] }
        return Array(SlashCommands.matches(String(draft.dropFirst()), in: SlashCommands.all(for: agent.cwd)).prefix(8))
    }

    /// The reply the session suggested, offered while the box is empty.
    func suggestion(for agent: Agent) -> String? {
        guard (drafts[agent.id] ?? "").isEmpty, let s = agent.suggestedReply, !s.isEmpty else { return nil }
        return s
    }

    func accept(_ command: SlashCommand, for agent: Agent) {
        drafts[agent.id] = "/" + command.name + (command.argumentHint == nil ? "" : " ")
        completionIndex = 0
    }

    /// Tab, arrows and Return while the reply box has focus. True when the key was used.
    func handleKey(_ keyCode: UInt16) -> Bool {
        // Escape closes the page panel, unless a command list is open in the reply box.
        if keyCode == 53, page != nil, !(composerFocused && selected.map { !completions(for: $0).isEmpty } == true) {
            withAnimation(KitMotion.appear) { page = nil }
            return true
        }
        guard composerFocused, let agent = selected, agent.canReply else { return false }
        let list = completions(for: agent)
        let pick = list.isEmpty ? nil : list[min(completionIndex, list.count - 1)]
        switch keyCode {
        case 48: // Tab
            if let pick { accept(pick, for: agent); return true }
            if let s = suggestion(for: agent) { drafts[agent.id] = s; return true }
            return false
        case 125 where !list.isEmpty: // Down
            completionIndex = (completionIndex + 1) % list.count
            return true
        case 126 where !list.isEmpty: // Up
            completionIndex = (completionIndex + list.count - 1) % list.count
            return true
        case 36, 76: // Return: finish the command name first, send on the next press
            guard let pick, "/" + pick.name != drafts[agent.id] else { return false }
            accept(pick, for: agent)
            return true
        default:
            return false
        }
    }
}

// MARK: - Views (parts and tokens from ChatKit.swift; spec in docs/chat-kit.md)

struct ChatView: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel

    var body: some View {
        HStack(spacing: 0) {
            SessionSidebar(model: model, notch: notch)
                .frame(width: KitMetric.sidebarWidth)
                .background(KitInk.sidebar)
            Rectangle().fill(KitInk.hairline).frame(width: 1)
            Group {
                if let agent = model.selected {
                    SessionDetail(model: model, notch: notch, agent: agent).id(agent.id)
                } else {
                    // No session to read, but a page opened from search still shows on the right.
                    GeometryReader { geo in
                        HStack(spacing: 0) {
                            Group {
                                if notch.agents.isEmpty {
                                    EmptyState(pose: "doze-2", title: "No Sessions",
                                               message: "Sessions show up here when you run claude in a terminal or start a background session.")
                                } else {
                                    EmptyState(pose: "wave-3", title: "Pick a Session", message: "Choose one on the left to read its conversation.")
                                }
                            }
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            if let page = model.page {
                                PagePanel(model: model, page: page, maxWidth: max(320, geo.size.width - 280))
                                    .transition(.move(edge: .trailing))
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(KitInk.window)
        }
        .ignoresSafeArea()
        .background(KitWindowChrome())
        .environment(\.openURL, OpenURLAction { model.open($0) })
        .kitNoFocusRing()
        .frame(minWidth: 720, minHeight: 460)
    }
}

private struct SessionSidebar: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel
    @FocusState private var listFocused: Bool
    @FocusState private var searchFocused: Bool

    /// Account first, then the project: folders your sessions work in, folders added before, or a new one.
    /// The session starts idle and opens here.
    private func newSessionMenu() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        if #available(macOS 14, *) { menu.addItem(.sectionHeader(title: "New Background Session On")) }
        let folders = model.projectFolders
        for target in model.sessionTargets {
            let item = NSMenuItem(title: target.title, action: nil, keyEquivalent: "")
            let projects = NSMenu()
            projects.autoenablesItems = false
            for folder in folders {
                let row = ActionMenuItem(ChatModel.folderTitle(folder, among: folders)) { [model] in
                    model.newBackgroundSession(in: folder, on: target.accountID)
                }
                row.toolTip = folder
                projects.addItem(row)
            }
            if !folders.isEmpty { projects.addItem(.separator()) }
            projects.addItem(ActionMenuItem("Add Project Folder…") { [model] in
                NSApp.activate(ignoringOtherApps: true)
                let panel = NSOpenPanel()
                panel.canChooseDirectories = true
                panel.canChooseFiles = false
                panel.prompt = "Start Here"
                panel.message = "The project folder for the new session, on \(target.title)."
                guard panel.runModal() == .OK, let folder = panel.url?.path else { return }
                model.addProjectFolder(folder)
                model.newBackgroundSession(in: folder, on: target.accountID)
            })
            item.submenu = projects
            menu.addItem(item)
        }
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }

    var body: some View {
        let shown = model.shown
        VStack(alignment: .leading, spacing: 0) {
            // The traffic lights live here.
            Color.clear.frame(height: KitMetric.titleBar)
            HStack(alignment: .firstTextBaseline) {
                Text("Sessions").kitType(.title).foregroundColor(KitInk.text)
                Spacer()
                Button { newSessionMenu() } label: {
                    Image(systemName: "plus").font(.system(size: 12, weight: .semibold))
                }
                .buttonStyle(.kit(.ghost))
                .help("New background session: pick an account (a free one gets a lane) and a folder")
                if notch.needsCount > 0 {
                    KitStatusPill(group: .needsInput, text: notch.needsCount == 1 ? "1 needs you" : "\(notch.needsCount) need you")
                }
            }
            .padding(.horizontal, 16).padding(.bottom, 10)
            // Filters appear once there's enough to filter.
            if notch.agents.count > 6 {
                HStack(spacing: 6) {
                    KitIconView(icon: .search, size: 11).foregroundColor(KitInk.textFaint)
                    TextField("Search", text: $model.search).focused($searchFocused)
                }
                .modifier(KitField(focused: searchFocused))
                .padding(.horizontal, 12).padding(.bottom, 8)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(AgentGroup.allCases, id: \.self) { group in
                        let agents = shown.filter { $0.group == group }
                        if !agents.isEmpty {
                            Text(group == .needsInput ? "Needs You" : group == .working ? "Working" : "Done")
                                .kitType(.section).foregroundColor(KitInk.textFaint)
                                .padding(.horizontal, 10).padding(.top, 12).padding(.bottom, 4)
                            ForEach(agents, id: \.id) { agent in
                                Button { model.selection = agent.id; listFocused = true } label: {
                                    SessionRowLabel(agent: agent, line: statusLine(agent),
                                                    account: model.lanesInUse ? model.account(for: agent) : nil)
                                }
                                .buttonStyle(KitSessionRowStyle(selected: model.selection == agent.id, focused: listFocused))
                                .accessibilityAddTraits(model.selection == agent.id ? .isSelected : [])
                                // The notch's actions: Hide drops it from the lists until it changes state;
                                // Remove stops a background session and deletes it (`claude rm`), after asking.
                                .contextMenu {
                                    Button("Hide") { notch.onHide?(agent) }
                                    if case .job = agent.target {
                                        Divider()
                                        Button("Remove Background Session…") { notch.onRemove?(agent) }
                                    }
                                }
                            }
                        }
                    }
                }
                .padding(.horizontal, 8).padding(.bottom, 12)
            }
            .focusable()
            .focused($listFocused)
            .onMoveCommand { direction in
                let ids = AgentGroup.allCases.flatMap { g in shown.filter { $0.group == g }.map(\.id) }
                guard !ids.isEmpty else { return }
                let index = model.selection.flatMap { ids.firstIndex(of: $0) } ?? -1
                switch direction {
                case .up: model.selection = ids[max(index - 1, 0)]
                case .down: model.selection = ids[min(index + 1, ids.count - 1)]
                default: break
                }
            }
        }
        .onChange(of: model.sidebarFocusRequest) { _ in listFocused = true }
    }

    /// What the row says under the name: what it's doing, or what it waits for.
    private func statusLine(_ agent: Agent) -> String {
        switch agent.group {
        case .working: return notch.snapshot(agent.id).activity ?? "Working"
        case .needsInput: return notch.question(agent.id) != nil ? "Asking you a question" : notch.permission(agent.id) != nil ? "Asking permission" : "Waiting for you"
        case .completed: return agent.label == "Idle" ? "Idle · \(agent.project)" : agent.label
        }
    }
}

private struct SessionRowLabel: View {
    let agent: Agent
    let line: String
    /// Shown only while lanes exist; otherwise every session is on the one main account.
    var account: ChatModel.SessionAccount?

    var body: some View {
        HStack(spacing: 10) {
            KitAvatar(name: agent.name, size: 30, status: agent.group)
            VStack(alignment: .leading, spacing: 2) {
                Text(agent.name).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                Text(line).kitType(.meta).foregroundColor(agent.group == .needsInput ? KitInk.accentText : KitInk.textMuted).lineLimit(1)
                if let account {
                    HStack(spacing: 4) {
                        Image(systemName: account.lane == nil ? "person.crop.circle" : "arrow.triangle.branch")
                            .font(.system(size: 9, weight: .semibold))
                        Text(account.email).lineLimit(1).truncationMode(.middle)
                    }
                    .kitType(.meta).foregroundColor(KitInk.textFaint)
                }
            }
            Spacer(minLength: 4)
            Text(ChatFormat.age(agent.since)).kitType(.meta).foregroundColor(KitInk.textFaint)
        }
        .help("\(agent.label) · \(agent.cwd)")
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(agent.name), \(agent.label), \(agent.project)")
    }
}

private struct SessionDetail: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel
    let agent: Agent
    @State private var dropping = false

    var body: some View {
        VStack(spacing: 0) {
            header
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            GeometryReader { geo in
            HStack(spacing: 0) {
                VStack(spacing: 0) {
                    if model.conversation.items.isEmpty && model.pending[agent.id] == nil {
                        EmptyState(pose: agent.group == .working ? "look-2" : "sit-1", title: "No Messages Yet",
                                   message: agent.group == .working ? "The session is starting." : "Nothing in this session's transcript yet.")
                    } else {
                        ConversationView(conversation: model.conversation,
                                         pending: model.pending[agent.id],
                                         live: agent.group == .working,
                                         expansion: model.expansion,
                                         pageOpen: model.page != nil)
                            .equatable()
                            // A fresh view per session, so switching back opens at the newest message.
                            .id(agent.id)
                    }
                    bottom
                }
                // Files and images dropped anywhere on the conversation go into the reply as paths, as in
                // Claude Code's terminal; Claude reads them from there.
                .onDrop(of: ChatModel.dropTypes, isTargeted: $dropping) { providers in
                    agent.canReply && model.receiveDrop(providers, for: agent.id)
                }
                if let page = model.page {
                    PagePanel(model: model, page: page, maxWidth: max(320, geo.size.width - 380))
                        .transition(.move(edge: .trailing))
                } else if model.showInspector {
                    Rectangle().fill(KitInk.hairline).frame(width: 1)
                    SessionInspector(agent: agent, conversation: model.conversation)
                        .frame(width: 250)
                        .transition(.move(edge: .trailing))
                }
            }
            }
        }
    }

    private var header: some View {
        HStack(spacing: 12) {
            // The status is the avatar's dot, as in the sidebar; the word is in its tooltip.
            KitAvatar(name: agent.name, size: 36, status: agent.group)
                .help(agent.label)
                .accessibilityLabel("\(agent.name), \(agent.label)")
            VStack(alignment: .leading, spacing: 4) {
                Text(agent.name).kitType(.title).foregroundColor(KitInk.text).lineLimit(1)
                HStack(spacing: 8) {
                    Text([agent.project, agent.host].filter { !$0.isEmpty }.joined(separator: " · "))
                        .kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                        .layoutPriority(1)
                    if let account = model.account(for: agent) {
                        AccountMenu(model: model, agent: agent, account: account)
                    }
                }
            }
            Spacer(minLength: 8)
            // Icon only, so the line under the title has the room.
            Button { AgentActions.open(agent.target, cwd: agent.cwd) } label: {
                KitIconView(icon: ChatFormat.openIcon(agent), size: 13)
            }
            .buttonStyle(.kit(.ghost))
            .help("\(ChatFormat.openTitle(agent)): \(ChatFormat.openHelp(agent))")
            .accessibilityLabel(ChatFormat.openTitle(agent))
            Button { withAnimation(KitMotion.appear) { model.showInspector.toggle() } } label: {
                Image(systemName: "sidebar.right").font(.system(size: 13, weight: .medium))
            }
            .buttonStyle(.kit(model.showInspector ? .secondary : .ghost))
            .help(model.showInspector ? "Hide the session details" : "Show the session details: branch, model, tokens, tasks, files changed")
        }
        .padding(.horizontal, 16)
        .padding(.top, 14).padding(.bottom, 12)
        .frame(minHeight: 64)
    }

    @ViewBuilder private var bottom: some View {
        VStack(spacing: 8) {
        if let error = model.accountErrors[agent.id] {
            AccountBanner(text: error, isError: true) { model.accountErrors[agent.id] = nil }
        } else if let note = model.accountNote(for: agent) {
            AccountBanner(text: note.text, isError: false) { model.dismissAccountNote(for: agent) }
        }
        Group {
            if let request = notch.permission(agent.id) {
                ChatPermissionCard(notch: notch, agent: agent, request: request)
            } else if let question = notch.question(agent.id) {
                ChatQuestionCard(notch: notch, agent: agent, question: question)
            } else {
                switch agent.target {
                case .job:
                    Composer(model: model, agent: agent, dropping: dropping)
                case .process:
                    Notice(text: agent.host == "claude -p"
                           ? "This session runs headless (claude -p) and can't take replies."
                           : "Replies to this session go to its terminal. The terminal button at the top right brings it forward.")
                case .resume:
                    Notice(text: "This session has ended. Resume it in a terminal to keep going.")
                }
            }
        }
        }
        .padding(.horizontal, 16).padding(.bottom, 16).padding(.top, 4)
        .frame(maxWidth: .infinity)
    }
}

/// The messages. Equatable so the notch's own updates (hover, peeks) don't re-lay out a long history.
private struct ConversationView: View, Equatable {
    let conversation: Conversation
    let pending: ChatModel.Pending?
    /// The session is working: its newest tool step stays open and a live row shows what it's doing.
    let live: Bool
    let expansion: ChatExpansion
    var pageOpen = false
    /// Following the newest message: true until you scroll up, and again once you're back at the bottom.
    /// Only your own scrolling changes it (`UserScrollWatcher`), so content growing below can't be
    /// mistaken for a scroll and pull you back down while you read.
    @State private var atBottom = true
    /// While our own animated jump runs, don't also snap to the end.
    @State private var jumpingUntil = Date.distantPast

    static func == (a: Self, b: Self) -> Bool {
        a.conversation == b.conversation && a.pending == b.pending && a.live == b.live
            && a.expansion === b.expansion && a.pageOpen == b.pageOpen
    }

    private struct Row: Identifiable {
        let id: String
        let item: ChatItem?
        let day: Date?
    }

    /// Items with a day separator wherever the calendar day changes.
    private var rows: [Row] {
        var out: [Row] = []
        var lastDay: Date?
        let calendar = Calendar.current
        for item in conversation.items {
            if let date = item.date {
                let day = calendar.startOfDay(for: date)
                if day != lastDay { out.append(Row(id: "day-\(day.timeIntervalSince1970)", item: nil, day: day)); lastDay = day }
            }
            out.append(Row(id: item.id, item: item, day: nil))
        }
        return out
    }

    var body: some View {
        let rows = self.rows
        let lastToolID = conversation.items.last(where: { if case .tools = $0.kind { return true } else { return false } })?.id
        let lastID = conversation.items.last?.id
        // "Working" can outlast the turn: a background command (status "shell") keeps a session busy
        // after its reply. Then the row says so instead of "Thinking…" with a clock that never stops.
        let turnOver: Bool = { if case .turnEnd = conversation.items.last?.kind { return true } else { return false } }()
        VStack(spacing: 0) {
        if !conversation.tasks.isEmpty {
            TaskPanel(tasks: conversation.tasks, expansion: expansion)
                        .padding(.horizontal, 16).padding(.top, 12)
                .frame(maxWidth: .infinity)
        }
        ScrollViewReader { proxy in
            GeometryReader { outer in
                ZStack(alignment: .bottomTrailing) {
                    ScrollView {
                        LazyVStack(alignment: .leading, spacing: 14) {
                            ForEach(rows) { row in
                                Group {
                                    if let day = row.day {
                                        DaySeparator(date: day)
                                    } else if let item = row.item {
                                        itemView(item, liveTools: live && !turnOver && item.id == lastToolID && item.id == lastID)
                                    }
                                }
                                .id(row.id)
                            }
                            if let pending, !pending.landed(in: conversation) {
                                UserBubble(text: pending.text, images: 0, date: nil, note: pending.sending ? "Sending…" : "Sent")
                                    .transition(.opacity.combined(with: .move(edge: .bottom)))
                            }
                            if live && turnOver {
                                HStack(spacing: 8) {
                                    ProgressView().controlSize(.small)
                                    Text("A background command is still running").kitType(.meta).foregroundColor(KitInk.textMuted)
                                }
                            } else if live {
                                HStack(spacing: 8) {
                                    ProgressView().controlSize(.small)
                                    Text(conversation.activity ?? "Thinking…").kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                                    // Like Claude Code: time since your prompt, and tokens written since.
                                    if let start = conversation.turnStart {
                                        TimelineView(.periodic(from: .now, by: 1)) { context in
                                            Text("(" + ChatFormat.workStats(since: start, now: context.date, tokens: conversation.turnTokens) + ")")
                                                .kitType(.meta).foregroundColor(KitInk.textFaint).monospacedDigit()
                                        }
                                        .fixedSize()
                                    }
                                }
                            }
                            Color.clear.frame(height: 1).id("end")
                                .background(GeometryReader { g in
                                    Color.clear.preference(key: ScrollEdgesKey.self,
                                                           value: ScrollEdges(end: g.frame(in: .named("chatScroll")).minY))
                                })
                        }
                        .background(UserScrollWatcher { following in
                            if following != atBottom { withAnimation(KitMotion.hover) { atBottom = following } }
                        })
                                        .padding(.horizontal, 16).padding(.vertical, 20)
                        .frame(maxWidth: .infinity)
                    }
                    .coordinateSpace(name: "chatScroll")
                    .onPreferenceChange(ScrollEdgesKey.self) { edges in
                        // Off screen (a lazy row not drawn), the end counts as far below.
                        let near = (edges.end ?? .infinity) < outer.size.height + 4
                        // A message or step grew below while following: stay on the newest.
                        if atBottom && !near && Date() >= jumpingUntil {
                            DispatchQueue.main.async { if atBottom { proxy.scrollTo("end", anchor: .bottom) } }
                        }
                    }
                    if !atBottom {
                        Button { jump(proxy) } label: {
                            HStack(spacing: 6) {
                                Image(systemName: "arrow.down").font(.system(size: 11, weight: .semibold))
                                Text("Latest")
                            }
                        }
                        .buttonStyle(.kit(.secondary))
                        .shadow(color: KitInk.shadow, radius: 8, y: 3)
                        .padding(20)
                        .help("Jump to the newest message")
                        .transition(.opacity)
                    }
                }
            }
            .onAppear {
                // Twice: rows below the fold are measured lazily, so the first jump can fall short.
                DispatchQueue.main.async { proxy.scrollTo("end", anchor: .bottom) }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { proxy.scrollTo("end", anchor: .bottom) }
            }
            // The page panel opening or closing rewraps every message; stay at the newest.
            .onChange(of: pageOpen) { _ in
                guard atBottom else { return }
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { proxy.scrollTo("end", anchor: .bottom) }
            }
            // Follow new messages only while you're at the bottom; never yank you from what you read.
            .onChange(of: rows.last?.id) { _ in
                if atBottom { jump(proxy) }
            }
            .onChange(of: pending) { _ in jump(proxy) }
        }
        }
    }

    private func jump(_ proxy: ScrollViewProxy) {
        atBottom = true
        jumpingUntil = Date().addingTimeInterval(0.5)
        withAnimation(KitMotion.appear) { proxy.scrollTo("end", anchor: .bottom) }
    }

    @ViewBuilder private func itemView(_ item: ChatItem, liveTools: Bool) -> some View {
        switch item.kind {
        case .user(let text, let images):
            UserBubble(text: text, images: images, date: item.date, note: nil)
        case .assistant(let text):
            AssistantMessage(text: text, date: item.date)
        case .thinking(let seconds, let text):
            ThinkingRow(id: item.id, seconds: seconds, text: text, expansion: expansion)
        case .tools(let calls):
            ToolStepView(id: item.id, calls: calls, live: liveTools, expansion: expansion)
        case .turnEnd(let seconds, let model):
            TurnFooter(seconds: seconds, model: model)
        case .notice(let text, let symbol):
            NoticeRow(text: text, symbol: symbol)
        case .commandOutput(let text):
            CommandOutputView(text: text)
        }
    }
}

/// The conversation's end marker, in the scroll view's space; nil when not drawn.
private struct ScrollEdges: Equatable {
    var end: CGFloat?
}

private struct ScrollEdgesKey: PreferenceKey {
    static var defaultValue = ScrollEdges()
    static func reduce(value: inout ScrollEdges, nextValue: () -> ScrollEdges) {
        if let end = nextValue().end { value.end = end }
    }
}

/// Tells the conversation when you scroll it (wheel, trackpad, scroller), which from inside SwiftUI
/// looks the same as our own scrolls and as content growing below. Reports whether you're following
/// the end: scrolling up stops at once (before the scroll lands, so nothing snaps you back), and
/// reaching the bottom again resumes.
private struct UserScrollWatcher: NSViewRepresentable {
    let onScroll: (_ following: Bool) -> Void

    func makeNSView(context: Context) -> WatchView { WatchView() }

    func updateNSView(_ view: WatchView, context: Context) { view.onScroll = onScroll }

    final class WatchView: NSView {
        var onScroll: ((Bool) -> Void)?
        private var monitor: Any?
        private var observer: NSObjectProtocol?
        /// How close to the end counts as at the bottom.
        private let slack: CGFloat = 16

        override func hitTest(_ point: NSPoint) -> NSView? { nil }

        override func viewDidMoveToWindow() {
            super.viewDidMoveToWindow()
            stop()
            guard window != nil else { return }
            monitor = NSEvent.addLocalMonitorForEvents(matching: .scrollWheel) { [weak self] event in
                self?.wheel(event)
                return event
            }
            // Dragging the scroller.
            observer = NotificationCenter.default.addObserver(forName: NSScrollView.didLiveScrollNotification, object: nil,
                                                              queue: .main) { [weak self] note in
                guard let self, let scroll = self.enclosingScrollView, note.object as? NSScrollView === scroll else { return }
                self.onScroll?(self.distanceFromEnd(scroll) < self.slack)
            }
        }

        private func wheel(_ event: NSEvent) {
            guard event.window === window, let scroll = enclosingScrollView,
                  scroll.bounds.contains(scroll.convert(event.locationInWindow, from: nil)),
                  let document = scroll.documentView, document.frame.height > scroll.contentView.bounds.height else { return }
            // Positive is toward older messages.
            if event.scrollingDeltaY > 0 { onScroll?(false) }
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                self.onScroll?(self.distanceFromEnd(scroll) < self.slack)
            }
        }

        private func distanceFromEnd(_ scroll: NSScrollView) -> CGFloat {
            guard let document = scroll.documentView else { return 0 }
            let visible = scroll.contentView.bounds
            return document.isFlipped ? document.frame.height - visible.maxY : visible.minY
        }

        private func stop() {
            if let monitor { NSEvent.removeMonitor(monitor) }
            if let observer { NotificationCenter.default.removeObserver(observer) }
            monitor = nil
            observer = nil
        }

        deinit { stop() }
    }
}

/// Claude's message: markdown on a card, with Copy and the time on hover, sitting just above the card's
/// top-right corner so they never cover the text.
private struct AssistantMessage: View {
    let text: String
    let date: Date?
    @State private var overCard = false
    @State private var overBar = false
    @State private var shown = false

    var body: some View {
        // Full width, no avatar: the card and its place on the left already say who wrote it.
        HStack(alignment: .top, spacing: 0) {
            MarkdownView(text: text)
                .padding(.horizontal, 14).padding(.vertical, 12)
                .frame(maxWidth: .infinity, alignment: .leading)
                .kitCard()
                .overlay(alignment: .topTrailing) {
                    if shown {
                        HStack(spacing: 2) {
                            if let date { Text(ChatFormat.time(date)).kitType(.meta).foregroundColor(KitInk.textFaint).padding(.horizontal, 4) }
                            CopyButton(text: text)
                        }
                        .fixedSize()
                        .padding(2)
                        .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm).fill(KitInk.card))
                        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusSm).stroke(KitInk.hairline, lineWidth: 1))
                        // Its bottom edge on the card's top border, at the right.
                        .alignmentGuide(.top) { $0[.bottom] + 2 }
                        .padding(.trailing, 10)
                        .onHover { overBar = $0; update() }
                        .transition(.opacity)
                    }
                }
        }
        .onHover { overCard = $0; update() }
    }

    /// Moving from the card up to the bar leaves one before entering the other: hide only once the
    /// pointer has been off both for a moment.
    private func update() {
        if overCard || overBar { shown = true; return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.2) {
            if !overCard && !overBar { shown = false }
        }
    }
}

private struct UserBubble: View {
    let text: String
    let images: Int
    let date: Date?
    let note: String?
    @State private var hovering = false

    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Spacer(minLength: 120)
            if hovering, note == nil {
                VStack(alignment: .trailing, spacing: 2) {
                    CopyButton(text: text)
                    if let date { Text(ChatFormat.time(date)).kitType(.meta).foregroundColor(KitInk.textFaint) }
                }
            }
            VStack(alignment: .trailing, spacing: 4) {
                CappedWidth(max: 720) {
                VStack(alignment: .leading, spacing: 6) {
                    if images > 0 {
                        Label(images == 1 ? "1 image" : "\(images) images", systemImage: "photo")
                            .kitType(.label).foregroundColor(KitInk.accentText)
                    }
                    if text.hasPrefix("! ") {
                        // A shell command: "!" in the shell color, the command in mono.
                        (Text("! ").foregroundColor(KitInk.shell) + Text(text.dropFirst(2)).foregroundColor(KitInk.text))
                            .kitType(.mono)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if Self.isPlain(text) {
                        // Hugs its text, so a short message is a short bubble at full window width.
                        Text(text).kitType(.body).foregroundColor(KitInk.text)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                    } else if !text.isEmpty {
                        // Your own messages can hold tables and code too (pastes), so they get markdown.
                        MarkdownView(text: text)
                    }
                }
                }
                .padding(.horizontal, 14).padding(.vertical, 10)
                .background(RoundedRectangle(cornerRadius: 16, style: .continuous)
                    .fill(text.hasPrefix("! ") ? KitInk.shellWash : KitInk.accentWash))
                if let note {
                    Text(note).kitType(.meta).foregroundColor(KitInk.textFaint)
                }
            }
            // Gets its width before the spacer, so a short reply stays on one line.
            .layoutPriority(1)
        }
        .onHover { hovering = $0 }
    }

    /// One paragraph with nothing markdown would draw differently.
    static func isPlain(_ text: String) -> Bool {
        !text.isEmpty && !text.contains(where: { "\n`*_[|#>".contains($0) })
    }
}

/// Offers its content at most `max` points and takes only what the content uses, so a short text
/// stays short (`.frame(maxWidth:)` would always grow to the full `max`).
private struct CappedWidth: Layout {
    let max: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let width = Swift.min(proposal.width ?? max, max)
        return subviews.first?.sizeThatFits(ProposedViewSize(width: width, height: proposal.height)) ?? .zero
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: bounds.height))
    }
}

private struct Composer: View {
    @ObservedObject var model: ChatModel
    let agent: Agent
    /// A file is being dragged over the conversation (the drop target is `SessionDetail`).
    let dropping: Bool
    @FocusState private var focused: Bool
    @State private var focusedBeforeDrag = false

    var body: some View {
        let draft = Binding(get: { model.drafts[agent.id] ?? "" },
                            set: { model.drafts[agent.id] = $0; model.completionIndex = 0 })
        let empty = draft.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let commands = model.completions(for: agent)
        let sending = model.isSending(agent.id)
        let shell = ShellCommand.isCommand(draft.wrappedValue)
        // What will actually run, when putting a copied command back together changed it.
        let fixed: String? = {
            guard shell else { return nil }
            let typed = draft.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines)
            let normal = ShellCommand.normalize(typed)
            return normal != typed && normal != "!" ? String(normal.dropFirst()) : nil
        }()
        // A session waiting on you gets its suggested reply as a one-click chip; otherwise Tab takes it.
        let showsChip = commands.isEmpty && empty && agent.group == .needsInput && !(agent.suggestedReply ?? "").isEmpty
        VStack(alignment: .leading, spacing: 8) {
            if showsChip, let suggested = agent.suggestedReply {
                Button { model.send(suggested) } label: {
                    HStack(spacing: 6) {
                        KitIconView(icon: .claude, size: 10)
                        Text(suggested).lineLimit(1)
                    }
                }
                .buttonStyle(.kit(.secondary))
                .disabled(sending)
                .help("Send the suggested reply")
            }
            VStack(alignment: .leading, spacing: 0) {
                if !commands.isEmpty {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(commands.enumerated()), id: \.offset) { index, command in
                            let picked = index == min(model.completionIndex, commands.count - 1)
                            Button { model.accept(command, for: agent) } label: {
                                HStack(spacing: 10) {
                                    Text("/" + command.name).kitType(.mono).foregroundColor(picked ? KitInk.accentText : KitInk.text)
                                    Text(command.description).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                                    Spacer(minLength: 0)
                                }
                                .padding(.horizontal, 10).frame(height: 26)
                                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                                    .fill(picked ? KitInk.selection : .clear))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .pointerCursor()
                        }
                    }
                    .padding(6)
                    Rectangle().fill(KitInk.hairline).frame(height: 1)
                }
                if shell {
                    // Shell mode, so it's clear Return runs a command rather than sending a message.
                    HStack(spacing: 6) {
                        KitIconView(icon: .terminal, size: 11)
                        Text("Shell command").kitType(.label)
                        Text("runs in \((agent.cwd as NSString).lastPathComponent) · Claude sees the output")
                            .kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .foregroundColor(KitInk.shell)
                    .padding(.horizontal, 14).padding(.top, 9)
                    .accessibilityElement(children: .combine)
                    if let fixed {
                        // A command copied from a terminal comes with its line wrapping; this is what runs.
                        VStack(alignment: .leading, spacing: 3) {
                            Text("Will run, with the copied line breaks joined:").kitType(.meta).foregroundColor(KitInk.textMuted)
                            Text(fixed).kitType(.mono).foregroundColor(KitInk.text)
                                .textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        .padding(.horizontal, 10).padding(.vertical, 7)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.raised))
                        .padding(.horizontal, 12).padding(.top, 6)
                        .accessibilityElement(children: .combine)
                    }
                }
                HStack(alignment: .bottom, spacing: 10) {
                    TextField(showsChip || model.suggestion(for: agent) == nil ? "Reply to \(agent.name), or / for commands…"
                              : "\(model.suggestion(for: agent) ?? "")   ⇥ Tab",
                              text: draft, axis: .vertical)
                        .textFieldStyle(.plain)
                        .kitType(shell ? .mono : .body).foregroundColor(KitInk.text)
                        .lineLimit(1...8)
                        .focused($focused)
                        .onSubmit { model.send() }
                        .padding(.vertical, 6)
                        .accessibilityLabel("Reply to \(agent.name)")
                    Button { model.send() } label: {
                        ZStack {
                            Circle().fill(empty ? KitInk.raised : shell ? KitInk.shell : KitInk.accent)
                            if sending {
                                ProgressView().controlSize(.small)
                            } else {
                                KitIconView(icon: .send, size: 13)
                                    .foregroundColor(empty ? KitInk.textFaint : shell ? KitInk.onShell : KitInk.onAccent)
                            }
                        }
                        .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.plain)
                    .disabled(empty || sending)
                    .help(shell ? "Run the command in the session (Return)" : "Send the reply (Return)")
                    .accessibilityLabel(shell ? "Run" : "Send")
                    .pointerCursor()
                }
                .padding(.leading, 14).padding(.trailing, 8).padding(.vertical, 8)
            }
            .background(ZStack {
                RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous).fill(KitInk.card)
                RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
                    .fill(shell ? KitInk.shellWash : .clear)
            })
            .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
                .strokeBorder(dropping ? KitInk.accent : shell ? KitInk.shell : focused ? KitInk.accent.opacity(0.6) : KitInk.hairline,
                              lineWidth: focused || dropping || shell ? 1.5 : 1))
            .shadow(color: KitInk.shadow, radius: 14, y: 6)
            .animation(KitMotion.hover, value: focused)
            .animation(KitMotion.hover, value: dropping)
            .animation(KitMotion.hover, value: shell)
            if let error = model.errors[agent.id] {
                HStack(spacing: 6) {
                    KitIconView(icon: .info, size: 11)
                    Text(error).kitType(.meta)
                }
                .foregroundColor(.red)
                .padding(.leading, 6)
            } else {
                Text(shell ? "Return runs it like ! in the terminal · delete the ! to send a message instead"
                     : "Return sends · ⌥Return new line · typed into the session with claude attach")
                    .kitType(.meta).foregroundColor(KitInk.textFaint)
                    .frame(maxWidth: .infinity)
            }
        }
        .onChange(of: focused) { model.composerFocused = $0 }
        .onChange(of: model.focusRequest) { _ in focused = true }
        // While the reply box has focus, its text editor registers for file drags and takes the drop
        // before SwiftUI's onDrop sees it (inserting nothing). So it lets go while a file is over the
        // conversation, and takes focus back after.
        .onChange(of: dropping) { over in
            if over {
                focusedBeforeDrag = focused
                focused = false
            } else if focusedBeforeDrag {
                focused = true
            }
        }
        .onAppear { DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { focused = true } }
        .onDisappear { model.composerFocused = false }
    }
}

private struct ChatQuestionCard: View {
    @ObservedObject var notch: NotchModel
    let agent: Agent
    let question: PendingQuestion
    @State private var typed = ""
    @FocusState private var typing: Bool

    var body: some View {
        let index = min(notch.choices.count, max(question.items.count - 1, 0))
        let item = question.items.isEmpty ? nil : question.items[index]
        let canAnswer = notch.canAnswer(agent, question)
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                KitClaudeMark(size: 20)
                Text(question.items.count > 1 ? "Claude is asking · \(index + 1) of \(question.items.count)" : "Claude is asking")
                    .kitType(.label).foregroundColor(KitInk.accentText)
                Spacer()
                if notch.choices.count > 0 {
                    Button { notch.undoChoice() } label: {
                        HStack(spacing: 4) { KitIconView(icon: .back, size: 11); Text("Back") }
                    }
                    .buttonStyle(.kit(.ghost))
                    .help("Change the previous answer")
                }
            }
            if let item {
                Text(item.question).kitType(.heading).foregroundColor(KitInk.text).textSelection(.enabled)
                if canAnswer {
                    ForEach(Array(item.options.enumerated()), id: \.offset) { i, option in
                        Button { notch.choose(.option(i + 1), for: agent.id) } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(option).kitType(.heading).foregroundColor(KitInk.text)
                                if i < item.details.count, !item.details[i].isEmpty {
                                    Text(item.details[i]).kitType(.meta).foregroundColor(KitInk.textMuted)
                                }
                            }
                        }
                        .buttonStyle(KitOptionStyle(number: i + 1))
                        .disabled(notch.sending)
                    }
                    TextField("Something else…", text: $typed)
                        .focused($typing)
                        .modifier(KitField(focused: typing))
                        .onSubmit {
                            let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
                            guard !text.isEmpty else { return }
                            typed = ""
                            notch.choose(.text(text), for: agent.id)
                        }
                } else {
                    HStack {
                        Text("This one takes more than one choice; answer it in the terminal.")
                            .kitType(.meta).foregroundColor(KitInk.textMuted)
                        Spacer()
                        Button("Answer in Terminal") { notch.answerInTerminal(agent.id) }.buttonStyle(.kit(.secondary))
                    }
                }
            }
            if let error = notch.sendError {
                Text(error).kitType(.meta).foregroundColor(.red)
            } else if notch.sending {
                Text("Sending the answer…").kitType(.meta).foregroundColor(KitInk.textMuted)
            }
        }
        .padding(16)
        .kitCard(radius: KitMetric.radiusComposer)
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
            .strokeBorder(KitInk.accent.opacity(0.45), lineWidth: 1))
        .shadow(color: KitInk.shadow, radius: 14, y: 6)
    }
}

private struct ChatPermissionCard: View {
    @ObservedObject var notch: NotchModel
    let agent: Agent
    let request: HookRequest

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                KitIconView(icon: .permission, size: 13).foregroundColor(KitInk.accentText)
                Text("Allow \(request.toolName)?").kitType(.heading).foregroundColor(KitInk.text)
            }
            ScrollView {
                Text(request.summary)
                    .kitType(.mono).foregroundColor(KitInk.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
            }
            .frame(maxHeight: 160)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.raised))
            HStack(spacing: 8) {
                Button("Deny") { notch.decide(.deny(nil), for: agent.id) }
                    .buttonStyle(.kit(.ghost))
                    .help("Refuse this tool call; Claude is told it was denied")
                Button("Answer in Terminal") { notch.decide(.terminal, for: agent.id) }
                    .buttonStyle(.kit(.ghost))
                    .help("Hand the prompt back to the session's terminal")
                Spacer()
                if request.canAlwaysAllow {
                    Button("Always Allow") { notch.decide(.allowAlways, for: agent.id) }
                        .buttonStyle(.kit(.secondary))
                        .help("Allow this and add a rule so it isn't asked again")
                }
                Button("Allow") { notch.decide(.allow, for: agent.id) }
                    .buttonStyle(.kit(.primary))
                    .help("Allow this one tool call")
            }
        }
        .padding(16)
        .kitCard(radius: KitMetric.radiusComposer)
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
            .strokeBorder(KitInk.accent.opacity(0.45), lineWidth: 1))
        .shadow(color: KitInk.shadow, radius: 14, y: 6)
    }
}

private struct Notice: View {
    let text: String

    var body: some View {
        HStack(spacing: 10) {
            KitIconView(icon: .info, size: 13).foregroundColor(KitInk.textMuted)
            Text(text).kitType(.body).foregroundColor(KitInk.textMuted)
            Spacer()
        }
        .padding(.horizontal, 16).padding(.vertical, 12)
        .kitCard(radius: KitMetric.radiusComposer, fill: KitInk.card.opacity(0.6))
    }
}

private struct EmptyState: View {
    let pose: String
    let title: String
    let message: String

    var body: some View {
        VStack(spacing: 10) {
            KitPet(pose: pose, size: 120)
            Text(title).kitType(.title).foregroundColor(KitInk.text)
            Text(message).kitType(.body).foregroundColor(KitInk.textMuted)
                .multilineTextAlignment(.center).frame(maxWidth: 320)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(20)
    }
}

enum ChatFormat {
    /// "47s · ↓ 2.3k tokens", "3m 5s · ↓ 12k tokens".
    static func workStats(since start: Date, now: Date, tokens: Int) -> String {
        let s = max(0, Int(now.timeIntervalSince(start)))
        let time = s < 60 ? "\(s)s" : s < 3600 ? "\(s / 60)m \(s % 60)s" : "\(s / 3600)h \(s % 3600 / 60)m"
        guard tokens > 0 else { return time }
        let count = tokens < 1000 ? "\(tokens)"
            : tokens < 10_000 ? String(format: "%.1fk", Double(tokens) / 1000)
            : "\(tokens / 1000)k"
        return "\(time) · ↓ \(count) tokens"
    }

    static func openTitle(_ agent: Agent) -> String {
        switch agent.target {
        case .job: return "Open in Terminal"
        case .process: return "Open in \(agent.host.isEmpty || agent.host == "claude -p" ? "Terminal" : agent.host)"
        case .resume: return "Resume in Terminal"
        }
    }

    static func openIcon(_ agent: Agent) -> KitIcon {
        if case .resume = agent.target { return .resume }
        return .terminal
    }

    static func openHelp(_ agent: Agent) -> String {
        switch agent.target {
        case .job: return "Attach to this background session in a terminal"
        case .process: return "Bring the session's terminal window forward"
        case .resume: return "Start this session again in a terminal with claude --resume"
        }
    }

    /// "now", "5m", "2h", "3d".
    static func age(_ date: Date) -> String {
        let s = max(0, Int(Date().timeIntervalSince(date)))
        if date == .distantPast { return "" }
        if s < 60 { return "now" }
        if s < 3600 { return "\(s / 60)m" }
        if s < 86_400 { return "\(s / 3600)h" }
        return "\(s / 86_400)d"
    }

    /// "12s", "4m 05s", "1h 12m".
    static func duration(_ seconds: Double) -> String {
        let s = Int(seconds.rounded())
        if s < 60 { return "\(max(s, 1))s" }
        if s < 3600 { return String(format: "%dm %02ds", s / 60, s % 60) }
        return "\(s / 3600)h \(s % 3600 / 60)m"
    }

    /// "claude-opus-5-5" -> "Opus 5.5".
    static func modelName(_ id: String) -> String {
        let parts = id.replacingOccurrences(of: "claude-", with: "").split(separator: "-")
        guard let family = parts.first else { return id }
        let version = parts.dropFirst().prefix { $0.allSatisfy(\.isNumber) && $0.count <= 2 }.joined(separator: ".")
        return family.prefix(1).uppercased() + family.dropFirst() + (version.isEmpty ? "" : " " + version)
    }

    /// "12.4k", "1.2M".
    static func tokens(_ n: Int) -> String {
        if n >= 1_000_000 { return String(format: "%.1fM", Double(n) / 1_000_000) }
        if n >= 1000 { return String(format: "%.1fk", Double(n) / 1000) }
        return "\(n)"
    }

    private static let timeFormat: DateFormatter = { let f = DateFormatter(); f.timeStyle = .short; f.dateStyle = .none; return f }()
    private static let dayFormat: DateFormatter = { let f = DateFormatter(); f.setLocalizedDateFormatFromTemplate("EEEE d MMMM"); return f }()

    static func time(_ date: Date) -> String { timeFormat.string(from: date) }

    static func day(_ date: Date) -> String {
        if Calendar.current.isDateInToday(date) { return "Today" }
        if Calendar.current.isDateInYesterday(date) { return "Yesterday" }
        return dayFormat.string(from: date)
    }
}

/// A saved Artifact beside the conversation, opened from a link in it. Drag its left edge to resize;
/// Escape or × closes it.
private struct PagePanel: View {
    @ObservedObject var model: ChatModel
    let page: PageLibrary.Entry
    let maxWidth: CGFloat
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
                HStack(spacing: 8) {
                    Image(systemName: "doc.richtext").font(.system(size: 12, weight: .medium)).foregroundColor(KitInk.accentText)
                    VStack(alignment: .leading, spacing: 1) {
                        Text(page.title).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                        Text("Saved copy · \(PageDates.short(page.updated))").kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                    }
                    Spacer(minLength: 4)
                    Button { _ = model.onOpenArtifact?(page.folder) } label: {
                        Image(systemName: "macwindow").font(.system(size: 12, weight: .medium))
                    }
                    .buttonStyle(.kit(.ghost)).help("Open in the Artifacts window")
                    if let link = URL(string: page.url), !page.url.isEmpty {
                        Button { NSWorkspace.shared.open(link) } label: {
                            Image(systemName: "safari").font(.system(size: 12, weight: .medium))
                        }
                        .buttonStyle(.kit(.ghost)).help("Open on claude.ai")
                    }
                    Button { withAnimation(KitMotion.appear) { model.page = nil } } label: {
                        Image(systemName: "xmark").font(.system(size: 11, weight: .semibold))
                    }
                    .buttonStyle(.kit(.ghost)).help("Close (Esc)")
                }
                .padding(.leading, 14).padding(.trailing, 8).frame(height: 48)
                Rectangle().fill(KitInk.hairline).frame(height: 1)
                PagePreview(page: page)
            }
        }
        .frame(width: shown)
    }
}

/// The session's account in its header, with a menu to move it to another account or start a new
/// terminal session on one. A lane moves as a whole; so does the main login. The menu is AppKit's: on
/// macOS 13 a SwiftUI borderless Menu drops a custom label.
private struct AccountMenu: View {
    @ObservedObject var model: ChatModel
    let agent: Agent
    let account: ChatModel.SessionAccount

    var body: some View {
        Button { popUp() } label: {
            HStack(spacing: 4) {
                Image(systemName: account.lane == nil ? "person.crop.circle" : "arrow.triangle.branch")
                    .font(.system(size: 10, weight: .semibold))
                Text(account.email).lineLimit(1).truncationMode(.middle)
                if let lane = account.lane { Text("· \(lane.name) lane").foregroundColor(KitInk.textFaint) }
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .pointerCursor()
        .help(account.lane.map { "This session runs in the \($0.name) lane, on \($0.email). Click to switch it." }
              ?? "This session runs on the main login. Click to switch it.")
    }

    private func popUp() {
        let free = model.freeAccounts
        let menu = NSMenu()
        menu.autoenablesItems = false
        func note(_ title: String) {
            let item = NSMenuItem(title: title, action: nil, keyEquivalent: "")
            item.isEnabled = false
            menu.addItem(item)
        }
        func header(_ title: String) {
            if #available(macOS 14, *) { menu.addItem(.sectionHeader(title: title)) } else { note(title) }
        }
        header(account.lane.map { "Switch the \($0.name) Lane To" } ?? "Switch the Main Login To")
        if free.isEmpty { note("No Free Saved Account") }
        for other in free {
            menu.addItem(ActionMenuItem(other.email) { [model, agent] in model.switchAccount(of: agent, to: other.id) })
        }
        note(account.lane == nil ? "Moves every session on the main login, background ones too."
                                 : "Moves every session in this lane.")
        menu.addItem(.separator())
        if case .job = agent.target, agent.transcriptPath != nil {
            header("Move Only This Session To")
            let here = account.lane?.accountID
            let targets = model.sessionTargets.filter { $0.accountID != here }
            if targets.isEmpty { note("No Other Account") }
            for target in targets {
                menu.addItem(ActionMenuItem(target.title) { [model, agent] in
                    model.moveSession(agent, to: target.accountID, title: target.title)
                })
            }
            note("Stops it and resumes the same conversation there.")
            menu.addItem(.separator())
        }
        if case .process = agent.target {
            header("New Terminal Session")
            if let lane = account.lane {
                menu.addItem(ActionMenuItem("In the \(lane.name) Lane") { [model, agent] in model.newTerminalSession(for: agent, on: lane.accountID) })
            }
            for other in free {
                menu.addItem(ActionMenuItem("On \(other.email)") { [model, agent] in model.newTerminalSession(for: agent, on: other.id) })
            }
            if account.lane == nil && free.isEmpty { note("No Free Saved Account") }
            menu.addItem(.separator())
        }
        header("New Background Session Here")
        let folder = agent.cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser.path : agent.cwd
        for target in model.sessionTargets {
            menu.addItem(ActionMenuItem(target.title) { [model, agent] in
                model.newBackgroundSession(in: folder, on: target.accountID, errorKey: agent.id)
            })
        }
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

/// A menu item that runs a closure (our own selector: macOS 26 adds icons to standard actions).
final class ActionMenuItem: NSMenuItem {
    private let run: () -> Void

    init(_ title: String, run: @escaping () -> Void) {
        self.run = run
        super.init(title: title, action: #selector(fire), keyEquivalent: "")
        target = self
    }

    required init(coder: NSCoder) { fatalError("not used") }

    @objc private func fire() { run() }
}

/// A limit switch that moved this session, or an account action that failed.
private struct AccountBanner: View {
    let text: String
    let isError: Bool
    let dismiss: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: isError ? "exclamationmark.triangle" : "arrow.triangle.swap")
                .font(.system(size: 12, weight: .medium))
                .foregroundColor(isError ? KitInk.needs : KitInk.accentText)
            Text(text).kitType(.label).foregroundColor(KitInk.text).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 4)
            Button(action: dismiss) { Image(systemName: "xmark").font(.system(size: 10, weight: .semibold)) }
                .buttonStyle(.kit(.ghost)).help("Dismiss")
        }
        .padding(.leading, 14).padding(.trailing, 6).padding(.vertical, 6)
        .kitCard(radius: KitMetric.radiusMd, fill: KitInk.accentWash)
    }
}

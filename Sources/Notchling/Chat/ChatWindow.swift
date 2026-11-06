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

final class ChatModel: ObservableObject {
    /// A reply on its way: shown in the conversation until the transcript has it.
    struct Pending: Equatable {
        let text: String
        let at: Date
        var sending: Bool
        /// Sent right after another reply: only the same words count as landed, since the earlier
        /// reply's line also shows up after this one was sent.
        var exact = false

        /// The transcript already shows this reply: the same words, or any message of yours written
        /// since it was sent (a long paste lands in another shape). Until then it shows as pending.
        func landed(in conversation: Conversation) -> Bool {
            let want = Self.words(text)
            // Queued behind a busy turn: it reached the session, and shows as a queued bubble.
            if conversation.queued.contains(where: { Self.words($0) == want }) { return true }
            return conversation.items.suffix(12).contains { item in
                guard case .user(let said, _) = item.kind else { return false }
                guard let date = item.date, !exact else { return Self.words(said) == want }
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
            subagents = []
            subagentID = nil
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
    /// Which accounts the Accounts sheet shows: Claude Code's, the Codex CLI's, or Antigravity's (whose
    /// login Antigravity CLI uses too).
    enum AccountTab: String, CaseIterable { case claude = "Claude Code", codex = "Codex", antigravity = "Antigravity" }
    @Published var accountSheet = false {
        didSet { if !accountSheet { closeAccounts() } }
    }
    @Published var accountTab = AccountTab.claude
    /// The Secret Vault sheet (`VaultSheet`): credentials agents use by name, never through the chat.
    @Published var vaultSheet = false
    /// The New Session sheet (`NewSessionSheet`): account, folder, Start.
    @Published var newSessionSheet = false {
        didSet { if !newSessionSheet { closeAccounts() } }
    }
    /// The Claude sign-in while it runs: the browser OAuth, saved next to the other accounts without
    /// touching the login sessions use.
    @Published var signIn: SignInModel? {
        didSet { if oldValue !== signIn { oldValue?.cancel() } }
    }
    let codex: CodexStore?
    /// Antigravity's saved Google logins. `agy` signs in through the same Keychain item (its log says
    /// "authenticated via keyring" as the app's account), so switching one switches both.
    let antigravity: AntigravityStore?

    /// Opens the Accounts sheet; Claude Code starts its browser sign-in at once, Codex shows its
    /// accounts with Add Account and Switch.
    func addAccount(_ tab: AccountTab = .claude) {
        accountTab = tab
        accountSheet = true
        switch tab {
        case .claude: startClaudeSignIn()
        case .codex: codex?.sync(force: true)
        case .antigravity: antigravity?.sync()
        }
    }

    func startClaudeSignIn() {
        guard let accounts else { return }
        let signIn = SignInModel(store: accounts)
        self.signIn = signIn
        signIn.start()
    }

    /// However the sheet closes (Esc, Done), stop waiting for the browser.
    private func closeAccounts() {
        signIn = nil
        codex?.cancelAdd()
    }
    /// The Artifact open in the panel beside the chat.
    @Published var page: PageLibrary.Entry? {
        didSet {
            pageChecked = nil
            if page != nil { file = nil }
        }
    }
    /// A file opened from a link in a message, shown in the same place as a page (`FilePanel`).
    @Published var file: URL? {
        didSet { if file != nil { page = nil } }
    }
    var panelOpen: Bool { page != nil || file != nil }

    /// The selected session's subagents (`Subagents`), newest last.
    @Published private(set) var subagents: [Subagent] = []
    /// The subagent being read in place of its session's conversation; nil shows the session.
    @Published var subagentID: String? {
        didSet {
            guard subagentID != oldValue else { return }
            subConversation = nil
            refresh()
        }
    }
    @Published private(set) var subConversation: Conversation?
    var viewedSubagent: Subagent? { subagentID.flatMap { id in subagents.first { $0.id == id } } }
    /// The subagent an Agent call started, for the call's Open button.
    func subagent(forCall toolUseID: String) -> Subagent? { subagents.first { $0.toolUseID == toolUseID } }
    /// The selected background session attached in a terminal sheet, for a command that opens a menu.
    @Published var terminal: TerminalSession?
    @Published var drafts: [String: String] = [:]
    @Published private(set) var pending: [String: Pending] = [:]
    /// Replies typed while the one before is still going out, sent in order after it. Before this,
    /// Return did nothing while a reply was being typed into the session.
    @Published private(set) var outbox: [String: [String]] = [:]
    private var modelHint: [String: (model: String, at: Date)] = [:]
    private var effortHint: [String: (effort: String, at: Date)] = [:]
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
    private var removedWatch: AnyCancellable?

    init(notch: NotchModel, accounts: AccountStore? = nil, codex: CodexStore? = nil, antigravity: AntigravityStore? = nil) {
        self.notch = notch
        self.accounts = accounts
        self.codex = codex
        self.antigravity = antigravity
        expansion.openCall = { [weak self] call in self?.subagentID = self?.expansion.subagentCalls[call] }
        watch = notch.$agents.sink { [weak self] list in
            for agent in list { self?.seen[agent.id] = agent }
        }
        // A removed session is gone: stop showing it, leave nothing picked.
        removedWatch = NotificationCenter.default.publisher(for: .sessionRemoved).sink { [weak self] note in
            guard let self, let id = note.object as? String else { return }
            self.seen[id] = nil
            if self.selection == id { self.selection = nil }
        }
        accountsWatch = accounts?.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async { self?.objectWillChange.send() }
        }
        codexWatch = codex?.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async { self?.objectWillChange.send() }
        }
        antigravityWatch = antigravity?.objectWillChange.sink { [weak self] _ in
            DispatchQueue.main.async { self?.objectWillChange.send() }
        }
        agyWatch = AntigravityRunner.shared.$version.sink { [weak self] _ in
            DispatchQueue.main.async { self?.antigravityChanged() }
        }
    }
    private var codexWatch: AnyCancellable?
    private var agyWatch: AnyCancellable?
    private var antigravityWatch: AnyCancellable?

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
    private var lanesChecked = Date.distantPast
    /// Limit switches by lane name ("" for the main login), shown in the sessions they moved.
    @Published var accountNotes: [String: AccountNote] = [:]
    @Published var accountErrors: [String: String] = [:]

    struct AccountNote: Equatable {
        let text: String
        let at: Date
    }

    func account(for agent: Agent) -> SessionAccount? {
        if case .codex = agent.target { return nil }
        if case .antigravity = agent.target { return nil }
        if let lane = lanes[agent.id] { return SessionAccount(email: lane.email, lane: lane) }
        // In a lane folder the lanes list doesn't name (yet): don't guess.
        if agent.configDir != nil { return nil }
        return accounts?.active.map { SessionAccount(email: $0.email, lane: nil) }
    }

    /// The limits of the account the session runs on, as last fetched.
    func usage(for agent: Agent) -> AccountUsage? {
        guard let accounts else { return nil }
        if case .codex = agent.target { return nil }
        if case .antigravity = agent.target { return nil }
        if let lane = lanes[agent.id] { return accounts.usage[lane.accountID] }
        if agent.configDir != nil { return nil }
        return accounts.activeID.flatMap { accounts.usage[$0] }
    }

    /// The limit bars the header shows: the Claude account's 5-hour and weekly limits, or for a Codex
    /// session the Codex login's (its first group, "Codex": 5h and week on Plus and Pro).
    func limits(for agent: Agent) -> [SessionLimit] {
        if case .codex = agent.target {
            guard let codex, let id = codex.activeID, let usage = codex.usage[id] else { return [] }
            let group = usage.groups.first { $0.name == "Codex" } ?? usage.groups.first
            return (group?.bars ?? []).map { SessionLimit(label: $0.label, name: SessionLimit.name($0.label), window: $0.window) }
        }
        guard let usage = usage(for: agent) else { return [] }
        return [usage.session.map { SessionLimit(label: "5h", name: "5-hour limit", window: $0) },
                usage.week.map { SessionLimit(label: "Week", name: "Weekly limit", window: $0) }].compactMap { $0 }
    }

    /// Saved accounts no session uses: not signed out, not the main login, not in a lane.
    var freeAccounts: [ClaudeAccount] {
        guard let accounts else { return [] }
        let taken = Set(LaneStore.all().map(\.accountID))
        return accounts.accounts.filter { !$0.needsLogin && $0.id != accounts.activeID && !taken.contains($0.id) }
    }

    func refreshLanes(force: Bool = false) {
        guard force || Date().timeIntervalSince(lanesChecked) > 5 else { return }
        lanesChecked = Date()
        let agents = notch.agents
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let all = LaneStore.all()
            var found: [String: Lane] = [:]
            for agent in agents {
                if let dir = agent.configDir, let lane = all.first(where: { $0.path == dir }) { found[agent.id] = lane }
            }
            DispatchQueue.main.async {
                guard let self else { return }
                if self.lanes != found { self.lanes = found }
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
    /// (picking one makes a lane for it). `accountID` nil means the main login. Titled by email only:
    /// where a session runs is Notchling's business, and a lane's name goes stale after a switch.
    struct SessionTarget { let title: String; let accountID: String? }

    var sessionTargets: [SessionTarget] {
        guard let accounts else { return [] }
        var out = [SessionTarget(title: accounts.active?.email ?? "Main Login", accountID: nil)]
        out += LaneStore.all().map { SessionTarget(title: $0.email, accountID: $0.accountID) }
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
    /// `done` (with nil on success, else what went wrong) replaces the alert: New Session shows it in the
    /// sheet. `trust` first accepts Claude Code's trust prompt for the folder.
    func newBackgroundSession(in folder: String, on accountID: String?, errorKey: String? = nil,
                              trust: Bool = false, done: ((String?) -> Void)? = nil) {
        let fail: (String) -> Void = { [weak self] text in
            if let done { done(text); return }
            if let errorKey { self?.accountErrors[errorKey] = text; return }
            let alert = NSAlert()
            alert.messageText = "Couldn't start the session"
            alert.informativeText = text
            alert.runModal()
        }
        if let errorKey { accountErrors[errorKey] = nil }
        guard let accounts else { return fail("Claude Code accounts aren't loaded yet.") }
        let start: (Lane?) -> Void = { [weak self] lane in
            DispatchQueue.global(qos: .userInitiated).async {
                if trust, !ReplySender.trustFolder(folder, configDir: lane?.path) {
                    DispatchQueue.main.async { fail("Couldn't trust the folder. Run claude there once and accept its prompt.") }
                    return
                }
                let result = Self.startBackground(in: folder, configDir: lane?.path)
                DispatchQueue.main.async {
                    switch result {
                    case .success(let shortID): done?(nil); self?.selectWhenListed(shortID)
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
                    Self.startBackground(in: cwd, configDir: lane?.path,
                                         arguments: Self.resumeArguments(conversation, job: shortID, in: lane?.path, flags: flags))
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

    /// The new job's short id from `claude --bg` output: "backgrounded · 6ce4c19c · name (idle — …)", or
    /// since 2.1.318 "backgrounded · 6220cc2b (idle — …)", maybe after a note line.
    static func jobID(inLaunchOutput text: String) -> String? {
        for line in text.split(separator: "\n") {
            let parts = line.components(separatedBy: " · ")
            guard parts.count > 1,
                  let id = parts[1].split(whereSeparator: \.isWhitespace).first.map(String.init) else { continue }
            if id.count == 8, id.allSatisfy(\.isHexDigit) { return id }
        }
        return nil
    }

    /// `--resume` for a session moving into `configDir`. Where that folder already has a background job
    /// for the conversation (the copy left there by an earlier move), Claude Code continues that job with
    /// its own saved options, and since 2.1.318 any flags passed start a separate copy instead; so flags
    /// go along only when there's no job to continue.
    static func resumeArguments(_ conversation: String, job shortID: String, in configDir: String?, flags: [String]) -> [String] {
        let dir = configDir.map { URL(fileURLWithPath: $0) } ?? LaneStore.mainDir
        let existing = FileManager.default.fileExists(atPath: dir.appendingPathComponent("jobs/\(shortID)/state.json").path)
        return (existing ? [] : flags) + ["--bg", "--resume", conversation]
    }

    /// The flags the job was started with (name, model, permission mode), so the moved session keeps them.
    static func launchFlags(forJob shortID: String, name: String) -> [String] {
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

    static func stopJob(_ shortID: String) -> Result<Void, Error> {
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

    static func startBackground(in folder: String, configDir: String?,
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
        if let id = jobID(inLaunchOutput: text) { return .success(id) }
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
        lanesChecked = .distantFuture
    }
    #endif

    func dismissAccountNote(for agent: Agent) {
        guard let current = account(for: agent) else { return }
        accountNotes[current.lane?.name ?? ""] = nil
    }

    var selected: Agent? {
        guard let id = selection else { return nil }
        return allAgents.first { $0.id == id } ?? seen[id]
    }

    /// Claude Code's sessions (the notch's), then Codex's and Antigravity CLI's, which only Chat shows.
    var allAgents: [Agent] {
        notch.agents + codexDrafts + (codexAgents + agyAgents).filter { !isClosed($0) } + agyDrafts
    }

    /// Codex and Antigravity rows the user closed, with when. The notch's Hide only knows Claude Code's
    /// sessions, so these are kept here; a row comes back when its session is used again later.
    @Published private var closedSessions: [String: Double] =
        UserDefaults.standard.dictionary(forKey: "chatClosedSessions") as? [String: Double] ?? [:]

    private func isClosed(_ agent: Agent) -> Bool {
        guard let at = closedSessions[agent.id] else { return false }
        return agent.since.timeIntervalSince1970 <= at
    }

    /// Takes a Codex or Antigravity session off the list (its history stays with the tool), and ends its
    /// Antigravity process.
    func closeSession(_ agent: Agent) {
        switch agent.target {
        case .antigravity(let session):
            guard !session.isEmpty else { closeAntigravity(agent); return }
            AntigravityRunner.shared.close(session)
        case .codex(let session) where session.isEmpty:
            removeCodex(agent)
            return
        default: break
        }
        closedSessions[agent.id] = max(Date().timeIntervalSince1970, agent.since.timeIntervalSince1970)
        // Forget entries older than the 14 days the lists cover.
        let cutoff = Date().timeIntervalSince1970 - 15 * 86_400
        closedSessions = closedSessions.filter { $0.value > cutoff }
        UserDefaults.standard.set(closedSessions, forKey: "chatClosedSessions")
        if selection == agent.id { selection = nil }
    }

    // MARK: Codex sessions

    /// Recent Codex CLI sessions (`CodexSessions`), re-read every couple of seconds.
    @Published private(set) var codexAgents: [Agent] = []
    /// New Codex sessions picked in New Session that wait for their first message, which starts them.
    @Published private(set) var codexDrafts: [Agent] = []
    private var codexListedAt = Date.distantPast
    private var codexListing = false

    func refreshCodex(force: Bool = false) {
        guard codex != nil, !codexListing, force || Date().timeIntervalSince(codexListedAt) > 2 else { return }
        codexListing = true
        let running = CodexRunner.shared.running
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let list = CodexSessions.list(running: running)
            DispatchQueue.main.async {
                guard let self else { return }
                self.codexListing = false
                self.codexListedAt = Date()
                if list.map(\.id) != self.codexAgents.map(\.id) || zip(list, self.codexAgents).contains(where: {
                    $0.group != $1.group || $0.since != $1.since || $0.name != $1.name
                }) {
                    self.codexAgents = list
                }
            }
        }
    }

    /// Deletes a Codex session after asking (`codex delete`); a draft just goes.
    func removeCodex(_ agent: Agent) {
        guard case .codex(let session) = agent.target else { return }
        if session.isEmpty {
            codexDrafts.removeAll { $0.id == agent.id }
            if selection == agent.id { selection = nil }
            return
        }
        let alert = NSAlert()
        alert.messageText = "Delete “\(agent.name)”?"
        alert.informativeText = "Codex deletes the session and its history. Files it changed stay as they are."
        alert.addButton(withTitle: "Delete")
        alert.addButton(withTitle: "Cancel")
        alert.buttons.first?.hasDestructiveAction = true
        guard alert.runModal() == .alertFirstButtonReturn else { return }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let failure = CodexRunner.delete(session)
            DispatchQueue.main.async {
                guard let self else { return }
                if let failure {
                    self.errors[agent.id] = "Couldn't delete it: \(failure)"
                    return
                }
                self.codexAgents.removeAll { $0.id == agent.id }
                if self.selection == agent.id { self.selection = nil }
                self.refreshCodex(force: true)
            }
        }
    }

    /// A new Codex session in `folder`: listed and selected at once, started by its first message.
    func newCodexSession(in folder: String) {
        addProjectFolder(folder)
        let agent = Agent(id: "codex-new:" + UUID().uuidString, name: "New Codex Session",
                          project: (folder as NSString).lastPathComponent, cwd: folder, group: .completed, label: "New",
                          detail: nil, since: Date(), target: .codex(sessionID: ""), host: "Codex")
        codexDrafts.insert(agent, at: 0)
        selection = agent.id
        focusRequest += 1
    }

    /// A Codex slash command: the model picker, a new session or delete here; the rest in Codex's own
    /// interface in the terminal sheet (`codex exec`, which Chat's turns use, has no slash commands).
    private func runCodexCommand(_ name: String, text: String, agent: Agent, session: String, clearDraft: Bool) {
        let id = agent.id
        let done = { [weak self] in if clearDraft { self?.drafts[id] = "" } }
        switch CodexCommands.route(name) {
        case .modelPicker:
            done()
            CodexModelPicker(model: self, agent: agent).popUp()
        case .newSession:
            done()
            newCodexSession(in: agent.cwd)
        case .delete:
            done()
            removeCodex(agent)
        case .ignore:
            done()
        case .terminal:
            // Two Codex processes on one session would both write its file.
            if !session.isEmpty, CodexRunner.shared.isRunning(session) {
                errors[id] = "Wait for this turn to end, or stop it, then run \(text)."
                return
            }
            done()
            errors[id] = nil
            terminal?.close()
            terminal = TerminalSession(program: CodexCommands.script(session: session),
                                       in: agent.cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser.path : agent.cwd,
                                       title: "Codex · \(agent.name)", typing: text)
        }
    }

    private func sendCodex(_ agent: Agent, sessionID: String, text: String) {
        let id = agent.id
        errors[id] = nil
        // One turn at a time: exec can't take a message mid-turn, so the next waits for this one.
        if pending[id]?.sending == true || (!sessionID.isEmpty && CodexRunner.shared.isRunning(sessionID)) {
            outbox[id, default: []].append(text)
            return
        }
        pending[id] = Pending(text: text, at: Date(), sending: true, exact: false)
        CodexRunner.shared.run(text, session: sessionID.isEmpty ? nil : sessionID, folder: agent.cwd,
                               pick: codexPicks[id] ?? CodexPick(), started: { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let session):
                var key = id
                if sessionID.isEmpty {
                    // The draft becomes the real session: carry its reply, box and selection over.
                    key = CodexSessions.agentID(session)
                    self.pending[key] = self.pending.removeValue(forKey: id)
                    self.drafts[key] = self.drafts.removeValue(forKey: id)
                    self.outbox[key] = self.outbox.removeValue(forKey: id)
                    if let pick = self.codexPicks[id] {
                        self.setCodexPick(CodexPick(), for: id)
                        self.setCodexPick(pick, for: key)
                    }
                    self.codexDrafts.removeAll { $0.id == id }
                    self.seen[key] = Agent(id: key, name: CodexSessions.title(text), project: agent.project, cwd: agent.cwd,
                                           group: .working, label: "Working", detail: nil, since: Date(),
                                           target: .codex(sessionID: session), host: "Codex")
                    if self.selection == id { self.selection = key }
                }
                self.pending[key]?.sending = false
                self.refreshCodex(force: true)
            case .failure(let error):
                self.pending[id] = nil
                self.errors[id] = error.localizedDescription
                let draft = self.drafts[id] ?? ""
                self.drafts[id] = ([text] + (draft.isEmpty ? [] : [draft])).joined(separator: "\n\n")
            }
        }, ended: { [weak self] session, failure in
            guard let self else { return }
            let key = session.map(CodexSessions.agentID) ?? id
            if let failure { self.errors[key] = failure }
            self.refreshCodex(force: true)
            self.refresh()
            // Messages typed during the turn go out now, one turn each.
            if var waiting = self.outbox[key], !waiting.isEmpty, let session {
                let next = waiting.removeFirst()
                self.outbox[key] = waiting.isEmpty ? nil : waiting
                let current = self.allAgents.first { $0.id == key } ?? self.seen[key] ?? agent
                self.sendCodex(current, sessionID: session, text: next)
            }
        })
    }

    /// The model and effort picked per Codex session (by row id), for its next turns. Kept across
    /// launches, since nothing else remembers them: exec takes them per turn.
    @Published private(set) var codexPicks: [String: CodexPick] = {
        guard let data = UserDefaults.standard.data(forKey: "codexPicks"),
              let saved = try? JSONDecoder().decode([String: CodexPick].self, from: data) else { return [:] }
        return saved
    }()

    func setCodexPick(_ pick: CodexPick, for id: String) {
        codexPicks[id] = pick == CodexPick() ? nil : pick
        if let data = try? JSONEncoder().encode(codexPicks.filter { !$0.key.hasPrefix("codex-new:") }) {
            UserDefaults.standard.set(data, forKey: "codexPicks")
        }
    }

    // MARK: Antigravity CLI sessions

    /// Recent Antigravity CLI conversations (`AntigravitySessions`), re-read every few seconds.
    @Published private(set) var agyAgents: [Agent] = []
    /// New Antigravity sessions picked in New Session that wait for their first message.
    @Published private(set) var agyDrafts: [Agent] = []
    private var agyListedAt = Date.distantPast
    private var agyListing = false

    func refreshAntigravity(force: Bool = false) {
        guard AntigravityRunner.binary() != nil, !agyListing, force || Date().timeIntervalSince(agyListedAt) > 3 else { return }
        agyListing = true
        let working = AntigravityRunner.shared.working
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let list = AntigravitySessions.list(live: working)
            DispatchQueue.main.async {
                guard let self else { return }
                self.agyListing = false
                self.agyListedAt = Date()
                // A session started here that the summaries don't list yet stays as it was.
                let listed = Set(list.map(\.id))
                let extra = self.agyStarted.filter { !listed.contains($0.id) }
                let merged = list.map { self.withLiveTitle($0) } + extra
                if merged.map(\.id) != self.agyAgents.map(\.id) || zip(merged, self.agyAgents).contains(where: {
                    $0.group != $1.group || $0.name != $1.name || $0.since != $1.since
                }) {
                    self.agyAgents = merged
                }
            }
        }
    }

    /// Sessions started from Chat, kept until Antigravity's summaries list them.
    private var agyStarted: [Agent] = []

    /// A row with the live state the runner knows: working, and the title Antigravity gave it.
    private func withLiveTitle(_ agent: Agent) -> Agent {
        guard case .antigravity(let session) = agent.target else { return agent }
        let runner = AntigravityRunner.shared
        let working = runner.isWorking(session)
        let title = runner.title(session) ?? agent.name
        guard working != (agent.group == .working) || title != agent.name else { return agent }
        return Agent(id: agent.id, name: title, project: agent.project, cwd: agent.cwd,
                     group: working ? .working : .completed, label: working ? "Working" : "Antigravity",
                     detail: nil, since: working ? Date() : agent.since, target: agent.target, host: "Antigravity")
    }

    /// The runner changed (a chunk, a tool call, a permission, a turn's end).
    private func antigravityChanged() {
        if let agent = selected, case .antigravity(let session) = agent.target, !session.isEmpty,
           let fresh = AntigravityRunner.shared.conversation(session), fresh != conversation {
            conversation = fresh
            dropDelivered(agent.id)
        }
        let updated = agyAgents.map { withLiveTitle($0) }
        if updated.map(\.name) != agyAgents.map(\.name) || updated.map(\.group) != agyAgents.map(\.group) { agyAgents = updated }
        objectWillChange.send()
    }

    /// A new Antigravity session in `folder`: listed and selected at once, started by its first message.
    func newAntigravitySession(in folder: String) {
        addProjectFolder(folder)
        let agent = Agent(id: "agy-new:" + UUID().uuidString, name: "New Antigravity Session",
                          project: (folder as NSString).lastPathComponent, cwd: folder, group: .completed, label: "New",
                          detail: nil, since: Date(), target: .antigravity(sessionID: ""), host: "Antigravity")
        agyDrafts.insert(agent, at: 0)
        selection = agent.id
        focusRequest += 1
    }

    /// Closes a draft, or the session's Antigravity process (the conversation stays in Antigravity).
    func closeAntigravity(_ agent: Agent) {
        guard case .antigravity(let session) = agent.target else { return }
        if session.isEmpty {
            agyDrafts.removeAll { $0.id == agent.id }
            if selection == agent.id { selection = nil }
        } else {
            AntigravityRunner.shared.close(session)
        }
    }

    private func sendAntigravity(_ agent: Agent, sessionID: String, text: String) {
        let id = agent.id
        errors[id] = nil
        let runner = AntigravityRunner.shared
        // One turn at a time: what's typed meanwhile goes out when this turn ends.
        if pending[id]?.sending == true || (!sessionID.isEmpty && runner.isWorking(sessionID)) {
            outbox[id, default: []].append(text)
            return
        }
        pending[id] = Pending(text: text, at: Date(), sending: true, exact: false)
        let ended: (String, String) -> (String?) -> Void = { [weak self] key, session in { failure in
            guard let self else { return }
            if let failure { self.errors[key] = failure }
            self.pending[key] = nil
            self.refreshAntigravity(force: true)
            if var waiting = self.outbox[key], !waiting.isEmpty {
                let next = waiting.removeFirst()
                self.outbox[key] = waiting.isEmpty ? nil : waiting
                let current = self.allAgents.first { $0.id == key } ?? self.seen[key] ?? agent
                self.sendAntigravity(current, sessionID: session, text: next)
            }
        } }
        guard sessionID.isEmpty else {
            pending[id]?.sending = false
            runner.prompt(sessionID, text: text, folder: agent.cwd, ended: ended(id, sessionID))
            return
        }
        runner.start(in: agent.cwd) { [weak self] result in
            guard let self else { return }
            switch result {
            case .success(let session):
                // The draft becomes the real session: carry its reply, box and selection over.
                let key = AntigravitySessions.agentID(session)
                self.pending[key] = self.pending.removeValue(forKey: id)
                self.pending[key]?.sending = false
                self.drafts[key] = self.drafts.removeValue(forKey: id)
                self.outbox[key] = self.outbox.removeValue(forKey: id)
                self.agyDrafts.removeAll { $0.id == id }
                let row = Agent(id: key, name: AntigravitySessions.title(text), project: agent.project, cwd: agent.cwd,
                                group: .working, label: "Working", detail: nil, since: Date(),
                                target: .antigravity(sessionID: session), host: "Antigravity")
                self.seen[key] = row
                self.agyStarted.append(row)
                self.agyAgents.insert(row, at: 0)
                if self.selection == id { self.selection = key }
                runner.prompt(session, text: text, folder: agent.cwd, ended: ended(key, session))
            case .failure(let error):
                self.pending[id] = nil
                self.errors[id] = error.localizedDescription
                let draft = self.drafts[id] ?? ""
                self.drafts[id] = ([text] + (draft.isEmpty ? [] : [draft])).joined(separator: "\n\n")
            }
        }
    }

    /// Sessions a Stop was sent to, until they stop working (or 20 s pass).
    @Published private(set) var stopping: [String: Date] = [:]

    /// The selected session's turn can be stopped from here: a working background Claude session (Esc
    /// through `claude attach`) or a Codex turn Chat runs. Terminal sessions belong to their window.
    func canStop(_ agent: Agent) -> Bool {
        guard agent.group == .working else { return false }
        switch agent.target {
        case .job: return agent.command == nil
        case .codex(let session): return !session.isEmpty && CodexRunner.shared.isRunning(session)
        case .antigravity(let session): return !session.isEmpty && AntigravityRunner.shared.isWorking(session)
        default: return false
        }
    }

    func isStopping(_ agent: Agent) -> Bool {
        guard let at = stopping[agent.id] else { return false }
        return agent.group == .working && Date().timeIntervalSince(at) < 20
    }

    /// Sessions whose queue is being taken back to edit.
    @Published private(set) var takingBack: Set<String> = []

    /// Takes the newest message that hasn't reached the conversation yet back to the reply box: the last
    /// one waiting in Chat's outbox, or the last one queued in the session (Up in its input box, as in the
    /// terminal; older queued ones go back in the queue). Sending it again queues it as usual.
    func editLatestQueued() {
        guard let agent = selected else { return }
        let id = agent.id
        let restore: (String) -> Void = { [weak self] text in
            guard let self else { return }
            let draft = self.drafts[id] ?? ""
            self.drafts[id] = draft.isEmpty ? text : text + "\n\n" + draft
            self.focusRequest += 1
        }
        if var waiting = outbox[id], let last = waiting.popLast() {
            outbox[id] = waiting.isEmpty ? nil : waiting
            restore(last)
            return
        }
        guard case .job(let jobID) = agent.target, let latest = conversation.queued.last,
              let transcript = agent.transcriptPath,
              pending[id]?.sending != true, !takingBack.contains(id) else { return }
        let keep = Array(conversation.queued.dropLast())
        errors[id] = nil
        // Holds other replies back while it's attached; they'd type into the same box.
        takingBack.insert(id)
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var failure: Error?
            var taken = false
            do {
                taken = try ReplySender.takeBackQueued(latest: latest, keep: keep, fromJob: jobID, transcript: transcript)
            } catch { failure = error }
            DispatchQueue.main.async {
                guard let self else { return }
                self.takingBack.remove(id)
                if let failure {
                    self.errors[id] = "Couldn't take it back: \(failure.localizedDescription)"
                } else if !taken {
                    self.errors[id] = "It was already sent."
                } else {
                    restore(latest)
                }
                self.refresh()
                self.sendNext(id, job: jobID)
            }
        }
    }

    /// Stops the turn, like Esc in the terminal. What was said so far stays.
    func stopTurn() {
        guard let agent = selected, canStop(agent), !isStopping(agent) else { return }
        let id = agent.id
        stopping[id] = Date()
        errors[id] = nil
        switch agent.target {
        case .codex(let session):
            CodexRunner.shared.stop(session)
        case .antigravity(let session):
            AntigravityRunner.shared.cancel(session)
        case .job(let jobID):
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                var failure: Error?
                do { try ReplySender.interrupt(job: jobID) } catch { failure = error }
                DispatchQueue.main.async {
                    guard let self else { return }
                    if let failure {
                        self.stopping[id] = nil
                        self.errors[id] = "Couldn't stop it: \(failure.localizedDescription)"
                    }
                    self.refresh()
                }
            }
        default:
            break
        }
        // Clear the mark once it's done, so a later turn can be stopped too.
        DispatchQueue.main.asyncAfter(deadline: .now() + 20) { [weak self] in self?.stopping[id] = nil }
    }

    /// The sidebar list: the notch's sessions, filtered by the search words.
    var shown: [Agent] {
        let words = search.lowercased().split(separator: " ")
        var list = allAgents.filter { agent in
            let text = "\(agent.name) \(agent.project) \(agent.host)".lowercased()
            return words.allSatisfy { text.contains($0) }
        }
        if let s = selected, !list.contains(where: { $0.id == s.id }), words.isEmpty { list.append(s) }
        return list
    }

    func refresh() {
        refreshPage()
        if accounts != nil { refreshLanes() }
        refreshCodex()
        refreshAntigravity()
        guard let agent = selected else { return }
        if case .antigravity(let session) = agent.target {
            if session.isEmpty {
                if conversation != Conversation() { conversation = Conversation() }
            } else {
                AntigravityRunner.shared.open(session, folder: agent.cwd)
                let fresh = AntigravityRunner.shared.conversation(session) ?? Conversation()
                if fresh != conversation { conversation = fresh }
                dropDelivered(agent.id)
            }
            return
        }
        let viewing = subagentID
        let isCodex: Bool = { if case .codex = agent.target { return true }; return false }()
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            guard let path = TranscriptReader.path(for: agent) else { return }
            let fresh = isCodex ? CodexReader.read(path) : ConversationReader.read(path)
            let subs = isCodex ? [] : Subagents.list(forTranscript: path, finished: fresh.finishedAgents)
            let sub = viewing.flatMap { id in subs.first { $0.id == id } }.map { ConversationReader.readSubagent($0.path) }
            DispatchQueue.main.async {
                guard let self, self.selection == agent.id else { return }
                if subs != self.subagents {
                    self.subagents = subs
                    let calls = Dictionary(subs.compactMap { s in s.toolUseID.map { ($0, s.id) } }, uniquingKeysWith: { a, _ in a })
                    if calls != self.expansion.subagentCalls { self.expansion.subagentCalls = calls }
                }
                if self.subagentID == viewing, sub != self.subConversation { self.subConversation = sub }
                var fresh = fresh
                // A model you just picked shows at once; the transcript catches up a few seconds later.
                if let hint = self.modelHint[agent.id] {
                    if fresh.facts.model == hint.model || Date().timeIntervalSince(hint.at) > 30 { self.modelHint[agent.id] = nil }
                    else { fresh.facts.model = hint.model }
                }
                if let hint = self.effortHint[agent.id] {
                    if fresh.facts.effort == hint.effort || Date().timeIntervalSince(hint.at) > 30 { self.effortHint[agent.id] = nil }
                    else { fresh.facts.effort = hint.effort }
                }
                if fresh != self.conversation { self.conversation = fresh }
                self.dropDelivered(agent.id)
            }
        }
    }

    /// A sent reply leaves the pending row once the transcript shows it, or its queue. One that never
    /// shows up didn't reach the session: say so and put it back in the box rather than drop it quietly.
    private func dropDelivered(_ id: String) {
        guard let p = pending[id] else { return }
        // Landed even while the send is still finishing: otherwise it shows twice for a moment.
        if p.landed(in: conversation) { pending[id] = nil; return }
        guard !p.sending, Date().timeIntervalSince(p.at) > Self.deliveryWait else { return }
        pending[id] = nil
        errors[id] = "The session didn't get this message. It's back in the reply box."
        if (drafts[id] ?? "").isEmpty { drafts[id] = p.text }
    }

    /// Claude Code writes a reply (or its queue entry) as soon as it's submitted; this leaves room for
    /// slow UserPromptSubmit hooks.
    static let deliveryWait: TimeInterval = 45

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

    /// `pages.json`'s modification date when `refreshPage` last read it for the shown page (nil: not read yet),
    /// so the 1-second tick parses the list only after it changed.
    private var pageChecked: Date??

    /// The panel's page again from the library, so a republish shows up.
    private func refreshPage() {
        guard let shown = page, !shown.url.isEmpty else { return }
        let stamp = PageLibrary.listModified
        if let checked = pageChecked, checked == stamp { return }
        pageChecked = stamp
        guard let fresh = PageLibrary.find(shown.url).first(where: { $0.folder == shown.folder }),
              fresh != shown else { return }
        page = fresh
    }

    // MARK: Replying

    func send(_ preset: String? = nil) {
        if let agent = selected, case .antigravity(let session) = agent.target {
            let text = (preset ?? drafts[agent.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty, AntigravityRunner.shared.permission(session) == nil else { return }
            if preset == nil { drafts[agent.id] = "" }
            sendAntigravity(agent, sessionID: session, text: text)
            return
        }
        if let agent = selected, case .codex(let session) = agent.target {
            let text = (preset ?? drafts[agent.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else { return }
            if let name = CodexCommands.command(in: text) {
                runCodexCommand(name, text: text, agent: agent, session: session, clearDraft: preset == nil)
                return
            }
            if preset == nil { drafts[agent.id] = "" }
            sendCodex(agent, sessionID: session, text: text)
            return
        }
        guard let agent = selected, agent.canType,
              notch.question(agent.id) == nil, notch.permission(agent.id) == nil else { return }
        let text = ShellCommand.normalize((preset ?? drafts[agent.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines))
        guard !text.isEmpty, text != "!" else { return }
        let id = agent.id
        // A terminal session the session host owns: typed into its pty, one reply at a time.
        guard case .job(let jobID) = agent.target else {
            guard pending[id]?.sending != true else { return }
            if preset == nil { drafts[id] = "" }
            errors[id] = nil
            pending[id] = Pending(text: text, at: Date(), sending: true)
            DispatchQueue.global(qos: .userInitiated).async { [weak self] in
                var failure: Error?
                do { try ReplySender.send(text, to: agent) } catch { failure = error }
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
            return
        }
        // A bare /model wants a picker, which a background session can't show: show ours.
        if text == "/model" {
            if preset == nil { drafts[id] = "" }
            ModelPicker.popUp(model: self, current: conversation.facts.model)
            return
        }
        if text == "/effort" {
            if preset == nil { drafts[id] = "" }
            EffortPicker.popUp(model: self, current: conversation.facts.effort)
            return
        }
        if preset == nil { drafts[id] = "" }
        errors[id] = nil
        // One attach at a time: two would type into the same box. The next waits its turn.
        if pending[id]?.sending == true || takingBack.contains(id) {
            outbox[id, default: []].append(text)
            return
        }
        if SlashCommands.opensMenu(text) {
            openTerminal(typing: text)
            return
        }
        deliver(text, to: id, job: jobID, exact: false)
    }

    private func deliver(_ text: String, to id: String, job jobID: String, exact: Bool) {
        if text.hasPrefix("/model "), let picked = text.split(separator: " ").last.map(String.init), picked.hasPrefix("claude-") {
            modelHint[id] = (picked, Date())
            if selection == id { conversation.facts.model = picked }
        }
        if text.hasPrefix("/effort "), let picked = text.split(separator: " ").last.map(String.init),
           EffortPicker.levels.contains(picked) {
            effortHint[id] = (picked, Date())
            if selection == id { conversation.facts.effort = picked }
        }
        pending[id] = Pending(text: text, at: Date(), sending: true, exact: exact)
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var failure: Error?
            do { try ReplySender.send(text, toJob: jobID) } catch { failure = error }
            DispatchQueue.main.async {
                guard let self else { return }
                if let failure {
                    // Nothing typed after it goes out either: all of it comes back to the box, in order.
                    let unsent = [text] + (self.outbox[id] ?? [])
                    self.outbox[id] = nil
                    self.modelHint[id] = nil
                    self.effortHint[id] = nil
                    self.pending[id] = nil
                    self.errors[id] = failure.localizedDescription
                    let draft = self.drafts[id] ?? ""
                    self.drafts[id] = (unsent + (draft.isEmpty ? [] : [draft])).joined(separator: "\n\n")
                    // A question or permission Chat can't show is open there: show it where it can be answered.
                    if case ReplySender.Failure.dialog = failure, self.selection == id, self.terminal == nil {
                        self.openTerminal(typing: nil)
                    }
                    return
                }
                self.pending[id]?.sending = false
                self.refresh()
                self.sendNext(id, job: jobID)
            }
        }
    }

    /// Sends the oldest waiting reply. A menu command opens the terminal sheet instead (if its session
    /// is still the one shown), and the rest wait until the sheet closes.
    private func sendNext(_ id: String, job jobID: String) {
        guard terminal == nil, pending[id]?.sending != true, !takingBack.contains(id),
              var waiting = outbox[id], !waiting.isEmpty else { return }
        let next = waiting.removeFirst()
        outbox[id] = waiting.isEmpty ? nil : waiting
        if SlashCommands.opensMenu(next), selection == id {
            openTerminal(typing: next)
        } else {
            deliver(next, to: id, job: jobID, exact: true)
        }
    }

    /// Attaches the selected background session in the terminal sheet and runs the command there,
    /// where its menu can show (a hidden attach closes before the menu draws).
    func openTerminal(typing command: String?) {
        guard let agent = selected, case .job(let jobID) = agent.target else { return }
        terminal?.close()
        terminal = TerminalSession(jobID: jobID, title: agent.name, command: command)
    }

    /// Runs a command from a message (a code block's Run) in the terminal sheet, in `folder` or the
    /// session's folder. Ours, not the session's: a long render outlasts a `!` command's time limit.
    func runCommand(_ script: String, in folder: String?) {
        guard let agent = selected else { return }
        let dir = folder ?? (agent.cwd.isEmpty ? FileManager.default.homeDirectoryForCurrentUser.path : agent.cwd)
        let jobID: String = { if case .job(let id) = agent.target { return id }; return "" }()
        terminal?.close()
        terminal = TerminalSession(run: script, in: dir, jobID: jobID)
    }

    func closeTerminal() {
        let jobID = terminal?.jobID
        terminal?.close()
        terminal = nil
        refresh()
        // Replies typed while the sheet was up go out now.
        if let jobID, let id = notch.agents.first(where: { if case .job(jobID) = $0.target { return true }; return false })?.id {
            sendNext(id, job: jobID)
        }
    }

    func isSending(_ id: String) -> Bool { pending[id]?.sending == true }

    /// Commands matching a draft like "/sd:ap"; empty once there's a space after the command.
    func completions(for agent: Agent) -> [SlashCommand] {
        let draft = drafts[agent.id] ?? ""
        guard draft.hasPrefix("/"), !draft.contains(where: \.isWhitespace) else { return [] }
        if case .codex = agent.target {
            return Array(SlashCommands.matches(String(draft.dropFirst()), in: CodexCommands.available()).prefix(8))
        }
        if case .antigravity(let session) = agent.target {
            return Array(SlashCommands.matches(String(draft.dropFirst()), in: AntigravityRunner.shared.commands(session)).prefix(8))
        }
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
        if keyCode == 53, panelOpen, !(composerFocused && selected.map { !completions(for: $0).isEmpty } == true) {
            withAnimation(KitMotion.appear) { page = nil; file = nil }
            return true
        }
        // Then it leaves a subagent for its session.
        if keyCode == 53, subagentID != nil, !composerFocused {
            subagentID = nil
            return true
        }
        // Then it stops a working session's turn, as in the terminal (not while a command list is open).
        if keyCode == 53, terminal == nil, !accountSheet, !newSessionSheet, !vaultSheet, let agent = selected, canStop(agent),
           completions(for: agent).isEmpty {
            stopTurn()
            return true
        }
        guard composerFocused, let agent = selected, agent.canType else { return false }
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
                .part("Sidebar", .trailing)
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
                                               message: "Sessions show up here when you run claude in a terminal or start a background session.",
                                               action: newSessionButton)
                                } else {
                                    EmptyState(pose: "wave-3", title: "Pick a Session",
                                               message: "Choose one on the left to read its conversation, or start a new one.",
                                               action: newSessionButton)
                                }
                            }
                            .frame(maxWidth: .infinity, maxHeight: .infinity)
                            if let page = model.page {
                                PagePanel(model: model, page: page, maxWidth: max(320, geo.size.width - 280)).part("Page Panel", .topTrailing)
                                    .transition(.move(edge: .trailing))
                            } else if let file = model.file {
                                FilePanel(model: model, url: file, maxWidth: max(320, geo.size.width - 280)).part("File Panel", .topTrailing)
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
        .sheet(item: $model.terminal) { session in
            TerminalSheet(session: session) { model.closeTerminal() }
        }
        .background(EmptyView().sheet(isPresented: $model.accountSheet) {
            AccountsSheet(model: model)
        })
        .background(EmptyView().sheet(isPresented: $model.newSessionSheet) {
            NewSessionSheet(model: model)
        })
        .background(EmptyView().sheet(isPresented: $model.vaultSheet) {
            VaultSheet { model.vaultSheet = false }
        })
        .environment(\.openURL, OpenURLAction { model.open($0) })
        .environment(\.chatFolder, model.selected?.cwd)
        .environment(\.runCommand, { [weak model] script, folder in model?.runCommand(script, in: folder) })
        .kitNoFocusRing()
        .frame(minWidth: 720, minHeight: 460)
    }

    private var newSessionButton: AnyView {
        AnyView(Button { model.newSessionSheet = true } label: {
            Label("New Session", systemImage: "plus")
        }
        .buttonStyle(.kit(.primary))
        .help("New session: pick a Claude Code or Codex account and a folder")
        .part("New Session Button", .bottom))
    }
}

private struct SessionSidebar: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel
    @FocusState private var listFocused: Bool
    @State private var hoveredRow: String?
    @FocusState private var searchFocused: Bool

    var body: some View {
        let shown = model.shown
        VStack(alignment: .leading, spacing: 0) {
            // The traffic lights live here.
            Color.clear.frame(height: KitMetric.titleBar)
            HStack(alignment: .firstTextBaseline) {
                Text("Sessions").kitType(.title).foregroundColor(KitInk.text)
                Spacer()
                Button { model.newSessionSheet = true } label: {
                    Image(systemName: "plus").font(.system(size: 12, weight: .semibold))
                }
                .buttonStyle(.kit(.ghost))
                .help("New session: pick a Claude Code or Codex account and a folder")
                .part("New Session Button", .bottomTrailing)
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
                .part("Session Search", .trailing)
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
                                .part("Group Heading", .trailing)
                            ForEach(agents, id: \.id) { agent in
                                let selected = model.selection == agent.id
                                let showsMenu = hoveredRow == agent.id || selected
                                Button { model.selection = agent.id; listFocused = true } label: {
                                    SessionRowLabel(agent: agent, line: statusLine(agent),
                                                    account: model.account(for: agent),
                                                    showsMenu: showsMenu)
                                }
                                .buttonStyle(KitSessionRowStyle(selected: selected, focused: listFocused))
                                .accessibilityAddTraits(selected ? .isSelected : [])
                                .part("Session Row", .trailing)
                                .onHover { inside in
                                    if inside { hoveredRow = agent.id } else if hoveredRow == agent.id { hoveredRow = nil }
                                }
                                .overlay(alignment: .trailing) {
                                    if showsMenu {
                                        Menu { rowActions(agent) } label: {
                                            KitIconView(icon: .more, size: 13).foregroundColor(KitInk.textMuted)
                                        }
                                        .menuStyle(.borderlessButton)
                                        .menuIndicator(.hidden)
                                        .fixedSize()
                                        .padding(.trailing, 10)
                                        .help("Session Actions")
                                        .accessibilityLabel("Session Actions")
                                        .pointerCursor()
                                        .part("Session Menu", .trailing)
                                    }
                                }
                                // The notch's actions: Hide drops it from the lists until it changes state;
                                // Remove stops a background session and deletes it (`claude rm`), after asking.
                                .contextMenu { rowActions(agent) }
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
        .safeAreaInset(edge: .bottom, spacing: 0) {
            // The ? button: names every part on screen, here and in the notch.
            HStack {
                KitPartsButton()
                Spacer()
                Button { model.vaultSheet = true } label: {
                    Image(systemName: "key").font(.system(size: 13, weight: .medium)).foregroundColor(KitInk.textMuted)
                }
                .buttonStyle(.kit(.ghost))
                .help("Secret Vault: passwords and tokens agents can use without seeing them")
                .accessibilityLabel("Secret Vault")
                .part("Secret Vault Button", .top)
            }
                .padding(.horizontal, 10).padding(.vertical, 8)
        }
        .onChange(of: model.sidebarFocusRequest) { _ in listFocused = true }
    }

    /// One list for the right-click menu and the ⋯ menu, so they never differ.
    @ViewBuilder private func rowActions(_ agent: Agent) -> some View {
        if ChatTakeover.offersTakeover(agent) {
            Button("Take Over…") { ChatTakeover.run(agent) { model.selection = $0 } }
            Divider()
        }
        if !agent.cwd.isEmpty, FileManager.default.fileExists(atPath: agent.cwd) {
            Button("Reveal in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: agent.cwd)]) }
        }
        if let id = agent.sessionID {
            Button("Copy Session ID") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(id, forType: .string)
            }
        }
        Divider()
        switch agent.target {
        case .codex, .antigravity:
            Button(agent.target.isDraft ? "Discard New Session" : "Close Session") { model.closeSession(agent) }
        default:
            Button("Hide") { notch.onHide?(agent) }
        }
        if case .job = agent.target {
            Divider()
            Button("Remove Background Session…") { notch.onRemove?(agent) }
        }
        if case .codex = agent.target {
            Divider()
            Button("Delete Codex Session…") { model.removeCodex(agent) }
        }
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
    var account: ChatModel.SessionAccount?
    /// The ⋯ menu sits where the age is, so the age hides but keeps its space and the row doesn't jump.
    var showsMenu = false

    var body: some View {
        HStack(spacing: 10) {
            KitAvatar(name: agent.name, size: 30, status: agent.group)
            VStack(alignment: .leading, spacing: 2) {
                // Three lines: who, on which account, doing what.
                HStack(spacing: 5) {
                    ToolIcon(tool: SessionTool(agent.target), size: 14).part("Tool Icon", .leading)
                    Text(agent.name).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                }
                if let account {
                    HStack(spacing: 4) {
                        Image(systemName: account.lane == nil ? "person.crop.circle" : "arrow.triangle.branch")
                            .font(.system(size: 9, weight: .semibold))
                        Text(account.email).lineLimit(1).truncationMode(.middle)
                    }
                    .kitType(.meta).foregroundColor(KitInk.textFaint)
                }
                Text(line).kitType(.meta).foregroundColor(agent.group == .needsInput ? KitInk.accentText : KitInk.textMuted).lineLimit(1)
            }
            Spacer(minLength: 4)
            Text(ChatFormat.age(agent.since)).kitType(.meta).foregroundColor(KitInk.textFaint)
                .opacity(showsMenu ? 0 : 1)
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
                    if let sub = model.viewedSubagent {
                        SubagentBanner(subagent: sub) { model.subagentID = nil }.part("Subagent Banner", .bottom)
                        if let conversation = model.subConversation {
                            ConversationView(conversation: conversation, pending: nil, live: sub.isRunning,
                                             expansion: model.expansion, pageOpen: model.panelOpen)
                                .equatable()
                                .id(sub.id)
                                .part("Subagent Conversation", .bottom)
                        } else {
                            ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
                        }
                    } else if let command = agent.command {
                        CommandJobView(agent: agent, command: command).part("Command Job", .top)
                    } else if model.conversation.items.isEmpty && model.pending[agent.id] == nil {
                        EmptyState(pose: agent.group == .working ? "look-2" : "sit-1", title: "No Messages Yet",
                                   message: agent.group == .working ? "The session is starting." : "Nothing in this session's transcript yet.")
                    } else {
                        ConversationView(conversation: model.conversation,
                                         pending: model.pending[agent.id],
                                         outbox: model.outbox[agent.id] ?? [],
                                         live: agent.group == .working,
                                         expansion: model.expansion,
                                         pageOpen: model.panelOpen,
                                         onEditLatest: { [weak model] in model?.editLatestQueued() })
                            .equatable()
                            // A fresh view per session, so switching back opens at the newest message.
                            .id(agent.id)
                            .part("Conversation", .bottom)
                    }
                    if model.viewedSubagent == nil { bottom }
                }
                // Files and images dropped anywhere on the conversation go into the reply as paths, as in
                // Claude Code's terminal; Claude reads them from there.
                .onDrop(of: ChatModel.dropTypes, isTargeted: $dropping) { providers in
                    agent.canType && model.receiveDrop(providers, for: agent.id)
                }
                if let page = model.page {
                    PagePanel(model: model, page: page, maxWidth: max(320, geo.size.width - 380)).part("Page Panel", .topTrailing)
                        .transition(.move(edge: .trailing))
                } else if let file = model.file {
                    FilePanel(model: model, url: file, maxWidth: max(320, geo.size.width - 380)).part("File Panel", .topTrailing)
                        .transition(.move(edge: .trailing))
                } else if model.showInspector {
                    Rectangle().fill(KitInk.hairline).frame(width: 1)
                    SessionInspector(agent: agent, conversation: model.conversation,
                                     subagents: model.subagents, viewing: model.subagentID,
                                     openSubagent: { id in model.subagentID = id })
                        .part("Details Panel", .topTrailing)
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
                .part("Avatar", .bottomLeading)
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    ToolIcon(tool: SessionTool(agent.target), size: 18).part("Tool Icon", .leading)
                    Text(agent.name).kitType(.title).foregroundColor(KitInk.text).lineLimit(1)
                        .part("Session Name", .topTrailing)
                }
                HStack(spacing: 8) {
                    Text([agent.project, agent.host].filter { !$0.isEmpty }.joined(separator: " · "))
                        .kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                        .layoutPriority(1)
                        .part("Project Line", .bottomLeading)
                    if let account = model.account(for: agent) {
                        AccountMenu(model: model, agent: agent, account: account)
                            .part("Account Menu", .bottomLeading)
                    } else if case .codex = agent.target, let email = model.codex?.active?.email {
                        // Codex has one login: every Codex session runs on it.
                        Button { model.addAccount(.codex) } label: {
                            HStack(spacing: 4) {
                                Image(systemName: "person.crop.circle").font(.system(size: 10, weight: .semibold))
                                Text(email).lineLimit(1).truncationMode(.middle)
                                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
                            }
                            .kitType(.meta).foregroundColor(KitInk.textMuted)
                        }
                        .buttonStyle(.kitPlain)
                        .help("The Codex account. Click to switch or add one.")
                        .part("Codex Account Button", .bottomLeading)
                    }
                }
            }
            Spacer(minLength: 8)
            SessionMeters(context: model.conversation.facts.contextTokens,
                          window: ChatFormat.contextWindow(model: model.conversation.facts.model),
                          limits: model.limits(for: agent))
            // Icon only, so the line under the title has the room.
            Button { AgentActions.open(agent.target, cwd: agent.cwd) } label: {
                KitIconView(icon: ChatFormat.openIcon(agent), size: 13)
            }
            .buttonStyle(.kit(.ghost))
            .help("\(ChatFormat.openTitle(agent)): \(ChatFormat.openHelp(agent))")
            .accessibilityLabel(ChatFormat.openTitle(agent))
            .part("Open Button", .top)
            Button { withAnimation(KitMotion.appear) { model.showInspector.toggle() } } label: {
                Image(systemName: "sidebar.right").font(.system(size: 13, weight: .medium))
            }
            .buttonStyle(.kit(model.showInspector ? .secondary : .ghost))
            .help(model.showInspector ? "Hide the session details" : "Show the session details: branch, model, tokens, tasks, files changed")
            .part("Details Button", .bottomTrailing)
            PhoneButton(bridge: PhoneService.shared.bridge)
        }
        .padding(.horizontal, 16)
        .padding(.top, 14).padding(.bottom, 12)
        .frame(minHeight: 64)
        .part("Header", .bottom)
    }

    @ViewBuilder private var bottom: some View {
        VStack(spacing: 8) {
        if let error = model.accountErrors[agent.id] {
            AccountBanner(text: error, isError: true) { model.accountErrors[agent.id] = nil }.part("Account Banner", .trailing)
        } else if let note = model.accountNote(for: agent) {
            AccountBanner(text: note.text, isError: false) { model.dismissAccountNote(for: agent) }.part("Account Banner", .trailing)
        }
        Group {
            if case .antigravity(let session) = agent.target, let request = AntigravityRunner.shared.permission(session) {
                AntigravityPermissionCard(session: session, request: request).part("Permission Card", .topTrailing)
            } else if let request = notch.permission(agent.id) {
                ChatPermissionCard(notch: notch, agent: agent, request: request).part("Permission Card", .topTrailing)
            } else if let question = notch.question(agent.id) {
                ChatQuestionCard(notch: notch, agent: agent, question: question).part("Question Card", .topTrailing)
            } else {
                switch agent.target {
                case .job where agent.command != nil:
                    Notice(text: "This background job runs a command, not a conversation, so it can't take replies.")
                        .part("Read-Only Notice", .trailing)
                case .job, .codex, .antigravity:
                    Composer(model: model, agent: agent, dropping: dropping)
                case .process where agent.repliesViaHost:
                    Composer(model: model, agent: agent, dropping: dropping)
                case .process:
                    Notice(text: agent.host == "claude -p"
                           ? "This session runs headless (claude -p) and can't take replies."
                           : "Replies to this session go to its terminal. The terminal button at the top right brings it forward.")
                        .part("Read-Only Notice", .trailing)
                case .resume:
                    Notice(text: "This session has ended. Resume it in a terminal to keep going.").part("Read-Only Notice", .trailing)
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
    /// Replies waiting for the one before to go out.
    var outbox: [String] = []
    /// The session is working: its newest tool step stays open and a live row shows what it's doing.
    let live: Bool
    let expansion: ChatExpansion
    var pageOpen = false
    /// Takes the newest queued or waiting message back to the reply box (not part of ==).
    var onEditLatest: (() -> Void)?
    /// Following the newest message: true until you scroll up, and again once you're back at the bottom.
    /// Only your own scrolling changes it (`UserScrollWatcher`), so content growing below can't be
    /// mistaken for a scroll and pull you back down while you read.
    @State private var atBottom = true
    /// While our own animated jump runs, don't also snap to the end.
    @State private var jumpingUntil = Date.distantPast
    /// Spaces out the follow-the-end corrections. A class, so setting it doesn't redraw.
    @State private var corrections = CorrectionClock()
    /// How many of the newest rows are drawn; Show Earlier Messages adds a page.
    @State private var shown = ConversationView.page
    /// Rows per page. A plain VStack, not a LazyVStack: lazy rows are measured as they scroll in, so
    /// their estimated heights kept changing under the follow-the-end scroll and SwiftUI re-measured
    /// forever inside one update (Notchling at 100% CPU, 5 Nov). Drawn rows have exact heights.
    static let page = 80

    static func == (a: Self, b: Self) -> Bool {
        a.conversation == b.conversation && a.pending == b.pending && a.outbox == b.outbox && a.live == b.live
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
        let allRows = self.rows
        let rows = Array(allRows.suffix(shown))
        let hidden = allRows.count - rows.count
        let lastToolID = conversation.items.last(where: { if case .tools = $0.kind { return true } else { return false } })?.id
        let lastID = conversation.items.last?.id
        // "Working" can outlast the turn: a background command (status "shell") keeps a session busy
        // after its reply. Then the row says so instead of "Thinking…" with a clock that never stops.
        let turnOver: Bool = { if case .turnEnd = conversation.items.last?.kind { return true } else { return false } }()
        VStack(spacing: 0) {
        if !conversation.tasks.isEmpty {
            TaskPanel(tasks: conversation.tasks, expansion: expansion).part("Task Panel", .topTrailing)
                        .padding(.horizontal, 16).padding(.top, 12)
                .frame(maxWidth: .infinity)
        }
        ScrollViewReader { proxy in
            GeometryReader { outer in
                ZStack(alignment: .bottomTrailing) {
                    ScrollView {
                        VStack(alignment: .leading, spacing: 14) {
                            if hidden > 0 {
                                Button("Show Earlier Messages") {
                                    // Keep the row you were reading in place as the page above it appears.
                                    let anchor = rows.first?.id
                                    shown += ConversationView.page
                                    if let anchor { DispatchQueue.main.async { proxy.scrollTo(anchor, anchor: .top) } }
                                }
                                .buttonStyle(.kit(.secondary))
                                .help("\(hidden) earlier")
                                .frame(maxWidth: .infinity)
                                .part("Show Earlier Button", .trailing)
                            }
                            ForEach(rows) { row in
                                Group {
                                    if let day = row.day {
                                        DaySeparator(date: day).part("Day Separator", .trailing)
                                    } else if let item = row.item {
                                        itemView(item, liveTools: live && !turnOver && item.id == lastToolID && item.id == lastID)
                                    }
                                }
                                .id(row.id)
                            }
                            // Only while it works: a queue left by a session that stopped never runs.
                            // The newest of these can be taken back to edit: the last waiting one, else the
                            // last queued one while nothing is on its way.
                            let sending = pending.map { !$0.landed(in: conversation) && $0.sending } ?? false
                            let editQueued = live && outbox.isEmpty && !sending
                            if live {
                                let queued = conversation.queued
                                ForEach(Array(queued.enumerated()), id: \.offset) { i, text in
                                    UserBubble(text: text, images: 0, date: nil, note: "Queued, sends when this turn ends",
                                               onEdit: editQueued && i == queued.count - 1 ? onEditLatest : nil)
                                        .part("Queued Bubble", .topTrailing)
                                }
                            }
                            if let pending, !pending.landed(in: conversation) {
                                UserBubble(text: pending.text, images: 0, date: nil, note: pending.sending ? "Sending…" : "Sent")
                                    .part("Sending Bubble", .topTrailing)
                                    .transition(.opacity.combined(with: .move(edge: .bottom)))
                            }
                            ForEach(Array(outbox.enumerated()), id: \.offset) { i, text in
                                UserBubble(text: text, images: 0, date: nil, note: "Waiting to send",
                                           onEdit: i == outbox.count - 1 ? onEditLatest : nil)
                                    .part("Waiting Bubble", .topTrailing)
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
                        // At most one correction per 0.3 s: lazy rows are measured as they appear, so each scroll
                        // can move the end again, and an unbounded chain pins the main thread (the hangs of 4 Nov).
                        if atBottom && !near && Date() >= jumpingUntil {
                            corrections.run { if atBottom { proxy.scrollTo("end", anchor: .bottom) } }
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
            .onChange(of: outbox.count) { _ in jump(proxy) }
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
                .part(text.hasPrefix("! ") ? "Shell Command Bubble" : "Your Message", .topTrailing)
        case .assistant(let text):
            AssistantMessage(text: text, date: item.date).part("Claude's Reply")
        case .thinking(let seconds, let text):
            ThinkingRow(id: item.id, seconds: seconds, text: text, expansion: expansion).part("Thinking Row", .trailing)
        case .tools(let calls):
            ToolStepView(id: item.id, calls: calls, live: liveTools, expansion: expansion).part("Tool Steps", .topTrailing)
        case .turnEnd(let seconds, let model):
            TurnFooter(seconds: seconds, model: model).part("Turn Footer", .trailing)
        case .notice(let text, let symbol):
            NoticeRow(text: text, symbol: symbol).part("Notice", .leading)
        case .commandOutput(let text):
            CommandOutputView(text: text).part("Command Output", .topTrailing)
        }
    }
}

/// Runs at most one scroll correction per interval; a request that comes too soon waits for the
/// next slot (one waiting at a time), so the view still ends at the newest message.
private final class CorrectionClock {
    private var next = Date.distantPast
    private var waiting = false

    func run(_ action: @escaping () -> Void) {
        let now = Date()
        if now >= next {
            next = now.addingTimeInterval(0.3)
            DispatchQueue.main.async(execute: action)
        } else if !waiting {
            waiting = true
            DispatchQueue.main.asyncAfter(deadline: .now() + next.timeIntervalSince(now)) { [self] in
                waiting = false
                next = Date().addingTimeInterval(0.3)
                action()
            }
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
    var onEdit: (() -> Void)? = nil
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
                    HStack(spacing: 8) {
                        Text(note).kitType(.meta).foregroundColor(KitInk.textFaint)
                        if let onEdit {
                            Button("Edit", action: onEdit)
                                .buttonStyle(.kitPlain)
                                .kitType(.meta).foregroundColor(KitInk.accentText)
                                .help("Take it back to the reply box to change it (↑). Send it again when it's right.")
                                .part("Edit Queued Button", .top)
                        }
                    }
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

/// The session's model, and a menu to change it: picking one types `/model <alias>` into the session,
/// as you would in the terminal. Full model ids, so the menu names exactly what you get.
/// The session's effort, switched by sending `/effort <level>` as in the terminal (Claude Code also
/// keeps it as the default for new sessions). The current level is the one on its latest reply.
struct EffortPicker: View {
    @ObservedObject var model: ChatModel
    let current: String?

    static let levels = ["low", "medium", "high", "xhigh", "max"]

    static func title(_ level: String) -> String {
        level == "xhigh" ? "Extra High" : level.prefix(1).uppercased() + level.dropFirst()
    }

    var body: some View {
        Button { Self.popUp(model: model, current: current) } label: {
            HStack(spacing: 4) {
                Image(systemName: "gauge.with.dots.needle.50percent").font(.system(size: 9, weight: .semibold))
                Text(current.map(Self.title) ?? "Effort")
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help("How hard the model thinks. Pick another to switch it (like /effort in the terminal, which also makes it the default for new sessions).")
    }

    static func popUp(model: ChatModel, current: String?) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        for level in levels {
            let row = ActionMenuItem(title(level)) { [model] in model.send("/effort \(level)") }
            row.state = current == level ? .on : .off
            menu.addItem(row)
        }
        let at = NSEvent.mouseLocation
        menu.popUp(positioning: nil, at: at, in: nil)
    }
}

struct ModelPicker: View {
    @ObservedObject var model: ChatModel
    let current: String?

    /// The current models, then older ones under More Models (Claude Code's own /model list).
    static let choices = ["claude-opus-5-5", "claude-fable-5-1", "claude-sonnet-5-5", "claude-haiku-5-5"]
    static let older = ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-5", "claude-fable-5",
                        "claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6"]

    var body: some View {
        Button { Self.popUp(model: model, current: current) } label: {
            HStack(spacing: 4) {
                Image(systemName: "cpu").font(.system(size: 9, weight: .semibold))
                Text(current.map(ChatFormat.modelName) ?? "Model")
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help("The model this session uses. Pick another to switch it (like /model in the terminal).")
    }

    /// Also what typing /model in the reply box opens, instead of sending it to a session that can't show a picker.
    static func popUp(model: ChatModel, current: String?) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let shown = current.map(ChatFormat.modelName)
        func item(_ id: String) -> NSMenuItem {
            let name = ChatFormat.modelName(id)
            let row = ActionMenuItem(name) { [model] in model.send("/model \(id)") }
            row.state = shown == name ? .on : .off
            return row
        }
        Self.choices.forEach { menu.addItem(item($0)) }
        menu.addItem(.separator())
        let more = NSMenuItem(title: "More Models", action: nil, keyEquivalent: "")
        let sub = NSMenu()
        sub.autoenablesItems = false
        Self.older.forEach { sub.addItem(item($0)) }
        more.submenu = sub
        menu.addItem(more)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

/// A Codex session's model and effort, for its next turns: Codex's own list (`CodexModels`), each
/// model's efforts in a submenu, and Default to go back to Codex's setting.
struct CodexModelPicker: View {
    @ObservedObject var model: ChatModel
    let agent: Agent

    private var pick: CodexPick { model.codexPicks[agent.id] ?? CodexPick() }
    private var models: [CodexModel] { CodexModels.load() }

    /// What the next turn uses: the pick, else what the last turn ran with.
    private var label: String {
        let slug = pick.model ?? model.conversation.facts.model
        let name = slug.map { s in models.first { $0.slug == s }?.name ?? s } ?? "Default Model"
        let effort = pick.effort ?? model.conversation.facts.effort
        return ["Codex", name, effort].compactMap { $0 }.joined(separator: " · ")
    }

    var body: some View {
        Button(action: popUp) {
            HStack(spacing: 4) {
                Image(systemName: "cpu").font(.system(size: 9, weight: .semibold))
                Text(label)
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help("The model and reasoning effort for this session's next turns.")
    }

    func popUp() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let current = pick
        let id = agent.id
        let running = model.conversation.facts.model
        for m in models {
            let item = ActionMenuItem(m.name) { [model] in
                // A new model starts at its own default effort unless one was picked.
                model.setCodexPick(CodexPick(model: m.slug, effort: current.effort.flatMap { m.efforts.contains($0) ? $0 : nil }), for: id)
            }
            item.toolTip = m.summary
            item.state = (current.model ?? running) == m.slug ? .on : .off
            if !m.efforts.isEmpty {
                let sub = NSMenu()
                sub.autoenablesItems = false
                for effort in m.efforts {
                    let title = effort.prefix(1).uppercased() + effort.dropFirst() + (effort == m.defaultEffort ? " (Default)" : "")
                    let row = ActionMenuItem(title) { [model] in model.setCodexPick(CodexPick(model: m.slug, effort: effort), for: id) }
                    row.state = (current.model ?? running) == m.slug && (current.effort ?? m.defaultEffort) == effort ? .on : .off
                    sub.addItem(row)
                }
                let pickModel = ActionMenuItem("Use \(m.name)") { [model] in
                    model.setCodexPick(CodexPick(model: m.slug, effort: nil), for: id)
                }
                sub.insertItem(pickModel, at: 0)
                sub.insertItem(.separator(), at: 1)
                item.submenu = sub
            }
            menu.addItem(item)
        }
        if models.isEmpty {
            let none = NSMenuItem(title: "Codex hasn't listed its models yet", action: nil, keyEquivalent: "")
            none.isEnabled = false
            menu.addItem(none)
        }
        menu.addItem(.separator())
        let reset = ActionMenuItem("Codex Default") { [model] in model.setCodexPick(CodexPick(), for: id) }
        reset.state = current == CodexPick() ? .on : .off
        menu.addItem(reset)
        let note = NSMenuItem(title: "Applies from the next turn.", action: nil, keyEquivalent: "")
        note.isEnabled = false
        menu.addItem(note)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

private struct Composer: View {
    @ObservedObject var model: ChatModel
    let agent: Agent
    /// A file is being dragged over the conversation (the drop target is `SessionDetail`).
    let dropping: Bool
    @FocusState private var focused: Bool
    @State private var focusedBeforeDrag = false

    private var placeholder: String {
        if case .codex(let session) = agent.target {
            return (session.isEmpty ? "What should Codex do in \(agent.project)?" : "Reply to \(agent.name)") + "   ⌥ Return new line"
        }
        return "Reply to \(agent.name), or / for commands…   ⌥ Return new line"
    }

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
                .part("Suggested Reply", .trailing)
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
                            .buttonStyle(.kitPlain)
                            .pointerCursor()
                        }
                    }
                    .padding(6)
                    .part("Command List", .topTrailing)
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
                    .part("Shell Header", .trailing)
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
                        .part("Will Run Box", .trailing)
                    }
                }
                HStack(alignment: .bottom, spacing: 10) {
                    TextField(showsChip || model.suggestion(for: agent) == nil ? placeholder
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
                            if sending && empty {
                                ProgressView().controlSize(.small)
                            } else {
                                KitIconView(icon: .send, size: 13)
                                    .foregroundColor(empty ? KitInk.textFaint : shell ? KitInk.onShell : KitInk.onAccent)
                            }
                        }
                        .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.kitPlain)
                    // Still works while one goes out: the next waits its turn.
                    .disabled(empty)
                    .help(shell ? "Run the command in the session (Return)" : "Send the reply (Return)")
                    .accessibilityLabel(shell ? "Run" : "Send")
                    .pointerCursor()
                    .part("Send Button", .top)
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
            .part("Composer", .bottomLeading)
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
                HStack(spacing: 8) {
                    if case .codex = agent.target {
                        CodexModelPicker(model: model, agent: agent).part("Codex Model Picker", .top)
                    } else if case .antigravity(let session) = agent.target {
                        AntigravityOptionsPicker(model: model, agent: agent, session: session).part("Antigravity Options", .top)
                    } else {
                        ModelPicker(model: model, current: model.conversation.facts.model).part("Model Picker", .top)
                        EffortPicker(model: model, current: model.conversation.facts.effort).part("Effort Picker", .top)
                    }
                    if model.canStop(agent) {
                        Spacer(minLength: 0)
                        let stopping = model.isStopping(agent)
                        Button { model.stopTurn() } label: {
                            HStack(spacing: 4) {
                                Image(systemName: "stop.fill").font(.system(size: 8))
                                Text(stopping ? "Stopping…" : "Stop")
                                if !stopping { Text("esc").foregroundColor(KitInk.textFaint) }
                            }
                            .kitType(.meta)
                        }
                        .buttonStyle(.kit(.ghost))
                        .disabled(stopping)
                        .help("Stop this turn, like Esc in the terminal. What it did so far stays.")
                        .part("Stop Button", .top)
                    }
                    if shell {
                        Text("Return runs it like ! in the terminal · delete the ! to send a message instead")
                            .kitType(.meta).foregroundColor(KitInk.textFaint)
                            .lineLimit(1)
                            .frame(maxWidth: .infinity)
                            .part("Composer Hint", .trailing)
                    }
                }
                .padding(.leading, 6)
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
    @State private var picked: Set<Int> = []
    @FocusState private var typing: Bool

    var body: some View {
        let index = min(notch.choices.count, max(question.items.count - 1, 0))
        let item = question.items.isEmpty ? nil : question.items[index]
        let canAnswer = notch.canAnswer(agent, question, multiSelect: true)
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
                if canAnswer, item.multiSelect {
                    Text("Select all that apply").kitType(.meta).foregroundColor(KitInk.textMuted)
                    ForEach(Array(item.options.enumerated()), id: \.offset) { i, option in
                        Button {
                            if picked.contains(i + 1) { picked.remove(i + 1) } else { picked.insert(i + 1) }
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(option).kitType(.heading).foregroundColor(KitInk.text)
                                if i < item.details.count, !item.details[i].isEmpty {
                                    Text(item.details[i]).kitType(.meta).foregroundColor(KitInk.textMuted)
                                }
                            }
                        }
                        .buttonStyle(KitOptionStyle(number: i + 1, selected: picked.contains(i + 1)))
                        .disabled(notch.sending)
                    }
                    HStack {
                        Spacer()
                        Button(index == question.items.count - 1 ? "Submit" : "Next") {
                            let numbers = picked.sorted()
                            picked = []
                            notch.choose(.options(numbers), for: agent.id)
                        }
                        .buttonStyle(.kit(.primary))
                        .disabled(picked.isEmpty || notch.sending)
                    }
                } else if canAnswer {
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
                        Text("Answer this one in the session's terminal.")
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
        .onChange(of: index) { _ in picked = [] }
        // SwiftUI can keep this card's state when another session's question takes its place.
        .onChange(of: question.toolUseID) { _ in picked = [] }
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

/// A background job that runs a shell command: what it runs, how it ended, and the end of its output file.
/// Claude Code keeps no transcript for it; sessions it starts (`claude -p`) have their own.
private struct CommandJobView: View {
    let agent: Agent
    let command: String
    @State private var output: String?

    private var outputPath: String? { CommandJob.outputPath(in: command, cwd: agent.cwd) }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                KitIconView(icon: .terminal, size: 13).foregroundColor(KitInk.textMuted)
                Text("Command Job").kitType(.heading).foregroundColor(KitInk.text)
                Text(status).kitType(.meta).foregroundColor(agent.label == "Failed" ? KitInk.needs : KitInk.textMuted)
            }
            Text("It runs a shell command instead of a conversation, so there's no chat history. Sessions it starts with claude -p keep their own transcripts.")
                .kitType(.body).foregroundColor(KitInk.textMuted).fixedSize(horizontal: false, vertical: true)
            box(CommandJob.readable(command)).frame(maxHeight: 110)
            if let path = outputPath {
                HStack(spacing: 6) {
                    Text("Output").kitType(.section).foregroundColor(KitInk.textFaint)
                    Text((path as NSString).lastPathComponent).kitType(.meta).foregroundColor(KitInk.textFaint)
                        .lineLimit(1).truncationMode(.middle).help(path)
                }
                .padding(.top, 4)
                ScrollViewReader { proxy in
                    box(output ?? "The output file isn't there.", id: "output-end")
                        .onChange(of: output) { _ in proxy.scrollTo("output-end", anchor: .bottom) }
                        .onAppear { DispatchQueue.main.async { proxy.scrollTo("output-end", anchor: .bottom) } }
                }
                .part("Command Output", .top)
            }
            Spacer(minLength: 0)
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        // Read again every few seconds: a running command keeps writing.
        .task(id: outputPath) {
            while !Task.isCancelled {
                let path = outputPath
                let text = await Task.detached { path.flatMap { CommandJob.tail($0) } }.value
                if text != output { output = text }
                try? await Task.sleep(nanoseconds: 3_000_000_000)
            }
        }
    }

    private func box(_ text: String, id: String? = nil) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                Text(text).kitType(.mono).foregroundColor(KitInk.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
                Color.clear.frame(height: 1).id(id ?? "")
            }
        }
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd).fill(KitInk.raised))
    }

    private var status: String {
        let state = [agent.label, agent.detail].compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: " · ")
        return state + " · " + ChatFormat.age(agent.since)
    }
}

private struct EmptyState: View {
    let pose: String
    let title: String
    let message: String
    var action: AnyView? = nil

    var body: some View {
        VStack(spacing: 10) {
            KitPet(pose: pose, size: 120)
            Text(title).kitType(.title).foregroundColor(KitInk.text)
            Text(message).kitType(.body).foregroundColor(KitInk.textMuted)
                .multilineTextAlignment(.center).frame(maxWidth: 320)
            if let action { action.padding(.top, 6) }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .padding(20)
    }
}

extension Notification.Name {
    /// A background session was removed (`claude rm`); the object is its agent id.
    static let sessionRemoved = Notification.Name("NotchlingSessionRemoved")
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
        case .codex, .antigravity: return "Open in Terminal"
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
        case .codex: return "Continue this Codex session in a terminal with codex resume"
        case .antigravity: return "Continue this Antigravity session in a terminal with agy --conversation"
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

    /// "12.4k", "1.2M", "400k".
    static func tokens(_ n: Int) -> String {
        func short(_ v: Double, _ unit: String) -> String {
            var s = String(format: "%.1f", v)
            if s.hasSuffix(".0") { s.removeLast(2) }
            return s + unit
        }
        if n >= 1_000_000 { return short(Double(n) / 1_000_000, "M") }
        if n >= 1000 { return short(Double(n) / 1000, "k") }
        return "\(n)"
    }

    /// The model's context window in Claude Code, where it's known (Opus 5.5's /context says 400k).
    /// Others give nil, and the header shows the count alone rather than guess a percentage.
    static func contextWindow(model: String?) -> Int? {
        guard let model else { return nil }
        if model.hasPrefix("claude-opus-5-5") { return 400_000 }
        return nil
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

    var body: some View {
        SidePanel(maxWidth: maxWidth) {
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
        } content: {
            PagePreview(page: page)
        }
    }
}

/// The session's account in its header, with a menu to move it to another account or start a new
/// terminal session on one. A lane moves as a whole; so does the main login. The menu is AppKit's: on
/// macOS 13 a SwiftUI borderless Menu drops a custom label.
/// How full the session's context is and how much of its account's limits is used: what /context and
/// /usage would say, without typing them.
/// One limit bar in the session header.
struct SessionLimit {
    let label: String
    let name: String
    let window: UsageWindow

    static func name(_ label: String) -> String {
        switch label {
        case "5h": return "5-hour limit"
        case "Week": return "Weekly limit"
        default: return "\(label) limit"
        }
    }
}

private struct SessionMeters: View {
    let context: Int
    let window: Int?
    let limits: [SessionLimit]

    var body: some View {
        // Top-aligned so the bars line up when only the limits have a reset line under them.
        HStack(alignment: .top, spacing: 14) {
            if context > 0 {
                let fraction = window.map { Double(context) / Double($0) }
                meter("Context", width: 120,
                      window.map { "\(ChatFormat.tokens(context)) / \(ChatFormat.tokens($0))" } ?? ChatFormat.tokens(context),
                      fraction,
                      help: "Context: \(context.formatted()) tokens"
                        + (window.map { " of \($0.formatted()) (\(Int((fraction ?? 0) * 100))%). Claude Code compacts the conversation near the limit." } ?? "."))
                    .part("Context Meter", .bottom)
            }
            if !limits.isEmpty {
                // Redrawn each minute so "in 2h 13m" counts down without a new usage fetch.
                TimelineView(.everyMinute) { _ in
                    HStack(alignment: .top, spacing: 10) {
                        ForEach(limits, id: \.label) { l in
                            limit(l.label, l.window, short: l.label.hasSuffix("h"), name: l.name)
                        }
                    }
                }
                .part("Limit Meters", .bottom)
            }
        }
    }

    private func limit(_ label: String, _ window: UsageWindow, short: Bool, name: String) -> some View {
        let reset = window.resetsAt.map { AccountRowView.resetText($0, short: short) }
        return meter(label, width: 64, "\(Int(window.percent.rounded()))%", window.percent / 100, below: reset,
                     help: "\(name): \(Int(window.percent.rounded()))% used" + (reset.map { ", resets \($0)" } ?? ""))
    }

    private func meter(_ label: String, width: CGFloat, _ value: String, _ fraction: Double?, below: String? = nil,
                       help: String) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            HStack(spacing: 4) {
                Text(label).kitType(.meta).foregroundColor(KitInk.textMuted)
                Text(value).kitType(.label).foregroundColor(KitInk.text).monospacedDigit()
            }
            .lineLimit(1).fixedSize()
            if let fraction {
                let f = min(max(fraction, 0), 1)
                Capsule().fill(KitInk.raised)
                    .overlay(alignment: .leading) {
                        GeometryReader { g in
                            Capsule().fill(f >= 0.8 ? KitInk.needs : KitInk.working).frame(width: max(g.size.width * f, 3))
                        }
                    }
                    .frame(height: 3)
            }
            if let below {
                Text(below).kitType(.meta).foregroundColor(KitInk.textFaint).monospacedDigit()
                    .lineLimit(1).fixedSize()
            }
        }
        .frame(minWidth: width, alignment: .leading)
        .fixedSize()
        .help(help)
        .accessibilityElement(children: .combine)
    }
}

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
                if account.lane != nil { Text("· lane").foregroundColor(KitInk.textFaint) }
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help(account.lane.map { "This session runs in the \($0.name) lane, on \($0.email). Click to switch it." }
              ?? "This session runs on the main login. Click to switch it.")
    }

    /// One question per menu: which account this session uses. Picking another moves just this session;
    /// where it then runs (the main login, a lane, a new lane) is Notchling's business, not a choice.
    /// Moving everything is one submenu below. Switching the main login or a whole lane lives in the
    /// menu bar (accounts list, Lanes), and new sessions start from the sidebar's +.
    func popUp() {
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
        let here = account.lane?.accountID
        let targets = model.sessionTargets
        header("This Session's Account")
        switch agent.target {
        case .job where agent.command == nil && agent.transcriptPath != nil:
            for target in targets {
                let current = target.accountID == here
                let item = ActionMenuItem(target.title) { [model, agent] in
                    guard !current else { return }
                    model.moveSession(agent, to: target.accountID, title: target.title)
                }
                item.state = current ? .on : .off
                menu.addItem(item)
            }
            note("Picking one moves only this session; the conversation continues there.")
        default:
            // A terminal session keeps the account it was started with.
            let item = NSMenuItem(title: account.email, action: nil, keyEquivalent: "")
            item.state = .on
            item.isEnabled = false
            menu.addItem(item)
            if case .process = agent.target {
                let others = targets.filter { $0.accountID != nil }
                if !others.isEmpty {
                    let open = NSMenuItem(title: "New Terminal Session On", action: nil, keyEquivalent: "")
                    let sub = NSMenu()
                    for target in others {
                        sub.addItem(ActionMenuItem(target.title) { [model, agent] in
                            if let id = target.accountID { model.newTerminalSession(for: agent, on: id) }
                        })
                    }
                    open.submenu = sub
                    menu.addItem(open)
                }
            }
        }
        if let accounts = model.accounts {
            let lanes = LaneStore.all()
            let all = targets.filter { target in
                !MoveAllPlan.make(agents: model.notch.agents, lanes: lanes, activeID: accounts.activeID, target: target.accountID).isEmpty
            }
            if !all.isEmpty {
                menu.addItem(.separator())
                let item = NSMenuItem(title: "Move All Sessions To", action: nil, keyEquivalent: "")
                let sub = NSMenu()
                for target in all {
                    sub.addItem(ActionMenuItem(target.title) { [model, agent] in
                        model.moveAll(to: target.accountID, email: target.title, from: agent)
                    })
                }
                item.submenu = sub
                menu.addItem(item)
            }
        }
        if model.accounts != nil {
            menu.addItem(.separator())
            menu.addItem(ActionMenuItem("Add Account…") { [model] in model.addAccount() })
            if model.codex?.isAvailable == true {
                menu.addItem(ActionMenuItem("Codex Accounts…") { [model] in model.addAccount(.codex) })
            }
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

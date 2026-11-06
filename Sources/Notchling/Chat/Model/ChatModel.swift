import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

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
    @Published var conversation = Conversation()
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
    @Published var subagents: [Subagent] = []
    /// The subagent being read in place of its session's conversation; nil shows the session.
    @Published var subagentID: String? {
        didSet {
            guard subagentID != oldValue else { return }
            subConversation = nil
            refresh()
        }
    }
    @Published var subConversation: Conversation?
    var viewedSubagent: Subagent? { subagentID.flatMap { id in subagents.first { $0.id == id } } }
    /// The subagent an Agent call started, for the call's Open button.
    func subagent(forCall toolUseID: String) -> Subagent? { subagents.first { $0.toolUseID == toolUseID } }
    /// The selected background session attached in a terminal sheet, for a command that opens a menu.
    @Published var terminal: TerminalSession?
    @Published var drafts: [String: String] = [:]
    @Published var pending: [String: Pending] = [:]
    /// Replies typed while the one before is still going out, sent in order after it. Before this,
    /// Return did nothing while a reply was being typed into the session.
    @Published var outbox: [String: [String]] = [:]
    var modelHint: [String: (model: String, at: Date)] = [:]
    var effortHint: [String: (effort: String, at: Date)] = [:]
    @Published var errors: [String: String] = [:]
    @Published var completionIndex = 0
    @Published var search = ""
    /// Bumped to put the cursor in the reply box (the window opening, a session picked from the notch).
    @Published var focusRequest = 0
    /// Bumped to give the sidebar keyboard focus.
    @Published var sidebarFocusRequest = 0
    var composerFocused = false
    /// Sessions seen, so one that drops off the list stays readable while it's open.
    var seen: [String: Agent] = [:]
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

    // Stored properties of the extensions in ChatModel+*.swift (an extension can't hold them).

    let accounts: AccountStore?
    private var accountsWatch: AnyCancellable?
    /// Lanes by session id, from the lane folder each session runs in. Re-read every few seconds, since a
    /// switch changes a lane's account.
    @Published var lanes: [String: Lane] = [:]
    var lanesChecked = Date.distantPast
    /// Limit switches by lane name ("" for the main login), shown in the sessions they moved.
    @Published var accountNotes: [String: AccountNote] = [:]
    @Published var accountErrors: [String: String] = [:]

    /// Codex and Antigravity rows the user closed, with when. The notch's Hide only knows Claude Code's
    /// sessions, so these are kept here; a row comes back when its session is used again later.
    @Published var closedSessions: [String: Double] =
        UserDefaults.standard.dictionary(forKey: "chatClosedSessions") as? [String: Double] ?? [:]

    /// Recent Codex CLI sessions (`CodexSessions`), re-read every couple of seconds.
    @Published var codexAgents: [Agent] = []
    /// New Codex sessions picked in New Session that wait for their first message, which starts them.
    @Published var codexDrafts: [Agent] = []
    var codexListedAt = Date.distantPast
    var codexListing = false

    /// The model and effort picked per Codex session (by row id), for its next turns. Kept across
    /// launches, since nothing else remembers them: exec takes them per turn.
    @Published var codexPicks: [String: CodexPick] = {
        guard let data = UserDefaults.standard.data(forKey: "codexPicks"),
              let saved = try? JSONDecoder().decode([String: CodexPick].self, from: data) else { return [:] }
        return saved
    }()

    /// Recent Antigravity CLI conversations (`AntigravitySessions`), re-read every few seconds.
    @Published var agyAgents: [Agent] = []
    /// New Antigravity sessions picked in New Session that wait for their first message.
    @Published var agyDrafts: [Agent] = []
    var agyListedAt = Date.distantPast
    var agyListing = false

    /// Sessions started from Chat, kept until Antigravity's summaries list them.
    var agyStarted: [Agent] = []

    /// Sessions a Stop was sent to, until they stop working (or 20 s pass).
    @Published var stopping: [String: Date] = [:]

    /// Sessions whose queue is being taken back to edit.
    @Published var takingBack: Set<String> = []

    /// `pages.json`'s modification date when `refreshPage` last read it for the shown page (nil: not read yet),
    /// so the 1-second tick parses the list only after it changed.
    var pageChecked: Date??
}

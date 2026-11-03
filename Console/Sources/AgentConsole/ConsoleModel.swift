import AppKit
import ConsoleCore
import Observation

enum SendState: Equatable {
    case idle
    case sending
    case failed(message: String, text: String)
}

/// A claude process Console started that has no session file yet (or whose session is not listed).
struct StartingTerminal: Identifiable, Equatable {
    let id: UUID
    let folder: String
    let model: String?
    /// The session this terminal resumes (a takeover); nil for a new session.
    var resumedFrom: String?

    /// Selection id used while the process has no session of its own.
    var selectionId: String { "pty:\(id.uuidString)" }
}

enum NewSessionModel: String, CaseIterable, Identifiable {
    case defaultModel = "default", opus, sonnet, haiku
    var id: String { rawValue }
    var title: String { self == .defaultModel ? "Mặc định" : rawValue }
    var argument: String? { self == .defaultModel ? nil : rawValue }
}

@MainActor
@Observable
final class ConsoleModel {
    let store: SessionStore
    let viz = VizStore()
    let bridge = BridgeController()
    let owned: OwnedSessions
    /// Where the pty sessions live: the session host (they outlive this app) or this process.
    @ObservationIgnored var terminals: TerminalBackend
    let paths: ClaudePaths

    var searchText = ""
    var showingNewSession = false
    var newSessionError: String?
    var drafts: [String: String] = [:]
    var sendStates: [String: SendState] = [:]
    var answerErrors: [String: String] = [:]
    var terminalShown: Set<String> = []
    /// A takeover waiting for the user's confirmation.
    var confirmingTakeover: Session?
    /// The session is mid-turn: a second confirmation before stopping it.
    var confirmingBusyStop: Session?
    var takeoverError: String?
    /// Old rows replaced by a resumed session that got a new id, so the list shows one session.
    private(set) var replacedSessionIds: Set<String> = []
    private(set) var starting: [StartingTerminal] = []
    private(set) var expanded: Set<String>
    private(set) var visualizeChoice: [String: String]

    @ObservationIgnored private let router: InputRouter
    @ObservationIgnored private let responder: HookResponder
    @ObservationIgnored private let suggester = VisualizeSuggester()
    @ObservationIgnored private let defaults = UserDefaults.standard
    @ObservationIgnored private var mappingTask: Task<Void, Never>?

    init(paths: ClaudePaths = .live) {
        self.paths = paths
        let owned = OwnedSessions()
        self.owned = owned
        // The host when it is installed and answers; otherwise sessions are children of this app, as before.
        let backend: TerminalBackend = TerminalBackends.hostIsInstalled ? TerminalBackends.connectOrLocal() : LocalBackend()
        terminals = backend
        for existing in backend.existingSessions() { owned.add(existing) }
        store = SessionStore(
            catalog: SessionCatalog(paths: paths, isOwned: { owned.isOwned(pid: $0) }),
            claudeDir: paths.claudeDir)
        router = InputRouter(job: JobReplySender(), pty: PTYMessageSender(owned: owned))
        let demo = UserDefaults.standard.bool(forKey: "demoPrompt")
        responder = HookResponder(responsesDir: demo
            ? FileManager.default.temporaryDirectory.appendingPathComponent("agentconsole-demo-responses")
            : paths.responsesDir)
        expanded = Set(defaults.stringArray(forKey: "expandedSessions") ?? [])
        visualizeChoice = (defaults.dictionary(forKey: "visualizeChoice") as? [String: String]) ?? [:]
    }

    // MARK: - Visual QA hooks (launch flags, off by default)

    /// `-demoPrompt YES`: shows a made-up permission card on the selected session. Answers go to a
    /// temp folder, never to Notchling.
    var demoPromptEnabled: Bool { defaults.bool(forKey: "demoPrompt") }

    func demoPrompt(for session: Session) -> PendingPrompt? {
        guard demoPromptEnabled else { return nil }
        return PendingPrompt(id: "demo-\(session.id)", sessionId: session.id, kind: .permission, toolName: "Bash",
                             summary: "swift test --package-path Console", created: Date())
    }

    /// `-demoTerminal YES`: opens a plain shell (not Claude) in a Console-owned pty, to check the Terminal view.
    func startDemoTerminalIfRequested() {
        guard defaults.bool(forKey: "demoTerminal") else { return }
        do {
            let pty = try terminals.spawnCommand(executable: URL(fileURLWithPath: "/bin/zsh"), arguments: ["-f", "-i"],
                                                 cwd: FileManager.default.temporaryDirectory)
            owned.add(pty)
            let entry = StartingTerminal(id: pty.id, folder: FileManager.default.temporaryDirectory.path, model: nil)
            starting.append(entry)
            select(ChatSelection(sessionId: entry.selectionId))
        } catch {
            ConsoleLog.error("startDemoTerminalIfRequested", "/bin/zsh", "\(error)")
        }
    }

    // MARK: - Derived

    var sessions: [Session] { store.sessions.filter { !replacedSessionIds.contains($0.id) } }

    /// What the phone sees: the sessions, plus Console-owned terminals that have not become a session yet
    /// (a new session waiting on the folder-trust prompt, say), so the phone can type into them.
    var bridgeSessions: [Session] {
        let pending: [Session] = starting.compactMap { entry in
            guard let pty = terminal(for: entry) else { return nil }
            return Session(id: entry.selectionId, pid: pty.rootPid, name: "Đang khởi động: \((entry.folder as NSString).lastPathComponent)",
                           project: (entry.folder as NSString).lastPathComponent, cwd: entry.folder, entrypoint: .cli, kind: "interactive",
                           state: .needsInput, statusUpdatedAt: pty.startedAt, startedAt: pty.startedAt, transcriptURL: nil,
                           control: .consolePTY)
        }
        return pending + sessions
    }

    var filteredSessions: [Session] {
        let query = searchText.trimmingCharacters(in: .whitespaces).lowercased()
        guard !query.isEmpty else { return sessions }
        return sessions.filter { session in
            session.name.lowercased().contains(query) || session.project.lowercased().contains(query)
                || session.subagents.contains { $0.description.lowercased().contains(query) || $0.type.lowercased().contains(query) }
        }
    }

    /// Order the list shows, for ↑/↓ and Enter in search.
    var navigationOrder: [ChatSelection] {
        var result: [ChatSelection] = []
        for session in filteredSessions {
            result.append(ChatSelection(sessionId: session.id))
            if expanded.contains(session.id) {
                result += session.subagents.map { ChatSelection(sessionId: session.id, subagentId: $0.id) }
            }
        }
        return result
    }

    var selectedSession: Session? {
        guard let id = store.selection?.sessionId else { return nil }
        return sessions.first { $0.id == id }
    }

    var selectedStarting: StartingTerminal? {
        guard let id = store.selection?.sessionId else { return nil }
        return starting.first { $0.selectionId == id }
    }

    var selectedSubagent: Subagent? {
        guard let selection = store.selection, let subagentId = selection.subagentId else { return nil }
        return selectedSession?.subagents.first { $0.id == subagentId }
    }

    // MARK: - Selection

    func select(_ selection: ChatSelection?) {
        store.selection = selection
    }

    func moveSelection(by delta: Int) {
        let order = navigationOrder
        guard !order.isEmpty else { return }
        guard let current = store.selection, let index = order.firstIndex(of: current) else {
            select(delta > 0 ? order.first : order.last)
            return
        }
        select(order[min(max(index + delta, 0), order.count - 1)])
    }

    func selectFirstSearchMatch() {
        if let first = filteredSessions.first { select(ChatSelection(sessionId: first.id)) }
    }

    // MARK: - Persistent per-session UI state

    func isExpanded(_ sessionId: String) -> Bool { expanded.contains(sessionId) }

    func setExpanded(_ sessionId: String, _ isOpen: Bool) {
        if isOpen { expanded.insert(sessionId) } else { expanded.remove(sessionId) }
        defaults.set(Array(expanded), forKey: "expandedSessions")
    }

    /// QA only (`-vizCwd <path>`): the folder the Visualize engine and the suggester use instead of the session's.
    var vizCwdOverride: String? { defaults.string(forKey: "vizCwd") }

    func suggestedTab(for session: Session) -> VisualizeTab {
        let signals = viz.stateSessionId == session.id ? (viz.bundle?.signals ?? .none) : .none
        return suggester.suggest(cwd: vizCwdOverride ?? session.cwd, signals: signals)
    }

    /// Keeps the Visualize engine pointed at the selected session. Called when selection or sessions change.
    func syncViz() {
        let session = selectedSession
        viz.setTarget(session: session, cwdOverride: vizCwdOverride)
    }

    func visualizeTab(for session: Session) -> VisualizeTab {
        visualizeChoice[session.id].flatMap(VisualizeTab.init(rawValue:)) ?? suggestedTab(for: session)
    }

    func chooseVisualizeTab(_ tab: VisualizeTab, for session: Session) {
        visualizeChoice[session.id] = tab.rawValue
        defaults.set(visualizeChoice, forKey: "visualizeChoice")
    }

    // MARK: - Sending

    func sendState(for sessionId: String) -> SendState { sendStates[sessionId] ?? .idle }

    func send(_ text: String, to session: Session) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, sendState(for: session.id) != .sending else { return }
        sendStates[session.id] = .sending
        let router = self.router
        Task {
            do {
                try await router.send(trimmed, to: session)
                drafts[session.id] = ""
                sendStates[session.id] = .idle
            } catch {
                ConsoleLog.error("ConsoleModel.send", session.id, "\(error)")
                sendStates[session.id] = .failed(message: error.localizedDescription, text: trimmed)
            }
        }
    }

    func retrySend(for session: Session) {
        guard case .failed(_, let text) = sendState(for: session.id) else { return }
        sendStates[session.id] = .idle
        send(text, to: session)
    }

    func dismissSendError(for sessionId: String) {
        sendStates[sessionId] = .idle
    }

    /// Same routing as the Send button, for callers that need the result (the phone bridge).
    func deliver(_ text: String, to session: Session) async throws {
        try await router.send(text, to: session)
    }

    // MARK: - Pending prompts

    /// Writes the answer like the card does, but throws so the bridge can report a failure.
    func submit(_ response: PromptResponse, to prompt: PendingPrompt) throws {
        switch response {
        case .allow: try responder.allow(prompt)
        case .deny: try responder.deny(prompt)
        case .answers(let choices): try responder.answer(prompt, choices: choices)
        }
        answerErrors[prompt.sessionId] = nil
    }

    func allow(_ prompt: PendingPrompt) { respond(prompt) { try responder.allow(prompt) } }
    func deny(_ prompt: PendingPrompt) { respond(prompt) { try responder.deny(prompt) } }
    func answer(_ prompt: PendingPrompt, choices: [String]) { respond(prompt) { try responder.answer(prompt, choices: choices) } }

    private func respond(_ prompt: PendingPrompt, _ write: () throws -> Void) {
        do {
            try write()
            answerErrors[prompt.sessionId] = nil
        } catch {
            ConsoleLog.error("ConsoleModel.respond", prompt.id, "\(error)")
            answerErrors[prompt.sessionId] = error.localizedDescription
        }
    }

    // MARK: - View-only sessions

    /// "Mở trên Mac": the Claude desktop app for its own sessions, otherwise the folder in Finder.
    func openOnMac(_ session: Session) {
        if session.entrypoint == .desktop, let pid = session.pid,
           let hostId = desktopHostSessionId(pid: pid),
           let url = URL(string: "claude://code/continue?session=\(hostId)") {
            let configuration = NSWorkspace.OpenConfiguration()
            configuration.activates = true
            NSWorkspace.shared.open(url, configuration: configuration)
            return
        }
        guard !session.cwd.isEmpty else {
            ConsoleLog.error("ConsoleModel.openOnMac", session.id, "no cwd to reveal")
            return
        }
        NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: session.cwd)])
    }

    private func desktopHostSessionId(pid: Int32) -> String? {
        let url = paths.sessionsDir.appendingPathComponent("\(pid).json")
        let id: String?
        do {
            let data = try Data(contentsOf: url)
            id = ((try JSONSerialization.jsonObject(with: data)) as? [String: Any])?["hostSessionId"] as? String
        } catch {
            ConsoleLog.error("desktopHostSessionId", url.path, "\(error)")
            return nil
        }
        guard let id, id.range(of: #"^local_[A-Za-z0-9-]{1,64}$"#, options: .regularExpression) != nil else { return nil }
        return id
    }

    // MARK: - Owned sessions

    func startSession(folder: URL, model: NewSessionModel) {
        newSessionError = nil
        do {
            try launchSession(folder: folder, model: model.argument)
            showingNewSession = false
        } catch {
            ConsoleLog.error("ConsoleModel.startSession", folder.path, "\(error)")
            newSessionError = error.localizedDescription
        }
    }

    /// Starts `claude` in a pty Console owns and selects it. The folder-trust prompt, if any, shows in its
    /// Terminal view for the user to answer.
    func launchSession(folder: URL, model: String?) throws {
        let pty = try terminals.spawnClaude(folder: folder, model: model)
        adopt(pty, folder: folder, model: model, resumedFrom: nil)
    }

    /// "Tiếp quản": `claude --resume <id>` in a pty Console owns, for a session whose original process is gone.
    /// Everything is checked again here, right before the spawn, because the list may be a second old.
    /// Returns the pty's selection id; the real session replaces it once claude writes its session file.
    @discardableResult
    func takeOver(sessionId: String, assumeStopped: Bool = false) throws -> any TerminalHandle {
        guard let session = sessions.first(where: { $0.id == sessionId }) else { throw TakeoverError.noTranscript }
        let plan: TakeoverPlan
        switch Takeover.evaluate(session, paths: paths, assumeStopped: assumeStopped) {
        case .failure(let error): throw error
        case .success(let value): plan = value
        }
        // Last look at the original's process, right before spawning.
        if case .failure(let error) = Takeover.evaluate(session, paths: paths, assumeStopped: assumeStopped) { throw error }
        let pty = try terminals.resume(sessionId: plan.sessionId, folder: plan.cwd)
        adopt(pty, folder: plan.cwd, model: nil, resumedFrom: session.id)
        return pty
    }

    /// "Dừng bản gốc và tiếp quản": SIGTERM to the verified claude process that still holds the session (waits up to
    /// 5 s, never SIGKILL), then the normal takeover. Returns the pty and the pid that was stopped (nil when the
    /// original was already gone).
    func takeOverStoppingOriginal(sessionId: String) async throws -> (pty: any TerminalHandle, stoppedPid: Int32?) {
        guard let session = sessions.first(where: { $0.id == sessionId }) else { throw TakeoverError.noTranscript }
        let stopped = await OriginalStopper.stop(sessionId: session.id, knownPid: session.pid, sessionsDir: paths.sessionsDir,
                                                 control: LiveProcessControl())
        switch stopped {
        case .failure(let error): throw error
        case .success(let pids):
            let pty = try takeOver(sessionId: sessionId, assumeStopped: !pids.isEmpty)
            return (pty, pids.first)
        }
    }

    func requestTakeover(_ session: Session) {
        takeoverError = nil
        confirmingBusyStop = nil
        confirmingTakeover = session
    }

    /// The first confirmation said yes. A session that is mid-turn needs a second, explicit one before it is stopped.
    func confirmTakeover(stopOriginal: Bool) {
        guard let session = confirmingTakeover else { return }
        if stopOriginal, case .stoppable(let busy)? = Takeover.state(for: session, paths: paths), busy {
            confirmingTakeover = nil
            confirmingBusyStop = session
            return
        }
        runTakeover(session, stopOriginal: stopOriginal)
    }

    func confirmBusyStop() {
        guard let session = confirmingBusyStop else { return }
        confirmingBusyStop = nil
        runTakeover(session, stopOriginal: true)
    }

    private func runTakeover(_ session: Session, stopOriginal: Bool) {
        confirmingTakeover = nil
        Task { @MainActor in
            do {
                if stopOriginal {
                    let result = try await takeOverStoppingOriginal(sessionId: session.id)
                    bridge.recordTakeover(sessionName: session.name, from: "Mac", pty: result.pty, stoppedPid: result.stoppedPid)
                } else {
                    let pty = try takeOver(sessionId: session.id)
                    bridge.recordTakeover(sessionName: session.name, from: "Mac", pty: pty, stoppedPid: nil)
                }
            } catch {
                ConsoleLog.error("ConsoleModel.runTakeover", session.id, "\(error)")
                takeoverError = error.localizedDescription
            }
        }
    }

    /// The real session a takeover pty became, once there is one.
    func session(ownedBy pty: any TerminalHandle) -> Session? {
        sessions.first { terminal(for: $0)?.id == pty.id }
    }

    private func adopt(_ pty: any TerminalHandle, folder: URL, model: String?, resumedFrom: String?) {
        owned.add(pty)
        let entry = StartingTerminal(id: pty.id, folder: folder.path, model: model, resumedFrom: resumedFrom)
        starting.append(entry)
        terminalShown.insert(entry.selectionId)
        select(ChatSelection(sessionId: entry.selectionId))
        watchForSession(of: pty, entry: entry)
        pty.onExit { [weak self] in
            Task { @MainActor in self?.ptyExited(entry) }
        }
    }

    /// The session file appears once claude is past its startup prompts; until then the user answers
    /// them in the Terminal view. When it shows up, the selection moves to the real session.
    private func watchForSession(of pty: any TerminalHandle, entry: StartingTerminal) {
        let sessionsDir = paths.sessionsDir
        let rootPid = pty.rootPid
        Task { [weak self] in
            for _ in 0..<600 {
                try? await Task.sleep(nanoseconds: 1_000_000_000)
                guard let self, pty.isRunning else { return }
                let id = await Task.detached { SessionPidMapper.sessionId(forRootPid: rootPid, sessionsDir: sessionsDir) }.value
                guard let id else { continue }
                await MainActor.run {
                    if let original = entry.resumedFrom, case .replaced(let old, _) = Takeover.resolution(original: original, resumed: id) {
                        self.replacedSessionIds.insert(old)
                    }
                    self.starting.removeAll { $0.id == entry.id }
                    if self.store.selection?.sessionId == entry.selectionId {
                        self.terminalShown.insert(id)
                        self.select(ChatSelection(sessionId: id))
                    }
                }
                return
            }
            ConsoleLog.error("watchForSession", sessionsDir.path, "no session file for pid \(rootPid) after 10 minutes")
        }
    }

    private func ptyExited(_ entry: StartingTerminal) {
        starting.removeAll { $0.id == entry.id }
        owned.remove(entry.id)
    }

    func terminal(for session: Session) -> (any TerminalHandle)? {
        session.pid.flatMap { owned.pty(owning: $0) }
    }

    func terminal(for entry: StartingTerminal) -> (any TerminalHandle)? {
        owned.all.first { $0.id == entry.id }
    }

    /// Called when the app quits. Sessions in the host keep running; only ones that are children of this app end.
    func terminateOwned() {
        guard !terminals.sessionsOutliveApp else { return }
        for pty in owned.running { pty.terminateAndWait() }
    }
}

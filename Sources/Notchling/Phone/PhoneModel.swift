import AppKit
import ConsoleCore

/// A claude process Notchling started for a phone that has no session file yet (or whose session is not listed).
struct StartingTerminal: Identifiable, Equatable {
    let id: UUID
    let folder: String
    let model: String?
    /// The session this terminal resumes (a takeover); nil for a new session.
    var resumedFrom: String?

    /// Id the phone sees while the process has no session of its own.
    var selectionId: String { "pty:\(id.uuidString)" }
}

/// What the phone bridge needs from the running app: the session list, the terminals that live in the session
/// host (hostd), sending, prompt answers, new sessions and takeovers. This was the model of the Agent Console app;
/// the window is gone, so only the parts the phone uses remain.
@MainActor
final class PhoneModel: ObservableObject {
    let store: SessionStore
    let bridge = BridgeController()
    nonisolated let owned: OwnedSessions
    /// Where the pty sessions live: the session host (they outlive this app) or this process.
    var terminals: TerminalBackend
    let paths: ClaudePaths

    /// Old rows replaced by a resumed session that got a new id, so the list shows one session.
    private(set) var replacedSessionIds: Set<String> = []
    private(set) var starting: [StartingTerminal] = []

    private let router: InputRouter
    private let responder: HookResponder

    init(paths: ClaudePaths = .live) {
        self.paths = paths
        let owned = OwnedSessions()
        self.owned = owned
        // The host when it is installed and answers; otherwise sessions are children of this app.
        let backend: TerminalBackend = TerminalBackends.hostIsInstalled ? TerminalBackends.connectOrLocal() : LocalBackend()
        terminals = backend
        for existing in backend.existingSessions() { owned.add(existing) }
        store = SessionStore(
            catalog: SessionCatalog(paths: paths, isOwned: { owned.isOwned(pid: $0) }),
            claudeDir: paths.claudeDir)
        router = InputRouter(job: JobReplySender(), pty: PTYMessageSender(owned: owned))
        responder = HookResponder(responsesDir: paths.responsesDir)
    }

    /// Starts polling ~/.claude for the phone's session list.
    func start() {
        store.start()
    }

    // MARK: - Derived

    var sessions: [Session] { store.sessions.filter { !replacedSessionIds.contains($0.id) } }

    /// What the phone sees: the sessions, plus owned terminals that have not become a session yet
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

    // MARK: - Sending

    /// Routes by `Session.control`: a job through `claude attach`, an owned terminal through its pty.
    func deliver(_ text: String, to session: Session) async throws {
        try await router.send(text, to: session)
    }

    /// Types `text` into the terminal that owns process `pid` (bracketed paste, then Enter). Blocking: call off the
    /// main thread. Chat's reply box uses it, so the Mac and the phone type into the same pty.
    nonisolated static func type(_ text: String, intoTerminalOwning pid: Int32, in owned: OwnedSessions) throws {
        guard let pty = owned.pty(owning: pid) else { throw SendError.notOwned }
        for chunk in TerminalKeys.replyChunks(text) {
            pty.write(chunk)
            Thread.sleep(forTimeInterval: 0.25)
        }
    }

    /// Writes raw key chunks (no bracketed paste) into the owned terminal, pausing between them so the TUI redraws.
    /// Blocking: call off the main thread.
    nonisolated static func keys(_ chunks: [String], intoTerminalOwning pid: Int32, in owned: OwnedSessions) throws {
        guard let pty = owned.pty(owning: pid) else { throw SendError.notOwned }
        for chunk in chunks {
            pty.write(chunk)
            Thread.sleep(forTimeInterval: 0.4)
        }
    }

    // MARK: - Pending prompts

    /// Writes the answer like Notchling's card does, but throws so the bridge can report a failure.
    func submit(_ response: PromptResponse, to prompt: PendingPrompt) throws {
        switch response {
        case .allow: try responder.allow(prompt)
        case .deny: try responder.deny(prompt)
        case .answers(let choices): try responder.answer(prompt, choices: choices)
        }
    }

    // MARK: - Owned sessions

    /// Starts `claude` in a pty the host owns. The folder-trust prompt, if any, is answered from the phone.
    func launchSession(folder: URL, model: String?) throws {
        let pty = try terminals.spawnClaude(folder: folder, model: model)
        adopt(pty, folder: folder, model: model, resumedFrom: nil)
    }

    /// "Tiếp quản": `claude --resume <id>` in an owned pty, for a session whose original process is gone.
    /// Everything is checked again here, right before the spawn, because the list may be a second old.
    @discardableResult
    func takeOver(sessionId: String, assumeStopped: Bool = false) throws -> any TerminalHandle {
        guard let session = sessions.first(where: { $0.id == sessionId }) else { throw TakeoverError.noTranscript }
        let plan: TakeoverPlan
        switch Takeover.evaluate(session, paths: paths, assumeStopped: assumeStopped) {
        case .failure(let error): throw error
        case .success(let value): plan = value
        }
        if case .failure(let error) = Takeover.evaluate(session, paths: paths, assumeStopped: assumeStopped) { throw error }
        let pty = try terminals.resume(sessionId: plan.sessionId, folder: plan.cwd)
        adopt(pty, folder: plan.cwd, model: nil, resumedFrom: session.id)
        return pty
    }

    /// "Dừng bản gốc và tiếp quản": SIGTERM to the verified claude process that still holds the session (waits up to
    /// 5 s, never SIGKILL), then the normal takeover.
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

    /// The real session a takeover pty became, once there is one.
    func session(ownedBy pty: any TerminalHandle) -> Session? {
        sessions.first { terminal(for: $0)?.id == pty.id }
    }

    private func adopt(_ pty: any TerminalHandle, folder: URL, model: String?, resumedFrom: String?) {
        owned.add(pty)
        let entry = StartingTerminal(id: pty.id, folder: folder.path, model: model, resumedFrom: resumedFrom)
        starting.append(entry)
        watchForSession(of: pty, entry: entry)
        pty.onExit { [weak self] in
            Task { @MainActor in self?.ptyExited(entry) }
        }
    }

    /// The session file appears once claude is past its startup prompts. When it shows up, the starting entry goes
    /// away and a resumed original is hidden in favour of the new session.
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

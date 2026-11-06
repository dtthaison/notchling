import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
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
    func closeAccounts() {
        signIn = nil
        codex?.cancelAdd()
    }

    // MARK: Accounts (lanes)

    /// The account a session runs on: its lane's, or the main login's. Nil when unknown (an ended session).
    struct SessionAccount: Equatable {
        let email: String
        let lane: Lane?
    }

    struct AccountNote: Equatable {
        let text: String
        let at: Date
    }

    func account(for agent: Agent) -> SessionAccount? {
        if case .codex = agent.target { return nil }
        if case .antigravity = agent.target { return nil }
        if case .gemini = agent.target { return nil }
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
        if case .gemini = agent.target { return nil }
        if let lane = lanes[agent.id] { return accounts.usage[lane.accountID] }
        if agent.configDir != nil { return nil }
        return accounts.activeID.flatMap { accounts.usage[$0] }
    }

    /// The limit bars the header shows: the Claude account's 5-hour and weekly limits, or for a Codex
    /// session the Codex login's (its first group, "Codex": 5h and week on Plus and Pro), or for an
    /// Antigravity CLI session the active Antigravity login's most used group.
    func limits(for agent: Agent) -> [SessionLimit] {
        if case .codex = agent.target {
            guard let codex, let id = codex.activeID, let usage = codex.usage[id] else { return [] }
            let group = usage.groups.first { $0.name == "Codex" } ?? usage.groups.first
            return (group?.bars ?? []).map { SessionLimit(label: $0.label, name: SessionLimit.name($0.label), window: $0.window) }
        }
        if case .antigravity = agent.target {
            // Limits exist only while the Antigravity app runs; the last numbers stay until it does again.
            guard let antigravity, let id = antigravity.activeID, let usage = antigravity.usage[id] else { return [] }
            return SessionLimit.mostUsedGroup(UsageReport(usage))
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
}

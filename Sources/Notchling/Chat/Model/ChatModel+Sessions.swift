import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    var selected: Agent? {
        guard let id = selection else { return nil }
        return allAgents.first { $0.id == id } ?? seen[id]
    }

    /// The Chat row id for a session found by search. A session in the sidebar is used as it is; one that
    /// isn't (older, closed, or ended) is added to `seen` so it can be selected and read, and a closed
    /// Codex or Antigravity row comes back.
    func prepareSession(_ ref: SessionMeta) -> String {
        let name = ref.title, project = (ref.folder as NSString).lastPathComponent
        let agent: Agent
        switch ref.tool {
        case .claude:
            if let listed = allAgents.first(where: { $0.sessionID == ref.sessionID }) { return listed.id }
            agent = Agent(id: ref.sessionID, name: name, project: project, cwd: ref.folder, group: .completed,
                          label: "Exited", detail: nil, since: ref.date,
                          target: .resume(sessionID: ref.sessionID, cwd: ref.folder),
                          transcriptPath: ref.path, configDir: ref.configDir)
        case .codex:
            agent = Agent(id: CodexSessions.agentID(ref.sessionID), name: name, project: project, cwd: ref.folder,
                          group: .completed, label: "Codex", detail: nil, since: ref.date,
                          target: .codex(sessionID: ref.sessionID), transcriptPath: ref.path, host: "Codex")
        case .antigravity:
            agent = Agent(id: AntigravitySessions.agentID(ref.sessionID), name: name, project: project, cwd: ref.folder,
                          group: .completed, label: "Antigravity", detail: nil, since: ref.date,
                          target: .antigravity(sessionID: ref.sessionID), host: "Antigravity")
        }
        if closedSessions[agent.id] != nil {
            closedSessions[agent.id] = nil
            UserDefaults.standard.set(closedSessions, forKey: "chatClosedSessions")
        }
        if allAgents.first(where: { $0.id == agent.id }) == nil { seen[agent.id] = agent }
        if ref.tool == .codex { refreshCodex(force: true) }
        if ref.tool == .antigravity { refreshAntigravity(force: true) }
        return agent.id
    }

    /// Claude Code's sessions (the notch's), then Codex's and Antigravity CLI's, which only Chat shows.
    var allAgents: [Agent] {
        notch.agents + codexDrafts + (codexAgents + agyAgents).filter { !isClosed($0) } + agyDrafts
    }

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
    func dropDelivered(_ id: String) {
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
}

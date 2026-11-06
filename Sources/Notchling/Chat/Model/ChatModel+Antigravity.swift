import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    // MARK: Antigravity CLI sessions

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
    func antigravityChanged() {
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

    func sendAntigravity(_ agent: Agent, sessionID: String, text: String) {
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
}

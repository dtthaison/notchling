import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    // MARK: Antigravity CLI and Gemini CLI sessions (Agent Client Protocol)

    private var installedACPTools: [ACPTool] { [ACPTool.antigravity, .gemini].filter { $0.binary != nil } }

    func refreshACP(force: Bool = false) {
        let tools = installedACPTools
        guard !tools.isEmpty, !acpListing, force || Date().timeIntervalSince(acpListedAt) > 3 else { return }
        acpListing = true
        let working = Dictionary(uniqueKeysWithValues: tools.map { ($0, $0.runner.working) })
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let list = tools.flatMap { $0.list(live: working[$0] ?? []) }.sorted { $0.since > $1.since }
            DispatchQueue.main.async {
                guard let self else { return }
                self.acpListing = false
                self.acpListedAt = Date()
                // A session started here that the tool's list doesn't show yet stays as it was.
                let listed = Set(list.map(\.id))
                let extra = self.acpStarted.filter { !listed.contains($0.id) }
                let merged = list.map { self.withLiveTitle($0) } + extra
                if merged.map(\.id) != self.acpAgents.map(\.id) || zip(merged, self.acpAgents).contains(where: {
                    $0.group != $1.group || $0.name != $1.name || $0.since != $1.since
                }) {
                    self.acpAgents = merged
                }
            }
        }
    }

    /// A row with the live state the runner knows: working, and the title the tool gave it.
    private func withLiveTitle(_ agent: Agent) -> Agent {
        guard let (tool, session) = agent.target.acp else { return agent }
        let runner = tool.runner
        let working = runner.isWorking(session)
        let title = runner.title(session) ?? agent.name
        guard working != (agent.group == .working) || title != agent.name else { return agent }
        return Agent(id: agent.id, name: title, project: agent.project, cwd: agent.cwd,
                     group: working ? .working : .completed, label: working ? "Working" : tool.name,
                     detail: nil, since: working ? Date() : agent.since, target: agent.target, host: tool.host)
    }

    /// A runner changed (a chunk, a tool call, a permission, a turn's end).
    func acpChanged() {
        if let agent = selected, let (tool, session) = agent.target.acp, !session.isEmpty,
           let fresh = tool.runner.conversation(session), fresh != conversation {
            conversation = fresh
            dropDelivered(agent.id)
        }
        let updated = acpAgents.map { withLiveTitle($0) }
        if updated.map(\.name) != acpAgents.map(\.name) || updated.map(\.group) != acpAgents.map(\.group) { acpAgents = updated }
        objectWillChange.send()
    }

    /// A new session in `folder`: listed and selected at once, started by its first message.
    func newACPSession(_ tool: ACPTool, in folder: String) {
        addProjectFolder(folder)
        let agent = Agent(id: tool.draftPrefix + UUID().uuidString, name: "New \(tool.name) Session",
                          project: (folder as NSString).lastPathComponent, cwd: folder, group: .completed, label: "New",
                          detail: nil, since: Date(), target: tool.target(""), host: tool.host)
        acpDrafts.insert(agent, at: 0)
        selection = agent.id
        focusRequest += 1
    }

    func newAntigravitySession(in folder: String) { newACPSession(.antigravity, in: folder) }
    func newGeminiSession(in folder: String) { newACPSession(.gemini, in: folder) }

    /// Closes a draft, or the session's process (the conversation stays with the tool).
    func closeACP(_ agent: Agent) {
        guard let (tool, session) = agent.target.acp else { return }
        if session.isEmpty {
            acpDrafts.removeAll { $0.id == agent.id }
            if selection == agent.id { selection = nil }
        } else {
            tool.runner.close(session)
        }
    }

    func sendACP(_ agent: Agent, sessionID: String, text: String) {
        guard let tool = agent.target.acp?.tool else { return }
        let id = agent.id
        errors[id] = nil
        let runner = tool.runner
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
            self.refreshACP(force: true)
            if var waiting = self.outbox[key], !waiting.isEmpty {
                let next = waiting.removeFirst()
                self.outbox[key] = waiting.isEmpty ? nil : waiting
                let current = self.allAgents.first { $0.id == key } ?? self.seen[key] ?? agent
                self.sendACP(current, sessionID: session, text: next)
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
                let key = tool.agentID(session)
                self.pending[key] = self.pending.removeValue(forKey: id)
                self.pending[key]?.sending = false
                self.drafts[key] = self.drafts.removeValue(forKey: id)
                self.outbox[key] = self.outbox.removeValue(forKey: id)
                self.acpDrafts.removeAll { $0.id == id }
                let row = Agent(id: key, name: tool.title(text), project: agent.project, cwd: agent.cwd,
                                group: .working, label: "Working", detail: nil, since: Date(),
                                target: tool.target(session), host: tool.host)
                self.seen[key] = row
                self.acpStarted.append(row)
                self.acpAgents.insert(row, at: 0)
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

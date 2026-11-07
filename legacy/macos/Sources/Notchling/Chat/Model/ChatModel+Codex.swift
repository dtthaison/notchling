import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    // MARK: Codex sessions

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
    func runCodexCommand(_ name: String, text: String, agent: Agent, session: String, clearDraft: Bool) {
        let id = agent.id
        let done = { [weak self] in if clearDraft { self?.clearComposer(id) } }
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

    func sendCodex(_ agent: Agent, sessionID: String, text: String) {
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

    func setCodexPick(_ pick: CodexPick, for id: String) {
        codexPicks[id] = pick == CodexPick() ? nil : pick
        if let data = try? JSONEncoder().encode(codexPicks.filter { !$0.key.hasPrefix("codex-new:") }) {
            UserDefaults.standard.set(data, forKey: "codexPicks")
        }
    }
}

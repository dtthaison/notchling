import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    // MARK: Replying

    func send(_ preset: String? = nil) {
        if let agent = selected, let (tool, session) = agent.target.acp {
            let text = (preset ?? drafts[agent.id] ?? "").trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty, tool.runner.permission(session) == nil else { return }
            if preset == nil { drafts[agent.id] = "" }
            sendACP(agent, sessionID: session, text: text)
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
                    // A question or permission is open in the session. A question gets read off its terminal and
                    // shown as a card here; anything Chat can't show opens in the terminal sheet.
                    if case ReplySender.Failure.dialog = failure, self.selection == id, self.terminal == nil {
                        guard let agent = self.selected, agent.id == id else { return }
                        self.notch.readScreenQuestion(agent, force: true) { [weak self] found in
                            guard let self else { return }
                            if found {
                                self.errors[id] = nil
                            } else if self.selection == id, self.terminal == nil {
                                self.openTerminal(typing: nil)
                            }
                        }
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
    func sendNext(_ id: String, job jobID: String) {
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
        if let (tool, session) = agent.target.acp {
            return Array(SlashCommands.matches(String(draft.dropFirst()), in: tool.runner.commands(session)).prefix(8))
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

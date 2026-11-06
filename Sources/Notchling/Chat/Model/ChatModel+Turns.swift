import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

extension ChatModel {
    /// The selected session's turn can be stopped from here: a working background Claude session (Esc
    /// through `claude attach`) or a Codex turn Chat runs. Terminal sessions belong to their window.
    func canStop(_ agent: Agent) -> Bool {
        guard agent.group == .working else { return false }
        switch agent.target {
        case .job: return agent.command == nil
        case .codex(let session): return !session.isEmpty && CodexRunner.shared.isRunning(session)
        case .antigravity(let session): return !session.isEmpty && AntigravityRunner.shared.isWorking(session)
        case .gemini(let session): return !session.isEmpty && GeminiRunner.shared.isWorking(session)
        default: return false
        }
    }

    func isStopping(_ agent: Agent) -> Bool {
        guard let at = stopping[agent.id] else { return false }
        return agent.group == .working && Date().timeIntervalSince(at) < 20
    }

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
        case .gemini(let session):
            GeminiRunner.shared.cancel(session)
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
}

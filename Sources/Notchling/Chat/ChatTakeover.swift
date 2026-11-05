import AppKit
import ConsoleCore

/// "Take Over…" for a session row in Chat: resumes a session that no Notchling-owned terminal holds inside the
/// session host, so Chat and the phone can type into it. The engine is the phone's (`PhoneModel.takeOver`).
enum ChatTakeover {
    struct Prompt: Equatable {
        let title: String
        let message: String
        let confirm: String
        let destructive: Bool
    }

    /// Terminal sessions the host doesn't own, and ended ones. Jobs take replies already; `claude -p` can't be resumed live.
    static func offersTakeover(_ agent: Agent) -> Bool {
        switch agent.target {
        case .job, .codex, .antigravity: return false
        case .process: return !agent.repliesViaHost && agent.host != "claude -p"
        case .resume: return true
        }
    }

    /// What to ask before acting; nil when the session is already controllable.
    static func prompt(for state: TakeoverState?, name: String) -> Prompt? {
        guard let state else { return nil }
        switch state {
        case .available:
            return Prompt(title: "Take Over “\(name)”?",
                          message: "Notchling runs claude --resume for it in the session host, in the same folder. The original has ended, so nothing writes twice. It then takes replies from Chat and the phone.",
                          confirm: "Take Over", destructive: false)
        case .stoppable:
            return Prompt(title: "Take Over “\(name)”?",
                          message: "Notchling stops the original (Claude desktop, VS Code or a terminal) and resumes the session in the session host. It then takes replies from Chat and the phone.",
                          confirm: "Stop Original and Take Over", destructive: true)
        case .unavailable(let why):
            return Prompt(title: "Can’t Take Over “\(name)”", message: why, confirm: "OK", destructive: false)
        }
    }

    /// Asked on top of the first prompt when stopping would cut a turn short.
    static func busyPrompt(name: String) -> Prompt {
        Prompt(title: "Session Is Running", message: "Its current turn stops midway.", confirm: "Stop and Take Over", destructive: true)
    }

    /// Checks the state once, at click time (it inspects processes, so never per render), asks, then takes over and
    /// selects the real session once claude has written its session file.
    @MainActor
    static func run(_ agent: Agent, select: @escaping (String) -> Void) {
        let phone = PhoneService.shared.model
        guard let session = phone.sessions.first(where: { $0.id == agent.id }) else {
            show(Prompt(title: "Can’t Take Over “\(agent.name)”", message: TakeoverError.noTranscript.localizedDescription,
                        confirm: "OK", destructive: false), cancel: false)
            return
        }
        let state = Takeover.state(for: session, paths: phone.paths)
        guard let first = prompt(for: state, name: agent.name), let state else { return }
        if case .unavailable = state { _ = show(first, cancel: false); return }
        guard show(first, cancel: true) else { return }
        var stopOriginal = false
        if case .stoppable(let busy) = state {
            stopOriginal = true
            if busy, !show(busyPrompt(name: agent.name), cancel: true) { return }
        }

        Task { @MainActor in
            do {
                let pty: any TerminalHandle
                var stoppedPid: Int32?
                if stopOriginal {
                    let result = try await phone.takeOverStoppingOriginal(sessionId: session.id)
                    pty = result.pty
                    stoppedPid = result.stoppedPid
                } else {
                    pty = try phone.takeOver(sessionId: session.id)
                }
                PhoneService.shared.bridge.recordTakeover(sessionName: session.name, from: "Mac", pty: pty, stoppedPid: stoppedPid)
                for _ in 0..<40 {
                    if let real = phone.session(ownedBy: pty) { select(real.id); return }
                    try await Task.sleep(nanoseconds: 500_000_000)
                }
            } catch {
                ConsoleLog.error("ChatTakeover.run", agent.id, "\(error)")
                show(Prompt(title: "Couldn’t Take Over", message: error.localizedDescription, confirm: "OK", destructive: false), cancel: false)
            }
        }
    }

    @MainActor
    @discardableResult
    private static func show(_ prompt: Prompt, cancel: Bool) -> Bool {
        NSApp.activate(ignoringOtherApps: true)
        let alert = NSAlert()
        alert.messageText = prompt.title
        alert.informativeText = prompt.message
        alert.addButton(withTitle: prompt.confirm)
        if cancel { alert.addButton(withTitle: "Cancel") }
        alert.buttons.first?.hasDestructiveAction = prompt.destructive
        return alert.runModal() == .alertFirstButtonReturn
    }
}

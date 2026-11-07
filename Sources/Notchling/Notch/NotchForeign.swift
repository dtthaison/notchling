import Foundation

/// Codex and Antigravity CLI sessions in the notch. They keep their own list (`NotchModel.foreignAgents`) apart
/// from Claude Code's, which Chat and the account moves read as `agents`. The pure parts here (grouping, the
/// conversation as notch lines, option order) are what `NotchForeignTests` checks; the rest drives the same
/// readers and runners Chat and the phone use, so a turn started here shows there and the other way round.
enum NotchForeign {
    /// Done rows older than this are left out of the notch (Chat lists 14 days), and at most `perTool` of each tool.
    static let maxAge: TimeInterval = 12 * 3600
    static let perTool = 8

    static func isForeign(_ agent: Agent) -> Bool {
        switch agent.target {
        case .codex, .antigravity: return true
        default: return false
        }
    }

    // MARK: Grouping

    /// Chat's rows as the notch lists them: not closed in Chat, recent, capped per tool, newest first, with the
    /// live state laid over the rows' own: a turn the runners are in the middle of is Working, and an Antigravity
    /// permission waiting for an answer is Needs Input. Finished rows read "Done".
    static func merge(_ rows: [Agent], closed: [String: Double], codexRunning: Set<String>, agyWorking: Set<String>,
                      permissions: [String: ACPPermission], now: Date = Date()) -> [Agent] {
        ForeignSessions.visible(rows, closed: closed, now: now, maxAge: maxAge, perTool: perTool).map { row in
            var agent = row
            switch row.target {
            case .codex(let id):
                if codexRunning.contains(id) { agent.group = .working }
            case .antigravity(let id):
                agent.group = (agyWorking.contains(id) || permissions[row.id] != nil) ? .working : .completed
            default:
                return row
            }
            if let permission = permissions[row.id] {
                // `detail` is a constant, so the row is rebuilt to carry what the permission is for.
                return Agent(id: agent.id, name: agent.name, project: agent.project, cwd: agent.cwd, group: .needsInput,
                             label: "Needs permission", detail: permission.call.title, since: agent.since, target: agent.target,
                             transcriptPath: agent.transcriptPath, host: agent.host)
            }
            agent.label = agent.group == .working ? "Working" : "Done"
            return agent
        }
    }

    // MARK: The conversation as notch lines

    /// A conversation as the notch's detail bubbles: your messages, the replies, and one dim line for the tools
    /// between them. Thinking, turn ends and notices stay in Chat.
    static func lines(from conversation: Conversation, limit: Int = 60) -> [ChatLine] {
        var out: [ChatLine] = []
        for item in conversation.items {
            switch item.kind {
            case .user(let text, _):
                out.append(ChatLine(id: out.count, kind: .user, text: text))
            case .assistant(let text):
                out.append(ChatLine(id: out.count, kind: .assistant, text: String(text.suffix(3000))))
            case .tools(let calls):
                guard !calls.isEmpty else { continue }
                out.append(ChatLine(id: out.count, kind: .tools, text: ForeignSessions.toolSummary(calls)))
            case .thinking, .turnEnd, .notice, .commandOutput:
                continue
            }
        }
        // Ids count from the start, so the tail keeps the ids it had and the list animates stably.
        return Array(out.suffix(limit))
    }

    static func snapshot(from conversation: Conversation) -> Snapshot {
        var snapshot = Snapshot()
        snapshot.lines = lines(from: conversation)
        snapshot.lastPrompt = snapshot.lines.last { $0.kind == .user }?.text
        snapshot.activity = conversation.activity
        return snapshot
    }

    // MARK: Permission options

    /// Antigravity's options in the order the notch lists them: the one-time allow first, other allows next,
    /// then the rejections, so the first row is the safe default a Return would take.
    static func orderedOptions(_ permission: ACPPermission) -> [ACPPermission.Option] {
        func rank(_ option: ACPPermission.Option) -> Int {
            switch option.kind {
            case "allow_once": return 0
            case "allow_always": return 1
            case "reject_once": return 3
            case "reject_always": return 4
            default: return option.allows ? 2 : 5
            }
        }
        return permission.options.enumerated().sorted {
            (rank($0.element), $0.offset) < (rank($1.element), $1.offset)
        }.map(\.element)
    }

    /// What the card shows of the call: the edit's diff, else the command or input, a few lines of it.
    static func preview(of permission: ACPPermission, maxLines: Int = 8) -> [(text: String, kind: DiffLine.Kind)] {
        let call = permission.call
        if !call.diff.isEmpty {
            return call.diff.prefix(maxLines).map { ($0.text, $0.kind) }
        }
        let text = call.input.isEmpty ? call.name : call.input
        return text.split(separator: "\n", omittingEmptySubsequences: false).prefix(maxLines).map { (String($0), DiffLine.Kind.context) }
    }

    static func permissionHeight(_ permission: ACPPermission) -> CGFloat {
        let lines = CGFloat(preview(of: permission).count)
        return 28 + 20 + 18 + lines * 15 + 22 + CGFloat(permission.options.count) * 36 + 28
    }

    // MARK: Live state (main thread)

    /// Antigravity permissions waiting, by row id.
    static func permissions(for rows: [Agent]) -> [String: ACPPermission] {
        let runner = AntigravityRunner.shared
        var out: [String: ACPPermission] = [:]
        for row in rows {
            guard case .antigravity(let id) = row.target, !id.isEmpty, let permission = runner.permission(id) else { continue }
            out[row.id] = permission
        }
        return out
    }

    /// The conversations of the Antigravity sessions already open in the runner, by row id. A session not open
    /// has none here: opening one starts an `agy acp` process, which the notch only does for a detail you open.
    static func agyConversations(for agents: [Agent]) -> [String: Conversation] {
        let runner = AntigravityRunner.shared
        var out: [String: Conversation] = [:]
        for agent in agents {
            guard case .antigravity(let id) = agent.target, !id.isEmpty, runner.isOpen(id),
                  let conversation = runner.peekConversation(id) else { continue }
            out[agent.id] = conversation
        }
        return out
    }

    /// The notch's snapshot of a Codex or Antigravity row (called off the main thread).
    static func snapshot(for agent: Agent, agy: [String: Conversation]) -> Snapshot {
        switch agent.target {
        case .codex:
            guard let path = agent.transcriptPath else { return Snapshot() }
            return snapshot(from: CodexReader.read(path))
        case .antigravity:
            return agy[agent.id].map(snapshot(from:)) ?? Snapshot()
        default:
            return Snapshot()
        }
    }

    /// The detail view opened on a session: an Antigravity one loads in its own `agy acp` process, whose history
    /// then flows into the snapshots.
    static func opened(_ agent: Agent) {
        guard case .antigravity(let id) = agent.target, !id.isEmpty else { return }
        AntigravityRunner.shared.open(id, folder: agent.cwd)
    }

    // MARK: Sending

    /// Runs the next turn the way Chat and the phone do. `done` gets nil once the turn is under way, or why not.
    /// One turn at a time: Chat queues what's typed meanwhile, the notch says to wait.
    static func send(_ text: String, to agent: Agent, done: @escaping (String?) -> Void) {
        switch agent.target {
        case .codex(let id):
            guard !id.isEmpty else { done("This Codex session hasn't started. Start it in Chat."); return }
            guard !text.hasPrefix("/") else { done("Codex's commands run in Chat. Open it there."); return }
            guard !CodexRunner.shared.isRunning(id) else { done("Codex is still on the last turn. Try again when it ends."); return }
            CodexRunner.shared.run(text, session: id, folder: agent.cwd, pick: ForeignSessionSource.codexPick(for: agent.id), started: { result in
                switch result {
                case .success: done(nil)
                case .failure(let error): done(error.localizedDescription)
                }
            }, ended: { _, failure in
                if let failure { NSLog("notch codex turn failed: %@", failure) }
                NotchForeignSource.shared.refreshSoon()
            })
        case .antigravity(let id):
            guard !id.isEmpty else { done("This Antigravity session hasn't started. Start it in Chat."); return }
            guard AntigravityRunner.binary() != nil else { done("Install Antigravity CLI first."); return }
            let runner = AntigravityRunner.shared
            guard !runner.isWorking(id) else { done("Antigravity is still on the last turn. Try again when it ends."); return }
            runner.prompt(id, text: text, folder: agent.cwd) { failure in
                if let failure { NSLog("notch agy turn failed: %@", failure) }
                NotchForeignSource.shared.refreshSoon()
            }
            done(runner.loadError(id))
        default:
            done("This session can't be replied to from here.")
        }
    }

    /// Answers an Antigravity permission with one of its own options; Chat's card sees it clear at once, since
    /// both read the runner.
    static func answer(_ option: ACPPermission.Option, for agent: Agent) {
        guard case .antigravity(let id) = agent.target else { return }
        AntigravityRunner.shared.answer(id, option: option.id)
    }
}

/// Where the notch gets Codex and Antigravity rows: Chat's own listing (`CodexSessions`, `AntigravitySessions`),
/// re-read from disk off the main thread at most every 3 s. `merge` lays the live state over the cached rows.
final class NotchForeignSource {
    static let shared = NotchForeignSource()

    private var rows: [Agent] = []
    private var listedAt = Date.distantPast
    private var listing = false
    /// Called on the main thread when a fresh listing differs from the last (a row appeared, changed group or title).
    var onChange: (() -> Void)?

    /// The cached rows; starts a re-read when they're 3 s old. Main thread.
    func current() -> [Agent] {
        refresh(force: false)
        return rows
    }

    func refreshSoon() { refresh(force: true) }

    private func refresh(force: Bool) {
        guard !listing, force || Date().timeIntervalSince(listedAt) > 3 else { return }
        listing = true
        let codexRunning = CodexRunner.shared.running
        let agyWorking = AntigravityRunner.shared.working
        DispatchQueue.global(qos: .utility).async { [weak self] in
            let list = CodexSessions.list(running: codexRunning)
                + (AntigravityRunner.binary() != nil ? AntigravitySessions.list(live: agyWorking) : [])
            DispatchQueue.main.async {
                guard let self else { return }
                self.listing = false
                self.listedAt = Date()
                let changed = list.map(\.id) != self.rows.map(\.id) || zip(list, self.rows).contains {
                    $0.group != $1.group || $0.since != $1.since || $0.name != $1.name
                }
                self.rows = list
                if changed { self.onChange?() }
            }
        }
    }
}

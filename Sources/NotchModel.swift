import AppKit
import SwiftUI

/// How wide the notch opens, picked in the menu.
enum NotchWidth: String, CaseIterable {
    case small, medium, large

    var title: String { rawValue.capitalized }
    /// The agent list and short peeks.
    var list: CGFloat { [.small: 300, .medium: 340, .large: 400][self]! }
    /// The conversation and the question and permission cards, which carry more text.
    var panel: CGFloat { [.small: 360, .medium: 420, .large: 520][self]! }
}

enum NotchState: Equatable {
    case collapsed
    case peek(String)    // an agent just replied or asked something; shown briefly
    case list
    case detail(String)  // one agent's conversation and reply box
}

final class NotchModel: ObservableObject {
    @Published var agents: [Agent] = [] { didSet { layoutChanged?() } }
    @Published private(set) var snapshots: [String: Snapshot] = [:] { didSet { layoutChanged?() } }
    /// Prompts a hook is holding open, by session id.
    @Published var requests: [String: HookRequest] = [:] { didSet { layoutChanged?() } }
    @Published private(set) var state: NotchState = .collapsed { didSet { layoutChanged?() } }
    /// Which of the three tabs under the camera is showing.
    @Published var tab: AgentGroup = .needsInput { didSet { layoutChanged?() } }
    @Published var draft = "" { didSet { if draft != oldValue { completionIndex = 0 } } }
    /// Highlighted row in the slash-command list.
    @Published var completionIndex = 0
    @Published var sending = false
    @Published var sendError: String?
    /// Answers picked so far for the open question, one per item.
    @Published private(set) var choices: [ReplySender.Choice] = [] { didSet { layoutChanged?() } }

    /// Size of the camera housing; zero width on screens without one.
    var notchSize = CGSize(width: 185, height: 32)
    /// Distance from the camera's centre to the screen's left and right edges; the pet roams that far.
    var screenSpan = (left: CGFloat(280), right: CGFloat(280))
    /// Called after anything that changes the panel's size.
    var layoutChanged: (() -> Void)?
    /// Asks the panel to take keyboard focus (for ⌘1…⌘9 and typing).
    var wantsKey: (() -> Void)?
    /// Hides everything in the Done tab until it changes again.
    var onClearCompleted: (() -> Void)?
    /// Hides one agent until it changes group; removes a background agent with `claude rm`.
    var onHide: ((Agent) -> Void)?
    var onRemove: ((Agent) -> Void)?
    /// While a menu is open the pointer can leave the shape without the notch folding away.
    var menuOpen = false

    /// Room for a count on each side of the camera, only while there's something to count.
    static let wingWidth: CGFloat = 20

    /// Real height of the question or permission card on show, measured as it lays out.
    @Published var cardHeight: CGFloat? { didSet { if cardHeight != oldValue { layoutChanged?() } } }
    @Published var width = NotchWidth(rawValue: UserDefaults.standard.string(forKey: "notchWidth") ?? "") ?? .medium {
        didSet { UserDefaults.standard.set(width.rawValue, forKey: "notchWidth"); layoutChanged?() }
    }

    @Published var showPet = UserDefaults.standard.object(forKey: "showPet") as? Bool ?? true {
        didSet { UserDefaults.standard.set(showPet, forKey: "showPet"); layoutChanged?() }
    }

    /// The pet rolls around the camera whenever any agent is working or waiting on you.
    var petWalking: Bool { showPet && workingCount + needsCount > 0 }
    static let flare: CGFloat = 8
    private var hovering = false
    private var pendingHover: DispatchWorkItem?
    private var pendingPeekEnd: DispatchWorkItem?
    /// Kept so the conversation stays readable if the agent drops off the list while open.
    private var detailSnapshot: Agent?
    /// Questions answered from here, hidden until the transcript catches up.
    private var answered = Set<String>()
    private var choicesFor: String?

    var needsCount: Int { agents.filter { $0.group == .needsInput }.count }
    var workingCount: Int { agents.filter { $0.group == .working }.count }

    func agent(_ id: String) -> Agent? {
        agents.first { $0.id == id } ?? (detailSnapshot?.id == id ? detailSnapshot : nil)
    }

    func snapshot(_ id: String) -> Snapshot { snapshots[id] ?? Snapshot() }

    func permission(_ id: String) -> HookRequest? {
        guard let r = requests[id], r.question == nil, !answered.contains(r.id) else { return nil }
        return r
    }

    /// Hook questions can be answered for any session; otherwise only background ones, by typing.
    func canAnswer(_ agent: Agent, _ q: PendingQuestion) -> Bool {
        q.answerable && (q.toolUseID.hasPrefix("hook:") || agent.canReply)
    }

    func question(_ id: String) -> PendingQuestion? {
        if let r = requests[id], let q = r.question, !answered.contains(r.id) { return q }
        guard let q = snapshots[id]?.question ?? agent(id).flatMap(Self.questionFromNeeds),
              !answered.contains(q.toolUseID) else { return nil }
        return q
    }

    /// A background job only writes the question into its transcript once it's answered. While it's
    /// open, its state carries a summary like "answer: Which target? (Production · Staging · Local)".
    private static func questionFromNeeds(_ agent: Agent) -> PendingQuestion? {
        guard agent.group == .needsInput, case .job = agent.target, let needs = agent.detail,
              needs.hasPrefix("answer: "), needs.hasSuffix(")"),
              let open = needs.range(of: " (", options: .backwards) else { return nil }
        let text = String(needs[needs.index(needs.startIndex, offsetBy: 8)..<open.lowerBound])
        let options = needs[open.upperBound..<needs.index(before: needs.endIndex)]
            .components(separatedBy: " · ").map { $0.trimmingCharacters(in: .whitespaces) }
        // Several questions come as "Q1 (…); Q2 (…)"; those go to the terminal.
        guard options.count >= 2, !text.contains("(") else { return nil }
        let item = PendingQuestion.Item(header: "", question: text, options: options, multiSelect: false)
        return PendingQuestion(toolUseID: "needs:\(agent.id):\(needs)", items: [item])
    }

    func go(_ new: NotchState) {
        guard new != state else { return }
        pendingPeekEnd?.cancel()
        if new == .list, state == .collapsed || isPeeking {
            // Open on the tab that most likely needs you.
            tab = AgentGroup.allCases.first { g in agents.contains { $0.group == g } } ?? .needsInput
        }
        withAnimation(.spring(response: 0.36, dampingFraction: 0.9)) { state = new }
    }

    private var isPeeking: Bool {
        if case .peek = state { return true }
        return false
    }

    // MARK: Transcripts

    /// Reads every agent's transcript off the main thread; unchanged files come from cache.
    func refreshSnapshots(_ done: @escaping (_ old: [String: Snapshot], _ new: [String: Snapshot]) -> Void) {
        let agents = self.agents
        DispatchQueue.global(qos: .utility).async { [weak self] in
            var fresh: [String: Snapshot] = [:]
            for agent in agents { fresh[agent.id] = TranscriptReader.snapshot(for: agent) }
            DispatchQueue.main.async {
                guard let self else { return }
                let old = self.snapshots
                if fresh != old { self.snapshots = fresh }
                done(old, fresh)
            }
        }
    }

    // MARK: Layout

    func rowHeight(_ agent: Agent) -> CGFloat {
        agent.group == .completed || snapshots[agent.id]?.lastPrompt == nil ? 44 : 58
    }

    func size(for state: NotchState) -> CGSize {
        let notch = notchSize
        let chrome = notch.height + 2 * Self.flare
        switch state {
        case .collapsed:
            // Exactly the camera housing; the pet and its count live outside it.
            return CGSize(width: notch.width + 2 * Self.flare, height: notch.height)
        case .peek(let id):
            if let r = permission(id) {
                return CGSize(width: width.panel, height: chrome + min(cardHeight ?? permissionHeight(r), 520) + 16)
            }
            if let q = question(id), let agent = agent(id) {
                return CGSize(width: width.panel,
                              height: chrome + min(cardHeight ?? questionHeight(q, canAnswer: canAnswer(agent, q)), 520) + 16)
            }
            let hasText = snapshots[id]?.lastReply != nil || agent(id)?.detail != nil
            return CGSize(width: width.list, height: chrome + (hasText ? 100 : 56))
        case .list:
            let rows = agents.filter { $0.group == tab }
            let content = rows.isEmpty ? 44 : rows.reduce(CGFloat(tab == .completed ? 28 : 0)) { $0 + rowHeight($1) + 4 }
            return CGSize(width: width.list, height: chrome + 34 + min(content, 300) + 6)
        case .detail:
            if case .detail(let id) = state, question(id) != nil || permission(id) != nil {
                return CGSize(width: width.panel, height: chrome + 470)
            }
            return CGSize(width: width.panel, height: chrome + 420)
        }
    }

    func questionHeight(_ q: PendingQuestion, canAnswer: Bool) -> CGFloat {
        let item = q.items[min(choices.count, q.items.count - 1)]
        let lines = CGFloat(min(3, item.question.count / 60 + 1))
        return 28 + lines * 17 + CGFloat(item.options.count) * 36 + 40 + 8
    }

    func permissionHeight(_ r: HookRequest) -> CGFloat {
        let lines = CGFloat(min(4, r.summary.count / 62 + 1))
        return 28 + 20 + lines * 16 + 14 + CGFloat(r.canAlwaysAllow ? 3 : 2) * 36 + 28
    }

    // MARK: Hover and peeking

    func setHover(_ inside: Bool) {
        hovering = inside
        pendingHover?.cancel()
        let work: DispatchWorkItem
        if inside {
            switch state {
            case .collapsed: work = DispatchWorkItem { [weak self] in self?.go(.list) }
            case .peek(let id):
                pendingPeekEnd?.cancel()
                if question(id) != nil || permission(id) != nil { wantsKey?() }
                return
            default: return
            }
            pendingHover = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.25, execute: work)
        } else {
            switch state {
            case .list, .peek: work = DispatchWorkItem { [weak self] in
                if self?.hovering == false, self?.menuOpen == false { self?.go(.collapsed) }
            }
            default: return
            }
            pendingHover = work
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6, execute: work)
        }
    }

    /// Slides out a banner for an agent that just replied or asked something.
    func peek(_ agent: Agent) {
        guard state == .collapsed || isPeeking else { return }
        cardHeight = nil
        resetChoices(for: agent.id)
        go(.peek(agent.id))
        let end = DispatchWorkItem { [weak self] in
            guard let self, case .peek = self.state, !self.hovering else { return }
            self.go(.collapsed)
        }
        pendingPeekEnd = end
        // A question stays out longer: it's waiting on you.
        let waiting = question(agent.id) != nil || permission(agent.id) != nil
        DispatchQueue.main.asyncAfter(deadline: .now() + (waiting ? 45 : 8), execute: end)
    }

    // MARK: Conversation

    func open(_ id: String) {
        detailSnapshot = agents.first { $0.id == id }
        draft = ""
        sendError = nil
        resetChoices(for: id)
        go(.detail(id))
    }

    func send(_ preset: String? = nil) {
        guard case .detail(let id) = state, let agent = agent(id), case .job(let jobID) = agent.target, !sending else { return }
        let text = (preset ?? draft).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        if preset == nil { draft = "" }
        run(id: id, restoreDraft: preset == nil ? text : nil) {
            try ReplySender.send(text, toJob: jobID)
        }
    }

    // MARK: Slash commands and suggestions

    /// Commands matching a draft like "/sd:ap"; empty once there's a space after the command.
    func completions(for agent: Agent) -> [SlashCommand] {
        guard draft.hasPrefix("/"), !draft.contains(where: \.isWhitespace) else { return [] }
        return SlashCommands.matches(String(draft.dropFirst()), in: SlashCommands.all(for: agent.cwd))
    }

    /// The reply the agent suggested, offered while the box is empty.
    func suggestion(for agent: Agent) -> String? {
        guard draft.isEmpty, let s = agent.suggestedReply, !s.isEmpty else { return nil }
        return s
    }

    /// Tab, arrows and Return while typing a reply. True when the key was used.
    func handleReplyKey(_ keyCode: UInt16) -> Bool {
        guard case .detail(let id) = state, let agent = agent(id), agent.canReply,
              question(id) == nil, permission(id) == nil else { return false }
        let list = completions(for: agent)
        switch keyCode {
        case 48: // Tab
            if !list.isEmpty {
                accept(list[min(completionIndex, list.count - 1)])
            } else if let s = suggestion(for: agent) {
                draft = s
            }
            return true
        case 125 where !list.isEmpty: // Down
            completionIndex = (completionIndex + 1) % list.count
            return true
        case 126 where !list.isEmpty: // Up
            completionIndex = (completionIndex + list.count - 1) % list.count
            return true
        case 36, 76: // Return: finish the command name first, send on the next press
            guard !list.isEmpty else { return false }
            let pick = list[min(completionIndex, list.count - 1)]
            if "/" + pick.name == draft { return false }
            accept(pick)
            return true
        default:
            return false
        }
    }

    func accept(_ command: SlashCommand) {
        draft = "/" + command.name + (command.argumentHint == nil ? "" : " ")
    }

    // MARK: Questions

    private func resetChoices(for id: String) {
        let q = question(id)?.toolUseID
        if choicesFor != q { choices = []; choicesFor = q }
    }

    /// Records one answer; once every item has one, types them all into the session.
    func choose(_ choice: ReplySender.Choice, for id: String) {
        guard let agent = agent(id), let q = question(id), canAnswer(agent, q), !sending else { return }
        resetChoices(for: id)
        withAnimation(.easeOut(duration: 0.2)) { choices.append(choice) }
        guard choices.count >= q.items.count else { return }
        let picked = choices

        if let request = requests[id], request.question?.toolUseID == q.toolUseID {
            // The hook is holding the question open: hand it the answers directly.
            var answers: [String: String] = [:]
            for (item, choice) in zip(q.items, picked) {
                switch choice {
                case .option(let n): answers[item.question] = item.options[n - 1]
                case .text(let t): answers[item.question] = t
                }
            }
            answered.insert(request.id)
            HookBridge.respond(to: request, ["action": "answer", "answers": answers])
        } else if case .job(let jobID) = agent.target {
            answered.insert(q.toolUseID)
            run(id: id, restoreDraft: nil, onFailure: { [weak self] in
                self?.answered.remove(q.toolUseID)
                self?.choices = []
            }) {
                try ReplySender.answer(picked, to: q, toJob: jobID)
            }
        }
        foldPeek()
    }

    enum Decision { case allow, allowAlways, deny, terminal }

    /// Answers a permission prompt, or hands it back to the terminal and jumps there.
    func decide(_ decision: Decision, for id: String) {
        guard let request = requests[id] else { return }
        answered.insert(request.id)
        switch decision {
        case .allow: HookBridge.respond(to: request, ["action": "allow"])
        case .allowAlways: HookBridge.respond(to: request, ["action": "allow", "always": true])
        case .deny: HookBridge.respond(to: request, ["action": "deny"])
        case .terminal:
            HookBridge.respond(to: request, ["action": "pass"])
            if let agent = agent(id) { AgentActions.open(agent.target, cwd: agent.cwd) }
        }
        foldPeek()
    }

    /// Hands a hook question back to the terminal prompt and jumps there.
    func answerInTerminal(_ id: String) {
        if let request = requests[id], request.question != nil {
            answered.insert(request.id)
            HookBridge.respond(to: request, ["action": "pass"])
        }
        if let agent = agent(id) { AgentActions.open(agent.target, cwd: agent.cwd) }
        foldPeek()
    }

    /// Answered straight from the banner; fold it away.
    private func foldPeek() {
        guard isPeeking else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
            if self?.hovering == false { self?.go(.collapsed) }
        }
    }

    func undoChoice() {
        if !choices.isEmpty { choices.removeLast() }
    }

    private func run(id: String, restoreDraft: String?, onFailure: (() -> Void)? = nil, _ work: @escaping () throws -> Void) {
        sending = true
        sendError = nil
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            var failure: Error?
            do { try work() } catch { failure = error }
            DispatchQueue.main.async {
                guard let self else { return }
                self.sending = false
                if let failure {
                    self.sendError = failure.localizedDescription
                    if let restoreDraft, self.draft.isEmpty { self.draft = restoreDraft }
                    onFailure?()
                }
            }
        }
    }
}

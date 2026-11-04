import AppKit
import SwiftUI

// The Chat window's agent parts: tool steps, thinking, tasks, turn footers, notices, the inspector.
// Built from the kit (ChatKit.swift); spec in docs/chat-kit.md.

/// Which tool steps and calls are open. Kept outside the rows so a refresh (or a lazy row coming
/// back on screen) doesn't fold them.
final class ChatExpansion: ObservableObject {
    @Published var open: Set<String> = []

    func toggle(_ id: String) {
        withAnimation(KitMotion.appear) {
            if open.contains(id) { open.remove(id) } else { open.insert(id) }
        }
    }
}

extension ToolCall.Kind {
    var symbol: String {
        switch self {
        case .shell: return "terminal"
        case .read: return "doc.text"
        case .edit: return "pencil"
        case .write: return "doc.badge.plus"
        case .search: return "magnifyingglass"
        case .web: return "globe"
        case .agent: return "person.2"
        case .task: return "checklist"
        case .ask: return "questionmark.bubble"
        case .skill: return "sparkles"
        case .other: return "puzzlepiece.extension"
        }
    }
}

enum ToolSummary {
    /// "Ran 2 commands, edited 3 files, searched 1 time", like the terminal's collapsed line.
    static func line(_ calls: [ToolCall]) -> String {
        var order: [String] = []
        var counts: [String: Int] = [:]
        var files: [String: Set<String>] = [:]
        for call in calls {
            let key: String
            switch call.kind {
            case .shell: key = "ran|command|commands"
            case .read: key = "read|file|files"
            case .edit: key = "edited|file|files"
            case .write: key = "wrote|file|files"
            case .search: key = "searched|time|times"
            case .web: key = "browsed|page|pages"
            case .agent: key = "ran|agent|agents"
            case .task: key = "updated|task|tasks"
            case .ask: key = "asked|question|questions"
            case .skill: key = "used|skill|skills"
            case .other: key = "used|tool|tools"
            }
            if counts[key] == nil { order.append(key) }
            counts[key, default: 0] += 1
            if [.read, .edit, .write].contains(call.kind) { files[key, default: []].insert(call.input) }
        }
        let text = order.map { key -> String in
            let p = key.split(separator: "|").map(String.init)
            let n = files[key]?.count ?? counts[key] ?? 0
            return "\(p[0]) \(n) \(n == 1 ? p[1] : p[2])"
        }.joined(separator: ", ")
        return text.prefix(1).uppercased() + text.dropFirst()
    }
}

/// A run of tool calls between two messages. Folded to one line; open, each call is its own row.
struct ToolStepView: View {
    let id: String
    let calls: [ToolCall]
    /// The newest step of a working session: open, with the running call spinning.
    let live: Bool
    @ObservedObject var expansion: ChatExpansion

    var body: some View {
        let open = live || expansion.open.contains(id)
        let errors = calls.filter(\.isError).count
        VStack(alignment: .leading, spacing: 0) {
            Button { expansion.toggle(id) } label: {
                HStack(spacing: 8) {
                    HStack(spacing: -4) {
                        ForEach(Array(uniqueKinds.prefix(4).enumerated()), id: \.offset) { _, kind in
                            Image(systemName: kind.symbol)
                                .font(.system(size: 9, weight: .semibold))
                                .foregroundColor(KitInk.textMuted)
                                .frame(width: 20, height: 20)
                                .background(Circle().fill(KitInk.card))
                                .overlay(Circle().strokeBorder(KitInk.hairline, lineWidth: 1))
                        }
                    }
                    Text(ToolSummary.line(calls)).kitType(.label).foregroundColor(KitInk.textMuted).lineLimit(1)
                    if errors > 0 {
                        Text(errors == 1 ? "1 failed" : "\(errors) failed").kitType(.label).foregroundColor(.red)
                    }
                    if live { ProgressView().controlSize(.mini) }
                    Image(systemName: "chevron.right")
                        .font(.system(size: 9, weight: .bold))
                        .foregroundColor(KitInk.textFaint)
                        .rotationEffect(.degrees(open ? 90 : 0))
                }
                .padding(.leading, 4).padding(.trailing, 10).frame(height: 28)
                .background(Capsule().fill(KitInk.raised.opacity(0.7)))
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .pointerCursor()
            .help(open ? "Hide the tool calls" : "Show each tool call")

            if open {
                VStack(alignment: .leading, spacing: 4) {
                    ForEach(calls) { call in
                        ToolCallRow(call: call, running: live && !call.finished, expansion: expansion)
                    }
                }
                .padding(.top, 8)
                .padding(.leading, 12)
                .overlay(alignment: .leading) {
                    Rectangle().fill(KitInk.hairline).frame(width: 1).padding(.top, 8)
                }
                .transition(.opacity)
            }
        }
    }

    private var uniqueKinds: [ToolCall.Kind] {
        var seen: [ToolCall.Kind] = []
        for c in calls where !seen.contains(c.kind) { seen.append(c.kind) }
        return seen
    }
}

/// One tool call: what it did, its status, and when opened its input, output or diff.
struct ToolCallRow: View {
    let call: ToolCall
    let running: Bool
    @ObservedObject var expansion: ChatExpansion
    @State private var hovering = false

    var body: some View {
        let open = expansion.open.contains(call.id)
        VStack(alignment: .leading, spacing: 6) {
            Button { if hasDetail { expansion.toggle(call.id) } } label: {
                HStack(spacing: 8) {
                    Image(systemName: call.kind.symbol)
                        .font(.system(size: 11, weight: .medium))
                        .foregroundColor(call.isError ? .red : KitInk.textMuted)
                        .frame(width: 16)
                    Text(call.title).kitType(.body).foregroundColor(KitInk.text).lineLimit(1)
                    if call.added + call.removed > 0 {
                        Text("+\(call.added)").kitType(.mono).foregroundColor(.green)
                        Text("−\(call.removed)").kitType(.mono).foregroundColor(.red)
                    }
                    Spacer(minLength: 8)
                    if running {
                        ProgressView().controlSize(.mini)
                    } else if call.isError {
                        Image(systemName: "xmark.octagon.fill").font(.system(size: 11)).foregroundColor(.red).help("This call failed")
                    }
                    if hasDetail {
                        Image(systemName: "chevron.right")
                            .font(.system(size: 9, weight: .bold)).foregroundColor(KitInk.textFaint)
                            .rotationEffect(.degrees(open ? 90 : 0))
                    }
                }
                .padding(.horizontal, 8).frame(minHeight: 28)
                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                    .fill(hovering && hasDetail ? KitInk.raised.opacity(0.6) : .clear))
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .onHover { hovering = $0 }
            .help(call.input.isEmpty ? call.title : String(call.input.prefix(300)))

            if open {
                VStack(alignment: .leading, spacing: 6) {
                    if !call.diff.isEmpty {
                        DiffView(path: call.input, lines: call.diff)
                    } else if !call.input.isEmpty, call.kind != .read {
                        MonoBox(title: call.kind == .shell ? "Command" : "Input", text: call.input, tone: .plain)
                    }
                    if let output = call.output, !output.isEmpty {
                        MonoBox(title: call.isError ? "Error" : "Output", text: output, tone: call.isError ? .error : .plain)
                    }
                }
                .padding(.leading, 24)
                .transition(.opacity)
            }
        }
    }

    private var hasDetail: Bool {
        !call.diff.isEmpty || (call.output.map { !$0.isEmpty } ?? false) || (!call.input.isEmpty && call.kind != .read)
    }
}

/// Monospaced text in a box, with Copy; red-tinted for errors.
struct MonoBox: View {
    enum Tone { case plain, error }
    let title: String
    let text: String
    let tone: Tone

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(title).kitType(.meta).foregroundColor(tone == .error ? .red : KitInk.textMuted)
                Spacer()
                CopyButton(text: text)
            }
            .padding(.leading, 10).padding(.trailing, 4).frame(height: 26)
            ScrollView(.vertical) {
                Text(text).kitType(.mono).foregroundColor(KitInk.text)
                    .textSelection(.enabled)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 10).padding(.bottom, 10)
            }
            .frame(maxHeight: 260)
            .fixedSize(horizontal: false, vertical: text.split(separator: "\n").count < 14)
        }
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            .fill(tone == .error ? Color.red.opacity(0.08) : KitInk.raised.opacity(0.7)))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous)
            .strokeBorder(tone == .error ? Color.red.opacity(0.3) : KitInk.hairline, lineWidth: 1))
    }
}

/// A file change: line numbers, green additions, red removals.
struct DiffView: View {
    let path: String
    let lines: [DiffLine]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 6) {
                Image(systemName: "doc.text").font(.system(size: 10))
                Text((path as NSString).lastPathComponent).kitType(.label)
                Text((path as NSString).deletingLastPathComponent.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                    .kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1).truncationMode(.head)
                Spacer()
                Button { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) } label: {
                    Image(systemName: "folder").font(.system(size: 10, weight: .medium))
                }
                .buttonStyle(.kit(.ghost))
                .help("Show the file in Finder")
            }
            .foregroundColor(KitInk.text)
            .padding(.leading, 10).padding(.trailing, 4).frame(height: 28)
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            ScrollView(.vertical) {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(lines.enumerated()), id: \.offset) { _, line in
                        row(line)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 4)
            }
            .frame(maxHeight: 320)
            .fixedSize(horizontal: false, vertical: lines.count < 16)
        }
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.card))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
        .clipShape(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous))
    }

    @ViewBuilder private func row(_ line: DiffLine) -> some View {
        if line.kind == .hunk {
            Text(line.text).kitType(.meta).foregroundColor(KitInk.textFaint)
                .padding(.horizontal, 10).padding(.vertical, 3)
        } else {
            HStack(spacing: 0) {
                Text(line.number.map(String.init) ?? "")
                    .font(.system(size: 11, design: .monospaced)).foregroundColor(KitInk.textFaint)
                    .frame(width: 40, alignment: .trailing)
                Text(line.kind == .add ? " + " : line.kind == .remove ? " − " : "   ")
                    .font(.system(size: 11.5, weight: .bold, design: .monospaced))
                    .foregroundColor(line.kind == .add ? .green : line.kind == .remove ? .red : KitInk.textFaint)
                Text(line.text.isEmpty ? " " : line.text)
                    .font(.system(size: 11.5, design: .monospaced)).foregroundColor(KitInk.text)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.trailing, 10)
            }
            .padding(.vertical, 1)
            .background(line.kind == .add ? Color.green.opacity(0.12) : line.kind == .remove ? Color.red.opacity(0.12) : .clear)
        }
    }
}

/// "Thought for 12s": all of a turn's thinking in one row. Claude Code keeps the text only sometimes; when it does, it opens.
struct ThinkingRow: View {
    let id: String
    let seconds: Double
    let text: String
    @ObservedObject var expansion: ChatExpansion

    var body: some View {
        let open = expansion.open.contains(id)
        VStack(alignment: .leading, spacing: 6) {
            Button { if !text.isEmpty { expansion.toggle(id) } } label: {
                HStack(spacing: 6) {
                    Image(systemName: "brain").font(.system(size: 10, weight: .medium))
                    Text(seconds >= 1 ? "Thought for \(ChatFormat.duration(seconds))" : "Thought briefly").kitType(.label)
                    if !text.isEmpty {
                        Image(systemName: "chevron.right").font(.system(size: 8, weight: .bold))
                            .rotationEffect(.degrees(open ? 90 : 0))
                    }
                }
                .foregroundColor(KitInk.textFaint)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if open {
                HStack(alignment: .top, spacing: 10) {
                    Rectangle().fill(KitInk.hairline).frame(width: 2)
                    Text(text).kitType(.meta).foregroundColor(KitInk.textMuted)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                }
            }
        }
    }
}

/// The session's task list with progress, pinned above the conversation.
struct TaskPanel: View {
    let tasks: [TaskEntry]
    @ObservedObject var expansion: ChatExpansion

    var body: some View {
        let done = tasks.filter { $0.status == .completed }.count
        let current = tasks.first { $0.status == .inProgress }
        // Pinned above the conversation, so folded to one line until opened.
        let open = expansion.open.contains("tasks-open")
        VStack(alignment: .leading, spacing: 8) {
            Button { expansion.toggle("tasks-open") } label: {
                HStack(spacing: 8) {
                    Image(systemName: "checklist").font(.system(size: 12, weight: .medium)).foregroundColor(KitInk.accentText)
                    Text("Tasks \(done)/\(tasks.count)").kitType(.heading).foregroundColor(KitInk.text)
                    if let current {
                        Text("Now: \(current.subject)").kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                    } else if done == tasks.count {
                        Text("All done").kitType(.meta).foregroundColor(KitInk.done)
                    }
                    Spacer()
                    ProgressBar(value: tasks.isEmpty ? 0 : Double(done) / Double(tasks.count)).frame(width: 90)
                    Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold)).foregroundColor(KitInk.textFaint)
                        .rotationEffect(.degrees(open ? 90 : 0))
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .pointerCursor()
            if open {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(tasks) { task in
                        HStack(alignment: .firstTextBaseline, spacing: 8) {
                            TaskStatusIcon(status: task.status)
                            Text(task.subject)
                                .kitType(.body)
                                .foregroundColor(task.status == .completed ? KitInk.textFaint : KitInk.text)
                                .strikethrough(task.status == .completed, color: KitInk.textFaint)
                                .fontWeight(task.status == .inProgress ? .semibold : .regular)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
            }
        }
        .padding(.horizontal, 14).padding(.vertical, 10)
        .kitCard()
    }
}

struct TaskStatusIcon: View {
    let status: TaskEntry.Status

    var body: some View {
        switch status {
        case .completed:
            Image(systemName: "checkmark.circle.fill").foregroundColor(KitInk.done)
        case .inProgress:
            Image(systemName: "circle.lefthalf.filled").foregroundColor(KitInk.accent)
        case .pending:
            Image(systemName: "circle").foregroundColor(KitInk.textFaint)
        }
    }
}

struct ProgressBar: View {
    let value: Double

    var body: some View {
        GeometryReader { geo in
            ZStack(alignment: .leading) {
                Capsule().fill(KitInk.raised)
                Capsule().fill(KitInk.accent).frame(width: max(4, geo.size.width * value))
            }
        }
        .frame(height: 5)
        .accessibilityValue("\(Int(value * 100)) percent")
    }
}

/// "Worked for 1m 12s · claude-opus-5-5" between turns.
struct TurnFooter: View {
    let seconds: Double
    let model: String?

    var body: some View {
        HStack(spacing: 10) {
            Rectangle().fill(KitInk.hairline).frame(height: 1)
            HStack(spacing: 5) {
                Image(systemName: "clock").font(.system(size: 9, weight: .medium))
                Text(["Worked for \(ChatFormat.duration(seconds))", model.map(ChatFormat.modelName)].compactMap { $0 }.joined(separator: " · "))
                    .kitType(.meta)
            }
            .foregroundColor(KitInk.textFaint)
            .fixedSize()
            Rectangle().fill(KitInk.hairline).frame(height: 1)
        }
        .padding(.vertical, 2)
    }
}

struct DaySeparator: View {
    let date: Date

    var body: some View {
        Text(ChatFormat.day(date))
            .kitType(.section).foregroundColor(KitInk.textFaint)
            .padding(.horizontal, 10).frame(height: 20)
            .background(Capsule().fill(KitInk.raised.opacity(0.7)))
            .frame(maxWidth: .infinity)
            .padding(.vertical, 6)
    }
}

/// What a slash command printed, in the terminal's monospace so tables like /context line up.
struct CommandOutputView: View {
    let text: String

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(text).kitType(.mono).foregroundColor(KitInk.textMuted)
                .textSelection(.enabled)
                .fixedSize(horizontal: true, vertical: false)
                .padding(.horizontal, 12).padding(.vertical, 8)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.raised.opacity(0.5)))
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
    }
}

struct NoticeRow: View {
    let text: String
    let symbol: String

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: symbol).font(.system(size: 10, weight: .medium))
            Text(text).kitType(.label)
        }
        .foregroundColor(KitInk.textMuted)
        .padding(.horizontal, 10).frame(height: 22)
        .overlay(Capsule().strokeBorder(KitInk.hairline, lineWidth: 1))
        .frame(maxWidth: .infinity)
    }
}

/// The session's receipt beside the conversation: where it runs, what it used, what it changed.
struct SessionInspector: View {
    let agent: Agent
    let conversation: Conversation
    /// The folder's working copy, read every few seconds while the panel is open.
    @State private var git: GitInfo?

    var body: some View {
        let facts = conversation.facts
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                section("Session") {
                    fact("folder", agent.cwd.replacingOccurrences(of: NSHomeDirectory(), with: "~"))
                    if git == nil, let branch = facts.branch { fact("arrow.triangle.branch", branch) }
                    if let model = facts.model { fact("cpu", ChatFormat.modelName(model)) }
                    fact(agent.canReply ? "moon.stars" : "terminal", agent.host)
                }
                if let git { gitSection(git).part("Git Status", .leading) }
                section("Activity") {
                    fact("arrow.left.arrow.right", facts.turns == 1 ? "1 turn" : "\(facts.turns) turns")
                    fact("terminal", facts.commands == 1 ? "1 command" : "\(facts.commands) commands")
                    fact("text.word.spacing", "\(ChatFormat.tokens(facts.inputTokens)) in · \(ChatFormat.tokens(facts.outputTokens)) out")
                }
                if !conversation.tasks.isEmpty {
                    let done = conversation.tasks.filter { $0.status == .completed }.count
                    section("Tasks · \(done)/\(conversation.tasks.count)") {
                        ProgressBar(value: Double(done) / Double(conversation.tasks.count))
                        ForEach(conversation.tasks) { task in
                            HStack(alignment: .firstTextBaseline, spacing: 6) {
                                TaskStatusIcon(status: task.status).font(.system(size: 10))
                                Text(task.subject).kitType(.meta)
                                    .foregroundColor(task.status == .completed ? KitInk.textFaint : KitInk.text)
                                    .lineLimit(2)
                            }
                        }
                    }
                }
                if !facts.files.isEmpty {
                    section("Edited in This Session · \(facts.files.count)") {
                        ForEach(facts.files.suffix(40), id: \.self) { path in
                            Button { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: path)]) } label: {
                                HStack(spacing: 6) {
                                    Image(systemName: "doc").font(.system(size: 10)).foregroundColor(KitInk.textFaint)
                                    Text((path as NSString).lastPathComponent).kitType(.meta).foregroundColor(KitInk.text).lineLimit(1)
                                    Spacer(minLength: 0)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                            .pointerCursor()
                            .help(path)
                        }
                    }
                }
            }
            .padding(16)
        }
        .background(KitInk.sidebar)
        .task(id: agent.cwd) {
            let folder = agent.cwd
            while !Task.isCancelled {
                let fresh = await Task.detached(priority: .utility) { GitInfo.read(folder) }.value
                if fresh != git { git = fresh }
                try? await Task.sleep(nanoseconds: 5_000_000_000)
            }
        }
    }

    @ViewBuilder private func gitSection(_ git: GitInfo) -> some View {
        section(git.changes.isEmpty ? "Git" : "Git · \(git.changes.count) uncommitted") {
            HStack(alignment: .firstTextBaseline, spacing: 8) {
                Image(systemName: "arrow.triangle.branch").font(.system(size: 10, weight: .medium))
                    .foregroundColor(KitInk.textFaint).frame(width: 14)
                Text(git.branch ?? "unknown branch").kitType(.meta).foregroundColor(KitInk.text)
                    .lineLimit(2).textSelection(.enabled)
                if git.ahead > 0 { Text("↑\(git.ahead)").kitType(.meta).foregroundColor(KitInk.working).monospacedDigit() }
                if git.behind > 0 { Text("↓\(git.behind)").kitType(.meta).foregroundColor(KitInk.needs).monospacedDigit() }
            }
            .help(git.upstream.map { "Tracks \($0)" + (git.ahead + git.behind > 0 ? ": \(git.ahead) to push, \(git.behind) to pull" : ", up to date") }
                  ?? "No upstream branch")
            if let commit = git.lastCommit {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Image(systemName: "smallcircle.filled.circle").font(.system(size: 10, weight: .medium))
                        .foregroundColor(KitInk.textFaint).frame(width: 14)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(commit).kitType(.meta).foregroundColor(KitInk.text).lineLimit(2).textSelection(.enabled)
                        if let age = git.lastCommitAge { Text(age).kitType(.meta).foregroundColor(KitInk.textFaint) }
                    }
                }
            }
            if git.changes.isEmpty {
                fact("checkmark.circle", "No uncommitted changes")
            } else {
                ForEach(git.changes.prefix(60)) { change in gitRow(change) }
                if git.changes.count > 60 {
                    Text("and \(git.changes.count - 60) more").kitType(.meta).foregroundColor(KitInk.textFaint)
                }
            }
        }
    }

    private func gitRow(_ change: GitInfo.Change) -> some View {
        let full = (agent.cwd as NSString).appendingPathComponent(change.path)
        let folder = (change.path as NSString).deletingLastPathComponent
        return Button {
            NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: full)])
        } label: {
            HStack(alignment: .firstTextBaseline, spacing: 6) {
                Text(change.status).font(.system(size: 10, weight: .bold, design: .monospaced))
                    .foregroundColor(Self.statusColor(change.status)).frame(width: 14)
                VStack(alignment: .leading, spacing: 1) {
                    Text((change.path as NSString).lastPathComponent).kitType(.meta)
                        .foregroundColor(change.status == "D" ? KitInk.textFaint : KitInk.text)
                        .strikethrough(change.status == "D").lineLimit(1).truncationMode(.middle)
                    if !folder.isEmpty {
                        Text(folder).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1).truncationMode(.head)
                    }
                }
                Spacer(minLength: 4)
                if let added = change.added, let removed = change.removed, added + removed > 0 {
                    HStack(spacing: 4) {
                        if added > 0 { Text("+\(added)").foregroundColor(KitInk.working) }
                        if removed > 0 { Text("−\(removed)").foregroundColor(KitInk.needs) }
                    }
                    .kitType(.meta).monospacedDigit()
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .pointerCursor()
        .help(Self.statusName(change.status) + ": " + change.path)
    }

    private static func statusColor(_ status: String) -> Color {
        switch status {
        case "A", "?": return KitInk.working
        case "D", "U": return KitInk.needs
        default: return KitInk.textMuted
        }
    }

    private static func statusName(_ status: String) -> String {
        switch status {
        case "A": return "Added"
        case "D": return "Deleted"
        case "R": return "Renamed"
        case "?": return "New, not tracked yet"
        case "U": return "Conflict"
        default: return "Modified"
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder _ content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title).kitType(.section).foregroundColor(KitInk.textFaint)
            content()
        }
    }

    private func fact(_ symbol: String, _ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Image(systemName: symbol).font(.system(size: 10, weight: .medium)).foregroundColor(KitInk.textFaint).frame(width: 14)
            Text(text).kitType(.meta).foregroundColor(KitInk.text).lineLimit(2).textSelection(.enabled)
        }
    }
}

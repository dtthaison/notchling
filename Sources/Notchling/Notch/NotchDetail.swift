import SwiftUI

struct DetailView: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    @FocusState private var focused: Bool
    /// Leaves the header and a few lines of conversation above a long question.
    static let questionBodyLimit: CGFloat = 230

    var body: some View {
        let question = model.question(agent.id)
        let permission = model.permission(agent.id)
        VStack(alignment: .leading, spacing: 10) {
            header
            conversation(showNeeds: question == nil && permission == nil)
            // Cards keep their full height and the conversation above gives way; squeezed, the
            // wrapped option text spills out of its buttons and overlaps.
            if let permission {
                PermissionCard(model: model, agent: agent, request: permission).part("Notch Permission Card", .topTrailing)
                    .fixedSize(horizontal: false, vertical: true)
                    .layoutPriority(1)
            } else if let question {
                QuestionCard(model: model, agent: agent, question: question, bodyLimit: Self.questionBodyLimit).part("Notch Question Card", .topTrailing)
                    .fixedSize(horizontal: false, vertical: true)
                    .layoutPriority(1)
            } else if model.canReply(agent) {
                replyBox.part("Notch Reply Box", .trailing)
            } else {
                HStack {
                    Text(agent.label == "Exited" ? "This session has exited. Open it to resume."
                         : agent.canReply ? "Open it in a terminal to reply there."
                                          : "This session runs in \(agent.host). Open it to reply there.")
                        .font(.system(size: 11)).foregroundColor(.secondary)
                    Spacer()
                    Button("Open") { AgentActions.open(agent.target, cwd: agent.cwd) }
                }
            }
        }
        .padding(.horizontal, 12)
        .padding(.top, 4)
        .padding(.bottom, 12)
        .onExitCommand { model.go(.list) }
    }

    private var header: some View {
        HStack(spacing: 10) {
            Button { model.go(.list) } label: { Image(systemName: "chevron.left") }
                .buttonStyle(.kitPlain).foregroundColor(.secondary)
            Circle().fill(agent.color).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 1) {
                Text(agent.name).font(.system(size: 13, weight: .semibold)).lineLimit(1)
                Text("\(agent.label) · \(agent.host) · \(agent.ago)")
                    .font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
            }
            Spacer()
            Button {
                model.go(.collapsed)
                model.onOpenChat?(agent.id)
            } label: {
                Image(systemName: "bubble.left.and.bubble.right")
            }
            .buttonStyle(.kitPlain).foregroundColor(.secondary).help("Open in Chat window")
            Button { AgentActions.open(agent.target, cwd: agent.cwd) } label: {
                Image(systemName: "arrow.up.forward.app")
            }
            .buttonStyle(.kitPlain).foregroundColor(.secondary).help("Open in terminal")
            Button { model.go(.collapsed) } label: { Image(systemName: "xmark") }
                .buttonStyle(.kitPlain).foregroundColor(.secondary)
        }
        .part("Notch Detail Header", .bottomTrailing)
    }

    private func conversation(showNeeds: Bool) -> some View {
        let lines = model.snapshot(agent.id).lines
        return ScrollViewReader { proxy in
            ScrollView {
                VStack(alignment: .leading, spacing: 10) {
                    if lines.isEmpty {
                        Text("No messages yet").font(.system(size: 12)).foregroundColor(.secondary)
                    }
                    ForEach(lines) { line in Bubble(line: line) }.part("Message Line", .trailing)
                    if showNeeds, agent.group == .needsInput, let needs = agent.detail, !needs.isEmpty {
                        HStack(alignment: .top, spacing: 8) {
                            Image(systemName: "hand.raised.fill").foregroundColor(.orange)
                            Text(needs).font(.system(size: 12)).textSelection(.enabled)
                        }
                        .padding(10)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: 10).fill(Color.orange.opacity(0.15)))
                    }
                    Color.clear.frame(height: 1).id("bottom")
                }
                .padding(.vertical, 4)
                // Room for the overlay scroll bar, so it never sits on top of the text.
                .padding(.trailing, 12)
            }
            .onAppear { proxy.scrollTo("bottom", anchor: .bottom) }
            .onChange(of: lines) { _ in
                withAnimation { proxy.scrollTo("bottom", anchor: .bottom) }
            }
        }
    }

    private var replyBox: some View {
        let completions = model.completions(for: agent)
        let suggestion = model.suggestion(for: agent)
        return VStack(alignment: .leading, spacing: 6) {
            if !completions.isEmpty {
                CommandList(model: model, commands: completions).part("Notch Command List", .topTrailing)
            }
            if completions.isEmpty, let suggested = agent.suggestedReply, !suggested.isEmpty, agent.group == .needsInput {
                Button { model.send(suggested) } label: {
                    Text(suggested).font(.system(size: 11, weight: .medium))
                        .padding(.horizontal, 10).padding(.vertical, 4)
                        .background(Capsule().fill(Color.orange.opacity(0.25)))
                }
                .buttonStyle(.kitPlain)
                .disabled(model.sending)
            }
            HStack(alignment: .bottom, spacing: 8) {
                TextField(suggestion.map { "\($0)   ⇥ Tab" } ?? "Reply, or / for commands…", text: $model.draft, axis: .vertical)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .lineLimit(1...5)
                    .focused($focused)
                    .onSubmit { model.send() }
                    .padding(.horizontal, 10).padding(.vertical, 8)
                    .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.09)))
                Button { model.send() } label: {
                    if model.sending {
                        ProgressView().controlSize(.small).frame(width: 24, height: 24)
                    } else {
                        Image(systemName: "arrow.up.circle.fill").font(.system(size: 24))
                    }
                }
                .buttonStyle(.kitPlain)
                .foregroundColor(model.draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? .secondary : .orange)
                .disabled(model.sending)
            }
            if let error = model.sendError {
                Text(error).font(.system(size: 11)).foregroundColor(.red)
            } else if model.sending {
                Text("Sending… it shows up here once the agent has it.").font(.system(size: 11)).foregroundColor(.secondary)
            }
        }
        .onAppear { DispatchQueue.main.asyncAfter(deadline: .now() + 0.25) { focused = true } }
    }
}

/// Slash commands matching what's typed; ↑↓ to move, Tab or Return to pick.
struct CommandList: View {
    @ObservedObject var model: NotchModel
    let commands: [SlashCommand]

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            ForEach(Array(commands.enumerated()), id: \.offset) { index, command in
                let selected = index == min(model.completionIndex, commands.count - 1)
                Button { model.accept(command) } label: {
                    VStack(alignment: .leading, spacing: 1) {
                        HStack(spacing: 6) {
                            Text("/" + command.name).font(.system(size: 12, weight: .semibold, design: .monospaced))
                            if let hint = command.argumentHint {
                                Text(hint).font(.system(size: 11, design: .monospaced)).foregroundColor(.secondary).lineLimit(1)
                            }
                        }
                        if !command.description.isEmpty {
                            Text(command.description).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                        }
                    }
                    .padding(.horizontal, 8)
                    .padding(.vertical, 4)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(selected ? 0.12 : 0)))
                    .contentShape(Rectangle())
                }
                .buttonStyle(.kitPlain)
            }
        }
        .padding(4)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(0.05)))
    }
}

/// One conversation entry, laid out like Claude Code's terminal: "❯" prompts on a shaded bar,
/// Claude's text after a "⏺", and dim tool summaries indented beneath it.
struct Bubble: View {
    let line: ChatLine

    var body: some View {
        switch line.kind {
        case .user:
            row("❯", .secondary, Text(line.text).foregroundColor(.white.opacity(0.9)))
                .padding(.vertical, 5)
                .padding(.horizontal, 6)
                .background(RoundedRectangle(cornerRadius: 4).fill(Color.white.opacity(0.1)))
        case .assistant:
            row("⏺", .white.opacity(0.85), Text(Self.markdown(line.text)))
                .padding(.horizontal, 6)
        case .tools:
            row("⎿", .secondary, Text(line.text).foregroundColor(.secondary))
                .padding(.leading, 18)
                .padding(.trailing, 6)
        }
    }

    private func row(_ mark: String, _ markColor: Color, _ text: Text) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Text(mark).foregroundColor(markColor)
            text.textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
            Spacer(minLength: 0)
        }
        .font(.system(size: 11.5, design: .monospaced))
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    static func markdown(_ s: String) -> AttributedString {
        (try? AttributedString(markdown: s, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace)))
            ?? AttributedString(s)
    }
}

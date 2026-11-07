import SwiftUI

/// "Claude asks": the question with its options as buttons, ⌘1…⌘9 to pick, and a free-text answer.
struct QuestionCard: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    let question: PendingQuestion
    var showsAgent = false
    /// The question and its options scroll past this; the header and answer box stay put.
    var bodyLimit: CGFloat = 400
    @State private var typed = ""

    /// Keeps a peek's whole card within `NotchModel.maxCard` with room for the header and answer box.
    static let peekBodyLimit: CGFloat = 400
    @FocusState private var typing: Bool

    var body: some View {
        let step = min(model.choices.count, question.items.count - 1)
        let item = question.items[step]
        let canAnswer = model.canAnswer(agent, question) && !model.sending

        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "bubble.left.fill").foregroundColor(.asks)
                Text("Claude asks").font(.system(size: 13, weight: .semibold)).foregroundColor(.asks)
                if question.items.count > 1 {
                    Text("\(step + 1) of \(question.items.count)").font(.system(size: 11)).foregroundColor(.secondary)
                }
                Spacer()
                if showsAgent {
                    Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                    HideButton(model: model).part("Hide Button", .bottom)
                }
                if step > 0 {
                    Button("Back") { model.undoChoice() }.buttonStyle(.kitPlain)
                        .font(.system(size: 11)).foregroundColor(.secondary)
                }
            }
            .frame(height: 22)

            ScrollingBody(limit: bodyLimit, resetKey: step) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(item.question)
                        .font(.system(size: 14, weight: .medium))
                        .fixedSize(horizontal: false, vertical: true)

                    ForEach(Array(item.options.enumerated()), id: \.offset) { index, label in
                        Button { model.choose(.option(index + 1), for: agent.id) } label: {
                            HStack(alignment: .top, spacing: 10) {
                                Text("⌘\(index + 1)")
                                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                                    .foregroundColor(.white.opacity(0.55))
                                    .frame(width: 30, height: 22)
                                    .background(RoundedRectangle(cornerRadius: 6).fill(Color.optionBadge))
                                // The whole label and what it means, wrapped, so the choice can be read in full.
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(label).font(.system(size: 13, weight: .medium))
                                        .fixedSize(horizontal: false, vertical: true)
                                    if index < item.details.count, !item.details[index].isEmpty {
                                        Text(item.details[index]).font(.system(size: 11.5)).foregroundColor(.secondary)
                                            .fixedSize(horizontal: false, vertical: true)
                                    }
                                }
                                .padding(.top, 2)
                                Spacer(minLength: 0)
                            }
                            .padding(.horizontal, 8)
                            .padding(.vertical, 6)
                            .frame(maxWidth: .infinity, minHeight: 34, alignment: .leading)
                            .background(RoundedRectangle(cornerRadius: 10).fill(Color.optionFill))
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.kitPlain)
                        .keyboardShortcut(KeyEquivalent(Character("\(min(index + 1, 9))")), modifiers: .command)
                        .disabled(!canAnswer)
                    }
                }
            }

            if model.canAnswer(agent, question) {
                TextField("Type something else…", text: $typed)
                    .textFieldStyle(.plain)
                    .font(.system(size: 13))
                    .focused($typing)
                    .onSubmit {
                        let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard !text.isEmpty else { return }
                        typed = ""
                        model.choose(.text(text), for: agent.id)
                    }
                    .padding(.horizontal, 12)
                    .frame(height: 34)
                    .background(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.12)))
                    .disabled(model.sending)
            } else {
                Button("Answer in \(agent.host) ›") { model.answerInTerminal(agent.id) }
                    .buttonStyle(.kitPlain).font(.system(size: 12)).foregroundColor(.asks)
            }

            if let error = model.sendError {
                Text(error).font(.system(size: 11)).foregroundColor(.red)
            } else if model.sending {
                Text("Sending…").font(.system(size: 11)).foregroundColor(.secondary)
            }
        }
    }
}

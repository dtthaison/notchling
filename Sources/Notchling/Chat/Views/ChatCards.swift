import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

struct ChatQuestionCard: View {
    @ObservedObject var notch: NotchModel
    let agent: Agent
    let question: PendingQuestion
    @State private var typed = ""
    @State private var picked: Set<Int> = []
    @FocusState private var typing: Bool

    var body: some View {
        let index = min(notch.choices.count, max(question.items.count - 1, 0))
        let item = question.items.isEmpty ? nil : question.items[index]
        let canAnswer = notch.canAnswer(agent, question, multiSelect: true)
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                KitClaudeMark(size: 20)
                Text(question.items.count > 1 ? "Claude is asking · \(index + 1) of \(question.items.count)" : "Claude is asking")
                    .kitType(.label).foregroundColor(KitInk.accentText)
                Spacer()
                if notch.choices.count > 0 {
                    Button { notch.undoChoice() } label: {
                        HStack(spacing: 4) { KitIconView(icon: .back, size: 11); Text("Back") }
                    }
                    .buttonStyle(.kit(.ghost))
                    .help("Change the previous answer")
                }
            }
            if let item {
                Text(item.question).kitType(.heading).foregroundColor(KitInk.text).textSelection(.enabled)
                if canAnswer, item.multiSelect {
                    Text("Select all that apply").kitType(.meta).foregroundColor(KitInk.textMuted)
                    ForEach(Array(item.options.enumerated()), id: \.offset) { i, option in
                        Button {
                            if picked.contains(i + 1) { picked.remove(i + 1) } else { picked.insert(i + 1) }
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(option).kitType(.heading).foregroundColor(KitInk.text)
                                if i < item.details.count, !item.details[i].isEmpty {
                                    Text(item.details[i]).kitType(.meta).foregroundColor(KitInk.textMuted)
                                }
                            }
                        }
                        .buttonStyle(KitOptionStyle(number: i + 1, selected: picked.contains(i + 1)))
                        .disabled(notch.sending)
                    }
                    HStack {
                        Spacer()
                        Button(index == question.items.count - 1 ? "Submit" : "Next") {
                            let numbers = picked.sorted()
                            picked = []
                            notch.choose(.options(numbers), for: agent.id)
                        }
                        .buttonStyle(.kit(.primary))
                        .disabled(picked.isEmpty || notch.sending)
                    }
                } else if canAnswer {
                    ForEach(Array(item.options.enumerated()), id: \.offset) { i, option in
                        Button { notch.choose(.option(i + 1), for: agent.id) } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(option).kitType(.heading).foregroundColor(KitInk.text)
                                if i < item.details.count, !item.details[i].isEmpty {
                                    Text(item.details[i]).kitType(.meta).foregroundColor(KitInk.textMuted)
                                }
                            }
                        }
                        .buttonStyle(KitOptionStyle(number: i + 1))
                        .disabled(notch.sending)
                    }
                    TextField("Something else…", text: $typed)
                        .focused($typing)
                        .modifier(KitField(focused: typing))
                        .onSubmit {
                            let text = typed.trimmingCharacters(in: .whitespacesAndNewlines)
                            guard !text.isEmpty else { return }
                            typed = ""
                            notch.choose(.text(text), for: agent.id)
                        }
                } else {
                    HStack {
                        Text("Answer this one in the session's terminal.")
                            .kitType(.meta).foregroundColor(KitInk.textMuted)
                        Spacer()
                        Button("Answer in Terminal") { notch.answerInTerminal(agent.id) }.buttonStyle(.kit(.secondary))
                    }
                }
            }
            if let error = notch.sendError {
                Text(error).kitType(.meta).foregroundColor(.red)
            } else if notch.sending {
                Text("Sending the answer…").kitType(.meta).foregroundColor(KitInk.textMuted)
            }
        }
        .padding(16)
        .kitCard(radius: KitMetric.radiusComposer)
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
            .strokeBorder(KitInk.accent.opacity(0.45), lineWidth: 1))
        .shadow(color: KitInk.shadow, radius: 14, y: 6)
        .onChange(of: index) { _ in picked = [] }
        // SwiftUI can keep this card's state when another session's question takes its place.
        .onChange(of: question.toolUseID) { _ in picked = [] }
    }
}

struct ChatPermissionCard: View {
    @ObservedObject var notch: NotchModel
    let agent: Agent
    let request: HookRequest

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                KitIconView(icon: .permission, size: 13).foregroundColor(KitInk.accentText)
                Text("Allow \(request.toolName)?").kitType(.heading).foregroundColor(KitInk.text)
            }
            ScrollView {
                Text(request.summary)
                    .kitType(.mono).foregroundColor(KitInk.text)
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(12)
            }
            .frame(maxHeight: 160)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.raised))
            HStack(spacing: 8) {
                Button("Deny") { notch.decide(.deny(nil), for: agent.id) }
                    .buttonStyle(.kit(.ghost))
                    .help("Refuse this tool call; Claude is told it was denied")
                Button("Answer in Terminal") { notch.decide(.terminal, for: agent.id) }
                    .buttonStyle(.kit(.ghost))
                    .help("Hand the prompt back to the session's terminal")
                Spacer()
                if request.canAlwaysAllow {
                    Button("Always Allow") { notch.decide(.allowAlways, for: agent.id) }
                        .buttonStyle(.kit(.secondary))
                        .help("Allow this and add a rule so it isn't asked again")
                }
                Button("Allow") { notch.decide(.allow, for: agent.id) }
                    .buttonStyle(.kit(.primary))
                    .help("Allow this one tool call")
            }
        }
        .padding(16)
        .kitCard(radius: KitMetric.radiusComposer)
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
            .strokeBorder(KitInk.accent.opacity(0.45), lineWidth: 1))
        .shadow(color: KitInk.shadow, radius: 14, y: 6)
    }
}

import AppKit
import SwiftUI

// Second Opinion: ask Codex or Antigravity CLI to review an edit or a reply (the engine behind
// `Notchling --delegate`), see it run under the message, read what it said.

/// The small button on a message or an edit that opens the form. The caller owns `isOpen`, so its hover
/// bar can stay up while the popover is.
struct SecondOpinionButton: View {
    let subject: SecondOpinionPrompt.Subject
    let anchor: String
    @Binding var isOpen: Bool
    @Environment(\.secondOpinion) private var context

    var body: some View {
        if let context {
            Button { isOpen.toggle() } label: {
                Image(systemName: "checkmark.bubble").font(.system(size: 11, weight: .medium))
            }
            .buttonStyle(.kit(.ghost))
            .help("Second Opinion… Ask Codex or Antigravity to review this")
            .accessibilityLabel("Second Opinion")
            .popover(isPresented: $isOpen, arrowEdge: .bottom) {
                SecondOpinionForm(subject: subject, anchor: anchor, context: context) { isOpen = false }
            }
            .part("Second Opinion Button", .topTrailing)
        }
    }
}

/// The popover: tool, model, effort and the prompt, prefilled from what is being reviewed.
struct SecondOpinionForm: View {
    let subject: SecondOpinionPrompt.Subject
    let anchor: String
    let context: SecondOpinionContext
    let close: () -> Void

    @State private var tool: Delegate.Tool
    @State private var model = ""
    @State private var effort = ""
    @State private var prompt: String
    @State private var codexModels: [SecondOpinionModels.Option] = []
    @State private var agyModels: [SecondOpinionModels.Option] = []
    @State private var loadingAgy = false

    private let available: [Delegate.Tool]

    init(subject: SecondOpinionPrompt.Subject, anchor: String, context: SecondOpinionContext, close: @escaping () -> Void) {
        self.subject = subject
        self.anchor = anchor
        self.context = context
        self.close = close
        var tools: [Delegate.Tool] = []
        if CodexStore.binary() != nil { tools.append(.codex) }
        if AntigravityRunner.binary() != nil { tools.append(.agy) }
        available = tools
        _tool = State(initialValue: tools.first ?? .codex)
        _prompt = State(initialValue: SecondOpinionPrompt.build(subject, folder: context.folder))
    }

    private var options: [SecondOpinionModels.Option] { tool == .codex ? codexModels : agyModels }

    /// The picked model's efforts; with the default model, every effort any model offers.
    private var efforts: [String] {
        if let m = options.first(where: { $0.model == model }) { return m.efforts }
        var seen: [String] = []
        for e in options.flatMap(\.efforts) where !seen.contains(e) { seen.append(e) }
        return seen
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Second Opinion").kitType(.heading).foregroundColor(KitInk.text)
            if available.isEmpty {
                Text("Neither Codex nor Antigravity CLI is installed.").kitType(.body).foregroundColor(KitInk.textMuted)
            } else {
                if available.count > 1 {
                    Picker("", selection: $tool) {
                        ForEach(available, id: \.self) { Text($0 == .codex ? "Codex" : "Antigravity").tag($0) }
                    }
                    .pickerStyle(.segmented).labelsHidden()
                    .part("Second Opinion Tool", .topLeading)
                }
                HStack(spacing: 8) {
                    picker("Model", selection: $model, items: options.map { ($0.model, $0.name) })
                    picker("Effort", selection: $effort, items: efforts.map { ($0, $0.capitalized) })
                }
                .part("Second Opinion Model", .topLeading)
                TextEditor(text: $prompt)
                    .font(KitType.mono.font)
                    .foregroundColor(KitInk.text)
                    .scrollContentBackground(.hidden)
                    .padding(6)
                    .frame(width: 420, height: 190)
                    .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.raised.opacity(0.7)))
                    .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).strokeBorder(KitInk.hairline, lineWidth: 1))
                    .part("Second Opinion Prompt", .topLeading)
                HStack {
                    Text("Read-only. It can look at the folder but not change it.").kitType(.meta).foregroundColor(KitInk.textFaint)
                    Spacer()
                    Button("Cancel", action: close).buttonStyle(.kit(.ghost)).keyboardShortcut(.cancelAction)
                    Button("Run", action: run).buttonStyle(.kit(.primary)).keyboardShortcut(.defaultAction)
                        .disabled(prompt.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
        .padding(16)
        .frame(width: 452)
        .background(KitInk.window)
        .onAppear { loadModels(for: tool) }
        .onChange(of: tool) { newTool in
            model = ""
            effort = ""
            loadModels(for: newTool)
        }
        .onChange(of: model) { _ in if !effort.isEmpty && !efforts.contains(effort) { effort = "" } }
    }

    /// A pop-up of the choices with "Default" first (which leaves the flag off).
    private func picker(_ title: String, selection: Binding<String>, items: [(String, String)]) -> some View {
        HStack(spacing: 6) {
            Text(title).kitType(.meta).foregroundColor(KitInk.textMuted)
            Picker("", selection: selection) {
                Text("Default").tag("")
                ForEach(items, id: \.0) { Text($0.1).tag($0.0) }
            }
            .labelsHidden()
            .frame(maxWidth: .infinity)
        }
    }

    private func loadModels(for tool: Delegate.Tool) {
        switch tool {
        case .codex:
            codexModels = SecondOpinionModels.codex()
        case .agy:
            guard agyModels.isEmpty, !loadingAgy else { return }
            loadingAgy = true
            DispatchQueue.global(qos: .userInitiated).async {
                let list = SecondOpinionModels.agy()
                DispatchQueue.main.async { agyModels = list; loadingAgy = false }
            }
        }
    }

    private func run() {
        let text = prompt.trimmingCharacters(in: .whitespacesAndNewlines)
        context.store.start(.init(session: context.session, anchor: anchor, subject: SecondOpinionPrompt.title(subject),
                                  tool: tool, model: model.isEmpty ? nil : model, effort: effort.isEmpty ? nil : effort,
                                  folder: context.folder, prompt: text))
        close()
    }
}

/// The cards for the opinions asked on these messages or tool calls, under the message.
struct SecondOpinionCards: View {
    let anchors: [String]
    @Environment(\.secondOpinion) private var context

    var body: some View {
        if let context {
            CardList(store: context.store, session: context.session, anchors: anchors, open: context.open)
        }
    }

    private struct CardList: View {
        @ObservedObject var store: SecondOpinionStore
        let session: String
        let anchors: [String]
        let open: (SecondOpinion) -> Void

        var body: some View {
            let runs = store.runs(session: session, anchors: anchors)
            if !runs.isEmpty {
                VStack(alignment: .leading, spacing: 6) {
                    ForEach(runs) { run in
                        SecondOpinionCard(run: run, store: store, open: open)
                    }
                }
                .padding(.top, 6)
            }
        }
    }
}

/// One opinion. Running: a compact line with the time and Cancel. Done: the reply in markdown, which
/// folds, and a link to the session the other tool made.
struct SecondOpinionCard: View {
    let run: SecondOpinion
    @ObservedObject var store: SecondOpinionStore
    let open: (SecondOpinion) -> Void
    @State private var folded = false

    private var tool: SessionTool { run.toolValue == .codex ? .codex : .antigravity }
    private var toolName: String { run.toolValue == .codex ? "Codex" : "Antigravity" }

    private var pick: String {
        [run.model, run.effort].compactMap { $0 }.joined(separator: " · ")
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            header
            if run.status != .running, !folded {
                Rectangle().fill(KitInk.hairline).frame(height: 1)
                VStack(alignment: .leading, spacing: 8) {
                    if let error = run.error, run.status == .failed {
                        Text(error).kitType(.body).foregroundColor(.red).textSelection(.enabled)
                    }
                    if !run.reply.isEmpty {
                        MarkdownView(text: run.reply).frame(maxWidth: .infinity, alignment: .leading)
                    } else if run.status == .cancelled {
                        Text("Cancelled before it answered.").kitType(.body).foregroundColor(KitInk.textMuted)
                    }
                }
                .padding(.horizontal, 14).padding(.vertical, 12)
            }
        }
        .kitCard(radius: KitMetric.radiusMd, fill: KitInk.card)
        .part(run.status == .running ? "Second Opinion Running" : "Second Opinion", .topTrailing)
    }

    private var header: some View {
        HStack(spacing: 8) {
            ToolIcon(tool: tool, size: 14)
            if run.status == .running {
                ProgressView().controlSize(.mini)
                Text("\(toolName) is reviewing \(run.subject)").kitType(.label).foregroundColor(KitInk.text).lineLimit(1)
                if !pick.isEmpty { Text(pick).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1) }
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    Text(ChatFormat.duration(context.date.timeIntervalSince(run.started)))
                        .kitType(.meta).foregroundColor(KitInk.textFaint).monospacedDigit()
                }
                .fixedSize()
                Spacer(minLength: 8)
                Button("Cancel") { store.cancel(run.id) }
                    .buttonStyle(.kit(.ghost))
                    .help("Stop this review")
            } else {
                Button { withAnimation(KitMotion.appear) { folded.toggle() } } label: {
                    HStack(spacing: 8) {
                        Text(title).kitType(.label).foregroundColor(KitInk.text).lineLimit(1)
                        if !pick.isEmpty { Text(pick).kitType(.meta).foregroundColor(KitInk.textFaint).lineLimit(1) }
                        if run.seconds > 0 { Text(ChatFormat.duration(run.seconds)).kitType(.meta).foregroundColor(KitInk.textFaint) }
                        Image(systemName: "chevron.right").font(.system(size: 9, weight: .bold)).foregroundColor(KitInk.textFaint)
                            .rotationEffect(.degrees(folded ? 0 : 90))
                        Spacer(minLength: 8)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.kitPlain)
                .help(folded ? "Show the review" : "Hide the review")
                if run.delegatedSession != nil {
                    Button { open(run) } label: {
                        HStack(spacing: 4) {
                            Text("Open Session")
                            Image(systemName: "arrow.right").font(.system(size: 9, weight: .semibold))
                        }
                    }
                    .buttonStyle(.kit(.ghost))
                    .help("Read the session \(toolName) made for this review")
                    .part("Open Delegated Session Button", .bottom)
                }
                if !run.reply.isEmpty { CopyButton(text: run.reply) }
                Button { store.remove(run.id) } label: {
                    Image(systemName: "xmark").font(.system(size: 10, weight: .medium))
                }
                .buttonStyle(.kit(.ghost))
                .help("Remove this review")
            }
        }
        .padding(.leading, 12).padding(.trailing, 6)
        .frame(minHeight: 34)
    }

    private var title: String {
        switch run.status {
        case .done: return "\(toolName) on \(run.subject)"
        case .failed: return "\(toolName) couldn't review \(run.subject)"
        default: return "\(toolName) on \(run.subject), cancelled"
        }
    }
}

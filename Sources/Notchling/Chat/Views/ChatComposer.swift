import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

struct Composer: View {
    @ObservedObject var model: ChatModel
    let agent: Agent
    /// A file is being dragged over the conversation (the drop target is `SessionDetail`).
    let dropping: Bool
    @State private var focused = false
    @State private var focusedBeforeDrag = false

    private var placeholder: String {
        if case .codex(let session) = agent.target {
            return (session.isEmpty ? "What should Codex do in \(agent.project)?" : "Reply to \(agent.name)") + "   ⌥ Return new line"
        }
        return "Reply to \(agent.name), or / for commands…   ⌥ Return new line"
    }

    var body: some View {
        let draft = Binding(get: { model.drafts[agent.id] ?? "" },
                            set: { model.drafts[agent.id] = $0; model.completionIndex = 0 })
        let attached = model.attachments[agent.id] ?? []
        let empty = draft.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && attached.isEmpty
        let commands = model.completions(for: agent)
        let sending = model.isSending(agent.id)
        let shell = ShellCommand.isCommand(draft.wrappedValue)
        // What will actually run, when putting a copied command back together changed it.
        let fixed: String? = {
            guard shell else { return nil }
            let typed = draft.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines)
            let normal = ShellCommand.normalize(typed)
            return normal != typed && normal != "!" ? String(normal.dropFirst()) : nil
        }()
        // A session waiting on you gets its suggested reply as a one-click chip; otherwise Tab takes it.
        let showsChip = commands.isEmpty && empty && agent.group == .needsInput && !(agent.suggestedReply ?? "").isEmpty
        // Tab takes it, unless the chip above already offers it.
        let suggestion = showsChip ? nil : model.suggestion(for: agent)
        VStack(alignment: .leading, spacing: 8) {
            if showsChip, let suggested = agent.suggestedReply {
                Button { model.send(suggested) } label: {
                    HStack(spacing: 6) {
                        KitIconView(icon: .claude, size: 10)
                        Text(suggested).lineLimit(1)
                    }
                }
                .buttonStyle(.kit(.secondary))
                .disabled(sending)
                .help("Send the suggested reply")
                .part("Suggested Reply", .trailing)
            }
            VStack(alignment: .leading, spacing: 0) {
                if !commands.isEmpty {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(Array(commands.enumerated()), id: \.offset) { index, command in
                            let picked = index == min(model.completionIndex, commands.count - 1)
                            Button { model.accept(command, for: agent) } label: {
                                HStack(spacing: 10) {
                                    Text("/" + command.name).kitType(.mono).foregroundColor(picked ? KitInk.accentText : KitInk.text)
                                    Text(command.description).kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                                    Spacer(minLength: 0)
                                }
                                .padding(.horizontal, 10).frame(height: 26)
                                .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous)
                                    .fill(picked ? KitInk.selection : .clear))
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.kitPlain)
                            .pointerCursor()
                        }
                    }
                    .padding(6)
                    .part("Command List", .topTrailing)
                    Rectangle().fill(KitInk.hairline).frame(height: 1)
                }
                if shell {
                    // Shell mode, so it's clear Return runs a command rather than sending a message.
                    HStack(spacing: 6) {
                        KitIconView(icon: .terminal, size: 11)
                        Text("Shell command").kitType(.label)
                        Text("runs in \((agent.cwd as NSString).lastPathComponent) · Claude sees the output")
                            .kitType(.meta).foregroundColor(KitInk.textMuted).lineLimit(1)
                        Spacer(minLength: 0)
                    }
                    .foregroundColor(KitInk.shell)
                    .padding(.horizontal, 14).padding(.top, 9)
                    .accessibilityElement(children: .combine)
                    .part("Shell Header", .trailing)
                    if let fixed {
                        // A command copied from a terminal comes with its line wrapping; this is what runs.
                        VStack(alignment: .leading, spacing: 3) {
                            Text("Will run, with the copied line breaks joined:").kitType(.meta).foregroundColor(KitInk.textMuted)
                            Text(fixed).kitType(.mono).foregroundColor(KitInk.text)
                                .textSelection(.enabled)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        .padding(.horizontal, 10).padding(.vertical, 7)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(RoundedRectangle(cornerRadius: KitMetric.radiusSm, style: .continuous).fill(KitInk.raised))
                        .padding(.horizontal, 12).padding(.top, 6)
                        .accessibilityElement(children: .combine)
                        .part("Will Run Box", .trailing)
                    }
                }
                if !attached.isEmpty {
                    AttachmentStrip(model: model, id: agent.id, urls: attached)
                }
                HStack(alignment: .bottom, spacing: 10) {
                    ReplyEditor(text: draft, focused: $focused,
                                placeholder: suggestion.map { "\($0)   ⇥ Tab" } ?? placeholder,
                                placeholderIsSuggestion: suggestion != nil,
                                font: (shell ? KitType.mono : KitType.body).nsFont,
                                accessibilityLabel: "Reply to \(agent.name)",
                                onSubmit: { model.send() })
                        .frame(maxWidth: .infinity)
                        .padding(.vertical, 6)
                    Button { model.send() } label: {
                        ZStack {
                            Circle().fill(empty ? KitInk.raised : shell ? KitInk.shell : KitInk.accent)
                            if sending && empty {
                                ProgressView().controlSize(.small)
                            } else {
                                KitIconView(icon: .send, size: 13)
                                    .foregroundColor(empty ? KitInk.textFaint : shell ? KitInk.onShell : KitInk.onAccent)
                            }
                        }
                        .frame(width: 30, height: 30)
                    }
                    .buttonStyle(.kitPlain)
                    // Still works while one goes out: the next waits its turn.
                    .disabled(empty)
                    .help(shell ? "Run the command in the session (Return)" : "Send the reply (Return)")
                    .accessibilityLabel(shell ? "Run" : "Send")
                    .pointerCursor()
                    .part("Send Button", .top)
                }
                .padding(.leading, 14).padding(.trailing, 8).padding(.vertical, 8)
            }
            .background(ZStack {
                RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous).fill(KitInk.card)
                RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
                    .fill(shell ? KitInk.shellWash : .clear)
            })
            .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
                .strokeBorder(dropping ? KitInk.accent : shell ? KitInk.shell : focused ? KitInk.accent.opacity(0.6) : KitInk.hairline,
                              lineWidth: focused || dropping || shell ? 1.5 : 1))
            .shadow(color: KitInk.shadow, radius: 14, y: 6)
            .part("Composer", .bottomLeading)
            .animation(KitMotion.hover, value: focused)
            .animation(KitMotion.hover, value: dropping)
            .animation(KitMotion.hover, value: shell)
            if let error = model.errors[agent.id] {
                HStack(spacing: 6) {
                    KitIconView(icon: .info, size: 11)
                    Text(error).kitType(.meta)
                }
                .foregroundColor(.red)
                .padding(.leading, 6)
            } else {
                HStack(spacing: 8) {
                    if case .codex = agent.target {
                        CodexModelPicker(model: model, agent: agent).part("Codex Model Picker", .top)
                    } else if let (tool, session) = agent.target.acp {
                        ACPOptionsPicker(model: model, agent: agent, tool: tool, session: session).part("\(tool.name) Options", .top)
                    } else {
                        ModelPicker(model: model, current: model.conversation.facts.model).part("Model Picker", .top)
                        EffortPicker(model: model, current: model.conversation.facts.effort).part("Effort Picker", .top)
                    }
                    if model.canStop(agent) {
                        Spacer(minLength: 0)
                        let stopping = model.isStopping(agent)
                        Button { model.stopTurn() } label: {
                            HStack(spacing: 4) {
                                Image(systemName: "stop.fill").font(.system(size: 8))
                                Text(stopping ? "Stopping…" : "Stop")
                                if !stopping { Text("esc").foregroundColor(KitInk.textFaint) }
                            }
                            .kitType(.meta)
                        }
                        .buttonStyle(.kit(.ghost))
                        .disabled(stopping)
                        .help("Stop this turn, like Esc in the terminal. What it did so far stays.")
                        .part("Stop Button", .top)
                    }
                    if shell {
                        Text("Return runs it like ! in the terminal · delete the ! to send a message instead")
                            .kitType(.meta).foregroundColor(KitInk.textFaint)
                            .lineLimit(1)
                            .frame(maxWidth: .infinity)
                            .part("Composer Hint", .trailing)
                    }
                }
                .padding(.leading, 6)
            }
        }
        .onChange(of: focused) { model.composerFocused = $0 }
        .onChange(of: model.focusRequest) { _ in focused = true }
        // While the reply box has focus, its text editor registers for file drags and takes the drop
        // before SwiftUI's onDrop sees it (inserting nothing). So it lets go while a file is over the
        // conversation, and takes focus back after.
        .onChange(of: dropping) { over in
            if over {
                focusedBeforeDrag = focused
                focused = false
            } else if focusedBeforeDrag {
                focused = true
            }
        }
        .onAppear { DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) { focused = true } }
        .onDisappear { model.composerFocused = false }
    }
}

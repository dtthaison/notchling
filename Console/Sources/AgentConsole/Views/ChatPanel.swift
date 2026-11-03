import ConsoleCore
import SwiftUI

struct ChatPanel: View {
    @Bindable var model: ConsoleModel

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(title: "Chat") { terminalToggle }
            if let entry = model.selectedStarting {
                if let pty = model.terminal(for: entry) {
                    TerminalContainer(pty: pty)
                } else {
                    ErrorState(message: "Terminal đã đóng.") { model.select(nil) }
                }
            } else if let session = model.selectedSession {
                SessionChat(model: model, session: session)
            } else if model.store.selection != nil {
                ErrorState(message: "Session không còn trong danh sách.") { model.store.reload() }
            } else {
                emptyState
            }
        }
        .background(Token.surface)
    }

    @ViewBuilder
    private var terminalToggle: some View {
        if let session = model.selectedSession, model.terminal(for: session) != nil {
            let shown = model.terminalShown.contains(session.id)
            Button {
                if shown { model.terminalShown.remove(session.id) } else { model.terminalShown.insert(session.id) }
            } label: {
                Label("Terminal", systemImage: "terminal")
                    .foregroundStyle(shown ? Token.primary : Token.muted)
            }
            .buttonStyle(SecondaryButtonStyle())
            .accessibilityLabel(shown ? "Ẩn terminal, quay lại chat" : "Hiện terminal")
            .help("Terminal trực tiếp của session này")
        }
    }

    private var emptyState: some View {
        VStack(spacing: Space.md) {
            Image(systemName: "bubble.left.and.text.bubble.right").font(.system(size: 20)).foregroundStyle(Token.muted)
                .accessibilityHidden(true)
            Text("Chọn một session để xem chat.").font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
            Text("⌘K để tìm, ↑ ↓ để chuyển trong danh sách.").font(TypeToken.bodySm).foregroundStyle(Token.muted)
            Button { model.showingNewSession = true } label: { Label("Session mới", systemImage: "plus") }
                .buttonStyle(SecondaryButtonStyle())
                .accessibilityLabel("Session mới")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct SessionChat: View {
    @Bindable var model: ConsoleModel
    let session: Session

    private var store: SessionStore { model.store }
    private var subagent: Subagent? { model.selectedSubagent }
    private var showingTerminal: Bool { model.terminalShown.contains(session.id) && model.terminal(for: session) != nil }

    var body: some View {
        VStack(spacing: 0) {
            ChatTitle(model: model, session: session, subagent: subagent)
            Divider1()
            if showingTerminal, let pty = model.terminal(for: session) {
                TerminalContainer(pty: pty)
            } else {
                transcript
                if subagent == nil, let prompt = session.pending ?? model.demoPrompt(for: session) {
                    PendingCard(
                        prompt: prompt, error: model.answerErrors[session.id],
                        onAllow: { model.allow(prompt) }, onDeny: { model.deny(prompt) },
                        onAnswer: { model.answer(prompt, choices: $0) })
                        .padding(.horizontal, Space.md).padding(.top, Space.sm)
                }
                Composer(model: model, session: session, cardIsShowing: subagent == nil && (session.pending ?? model.demoPrompt(for: session)) != nil, readOnlyReason: subagent == nil ? nil : "Transcript của subagent: chỉ xem.")
            }
        }
    }

    @ViewBuilder
    private var transcript: some View {
        let selection = store.selection
        if store.chatLoadedFor != selection {
            ChatSkeleton()
        } else if let error = store.chatError {
            ErrorState(message: error) { store.reload() }
        } else if store.chat.isEmpty {
            VStack(spacing: Space.sm) {
                Text("Chưa có tin nhắn nào.").font(TypeToken.bodyMd).foregroundStyle(Token.muted)
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            MessageList(messages: store.chat, resetKey: selection, transcript: session.transcriptURL).id(selection)
        }
    }
}

private struct ChatTitle: View {
    @Bindable var model: ConsoleModel
    let session: Session
    let subagent: Subagent?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: Space.sm) {
                StatusDot(state: session.state)
                Text(subagent.map { $0.description.isEmpty ? $0.type : $0.description } ?? session.name)
                    .font(TypeToken.titleMd).foregroundStyle(Token.onSurface).lineLimit(1)
                    .accessibilityAddTraits(.isHeader)
                if session.state == .needsInput { InputBadge() }
                if subagent != nil { Text("subagent · \(session.name)").font(TypeToken.bodySm).foregroundStyle(Token.muted).lineLimit(1) }
                Spacer(minLength: Space.sm)
                if subagent == nil, session.control == .viewOnly { TakeoverButton(model: model, session: session) }
            }
            HStack(spacing: Space.sm) {
                Text(session.project).font(TypeToken.codeSm).foregroundStyle(Token.muted)
                if let branch = GitBranch.current(cwd: session.cwd) {
                    Label(branch, systemImage: "arrow.triangle.branch").font(TypeToken.codeSm).foregroundStyle(Token.muted)
                }
                if let modelName = modelName {
                    Text(modelName).font(TypeToken.codeSm).foregroundStyle(Token.muted)
                }
            }
            .lineLimit(1)
        }
        .padding(.horizontal, Space.md).padding(.vertical, Space.sm)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private var modelName: String? {
        subagent.map { $0.model.isEmpty ? nil : $0.model } ?? model.store.chatModel
    }
}

private struct ChatSkeleton: View {
    var body: some View {
        VStack(alignment: .leading, spacing: Space.md) {
            ForEach(0..<4, id: \.self) { index in
                RoundedRectangle(cornerRadius: Radius.lg).fill(Token.surfaceRaised)
                    .frame(width: CGFloat(260 + index * 40), height: index.isMultiple(of: 2) ? 44 : 28)
                    .frame(maxWidth: .infinity, alignment: index.isMultiple(of: 2) ? .trailing : .leading)
            }
            Spacer()
        }
        .padding(Space.md)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Đang tải chat")
    }
}

/// Messages, auto-scrolling to the newest unless the user scrolled up; then a "Tin mới" pill appears.
private struct MessageList: View {
    let messages: [ChatMessage]
    let resetKey: ChatSelection?
    let transcript: URL?

    @State private var atBottom = true
    @State private var unseen = 0
    @State private var knownIds: Set<UUID> = []
    @State private var settled = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        ScrollViewReader { proxy in
            ZStack(alignment: .bottom) {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: Space.md) {
                        ForEach(messages) { message in
                            MessageRow(message: message, transcript: transcript, animateIn: settled && !knownIds.contains(message.id))
                                .id(message.id)
                        }
                        Color.clear.frame(height: 1).id("bottom")
                            .onAppear { atBottom = true; unseen = 0 }
                            .onDisappear { atBottom = false }
                    }
                    .padding(Space.md)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                if unseen > 0 && !atBottom {
                    Button {
                        scrollToBottom(proxy)
                    } label: {
                        Label("Tin mới", systemImage: "arrow.down")
                    }
                    .buttonStyle(SecondaryButtonStyle())
                    .padding(.bottom, Space.sm)
                    .transition(.opacity)
                    .accessibilityLabel("Có \(unseen) tin mới. Cuộn xuống cuối.")
                }
            }
            .onAppear {
                knownIds = Set(messages.map(\.id))
                scrollToBottom(proxy, animated: false)
                DispatchQueue.main.async { settled = true }
            }
            .onChange(of: resetKey) { _, _ in
                settled = false
                knownIds = Set(messages.map(\.id))
                unseen = 0
                scrollToBottom(proxy, animated: false)
                DispatchQueue.main.async { settled = true }
            }
            .onChange(of: messages.last?.id) { _, _ in
                let fresh = messages.filter { !knownIds.contains($0.id) }.count
                if atBottom {
                    scrollToBottom(proxy)
                } else if fresh > 0 {
                    unseen += fresh
                }
            }
        }
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy, animated: Bool = true) {
        DispatchQueue.main.async {
            if animated && !reduceMotion {
                withAnimation(Motion.enter) { proxy.scrollTo("bottom", anchor: .bottom) }
            } else {
                proxy.scrollTo("bottom", anchor: .bottom)
            }
            unseen = 0
        }
    }
}

private struct MessageRow: View {
    let message: ChatMessage
    let transcript: URL?
    let animateIn: Bool

    @State private var shown = false
    @State private var expanded = false
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        content
            .opacity(shown || !animateIn ? 1 : 0)
            .offset(y: shown || !animateIn || reduceMotion ? 0 : 8)
            .onAppear {
                guard animateIn, !shown else { return }
                withAnimation(reduceMotion ? Motion.fade : Motion.enter) { shown = true }
            }
    }

    @ViewBuilder
    private var content: some View {
        switch message.role {
        case .user:
            HStack(spacing: 0) {
                Spacer(minLength: 48)
                VStack(alignment: .trailing, spacing: Space.sm) {
                    if !message.text.isEmpty {
                        MarkdownText(text: message.text)
                    }
                    if message.queued {
                        Label("đang chờ", systemImage: "clock").font(TypeToken.labelSm).foregroundStyle(Token.statusInput)
                    }
                    ChatPictures(images: message.images, transcript: transcript)
                }
                .padding(Space.md)
                .background(Token.surfaceRaised, in: RoundedRectangle(cornerRadius: Radius.lg))
                .opacity(message.queued ? 0.8 : 1)
                .frame(maxWidth: Metric.maxMessageWidth, alignment: .trailing)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Bạn: \(message.text)\(message.queued ? ", đang chờ" : "")")
        case .assistant:
            VStack(alignment: .leading, spacing: Space.sm) {
                if !message.text.isEmpty { MarkdownText(text: message.text) }
                ChatPictures(images: message.images, transcript: transcript)
            }
            .frame(maxWidth: Metric.maxMessageWidth, alignment: .leading)
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Claude: \(message.text)")
        case .tools:
            VStack(alignment: .leading, spacing: Space.sm) {
            Button {
                withAnimation(Motion.hover) { expanded.toggle() }
            } label: {
                HStack(alignment: .top, spacing: Space.sm) {
                    Image(systemName: "chevron.right").font(.system(size: 9, weight: .semibold))
                        .rotationEffect(.degrees(expanded ? 90 : 0)).padding(.top, 3)
                    Text(message.toolSummary ?? message.text)
                        .font(TypeToken.codeSm).lineLimit(expanded ? nil : 1)
                        .multilineTextAlignment(.leading)
                }
                .foregroundStyle(Token.muted)
                .frame(maxWidth: Metric.maxMessageWidth, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Công cụ: \(message.toolSummary ?? message.text)")
            .accessibilityHint(expanded ? "Thu gọn" : "Mở rộng")
            if !message.images.isEmpty { ChatPictures(images: message.images, transcript: transcript) }
            }
        }
    }
}

private struct Composer: View {
    @Bindable var model: ConsoleModel
    let session: Session
    /// The pending card owns the screen's one primary button; Send steps down to secondary meanwhile.
    let cardIsShowing: Bool
    let readOnlyReason: String?

    @State private var contentHeight: CGFloat = 20

    private var control: SessionControl { session.control }
    private var isViewOnly: Bool { control == .viewOnly || readOnlyReason != nil }
    private var state: SendState { model.sendState(for: session.id) }
    private var draft: Binding<String> {
        Binding(get: { model.drafts[session.id] ?? "" }, set: { model.drafts[session.id] = $0 })
    }
    private var canSend: Bool {
        !isViewOnly && state != .sending && !draft.wrappedValue.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    var body: some View {
        VStack(alignment: .leading, spacing: Space.sm) {
            if case .failed(let message, _) = state {
                HStack(spacing: Space.sm) {
                    Image(systemName: "xmark.octagon.fill").font(.system(size: 11)).accessibilityHidden(true)
                    Text("Không gửi được: \(message)").font(TypeToken.bodySm).lineLimit(2)
                    Spacer()
                    Button("Thử lại") { model.retrySend(for: session) }.buttonStyle(SecondaryButtonStyle())
                        .accessibilityLabel("Thử gửi lại")
                    Button("Bỏ") { model.dismissSendError(for: session.id) }.buttonStyle(SecondaryButtonStyle())
                        .accessibilityLabel("Bỏ lỗi")
                }
                .foregroundStyle(Token.statusError)
            }
            if isViewOnly {
                viewOnlyRow
            } else {
                inputRow
            }
        }
        .padding(Space.md)
        .background(Token.surface)
        .overlay(alignment: .top) { Divider1() }
    }

    private var inputRow: some View {
        HStack(alignment: .bottom, spacing: Space.sm) {
            ZStack(alignment: .topLeading) {
                ComposerTextView(text: draft, contentHeight: $contentHeight, isEnabled: state != .sending) {
                    if canSend { model.send(draft.wrappedValue, to: session) }
                }
                if draft.wrappedValue.isEmpty {
                    Text("Nhắn cho session này. Enter gửi, Shift-Enter xuống dòng")
                        .font(TypeToken.bodyMd).foregroundStyle(Token.muted)
                        .padding(.top, 2).allowsHitTesting(false).accessibilityHidden(true)
                }
            }
            .padding(.horizontal, Space.sm).padding(.vertical, Space.sm - 2)
            .frame(height: min(max(Metric.inputHeight, contentHeight + 12), 140))
            .background(Token.neutral, in: RoundedRectangle(cornerRadius: Radius.md))
            .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))

            sendButton
        }
    }

    @ViewBuilder
    private var sendButton: some View {
        let action = { model.send(draft.wrappedValue, to: session) }
        let label = HStack(spacing: 6) {
            if state == .sending { ProgressView().controlSize(.small).scaleEffect(0.7).frame(width: 12, height: 12) }
            else { Image(systemName: "paperplane.fill").font(.system(size: 10)) }
            Text(state == .sending ? "Đang gửi" : "Gửi")
        }
        Group {
            if cardIsShowing {
                Button(action: action) { label }.buttonStyle(SecondaryButtonStyle())
            } else {
                Button(action: action) { label }.buttonStyle(PrimaryButtonStyle())
            }
        }
        .disabled(!canSend)
        .keyboardShortcut(.return, modifiers: .command)
        .accessibilityLabel(state == .sending ? "Đang gửi" : "Gửi tin nhắn")
        .help("Gửi (Enter hoặc ⌘Enter)")
        .frame(height: Metric.inputHeight, alignment: .bottom)
    }

    private var viewOnlyRow: some View {
        HStack(spacing: Space.md) {
            Image(systemName: "eye").font(.system(size: 12)).foregroundStyle(Token.muted).accessibilityHidden(true)
            Text(readOnlyReason ?? "Session mở ngoài Console — chỉ xem")
                .font(TypeToken.bodyMd).foregroundStyle(Token.muted)
            Spacer()
            if readOnlyReason == nil {
                Button { model.openOnMac(session) } label: {
                    Label("Mở trên Mac", systemImage: "arrow.up.forward.app")
                }
                .buttonStyle(SecondaryButtonStyle())
                .accessibilityLabel("Mở session này trên Mac")
            }
        }
        .frame(minHeight: Metric.inputHeight)
        .padding(.horizontal, Space.sm)
        .background(Token.neutral.opacity(0.6), in: RoundedRectangle(cornerRadius: Radius.md))
        .overlay(RoundedRectangle(cornerRadius: Radius.md).stroke(Token.border, lineWidth: 1))
        .accessibilityElement(children: .contain)
    }
}

/// "Tiếp quản": enabled when the original is gone, or still running but stoppable (the confirmation then offers
/// "Dừng bản gốc và tiếp quản").
struct TakeoverButton: View {
    @Bindable var model: ConsoleModel
    let session: Session

    var body: some View {
        let state = Takeover.state(for: session, paths: model.paths)
        let reason: String? = switch state {
        case .available?, .stoppable?: nil
        case .unavailable(let why)?: why
        case nil: TakeoverError.alreadyControllable.localizedDescription
        }
        let stoppable: Bool = { if case .stoppable? = state { return true } else { return false } }()
        Button { model.requestTakeover(session) } label: { Label("Tiếp quản", systemImage: "arrow.down.right.and.arrow.up.left") }
            .buttonStyle(SecondaryButtonStyle())
            .disabled(reason != nil)
            .help(reason ?? (stoppable ? "Dừng bản đang mở ở nơi khác rồi mở lại trong Console" : "Mở lại session này trong Terminal của Console để nhắn từ điện thoại"))
            .accessibilityLabel(reason.map { "Tiếp quản, không dùng được: \($0)" } ?? "Tiếp quản session")
    }
}

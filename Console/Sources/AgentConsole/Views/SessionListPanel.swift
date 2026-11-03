import ConsoleCore
import SwiftUI

struct SessionListPanel: View {
    @Bindable var model: ConsoleModel
    @FocusState private var listFocused: Bool

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(title: "Session") {
                Button {
                    model.showingNewSession = true
                } label: {
                    Label("Session mới", systemImage: "plus")
                }
                .buttonStyle(SecondaryButtonStyle())
                .keyboardShortcut("n", modifiers: .command)
                .accessibilityLabel("Session mới")
                .help("Session mới (⌘N)")
            }
            content
        }
        .background(Token.surface)
    }

    @ViewBuilder
    private var content: some View {
        if let message = model.store.loadError {
            ErrorState(message: message) { model.store.reload() }
        } else if !model.store.hasLoaded {
            SkeletonRows().frame(maxHeight: .infinity, alignment: .top)
        } else if model.sessions.isEmpty && model.starting.isEmpty {
            EmptySessions { model.showingNewSession = true }
        } else if model.filteredSessions.isEmpty && model.starting.isEmpty {
            VStack(spacing: Space.sm) {
                Text("Không có session khớp \"\(model.searchText)\"").font(TypeToken.bodySm).foregroundStyle(Token.muted)
                Button("Xoá tìm kiếm") { model.searchText = "" }.buttonStyle(SecondaryButtonStyle())
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            list
        }
    }

    private var list: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 0, pinnedViews: []) {
                    ForEach(model.starting) { entry in
                        StartingRow(entry: entry, isSelected: model.store.selection?.sessionId == entry.selectionId) {
                            model.select(ChatSelection(sessionId: entry.selectionId))
                        }
                    }
                    ForEach(SessionState.allCases, id: \.self) { state in
                        let group = model.filteredSessions.filter { $0.state == state }
                        if !group.isEmpty {
                            GroupHeader(state: state, count: group.count)
                            ForEach(group) { session in
                                SessionRow(model: model, session: session)
                                    .id(session.id)
                                if model.isExpanded(session.id) {
                                    ForEach(session.subagents) { agent in
                                        SubagentRow(model: model, session: session, agent: agent)
                                            .id("\(session.id)/\(agent.id)")
                                    }
                                }
                            }
                        }
                    }
                }
            }
            .focusable()
            .focused($listFocused)
            .focusEffectDisabled()
            .onKeyPress(.upArrow) { model.moveSelection(by: -1); return .handled }
            .onKeyPress(.downArrow) { model.moveSelection(by: 1); return .handled }
            .onKeyPress(.rightArrow) {
                guard let id = model.store.selection?.sessionId else { return .ignored }
                model.setExpanded(id, true)
                return .handled
            }
            .onKeyPress(.leftArrow) {
                guard let id = model.store.selection?.sessionId else { return .ignored }
                model.setExpanded(id, false)
                return .handled
            }
            .onChange(of: model.store.selection) { _, selection in
                guard let selection else { return }
                proxy.scrollTo(selection.subagentId.map { "\(selection.sessionId)/\($0)" } ?? selection.sessionId)
            }
        }
    }
}

private struct EmptySessions: View {
    let create: () -> Void

    var body: some View {
        VStack(spacing: Space.md) {
            Image(systemName: "terminal").font(.system(size: 20)).foregroundStyle(Token.muted).accessibilityHidden(true)
            Text("Chưa có session Claude Code nào đang mở.").font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
            Text("Mở một session trong Console để nhắn từ đây.").font(TypeToken.bodySm).foregroundStyle(Token.muted)
            Button(action: create) { Label("Session mới", systemImage: "plus") }
                .buttonStyle(SecondaryButtonStyle())
                .accessibilityLabel("Session mới")
        }
        .multilineTextAlignment(.center)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

private struct GroupHeader: View {
    let state: SessionState
    let count: Int

    var body: some View {
        HStack(spacing: Space.sm) {
            Text(state.vietnamese).font(TypeToken.labelSm).foregroundStyle(Token.muted)
            Text("\(count)")
                .labelSmStyle().foregroundStyle(Token.muted)
                .contentTransition(.numericText())
                .animation(Motion.enter, value: count)
            Spacer()
        }
        .padding(.horizontal, Space.md)
        .frame(height: 24)
        .overlay(alignment: .top) { Divider1() }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(state.vietnamese), \(count) session")
        .accessibilityAddTraits(.isHeader)
    }
}

/// Owned process that has not become a session yet (typically waiting on the folder-trust prompt).
private struct StartingRow: View {
    let entry: StartingTerminal
    let isSelected: Bool
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            HStack(spacing: Space.sm) {
                Image(systemName: "terminal").font(.system(size: 11)).foregroundStyle(Token.muted).frame(width: 16)
                Text((entry.folder as NSString).lastPathComponent).font(TypeToken.bodyMd).foregroundStyle(Token.onSurface)
                Text("đang khởi động, mở Terminal để trả lời").font(TypeToken.bodySm).foregroundStyle(Token.muted).lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, Space.md)
            .frame(height: Metric.sessionRowHeight)
            .background(isSelected ? Token.surfaceRaised : Color.clear)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Session đang khởi động trong \((entry.folder as NSString).lastPathComponent). Mở Terminal để trả lời prompt.")
    }
}

struct SessionRow: View {
    @Bindable var model: ConsoleModel
    let session: Session
    @State private var hovering = false

    private var isSelected: Bool {
        model.store.selection == ChatSelection(sessionId: session.id)
    }

    private var runningSubagents: Int { session.subagents.filter { $0.state == .running }.count }

    var body: some View {
        HStack(spacing: Space.sm) {
            disclosure
            StatusDot(state: session.state)
            Image(systemName: session.state.symbol)
                .font(.system(size: 10)).foregroundStyle(session.state.labelColor).frame(width: 12)
                .accessibilityHidden(true)
            Text(session.name).font(TypeToken.bodyMd).foregroundStyle(Token.onSurface).lineLimit(1)
            Text(session.project).font(TypeToken.bodySm).foregroundStyle(Token.muted).lineLimit(1)
            if runningSubagents > 0 {
                HStack(spacing: 2) {
                    Image(systemName: "arrow.triangle.branch").font(.system(size: 9))
                    Text("\(runningSubagents)").labelSmStyle().contentTransition(.numericText())
                }
                .foregroundStyle(Token.statusRunning)
                .padding(.horizontal, 4).frame(height: 16)
                .overlay(RoundedRectangle(cornerRadius: Radius.sm).stroke(Token.border, lineWidth: 1))
                .accessibilityHidden(true)
            }
            Spacer(minLength: Space.sm)
            if session.control == .viewOnly {
                Image(systemName: "eye").font(.system(size: 11)).foregroundStyle(Token.muted)
                    .help("Session mở ngoài Console, chỉ xem. Nhắn chữ vào đây cần mở nó từ Console.")
                    .accessibilityHidden(true)
            }
            if session.state == .needsInput { InputBadge() }
            RelativeTimeLabel(date: session.lastActivity).frame(minWidth: 48, alignment: .trailing)
        }
        .padding(.horizontal, Space.md)
        .frame(height: Metric.sessionRowHeight)
        .background(rowBackground)
        .overlay(alignment: .leading) {
            if isSelected { Token.primary.frame(width: 2) }
        }
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .animation(Motion.hover, value: hovering)
        .onTapGesture { model.select(ChatSelection(sessionId: session.id)) }
        .contextMenu {
            if session.control == .viewOnly {
                let state = Takeover.state(for: session, paths: model.paths)
                let blocked: String? = { if case .unavailable(let why)? = state { return why } else { return nil } }()
                Button("Tiếp quản…") { model.requestTakeover(session) }.disabled(blocked != nil)
                if let blocked { Text(blocked) }
            }
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(spokenLabel)
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction { model.select(ChatSelection(sessionId: session.id)) }
        .accessibilityAction(named: "Mở hoặc thu gọn subagent") {
            if !session.subagents.isEmpty { model.setExpanded(session.id, !model.isExpanded(session.id)) }
        }
    }

    private var rowBackground: Color {
        if isSelected { return Token.surfaceRaised }
        return hovering ? Token.surfaceRaised.opacity(0.5) : Color.clear
    }

    @ViewBuilder
    private var disclosure: some View {
        if session.subagents.isEmpty {
            Color.clear.frame(width: 14, height: 14)
        } else {
            Button {
                model.setExpanded(session.id, !model.isExpanded(session.id))
            } label: {
                Image(systemName: "chevron.right")
                    .font(.system(size: 9, weight: .semibold))
                    .rotationEffect(.degrees(model.isExpanded(session.id) ? 90 : 0))
                    .animation(Motion.hover, value: model.isExpanded(session.id))
                    .frame(width: 14, height: 14)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .foregroundStyle(Token.muted)
            .accessibilityLabel(model.isExpanded(session.id) ? "Thu gọn \(session.subagents.count) subagent" : "Mở \(session.subagents.count) subagent")
        }
    }

    private var spokenLabel: String {
        var parts = [session.name, "project \(session.project)", "trạng thái \(session.state.vietnamese)"]
        if runningSubagents > 0 { parts.append("\(runningSubagents) subagent đang chạy") }
        if session.control == .viewOnly { parts.append("chỉ xem") }
        if let pending = session.pending { parts.append("đang chờ: \(pending.summary)") }
        parts.append(RelativeTimeLabel.spoken(since: session.lastActivity, now: Date()))
        return parts.joined(separator: ", ")
    }
}

struct SubagentRow: View {
    @Bindable var model: ConsoleModel
    let session: Session
    let agent: Subagent
    @State private var hovering = false

    private var selection: ChatSelection { ChatSelection(sessionId: session.id, subagentId: agent.id) }
    private var isSelected: Bool { model.store.selection == selection }
    private var state: SessionState { agent.state == .running ? .running : .done }

    var body: some View {
        HStack(spacing: Space.sm) {
            StatusDot(state: state)
            Text(agent.type).font(TypeToken.bodySm).foregroundStyle(Token.muted)
            Text("·").foregroundStyle(Token.muted).accessibilityHidden(true)
            Text(agent.description).font(TypeToken.bodySm).foregroundStyle(Token.muted).lineLimit(1)
            if !agent.model.isEmpty {
                Text("·").foregroundStyle(Token.muted).accessibilityHidden(true)
                Text(agent.model).font(TypeToken.codeSm).foregroundStyle(Token.muted)
            }
            Spacer(minLength: Space.sm)
            Text(state.vietnamese).labelSmStyle().foregroundStyle(state.labelColor)
        }
        .padding(.leading, Space.md + 14 + Space.sm + Metric.subagentIndent - Space.sm)
        .padding(.trailing, Space.md)
        .frame(height: Metric.subagentRowHeight)
        .background(isSelected ? Token.surfaceRaised : (hovering ? Token.surfaceRaised.opacity(0.5) : Color.clear))
        .overlay(alignment: .leading) { if isSelected { Token.primary.frame(width: 2) } }
        .contentShape(Rectangle())
        .onHover { hovering = $0 }
        .animation(Motion.hover, value: hovering)
        .onTapGesture { model.select(selection) }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("Subagent \(agent.type): \(agent.description), model \(agent.model), \(state.vietnamese). Chỉ xem transcript.")
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
        .accessibilityAction { model.select(selection) }
    }
}

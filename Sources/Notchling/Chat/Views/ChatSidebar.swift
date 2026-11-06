import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

struct SessionSidebar: View {
    @ObservedObject var model: ChatModel
    @ObservedObject var notch: NotchModel
    @FocusState private var listFocused: Bool
    @State private var hoveredRow: String?
    @FocusState private var searchFocused: Bool

    var body: some View {
        let shown = model.shown
        VStack(alignment: .leading, spacing: 0) {
            // The traffic lights live here.
            Color.clear.frame(height: KitMetric.titleBar)
            HStack(alignment: .firstTextBaseline) {
                Text("Sessions").kitType(.title).foregroundColor(KitInk.text)
                Spacer()
                Button { model.newSessionSheet = true } label: {
                    Image(systemName: "plus").font(.system(size: 12, weight: .semibold))
                }
                .buttonStyle(.kit(.ghost))
                .help("New session: pick a Claude Code or Codex account and a folder")
                .part("New Session Button", .bottomTrailing)
                if notch.needsCount > 0 {
                    KitStatusPill(group: .needsInput, text: notch.needsCount == 1 ? "1 needs you" : "\(notch.needsCount) need you")
                }
            }
            .padding(.horizontal, 16).padding(.bottom, 10)
            // Filters appear once there's enough to filter.
            if notch.agents.count > 6 {
                HStack(spacing: 6) {
                    KitIconView(icon: .search, size: 11).foregroundColor(KitInk.textFaint)
                    TextField("Search", text: $model.search).focused($searchFocused)
                }
                .modifier(KitField(focused: searchFocused))
                .part("Session Search", .trailing)
                .padding(.horizontal, 12).padding(.bottom, 8)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 2) {
                    ForEach(AgentGroup.allCases, id: \.self) { group in
                        let agents = shown.filter { $0.group == group }
                        if !agents.isEmpty {
                            Text(group == .needsInput ? "Needs You" : group == .working ? "Working" : "Done")
                                .kitType(.section).foregroundColor(KitInk.textFaint)
                                .padding(.horizontal, 10).padding(.top, 12).padding(.bottom, 4)
                                .part("Group Heading", .trailing)
                            ForEach(agents, id: \.id) { agent in
                                let selected = model.selection == agent.id
                                let showsMenu = hoveredRow == agent.id || selected
                                Button { model.selection = agent.id; listFocused = true } label: {
                                    SessionRowLabel(agent: agent, line: statusLine(agent),
                                                    account: model.account(for: agent),
                                                    showsMenu: showsMenu)
                                }
                                .buttonStyle(KitSessionRowStyle(selected: selected, focused: listFocused))
                                .accessibilityAddTraits(selected ? .isSelected : [])
                                .part("Session Row", .trailing)
                                .onHover { inside in
                                    if inside { hoveredRow = agent.id } else if hoveredRow == agent.id { hoveredRow = nil }
                                }
                                .overlay(alignment: .trailing) {
                                    if showsMenu {
                                        Menu { rowActions(agent) } label: {
                                            KitIconView(icon: .more, size: 13).foregroundColor(KitInk.textMuted)
                                        }
                                        .menuStyle(.borderlessButton)
                                        .menuIndicator(.hidden)
                                        .fixedSize()
                                        .padding(.trailing, 10)
                                        .help("Session Actions")
                                        .accessibilityLabel("Session Actions")
                                        .pointerCursor()
                                        .part("Session Menu", .trailing)
                                    }
                                }
                                // The notch's actions: Hide drops it from the lists until it changes state;
                                // Remove stops a background session and deletes it (`claude rm`), after asking.
                                .contextMenu { rowActions(agent) }
                            }
                        }
                    }
                }
                .padding(.horizontal, 8).padding(.bottom, 12)
            }
            .focusable()
            .focused($listFocused)
            .onMoveCommand { direction in
                let ids = AgentGroup.allCases.flatMap { g in shown.filter { $0.group == g }.map(\.id) }
                guard !ids.isEmpty else { return }
                let index = model.selection.flatMap { ids.firstIndex(of: $0) } ?? -1
                switch direction {
                case .up: model.selection = ids[max(index - 1, 0)]
                case .down: model.selection = ids[min(index + 1, ids.count - 1)]
                default: break
                }
            }
        }
        .safeAreaInset(edge: .bottom, spacing: 0) {
            // The ? button: names every part on screen, here and in the notch.
            HStack {
                KitPartsButton()
                Spacer()
                footerButton("server.rack", "Machines", "Machines: the overnight worker Macs and servers") { model.openMachines?() }
                footerButton("doc.richtext", "Artifacts", "Artifacts saved on this Mac") { model.openArtifacts?() }
                footerButton("key", "Secret Vault", "Secret Vault: passwords and tokens agents can use without seeing them") {
                    model.vaultSheet = true
                }
                footerButton("gearshape", "Settings", "Accounts, token cost and settings (⌘,)") {
                    model.appTab.page = .settings
                    model.appSheet = true
                }
            }
                .padding(.horizontal, 10).padding(.vertical, 8)
        }
        .onChange(of: model.sidebarFocusRequest) { _ in listFocused = true }
    }

    /// An icon button in the sidebar's bottom bar, tagged with its name for the ? button.
    private func footerButton(_ icon: String, _ name: String, _ help: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            Image(systemName: icon).font(.system(size: 13, weight: .medium)).foregroundColor(KitInk.textMuted)
        }
        .buttonStyle(.kit(.ghost))
        .help(help)
        .accessibilityLabel(name)
        .part("\(name) Button", .top)
    }

    /// One list for the right-click menu and the ⋯ menu, so they never differ.
    @ViewBuilder private func rowActions(_ agent: Agent) -> some View {
        if ChatTakeover.offersTakeover(agent) {
            Button("Take Over…") { ChatTakeover.run(agent) { model.selection = $0 } }
            Divider()
        }
        if !agent.cwd.isEmpty, FileManager.default.fileExists(atPath: agent.cwd) {
            Button("Reveal in Finder") { NSWorkspace.shared.activateFileViewerSelecting([URL(fileURLWithPath: agent.cwd)]) }
        }
        if let id = agent.sessionID {
            Button("Copy Session ID") {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(id, forType: .string)
            }
        }
        Divider()
        switch agent.target {
        case .codex, .antigravity, .gemini:
            Button(agent.target.isDraft ? "Discard New Session" : "Close Session") { model.closeSession(agent) }
        default:
            Button("Hide") { notch.onHide?(agent) }
        }
        if case .job = agent.target {
            Divider()
            Button("Remove Background Session…") { notch.onRemove?(agent) }
        }
        if case .codex = agent.target {
            Divider()
            Button("Delete Codex Session…") { model.removeCodex(agent) }
        }
    }

    /// What the row says under the name: what it's doing, or what it waits for.
    private func statusLine(_ agent: Agent) -> String {
        switch agent.group {
        case .working: return notch.snapshot(agent.id).activity ?? "Working"
        case .needsInput: return notch.question(agent.id) != nil ? "Asking you a question" : notch.permission(agent.id) != nil ? "Asking permission" : "Waiting for you"
        case .completed: return agent.label == "Idle" ? "Idle · \(agent.project)" : agent.label
        }
    }
}

private struct SessionRowLabel: View {
    let agent: Agent
    let line: String
    var account: ChatModel.SessionAccount?
    /// The ⋯ menu sits where the age is, so the age hides but keeps its space and the row doesn't jump.
    var showsMenu = false

    var body: some View {
        HStack(spacing: 10) {
            KitAvatar(name: agent.name, size: 30, status: agent.group)
            VStack(alignment: .leading, spacing: 2) {
                // Three lines: who, on which account, doing what.
                HStack(spacing: 5) {
                    ToolIcon(tool: SessionTool(agent.target), size: 14).part("Tool Icon", .leading)
                    Text(agent.name).kitType(.heading).foregroundColor(KitInk.text).lineLimit(1)
                }
                if let account {
                    HStack(spacing: 4) {
                        Image(systemName: account.lane == nil ? "person.crop.circle" : "arrow.triangle.branch")
                            .font(.system(size: 9, weight: .semibold))
                        Text(account.email).lineLimit(1).truncationMode(.middle)
                    }
                    .kitType(.meta).foregroundColor(KitInk.textFaint)
                }
                Text(line).kitType(.meta).foregroundColor(agent.group == .needsInput ? KitInk.accentText : KitInk.textMuted).lineLimit(1)
            }
            Spacer(minLength: 4)
            Text(ChatFormat.age(agent.since)).kitType(.meta).foregroundColor(KitInk.textFaint)
                .opacity(showsMenu ? 0 : 1)
        }
        .help("\(agent.label) · \(agent.cwd)")
        .accessibilityElement(children: .combine)
        .accessibilityLabel("\(agent.name), \(agent.label), \(agent.project)")
    }
}

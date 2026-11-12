import SwiftUI

struct Tag: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(size: 11, weight: .medium))
            .foregroundColor(.white.opacity(0.6))
            .lineLimit(1)
            .padding(.horizontal, 7)
            .padding(.vertical, 3)
            .background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(0.09)))
    }
}

/// The three tabs that sit right under the camera.
struct TabBar: View {
    @ObservedObject var model: NotchModel
    @State private var shown = false

    static let height: CGFloat = 34

    var body: some View {
        HStack(spacing: 4) {
            ForEach(AgentGroup.allCases, id: \.self) { group in
                let count = model.listed.filter { $0.group == group }.count + model.runs(in: group).count
                let selected = model.tab == group
                Button { model.tab = group } label: {
                    HStack(spacing: 4) {
                        Circle().fill(group.color).frame(width: 5, height: 5)
                        Text(group.shortTitle).font(.system(size: 10.5, weight: .semibold))
                        Text("\(count)").font(.system(size: 10.5, weight: .bold, design: .rounded))
                            .foregroundColor(count > 0 ? group.color : .secondary)
                    }
                    .foregroundColor(selected ? .white : .secondary)
                    .padding(.horizontal, 4)
                    .frame(maxWidth: .infinity, minHeight: 26)
                    .background(Capsule().fill(Color.white.opacity(selected ? 0.14 : 0)))
                    .contentShape(Capsule())
                }
                .buttonStyle(.kitPlain)
            }
        }
        .padding(.horizontal, 4)
        .frame(height: Self.height)
        // Grow out of the middle once the mark and count have slid in under the camera.
        .scaleEffect(x: shown ? 1 : 0.15, y: shown ? 1 : 0.6, anchor: .center)
        .opacity(shown ? 1 : 0)
        .onAppear {
            withAnimation(.spring(response: 0.34, dampingFraction: 0.86).delay(0.22)) { shown = true }
        }
    }
}

struct AgentRow: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    var trailing: String?
    @State private var hovered = false

    var body: some View {
        let snapshot = model.snapshot(agent.id)
        HStack(alignment: .top, spacing: 9) {
            Circle().fill(agent.color).frame(width: 8, height: 8)
                .frame(width: 10, height: 18)

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    if NotchForeign.isForeign(agent) {
                        ToolIcon(tool: SessionTool(agent.target), size: 13).part("Tool Icon", .bottom)
                    }
                    Text(agent.name).font(.system(size: 12.5, weight: .medium)).lineLimit(1)
                    // Subagents at work, which can go on after the session's own turn ended.
                    if snapshot.subagentsActive > 0 {
                        HStack(spacing: 2) {
                            Image(systemName: "person.2.fill").font(.system(size: 8.5))
                            Text("\(snapshot.subagentsActive)").font(.system(size: 10.5, weight: .semibold)).monospacedDigit()
                        }
                        .foregroundColor(.asks)
                        .padding(.horizontal, 5).padding(.vertical, 1)
                        .background(Capsule().fill(Color.asks.opacity(0.18)))
                        .help(snapshot.subagentsActive == 1 ? "1 subagent working" : "\(snapshot.subagentsActive) subagents working")
                        .part("Subagent Badge", .bottom)
                    }
                    Spacer(minLength: 4)
                    Text("\(agent.shortHost) · \(trailing ?? agent.ago)")
                        .font(.system(size: 10.5)).foregroundColor(.secondary).lineLimit(1)
                        .fixedSize()
                }
                if agent.group == .completed {
                    if let text = snapshot.lastReply ?? agent.detail {
                        Text(oneLine(text)).font(.system(size: 12)).foregroundColor(.secondary).lineLimit(1)
                    }
                } else {
                    if let prompt = snapshot.lastPrompt {
                        Text("You: " + oneLine(prompt))
                            .font(.system(size: 12)).foregroundColor(.secondary).lineLimit(1)
                    }
                    Text(oneLine(statusLine(snapshot)))
                        .font(.system(size: 12)).foregroundColor(agent.color).lineLimit(1)
                }
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .frame(height: model.rowHeight(agent), alignment: .top)
        .background(RoundedRectangle(cornerRadius: 10).fill(Color.white.opacity(hovered ? 0.08 : 0)))
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
    }

    private func statusLine(_ snapshot: Snapshot) -> String {
        if let r = model.permission(agent.id) { return "Wants to use \(r.toolName): \(r.summary)" }
        if let p = model.agyPermission(agent.id) { return "Wants to \(p.call.title)" }
        if let q = model.question(agent.id), let first = q.items.first { return "Asks: \(first.question)" }
        if agent.group == .needsInput { return agent.detail ?? snapshot.lastReply ?? agent.label }
        return snapshot.activity ?? agent.detail ?? "Thinking…"
    }
}

struct AgentListView: View {
    @ObservedObject var model: NotchModel

    var body: some View {
        let members = model.listed.filter { $0.group == model.tab }
        let runs = model.runs(in: model.tab)
        VStack(spacing: 0) {
            TabBar(model: model).part("Notch Tabs", .trailing)
            if members.isEmpty && runs.isEmpty {
                Text(model.listed.isEmpty ? (model.foreignAgents.isEmpty ? "No Claude agents running" : "No agents running") : "Nothing \(model.tab.title.lowercased())")
                    .font(.system(size: 12)).foregroundColor(.secondary)
                    .frame(maxWidth: .infinity, minHeight: 40)
            } else {
                ScrollView {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(members, id: \.id) { agent in
                            AgentRow(model: model, agent: agent).part("Notch Session Row", .trailing)
                                .onTapGesture { model.open(agent.id) }
                                .pointerCursor()
                                .contextMenu {
                                    Button("Open in Chat") { model.go(.collapsed); model.onOpenChat?(agent.id) }
                                    Button("Hide") { model.onHide?(agent) }
                                    if case .job = agent.target {
                                        Divider()
                                        Button("Remove Agent…") { model.onRemove?(agent) }
                                    }
                                }
                        }
                        if !runs.isEmpty {
                            Text("Runs").font(KitType.section.font).foregroundColor(.secondary)
                                .padding(.horizontal, 8)
                                .frame(height: NotchModel.runsHeaderHeight, alignment: .bottom)
                            ForEach(runs) { RunRow(item: $0) }
                        }
                        if model.tab == .completed {
                            Button("Clear all") { model.onClearCompleted?() }
                                .buttonStyle(.kitPlain)
                                .font(.system(size: 11)).foregroundColor(.secondary)
                                .frame(maxWidth: .infinity)
                                .frame(height: 24)
                        }
                    }
                    .padding(.leading, 6)
                    .padding(.trailing, 12)
                    .padding(.top, 2)
                    .padding(.bottom, 6)
                }
            }
        }
    }
}

/// A long run sd started: its name, how long it has gone (of its limit), the Mac's load and the last line of its log.
struct RunRow: View {
    let item: RunItem
    @State private var hovered = false

    private var color: Color {
        switch (item.kind, item.rc) {
        case (.active, _): return AgentGroup.working.color
        case (_, 0?): return AgentGroup.completed.color
        case (_, 124?): return .orange
        default: return .red
        }
    }

    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            Circle().fill(color).frame(width: 8, height: 8)
                .frame(width: 10, height: 18)
            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(item.name).font(KitType.label.font).lineLimit(1)
                    Spacer(minLength: 4)
                    Text(item.trailing).font(KitType.meta.font).foregroundColor(.secondary).lineLimit(1).fixedSize()
                }
                Text(oneLine(item.detail)).font(KitType.label.font).foregroundColor(item.kind == .active ? color : .secondary).lineLimit(1)
            }
        }
        .padding(.horizontal, 8)
        .padding(.vertical, 6)
        .frame(height: NotchModel.runRowHeight, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 10).fill(KitInk.terminalText.opacity(hovered ? 0.08 : 0)))
        .contentShape(Rectangle())
        .onHover { hovered = $0 }
        .onTapGesture { open() }
        .pointerCursor()
        .contextMenu { Button("Open Log") { open() } }
        .help(item.log)
    }

    private func open() { NSWorkspace.shared.open(URL(fileURLWithPath: item.log)) }
}

import SwiftUI

/// "Antigravity wants to run …": the call (an edit's diff, or the command) and Antigravity's own options, in the
/// order `NotchForeign.orderedOptions` gives. Answering goes through the runner Chat reads, so the card in Chat
/// clears too.
struct AgyPermissionCard: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    let permission: ACPPermission
    var showsAgent = false

    var body: some View {
        let options = NotchForeign.orderedOptions(permission)
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "lock.shield.fill").foregroundColor(.orange)
                Text("Permission needed").font(.system(size: 13, weight: .semibold)).foregroundColor(.orange)
                Spacer()
                if showsAgent {
                    ToolIcon(tool: .antigravity, size: 13)
                    Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                    HideButton(model: model).part("Hide Button", .bottom)
                }
            }
            .frame(height: 22)

            Text("Antigravity wants to **\(permission.call.title)**")
                .font(.system(size: 13.5)).lineLimit(2).fixedSize(horizontal: false, vertical: true)
            VStack(alignment: .leading, spacing: 1) {
                ForEach(Array(NotchForeign.preview(of: permission).enumerated()), id: \.offset) { _, line in
                    Text(Self.mark(line.kind) + line.text)
                        .font(.system(size: 12, design: .monospaced))
                        .foregroundColor(Self.color(line.kind))
                        .lineLimit(2)
                }
            }
            .textSelection(.enabled)
            .fixedSize(horizontal: false, vertical: true)
            .padding(8)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.07)))

            ForEach(Array(options.enumerated()), id: \.element.id) { index, option in
                row(index + 1, option)
            }

            Button("Answer in Chat ›") { model.go(.collapsed); model.onOpenChat?(agent.id) }
                .buttonStyle(.kitPlain).font(.system(size: 12)).foregroundColor(.asks)
                .frame(height: 22)
        }
    }

    private func row(_ n: Int, _ option: ACPPermission.Option) -> some View {
        Button { model.decideAntigravity(option, for: agent.id) } label: {
            HStack(spacing: 12) {
                Text("⌘\(n)")
                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                    .foregroundColor(.white.opacity(0.55))
                    .frame(width: 30, height: 24)
                    .background(RoundedRectangle(cornerRadius: 6).fill(Color.optionBadge))
                Text(option.name).font(.system(size: 13.5)).foregroundColor(option.allows ? .green : .red).lineLimit(1)
                Spacer()
            }
            .padding(.horizontal, 8)
            .frame(height: 32)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.optionFill))
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .keyboardShortcut(KeyEquivalent(Character("\(min(n, 9))")), modifiers: .command)
    }

    private static func mark(_ kind: DiffLine.Kind) -> String {
        switch kind {
        case .add: return "+ "
        case .remove: return "- "
        default: return ""
        }
    }

    private static func color(_ kind: DiffLine.Kind) -> Color {
        switch kind {
        case .add: return .green
        case .remove: return .red
        default: return .white.opacity(0.85)
        }
    }
}

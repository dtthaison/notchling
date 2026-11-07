import SwiftUI

/// "Claude wants to run …": allow once, always allow, deny, or hand it back to the terminal.
struct PermissionCard: View {
    @ObservedObject var model: NotchModel
    let agent: Agent
    let request: HookRequest
    var showsAgent = false
    @State private var instead = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 7) {
                Image(systemName: "lock.shield.fill").foregroundColor(.orange)
                Text("Permission needed").font(.system(size: 13, weight: .semibold)).foregroundColor(.orange)
                Spacer()
                if showsAgent {
                    Text(agent.name).font(.system(size: 11)).foregroundColor(.secondary).lineLimit(1)
                    HideButton(model: model).part("Hide Button", .bottom)
                }
            }
            .frame(height: 22)

            Text("Claude wants to use **\(request.toolName)**").font(.system(size: 13.5)).frame(height: 18)
            Text(request.summary)
                .font(.system(size: 12, design: .monospaced))
                .foregroundColor(.white.opacity(0.85))
                .lineLimit(10)
                .fixedSize(horizontal: false, vertical: true)
                .textSelection(.enabled)
                .padding(8)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(RoundedRectangle(cornerRadius: 8).fill(Color.white.opacity(0.07)))

            option(1, "Allow", .green) { model.decide(.allow, for: agent.id) }
            if request.canAlwaysAllow {
                option(2, "Always allow this", .green) { model.decide(.allowAlways, for: agent.id) }
            }
            option(request.canAlwaysAllow ? 3 : 2, "Deny", .red) { model.decide(.deny(nil), for: agent.id) }
            // Like the terminal's "No, and tell Claude what to do differently".
            TextField("Deny, and tell Claude what to do instead…", text: $instead)
                .textFieldStyle(.plain)
                .font(.system(size: 13))
                .onSubmit {
                    let text = instead.trimmingCharacters(in: .whitespacesAndNewlines)
                    guard !text.isEmpty else { return }
                    model.decide(.deny(text), for: agent.id)
                }
                .padding(.horizontal, 12)
                .frame(height: 34)
                .background(RoundedRectangle(cornerRadius: 10).stroke(Color.white.opacity(0.12)))

            Button("Answer in \(agent.host) ›") { model.decide(.terminal, for: agent.id) }
                .buttonStyle(.kitPlain).font(.system(size: 12)).foregroundColor(.asks)
                .frame(height: 22)
        }
    }

    private func option(_ n: Int, _ label: String, _ tint: Color, _ action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Text("⌘\(n)")
                    .font(.system(size: 12, weight: .semibold, design: .rounded))
                    .foregroundColor(.white.opacity(0.55))
                    .frame(width: 30, height: 24)
                    .background(RoundedRectangle(cornerRadius: 6).fill(Color.optionBadge))
                Text(label).font(.system(size: 13.5)).foregroundColor(tint)
                Spacer()
            }
            .padding(.horizontal, 8)
            .frame(height: 32)
            .background(RoundedRectangle(cornerRadius: 10).fill(Color.optionFill))
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .keyboardShortcut(KeyEquivalent(Character("\(n)")), modifiers: .command)
    }
}

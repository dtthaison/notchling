import AppKit
import SwiftUI

/// A permission an Antigravity CLI session asks for mid-turn: what the tool would do (with the edit's
/// diff when there is one) and Antigravity's own options. Stopping the turn answers it as cancelled.
struct AntigravityPermissionCard: View {
    let session: String
    let request: ACPPermission

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(spacing: 8) {
                KitIconView(icon: .permission, size: 13).foregroundColor(KitInk.accentText)
                Text("Allow \(request.call.title)?").kitType(.heading).foregroundColor(KitInk.text).lineLimit(2)
            }
            ScrollView {
                VStack(alignment: .leading, spacing: 1) {
                    if request.call.diff.isEmpty {
                        Text(request.call.input.isEmpty ? request.call.name : request.call.input)
                            .kitType(.mono).foregroundColor(KitInk.text)
                    } else {
                        ForEach(Array(request.call.diff.enumerated()), id: \.offset) { _, line in
                            Text(prefix(line) + line.text)
                                .kitType(.mono)
                                .foregroundColor(line.kind == .add ? KitInk.done : line.kind == .remove ? .red : KitInk.textMuted)
                        }
                    }
                }
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(12)
            }
            .frame(maxHeight: 160)
            .fixedSize(horizontal: false, vertical: true)
            .background(RoundedRectangle(cornerRadius: KitMetric.radiusMd, style: .continuous).fill(KitInk.raised))
            HStack(spacing: 8) {
                ForEach(request.options.filter { !$0.allows }) { option in
                    Button(option.name) { AntigravityRunner.shared.answer(session, option: option.id) }
                        .buttonStyle(.kit(.ghost))
                }
                Spacer()
                // The one-time Allow goes last, where Return's default button sits in the Claude card.
                ForEach(request.options.filter(\.allows).sorted { a, _ in a.kind != "allow_once" }) { option in
                    Button(option.name) { AntigravityRunner.shared.answer(session, option: option.id) }
                        .buttonStyle(.kit(option.kind == "allow_once" ? .primary : .secondary))
                }
            }
        }
        .padding(16)
        .kitCard(radius: KitMetric.radiusComposer)
        .overlay(RoundedRectangle(cornerRadius: KitMetric.radiusComposer, style: .continuous)
            .strokeBorder(KitInk.accent.opacity(0.45), lineWidth: 1))
        .shadow(color: KitInk.shadow, radius: 14, y: 6)
    }

    private func prefix(_ line: DiffLine) -> String {
        switch line.kind {
        case .add: return "+ "
        case .remove: return "- "
        default: return "  "
        }
    }
}

/// An Antigravity CLI session's options (Model, Thinking Level, Mode), as Antigravity lists them for the
/// session; picking one applies from the next turn. A session not started yet shows its tool name.
struct AntigravityOptionsPicker: View {
    @ObservedObject var model: ChatModel
    @ObservedObject private var runner = AntigravityRunner.shared
    let agent: Agent
    let session: String

    private var options: [ACPConfigOption] { session.isEmpty ? [] : runner.config(session) }

    private var label: String {
        let parts = options.filter { $0.id == "model" || $0.id == "thought_level" }.map(\.currentName)
        if session.isEmpty { return "Antigravity" }
        if parts.isEmpty { return runner.isLoading(session) ? "Antigravity · Loading…" : "Antigravity" }
        return (["Antigravity"] + parts).joined(separator: " · ")
    }

    var body: some View {
        Button(action: popUp) {
            HStack(spacing: 4) {
                Image(systemName: "cpu").font(.system(size: 9, weight: .semibold))
                Text(label)
                if !options.isEmpty { Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold)) }
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .disabled(options.isEmpty)
        .help(session.isEmpty ? "Your first message starts the session; then its model and mode can be picked here."
              : "The model, thinking level and mode for this session's next turns.")
    }

    func popUp() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let id = agent.id
        for option in options {
            let item = NSMenuItem(title: option.name, action: nil, keyEquivalent: "")
            let sub = NSMenu()
            sub.autoenablesItems = false
            for value in option.values {
                let row = ActionMenuItem(value.name) { [model, session] in
                    AntigravityRunner.shared.set(session, option: option.id, to: value.value) { failure in
                        model.errors[id] = "Couldn't change it: \(failure)"
                    }
                }
                row.state = value.value == option.current ? .on : .off
                sub.addItem(row)
            }
            item.submenu = sub
            menu.addItem(item)
        }
        let note = NSMenuItem(title: "Applies from the next turn.", action: nil, keyEquivalent: "")
        note.isEnabled = false
        menu.addItem(.separator())
        menu.addItem(note)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

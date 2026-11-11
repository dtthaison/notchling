import AppKit
import Combine
import SwiftUI
import UniformTypeIdentifiers

/// The session's model, and a menu to change it: picking one types `/model <alias>` into the session,
/// as you would in the terminal. Full model ids, so the menu names exactly what you get.
/// The session's effort, switched by sending `/effort <level>` as in the terminal (Claude Code also
/// keeps it as the default for new sessions). The current level is the one on its latest reply.
struct EffortPicker: View {
    @ObservedObject var model: ChatModel
    let current: String?

    static let levels = ["low", "medium", "high", "xhigh", "max"]

    static func title(_ level: String) -> String {
        level == "xhigh" ? "Extra High" : level.prefix(1).uppercased() + level.dropFirst()
    }

    var body: some View {
        Button { Self.popUp(model: model, current: current) } label: {
            HStack(spacing: 4) {
                Image(systemName: "gauge.with.dots.needle.50percent").font(.system(size: 9, weight: .semibold))
                Text(current.map(Self.title) ?? "Effort")
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help("How hard the model thinks. Pick another to switch it (like /effort in the terminal, which also makes it the default for new sessions).")
    }

    static func popUp(model: ChatModel, current: String?) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        for level in levels {
            let row = ActionMenuItem(title(level)) { [model] in model.send("/effort \(level)") }
            row.state = current == level ? .on : .off
            menu.addItem(row)
        }
        let at = NSEvent.mouseLocation
        menu.popUp(positioning: nil, at: at, in: nil)
    }
}

struct ModelPicker: View {
    @ObservedObject var model: ChatModel
    let current: String?

    /// The current models, then older ones under More Models (Claude Code's own /model list).
    static let choices = ["claude-opus-5-5", "claude-fable-5-2", "claude-sonnet-5-5", "claude-haiku-5-5"]
    static let older = ["claude-haiku-4-5", "claude-sonnet-5", "claude-opus-5", "claude-fable-5-1", "claude-fable-5",
                        "claude-opus-4-8", "claude-opus-4-7", "claude-opus-4-6", "claude-sonnet-4-6"]

    var body: some View {
        Button { Self.popUp(model: model, current: current) } label: {
            HStack(spacing: 4) {
                Image(systemName: "cpu").font(.system(size: 9, weight: .semibold))
                Text(current.map(ChatFormat.modelName) ?? "Model")
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help("The model this session uses. Pick another to switch it (like /model in the terminal).")
    }

    /// Also what typing /model in the reply box opens, instead of sending it to a session that can't show a picker.
    static func popUp(model: ChatModel, current: String?) {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let shown = current.map(ChatFormat.modelName)
        func item(_ id: String) -> NSMenuItem {
            let name = ChatFormat.modelName(id)
            let row = ActionMenuItem(name) { [model] in model.send("/model \(id)") }
            row.state = shown == name ? .on : .off
            return row
        }
        Self.choices.forEach { menu.addItem(item($0)) }
        menu.addItem(.separator())
        let more = NSMenuItem(title: "More Models", action: nil, keyEquivalent: "")
        let sub = NSMenu()
        sub.autoenablesItems = false
        Self.older.forEach { sub.addItem(item($0)) }
        more.submenu = sub
        menu.addItem(more)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

/// A Codex session's model and effort, for its next turns: Codex's own list (`CodexModels`), each
/// model's efforts in a submenu, and Default to go back to Codex's setting.
struct CodexModelPicker: View {
    @ObservedObject var model: ChatModel
    let agent: Agent

    private var pick: CodexPick { model.codexPicks[agent.id] ?? CodexPick() }
    private var models: [CodexModel] { CodexModels.load() }

    /// What the next turn uses: the pick, else what the last turn ran with.
    private var label: String {
        let slug = pick.model ?? model.conversation.facts.model
        let name = slug.map { s in models.first { $0.slug == s }?.name ?? s } ?? "Default Model"
        let effort = pick.effort ?? model.conversation.facts.effort
        return ["Codex", name, effort].compactMap { $0 }.joined(separator: " · ")
    }

    var body: some View {
        Button(action: popUp) {
            HStack(spacing: 4) {
                Image(systemName: "cpu").font(.system(size: 9, weight: .semibold))
                Text(label)
                Image(systemName: "chevron.down").font(.system(size: 7, weight: .bold))
            }
            .kitType(.meta).foregroundColor(KitInk.textMuted)
            .fixedSize()
            .contentShape(Rectangle())
        }
        .buttonStyle(.kitPlain)
        .pointerCursor()
        .help("The model and reasoning effort for this session's next turns.")
    }

    func popUp() {
        let menu = NSMenu()
        menu.autoenablesItems = false
        let current = pick
        let id = agent.id
        let running = model.conversation.facts.model
        for m in models {
            let item = ActionMenuItem(m.name) { [model] in
                // A new model starts at its own default effort unless one was picked.
                model.setCodexPick(CodexPick(model: m.slug, effort: current.effort.flatMap { m.efforts.contains($0) ? $0 : nil }), for: id)
            }
            item.toolTip = m.summary
            item.state = (current.model ?? running) == m.slug ? .on : .off
            if !m.efforts.isEmpty {
                let sub = NSMenu()
                sub.autoenablesItems = false
                for effort in m.efforts {
                    let title = effort.prefix(1).uppercased() + effort.dropFirst() + (effort == m.defaultEffort ? " (Default)" : "")
                    let row = ActionMenuItem(title) { [model] in model.setCodexPick(CodexPick(model: m.slug, effort: effort), for: id) }
                    row.state = (current.model ?? running) == m.slug && (current.effort ?? m.defaultEffort) == effort ? .on : .off
                    sub.addItem(row)
                }
                let pickModel = ActionMenuItem("Use \(m.name)") { [model] in
                    model.setCodexPick(CodexPick(model: m.slug, effort: nil), for: id)
                }
                sub.insertItem(pickModel, at: 0)
                sub.insertItem(.separator(), at: 1)
                item.submenu = sub
            }
            menu.addItem(item)
        }
        if models.isEmpty {
            let none = NSMenuItem(title: "Codex hasn't listed its models yet", action: nil, keyEquivalent: "")
            none.isEnabled = false
            menu.addItem(none)
        }
        menu.addItem(.separator())
        let reset = ActionMenuItem("Codex Default") { [model] in model.setCodexPick(CodexPick(), for: id) }
        reset.state = current == CodexPick() ? .on : .off
        menu.addItem(reset)
        let note = NSMenuItem(title: "Applies from the next turn.", action: nil, keyEquivalent: "")
        note.isEnabled = false
        menu.addItem(note)
        menu.popUp(positioning: nil, at: NSEvent.mouseLocation, in: nil)
    }
}

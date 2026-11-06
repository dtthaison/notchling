import AppKit
import SwiftUI

/// Which tool runs a session: Claude Code, Codex, Antigravity CLI or Gemini CLI.
enum SessionTool {
    case claude, codex, antigravity, gemini

    init(_ target: AgentTarget) {
        switch target {
        case .codex: self = .codex
        case .antigravity: self = .antigravity
        case .gemini: self = .gemini
        case .job, .process, .resume: self = .claude
        }
    }

    var name: String {
        switch self {
        case .claude: return "Claude Code"
        case .codex: return "Codex"
        case .antigravity: return "Antigravity CLI"
        case .gemini: return "Gemini CLI"
        }
    }

    /// The installed app whose icon stands for the tool (the CLIs have none of their own).
    fileprivate var bundleIDs: [String] {
        switch self {
        case .claude: return ["com.anthropic.claudefordesktop"]
        case .codex: return ["com.openai.codex"]
        case .antigravity: return ["com.google.antigravity", "com.google.antigravity-ide"]
        case .gemini: return []  // no app of its own, and no logo shipped here: the symbol stands in
        }
    }

    fileprivate var symbol: String {
        switch self {
        case .claude: return "sparkle"
        case .codex: return "chevron.left.forwardslash.chevron.right"
        case .antigravity: return "paperplane"
        case .gemini: return "rhombus"
        }
    }
}

/// The tool's app icon in front of a session's title, so it's clear whose session it is. Falls back to
/// a symbol when the app isn't installed.
struct ToolIcon: View {
    let tool: SessionTool
    var size: CGFloat = 14

    private static var cache: [String: NSImage] = [:]

    private static func image(for tool: SessionTool) -> NSImage? {
        for id in tool.bundleIDs {
            if let hit = cache[id] { return hit }
            guard let url = NSWorkspace.shared.urlForApplication(withBundleIdentifier: id) else { continue }
            let icon = NSWorkspace.shared.icon(forFile: url.path)
            cache[id] = icon
            return icon
        }
        return nil
    }

    var body: some View {
        Group {
            if let image = Self.image(for: tool) {
                Image(nsImage: image).resizable().interpolation(.high)
            } else {
                Image(systemName: tool.symbol).font(.system(size: size * 0.75, weight: .semibold))
                    .foregroundColor(KitInk.textMuted)
            }
        }
        .frame(width: size, height: size)
        .help(tool.name)
        .accessibilityLabel(tool.name)
    }
}

import Foundation

/// Codex's slash commands (CLI 0.166, descriptions as its own picker shows them). A Chat turn is
/// `codex exec`, which has no slash commands, so Chat does a few itself and runs the rest in Codex's
/// own interface: the terminal sheet opens `codex resume <id>` and types the command there.
enum CodexCommands {
    enum Route: Equatable {
        /// Chat's own model and effort picker.
        case modelPicker
        /// A new Codex session in the same folder.
        case newSession
        /// Delete this session, after asking.
        case delete
        /// Nothing to do: Chat isn't a Codex process to quit.
        case ignore
        /// Typed into `codex resume` in the terminal sheet.
        case terminal
    }

    static let all: [SlashCommand] = [
        ("model", "choose what model and reasoning effort to use"),
        ("review", "review my current changes and find issues"),
        ("new", "start a new chat during a conversation"),
        ("init", "create an AGENTS.md file with instructions for Codex"),
        ("compact", "summarize conversation to prevent hitting the context limit"),
        ("status", "show current session configuration and token usage"),
        ("diff", "show git diff (including untracked files)"),
        ("mention", "mention a file"),
        ("permissions", "choose what Codex is allowed to do"),
        ("plan", "switch to Plan mode"),
        ("goal", "set or view the goal for a long-running task"),
        ("side", "start a side conversation in an ephemeral fork"),
        ("fork", "fork the current chat"),
        ("resume", "resume a saved chat"),
        ("rename", "rename the current thread"),
        ("copy", "copy the last response or part of it"),
        ("export", "export the conversation as markdown"),
        ("usage", "view account usage or use a usage limit reset"),
        ("mcp", "list MCP tools; use /mcp verbose or /mcp login <name>"),
        ("apps", "manage apps"),
        ("plugins", "browse plugins"),
        ("skills", "use skills to improve how Codex performs specific tasks"),
        ("prompts", "run or save reusable prompts and slash commands"),
        ("hooks", "view and manage lifecycle hooks"),
        ("memories", "configure memory use and generation"),
        ("experimental", "toggle experimental features"),
        ("settings", "configure Codex preferences"),
        ("worktree", "start or continue a conversation in a new worktree"),
        ("app", "continue this session in the Desktop app"),
        ("import", "import setup, this project, and recent chats from Claude Code"),
        ("cd", "change the current working directory"),
        ("pwd", "show the current working directory"),
        ("ps", "list background terminals"),
        ("subagents", "switch between this session's subagents"),
        ("warnings", "view retained warnings and diagnostic details"),
        ("rollout", "print the rollout file path"),
        ("feedback", "send logs to maintainers"),
        ("archive", "archive this session"),
        ("delete", "permanently delete this session"),
        ("clear", "clear the terminal and start a new chat"),
        ("logout", "log out of Codex"),
        ("quit", "exit Codex"),
    ].map { SlashCommand(name: $0.0, description: $0.1, argumentHint: nil) }

    /// Codex's commands plus your saved prompts (`~/.codex/prompts/<name>.md`, run as /prompts:<name>).
    static func available() -> [SlashCommand] {
        let dir = CodexStore.home.appendingPathComponent("prompts")
        let files = ((try? FileManager.default.contentsOfDirectory(atPath: dir.path)) ?? []).filter { $0.hasSuffix(".md") }.sorted()
        let prompts = files.map { file -> SlashCommand in
            let name = String(file.dropLast(3))
            let text = (try? String(contentsOf: dir.appendingPathComponent(file), encoding: .utf8)) ?? ""
            return SlashCommand(name: "prompts:" + name, description: promptDescription(text) ?? "your saved prompt", argumentHint: nil)
        }
        return all + prompts
    }

    /// A prompt file's `description:` front matter, else its first line.
    static func promptDescription(_ text: String) -> String? {
        let lines = text.split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
        if lines.first == "---", let d = lines.dropFirst().prefix { $0 != "---" }.first(where: { $0.hasPrefix("description:") }) {
            return d.dropFirst("description:".count).trimmingCharacters(in: CharacterSet(charactersIn: " \"'"))
        }
        return lines.first { !$0.isEmpty && $0 != "---" }.map { String($0.prefix(80)) }
    }

    /// The command name in `text` ("/review focus on auth" is "review"), if it's one of Codex's.
    static func command(in text: String, among commands: [SlashCommand] = available()) -> String? {
        guard text.hasPrefix("/") else { return nil }
        let name = text.dropFirst().split(whereSeparator: \.isWhitespace).first.map(String.init) ?? ""
        return commands.contains { $0.name == name } ? name : nil
    }

    static func route(_ name: String) -> Route {
        switch name {
        case "model": return .modelPicker
        case "new", "clear": return .newSession
        case "delete": return .delete
        case "quit", "exit": return .ignore
        default: return .terminal
        }
    }

    /// What the terminal sheet runs: Codex's interface on this session, or on a new one in the folder
    /// when it hasn't started yet.
    static func script(session: String) -> String {
        session.isEmpty ? "codex" : "codex resume \(session)"
    }
}

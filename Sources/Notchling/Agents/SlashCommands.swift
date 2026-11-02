import Foundation

struct SlashCommand: Equatable {
    let name: String         // without the slash, e.g. "clear" or "sd:approve"
    let description: String
    let argumentHint: String?
}

/// The slash commands a session can run: Claude Code's built-ins, plus the user's, the project's and
/// enabled plugins' commands and skills. Cached per folder for a short while.
enum SlashCommands {
    private static let home = FileManager.default.homeDirectoryForCurrentUser
    private static var cache: [String: (at: Date, commands: [SlashCommand])] = [:]
    private static let lock = NSLock()

    private static let builtIns: [SlashCommand] = [
        ("clear", "Clear the conversation and start fresh"),
        ("compact", "Summarise the conversation to free up context"),
        ("context", "Show how much context is used"),
        ("cost", "Show the cost of this session"),
        ("usage", "Show plan usage limits"),
        ("model", "Change the model"),
        ("effort", "Change the effort level"),
        ("review", "Review a pull request"),
        ("security-review", "Security review of pending changes"),
        ("init", "Write a CLAUDE.md for this project"),
        ("memory", "Edit memory files"),
        ("rewind", "Go back to an earlier point"),
        ("rename", "Rename this session"),
        ("status", "Show version, model and account"),
        ("todos", "Show the current task list"),
        ("help", "Show help"),
    ].map { SlashCommand(name: $0.0, description: $0.1, argumentHint: nil) }

    static func all(for cwd: String) -> [SlashCommand] {
        lock.lock()
        if let hit = cache[cwd], Date().timeIntervalSince(hit.at) < 60 {
            lock.unlock()
            return hit.commands
        }
        lock.unlock()

        var found: [SlashCommand] = []
        let user = home.appendingPathComponent(".claude")
        found += scan(user, prefix: nil)
        if !cwd.isEmpty { found += scan(URL(fileURLWithPath: cwd).appendingPathComponent(".claude"), prefix: nil) }
        for (name, path) in enabledPlugins() { found += scan(URL(fileURLWithPath: path), prefix: name) }

        // Later sources don't override earlier ones with the same name; built-ins come first.
        var seen = Set<String>()
        let commands = (builtIns + found.sorted { $0.name < $1.name }).filter { seen.insert($0.name).inserted }
        lock.lock()
        cache[cwd] = (Date(), commands)
        lock.unlock()
        return commands
    }

    /// Commands matching what's typed after the slash: prefix matches first, then the rest.
    static func matches(_ typed: String, in commands: [SlashCommand], limit: Int = 6) -> [SlashCommand] {
        let q = typed.lowercased()
        let prefix = commands.filter { $0.name.lowercased().hasPrefix(q) || $0.name.lowercased().split(separator: ":").last?.hasPrefix(q) == true }
        let rest = q.isEmpty ? [] : commands.filter { $0.name.lowercased().contains(q) && !prefix.contains($0) }
        return Array((prefix + rest).prefix(limit))
    }

    // MARK: - Reading

    /// <dir>/commands/*.md and <dir>/skills/<name>/SKILL.md.
    private static func scan(_ dir: URL, prefix: String?) -> [SlashCommand] {
        let fm = FileManager.default
        var result: [SlashCommand] = []
        let commandsDir = dir.appendingPathComponent("commands")
        for file in (try? fm.contentsOfDirectory(atPath: commandsDir.path)) ?? [] where file.hasSuffix(".md") {
            let meta = frontMatter(commandsDir.appendingPathComponent(file))
            result.append(make(String(file.dropLast(3)), meta, prefix))
        }
        let skillsDir = dir.appendingPathComponent("skills")
        for sub in (try? fm.contentsOfDirectory(atPath: skillsDir.path)) ?? [] {
            let file = skillsDir.appendingPathComponent(sub).appendingPathComponent("SKILL.md")
            guard fm.fileExists(atPath: file.path) else { continue }
            let meta = frontMatter(file)
            result.append(make(meta["name"] ?? sub, meta, prefix))
        }
        return result
    }

    private static func make(_ name: String, _ meta: [String: String], _ prefix: String?) -> SlashCommand {
        var description = meta["description"] ?? ""
        if let end = description.firstIndex(of: "."), description.distance(from: description.startIndex, to: end) < 110 {
            description = String(description[...end])
        }
        return SlashCommand(name: prefix.map { "\($0):\(name)" } ?? name,
                            description: description,
                            argumentHint: meta["argument-hint"])
    }

    /// The simple `key: value` lines between the opening `---` lines.
    private static func frontMatter(_ url: URL) -> [String: String] {
        guard let handle = FileHandle(forReadingAtPath: url.path) else { return [:] }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: 4096), let text = String(data: data, encoding: .utf8),
              text.hasPrefix("---") else { return [:] }
        var meta: [String: String] = [:]
        for line in text.split(separator: "\n").dropFirst() {
            if line.hasPrefix("---") { break }
            guard let colon = line.firstIndex(of: ":") else { continue }
            let key = line[..<colon].trimmingCharacters(in: .whitespaces)
            var value = line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)
            if value.count >= 2, value.first == "\"" || value.first == "'", value.last == value.first {
                value = String(value.dropFirst().dropLast())
            }
            meta[key] = value
        }
        return meta
    }

    /// Plugin name → install path, for plugins turned on in settings.
    private static func enabledPlugins() -> [(String, String)] {
        let claude = home.appendingPathComponent(".claude")
        guard let settings = HookBridge.readJSON(claude.appendingPathComponent("settings.json")),
              let enabled = settings["enabledPlugins"] as? [String: Bool],
              let installed = HookBridge.readJSON(claude.appendingPathComponent("plugins/installed_plugins.json"))?["plugins"]
                as? [String: [[String: Any]]] else { return [] }
        return enabled.compactMap { key, on in
            guard on, let path = installed[key]?.first?["installPath"] as? String else { return nil }
            return (String(key.split(separator: "@").first ?? Substring(key)), path)
        }
    }
}

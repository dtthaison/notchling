import Foundation

/// Gemini CLI sessions for Chat's sidebar. Gemini keeps one file per session in
/// `~/.gemini/tmp/<project slug>/chats/session-<time>-<first 8 of id>.jsonl` (older versions: `.json`),
/// and `~/.gemini/projects.json` maps each project folder to its slug. ACP has no `session/list`, so the
/// files are read: the first record carries the session id, later ones the messages (`type` "user" or
/// "gemini", `content` a string or a list of `{text}` parts) and a `$set` record may carry a `summary`.
enum GeminiSessions {
    static let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".gemini")
    static let recent: TimeInterval = 14 * 86400
    /// Only the start of a file is read: the id, summary and first question are all near the top.
    private static let headBytes = 512 * 1024
    private static let legacyLimit = 20 * 1024 * 1024

    static func agentID(_ sessionID: String) -> String { ACPTool.gemini.agentID(sessionID) }

    struct Row: Equatable {
        let id: String
        let title: String
        let folder: String
        let modified: Date
    }

    /// What a session file says about itself.
    struct Summary: Equatable {
        var id: String
        var summary: String?
        var firstUser: String?
    }

    static func list(now: Date = Date(), live: Set<String> = []) -> [Agent] {
        rows(now: now).map { row in
            let working = live.contains(row.id)
            return Agent(id: agentID(row.id), name: row.title, project: (row.folder as NSString).lastPathComponent,
                         cwd: row.folder, group: working ? .working : .completed, label: working ? "Working" : "Gemini",
                         detail: nil, since: row.modified, target: .gemini(sessionID: row.id), host: "Gemini")
        }
    }

    static func rows(now: Date = Date(), root: URL = GeminiSessions.root) -> [Row] {
        let fm = FileManager.default
        let projects = projectFolders(root.appendingPathComponent("projects.json"))
        var best: [String: Row] = [:]
        for (folder, slug) in projects {
            let chats = root.appendingPathComponent("tmp/\(slug)/chats")
            guard let names = try? fm.contentsOfDirectory(atPath: chats.path) else { continue }
            for name in names where name.hasPrefix("session-") && (name.hasSuffix(".jsonl") || name.hasSuffix(".json")) {
                let url = chats.appendingPathComponent(name)
                guard let modified = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate,
                      now.timeIntervalSince(modified) < recent,
                      let summary = read(url),
                      let text = summary.summary?.nilIfBlank ?? summary.firstUser?.nilIfBlank else { continue }
                let row = Row(id: summary.id, title: title(text), folder: folder, modified: modified)
                if let old = best[row.id], old.modified >= row.modified { continue }
                best[row.id] = row
            }
        }
        return best.values.sorted { $0.modified > $1.modified }.prefix(200).map { $0 }
    }

    /// `{"projects": {"/path/to/folder": "slug"}}`.
    static func projectFolders(_ file: URL) -> [String: String] {
        guard let data = try? Data(contentsOf: file),
              let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let projects = json["projects"] as? [String: String] else { return [:] }
        return projects
    }

    /// The id, summary and first user message of a session file; nil when it holds no conversation yet.
    static func read(_ url: URL) -> Summary? {
        if url.pathExtension == "json" {
            guard let size = (try? url.resourceValues(forKeys: [.fileSizeKey]))?.fileSize, size < legacyLimit,
                  let data = try? Data(contentsOf: url) else { return nil }
            return summary(legacy: data)
        }
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        guard let data = try? handle.read(upToCount: headBytes) else { return nil }
        var lines = String(decoding: data, as: UTF8.self).components(separatedBy: "\n")
        // A cut-off last line is not JSON; drop it when the file went on past what was read.
        if data.count >= headBytes { lines.removeLast() }
        return summary(lines: lines)
    }

    /// From the lines of a `.jsonl` session file.
    static func summary(lines: [String]) -> Summary? {
        var result = Summary(id: "")
        for line in lines {
            guard let record = (try? JSONSerialization.jsonObject(with: Data(line.utf8))) as? [String: Any] else { continue }
            if let set = record["$set"] as? [String: Any] {
                if let s = set["summary"] as? String, !s.isEmpty { result.summary = s }
            } else if let id = record["sessionId"] as? String, record["type"] == nil {
                result.id = id
                if let s = record["summary"] as? String, !s.isEmpty { result.summary = s }
                if (record["kind"] as? String) == "subagent" { return nil }
            } else if result.firstUser == nil, (record["type"] as? String) == "user" {
                let text = messageText(record["content"])
                if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { result.firstUser = text }
            }
        }
        return result.id.isEmpty || (result.firstUser == nil && result.summary == nil) ? nil : result
    }

    /// From a legacy `.json` session file: one object with `messages`.
    static func summary(legacy data: Data) -> Summary? {
        guard let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let id = json["sessionId"] as? String else { return nil }
        if (json["kind"] as? String) == "subagent" { return nil }
        var result = Summary(id: id)
        if let s = json["summary"] as? String, !s.isEmpty { result.summary = s }
        for message in json["messages"] as? [[String: Any]] ?? [] where (message["type"] as? String) == "user" {
            let text = messageText(message["content"])
            if !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { result.firstUser = text; break }
        }
        return result.firstUser == nil && result.summary == nil ? nil : result
    }

    /// A message's text: a string, or a list of parts where `{"text": …}` ones count.
    static func messageText(_ content: Any?) -> String {
        if let s = content as? String { return s }
        if let parts = content as? [[String: Any]] { return parts.compactMap { $0["text"] as? String }.joined() }
        if let part = content as? [String: Any], let s = part["text"] as? String { return s }
        return ""
    }

    /// A one-line title of at most 80 characters.
    static func title(_ text: String) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map { $0.trimmingCharacters(in: .whitespaces) } ?? text
        return String(line.prefix(80))
    }
}

private extension String {
    var nilIfBlank: String? { trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? nil : self }
}

/// What `gemini` is signed in as, as far as its files say. Notchling doesn't switch Gemini accounts: the
/// CLI keeps one login (`gemini` in a terminal changes it).
enum GeminiLogin {
    struct State: Equatable {
        /// The Google account, from `google_accounts.json`.
        var email: String?
        /// `security.auth.selectedType` in settings.json (oauth-personal, gemini-api-key, vertex-ai…).
        var method: String?
        /// Whether any sign-in is on record: an account, a saved login file or an API key in the environment.
        var signedIn: Bool
    }

    static func read(home: URL = FileManager.default.homeDirectoryForCurrentUser,
                     environment: [String: String] = ProcessInfo.processInfo.environment) -> State {
        let dir = home.appendingPathComponent(".gemini")
        func json(_ name: String) -> [String: Any]? {
            guard let data = try? Data(contentsOf: dir.appendingPathComponent(name)) else { return nil }
            return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        }
        let email = (json("google_accounts.json")?["active"] as? String).flatMap { $0.isEmpty ? nil : $0 }
        let method = ((json("settings.json")?["security"] as? [String: Any])?["auth"] as? [String: Any])?["selectedType"] as? String
        let hasFile = FileManager.default.fileExists(atPath: dir.appendingPathComponent("oauth_creds.json").path)
        let hasKey = ["GEMINI_API_KEY", "GOOGLE_API_KEY", "GOOGLE_GENAI_USE_VERTEXAI", "GOOGLE_GENAI_USE_GCA"]
            .contains { !(environment[$0] ?? "").isEmpty }
        return State(email: email, method: method, signedIn: email != nil || hasFile || hasKey || method != nil)
    }
}

/// Gemini CLI over `gemini --acp`.
final class GeminiRunner: ACPRunner {
    static let shared = GeminiRunner()

    private init() { super.init(tool: .gemini) }

    /// The `gemini` binary: Homebrew's, else the usual npm and installer folders.
    static func binary() -> String? {
        let home = NSHomeDirectory()
        let candidates = ["/opt/homebrew/bin/gemini", "/usr/local/bin/gemini", home + "/.local/bin/gemini",
                          home + "/.npm-global/bin/gemini", home + "/.volta/bin/gemini", home + "/.bun/bin/gemini"]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }
}

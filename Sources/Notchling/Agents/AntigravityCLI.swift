import Foundation
import SQLite3

/// Antigravity CLI conversations for Chat's sidebar, from `conversation_summaries.db` in
/// ~/.gemini/antigravity-cli. The database is in WAL mode and a read-only open fails while no WAL file
/// exists, so it's read from a copy (database and WAL) and Antigravity's own file is never opened.
enum AntigravitySessions {
    static let root = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".gemini/antigravity-cli")
    static let recent: TimeInterval = 14 * 86400

    static func agentID(_ sessionID: String) -> String { "agy:" + sessionID }

    struct Row: Equatable {
        let id: String
        let title: String
        let folder: String
        let modified: Date
        let running: Bool
    }

    static func list(now: Date = Date(), live: Set<String> = []) -> [Agent] {
        rows(now: now).map { row in
            let working = live.contains(row.id)
            return Agent(id: agentID(row.id), name: row.title, project: (row.folder as NSString).lastPathComponent,
                         cwd: row.folder, group: working ? .working : .completed, label: working ? "Working" : "Antigravity",
                         detail: nil, since: row.modified, target: .antigravity(sessionID: row.id), host: "Antigravity")
        }
    }

    private static let copyLock = NSLock()

    static func rows(now: Date = Date()) -> [Row] {
        let source = root.appendingPathComponent("conversation_summaries.db")
        guard FileManager.default.fileExists(atPath: source.path) else { return [] }
        copyLock.lock(); defer { copyLock.unlock() }
        let dir = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Application Support/Notchling/antigravity")
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let copy = dir.appendingPathComponent("summaries.db")
        for suffix in ["", "-wal", "-shm"] {
            let to = URL(fileURLWithPath: copy.path + suffix)
            try? FileManager.default.removeItem(at: to)
            let from = URL(fileURLWithPath: source.path + suffix)
            if suffix != "-shm", FileManager.default.fileExists(atPath: from.path) {
                try? FileManager.default.copyItem(at: from, to: to)
            }
        }
        var db: OpaquePointer?
        guard sqlite3_open_v2(copy.path, &db, SQLITE_OPEN_READWRITE, nil) == SQLITE_OK, let db else { return [] }
        defer { sqlite3_close(db) }
        let sql = """
            SELECT conversation_id, title, preview, workspace_uris, status, last_modified_time
            FROM conversation_summaries WHERE nesting_depth = 0 ORDER BY last_modified_time DESC LIMIT 200
            """
        var statement: OpaquePointer?
        guard sqlite3_prepare_v2(db, sql, -1, &statement, nil) == SQLITE_OK, let statement else { return [] }
        defer { sqlite3_finalize(statement) }
        func column(_ i: Int32) -> String {
            sqlite3_column_text(statement, i).map { String(cString: $0) } ?? ""
        }
        var out: [Row] = []
        while sqlite3_step(statement) == SQLITE_ROW {
            guard let modified = date(column(5)), now.timeIntervalSince(modified) < recent else { continue }
            let title = [column(1), column(2)].first { !$0.isEmpty } ?? "Antigravity Session"
            out.append(Row(id: column(0), title: String(title.prefix(80)), folder: folder(column(3)),
                           modified: modified, running: column(4).contains("RUNNING")))
        }
        return out
    }

    /// `2026-11-05 16:57:03.335872+00:00` (UTC).
    static func date(_ text: String) -> Date? {
        let f = DateFormatter()
        f.locale = Locale(identifier: "en_US_POSIX")
        f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return f.date(from: String(text.prefix(19)))
    }

    /// The first of `["file:///path", …]`.
    static func folder(_ uris: String) -> String {
        guard let list = (try? JSONSerialization.jsonObject(with: Data(uris.utf8))) as? [String],
              let first = list.first, let url = URL(string: first), url.isFileURL else { return "" }
        return url.path
    }

    /// A title from the first message, as Codex's rows do, until Antigravity names it.
    static func title(_ text: String) -> String {
        let line = text.split(whereSeparator: \.isNewline).first.map(String.init) ?? text
        return line.count > 60 ? String(line.prefix(57)) + "…" : line
    }
}

/// Antigravity CLI over `agy acp`.
final class AntigravityRunner: ACPRunner {
    static let shared = AntigravityRunner()

    private init() { super.init(tool: .antigravity) }

    /// The `agy` binary: the installer's ~/.local/bin, else Homebrew's.
    static func binary() -> String? {
        let candidates = [NSHomeDirectory() + "/.local/bin/agy", "/opt/homebrew/bin/agy", "/usr/local/bin/agy"]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }
}

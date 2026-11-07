import Foundation

/// A line of `<git common dir>/sd/runs.jsonl` that sd appends when it starts a long run (sd >= 1.21.1).
struct RunStart: Equatable {
    var key: String
    var log: String
    var cwd: String
    var pid: Int32?
    /// The Claude session that started it.
    var session: String?
    /// Seconds the run may take; nil when sd set no limit.
    var limit: TimeInterval?
    var startedAt: Date
}

/// The line sd appends when a run ends. It has no `event` key and is matched to its start by `log`.
struct RunFinish: Equatable {
    var key: String
    var seconds: TimeInterval
    var rc: Int
    var log: String
    var at: Date
}

enum RunLine: Equatable {
    case start(RunStart)
    case finish(RunFinish)

    /// nil for anything that isn't a record we understand: bad JSON, an unknown `event`, a missing log or time.
    /// Unknown keys are ignored.
    static func parse(_ line: String) -> RunLine? {
        guard let data = line.data(using: .utf8),
              let object = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else { return nil }
        guard let log = object["log"] as? String, !log.isEmpty else { return nil }
        let key = object["key"] as? String ?? object["cmd"] as? String ?? ""
        switch object["event"] {
        case nil:
            guard let rc = int(object["rc"]), let at = (object["at"] as? String).flatMap(date) else { return nil }
            return .finish(RunFinish(key: key, seconds: double(object["seconds"]) ?? 0, rc: rc, log: log, at: at))
        case let event as String where event == "start":
            guard let started = (object["started_at"] as? String).flatMap(date) else { return nil }
            let limit = double(object["limit"]).flatMap { $0 > 0 ? $0 : nil }
            let session = (object["session"] as? String).flatMap { $0.isEmpty ? nil : $0 }
            return .start(RunStart(key: key, log: log, cwd: object["cwd"] as? String ?? "", pid: int(object["pid"]).map(Int32.init(truncatingIfNeeded:)),
                                   session: session, limit: limit, startedAt: started))
        default:
            return nil
        }
    }

    private static func int(_ value: Any?) -> Int? {
        if let n = value as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() { return n.intValue }
        return (value as? String).flatMap { Int($0) }
    }

    private static func double(_ value: Any?) -> Double? {
        if let n = value as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() { return n.doubleValue }
        return (value as? String).flatMap { Double($0) }
    }

    private static let plain: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime]
        return f
    }()
    private static let fractional: ISO8601DateFormatter = {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return f
    }()

    private static func date(_ text: String) -> Date? { plain.date(from: text) ?? fractional.date(from: text) }
}

/// Where a repository keeps its shared git data (and so sd's `sd/` folder), found by reading `.git`
/// instead of asking git: a worktree's `.git` is a file that points into `<common>/worktrees/<name>`.
enum GitCommonDir {
    /// Walks up from `cwd` to the first `.git`. `stopAt` is for tests.
    static func resolve(cwd: String, stopAt: String = "/") -> String? {
        guard cwd.hasPrefix("/") else { return nil }
        var folder = URL(fileURLWithPath: cwd).standardizedFileURL
        let fm = FileManager.default
        while true {
            let dotGit = folder.appendingPathComponent(".git")
            var isDirectory: ObjCBool = false
            if fm.fileExists(atPath: dotGit.path, isDirectory: &isDirectory) {
                return isDirectory.boolValue ? dotGit.path : commonDir(fromFile: dotGit)
            }
            if folder.path == stopAt || folder.path == "/" { return nil }
            folder = folder.deletingLastPathComponent()
        }
    }

    /// `gitdir: <path>` (relative to the file's folder when not absolute), then that folder's `commondir`
    /// (relative to it), which is absent for a submodule: its gitdir is the answer then.
    private static func commonDir(fromFile file: URL) -> String? {
        guard let text = try? String(contentsOf: file, encoding: .utf8),
              let line = text.split(separator: "\n").first(where: { $0.hasPrefix("gitdir:") }) else { return nil }
        let raw = line.dropFirst("gitdir:".count).trimmingCharacters(in: .whitespaces)
        guard !raw.isEmpty else { return nil }
        let gitdir = URL(fileURLWithPath: raw, relativeTo: file.deletingLastPathComponent()).standardizedFileURL
        let pointer = gitdir.appendingPathComponent("commondir")
        guard let common = (try? String(contentsOf: pointer, encoding: .utf8))?.trimmingCharacters(in: .whitespacesAndNewlines),
              !common.isEmpty else {
            // git always writes `commondir` for a worktree; this is for a hand-made one
            let parent = gitdir.deletingLastPathComponent()
            return parent.lastPathComponent == "worktrees" ? parent.deletingLastPathComponent().path : gitdir.path
        }
        return URL(fileURLWithPath: common, relativeTo: gitdir).standardizedFileURL.path
    }
}

/// Words for the notch rows.
enum RunFormat {
    static let nameLimit = 28

    /// "45s", "32m", "2h", "1h 5m".
    static func duration(_ seconds: TimeInterval) -> String {
        let s = max(0, Int(seconds))
        if s < 60 { return "\(s)s" }
        if s < 3600 { return "\(s / 60)m" }
        let (h, m) = (s / 3600, (s % 3600) / 60)
        return m == 0 ? "\(h)h" : "\(h)h \(m)m"
    }

    /// "32m / 2h", or just "32m" when sd set no limit.
    static func progress(elapsed: TimeInterval, limit: TimeInterval?) -> String {
        guard let limit else { return duration(elapsed) }
        return "\(duration(elapsed)) / \(duration(limit))"
    }

    /// 124 is `timeout`'s code for a run stopped at its limit.
    static func exit(_ rc: Int) -> String {
        switch rc {
        case 0: return "ok"
        case 124: return "stopped at limit"
        default: return "exit \(rc)"
        }
    }

    /// The recipe or command out of sd's key, which holds the whole command line:
    /// `caffeinate -i mise exec -- just --justfile f --set a b perf suite=x` is "perf".
    static func shortName(_ key: String) -> String {
        var words = key.split(whereSeparator: \.isWhitespace).map(String.init)
        // wrappers in front of the command
        var changed = true
        while changed, let first = words.first {
            changed = true
            switch first {
            case "caffeinate", "nice", "time", "env", "nohup":
                words.removeFirst()
                while let f = words.first, f.hasPrefix("-") { words.removeFirst() }
            case "mise":
                words.removeFirst()
                if words.first == "exec" { words.removeFirst() }
                if words.first == "--" { words.removeFirst() }
            default:
                changed = false
            }
        }
        guard let command = words.first else { return "run" }
        let name: String
        if command == "just" {
            name = justRecipe(Array(words.dropFirst())) ?? "just"
        } else {
            let base = (command as NSString).lastPathComponent
            let next = words.dropFirst().first
            name = (next.map { isPlainWord($0) } ?? false) && !command.contains("/") && !command.hasSuffix(".sh")
                ? "\(base) \(next!)" : base
        }
        return truncated(name, to: nameLimit)
    }

    /// The first word of `just`'s arguments that isn't an option or an option's value.
    private static func justRecipe(_ args: [String]) -> String? {
        var i = 0
        while i < args.count {
            let word = args[i]
            switch word {
            case "--set": i += 3
            case "--justfile", "-f", "--working-directory", "-d": i += 2
            default:
                if word.hasPrefix("-") { i += 1 } else if isPlainWord(word) { return word } else { i += 1 }
            }
        }
        return nil
    }

    /// A recipe or subcommand: letters, digits, dashes. Flags, `a=b`, paths and commas are arguments.
    private static func isPlainWord(_ word: String) -> Bool {
        guard let first = word.first, first.isLetter else { return false }
        return word.allSatisfy { $0.isLetter || $0.isNumber || $0 == "-" || $0 == "_" }
    }

    static func truncated(_ text: String, to limit: Int) -> String {
        text.count <= limit ? text : String(text.prefix(limit - 1)) + "…"
    }
}

/// The last line of a run's log.
enum RunLog {
    static let lineLimit = 80
    private static let tailBytes: UInt64 = 8192

    static func lastLine(of path: String) -> String? {
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        guard let size = try? handle.seekToEnd() else { return nil }
        try? handle.seek(toOffset: size > tailBytes ? size - tailBytes : 0)
        guard let data = try? handle.readToEnd() else { return nil }
        return lastLine(in: data)
    }

    /// Escape codes dropped; a `\r` progress line shows its final state; at most 80 characters.
    static func lastLine(in data: Data) -> String? {
        let text = String(decoding: data, as: UTF8.self).replacingOccurrences(of: "\r\n", with: "\n")
        let clean = text.replacingOccurrences(of: "\u{1B}\\[[0-9;?]*[ -/]*[@-~]", with: "", options: .regularExpression)
        for raw in clean.split(separator: "\n", omittingEmptySubsequences: true).reversed() {
            let pieces = raw.split(separator: "\r", omittingEmptySubsequences: true)
            guard let last = pieces.last else { continue }
            let line = last.trimmingCharacters(in: .whitespaces)
            if !line.isEmpty { return RunFormat.truncated(line, to: lineLimit) }
        }
        return nil
    }
}

/// Is the process in a start record still that run?
enum RunProcess {
    /// A run's record is written right after its process starts; a process that began later than this is a pid reused.
    static let identityTolerance: TimeInterval = 2

    /// When the process began, from the kernel (`proc_pidinfo`, which answers for other users' processes too);
    /// nil when there is no such process or it has ended and waits to be reaped.
    static func startTime(of pid: Int32) -> Date? {
        guard pid > 0 else { return nil }
        var info = proc_bsdinfo()
        let size = Int32(MemoryLayout<proc_bsdinfo>.size)
        guard proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, &info, size) == size, info.pbi_status != UInt32(SZOMB) else { return nil }
        return Date(timeIntervalSince1970: TimeInterval(info.pbi_start_tvsec) + TimeInterval(info.pbi_start_tvusec) / 1_000_000)
    }

    /// Alive, and began no later than `startedAt` plus the tolerance. Not `kill(pid, 0)`: a pid sd's run left
    /// behind may now belong to anything, and EPERM only says that it does.
    static func isAlive(pid: Int32, startedAt: Date) -> Bool {
        guard let began = startTime(of: pid) else { return false }
        return began <= startedAt.addingTimeInterval(identityTolerance)
    }
}

import Foundation

/// A background job that runs a shell command (`claude --bg` with template "exec") rather than a
/// conversation. Claude Code keeps no transcript for it, so Chat shows the command and, when the
/// command sends its output to a file (`> run.log 2>&1`), the end of that file.
enum CommandJob {
    /// The command without its long `env PATH=… HOME=…` prefix, which is the same for every job.
    static func readable(_ command: String) -> String {
        command.replacingOccurrences(of: #"env( [A-Z_][A-Z0-9_]*=('[^']*'|"[^"]*"|\S*))+ "#, with: "env … ",
                                     options: .regularExpression)
    }

    /// The file the command's output goes to: the target of its last `>` or `>>` (not `2>`, not `>&`).
    static func outputPath(in command: String, cwd: String) -> String? {
        let pattern = #"(?:^|[\s;&|])(?:1)?>>?\s*('[^']+'|"[^"]+"|[^\s;&|<>]+)"#
        guard let regex = try? NSRegularExpression(pattern: pattern) else { return nil }
        let ns = command as NSString
        guard let match = regex.matches(in: command, range: NSRange(location: 0, length: ns.length)).last else { return nil }
        var path = ns.substring(with: match.range(at: 1))
        if path.hasPrefix("&") { return nil }
        if let first = path.first, first == "'" || first == "\"" { path = String(path.dropFirst().dropLast()) }
        if path == "/dev/null" { return nil }
        if path.hasPrefix("~/") { path = FileManager.default.homeDirectoryForCurrentUser.path + path.dropFirst(1) }
        return path.hasPrefix("/") ? path : (cwd as NSString).appendingPathComponent(path)
    }

    /// The last `maxLines` lines of the file, read from its last 64 KB.
    static func tail(_ path: String, maxLines: Int = 200) -> String? {
        guard let handle = FileHandle(forReadingAtPath: path) else { return nil }
        defer { try? handle.close() }
        let size = (try? handle.seekToEnd()) ?? 0
        let start = size > 65_536 ? size - 65_536 : 0
        try? handle.seek(toOffset: start)
        guard let data = try? handle.readToEnd() else { return nil }
        var lines = String(decoding: data, as: UTF8.self).components(separatedBy: "\n")
        if start > 0 { lines.removeFirst() }   // a partial line
        while lines.last?.isEmpty == true { lines.removeLast() }
        return lines.suffix(maxLines).joined(separator: "\n")
    }
}

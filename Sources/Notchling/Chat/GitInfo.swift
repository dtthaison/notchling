import Foundation

/// The working copy a session runs in, for the inspector: branch, how far it is from its upstream,
/// the last commit, and what's changed and not committed yet.
struct GitInfo: Equatable {
    struct Change: Equatable, Identifiable {
        /// "M" modified, "A" added, "D" deleted, "R" renamed, "?" untracked, "U" conflicted.
        let status: String
        let path: String
        var added: Int?
        var removed: Int?
        var id: String { path }
    }

    var branch: String?
    var upstream: String?
    var ahead = 0
    var behind = 0
    var lastCommit: String?
    var lastCommitAge: String?
    var changes: [Change] = []

    /// `git status --porcelain=v1 --branch` output.
    static func parseStatus(_ text: String) -> GitInfo {
        var info = GitInfo()
        for line in text.split(separator: "\n", omittingEmptySubsequences: true).map(String.init) {
            if line.hasPrefix("## ") {
                parseBranch(String(line.dropFirst(3)), into: &info)
                continue
            }
            guard line.count > 3 else { continue }
            let xy = String(line.prefix(2))
            var path = String(line.dropFirst(3))
            if let arrow = path.range(of: " -> ") { path = String(path[arrow.upperBound...]) }
            if path.hasPrefix("\""), path.hasSuffix("\""), path.count > 1 { path = String(path.dropFirst().dropLast()) }
            info.changes.append(Change(status: status(xy), path: path))
        }
        return info
    }

    /// "main...origin/main [ahead 2, behind 1]", "HEAD (no branch)", "No commits yet on main".
    private static func parseBranch(_ text: String, into info: inout GitInfo) {
        if text.hasPrefix("No commits yet on ") { info.branch = String(text.dropFirst("No commits yet on ".count)); return }
        if text.hasPrefix("HEAD (no branch)") { info.branch = "detached HEAD"; return }
        var rest = text
        var counts = ""
        if let open = rest.range(of: " [") {
            counts = String(rest[open.upperBound...]).trimmingCharacters(in: CharacterSet(charactersIn: "]"))
            rest = String(rest[..<open.lowerBound])
        }
        let parts = rest.components(separatedBy: "...")
        info.branch = parts.first
        info.upstream = parts.count > 1 ? parts[1] : nil
        for part in counts.components(separatedBy: ", ") {
            let words = part.split(separator: " ")
            guard words.count == 2, let n = Int(words[1]) else { continue }
            if words[0] == "ahead" { info.ahead = n } else if words[0] == "behind" { info.behind = n }
        }
    }

    private static func status(_ xy: String) -> String {
        if xy == "??" { return "?" }
        let x = xy.first ?? " ", y = xy.last ?? " "
        if x == "U" || y == "U" || xy == "AA" || xy == "DD" { return "U" }
        for c in [x, y] where c == "R" || c == "C" { return "R" }
        for c in [x, y] where c == "D" { return "D" }
        for c in [x, y] where c == "A" { return "A" }
        return "M"
    }

    /// `git diff --numstat HEAD` output: added and removed lines per path ("-" for binary files).
    mutating func applyNumstat(_ text: String) {
        var counts: [String: (Int?, Int?)] = [:]
        for line in text.split(separator: "\n") {
            let cols = line.split(separator: "\t", maxSplits: 2).map(String.init)
            guard cols.count == 3 else { continue }
            var path = cols[2]
            // A rename reads "dir/{old => new}/file" or "old => new".
            if let arrow = path.range(of: " => ") {
                if let open = path.range(of: "{"), let close = path.range(of: "}") {
                    path = path.replacingCharacters(in: open.lowerBound..<close.upperBound,
                                                    with: String(path[arrow.upperBound..<close.lowerBound]))
                } else {
                    path = String(path[arrow.upperBound...])
                }
            }
            counts[path] = (Int(cols[0]), Int(cols[1]))
        }
        for i in changes.indices {
            if let (a, r) = counts[changes[i].path] { changes[i].added = a; changes[i].removed = r }
        }
    }

    /// Reads the folder's working copy; nil when it isn't one (or git isn't there).
    static func read(_ folder: String) -> GitInfo? {
        guard !folder.isEmpty, let status = git(["status", "--porcelain=v1", "--branch", "--untracked-files=all"], in: folder) else { return nil }
        var info = parseStatus(status)
        if !info.changes.isEmpty, let numstat = git(["diff", "--numstat", "HEAD"], in: folder) { info.applyNumstat(numstat) }
        if let log = git(["log", "-1", "--format=%h %s%x09%cr"], in: folder) {
            let cols = log.trimmingCharacters(in: .whitespacesAndNewlines).components(separatedBy: "\t")
            info.lastCommit = cols.first.flatMap { $0.isEmpty ? nil : $0 }
            info.lastCommitAge = cols.count > 1 ? cols[1] : nil
        }
        return info
    }

    private static func git(_ arguments: [String], in folder: String) -> String? {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/git")
        // No lock taken on the index: the session's own git commands must never wait on us.
        p.arguments = ["-c", "core.quotepath=off", "--no-optional-locks", "-C", folder] + arguments
        let out = Pipe()
        p.standardOutput = out
        p.standardError = FileHandle.nullDevice
        p.standardInput = FileHandle.nullDevice
        do { try p.run() } catch { return nil }
        let data = out.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        guard p.terminationStatus == 0 else { return nil }
        return String(decoding: data, as: UTF8.self)
    }
}

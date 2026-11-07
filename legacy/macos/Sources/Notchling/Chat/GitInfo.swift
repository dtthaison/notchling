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

    /// A local branch, or a remote branch that has no local copy.
    struct Branch: Equatable, Identifiable {
        let name: String
        var isRemote = false
        var isCurrent = false
        var ahead = 0
        var behind = 0
        /// The branch's upstream is gone from the remote.
        var gone = false
        var hasUpstream = false
        var age = ""
        /// Folder of another working copy that has this branch checked out.
        var worktree: String?
        var id: String { (isRemote ? "r/" : "l/") + name }
    }

    var branch: String?
    var upstream: String?
    var ahead = 0
    var behind = 0
    var lastCommit: String?
    var lastCommitAge: String?
    var changes: [Change] = []
    var branches: [Branch] = []

    /// `git for-each-ref` output in the format `branchFormat` names, newest commit first. Local
    /// branches all show; a remote branch shows only when no local branch has its name.
    static func parseBranches(_ text: String, folder: String = "") -> [Branch] {
        var local: [Branch] = [], remote: [Branch] = []
        for line in text.split(separator: "\n") {
            let c = line.split(separator: "\t", omittingEmptySubsequences: false).map(String.init)
            guard c.count >= 6 else { continue }
            let ref = c[1]
            if ref.hasPrefix("refs/heads/") {
                var b = Branch(name: String(ref.dropFirst("refs/heads/".count)), isCurrent: c[0] == "*", age: c[4])
                b.hasUpstream = !c[2].isEmpty
                b.gone = c[3] == "[gone]"
                for part in c[3].trimmingCharacters(in: CharacterSet(charactersIn: "[]")).components(separatedBy: ", ") {
                    let w = part.split(separator: " ")
                    guard w.count == 2, let n = Int(w[1]) else { continue }
                    if w[0] == "ahead" { b.ahead = n } else if w[0] == "behind" { b.behind = n }
                }
                let wt = c[5]
                if !wt.isEmpty, !b.isCurrent, wt != folder { b.worktree = wt }
                local.append(b)
            } else if ref.hasPrefix("refs/remotes/") {
                let short = String(ref.dropFirst("refs/remotes/".count))
                guard !short.hasSuffix("/HEAD"), short.contains("/") else { continue }
                remote.append(Branch(name: short, isRemote: true, age: c[4]))
            }
        }
        let names = Set(local.map(\.name))
        remote.removeAll { names.contains(String($0.name.drop { $0 != "/" }.dropFirst())) }
        return local + remote
    }

    static let branchFormat = "%(HEAD)%09%(refname)%09%(upstream:short)%09%(upstream:track)%09%(committerdate:relative)%09%(worktreepath)"

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
        if let refs = git(["for-each-ref", "--sort=-committerdate", "--format=" + branchFormat, "refs/heads", "refs/remotes"], in: folder) {
            info.branches = parseBranches(refs, folder: (folder as NSString).standardizingPath)
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

import Foundation

/// Where a merge into `main` happens on this Mac.
enum MergeTarget: Equatable {
    /// A worktree that already has `main` checked out.
    case existing(path: String)
    /// Notchling's own worktree for the repo, created with `git worktree add <path> main` the first time.
    case own(path: String)
}

/// What a merge got through, step by step.
struct MergeOutcome: Equatable {
    var fetched = false
    var merged = false
    var checked = false
    var pushed = false
    var closed = false
    var commit: String?
    var error: String?
    var conflictFiles: [String] = []
}

/// Merges a machine's ready branch into `main` from the MacBook.
///
/// The one thing it must never do is merge or break `main` without you: it works only in a `main` worktree
/// that is clean, pushes nothing but `main` to `origin` and only after the repo's check passed, and never
/// forces anything.
enum MergePlan {
    /// A fetch can take a while over ssh.
    private static let fetchTimeout: TimeInterval = 300
    /// Every other git call.
    private static let gitTimeout: TimeInterval = 60
    /// The repo's check (build, tests).
    private static let checkTimeout: TimeInterval = 1800
    private static let gitPath = "/usr/bin/git"
    private static let shellPath = "/bin/sh"

    /// `ProcessRunner` needs a full path, and a menu bar app has no Homebrew in its PATH.
    private static var ghPath: String {
        ["/opt/homebrew/bin/gh", "/usr/local/bin/gh", "/usr/bin/gh"]
            .first { FileManager.default.isExecutableFile(atPath: $0) } ?? "/opt/homebrew/bin/gh"
    }

    // MARK: Target

    /// The worktree that has `main` checked out, from `git worktree list --porcelain`; with none, Notchling's own
    /// folder for the repo (written with `~`, expanded when it is used).
    static func target(repo: MachineRepo, worktreeList: String) -> MergeTarget {
        for block in worktreeList.components(separatedBy: "\n\n") {
            var path: String?
            var onMain = false
            var skip = false
            for line in block.split(whereSeparator: \.isNewline).map(String.init) {
                if line.hasPrefix("worktree ") { path = String(line.dropFirst("worktree ".count)) }
                else if line == "branch refs/heads/main" { onMain = true }
                else if line == "bare" || line == "prunable" || line.hasPrefix("prunable ") { skip = true }
            }
            if onMain, !skip, let path, !path.isEmpty { return .existing(path: path) }
        }
        let slug = repo.github.replacingOccurrences(of: "/", with: "-")
        return .own(path: "~/Library/Application Support/Notchling/merge/\(slug)")
    }

    // MARK: Merge

    static func merge(_ e: ReadyEntry, machine: Machine, repo: MachineRepo, runner: CommandRunning) async -> MergeOutcome {
        var o = MergeOutcome()
        let path: String
        switch await resolveTarget(repo: repo, runner: runner) {
        case .failure(let f): o.error = f.message; return o
        case .success(let p): path = p
        }

        func git(_ args: [String], timeout: TimeInterval? = nil) async -> CommandResult {
            await runner.run([gitPath, "-C", path] + args, cwd: nil, timeout: timeout ?? gitTimeout)
        }
        let ref = "refs/machines/\(machine.name)/\(e.issue)"

        // 1. Main must be clean, or nothing else runs.
        let status = await git(["status", "--porcelain"])
        guard status.code == 0 else { o.error = "git status failed in \(path): \(tail(status))"; return o }
        guard status.out.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            o.error = "main has local changes in \(path)"
            return o
        }

        // 2. Local main catches up with origin.
        let fetchOrigin = await git(["fetch", "origin", "main"], timeout: fetchTimeout)
        guard fetchOrigin.code == 0 else { o.error = "fetch origin main failed: \(tail(fetchOrigin))"; return o }
        let catchUp = await git(["merge", "--ff-only", "origin/main"])
        guard catchUp.code == 0 else { o.error = "main cannot fast-forward to origin/main: \(tail(catchUp))"; return o }

        // 3. The machine's branch comes into a private ref. The `+` lets a branch the machine rewrote replace
        //    the ref left by an earlier try; the ref is Notchling's own and nothing is pushed from it.
        let fetchBranch = await git(["fetch", SSH.gitURL(machine, path: e.worktree), "+\(e.branch):\(ref)"],
                                    timeout: fetchTimeout)
        guard fetchBranch.code == 0 else { o.error = "fetch from \(machine.name) failed: \(tail(fetchBranch))"; return o }
        o.fetched = true

        // 4. The branch must be the commit that was shown as ready.
        let tip = await git(["rev-parse", ref])
        guard tip.code == 0, !e.commit.isEmpty, tip.out.trimmingCharacters(in: .whitespacesAndNewlines) == e.commit else {
            o.error = "branch moved on the machine; refresh"
            return o
        }

        // 5. The merge commit.
        let merge = await git(["merge", "--no-ff", "-m", "Merge \(e.branch) (\(repo.github)#\(e.issue))", ref])
        if merge.code != 0 {
            let diff = await git(["diff", "--name-only", "--diff-filter=U"])
            let files = diff.out.split(whereSeparator: \.isNewline).map(String.init).filter { !$0.isEmpty }
            _ = await git(["merge", "--abort"])
            guard !files.isEmpty else { o.error = "merge failed: \(tail(merge))"; return o }
            o.conflictFiles = files
            o.error = "merge conflict in \(files.joined(separator: ", "))"
            let note = "notchling: merging \(e.branch) into main conflicts in \(files.joined(separator: ", ")). Needs you."
            let comment = await gh(["issue", "comment", "\(e.issue)", "-R", repo.github, "--body", note], runner)
            let label = await gh(["issue", "edit", "\(e.issue)", "-R", repo.github,
                                  "--remove-label", "sd:ready", "--add-label", "sd:needs-you"], runner)
            if comment.code != 0 || label.code != 0 {
                o.error = (o.error ?? "") + " (the issue could not be updated: \(tail(comment.code != 0 ? comment : label)))"
            }
            return o
        }
        // A merge that found nothing to do leaves ORIG_HEAD from an older merge, so it must not reach the reset below.
        guard !merge.out.contains("Already up to date") else {
            o.error = "\(e.branch) is already in main; nothing to merge"
            return o
        }
        o.merged = true
        let head = await git(["rev-parse", "HEAD"])
        guard head.code == 0 else {
            o.error = "git rev-parse HEAD failed: \(tail(head))"
            return o
        }
        let sha = head.out.trimmingCharacters(in: .whitespacesAndNewlines)
        o.commit = sha

        // 6. The repo's check, in the target.
        let check = await runner.run([shellPath, "-c", repo.checkCommand], cwd: path, timeout: checkTimeout)
        guard check.code == 0 else {
            // Step 1 proved the target clean, so this loses only the merge.
            let reset = await git(["reset", "--hard", "ORIG_HEAD"])
            o.merged = false
            o.commit = nil
            o.error = "check failed: \(tail(check))"
            if reset.code != 0 { o.error = (o.error ?? "") + " (and the merge could not be undone in \(path): \(tail(reset)))" }
            return o
        }
        o.checked = true

        return await pushAndClose(&o, e, repo: repo, sha: sha, path: path, runner: runner)
    }

    /// Repeats the push and the issue close alone, after a push failed.
    static func retryPush(_ e: ReadyEntry, machine: Machine, repo: MachineRepo, runner: CommandRunning) async -> MergeOutcome {
        var o = MergeOutcome()
        let path: String
        switch await resolveTarget(repo: repo, runner: runner) {
        case .failure(let f): o.error = f.message; return o
        case .success(let p): path = p
        }
        func git(_ args: [String]) async -> CommandResult {
            await runner.run([gitPath, "-C", path] + args, cwd: nil, timeout: gitTimeout)
        }
        // Only a merge that is still in main (so its check passed) goes out; a rolled-back one does not.
        let ref = "refs/machines/\(machine.name)/\(e.issue)"
        let inMain = await git(["merge-base", "--is-ancestor", ref, "HEAD"])
        guard inMain.code == 0 else { o.error = "the merge of \(e.branch) is not in main here; nothing to push"; return o }
        let head = await git(["rev-parse", "HEAD"])
        guard head.code == 0 else { o.error = "git rev-parse HEAD failed: \(tail(head))"; return o }
        let sha = head.out.trimmingCharacters(in: .whitespacesAndNewlines)
        o.fetched = true
        o.merged = true
        o.checked = true
        o.commit = sha
        return await pushAndClose(&o, e, repo: repo, sha: sha, path: path, runner: runner)
    }

    // MARK: Steps 7 and 8

    /// Pushes `main` (and nothing else, never forced) then closes the issue. A failed push keeps the merge commit.
    private static func pushAndClose(_ o: inout MergeOutcome, _ e: ReadyEntry, repo: MachineRepo, sha: String,
                                     path: String, runner: CommandRunning) async -> MergeOutcome {
        let push = await runner.run([gitPath, "-C", path, "push", "origin", "main"], cwd: nil, timeout: gitTimeout)
        guard push.code == 0 else {
            o.error = "push failed: \(tail(push))"
            return o
        }
        o.pushed = true
        let close = await gh(["issue", "close", "\(e.issue)", "-R", repo.github, "--comment", "notchling: merged in \(sha)"], runner)
        guard close.code == 0 else {
            o.error = "pushed, but the issue could not be closed: \(tail(close))"
            return o
        }
        o.closed = true
        let label = await gh(["issue", "edit", "\(e.issue)", "-R", repo.github, "--remove-label", "sd:ready"], runner)
        if label.code != 0 { o.error = "closed, but sd:ready could not be removed: \(tail(label))" }
        return o
    }

    // MARK: Helpers

    private struct Failure: Error { let message: String }

    /// The folder to merge in, with `~` expanded and its parent made. Notchling's own worktree is added the first time.
    private static func resolveTarget(repo: MachineRepo, runner: CommandRunning) async -> Result<String, Failure> {
        let list = await runner.run([gitPath, "-C", repo.localPath, "worktree", "list", "--porcelain"], cwd: nil, timeout: gitTimeout)
        guard list.code == 0 else { return .failure(Failure(message: "git worktree list failed in \(repo.localPath): \(tail(list))")) }
        switch target(repo: repo, worktreeList: list.out) {
        case .existing(let path):
            return .success(path)
        case .own(let raw):
            let path = expandHome(raw)
            let parent = (path as NSString).deletingLastPathComponent
            let mkdir = await runner.run(["/bin/mkdir", "-p", parent], cwd: nil, timeout: gitTimeout)
            guard mkdir.code == 0 else { return .failure(Failure(message: "cannot make \(parent): \(tail(mkdir))")) }
            let add = await runner.run([gitPath, "-C", repo.localPath, "worktree", "add", path, "main"], cwd: nil, timeout: gitTimeout)
            guard add.code == 0 else { return .failure(Failure(message: "cannot add the main worktree at \(path): \(tail(add))")) }
            return .success(path)
        }
    }

    private static func gh(_ args: [String], _ runner: CommandRunning) async -> CommandResult {
        await runner.run([ghPath] + args, cwd: nil, timeout: gitTimeout)
    }

    private static func expandHome(_ path: String) -> String {
        guard path == "~" || path.hasPrefix("~/") else { return path }
        return FileManager.default.homeDirectoryForCurrentUser.path + path.dropFirst()
    }

    /// The end of what a command printed, enough to see why it failed.
    private static func tail(_ r: CommandResult) -> String {
        if r.code == 124 { return "timed out" }
        let text = (r.err + "\n" + r.out).trimmingCharacters(in: .whitespacesAndNewlines)
        let lines = text.split(whereSeparator: \.isNewline).suffix(20).joined(separator: "\n")
        return lines.count > 1500 ? "…" + lines.suffix(1500) : lines
    }
}

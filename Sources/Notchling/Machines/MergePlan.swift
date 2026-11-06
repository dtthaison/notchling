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

    /// The merge was checked and sits on main, and only the push is left.
    var canRetryPush: Bool { merged && checked && !pushed }
    /// The merge sits on main and has not reached origin; undoing it lets the item be merged again.
    var canUndo: Bool { merged && !pushed }
}

/// Merges a machine's ready branch into `main` from the MacBook.
///
/// The one thing it must never do is merge or break `main` without you: it works only in a `main` worktree
/// that is clean, pushes nothing but `main` to `origin` and only after the repo's check passed, and never
/// forces anything.
///
/// Everything it asks git is spelled with full refnames (`refs/heads/main`, `refs/remotes/origin/main`,
/// `refs/machines/...`): a short name resolves to a tag first, and a machine must never be able to plant a tag
/// that stands in for `main`. Nothing it fetches from a machine brings tags (`--no-tags`).
enum MergePlan {
    /// A fetch can take a while over ssh.
    private static let fetchTimeout: TimeInterval = 300
    /// Every other git call.
    private static let gitTimeout: TimeInterval = 60
    /// The repo's check (build, tests).
    private static let checkTimeout: TimeInterval = 1800
    private static var gitPath: String { MachineTools.path("git") }
    private static var ghPath: String { MachineTools.path("gh") }
    private static let localMain = "refs/heads/main"
    private static let originMain = "refs/remotes/origin/main"
    /// Fetches `main` from origin into its remote-tracking ref, and nothing else (no tags).
    private static let fetchOriginArgs = ["fetch", "--no-tags", "origin", "+\(localMain):\(originMain)"]
    /// Commits on `main` that origin does not have.
    private static let unpushed = "\(originMain)..\(localMain)"

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
        if let why = refusedBranch(e.branch) { o.error = why; return o }
        let path: String
        switch await resolveTarget(repo: repo, runner: runner) {
        case .failure(let f): o.error = f.message; return o
        case .success(let p): path = p
        }

        func git(_ args: [String], timeout: TimeInterval? = nil) async -> CommandResult {
            await runner.run([gitPath, "-C", path] + args, cwd: nil, timeout: timeout ?? gitTimeout)
        }
        let ref = "refs/machines/\(machine.name)/\(e.issue)"

        // The branch name goes into a refspec: git must read it as one plain branch.
        if let why = await gitRefusesBranch(e.branch, path: path, runner: runner) { o.error = why; return o }

        // 1. Main must be clean, or nothing else runs.
        let status = await git(["status", "--porcelain"])
        guard status.code == 0 else { o.error = "git status failed in \(path): \(tail(status))"; return o }
        guard trimmed(status).isEmpty else {
            o.error = "main has local changes in \(path)"
            return o
        }

        // 2. Local main catches up with origin, and must not be ahead of it: the push below would send those
        //    commits too, although nobody asked for that.
        let fetchOrigin = await git(fetchOriginArgs, timeout: fetchTimeout)
        guard fetchOrigin.code == 0 else { o.error = "fetch origin main failed: \(tail(fetchOrigin))"; return o }
        let catchUp = await git(["merge", "--ff-only", originMain])
        guard catchUp.code == 0 else { o.error = "main cannot fast-forward to origin/main: \(tail(catchUp))"; return o }
        let ahead = await git(["rev-list", "--count", unpushed])
        guard ahead.code == 0, let count = Int(trimmed(ahead)) else {
            o.error = "git rev-list failed: \(tail(ahead))"
            return o
        }
        if count > 0 {
            // An earlier try of this very item (Notchling quit during its check, say) leaves exactly its merge.
            // It was never proven by a check, so it is for Undo, not for a push.
            let alone = await git(["rev-list", "--first-parent", "--count", unpushed])
            if alone.code == 0, trimmed(alone) == "1", let mine = await ownMerge(e, of: localMain, git: { await git($0) }) {
                o.merged = true
                o.commit = mine
                o.error = "merged locally, not pushed: main already holds the merge of \(e.branch) from an earlier try, "
                    + "and nothing here proves its check passed. Undo the merge, then merge again."
                return o
            }
            o.error = "main has \(count) commit(s) that are not on origin; push or undo them yourself"
            return o
        }

        // 3. The machine's branch comes into a private ref, without its tags. The `+` lets a branch the machine
        //    rewrote replace the ref left by an earlier try; the ref is Notchling's own and nothing is pushed from it.
        let fetchBranch = await git(["fetch", "--no-tags", SSH.gitURL(machine, path: e.worktree),
                                     "+refs/heads/\(e.branch):\(ref)"], timeout: fetchTimeout)
        guard fetchBranch.code == 0 else { o.error = "fetch from \(machine.name) failed: \(tail(fetchBranch))"; return o }
        o.fetched = true

        // 4. The branch must be the commit that was shown as ready.
        let tip = await git(["rev-parse", ref])
        guard tip.code == 0, !e.commit.isEmpty, trimmed(tip) == e.commit else {
            o.error = "branch moved on the machine; refresh"
            return o
        }

        // 5. The merge commit, of the verified sha itself: nothing that rewrites the private ref meanwhile can
        //    change what is merged. `pre` is where a failed check puts main back.
        let preHead = await git(["rev-parse", "HEAD"])
        let pre = trimmed(preHead)
        guard preHead.code == 0, !pre.isEmpty else { o.error = "git rev-parse HEAD failed: \(tail(preHead))"; return o }
        let merge = await git(["merge", "--no-ff", "-m", "Merge \(e.branch) (\(repo.github)#\(e.issue))", e.commit])
        if merge.code != 0 {
            // A merge that never started (unrelated histories, a file it would overwrite) has nothing to abort,
            // and its own error is the one to show.
            let started = await git(["rev-parse", "-q", "--verify", "MERGE_HEAD"])
            guard started.code == 0 else { o.error = "merge failed: \(tail(merge))"; return o }
            let diff = await git(["diff", "--name-only", "--diff-filter=U"])
            let files = diff.out.split(whereSeparator: \.isNewline).map(String.init).filter { !$0.isEmpty }
            let abort = await git(["merge", "--abort"])
            guard abort.code == 0 else {
                o.error = "merge could not be aborted in \(path); fix it by hand"
                return o
            }
            guard diff.code == 0 else { o.error = "merge failed, and the conflicting files could not be listed: \(tail(merge))"; return o }
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
        let head = await git(["rev-parse", "HEAD"])
        let sha = trimmed(head)
        guard head.code == 0, !sha.isEmpty else { o.error = "git rev-parse HEAD failed: \(tail(head))"; return o }
        // The branch was already in main: no merge commit, nothing to check or push.
        guard sha != pre else {
            o.error = "\(e.branch) is already in main; nothing to merge"
            return o
        }
        o.merged = true
        o.commit = sha

        // 6. The repo's check, in the target (the user's own main checkout when they have one; the merge is
        //    theirs to approve), in the user's login shell so the tools the repo needs are on its PATH. It runs a
        //    long time, so main is re-verified before anything acts on the result.
        let invocation = checkInvocation(repo.checkCommand)
        let check = await runner.run(invocation.argv, cwd: path, env: invocation.env, timeout: checkTimeout)
        guard check.code == 0 else {
            if let changed = await changeAfterCheck(sha, path: path, runner: runner, requireClean: false) {
                o.error = changed
                return o
            }
            // `--keep` never discards an edit made during the check.
            let reset = await git(["reset", "--keep", pre])
            if reset.code == 0 {
                o.error = "check failed: \(tail(check))"
                o.merged = false
                o.commit = nil
            } else {
                // The merge is still on main and its check failed: nothing here may offer to push it.
                o.error = "check failed and main could not be put back; fix it by hand in \(path) "
                    + "(check: \(tail(check)); reset: \(tail(reset)))"
            }
            return o
        }
        // The check ran on what was in the folder, so the merge counts as checked only if that was the commit
        // that is about to be pushed.
        if let changed = await changeAfterCheck(sha, path: path, runner: runner, requireClean: true) {
            o.error = changed
            return o
        }
        o.checked = true

        return await pushAndClose(&o, e, repo: repo, sha: sha, path: path, runner: runner)
    }

    /// Repeats the push and the issue close alone, after a push failed. It pushes only the merge commit this
    /// branch made, sitting alone on top of origin/main with a clean tree on main; anything else is the user's.
    /// Only a merge whose check passed is ever offered here (`MergeOutcome.canRetryPush`).
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
        func refuse(_ why: String) -> MergeOutcome {
            o.error = "cannot push again: \(why)"
            return o
        }
        let status = await git(["status", "--porcelain"])
        guard status.code == 0, trimmed(status).isEmpty else {
            return refuse("main has local changes in \(path)")
        }
        let branch = await git(["symbolic-ref", "HEAD"])
        guard branch.code == 0, trimmed(branch) == localMain else {
            return refuse("\(path) is not on main")
        }
        let second = await git(["rev-parse", "HEAD^2"])
        guard second.code == 0, !e.commit.isEmpty, trimmed(second) == e.commit else {
            return refuse("main's last commit is not the merge of \(e.branch)")
        }
        // First parents only: the merged branch's own commits are not on origin either.
        let ahead = await git(["rev-list", "--first-parent", "--count", unpushed])
        guard ahead.code == 0, trimmed(ahead) == "1" else {
            return refuse("main is not exactly one merge ahead of origin")
        }
        let head = await git(["rev-parse", "HEAD"])
        let sha = trimmed(head)
        guard head.code == 0, !sha.isEmpty else { return refuse("git rev-parse HEAD failed: \(tail(head))") }
        o.fetched = true
        o.merged = true
        o.checked = true
        o.commit = sha
        return await pushAndClose(&o, e, repo: repo, sha: sha, path: path, runner: runner)
    }

    /// Takes this item's merge off `main` again, while it sits there alone and unpushed: after a push origin
    /// refused, a change made during the check, or a quit during the check. `main` goes back to the commit the
    /// merge was made on, so the item can be merged again on top of what origin has now. Nothing else is touched:
    /// the tree must be clean, `main` checked out, `HEAD` a two-parent merge whose second parent is this item's
    /// commit, and exactly one first-parent commit ahead of origin once origin is fetched.
    /// Success is an outcome with no error and nothing merged.
    static func undoMerge(_ e: ReadyEntry, machine: Machine, repo: MachineRepo, runner: CommandRunning) async -> MergeOutcome {
        var o = MergeOutcome()
        let path: String
        switch await resolveTarget(repo: repo, runner: runner) {
        case .failure(let f): o.error = f.message; return o
        case .success(let p): path = p
        }
        func git(_ args: [String], timeout: TimeInterval? = nil) async -> CommandResult {
            await runner.run([gitPath, "-C", path] + args, cwd: nil, timeout: timeout ?? gitTimeout)
        }
        func refuse(_ why: String) -> MergeOutcome {
            o.error = "cannot undo the merge: \(why)"
            return o
        }
        let status = await git(["status", "--porcelain"])
        guard status.code == 0, trimmed(status).isEmpty else {
            return refuse("main has local changes in \(path); put them away first")
        }
        let branch = await git(["symbolic-ref", "HEAD"])
        guard branch.code == 0, trimmed(branch) == localMain else {
            return refuse("\(path) is not on main")
        }
        guard await ownMerge(e, of: "HEAD", git: { await git($0) }) != nil else {
            return refuse("main's last commit is not a merge of \(e.branch) alone")
        }
        // Whether the merge sits alone on top of origin is judged against what origin has now.
        let fetch = await git(fetchOriginArgs, timeout: fetchTimeout)
        guard fetch.code == 0 else { return refuse("fetch origin main failed: \(tail(fetch))") }
        let ahead = await git(["rev-list", "--first-parent", "--count", unpushed])
        guard ahead.code == 0, trimmed(ahead) == "1" else {
            return refuse("main holds other commits that are not on origin; undo them yourself")
        }
        let reset = await git(["reset", "--keep", "HEAD^1"])
        guard reset.code == 0 else { return refuse("git reset failed: \(tail(reset))") }
        return MergeOutcome()
    }

    /// The sha of `revision` when it is a two-parent merge whose second parent is this item's commit, else nil.
    private static func ownMerge(_ e: ReadyEntry, of revision: String,
                                 git: ([String]) async -> CommandResult) async -> String? {
        guard !e.commit.isEmpty else { return nil }
        let parents = await git(["rev-list", "--parents", "-n", "1", revision])
        let parts = trimmed(parents).split(separator: " ").map(String.init)
        guard parents.code == 0, parts.count == 3, parts[2] == e.commit else { return nil }
        return parts[0]
    }

    // MARK: Steps 7 and 8

    private static let changedError = "main changed during the check; nothing pushed"
    private static let dirtyError = "main has local changes made during the check; nothing pushed"

    /// Why the result of the check can no longer be used, or nil while the target is still on `main` at the
    /// merge commit (and, before a push, clean: the check ran on that tree and the push sends that commit).
    private static func changeAfterCheck(_ sha: String, path: String, runner: CommandRunning, requireClean: Bool) async -> String? {
        let branch = await runner.run([gitPath, "-C", path, "symbolic-ref", "HEAD"], cwd: nil, timeout: gitTimeout)
        guard branch.code == 0, trimmed(branch) == localMain else { return changedError }
        let head = await runner.run([gitPath, "-C", path, "rev-parse", "HEAD"], cwd: nil, timeout: gitTimeout)
        guard head.code == 0, trimmed(head) == sha else { return changedError }
        if requireClean {
            let status = await runner.run([gitPath, "-C", path, "status", "--porcelain"], cwd: nil, timeout: gitTimeout)
            guard status.code == 0 else { return changedError }
            guard trimmed(status).isEmpty else { return dirtyError }
        }
        return nil
    }

    /// Pushes `main` (and nothing else, never forced) then closes the issue. A failed push keeps the merge commit.
    private static func pushAndClose(_ o: inout MergeOutcome, _ e: ReadyEntry, repo: MachineRepo, sha: String,
                                     path: String, runner: CommandRunning) async -> MergeOutcome {
        let push = await runner.run([gitPath, "-C", path, "push", "origin", "\(localMain):\(localMain)"], cwd: nil, timeout: gitTimeout)
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

    // MARK: The check's shell

    /// The user's login shell with the directories the repos' tools live in put in front of PATH. A menu bar app
    /// starts with a bare PATH, and a repo's `just` comes from mise and its `cargo` from `~/.cargo/bin`.
    static func checkInvocation(_ checkCommand: String,
                                environment: [String: String] = ProcessInfo.processInfo.environment,
                                home: String = FileManager.default.homeDirectoryForCurrentUser.path,
                                isExecutable: (String) -> Bool = { FileManager.default.isExecutableFile(atPath: $0) })
        -> (argv: [String], env: [String: String]) {
        let shell = environment["SHELL"].flatMap { $0.hasPrefix("/") && isExecutable($0) ? $0 : nil } ?? "/bin/zsh"
        let tools = ["\(home)/.cargo/bin", "\(home)/.local/share/mise/shims", "/opt/homebrew/bin", "/usr/local/bin"]
        let path = (tools + [environment["PATH"] ?? "/usr/bin:/bin:/usr/sbin:/sbin"]).joined(separator: ":")
        return ([shell, "-lc", checkCommand], ["PATH": path])
    }

    // MARK: Branch names

    /// A branch name git must never be handed as an option or a refspec part: empty, starting with `-`, or
    /// holding a space or control character. Git's own rules (`check-ref-format`) judge the rest.
    static func refusedBranch(_ branch: String) -> String? {
        let odd = branch.isEmpty || branch.hasPrefix("-")
            || branch.unicodeScalars.contains { $0.value <= 0x20 || $0.value == 0x7f }
        return odd ? notABranch(branch) : nil
    }

    /// `git check-ref-format --branch` must accept the name and print it back unchanged (it expands `@{-1}`).
    static func gitRefusesBranch(_ branch: String, path: String, runner: CommandRunning) async -> String? {
        let r = await runner.run([gitPath, "-C", path, "check-ref-format", "--branch", branch], cwd: nil, timeout: gitTimeout)
        return r.code == 0 && trimmed(r) == branch ? nil : notABranch(branch)
    }

    /// Both checks, for a caller that has the repo's folder and no other use for a target.
    static func branchError(_ branch: String, path: String, runner: CommandRunning) async -> String? {
        if let why = refusedBranch(branch) { return why }
        return await gitRefusesBranch(branch, path: path, runner: runner)
    }

    private static func notABranch(_ branch: String) -> String { "\"\(branch)\" is not a branch name git accepts" }

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

    private static func trimmed(_ r: CommandResult) -> String { r.out.trimmingCharacters(in: .whitespacesAndNewlines) }

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

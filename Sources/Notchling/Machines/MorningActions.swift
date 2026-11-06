import Foundation

/// What you can do with a morning item. Every action runs `gh`, `git` or `scp` on this Mac and never touches a
/// worker machine except to read from it. One action per item at a time: a second one on the same id is ignored.
@MainActor
final class MorningActions: ObservableObject {
    typealias MergeFunction = (ReadyEntry, Machine, MachineRepo, CommandRunning) async -> MergeOutcome

    /// Item ids with an action in flight. An id leaves the set when its action ends, failed or not.
    @Published private(set) var running: Set<String> = []
    /// The last failure of an item's action, by item id. A new action on the item clears it.
    @Published private(set) var lastError: [String: String] = [:]
    /// What the last merge or push retry of an item got through, by item id (the window offers "retry push"
    /// when it merged but did not push).
    @Published private(set) var outcomes: [String: MergeOutcome] = [:]

    /// Prefix of every comment Notchling leaves on an issue.
    static let commentPrefix = "notchling: "

    /// The branch commit each outcome was made for, by item id.
    private var outcomeCommits: [String: String] = [:]

    private let runner: CommandRunning
    private let machines: () -> [Machine]
    private let mergeFunction: MergeFunction
    private let retryPushFunction: MergeFunction
    private let cacheRoot: URL

    init(runner: CommandRunning = ProcessRunner(),
         machines: @escaping () -> [Machine] = { MachineRegistry.all() },
         merge: @escaping MergeFunction = MergePlan.merge,
         retryPush: @escaping MergeFunction = MergePlan.retryPush,
         cacheRoot: URL = FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library/Caches/Notchling/machines")) {
        self.runner = runner
        self.machines = machines
        self.mergeFunction = merge
        self.retryPushFunction = retryPush
        self.cacheRoot = cacheRoot
    }

    // MARK: Actions on a ready branch

    /// Each action returns the task that runs it, so a caller (or a test) can wait for the end. An ignored
    /// action returns a task that is already done.
    @discardableResult func merge(_ item: MorningItem) -> Task<Void, Never> { mergeLike(item, mergeFunction) }

    @discardableResult func retryPush(_ item: MorningItem) -> Task<Void, Never> { mergeLike(item, retryPushFunction) }

    /// Puts the issue back in the worker's queue with a note.
    @discardableResult func sendBack(_ item: MorningItem, note: String) -> Task<Void, Never> {
        comment(item, Self.commentPrefix + note, then: ["--add-label", "sd:queued", "--remove-label", "sd:ready"])
    }

    // MARK: Actions on a question

    @discardableResult func answer(_ item: MorningItem, text: String) -> Task<Void, Never> {
        comment(item, Self.commentPrefix + text, then: ["--add-label", "sd:queued", "--remove-label", "sd:needs-you"])
    }

    @discardableResult func useRecommended(_ item: MorningItem) -> Task<Void, Never> {
        answer(item, text: "use recommended")
    }

    // MARK: Actions on a stuck job

    @discardableResult func retry(_ item: MorningItem) -> Task<Void, Never> {
        start(item) { [self] in await edit(item, ["--add-label", "sd:queued", "--remove-label", "sd:stuck"]) }
    }

    /// Closes the issue as not planned.
    @discardableResult func close(_ item: MorningItem) -> Task<Void, Never> {
        start(item) { [self] in
            await gh(["issue", "close", "\(item.issue)", "-R", item.repo, "--reason", "not planned"])
        }
    }

    /// Same as `close`: the worker removes the worktree when the issue closes.
    @discardableResult func drop(_ item: MorningItem) -> Task<Void, Never> { close(item) }

    /// The outcome of this item's last merge or push retry, only while the item still has the commit it was made
    /// for. A branch sent back and returned with a new commit starts clean.
    func outcome(for item: MorningItem) -> MergeOutcome? {
        guard case .ready(let entry) = item.entry, outcomeCommits[item.id] == entry.commit else { return nil }
        return outcomes[item.id]
    }

    /// Forgets the outcome of every item that left `items` or now has another commit.
    func pruneOutcomes(keeping items: [MorningItem]) {
        for id in Array(outcomes.keys) {
            if let item = items.first(where: { $0.id == id }), outcome(for: item) != nil { continue }
            outcomes[id] = nil
            outcomeCommits[id] = nil
        }
    }

    // MARK: Reading

    /// The diff of the branch against `main`, or the error text. Fetches the branch into
    /// `refs/machines/<machine>/<issue>` of the repo's checkout first (the same ref a merge uses), which leaves
    /// the work tree alone.
    func diff(_ item: MorningItem) async -> String {
        guard let (machine, repo) = lookup(item) else { return Self.unknownMachine(item) }
        guard let source = branchSource(item) else { return "\(item.id) has no branch on the machine" }
        let git = MachineTools.path("git")
        let ref = "refs/machines/\(machine.name)/\(item.issue)"
        let ssh = "ssh -o BatchMode=yes -o ConnectTimeout=8"
        let fetch = await runner.run([git, "-C", repo.localPath, "-c", "core.sshCommand=\(ssh)", "fetch",
                                      SSH.gitURL(machine, path: source.worktree), "+\(source.branch):\(ref)"],
                                     cwd: nil, timeout: 120)
        guard fetch.code == 0 else { return Self.failure(fetch, "git fetch") }
        let diff = await runner.run([git, "-C", repo.localPath, "diff", "main...\(ref)"], cwd: nil, timeout: 60)
        return diff.code == 0 ? diff.out : Self.failure(diff, "git diff")
    }

    /// Copies the PNG and JPG files of the verify evidence folder from the machine into
    /// `~/Library/Caches/Notchling/machines/<machine>/<issue>/` and returns the local files. Only a ready
    /// branch has an evidence folder.
    func screenshots(_ item: MorningItem) async -> [URL] {
        guard case .ready(let entry) = item.entry, !entry.evidenceDir.isEmpty,
              let (machine, _) = lookup(item) else { return [] }
        let listing = await runner.run(SSH.argv(machine, ["ls", "-1", entry.evidenceDir]), cwd: nil, timeout: 30)
        guard listing.code == 0 else { return [] }
        let names = listing.out.split(separator: "\n").map(String.init).filter(Self.isImage).sorted()
        guard !names.isEmpty else { return [] }

        let dir = cacheRoot.appendingPathComponent(machine.name).appendingPathComponent("\(item.issue)", isDirectory: true)
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        var copied: [URL] = []
        for name in names {
            let remote = entry.evidenceDir.hasSuffix("/") ? entry.evidenceDir + name : entry.evidenceDir + "/" + name
            let target = dir.appendingPathComponent(name)
            // -O: the legacy protocol runs the path through the remote shell, so the quoting below holds
            let argv = ["/usr/bin/scp", "-O", "-q", "-o", "BatchMode=yes", "-o", "ConnectTimeout=8",
                        "\(machine.user)@\(machine.host):\(LaneStore.quote(remote))", target.path]
            if await runner.run(argv, cwd: nil, timeout: 60).code == 0 { copied.append(target) }
        }
        return copied
    }

    // MARK: Plumbing

    private func mergeLike(_ item: MorningItem, _ function: @escaping MergeFunction) -> Task<Void, Never> {
        start(item) { [self] in
            guard case .ready(let entry) = item.entry else { return "\(item.id) is not a ready branch" }
            guard let (machine, repo) = lookup(item) else { return Self.unknownMachine(item) }
            let outcome = await function(entry, machine, repo, runner)
            outcomes[item.id] = outcome
            outcomeCommits[item.id] = entry.commit
            return Self.failure(of: outcome)
        }
    }

    /// Comments, then edits labels. A failed comment stops the action before the labels move.
    private func comment(_ item: MorningItem, _ body: String, then labels: [String]) -> Task<Void, Never> {
        start(item) { [self] in
            if let error = await gh(["issue", "comment", "\(item.issue)", "-R", item.repo, "--body", body]) { return error }
            return await edit(item, labels)
        }
    }

    private func edit(_ item: MorningItem, _ flags: [String]) async -> String? {
        await gh(["issue", "edit", "\(item.issue)", "-R", item.repo] + flags)
    }

    /// One `gh` call; the error text, or nil when it worked.
    private func gh(_ args: [String]) async -> String? {
        let result = await runner.run([MachineTools.path("gh")] + args, cwd: nil, timeout: 30)
        return result.code == 0 ? nil : Self.failure(result, "gh \(args.prefix(2).joined(separator: " "))")
    }

    /// Runs `work` unless this item already has an action; its error text becomes `lastError`.
    private func start(_ item: MorningItem, _ work: @escaping @MainActor () async -> String?) -> Task<Void, Never> {
        guard !running.contains(item.id) else { return Task {} }
        running.insert(item.id)
        lastError[item.id] = nil
        return Task { @MainActor in
            if let error = await work() { lastError[item.id] = error }
            running.remove(item.id)
        }
    }

    private func lookup(_ item: MorningItem) -> (Machine, MachineRepo)? {
        guard let machine = machines().first(where: { $0.name == item.machine }),
              let repo = machine.repos.first(where: { $0.github == item.repo }) else { return nil }
        return (machine, repo)
    }

    private func branchSource(_ item: MorningItem) -> (branch: String, worktree: String)? {
        switch item.entry {
        case .ready(let e): return e.branch.isEmpty || e.worktree.isEmpty ? nil : (e.branch, e.worktree)
        case .needsYou(let e): return e.branch.flatMap { b in e.worktree.map { (b, $0) } }
        case .stuck(let e): return e.branch.flatMap { b in e.worktree.map { (b, $0) } }
        }
    }

    private static func isImage(_ name: String) -> Bool {
        ["png", "jpg", "jpeg"].contains((name as NSString).pathExtension.lowercased())
    }

    private static func unknownMachine(_ item: MorningItem) -> String {
        "\(item.machine) or \(item.repo) is not in the machines list"
    }

    private static func failure(_ r: CommandResult, _ what: String) -> String {
        let text = r.err.trimmingCharacters(in: .whitespacesAndNewlines)
        if r.code == 124 { return "\(what) timed out" }
        return text.isEmpty ? "\(what) exited \(r.code)" : text
    }

    /// nil when the merge or the push retry went all the way through.
    private static func failure(of o: MergeOutcome) -> String? {
        if let e = o.error { return e }
        if !o.conflictFiles.isEmpty { return "merge conflict in \(o.conflictFiles.joined(separator: ", "))" }
        if !o.pushed { return "main was not pushed" }
        if !o.closed { return "the issue was not closed" }
        return nil
    }
}

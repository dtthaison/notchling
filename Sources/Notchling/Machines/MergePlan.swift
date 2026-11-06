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
/// Interface stub so Task 5 can build against it in parallel; Task 4 implements it.
enum MergePlan {
    static func target(repo: MachineRepo, worktreeList: String) -> MergeTarget {
        .own(path: "")
    }

    static func merge(_ e: ReadyEntry, machine: Machine, repo: MachineRepo, runner: CommandRunning) async -> MergeOutcome {
        MergeOutcome(error: "not implemented")
    }

    /// Repeats the push and the issue close alone, after a push failed.
    static func retryPush(_ e: ReadyEntry, machine: Machine, repo: MachineRepo, runner: CommandRunning) async -> MergeOutcome {
        MergeOutcome(error: "not implemented")
    }
}

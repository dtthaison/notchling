import Foundation

/// Steps 9–11 for one repo of the sd worker job: deploy key, clone, push refused.
/// Task 6 fills the bodies; the metadata and signatures here are final.
enum RepoSteps {
    static func all(for machine: Machine, repo: MachineRepo) -> [SetupStep] {
        let github = repo.github
        return [
            SetupStep(id: .deployKey(github), title: "Deploy key (\(github))", needs: [.reachable, .baseTools],
                      cadence: .slow, probe: .notBuilt, fix: .notBuiltRun()),
            SetupStep(id: .clone(github), title: "Clone (\(github))", needs: [.deployKey(github)],
                      cadence: .everyRefresh, probe: .notBuilt, fix: .notBuiltRun()),
            SetupStep(id: .pushRefused(github), title: "Push is refused (\(github))", needs: [.clone(github)],
                      cadence: .slow, probe: .notBuilt,
                      fix: .howTo("The deploy key must be read-only. Remove it in the repo's settings on GitHub and set it up again.")),
        ]
    }

    /// `owner/name` as one word for file and host names: lowercase, anything outside `[a-z0-9-]` becomes `-`.
    static func slug(_ github: String) -> String { "" }
    /// The ssh host alias git uses for the repo: `github-agents-<slug>`.
    static func alias(_ github: String) -> String { "" }
    /// The private key file on the machine: `~/.ssh/agents-<slug>`.
    static func keyFile(_ github: String) -> String { "" }
    /// The clone URL through the alias: `git@github-agents-<slug>:<owner>/<name>.git`.
    static func remoteURL(_ github: String) -> String { "" }
}

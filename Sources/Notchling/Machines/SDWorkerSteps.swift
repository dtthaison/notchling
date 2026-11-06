import Foundation

/// A plugin as this Mac has it installed.
struct InstalledPlugin: Equatable, Sendable {
    let version: String
    let path: String
}

/// What this Mac has installed, read once per refresh: the plugin versions to put on a machine.
struct MacFacts: Sendable {
    /// By plugin id, e.g. `sd@system-design`.
    var plugins: [String: InstalledPlugin]
    /// The marketplace manifests by marketplace name.
    var marketplaceJSON: [String: String]

    static func load(home: URL) -> MacFacts { MacFacts(plugins: [:], marketplaceJSON: [:]) }
}

/// Steps 12 and 15–18 of the sd worker job: plugins, worker.json, service, setup check, running.
/// Task 9 fills the bodies; the metadata and signatures here are final.
enum SDWorkerSteps {
    static func all(for machine: Machine, job: SDWorkerSettings, all: [Machine]) -> [SetupStep] {
        let linux = machine.os == .linux
        let clones = job.repos.map { StepID.clone($0.github) }
        return [
            SetupStep(id: .plugins, title: "Plugins installed", needs: [.claudeCode], cadence: .everyRefresh,
                      probe: .notBuilt, fix: .notBuiltRun()),
            SetupStep(id: .workerJSON, title: "worker.json written",
                      needs: [.plugins, .issuesToken, .claudeLogin] + clones, cadence: .everyRefresh,
                      probe: .notBuilt, fix: .notBuiltRun()),
            SetupStep(id: .service, title: "Service installed", needs: [.workerJSON, .baseTools], cadence: .everyRefresh,
                      probe: .notBuilt, fix: .notBuiltRun(needsAdmin: linux)),
            SetupStep(id: .sdCheck, title: "sd worker setup --check", needs: [.service], cadence: .slow,
                      probe: .notBuilt,
                      fix: .howTo("Read the rows under this step. Fix what they name, then check again.")),
            SetupStep(id: .running, title: "Running", needs: [.sdCheck], cadence: .everyRefresh,
                      probe: .notBuilt, fix: .toggle(needsAdmin: linux)),
        ]
    }
}

import Foundation

/// Steps 1–6 and 8 of the catalog: the machine itself. Claude login (7) comes from `SecretSteps`.
/// Task 5 fills the probes and Fixes; the metadata here is final.
enum MachineSteps {
    /// Reachable, Worker user, Stays awake, FileVault, Base tools, Claude Code, Xcode. The catalog leaves out the
    /// macOS-only ones for a Linux machine.
    static func all(for machine: Machine) -> [SetupStep] {
        let linux = machine.os == .linux
        return [
            SetupStep(id: .reachable, title: "Reachable", needs: [], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .howTo("Join the machine to Tailscale and turn on Tailscale SSH, then check again.")),
            SetupStep(id: .workerUser, title: "Worker user", needs: [.reachable], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .howTo("Create the worker user on the machine and give it sudo, then check again.")),
            SetupStep(id: .staysAwake, title: "Stays awake", needs: [.reachable], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .notBuiltRun(needsAdmin: true)),
            SetupStep(id: .filevault, title: "FileVault", needs: [.reachable], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .howTo("Turn FileVault on or off in System Settings on the machine, then check again.")),
            SetupStep(id: .baseTools, title: "Base tools", needs: [.reachable], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .notBuiltRun(needsAdmin: linux)),
            SetupStep(id: .claudeCode, title: "Claude Code", needs: [.reachable], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .notBuiltRun()),
            SetupStep(id: .xcode, title: "Xcode", needs: [.reachable], cadence: .everyRefresh, probe: .notBuilt,
                      fix: .howTo("Notchling doesn't install Xcode, which needs your Apple ID. On the machine: App Store, open it once, Xcode > Settings > Components for the iOS runtime, then Check again.")),
        ]
    }
}

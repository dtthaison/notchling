import Foundation

/// Made only by a button's action: the user's click is what lets a Fix run.
struct FixApproval {
    let adminPassword: String?
    let asRootConfirmed: Bool
}

enum AdminAnswer { case password(String), rootConfirmed, cancelled }

/// Runs the checks and the Fixes. Task 10 fills the bodies; the signatures here are final.
///
/// Output reaches the engine from any thread (`FixContext.log` fires from the stdout and stderr readers and
/// for a moment after a run returned). The engine's log closure appends each line to a lock-protected buffer, or
/// yields it into one serial `AsyncStream`, so the order is kept and the MainActor drains it. It never starts a
/// MainActor `Task` per line (those can reorder), and it drops lines that arrive after its Fix ended.
@MainActor
final class SetupEngine: ObservableObject {
    /// By machine name.
    @Published private(set) var results: [String: [StepID: CheckResult]] = [:]
    /// The Fix in flight per machine.
    @Published private(set) var running: [String: StepID] = [:]
    /// Redacted, the last 2000 lines per machine, memory only.
    @Published private(set) var logs: [String: [String]] = [:]
    /// Secret name → the usages to update after the value changed.
    @Published private(set) var updateOffers: [String: [SecretUsage]] = [:]
    /// Detected facts were not saved because the registry is unreadable.
    @Published private(set) var saveRefused = false

    init(runner: CommandRunning, secrets: SecretSource, http: HTTPFetching, mac: @escaping () -> MacFacts,
         machines: @escaping () -> [Machine], save: @escaping ([Machine]) -> Bool, clock: @escaping () -> Date,
         steps: @escaping (Machine, [Machine]) -> [SetupStep] = StepCatalog.steps,
         usages: @escaping ([Machine]) -> [SecretUsage] = SecretSteps.usages,
         deliver: @escaping (SecretUsage, FixContext) async -> FixOutcome = SecretSteps.deliver,
         setVaultExpiry: @escaping (String, String) -> Void = { try? SecretVault.setExpires($0, day: $1) },
         observeVault: Bool = true) {}

    func steps(for machine: Machine) -> [SetupStep] { [] }

    /// Checks only, never a Fix. `names` nil means every machine.
    func refresh(_ names: [String]? = nil, includeSlow: Bool = false) async {}

    /// For the Add sheet: the machine steps of a draft, nothing saved.
    func check(draft: Machine) async -> [StepID: CheckResult] { [:] }

    func runFix(_ id: StepID, on machine: String, approval: FixApproval) async -> FixOutcome {
        FixOutcome(ok: false, message: "not built yet")
    }

    func setUpAll(on machine: String, askAdmin: @escaping (SetupStep) async -> AdminAnswer) async -> FixOutcome {
        FixOutcome(ok: false, message: "not built yet")
    }

    func toggleRunning(on machine: String, to on: Bool, approval: FixApproval) async -> FixOutcome {
        FixOutcome(ok: false, message: "not built yet")
    }

    /// Makes the update offer; sends nothing.
    func vaultValueChanged(name: String) {}

    func updateMachines(for secret: String, approval: FixApproval) async -> [FixOutcome] { [] }
}

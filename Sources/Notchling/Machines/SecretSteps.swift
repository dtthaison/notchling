import AppKit
import Foundation

/// How a secret sits in its file on the machine.
enum SecretFormat: Equatable {
    /// The value alone, no newline (`github-token`).
    case raw
    /// `KEY=<value>\n` with this key (`claude.env`).
    case keyValue(String)
}

/// One place a vault secret is delivered to: a file on one machine.
struct SecretUsage: Equatable {
    let name: String
    let machine: String
    let remotePath: String
    let format: SecretFormat
}

/// Secrets: usages, delivery, steps 7 (Claude login) and 13 (issues token), token creation, renames.
/// Task 7 fills the bodies; the metadata and signatures here are final.
enum SecretSteps {
    static func claudeLogin(for machine: Machine) -> SetupStep {
        SetupStep(id: .claudeLogin, title: "Claude login", needs: [.reachable], cadence: .everyRefresh,
                  probe: .notBuilt, fix: .notBuiltRun())
    }

    /// `open` and `form` are the seam for the creation flow the Fix starts through `TokenCreation.start` when the
    /// vault lacks the token: tests pass their own. The defaults are the production ones (Task 7 points `form`
    /// at `VaultPrompt.open(request:done:)`).
    static func issuesToken(for machine: Machine, job: SDWorkerSettings,
                            open: @escaping @Sendable (URL) -> Void = { NSWorkspace.shared.open($0) },
                            form: @escaping @Sendable (VaultRequest) -> Void = { _ in }) -> SetupStep {
        SetupStep(id: .issuesToken, title: "GitHub issues token delivered", needs: [.reachable], cadence: .everyRefresh,
                  probe: .notBuilt, fix: .notBuiltRun())
    }

    /// Every secret file of the machines.
    static func usages(in machines: [Machine]) -> [SecretUsage] { [] }

    /// Writes the vault value to the usage's file over ssh's stdin. The outcome never contains the value.
    static func deliver(_ usage: SecretUsage, ctx: FixContext) async -> FixOutcome {
        FixOutcome(ok: false, message: "not built yet")
    }

    /// A read-only check of the file's mode and size; its content is never read.
    static func verifyProbe(_ usage: SecretUsage, linux: Bool) -> Probe { .notBuilt }
}

/// Opens GitHub's token page, pre-filled, and the vault form for the value.
enum TokenCreation {
    static func pageURL(secret: String, machine: String, repos: [String], today: Date) -> URL {
        URL(string: "https://github.com/settings/personal-access-tokens/new")!
    }

    /// The clicks the user makes on that page, in order.
    static func clicks(repos: [String]) -> [String] { [] }

    static func request(secret: String, machine: String, repos: [String], today: Date) -> VaultRequest {
        VaultRequest(op: .request, name: secret)
    }

    @MainActor
    static func start(secret: String, machine: String, repos: [String], open: (URL) -> Void, form: (VaultRequest) -> Void) {}
}

extension SecretVault {
    /// Sets or clears the expiry day (`YYYY-MM-DD`) of a saved secret in the index at `url`.
    static func setExpires(_ name: String, day: String?, in url: URL = indexURL) throws {}
}

/// The vault as a `SecretSource`.
struct VaultSecretSource: SecretSource {
    init() {}
    func value(named: String) -> String? { nil }
    func entry(named: String) -> VaultEntry? { nil }
}

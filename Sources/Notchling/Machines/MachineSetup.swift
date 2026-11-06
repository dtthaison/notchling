import Foundation

/// The step model of machine setup: a step has a Check (a read-only probe) and a Fix (run only on a click).
/// This file holds the types, the constants, the catalog and `MachineRun`; `MachineSteps`, `RepoSteps`,
/// `SecretSteps`, `TokenScope` and `SDWorkerSteps` build the steps, `SetupEngine` runs them.

/// A step's id. Per-repo steps carry the repo: `clone:owner/name`.
struct StepID: Hashable, Codable {
    let raw: String

    static let reachable = StepID(raw: "reachable")
    static let workerUser = StepID(raw: "worker-user")
    static let staysAwake = StepID(raw: "stays-awake")
    static let filevault = StepID(raw: "filevault")
    static let baseTools = StepID(raw: "base-tools")
    static let claudeCode = StepID(raw: "claude-code")
    static let claudeLogin = StepID(raw: "claude-login")
    static let xcode = StepID(raw: "xcode")
    static let plugins = StepID(raw: "plugins")
    static let issuesToken = StepID(raw: "issues-token")
    static let tokenScope = StepID(raw: "token-scope")
    static let workerJSON = StepID(raw: "worker-json")
    static let service = StepID(raw: "service")
    static let sdCheck = StepID(raw: "sd-check")
    static let running = StepID(raw: "running")

    /// `github` is `owner/name`.
    static func deployKey(_ github: String) -> StepID { StepID(raw: "deploy-key:\(github)") }
    static func clone(_ github: String) -> StepID { StepID(raw: "clone:\(github)") }
    static func pushRefused(_ github: String) -> StepID { StepID(raw: "push-refused:\(github)") }
}

/// What a Check found. The text is shown as plain `Text`, never as Markdown.
enum CheckState: Equatable { case pass(String), fail(String), cantTell(String) }

/// One line under a step: a sub-check and what it found.
struct SubRow: Equatable { let name: String; let ok: Bool; let detail: String }

/// Facts a Check learned about the machine. The engine merges them into the machine and saves them. Every
/// value came from the machine and is untrusted until a step validated it.
struct Detected: Equatable {
    var os: MachineOS?
    var home: String?
    var fileVault: Bool?
    var python: String?
    var sdPath: String?
    var abilities: MachineAbilities?
    var tokenExpires: String?
}

struct CheckResult: Equatable {
    var state: CheckState
    var rows: [SubRow] = []
    var detected = Detected()
}

/// The stdout and stderr of one probe, at most 64 KiB.
struct RemoteOutput: Equatable { let text: String; let code: Int32 }

enum Probe {
    /// A read-only `sh` script for the machine. The engine batches the scripts of one machine into one ssh and
    /// hands each its own output.
    case remote(script: String, parse: @Sendable (RemoteOutput, StepContext) async -> CheckResult)
    /// A check on this Mac.
    case local(@Sendable (StepContext) async -> CheckResult)
}

enum Cadence { case everyRefresh, slow }

/// What a click on **Set up** runs. `display` is the exact commands the user sees first.
struct FixPlan {
    let display: String
    let needsAdmin: Bool
    let run: @Sendable (FixContext) async -> FixOutcome
}

enum StepFix {
    case run(FixPlan)
    /// A check-only step: text for the user instead of commands.
    case howTo(String)
    /// Running: a switch, not a Fix.
    case toggle(needsAdmin: Bool)
}

struct SetupStep {
    let id: StepID
    let title: String
    let needs: [StepID]
    let cadence: Cadence
    let probe: Probe
    let fix: StepFix
}

/// The vault as a step sees it. A value is read here and travels only on a process's stdin or in an HTTP header.
protocol SecretSource: Sendable {
    func value(named: String) -> String?
    func entry(named: String) -> VaultEntry?
}

/// `headers` has lowercased names.
struct HTTPAnswer: Equatable { let status: Int; let headers: [String: String]; let body: Data }

protocol HTTPFetching: Sendable { func get(_ url: URL, bearer: String) async -> HTTPAnswer }

struct StepContext: Sendable {
    let machine: Machine
    let all: [Machine]
    let runner: CommandRunning
    let secrets: SecretSource
    let http: HTTPFetching
    let mac: MacFacts
    let now: Date
}

struct FixContext: Sendable {
    let step: StepContext
    /// For one admin step. Never stored, never logged.
    let adminPassword: String?
    /// Output of a running Fix. It can fire from any thread, from the stdout and stderr readers at once, and for a
    /// moment after `MachineRun` returned (an orphan holding the pipe). The engine's closure must be thread-safe
    /// and keep the order: append to a lock-protected buffer or yield into one serial `AsyncStream`, never one
    /// MainActor `Task` per line. It drops lines once the Fix has ended.
    let log: @Sendable (String) -> Void
}

struct FixOutcome: Equatable { let ok: Bool; let message: String }

/// The only ways a Fix touches a machine or runs on this Mac.
enum MachineRun {
    /// A script for `/bin/sh` on the machine as the worker user. `stdin` is for a value to store; it never
    /// goes into the script.
    static func script(_ s: String, stdin: Data? = nil, ctx: FixContext, timeout: TimeInterval = 600) async -> CommandResult {
        await ctx.step.runner.run(SSH.script(ctx.step.machine, s), cwd: nil, env: [:], stdin: stdin,
                                  timeout: timeout, onOutput: ctx.log)
    }

    /// A script with admin rights. Through sudo the password is the first line of stdin and nothing else carries
    /// it; with no password the call is refused (126) before anything runs. As root over ssh there is no sudo and
    /// no stdin. The script runs with root's `~`, so it takes absolute paths (`MachinePaths.absolute`).
    static func admin(_ s: String, ctx: FixContext, timeout: TimeInterval = 900) async -> CommandResult {
        let machine = ctx.step.machine
        let asRoot = machine.admin == .rootSSH
        var stdin: Data?
        if !asRoot {
            guard let password = ctx.adminPassword else {
                return CommandResult(code: 126, out: "", err: "needs the admin password")
            }
            stdin = Data((password + "\n").utf8)
        }
        return await ctx.step.runner.run(SSH.admin(machine, s, asRoot: asRoot), cwd: nil, env: [:], stdin: stdin,
                                         timeout: timeout, onOutput: ctx.log)
    }

    /// A command on this Mac. `argv[0]` is an absolute path.
    static func local(_ argv: [String], stdin: Data? = nil, ctx: FixContext, timeout: TimeInterval = 120) async -> CommandResult {
        await ctx.step.runner.run(argv, cwd: nil, env: [:], stdin: stdin, timeout: timeout, onOutput: ctx.log)
    }
}

enum MachinePaths {
    /// Home-relative: `RemoteShell.path` expands the `~` for the worker user. Under sudo or as root `~` is
    /// someone else's home, so an admin step takes `absolute(_:home:)` with the worker's home instead.
    static let claudeEnv = "~/.config/agents/claude.env", githubToken = "~/.config/agents/github-token"
    static let workerJSON = "~/.config/sd/worker.json", marketplace = "~/.local/share/agents/system-design"
    static let claudeKey = "CLAUDE_CODE_OAUTH_TOKEN"

    static func sdPy(home: String) -> String { "\(home)/.local/share/agents/system-design/plugin/scripts/sd.py" }

    /// `path` with a leading `~` replaced by `home`; an absolute path stays as it is. Nil when `home` is not a
    /// safe path (it came from the machine), so a hostile home never reaches a script.
    static func absolute(_ path: String, home: String) -> String? {
        guard RemoteShell.isSafeMachinePath(home) else { return nil }
        if path == "~" { return home }
        if path.hasPrefix("~/") { return home + String(path.dropFirst(1)) }
        return path
    }
}

enum MachineSafety {
    /// Services on a worker machine that no step ever touches.
    static let productionServices = ["lingeva-app", "n8n", "inv3str", "postgres", "yuribot", "nginx", "caddy"]
}

extension Notification.Name {
    /// A vault value was saved or renewed. `userInfo` is `["name": String]` only, never the value.
    static let vaultValueChanged = Notification.Name("local.notchling.vaultValueChanged")
}

/// The steps of a machine, in the order they show and run.
enum StepCatalog {
    private static let macOnly: Set<StepID> = [.staysAwake, .filevault, .xcode]

    /// Machine steps (Claude login in its place after Claude Code), then, for the sd worker job, each repo's
    /// steps and the job's own (the issues token and its scope after Plugins installed). `os == nil` keeps the
    /// macOS steps; they wait for Reachable to tell.
    static func steps(for machine: Machine, all: [Machine]) -> [SetupStep] {
        var steps = MachineSteps.all(for: machine).flatMap { step in
            step.id == .claudeCode ? [step, SecretSteps.claudeLogin(for: machine)] : [step]
        }
        if let job = machine.sdWorker {
            for repo in job.repos { steps += RepoSteps.all(for: machine, repo: repo) }
            steps += SDWorkerSteps.all(for: machine, job: job, all: all).flatMap { step in
                step.id == .plugins
                    ? [step, SecretSteps.issuesToken(for: machine, job: job), TokenScope.step(for: machine, job: job)]
                    : [step]
            }
        }
        if machine.os == .linux { steps.removeAll { macOnly.contains($0.id) } }
        return steps
    }
}

/// What the group B stubs return until their task fills them in.
extension Probe {
    static var notBuilt: Probe { .local { _ in CheckResult(state: .cantTell("not built yet")) } }
}

extension StepFix {
    static func notBuiltRun(needsAdmin: Bool = false) -> StepFix {
        .run(FixPlan(display: "not built yet", needsAdmin: needsAdmin) { _ in FixOutcome(ok: false, message: "not built yet") })
    }
}

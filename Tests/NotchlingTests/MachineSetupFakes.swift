import Foundation
@testable import Notchling

/// One call a `ScriptedRunner` saw.
struct RecordedCall: Equatable {
    let argv: [String]
    let env: [String: String]
    let stdin: Data?
    let cwd: String?
}

/// A runner that starts nothing. Rules are tried in order and the first match answers; its `chunks` go to the
/// stream before the result, the way a real run's output arrives. No match gives `CommandResult(code: 0, …)`.
final class ScriptedRunner: CommandRunning, @unchecked Sendable {
    typealias Rule = (match: ([String]) -> Bool, result: CommandResult, chunks: [String])

    private let lock = NSLock()
    private var rules: [Rule]
    private var recorded: [RecordedCall] = []

    init(rules: [Rule] = []) { self.rules = rules }

    var calls: [RecordedCall] { lock.withLock { recorded } }

    /// Adds a rule after the existing ones.
    func on(_ match: @escaping ([String]) -> Bool, result: CommandResult = CommandResult(code: 0, out: "", err: ""),
            chunks: [String] = []) {
        lock.withLock { rules.append((match, result, chunks)) }
    }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: env, stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        let rule: Rule? = lock.withLock {
            recorded.append(RecordedCall(argv: argv, env: env, stdin: stdin, cwd: cwd))
            return rules.first { $0.match(argv) }
        }
        for chunk in rule?.chunks ?? [] { onOutput?(chunk) }
        return rule?.result ?? CommandResult(code: 0, out: "", err: "")
    }
}

struct FakeSecrets: SecretSource {
    var values: [String: String] = [:]
    var entries: [String: VaultEntry] = [:]
    func value(named: String) -> String? { values[named] }
    func entry(named: String) -> VaultEntry? { entries[named] }
}

/// Answers by path plus query (`/user/repos?page=2`) and records every request with its bearer.
final class FakeHTTP: HTTPFetching, @unchecked Sendable {
    private let lock = NSLock()
    private let answers: [String: HTTPAnswer]
    private var seen: [(URL, String)] = []

    init(answers: [String: HTTPAnswer] = [:]) { self.answers = answers }

    var requests: [(URL, String)] { lock.withLock { seen } }

    func get(_ url: URL, bearer: String) async -> HTTPAnswer {
        lock.withLock { seen.append((url, bearer)) }
        let key = url.path + (url.query.map { "?" + $0 } ?? "")
        return answers[key] ?? HTTPAnswer(status: 404, headers: [:], body: Data())
    }
}

/// Collects the lines a Fix logs. The log callback can fire from any thread.
final class LogBox: @unchecked Sendable {
    private let lock = NSLock()
    private var collected: [String] = []
    func add(_ text: String) { lock.withLock { collected.append(text) } }
    var lines: [String] { lock.withLock { collected } }
}

enum TestMachines {
    static let repo = "dtthaison/lingeva-mono"

    private static func machine(_ name: String, user: String, home: String, os: MachineOS, python: String,
                                kind: String) -> Machine {
        Machine(name: name, host: name, user: user, os: os,
                abilities: MachineAbilities(home: home),
                jobs: [Job(kind: .sdWorker, sdWorker: SDWorkerSettings(
                    python: python, sdPath: MachinePaths.sdPy(home: home),
                    repos: [MachineRepo(github: repo, localPath: "/Users/sondoan/go/src/pp/lingeva-mono", checkCommand: "true")],
                    kinds: [kind], issuesSecret: "\(name)-github-issues"))])
    }

    static func mac(_ name: String = "m1") -> Machine {
        machine(name, user: "sondoan", home: "/Users/sondoan", os: .macOS, python: "/opt/homebrew/bin/python3", kind: "ios")
    }

    static func linux(_ name: String = "vps-sing-1") -> Machine {
        machine(name, user: "agent", home: "/home/agent", os: .linux, python: "/usr/bin/python3", kind: "backend")
    }
}

extension MacFacts {
    static let empty = MacFacts(plugins: [:], marketplaceJSON: [:])
}

extension StepContext {
    static func test(machine: Machine = TestMachines.mac(), all: [Machine]? = nil,
                     runner: CommandRunning = ScriptedRunner(), secrets: SecretSource = FakeSecrets(),
                     http: HTTPFetching = FakeHTTP(), mac: MacFacts = .empty,
                     now: Date = Date(timeIntervalSince1970: 1_800_000_000)) -> StepContext {
        StepContext(machine: machine, all: all ?? [machine], runner: runner, secrets: secrets, http: http,
                    mac: mac, now: now)
    }
}

extension FixContext {
    static func test(_ step: StepContext = .test(), adminPassword: String? = nil,
                     log: @escaping @Sendable (String) -> Void = { _ in }) -> FixContext {
        FixContext(step: step, adminPassword: adminPassword, log: log)
    }
}

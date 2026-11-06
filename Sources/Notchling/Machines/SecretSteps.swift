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
///
/// A value is read from the vault here and goes one way: the bytes of ssh's stdin. It is never in an argv, an
/// environment, a log line, a display, a check result or a message; anything a machine says back is redacted
/// with the value first, and only a file's mode and size are ever read from the machine.
enum SecretSteps {
    /// Opens the vault form (the production `form` of both steps). Called on the main thread.
    static let openVaultForm: @Sendable (VaultRequest) -> Void = { request in
        VaultPrompt.open(request: request, done: { _ in })
    }

    static func claudeUsage(for machine: Machine) -> SecretUsage {
        SecretUsage(name: machine.claudeSecret, machine: machine.name, remotePath: MachinePaths.claudeEnv,
                    format: .keyValue(MachinePaths.claudeKey))
    }

    static func issuesUsage(for machine: Machine, job: SDWorkerSettings) -> SecretUsage {
        SecretUsage(name: job.issuesSecret, machine: machine.name, remotePath: MachinePaths.githubToken, format: .raw)
    }

    /// Step 7. When the vault lacks the token the Fix opens the vault form; otherwise it delivers.
    static func claudeLogin(for machine: Machine,
                            form: @escaping @Sendable (VaultRequest) -> Void = openVaultForm) -> SetupStep {
        let usage = claudeUsage(for: machine)
        let request = VaultRequest(
            op: .request, name: usage.name, kind: "token", url: "https://claude.ai",
            note: "Claude Code OAuth token of the separate worker account (every machine)",
            reason: "Machines: the Claude token the workers run with",
            howTo: "In your own Terminal (not an agent): run claude setup-token, sign in with the worker account in a private browser window, paste the token here, then clear the Terminal.")
        let plan = FixPlan(display: display(for: usage, machine: machine,
                                            missing: "Open the vault form for \(usage.name), if it is not in the vault yet."),
                           needsAdmin: false) { ctx in
            guard hasValue(ctx.step.secrets, usage.name) else {
                await MainActor.run { form(request) }
                return FixOutcome(ok: false, message: "Opened the form for \(usage.name). Save it, then Set up again to send it.")
            }
            return await deliver(usage, ctx: ctx)
        }
        return SetupStep(id: .claudeLogin, title: "Claude login", needs: [.reachable], cadence: .everyRefresh,
                         probe: verifyProbe(usage, linux: machine.os == .linux), fix: .run(plan))
    }

    /// `open` and `form` are the seam for the creation flow the Fix starts through `TokenCreation.start` when the
    /// vault lacks the token: tests pass their own.
    static func issuesToken(for machine: Machine, job: SDWorkerSettings,
                            open: @escaping @Sendable (URL) -> Void = { NSWorkspace.shared.open($0) },
                            form: @escaping @Sendable (VaultRequest) -> Void = openVaultForm) -> SetupStep {
        let usage = issuesUsage(for: machine, job: job)
        let repos = job.repos.map(\.github)
        let plan = FixPlan(display: display(for: usage, machine: machine,
                                            missing: "Open GitHub's token page, pre-filled, and the vault form for \(usage.name), if it is not in the vault yet."),
                           needsAdmin: false) { ctx in
            guard hasValue(ctx.step.secrets, usage.name) else {
                let now = ctx.step.now
                await MainActor.run {
                    TokenCreation.start(secret: usage.name, machine: machine.name, repos: repos, today: now,
                                        open: open, form: form)
                }
                return FixOutcome(ok: false, message: "Opened GitHub's token page and the form for \(usage.name). Create the token, save it, then Set up again to send it.")
            }
            return await deliver(usage, ctx: ctx)
        }
        return SetupStep(id: .issuesToken, title: "GitHub issues token delivered", needs: [.reachable], cadence: .everyRefresh,
                         probe: verifyProbe(usage, linux: machine.os == .linux), fix: .run(plan))
    }

    /// Every secret file of the machines: the Claude token per machine, the issues token per sd worker job.
    static func usages(in machines: [Machine]) -> [SecretUsage] {
        machines.flatMap { machine -> [SecretUsage] in
            var list = [claudeUsage(for: machine)]
            for job in machine.jobs {
                if let settings = job.sdWorker { list.append(issuesUsage(for: machine, job: settings)) }
            }
            return list
        }
    }

    // MARK: Delivery

    /// Writes the vault value to the usage's file over ssh's stdin. The outcome never contains the value.
    static func deliver(_ usage: SecretUsage, ctx: FixContext) async -> FixOutcome {
        guard usage.machine == ctx.step.machine.name else {
            return FixOutcome(ok: false, message: "\(usage.name) is for \(usage.machine), not \(ctx.step.machine.name)")
        }
        guard let value = ctx.step.secrets.value(named: usage.name), !value.isEmpty else {
            return FixOutcome(ok: false, message: "\(usage.name) is not in the vault")
        }
        guard !value.unicodeScalars.contains(where: { $0 == "\n" || $0 == "\r" || $0 == "\u{0}" }) else {
            return FixOutcome(ok: false, message: "the value of \(usage.name) has a line break; save it again")
        }
        // Redactor holds a tail back across reads only for values it can match; no real token is this short.
        guard value.utf8.count >= minimumLength else {
            return FixOutcome(ok: false, message: "the value of \(usage.name) is too short; save it again")
        }
        let text: String
        switch usage.format {
        case .raw:
            text = value
        case .keyValue(let key):
            guard key.range(of: #"^[A-Za-z_][A-Za-z0-9_]*$"#, options: .regularExpression) != nil else {
                return FixOutcome(ok: false, message: "\(key) is not a variable name")
            }
            text = "\(key)=\(value)\n"
        }
        let filter = SecretLogFilter(value: value, name: usage.name, sink: ctx.log)
        let safe = FixContext(step: ctx.step, adminPassword: nil, log: { filter.feed($0) })
        let result = await MachineRun.script(RemoteShell.writeFile(usage.remotePath), stdin: Data(text.utf8), ctx: safe)
        filter.finish()
        guard result.code == 0 else {
            let said = shown(result.err.isEmpty ? result.out : result.err, filter: filter)
            return FixOutcome(ok: false, message: filter.redact(
                "could not write \(usage.remotePath) on \(usage.machine) (code \(result.code))" + (said.isEmpty ? "" : ": \(said)")))
        }
        return FixOutcome(ok: true, message: "\(usage.name) is on \(usage.machine) at \(usage.remotePath)")
    }

    /// A read-only check of the file's mode and size; its content is never read.
    static func verifyProbe(_ usage: SecretUsage, linux: Bool) -> Probe {
        .remote(script: RemoteShell.stat([usage.remotePath], linux: linux)) { output, _ in
            guard output.code == 0 else { return CheckResult(state: .cantTell("the check didn't run")) }
            if output.text.split(separator: "\n", omittingEmptySubsequences: true).first == "missing" {
                return CheckResult(state: .fail("not on the machine"))
            }
            guard let facts = RemoteShell.parseStat(output.text, count: 1).first ?? nil else {
                return CheckResult(state: .cantTell("couldn't read the file's mode and size"))
            }
            if facts.mode != "600" { return CheckResult(state: .fail("mode \(facts.mode), should be 600")) }
            if facts.size == 0 { return CheckResult(state: .fail("empty")) }
            return CheckResult(state: .pass("mode 600, \(facts.size) bytes"))
        }
    }

    /// The shortest value delivered. Real tokens are 90 characters or more.
    private static let minimumLength = 8

    /// What a machine said, for a failure message: redacted first, then cut, so a cut can't leave a piece of the
    /// value. Output that the runner itself cut (the value's front may be gone) or that is far longer than an
    /// error is not shown.
    private static func shown(_ said: String, filter: SecretLogFilter) -> String {
        let text = said.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.hasPrefix("[earlier output cut]"), text.count <= 4000 else { return "(output not shown)" }
        return String(filter.redact(text).prefix(300))
    }

    private static func hasValue(_ secrets: SecretSource, _ name: String) -> Bool {
        !(secrets.value(named: name) ?? "").isEmpty
    }

    /// The commands the user sees before the Fix runs: the placeholder stands for the value, which only ssh's
    /// stdin carries.
    private static func display(for usage: SecretUsage, machine: Machine, missing: String) -> String {
        let source: String
        switch usage.format {
        case .raw: source = "printf %s '[secret:\(usage.name)]'"
        case .keyValue(let key): source = "printf '\(key)=%s\\n' '[secret:\(usage.name)]'"
        }
        let ssh = SSH.script(machine, RemoteShell.writeFile(usage.remotePath)).map(LaneStore.quote).joined(separator: " ")
        return "\(missing)\nOtherwise: \(source) | \(ssh)"
    }
}

/// Hides one value in a Fix's output. `Redactor` holds back the tail of a read that could start the value, so a
/// value split across two reads is still caught (deliver refuses values too short for that to hold).
private final class SecretLogFilter: @unchecked Sendable {
    private let lock = NSLock()
    private let value: String
    private let marker: String
    private let sink: @Sendable (String) -> Void
    private let redactor: Redactor

    init(value: String, name: String, sink: @escaping @Sendable (String) -> Void) {
        self.value = value
        marker = "[secret:\(name)]"
        self.sink = sink
        redactor = Redactor([value: marker])
    }

    func feed(_ chunk: String) {
        lock.lock(); defer { lock.unlock() }
        emit(redactor.feed(Data(chunk.utf8)))
    }

    /// Flushes what the redactor held back while a value might have been split across chunks.
    func finish() {
        lock.lock(); defer { lock.unlock() }
        emit(redactor.finish())
    }

    func redact(_ text: String) -> String {
        text.replacingOccurrences(of: value, with: marker)
    }

    private func emit(_ data: Data) {
        guard !data.isEmpty else { return }
        sink(String(decoding: data, as: UTF8.self))
    }
}

/// Opens GitHub's token page, pre-filled, and the vault form for the value.
enum TokenCreation {
    /// GitHub's pre-fill parameters. The link can't preset the repository choice, so `clicks` names it.
    static func pageURL(secret: String, machine: String, repos: [String], today: Date) -> URL {
        var parts = URLComponents(string: "https://github.com/settings/personal-access-tokens/new")!
        var items = [URLQueryItem(name: "name", value: secret),
                     URLQueryItem(name: "description", value: "Issues only, for the sd worker on \(machine)")]
        if let owner = repos.first?.split(separator: "/").first {
            items.append(URLQueryItem(name: "target_name", value: String(owner)))
        }
        items += [URLQueryItem(name: "expires_in", value: "366"), URLQueryItem(name: "issues", value: "write")]
        parts.queryItems = items
        return parts.url!
    }

    /// The clicks the user makes on that page, in order.
    static func clicks(repos: [String]) -> [String] {
        ["Repository access: choose Only select repositories, then pick \(repos.joined(separator: ", ")).",
         "Permissions: Issues Read and write. Leave Contents at No access.",
         "Generate token, copy it, and delete any old token for this machine on the same page.",
         "Paste it into Notchling's form and Save."]
    }

    static func request(secret: String, machine: String, repos: [String], today: Date) -> VaultRequest {
        let page = pageURL(secret: secret, machine: machine, repos: repos, today: today)
        let steps = clicks(repos: repos).enumerated().map { "\($0.offset + 1). \($0.element)" }
        let end = Calendar.current.date(byAdding: .day, value: 366, to: today) ?? today
        return VaultRequest(op: .request, name: secret, kind: "token", url: "https://github.com",
                            note: "Issues-only token for the sd worker on \(machine): \(repos.joined(separator: ", "))",
                            reason: "Machines: the GitHub issues token for \(machine)",
                            howTo: ([page.absoluteString] + steps).joined(separator: "\n"),
                            expires: VaultExpiry.day(of: end))
    }

    /// Opens the page and the form. Nothing is saved until the user presses Save in the form.
    @MainActor
    static func start(secret: String, machine: String, repos: [String], today: Date = Date(),
                      open: (URL) -> Void, form: (VaultRequest) -> Void) {
        open(pageURL(secret: secret, machine: machine, repos: repos, today: today))
        form(request(secret: secret, machine: machine, repos: repos, today: today))
    }
}

/// The vault names machine management uses, and the earlier names they replace.
enum SecretNames {
    static let legacy: [(from: String, to: String)] = [
        ("sd-worker-m1", "m1-github-issues"),
        ("sd-worker-vps-sing-1", "vps-sing-1-github-issues"),
        ("sd-claude", "claude-worker"),
    ]

    /// The saved entries still under an old name whose new name is free. A rename happens only on the user's click.
    static func pendingRenames(in entries: [VaultEntry]) -> [(VaultEntry, String)] {
        legacy.compactMap { pair in
            guard let old = entries.first(where: { $0.id == pair.from.lowercased() }),
                  !entries.contains(where: { $0.id == pair.to.lowercased() }) else { return nil }
            return (old, pair.to)
        }
    }
}

/// The vault as a `SecretSource`. The app reads the Keychain itself for a delivery, which is the user's own click;
/// an entry's "ask before each use" flag gates agents and is not consulted here.
struct VaultSecretSource: SecretSource {
    private let lookup: @Sendable (String) -> VaultEntry?
    private let read: @Sendable (VaultEntry) -> VaultValue?

    /// The defaults are the real index and the app's Keychain item; tests pass their own.
    init(entry: @escaping @Sendable (String) -> VaultEntry? = { SecretVault.entry($0) },
         value: @escaping @Sendable (VaultEntry) -> VaultValue? = { (try? VaultStore.shared.value(for: $0)) ?? nil }) {
        lookup = entry
        read = value
    }

    func value(named name: String) -> String? { lookup(name).flatMap(read)?.secret }
    func entry(named name: String) -> VaultEntry? { lookup(name) }
}

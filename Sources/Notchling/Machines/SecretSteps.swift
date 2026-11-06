import AppKit
import CryptoKit
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
        guard let text = fileText(usage.format, value: value) else {
            if case .keyValue(let key) = usage.format { return FixOutcome(ok: false, message: "\(key) is not a variable name") }
            return FixOutcome(ok: false, message: "the file for \(usage.name) can't be built")
        }
        let filter = SecretLogFilter(value: value, name: usage.name, sink: ctx.log)
        let safe = FixContext(step: ctx.step, adminPassword: nil, log: { filter.feed($0) }, mask: ctx.mask)
        let result = await MachineRun.script(RemoteShell.writeFile(usage.remotePath), stdin: Data(text.utf8), ctx: safe)
        filter.finish()
        // this value's own mask, then the engine's (every vault value of the machine), in both places
        func hide(_ text: String) -> String { ctx.mask(filter.redact(text)) }
        guard result.code == 0 else {
            let said = shown(result.err.isEmpty ? result.out : result.err, hide: hide)
            return FixOutcome(ok: false, message: hide(
                "could not write \(usage.remotePath) on \(usage.machine) (code \(result.code))" + (said.isEmpty ? "" : ": \(said)")))
        }
        return FixOutcome(ok: true, message: "\(usage.name) is on \(usage.machine) at \(usage.remotePath)")
    }

    /// The exact text `deliver` writes for `value`, or nil when the format is not a plain variable name.
    static func fileText(_ format: SecretFormat, value: String) -> String? {
        switch format {
        case .raw:
            return value
        case .keyValue(let key):
            guard key.range(of: #"^[A-Za-z_][A-Za-z0-9_]*$"#, options: .regularExpression) != nil else { return nil }
            return "\(key)=\(value)\n"
        }
    }

    /// A read-only check of the delivered file: its mode and size, and whether it is the vault's value. The
    /// machine prints the SHA-256 of the file and nothing else about its content; the app hashes the text it would
    /// send and compares the two in memory. The result says "same" or "different" and holds neither hash, and no
    /// hash is stored or logged.
    static func verifyProbe(_ usage: SecretUsage, linux: Bool) -> Probe {
        let script = RemoteShell.stat([usage.remotePath], linux: linux) + "; " + RemoteShell.sha256(usage.remotePath, linux: linux)
        return .remote(script: script) { output, ctx in
            guard output.code == 0 else { return CheckResult(state: .cantTell("the check didn't run")) }
            let lines = output.text.split(separator: "\n", omittingEmptySubsequences: true)
            if lines.first == "missing" {
                return CheckResult(state: .fail("not on the machine"))
            }
            guard let facts = RemoteShell.parseStat(output.text, count: 1).first ?? nil else {
                return CheckResult(state: .cantTell("couldn't read the file's mode and size"))
            }
            if facts.mode != "600" { return CheckResult(state: .fail("mode \(facts.mode), should be 600")) }
            if facts.size == 0 { return CheckResult(state: .fail("empty")) }
            let found = "mode 600, \(facts.size) bytes"
            // nothing in the vault to compare with: the mode and size are all there is to say
            guard let value = ctx.secrets.value(named: usage.name), !value.isEmpty,
                  let expected = fileText(usage.format, value: value) else { return CheckResult(state: .pass(found)) }
            let remote = lines.count > 1 ? lines[1].lowercased() : ""
            guard remote.count == 64, remote.allSatisfy(\.isHexDigit) else {
                return CheckResult(state: .cantTell("couldn't compare the file with the vault"))
            }
            let local = SHA256.hash(data: Data(expected.utf8)).map { String(format: "%02x", $0) }.joined()
            return local == remote
                ? CheckResult(state: .pass(found + ", same as the vault"))
                : CheckResult(state: .fail("different from the vault (Send again)"))
        }
    }

    /// The shortest value delivered. Real tokens are 90 characters or more.
    private static let minimumLength = 8

    /// What a machine said, for a failure message: masked first, then trimmed and cut, so neither can leave a piece
    /// of the value (a value with a space at its edge no longer matches once the edge is trimmed). Output that the
    /// runner itself cut (the value's front may be gone) or that is far longer than an error is not shown.
    private static func shown(_ said: String, hide: (String) -> String) -> String {
        guard !said.hasPrefix("[earlier output cut]"), said.count <= 4000 else { return "(output not shown)" }
        return String(hide(said).trimmingCharacters(in: .whitespacesAndNewlines).prefix(300))
    }

    private static func hasValue(_ secrets: SecretSource, _ name: String) -> Bool {
        !(secrets.value(named: name) ?? "").isEmpty
    }

    /// The commands the user sees before the Fix runs: the placeholder stands for the value, which only ssh's
    /// stdin carries.
    private static func display(for usage: SecretUsage, machine: Machine, missing: String) -> String {
        "\(missing)\nOtherwise: \(sendCommand(usage, machine: machine))"
    }

    /// The confirmation of Send again: the same delivery as the Fix, with the value as a placeholder.
    static func sendDisplay(for usage: SecretUsage, machine: Machine) -> String {
        "Send the vault value of \(usage.name) to \(usage.machine) again:\n\(sendCommand(usage, machine: machine))"
    }

    private static func sendCommand(_ usage: SecretUsage, machine: Machine) -> String {
        let source: String
        switch usage.format {
        case .raw: source = "printf %s '[secret:\(usage.name)]'"
        case .keyValue(let key): source = "printf '\(key)=%s\\n' '[secret:\(usage.name)]'"
        }
        let ssh = SSH.script(machine, RemoteShell.writeFile(usage.remotePath)).map(LaneStore.quote).joined(separator: " ")
        return "\(source) | \(ssh)"
    }
}

/// Hides one value in a Fix's output. `Redactor` holds back the tail of a read that could start the value, so a
/// value split across two reads is still caught (deliver refuses values too short for that to hold).
///
/// One `Redactor` per pipe (`OutputStreamTag.current`): what a redactor holds back belongs to the pipe it came from,
/// and the next read of the other pipe must not carry it on. The log (`SetupFixLog`) keeps a partial line per
/// pipe, so a held tail sent out under the other pipe's tag would split the value across two lines.
private final class SecretLogFilter: @unchecked Sendable {
    private let lock = NSLock()
    private let value: String
    private let marker: String
    private let sink: @Sendable (String) -> Void
    private var redactors: [OutputChannel: Redactor] = [:]

    init(value: String, name: String, sink: @escaping @Sendable (String) -> Void) {
        self.value = value
        marker = "[secret:\(name)]"
        self.sink = sink
    }

    private func redactor(_ channel: OutputChannel) -> Redactor {
        if let existing = redactors[channel] { return existing }
        let made = Redactor([value: marker])
        redactors[channel] = made
        return made
    }

    func feed(_ chunk: String) {
        let channel = OutputStreamTag.current
        lock.lock(); defer { lock.unlock() }
        emit(redactor(channel).feed(Data(chunk.utf8)), on: channel)
    }

    /// Flushes what each redactor held back while a value might have been split across chunks, to the pipe it
    /// came from.
    func finish() {
        lock.lock(); defer { lock.unlock() }
        for channel in [OutputChannel.stdout, .stderr] {
            if let held = redactors[channel] { emit(held.finish(), on: channel) }
        }
    }

    func redact(_ text: String) -> String {
        text.replacingOccurrences(of: value, with: marker)
    }

    /// Forwards under the pipe's own tag, so the sink (the engine's log) puts the text in that pipe's line.
    private func emit(_ data: Data, on channel: OutputChannel) {
        guard !data.isEmpty else { return }
        OutputStreamTag.with(channel) { sink(String(decoding: data, as: UTF8.self)) }
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

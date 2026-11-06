import Foundation

/// Made only by a button's action: the user's click is what lets a Fix run.
struct FixApproval {
    let adminPassword: String?
    let asRootConfirmed: Bool
}

enum AdminAnswer { case password(String), rootConfirmed, cancelled }

/// One step of the list Set up all showed: what it will run, and whether it was waiting for an earlier step.
struct ApprovedStep: Equatable {
    let id: StepID
    let title: String
    let display: String
    let waiting: Bool
}

/// Runs the checks and the Fixes.
///
/// - A **check** is read-only. `refresh` batches the `.remote` probes of a machine into one ssh per phase (the
///   cheap ones with 60 s, then the slow ones whose needs passed with 180 s); `.local` probes run up to four at
///   a time. A step whose need isn't passing says `waits for <title>` and its probe's answer is dropped.
/// - A **Fix** runs only from `runFix`, `setUpAll`, `toggleRunning` and `updateMachines`, which a button calls with
///   a `FixApproval`. Refresh, Add, opening the window and a changed vault value never reach them.
/// - **Secrets.** Every string that leaves the engine (a result, a log line, a Fix's message) goes through the
///   masks of the machine: its vault values as `[secret:<name>]`, the admin password as `[admin password]`. The
///   password lives in the Fix's context for that one step; no property of the engine holds it.
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

    private let runner: CommandRunning
    private let secrets: SecretSource
    private let http: HTTPFetching
    private let mac: () -> MacFacts
    private let machineList: () -> [Machine]
    private let save: ([Machine]) -> Bool
    private let clock: () -> Date
    private let stepList: (Machine, [Machine]) -> [SetupStep]
    private let usageList: ([Machine]) -> [SecretUsage]
    private let deliver: (SecretUsage, FixContext) async -> FixOutcome
    private let setVaultExpiry: (String, String) -> Void
    private let toggleScript: (Bool, Bool) -> String
    private var vaultObserver: NSObjectProtocol?

    /// When each machine's slow checks last ran.
    private var lastSlow: [String: Date] = [:]
    /// Facts the checks learned, by machine: laid over the registry's machine whether or not they could be saved.
    private var facts: [String: Detected] = [:]
    /// What was last saved, so unchanged facts are not saved again.
    private var savedState: [String: Machine] = [:]
    /// The token expiry last given to the vault, by machine.
    private var expirySent: [String: String] = [:]
    /// How often the offer of a secret was made; an update that finds it changed leaves the newer offer alone.
    private var offerRevision: [String: Int] = [:]

    private static let logLimit = 2000
    private static let cheapTimeout: TimeInterval = 60
    private static let slowTimeout: TimeInterval = 180
    private static let slowEvery: TimeInterval = 10 * 60
    private static let localParallel = 4
    /// The most a probe's output may be (the `head -c` in the batch wrapper).
    private static let probeCap = 65536
    private static let connectErrors = ["Could not resolve hostname", "timed out", "No route to host", "Network is unreachable"]

    /// - Parameter toggleScript: `(linux, on)` → the shell script of the Running switch (the default is
    ///   `SDWorkerSteps.toggleScript`: systemd on Linux, the LaunchAgent on a Mac, the worker's own service only).
    init(runner: CommandRunning, secrets: SecretSource, http: HTTPFetching, mac: @escaping () -> MacFacts,
         machines: @escaping () -> [Machine], save: @escaping ([Machine]) -> Bool, clock: @escaping () -> Date,
         steps: @escaping (Machine, [Machine]) -> [SetupStep] = StepCatalog.steps,
         usages: @escaping ([Machine]) -> [SecretUsage] = SecretSteps.usages,
         deliver: @escaping (SecretUsage, FixContext) async -> FixOutcome = SecretSteps.deliver,
         setVaultExpiry: @escaping (String, String) -> Void = { try? SecretVault.setExpires($0, day: $1) },
         observeVault: Bool = true,
         toggleScript: @escaping (Bool, Bool) -> String = SDWorkerSteps.toggleScript) {
        self.runner = runner
        self.secrets = secrets
        self.http = http
        self.mac = mac
        self.machineList = machines
        self.save = save
        self.clock = clock
        self.stepList = steps
        self.usageList = usages
        self.deliver = deliver
        self.setVaultExpiry = setVaultExpiry
        self.toggleScript = toggleScript
        if observeVault {
            // the notification carries the name only; the handler makes the offer and sends nothing
            vaultObserver = NotificationCenter.default.addObserver(forName: .vaultValueChanged, object: nil, queue: .main) { [weak self] note in
                guard let name = note.userInfo?["name"] as? String else { return }
                MainActor.assumeIsolated { self?.vaultValueChanged(name: name) }
            }
        }
    }

    deinit {
        if let vaultObserver { NotificationCenter.default.removeObserver(vaultObserver) }
    }

    // MARK: machines and steps

    /// The registry's machines with the facts the checks learned laid over them.
    private func currentMachines() -> [Machine] { machineList().map(withFacts) }

    private func machine(named name: String) -> Machine? { currentMachines().first { $0.name == name } }

    private func withFacts(_ m: Machine) -> Machine {
        guard let d = facts[m.name] else { return m }
        return Self.applying(d, to: m)
    }

    /// `m` with the detected facts merged in. A path from a machine counts only when it is a safe one.
    private static func applying(_ d: Detected, to m: Machine) -> Machine {
        var m = m
        if let os = d.os { m.os = os }
        if let fileVault = d.fileVault { m.fileVault = fileVault }
        if let abilities = d.abilities {
            var a = abilities
            if a.home == nil { a.home = m.abilities?.home }
            m.abilities = a
        }
        if let home = d.home, RemoteShell.isSafeMachinePath(home) {
            var a = m.abilities ?? MachineAbilities()
            a.home = home
            m.abilities = a
        }
        if var job = m.sdWorker {
            if let python = d.python, RemoteShell.isSafeMachinePath(python) { job.python = python }
            if let sdPath = d.sdPath, RemoteShell.isSafeMachinePath(sdPath) { job.sdPath = sdPath }
            if let day = d.tokenExpires { job.tokenExpires = day }
            m.sdWorker = job
        }
        return m
    }

    private static func merged(_ old: Detected?, _ new: Detected) -> Detected {
        var d = old ?? Detected()
        if let v = new.os { d.os = v }
        if let v = new.home { d.home = v }
        if let v = new.fileVault { d.fileVault = v }
        if let v = new.python { d.python = v }
        if let v = new.sdPath { d.sdPath = v }
        if let v = new.abilities { d.abilities = v }
        if let v = new.tokenExpires { d.tokenExpires = v }
        return d
    }

    func steps(for machine: Machine) -> [SetupStep] { stepList(withFacts(machine), currentMachines()) }

    /// `machine` with the facts the checks learned laid over it (what its steps are built from).
    func overlaid(_ machine: Machine) -> Machine { withFacts(machine) }

    /// The script the Running switch would run, for the confirmation that shows it first. Runs nothing.
    func toggleDisplay(on machine: String, to on: Bool) -> String? {
        guard let m = self.machine(named: machine) else { return nil }
        return toggleScript(m.os == .linux, on)
    }

    /// The vault values of the machine's usages, each hiding as `[secret:<name>]`.
    private func masks(for machine: Machine, adminPassword: String? = nil) -> SetupMasks {
        var names = Set(usageList(currentMachines()).filter { $0.machine == machine.name }.map(\.name))
        names.insert(machine.claudeSecret)
        if let issues = machine.sdWorker?.issuesSecret, !issues.isEmpty { names.insert(issues) }
        var masks = SetupMasks(adminPassword: adminPassword)
        for name in names.sorted() {
            if let value = secrets.value(named: name), !value.isEmpty { masks.vault[value] = "[secret:\(name)]" }
        }
        return masks
    }

    /// The context of one machine. A Fix gets its own machine's, never another's. Its `mask` is `masks`: the
    /// machine's vault values (and a Fix's admin password), so a step masks what a machine said before it cuts it.
    /// The caller reads the vault once (`masks(for:)`) for a refresh or a Fix and hands the result to everything
    /// in it.
    private func stepContext(_ machine: Machine, masks: SetupMasks) -> StepContext {
        StepContext(machine: machine, all: currentMachines(), runner: runner, secrets: secrets, http: http,
                    mac: mac(), now: clock(), mask: { masks.apply($0) })
    }

    // MARK: checks

    /// The outcome of checking one machine.
    private struct Evaluation {
        var results: [StepID: CheckResult] = [:]
        var detected = Detected()
        /// Whether an ssh was tried.
        var tried = false
        /// The machine did not answer ssh; `connectError` when the reason looks like a missing connection.
        var unreachable = false
        var connectError = false
        var slowRan = false
    }

    /// Checks only, never a Fix. `names` nil means every machine.
    func refresh(_ names: [String]? = nil, includeSlow: Bool = false) async {
        let targets = currentMachines().filter { names?.contains($0.name) ?? true }
        guard !targets.isEmpty else { return }
        // each machine's vault values are read once for the whole refresh: the contexts of both passes, the batch
        // cuts and the published results all use them
        var maskOf: [String: SetupMasks] = [:]
        for m in targets { maskOf[m.name] = masks(for: m) }
        var evaluations: [String: Evaluation] = [:]
        await withTaskGroup(of: (String, Evaluation).self) { group in
            for m in targets {
                let masks = maskOf[m.name] ?? SetupMasks()
                group.addTask { @MainActor [self] in
                    let due = lastSlow[m.name].map { clock().timeIntervalSince($0) > Self.slowEvery } ?? false
                    return (m.name, await evaluate(m, slow: includeSlow || due, masks: masks))
                }
            }
            for await (name, e) in group { evaluations[name] = e }
        }

        // every machine that was tried failed to connect: this Mac is probably offline
        let tried = evaluations.values.filter(\.tried)
        let offline = tried.count >= 2 && tried.allSatisfy { $0.unreachable && $0.connectError }

        var learned = false
        for m in targets {
            guard var e = evaluations[m.name] else { continue }
            if offline, e.results[.reachable] != nil {
                e.results[.reachable] = CheckResult(state: .cantTell("no machine answered; this Mac may be offline"))
            }
            let masks = maskOf[m.name] ?? SetupMasks()
            let known = Set(stepList(m, currentMachines()).map(\.id))
            var published = (results[m.name] ?? [:]).filter { known.contains($0.key) }
            for (id, r) in e.results { published[id] = masks.apply(r) }
            results[m.name] = published
            if e.slowRan { lastSlow[m.name] = clock() }
            if e.detected != Detected() {
                facts[m.name] = Self.merged(facts[m.name], e.detected)
                learned = true
            }
        }
        if learned { saveFacts(for: Set(targets.map(\.name))) }
    }

    /// For the Add sheet: the machine steps of a draft, nothing saved.
    func check(draft: Machine) async -> [StepID: CheckResult] {
        var machine = draft
        machine.jobs = []
        let masks = masks(for: machine)
        let e = await evaluate(machine, slow: false, usePrior: false, masks: masks)
        return e.results.mapValues { masks.apply($0) }
    }

    /// Writes the facts the checks learned into the registry, when they changed anything.
    private func saveFacts(for names: Set<String>) {
        var list = machineList()
        var changed = false
        for (i, raw) in list.enumerated() where names.contains(raw.name) {
            let merged = withFacts(raw)
            if let day = merged.sdWorker?.tokenExpires, let secret = merged.sdWorker?.issuesSecret, !secret.isEmpty,
               expirySent[raw.name] != day {
                expirySent[raw.name] = day
                setVaultExpiry(secret, day)
            }
            guard merged != raw, merged != savedState[raw.name] else { continue }
            list[i] = merged
            changed = true
        }
        guard changed else { return }
        if save(list) {
            saveRefused = false
            for m in list where names.contains(m.name) {
                savedState[m.name] = m
                facts[m.name] = nil          // the registry holds them now; a later edit there is not hidden by the overlay
            }
        } else {
            saveRefused = true
        }
    }

    /// Checks one machine. A fact the first pass detected (the `sdPath` of Plugins installed, a home) changes the
    /// steps that come after it: their scripts and contexts are built from the machine. So when the facts changed
    /// the machine, the steps are built again from it and checked once more; later refreshes find nothing new.
    private func evaluate(_ m: Machine, slow: Bool, usePrior: Bool = true, masks: SetupMasks) async -> Evaluation {
        let first = await evaluateOnce(m, slow: slow, usePrior: usePrior, masks: masks)
        // only what the steps are built from starts another pass (not FileVault). The token expiry is one of them:
        // worker.json's check compares the file with a `token_expires` built from it.
        let buildFacts = Detected(os: first.detected.os, home: first.detected.home, python: first.detected.python,
                                  sdPath: first.detected.sdPath, tokenExpires: first.detected.tokenExpires)
        let learned = Self.applying(buildFacts, to: m)
        guard !first.unreachable, learned != m else { return first }
        var second = await evaluateOnce(learned, slow: slow, usePrior: usePrior, masks: masks)
        second.slowRan = second.slowRan || first.slowRan
        return second
    }

    /// Runs the checks of one machine: the cheap remote probes in one ssh, then the rest in waves as their needs
    /// become known.
    private func evaluateOnce(_ m: Machine, slow: Bool, usePrior: Bool, masks: SetupMasks) async -> Evaluation {
        let list = stepList(m, currentMachines())
        let ctx = stepContext(m, masks: masks)
        let prior = usePrior ? (results[m.name] ?? [:]) : [:]
        var e = Evaluation()
        var final: [StepID: CheckResult] = [:]
        var detections: [Detected] = []

        // phase 1: every cheap remote probe, one ssh
        var raw: [StepID: CheckResult] = [:]
        let cheap = list.filter { s in
            if case .remote = s.probe, s.cadence == .everyRefresh { return true }
            return false
        }
        if !cheap.isEmpty {
            e.tried = true
            switch await runBatch(cheap, on: m, ctx: ctx, masks: masks, timeout: Self.cheapTimeout) {
            case .unreachable(let line, let connect):
                e.unreachable = true
                e.connectError = connect
                for s in list {
                    final[s.id] = s.id == .reachable
                        ? CheckResult(state: .fail(line)) : CheckResult(state: .cantTell("\(m.name) is not reachable"))
                }
                e.results = final
                return e
            case .answers(let r):
                raw = r
            }
        }

        // the rest, wave by wave: a step is ready when every need has an answer
        let ids = Set(list.map(\.id))
        let titles = Dictionary(list.map { ($0.id, $0.title) }, uniquingKeysWith: { a, _ in a })
        var pending = list

        func blockingNeed(_ s: SetupStep) -> String? {
            for need in s.needs {
                if case .pass? = final[need]?.state { continue }
                return titles[need] ?? need.raw
            }
            return nil
        }

        while !pending.isEmpty {
            let ready = pending.filter { s in s.needs.allSatisfy { final[$0] != nil || !ids.contains($0) } }
            guard !ready.isEmpty else {
                for s in pending { final[s.id] = CheckResult(state: .cantTell("waits for \(s.needs.first.map { titles[$0] ?? $0.raw } ?? "another step")")) }
                break
            }
            var slowRemote: [SetupStep] = [], locals: [SetupStep] = []
            for s in ready {
                if let need = blockingNeed(s) {
                    final[s.id] = CheckResult(state: .cantTell("waits for \(need)"))
                    continue
                }
                switch (s.probe, s.cadence) {
                case (.remote, .everyRefresh):
                    let r = raw[s.id] ?? CheckResult(state: .cantTell("no answer from the machine"))
                    final[s.id] = r
                    detections.append(r.detected)
                case (_, .slow) where !slow:
                    final[s.id] = prior[s.id] ?? CheckResult(state: .cantTell("not checked yet"))
                case (.remote, .slow):
                    slowRemote.append(s)
                case (.local, _):
                    locals.append(s)
                }
            }
            if !slowRemote.isEmpty {
                e.slowRan = true
                switch await runBatch(slowRemote, on: m, ctx: ctx, masks: masks, timeout: Self.slowTimeout) {
                case .unreachable:
                    for s in slowRemote { final[s.id] = CheckResult(state: .cantTell("\(m.name) is not reachable")) }
                case .answers(let r):
                    for s in slowRemote {
                        let result = r[s.id] ?? CheckResult(state: .cantTell("no answer from the machine"))
                        final[s.id] = result
                        detections.append(result.detected)
                    }
                }
            }
            if !locals.isEmpty {
                if locals.contains(where: { $0.cadence == .slow }) { e.slowRan = true }
                let done = await runLocals(locals, ctx: ctx)
                for s in locals {
                    let result = done[s.id] ?? CheckResult(state: .cantTell("no answer"))
                    final[s.id] = result
                    detections.append(result.detected)
                }
            }
            pending.removeAll { final[$0.id] != nil }
        }

        for d in detections { e.detected = Self.merged(e.detected, d) }
        e.results = final
        return e
    }

    private enum BatchOutcome {
        case unreachable(String, connect: Bool)
        case answers([StepID: CheckResult])
    }

    /// One ssh for the probes of `steps`. Each probe's output is cut from the rest by a marker line that carries
    /// this call's random nonce and the probe's index; anything else, whatever it looks like, is probe output.
    private func runBatch(_ steps: [SetupStep], on m: Machine, ctx: StepContext, masks: SetupMasks,
                          timeout: TimeInterval) async -> BatchOutcome {
        let nonce = (0..<16).map { _ in String("0123456789abcdef".randomElement()!) }.joined()
        var probes: [(step: SetupStep, parse: @Sendable (RemoteOutput, StepContext) async -> CheckResult)] = []
        var wrapped: [String] = []
        for s in steps {
            guard case .remote(let script, let parse) = s.probe else { continue }
            let index = probes.count
            probes.append((s, parse))
            // a shell of its own per probe: one that does not parse cannot break the others
            wrapped.append("out=$( /bin/sh -c \(LaneStore.quote(script)) </dev/null 2>&1 ); c=$?; printf '%s' \"$out\" | head -c 65536; printf '\\n@@\(nonce) \(index) %s@@\\n' \"$c\"")
        }
        let r = await runner.run(SSH.script(m, wrapped.joined(separator: "\n")), cwd: nil, timeout: timeout)

        var answers: [Int: RemoteOutput] = [:]
        var current: [String] = []
        let prefix = "@@\(nonce) "
        for line in r.out.components(separatedBy: "\n") {
            if line.hasPrefix(prefix), line.hasSuffix("@@") {
                let parts = line.dropFirst(prefix.count).dropLast(2).split(separator: " ")
                if parts.count == 2, let index = Int(parts[0]), let code = Int32(parts[1]),
                   probes.indices.contains(index), answers[index] == nil {
                    answers[index] = RemoteOutput(text: current.joined(separator: "\n"), code: code)
                    current = []
                    continue
                }
            }
            current.append(line)
        }

        // not reachable: ssh said 255, timed out, or nothing of ours came back. A probe's own exit code (even 255)
        // travels in its marker and says nothing about the machine.
        if r.code == 255 || r.code == 124 || answers.isEmpty {
            let first = r.err.split(whereSeparator: \.isNewline).first.map { String($0).trimmingCharacters(in: .whitespaces) } ?? ""
            let line: String
            switch r.code {
            case 124: line = first.isEmpty ? "timed out" : first
            case 127: line = first.isEmpty ? "ssh not found" : first
            case 0: line = first.isEmpty ? "no answer from the machine" : first
            default: line = first.isEmpty ? "ssh exited \(r.code)" : first
            }
            let connect = r.code == 124 || Self.connectErrors.contains { r.err.contains($0) }
            return .unreachable(line, connect: connect)
        }

        // output the machine (`head -c`) or ProcessRunner (2 MiB) cut ends or starts in a half line that can hold a
        // piece of a value: mask it, then drop that line
        for (i, out) in answers where out.text.hasPrefix(ProcessRunner.cutNotice) || out.text.utf8.count >= Self.probeCap {
            answers[i] = RemoteOutput(text: masks.applyToCut(out.text, headCut: out.text.hasPrefix(ProcessRunner.cutNotice),
                                                             tailCut: out.text.utf8.count >= Self.probeCap), code: out.code)
        }

        var parsed: [StepID: CheckResult] = [:]
        for (i, probe) in probes.enumerated() {
            if let out = answers[i] {
                parsed[probe.step.id] = await probe.parse(out, ctx)
            } else {
                parsed[probe.step.id] = CheckResult(state: .cantTell("no answer from the machine"))
            }
        }
        return .answers(parsed)
    }

    /// The local probes, four at a time.
    private func runLocals(_ steps: [SetupStep], ctx: StepContext) async -> [StepID: CheckResult] {
        var out: [StepID: CheckResult] = [:]
        var rest = steps[...]
        while !rest.isEmpty {
            let chunk = Array(rest.prefix(Self.localParallel))
            rest = rest.dropFirst(chunk.count)
            await withTaskGroup(of: (StepID, CheckResult).self) { group in
                for s in chunk {
                    guard case .local(let probe) = s.probe else { continue }
                    group.addTask { (s.id, await probe(ctx)) }
                }
                for await (id, r) in group { out[id] = r }
            }
        }
        return out
    }

    // MARK: Fixes

    private func appendLog(_ name: String, _ lines: [String]) {
        guard !lines.isEmpty else { return }
        logs[name] = Array(((logs[name] ?? []) + lines).suffix(Self.logLimit))
    }

    /// Why a Fix can't run now, or nil. Nothing has run when this answers.
    private func refusal(for step: SetupStep, on m: Machine, approval: FixApproval, needsAdmin: Bool, checkNeeds: Bool = true) -> String? {
        if running[m.name] != nil { return "another Fix is running on \(m.name)" }
        if checkNeeds {
            let list = stepList(m, currentMachines())
            for need in step.needs {
                if case .pass? = results[m.name]?[need]?.state { continue }
                return "waits for \(list.first { $0.id == need }?.title ?? need.raw)"
            }
        }
        if needsAdmin {
            if m.admin == .rootSSH {
                if !approval.asRootConfirmed { return "needs your confirmation to run as root on \(m.name)" }
            } else if (approval.adminPassword ?? "").isEmpty {
                return "needs the admin password"
            }
        }
        return nil
    }

    func runFix(_ id: StepID, on machine: String, approval: FixApproval) async -> FixOutcome {
        guard let m = self.machine(named: machine) else { return FixOutcome(ok: false, message: "no machine named \(machine)") }
        guard let step = stepList(m, currentMachines()).first(where: { $0.id == id }) else {
            return FixOutcome(ok: false, message: "\(machine) has no step \(id.raw)")
        }
        let plan: FixPlan
        switch step.fix {
        case .howTo(let text): return FixOutcome(ok: false, message: "\(step.title) has no Fix. \(text)")
        case .toggle: return FixOutcome(ok: false, message: "\(step.title) is a switch, not a Fix")
        case .run(let p): plan = p
        }
        if let why = refusal(for: step, on: m, approval: approval, needsAdmin: plan.needsAdmin) {
            return FixOutcome(ok: false, message: why)
        }
        // only an admin step on a sudo machine gets the password, and only for this one step
        let password = plan.needsAdmin && m.admin == .sudo ? approval.adminPassword : nil
        let outcome = await perform(step: id, title: step.title, on: m, adminPassword: password) { ctx in await plan.run(ctx) }
        await refresh([machine], includeSlow: true)
        return outcome
    }

    /// Runs `body` as the Fix in flight on `m`: logs `▸ <title>` and the outcome, streams its output into the
    /// log, and masks it all.
    private func perform(step id: StepID, title: String, on m: Machine, adminPassword: String?,
                         body: (FixContext) async -> FixOutcome) async -> FixOutcome {
        running[m.name] = id
        let masks = masks(for: m, adminPassword: adminPassword)
        appendLog(m.name, ["▸ \(title)"])

        var continuation: AsyncStream<String>.Continuation!
        let stream = AsyncStream<String> { continuation = $0 }
        let log = SetupFixLog(masks: masks, continuation: continuation)
        let name = m.name
        let drain = Task { @MainActor [weak self] in
            for await line in stream { self?.appendLog(name, [line]) }
        }

        // `mask` hides the vault values and this Fix's admin password, so a step masks before it cuts a failure line
        let ctx = FixContext(step: stepContext(m, masks: masks), adminPassword: adminPassword, log: { log.add($0) },
                             mask: { masks.apply($0) })
        let raw = await body(ctx)
        log.end()
        await drain.value

        let message = masks.apply(raw.message)
        appendLog(m.name, message.components(separatedBy: "\n").enumerated().map { $0.offset == 0 ? "\(raw.ok ? "✓" : "✗") \($0.element)" : $0.element })
        running[m.name] = nil
        return FixOutcome(ok: raw.ok, message: message)
    }

    /// What Set up all will do, for the confirmation: every step with a run Fix that is ✗ now or waits for an earlier
    /// step (it becomes ✗ once that one is fixed), in catalog order, with the commands the user sees first.
    func setUpAllPlan(for machine: String) -> [ApprovedStep] {
        guard let m = self.machine(named: machine) else { return [] }
        return stepList(m, currentMachines()).compactMap { step in
            guard case .run(let plan) = step.fix, let state = results[machine]?[step.id]?.state else { return nil }
            switch state {
            case .fail: return ApprovedStep(id: step.id, title: step.title, display: plan.display, waiting: false)
            case .cantTell(let why) where why.hasPrefix("waits for"):
                return ApprovedStep(id: step.id, title: step.title, display: plan.display, waiting: true)
            default: return nil
            }
        }
    }

    /// Runs the `approved` steps that are ✗ when their turn comes, in catalog order, and stops at the first Fix that
    /// fails (a secret step with no value yet fails with its message and stops it too). A step is looked at when its
    /// turn comes, so one that waited for an earlier Fix runs after it. It runs nothing the user was not shown: a
    /// step outside `approved` that is ✗ at its turn, or one whose commands differ from the ones shown, stops the
    /// run ("needs a look"; the user clicks Set up all again and sees the new list).
    func setUpAll(on machine: String, approved: [ApprovedStep],
                  askAdmin: @escaping (SetupStep) async -> AdminAnswer) async -> FixOutcome {
        guard let first = self.machine(named: machine) else { return FixOutcome(ok: false, message: "no machine named \(machine)") }
        if running[machine] != nil { return FixOutcome(ok: false, message: "another Fix is running on \(machine)") }
        var fixed = 0
        for id in stepList(first, currentMachines()).map(\.id) {
            // the step as it is now: an earlier Fix may have changed what this one does
            guard let m = self.machine(named: machine), let step = stepList(m, currentMachines()).first(where: { $0.id == id }),
                  case .run(let plan) = step.fix, case .fail? = results[machine]?[step.id]?.state else { continue }
            guard let shown = approved.first(where: { $0.id == step.id }), shown.display == plan.display else {
                return FixOutcome(ok: false, message: "\(step.title) needs a look; Set up again")
            }
            var approval = FixApproval(adminPassword: nil, asRootConfirmed: false)
            if plan.needsAdmin {
                switch await askAdmin(step) {
                case .cancelled: return FixOutcome(ok: false, message: "Set up all stopped before \(step.title): cancelled")
                case .password(let p): approval = FixApproval(adminPassword: p, asRootConfirmed: false)
                case .rootConfirmed: approval = FixApproval(adminPassword: nil, asRootConfirmed: true)
                }
            }
            let outcome = await runFix(step.id, on: machine, approval: approval)
            guard outcome.ok else { return FixOutcome(ok: false, message: "\(step.title): \(outcome.message)") }
            fixed += 1
        }
        return FixOutcome(ok: true, message: fixed == 0 ? "nothing to set up" : "\(fixed) step\(fixed == 1 ? "" : "s") set up")
    }

    func toggleRunning(on machine: String, to on: Bool, approval: FixApproval) async -> FixOutcome {
        guard let m = self.machine(named: machine) else { return FixOutcome(ok: false, message: "no machine named \(machine)") }
        guard let step = stepList(m, currentMachines()).first(where: { $0.id == .running }), case .toggle(let needsAdmin) = step.fix else {
            return FixOutcome(ok: false, message: "\(machine) has no Running switch")
        }
        // turning the worker off stays possible when its checks are failing
        if let why = refusal(for: step, on: m, approval: approval, needsAdmin: needsAdmin, checkNeeds: on) {
            return FixOutcome(ok: false, message: why)
        }
        let script = toggleScript(m.os == .linux, on)
        let password = needsAdmin && m.admin == .sudo ? approval.adminPassword : nil
        let outcome = await perform(step: .running, title: on ? "Turn Running on" : "Turn Running off", on: m, adminPassword: password) { ctx in
            let r = needsAdmin ? await MachineRun.admin(script, ctx: ctx) : await MachineRun.script(script, ctx: ctx)
            let text = (r.err.isEmpty ? r.out : r.err).trimmingCharacters(in: .whitespacesAndNewlines)
            if r.code == 0 { return FixOutcome(ok: true, message: on ? "Running is on" : "Running is off") }
            return FixOutcome(ok: false, message: text.isEmpty ? "exited \(r.code)" : text)
        }
        await refresh([machine])
        return outcome
    }

    // MARK: renewed secrets

    /// Makes the update offer from the name; sends nothing.
    func vaultValueChanged(name: String) {
        offerRevision[name, default: 0] += 1
        let found = usageList(currentMachines()).filter { $0.name == name }
        updateOffers[name] = found.isEmpty ? nil : found
    }

    /// Delivers the renewed value to each usage (on the click that calls this), checks that usage's step again,
    /// and clears the offer. A usage whose delivery failed stays in it.
    func updateMachines(for secret: String, approval: FixApproval) async -> [FixOutcome] {
        var outcomes: [FixOutcome] = []
        var failed: [SecretUsage] = []
        let revision = offerRevision[secret]
        for usage in updateOffers[secret] ?? [] {
            guard let m = self.machine(named: usage.machine) else {
                outcomes.append(FixOutcome(ok: false, message: "no machine named \(usage.machine)"))
                failed.append(usage)
                continue
            }
            if running[m.name] != nil {
                outcomes.append(FixOutcome(ok: false, message: "another Fix is running on \(m.name)"))
                failed.append(usage)
                continue
            }
            let isIssues = usage.name == m.sdWorker?.issuesSecret
                || (usage.name != m.claudeSecret && usage.remotePath == MachinePaths.githubToken)
            let deliver = deliver
            // `perform` builds the context from this usage's own machine
            let outcome = await perform(step: isIssues ? .issuesToken : .claudeLogin, title: "Update \(usage.name)", on: m, adminPassword: nil) { ctx in
                await deliver(usage, ctx)
            }
            // check the delivered file again; a renewed issues token also has a new scope and expiry
            await refresh([m.name], includeSlow: isIssues)
            outcomes.append(outcome)
            if !outcome.ok { failed.append(usage) }
        }
        // a value renewed while this ran has its own, newer offer
        if offerRevision[secret] == revision { updateOffers[secret] = failed.isEmpty ? nil : failed }
        return outcomes
    }
}

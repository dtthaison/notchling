import XCTest
@testable import Notchling

// MARK: fakes

/// Runs nothing. `handler` answers each call (and may stream `chunks` first, the way a real run's output arrives).
private final class EngineRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [RecordedCall] = []
    var handler: (RecordedCall) -> (CommandResult, [String]) = { _ in (CommandResult(code: 0, out: "", err: ""), []) }
    /// Chunks that arrive after `handler`'s, each tagged with its pipe the way `ProcessRunner` tags them.
    var taggedChunks: (RecordedCall) -> [(OutputChannel, String)] = { _ in [] }

    var calls: [RecordedCall] { lock.withLock { recorded } }
    /// The batched probe calls (one ssh each).
    var batches: [RecordedCall] { calls.filter { $0.argv.last?.contains("out=$( /bin/sh -c ") == true } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        let call = RecordedCall(argv: argv, env: env, stdin: stdin, cwd: cwd)
        lock.withLock { recorded.append(call) }
        let (result, chunks) = handler(call)
        for chunk in chunks { onOutput?(chunk) }
        for (channel, chunk) in taggedChunks(call) { OutputStreamTag.with(channel) { onOutput?(chunk) } }
        return result
    }
}

/// Answers a batched probe script the way the machine would: each probe's output, then its marker line.
private enum FakeSSH {
    static func batch(_ call: RecordedCall, reply: (String) -> (out: String, code: Int32)) -> CommandResult? {
        guard let script = call.argv.last, script.contains("out=$( /bin/sh -c ") else { return nil }
        let nonceRe = try! NSRegularExpression(pattern: "@@([0-9a-f]{16}) 0 %s@@")
        guard let m = nonceRe.firstMatch(in: script, range: NSRange(script.startIndex..., in: script)),
              let r = Range(m.range(at: 1), in: script) else { return nil }
        let nonce = String(script[r])
        var out = ""
        let segments = script.components(separatedBy: "out=$( /bin/sh -c ").dropFirst()
        for (i, segment) in segments.enumerated() {
            // each probe is `/bin/sh -c '<script>' </dev/null`, quoted once more inside the ssh command line
            var probe = segment.components(separatedBy: " </dev/null").first ?? ""
            let q = "'\\''"
            if probe.hasPrefix(q) { probe.removeFirst(q.count) }
            if probe.hasSuffix(q) { probe.removeLast(q.count) }
            let (text, code) = reply(probe)
            out += text + "\n@@\(nonce) \(i) \(code)@@\n"
        }
        return CommandResult(code: 0, out: out, err: "")
    }

    static func isAdmin(_ call: RecordedCall) -> Bool { call.argv.last?.contains("sudo -S") == true }
}

/// Runs what would go over ssh in a local /bin/sh instead, so a batch is parsed by a real shell.
private struct LocalShellRunner: CommandRunning {
    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        guard let remote = argv.last else { return CommandResult(code: 127, out: "", err: "") }
        return await ProcessRunner().run(["/bin/sh", "-c", remote], cwd: nil, timeout: timeout)
    }
}

private final class Counter: @unchecked Sendable {
    private let lock = NSLock()
    private var items: [String] = []
    func add(_ s: String) { lock.withLock { items.append(s) } }
    var all: [String] { lock.withLock { items } }
    var count: Int { all.count }
}

private final class Gate: @unchecked Sendable {
    private let lock = NSLock()
    private var isOpen = false
    private var waiters: [CheckedContinuation<Void, Never>] = []
    func wait() async {
        await withCheckedContinuation { c in
            lock.lock()
            if isOpen { lock.unlock(); c.resume() } else { waiters.append(c); lock.unlock() }
        }
    }
    func open() {
        lock.lock()
        isOpen = true
        let list = waiters
        waiters = []
        lock.unlock()
        list.forEach { $0.resume() }
    }
}

private final class Box<T>: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: T
    init(_ v: T) { stored = v }
    var value: T {
        get { lock.withLock { stored } }
        set { lock.withLock { stored = newValue } }
    }
}

/// The machines as the engine sees them, and what it saved.
private final class Store: @unchecked Sendable {
    private let lock = NSLock()
    private var list: [Machine]
    private(set) var saves: [[Machine]] = []
    var saveOK = true
    var applySaves = true
    init(_ machines: [Machine]) { list = machines }
    var machines: [Machine] { lock.withLock { list } }
    func save(_ new: [Machine]) -> Bool {
        lock.withLock {
            saves.append(new)
            if saveOK && applySaves { list = new }
            return saveOK
        }
    }
}

private func step(_ raw: String, title: String? = nil, needs: [String] = [], cadence: Cadence = .everyRefresh,
                  fix: StepFix = .howTo("do it by hand"),
                  parse: @escaping @Sendable (RemoteOutput, StepContext) async -> CheckResult = { out, _ in
                      out.code == 0 ? CheckResult(state: .pass(out.text)) : CheckResult(state: .fail(out.text))
                  }) -> SetupStep {
    SetupStep(id: StepID(raw: raw), title: title ?? raw.uppercased(), needs: needs.map { StepID(raw: $0) },
              cadence: cadence, probe: .remote(script: "echo probe-\(raw)", parse: parse), fix: fix)
}

private func runFix(_ counter: Counter, _ name: String, ok: Bool = true, admin: Bool = false) -> StepFix {
    .run(FixPlan(display: "do \(name)", needsAdmin: admin) { _ in
        counter.add(name)
        return FixOutcome(ok: ok, message: ok ? "did \(name)" : "could not \(name)")
    })
}

@MainActor
final class MachinesSetupEngineTests: XCTestCase {
    private let t0 = Date(timeIntervalSince1970: 1_800_000_000)

    /// Everything one test needs; `answers` maps a probe's script to its (output, code), default `ok`, 0.
    private final class World {
        let runner = EngineRunner()
        let store: Store
        let clock: Box<Date>
        let answers = Box<[String: (String, Int32)]>([:])
        let expiry = Counter()
        var engine: SetupEngine!
        init(machines: [Machine], now: Date) {
            store = Store(machines)
            clock = Box(now)
            runner.handler = { [answers] call in
                if let r = FakeSSH.batch(call, reply: { answers.value[$0].map { (out: $0.0, code: $0.1) } ?? (out: "ok", code: 0) }) {
                    return (r, [])
                }
                return (CommandResult(code: 0, out: "", err: ""), [])
            }
        }
    }

    private func world(_ machines: [Machine] = [TestMachines.mac("m1")], secrets: FakeSecrets = FakeSecrets(),
                       steps: @escaping (Machine, [Machine]) -> [SetupStep],
                       usages: @escaping ([Machine]) -> [SecretUsage] = { _ in [] },
                       deliver: @escaping (SecretUsage, FixContext) async -> FixOutcome = { _, _ in FixOutcome(ok: true, message: "") },
                       observeVault: Bool = false) -> World {
        let w = World(machines: machines, now: t0)
        let store = w.store, clock = w.clock, expiry = w.expiry
        w.engine = SetupEngine(
            runner: w.runner, secrets: secrets, http: FakeHTTP(), mac: { .empty },
            machines: { store.machines }, save: { store.save($0) }, clock: { clock.value },
            steps: steps, usages: usages, deliver: deliver,
            setVaultExpiry: { name, day in expiry.add("\(name)=\(day)") }, observeVault: observeVault)
        return w
    }

    private func admin(password: String? = nil, root: Bool = false) -> FixApproval {
        FixApproval(adminPassword: password, asRootConfirmed: root)
    }

    private func text(_ state: CheckState?) -> String? {
        switch state {
        case .pass(let s), .fail(let s), .cantTell(let s): return s
        case nil: return nil
        }
    }

    // MARK: batching

    func testOneSshPerMachineForTheCheapChecks() async {
        let w = world { _, _ in [step("a"), step("b"), step("c")] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.calls.count, 1)
        let script = w.runner.calls[0].argv.last ?? ""
        for p in ["echo probe-a", "echo probe-b", "echo probe-c"] { XCTAssertTrue(script.contains(p), p) }
        XCTAssertEqual(script.components(separatedBy: "out=$( /bin/sh -c ").count - 1, 3)
        // one nonce for the call: three marker lines, all of them the same 16 hex characters
        let re = try! NSRegularExpression(pattern: "@@([0-9a-f]{16}) \\d %s@@")
        let nonces = Set(re.matches(in: script, range: NSRange(script.startIndex..., in: script)).compactMap {
            Range($0.range(at: 1), in: script).map { String(script[$0]) }
        })
        XCTAssertEqual(nonces.count, 1)
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("ok"))
        XCTAssertEqual(w.engine.results["m1"]?.count, 3)
    }

    func testEachCallGetsAFreshNonce() async {
        let w = world { _, _ in [step("a")] }
        await w.engine.refresh(["m1"])
        await w.engine.refresh(["m1"])
        let nonces = w.runner.calls.map { call -> String in
            let s = call.argv.last ?? ""
            let r = s.range(of: "@@")!
            return String(s[r.upperBound...].prefix(16))
        }
        XCTAssertEqual(nonces.count, 2)
        XCTAssertNotEqual(nonces[0], nonces[1])
    }

    func testMarkersSplitOutputAndCodes() async {
        let w = world { _, _ in [
            step("a", parse: { out, _ in CheckResult(state: .pass("\(out.code)|\(out.text)")) }),
            step("b", parse: { out, _ in CheckResult(state: .pass("\(out.code)|\(out.text)")) }),
            step("c", parse: { out, _ in CheckResult(state: .pass("\(out.code)|\(out.text)")) }),
        ] }
        w.answers.value = ["echo probe-a": ("hello\nworld", 0), "echo probe-b": ("", 3), "echo probe-c": ("x", 127)]
        await w.engine.refresh(["m1"])
        let r = w.engine.results["m1"]
        XCTAssertEqual(r?[StepID(raw: "a")]?.state, .pass("0|hello\nworld"))
        XCTAssertEqual(r?[StepID(raw: "b")]?.state, .pass("3|"))
        XCTAssertEqual(r?[StepID(raw: "c")]?.state, .pass("127|x"))
    }

    func testAMarkerWithAnotherNonceIsJustText() async {
        let fake = "@@0123456789abcdef 1 0@@"
        let w = world { _, _ in [
            step("a", parse: { out, _ in CheckResult(state: .pass(out.text)) }),
            step("b", parse: { out, _ in CheckResult(state: .pass("\(out.code)|\(out.text)")) }),
        ] }
        w.answers.value = ["echo probe-a": ("before\n\(fake)\nafter", 0), "echo probe-b": ("mine", 5)]
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("before\n\(fake)\nafter"))
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "b")]?.state, .pass("5|mine"))
    }

    func testSlowChecksRunOnlyWhenAskedOrAfterTenMinutes() async {
        let w = world { _, _ in [step("r"), step("s", needs: ["r"], cadence: .slow)] }
        let s = StepID(raw: "s")
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.batches.count, 1, "the cheap probe only")
        XCTAssertEqual(w.engine.results["m1"]?[s]?.state, .cantTell("not checked yet"))

        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.runner.batches.count, 3, "phase 1, then the slow probes")
        XCTAssertEqual(w.engine.results["m1"]?[s]?.state, .pass("ok"))
        XCTAssertTrue((w.runner.batches.last?.argv.last ?? "").contains("echo probe-s"))
        XCTAssertFalse((w.runner.batches.last?.argv.last ?? "").contains("echo probe-r"))

        w.clock.value = t0.addingTimeInterval(9 * 60)
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.batches.count, 4, "nine minutes after the slow run: cheap probes only")
        XCTAssertEqual(w.engine.results["m1"]?[s]?.state, .pass("ok"), "the slow result stays")

        w.clock.value = t0.addingTimeInterval(11 * 60)
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.batches.count, 6, "over ten minutes: the slow probes run again")
    }

    func testASlowProbeWhoseNeedFailsIsNotRun() async {
        let w = world { _, _ in [step("r"), step("s", needs: ["r"], cadence: .slow)] }
        w.answers.value = ["echo probe-r": ("broken", 1)]
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.runner.batches.count, 1)
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "s")]?.state, .cantTell("waits for R"))
    }

    // MARK: states

    func testUnreachableMakesEveryOtherStepCantTell() async {
        let w = world { _, _ in [step("reachable", title: "Reachable"), step("a"), step("b")] }
        w.runner.handler = { _ in
            (CommandResult(code: 255, out: "", err: "ssh: connect to host m1 port 22: Operation timed out\nsecond line\n"), [])
        }
        await w.engine.refresh(["m1"])
        let r = w.engine.results["m1"]
        XCTAssertEqual(r?[.reachable]?.state, .fail("ssh: connect to host m1 port 22: Operation timed out"))
        XCTAssertEqual(r?[StepID(raw: "a")]?.state, .cantTell("m1 is not reachable"))
        XCTAssertEqual(r?[StepID(raw: "b")]?.state, .cantTell("m1 is not reachable"))
    }

    func testAllMachinesDownSaysThisMacMayBeOffline() async {
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")]) { _, _ in [step("reachable", title: "Reachable"), step("a")] }
        w.runner.handler = { call in
            let host = call.argv.contains { $0.hasSuffix("@m1") } ? "m1" : "vps"
            return (CommandResult(code: 255, out: "", err: "ssh: Could not resolve hostname \(host): nodename nor servname provided\n"), [])
        }
        await w.engine.refresh()
        for name in ["m1", "vps"] {
            XCTAssertEqual(w.engine.results[name]?[.reachable]?.state,
                           .cantTell("no machine answered; this Mac may be offline"), name)
            XCTAssertEqual(w.engine.results[name]?[StepID(raw: "a")]?.state, .cantTell("\(name) is not reachable"))
        }
    }

    func testOneMachineDownWhileAnotherAnswersIsJustThatMachine() async {
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")]) { _, _ in [step("reachable", title: "Reachable")] }
        let base = w.runner.handler
        w.runner.handler = { call in
            call.argv.contains { $0.hasSuffix("@m1") }
                ? (CommandResult(code: 255, out: "", err: "ssh: connect to host m1 port 22: No route to host\n"), [])
                : base(call)
        }
        await w.engine.refresh()
        XCTAssertEqual(w.engine.results["m1"]?[.reachable]?.state, .fail("ssh: connect to host m1 port 22: No route to host"))
        XCTAssertEqual(w.engine.results["vps"]?[.reachable]?.state, .pass("ok"))
    }

    func testTwoMachinesRefusedForAnotherReasonAreNotAnOfflineMac() async {
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")]) { _, _ in [step("reachable", title: "Reachable")] }
        w.runner.handler = { _ in (CommandResult(code: 255, out: "", err: "agent@vps: Permission denied (publickey).\n"), []) }
        await w.engine.refresh()
        XCTAssertEqual(w.engine.results["vps"]?[.reachable]?.state, .fail("agent@vps: Permission denied (publickey)."))
    }

    func testANeedThatFailsMakesTheStepWait() async {
        let localRuns = Counter()
        let w = world { _, _ in [
            step("base", title: "Base tools"),
            step("tools", title: "Tools", needs: ["base"]),
            SetupStep(id: StepID(raw: "local"), title: "Local", needs: [StepID(raw: "base")], cadence: .everyRefresh,
                      probe: .local { _ in localRuns.add("ran"); return CheckResult(state: .pass("fine")) }, fix: .howTo("x")),
            step("gone", title: "Gone", needs: ["not-in-the-list"]),
        ] }
        w.answers.value = ["echo probe-base": ("missing", 1)]
        await w.engine.refresh(["m1"])
        let r = w.engine.results["m1"]
        XCTAssertEqual(r?[StepID(raw: "base")]?.state, .fail("missing"))
        XCTAssertEqual(r?[StepID(raw: "tools")]?.state, .cantTell("waits for Base tools"))
        XCTAssertEqual(r?[StepID(raw: "local")]?.state, .cantTell("waits for Base tools"))
        XCTAssertEqual(localRuns.count, 0, "a local probe whose need fails does not run")
        XCTAssertEqual(r?[StepID(raw: "gone")]?.state, .cantTell("waits for not-in-the-list"))
    }

    func testLocalProbesRunWhenTheirNeedsPass() async {
        let w = world { _, _ in [
            step("a"),
            SetupStep(id: StepID(raw: "l"), title: "L", needs: [StepID(raw: "a")], cadence: .everyRefresh,
                      probe: .local { ctx in CheckResult(state: .pass("on \(ctx.machine.name)")) }, fix: .howTo("x")),
        ] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "l")]?.state, .pass("on m1"))
    }

    // MARK: no Fix without a click

    func testRefreshRunsNoFix() async {
        let fixes = Counter()
        let draft = Machine(name: "new", host: "new", user: "agent")
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")]) { _, _ in [
            step("a", fix: runFix(fixes, "a")), step("b", fix: runFix(fixes, "b", admin: true)),
            step("s", cadence: .slow, fix: runFix(fixes, "s")),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1), "echo probe-b": ("bad", 1), "echo probe-s": ("bad", 1)]
        await w.engine.refresh(includeSlow: true)
        _ = await w.engine.check(draft: draft)
        XCTAssertEqual(fixes.count, 0)
        XCTAssertEqual(w.runner.calls.filter { !(($0.argv.last ?? "").contains("out=$( /bin/sh -c ")) }.count, 0, "only probe calls")
    }

    func testCheckDraftRunsTheMachineStepsAndSavesNothing() async {
        let seen = Box<[Machine]>([])
        let w = world { m, _ in seen.value.append(m); return [step("a"), step("b")] }
        var draft = Machine(name: "new", host: "new.tail", user: "agent")
        draft.jobs = [Job(kind: .sdWorker, sdWorker: SDWorkerSettings(python: "python3", sdPath: "", repos: [], kinds: [], issuesSecret: "x"))]
        let r = await w.engine.check(draft: draft)
        XCTAssertEqual(r[StepID(raw: "a")]?.state, .pass("ok"))
        XCTAssertEqual(r.count, 2)
        XCTAssertEqual(w.runner.batches.count, 1)
        XCTAssertTrue(w.runner.batches[0].argv.contains("agent@new.tail"))
        XCTAssertTrue(seen.value.allSatisfy { $0.jobs.isEmpty }, "the Add sheet checks the machine steps only")
        XCTAssertTrue(w.store.saves.isEmpty)
        XCTAssertTrue(w.engine.results.isEmpty)
    }

    func testAVaultChangeOnlyOffersTheUpdate() async {
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")], steps: { _, _ in [step("a")] }, usages: { machines in
            machines.map { SecretUsage(name: "claude-worker", machine: $0.name, remotePath: MachinePaths.claudeEnv,
                                       format: .keyValue(MachinePaths.claudeKey)) }
                + [SecretUsage(name: "m1-github-issues", machine: "m1", remotePath: MachinePaths.githubToken, format: .raw)]
        })
        w.engine.vaultValueChanged(name: "claude-worker")
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.count, 2)
        XCTAssertNil(w.engine.updateOffers["m1-github-issues"])
        XCTAssertEqual(w.runner.calls.count, 0)
        w.engine.vaultValueChanged(name: "unused-name")
        XCTAssertNil(w.engine.updateOffers["unused-name"])
    }

    func testTheVaultNotificationMakesTheOffer() async {
        let w = world(steps: { _, _ in [step("a")] }, usages: { _ in
            [SecretUsage(name: "claude-worker", machine: "m1", remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey))]
        }, observeVault: true)
        NotificationCenter.default.post(name: .vaultValueChanged, object: nil, userInfo: ["name": "claude-worker"])
        for _ in 0..<100 where w.engine.updateOffers["claude-worker"] == nil { try? await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.count, 1)
        XCTAssertEqual(w.runner.calls.count, 0)
    }

    // MARK: runFix

    func testRunFixRunsTheFixLogsItAndChecksAgain() async {
        let fixes = Counter()
        let w = world { _, _ in [step("a", title: "Thing", fix: runFix(fixes, "a"))] }
        w.answers.value = ["echo probe-a": ("bad", 1)]
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "a")]?.state, .fail("bad"))
        w.answers.value = [:]
        let out = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        XCTAssertEqual(out, FixOutcome(ok: true, message: "did a"))
        XCTAssertEqual(fixes.all, ["a"])
        XCTAssertEqual(w.engine.logs["m1"], ["▸ Thing", "✓ did a"])
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("ok"), "checked again after the Fix")
        XCTAssertNil(w.engine.running["m1"])
    }

    func testRunFixRefusesWhileAnotherRunsOnTheMachine() async {
        let gate = Gate()
        let slow = StepFix.run(FixPlan(display: "slow", needsAdmin: false) { _ in
            await gate.wait()
            return FixOutcome(ok: true, message: "slow done")
        })
        let fixes = Counter()
        let w = world { _, _ in [step("a", fix: slow), step("b", fix: runFix(fixes, "b"))] }
        let first = Task { await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: self.admin()) }
        while w.engine.running["m1"] == nil { await Task.yield() }
        XCTAssertEqual(w.engine.running["m1"], StepID(raw: "a"))
        let second = await w.engine.runFix(StepID(raw: "b"), on: "m1", approval: admin())
        XCTAssertFalse(second.ok)
        XCTAssertEqual(fixes.count, 0)
        gate.open()
        let done = await first.value
        XCTAssertTrue(done.ok)
        XCTAssertNil(w.engine.running["m1"])
    }

    func testRunFixRefusesWhenANeedFails() async {
        let fixes = Counter()
        let w = world { _, _ in [step("a", title: "Alpha"), step("b", needs: ["a"], fix: runFix(fixes, "b"))] }
        w.answers.value = ["echo probe-a": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.runFix(StepID(raw: "b"), on: "m1", approval: admin())
        XCTAssertEqual(out, FixOutcome(ok: false, message: "waits for Alpha"))
        XCTAssertEqual(fixes.count, 0)
    }

    func testRunFixRefusesHowToAndToggleStepsAndUnknownOnes() async {
        let w = world { _, _ in [
            step("h", fix: .howTo("Do it by hand.")),
            step("t", fix: .toggle(needsAdmin: false)),
        ] }
        await w.engine.refresh(["m1"])
        let how = await w.engine.runFix(StepID(raw: "h"), on: "m1", approval: admin())
        let toggle = await w.engine.runFix(StepID(raw: "t"), on: "m1", approval: admin())
        let unknown = await w.engine.runFix(StepID(raw: "zzz"), on: "m1", approval: admin())
        let machine = await w.engine.runFix(StepID(raw: "h"), on: "nope", approval: admin())
        XCTAssertFalse(how.ok); XCTAssertTrue(how.message.contains("Do it by hand."))
        XCTAssertFalse(toggle.ok)
        XCTAssertFalse(unknown.ok)
        XCTAssertFalse(machine.ok)
        XCTAssertEqual(w.runner.calls.count, 1, "only the refresh's probe call")
    }

    // MARK: admin

    private func adminStep(_ raw: String = "adm", root: Bool = false) -> SetupStep {
        step(raw, fix: .run(FixPlan(display: "sudo thing", needsAdmin: true) { ctx in
            let r = await MachineRun.admin("echo installing", ctx: ctx)
            return FixOutcome(ok: r.code == 0, message: "machine said: \(r.out)")
        }))
    }

    /// The fake machine behind sudo echoes whatever came on stdin.
    private func echoStdin(_ w: World, chunks: [String]? = nil) {
        let probe = w.runner.handler
        w.runner.handler = { call in
            if FakeSSH.isAdmin(call) || call.argv.last?.contains("root") == true {
                let echoed = call.stdin.map { String(decoding: $0, as: UTF8.self) } ?? ""
                return (CommandResult(code: 0, out: echoed, err: ""), chunks ?? [echoed])
            }
            return probe(call)
        }
    }

    func testAdminPasswordGoesOnlyToStdinAndIsMasked() async {
        let pw = "hunter2-Pw!"
        let w = world { _, _ in [self.adminStep()] }
        echoStdin(w, chunks: ["hunter2-", "Pw!\n", "done\n"])
        let out = await w.engine.runFix(StepID(raw: "adm"), on: "m1", approval: admin(password: pw))

        XCTAssertTrue(out.ok)
        let adminCalls = w.runner.calls.filter(FakeSSH.isAdmin)
        XCTAssertEqual(adminCalls.count, 1)
        XCTAssertEqual(adminCalls[0].stdin, Data((pw + "\n").utf8), "the password is sudo's stdin")
        for call in w.runner.calls {
            XCTAssertFalse(call.argv.joined(separator: " ").contains(pw))
            XCTAssertFalse(call.env.values.joined().contains(pw))
        }
        let log = (w.engine.logs["m1"] ?? []).joined(separator: "\n")
        XCTAssertTrue(log.contains("[admin password]"), log)
        XCTAssertFalse(log.contains(pw), log)
        XCTAssertFalse(log.contains("hunter2"), "a value cut between two chunks is still masked")
        XCTAssertFalse(out.message.contains(pw))
        XCTAssertTrue(out.message.contains("[admin password]"))
        XCTAssertFalse(String(describing: w.engine.results).contains(pw))
        XCTAssertFalse(String(describing: w.engine.updateOffers).contains(pw))
        let engine: SetupEngine = w.engine
        for child in Mirror(reflecting: engine).children {
            XCTAssertFalse(String(reflecting: child.value).contains(pw), "\(child.label ?? "?") holds the password")
        }
    }

    func testAShortAdminPasswordIsMaskedToo() async {
        let w = world { _, _ in [self.adminStep()] }
        echoStdin(w)
        let out = await w.engine.runFix(StepID(raw: "adm"), on: "m1", approval: admin(password: "ab"))
        XCTAssertTrue(out.message.contains("[admin password]"), out.message)
        XCTAssertFalse((w.engine.logs["m1"] ?? []).joined().contains("ab\n"))
    }

    func testAdminStepRefusesWithoutAPassword() async {
        let w = world { _, _ in [self.adminStep()] }
        for approval in [admin(), admin(password: ""), admin(root: true)] {
            let out = await w.engine.runFix(StepID(raw: "adm"), on: "m1", approval: approval)
            XCTAssertFalse(out.ok)
            XCTAssertTrue(out.message.contains("admin password"), out.message)
        }
        XCTAssertEqual(w.runner.calls.count, 0, "refused before any command ran")
        XCTAssertNil(w.engine.logs["m1"])
    }

    func testRootSSHNeedsConfirmationNotAPassword() async {
        var m = TestMachines.linux("vps")
        m.admin = .rootSSH
        let w = world([m]) { _, _ in [self.adminStep()] }
        echoStdin(w)
        let refused = await w.engine.runFix(StepID(raw: "adm"), on: "vps", approval: admin(password: "hunter2-Pw!"))
        XCTAssertFalse(refused.ok)
        XCTAssertEqual(w.runner.calls.count, 0)
        let out = await w.engine.runFix(StepID(raw: "adm"), on: "vps", approval: admin(root: true))
        XCTAssertTrue(out.ok, out.message)
        let call = w.runner.calls.first { $0.argv.contains("root@vps") }
        XCTAssertNotNil(call, "ssh as root")
        XCTAssertNil(call?.stdin)
        XCTAssertFalse(w.runner.calls.contains { FakeSSH.isAdmin($0) })
    }

    func testANonAdminStepNeverGetsThePassword() async {
        let seen = Box<String?>("untouched")
        let fix = StepFix.run(FixPlan(display: "plain", needsAdmin: false) { ctx in
            seen.value = ctx.adminPassword
            return FixOutcome(ok: true, message: "ok")
        })
        let w = world { _, _ in [step("p", fix: fix)] }
        _ = await w.engine.runFix(StepID(raw: "p"), on: "m1", approval: admin(password: "hunter2-Pw!"))
        XCTAssertNil(seen.value)
    }

    // MARK: redaction

    func testLogAndResultsRedactEveryValueOfTheMachine() async {
        let claude = "sk-ant-oat01-XYZ0", issues = "github_pat_ABC0"
        let secrets = FakeSecrets(values: ["claude-worker": claude, "m1-github-issues": issues])
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { _ in
            FixOutcome(ok: false, message: "failed with \(claude) and \(issues)")
        })
        let w = world(secrets: secrets) { _, _ in [
            step("a", fix: fix, parse: { out, _ in
                CheckResult(state: .fail("token \(claude) refused"), rows: [SubRow(name: "row \(issues)", ok: false, detail: "saw \(claude)")])
            }),
        ] }
        let probe = w.runner.handler
        w.runner.handler = { call in
            if FakeSSH.batch(call, reply: { _ in ("", 0) }) != nil { return probe(call) }
            return (CommandResult(code: 0, out: "", err: ""), [])
        }
        await w.engine.refresh(["m1"])
        let result = w.engine.results["m1"]?[StepID(raw: "a")]
        XCTAssertEqual(result?.state, .fail("token [secret:claude-worker] refused"))
        XCTAssertEqual(result?.rows, [SubRow(name: "row [secret:m1-github-issues]", ok: false, detail: "saw [secret:claude-worker]")])

        // a Fix that streams the values back, one cut between two chunks
        let streaming = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
            ctx.log("using \(issues)\nand sk-ant-oat")
            ctx.log("01-XYZ0 here\n")
            return FixOutcome(ok: false, message: "failed with \(claude) and \(issues)")
        })
        let w2 = world(secrets: secrets) { _, _ in [step("a", fix: streaming)] }
        let out = await w2.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        XCTAssertEqual(out.message, "failed with [secret:claude-worker] and [secret:m1-github-issues]")
        let log = w2.engine.logs["m1"] ?? []
        XCTAssertEqual(log, ["▸ A", "using [secret:m1-github-issues]", "and [secret:claude-worker] here",
                             "✗ failed with [secret:claude-worker] and [secret:m1-github-issues]"])
        XCTAssertFalse(log.joined().contains(claude))
        XCTAssertFalse(log.joined().contains(issues))
    }

    /// Redact first, cut afterwards: a cut through a token leaves a fragment that no longer matches the value.
    private let longToken = "sk-ant-oat01-" + String((0..<95).map { i in
        Array("ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789-_")[(i * 7 + i / 3) % 57]
    })

    private func noFragment(of value: String, in text: String, file: StaticString = #filePath, line: UInt = #line) {
        let chars = Array(value)
        for i in 0...(chars.count - 8) {
            XCTAssertFalse(text.contains(String(chars[i..<(i + 8)])), "a fragment of the value shows at \(i)", file: file, line: line)
        }
    }

    func testALongLineIsMaskedBeforeItIsCut() async {
        XCTAssertGreaterThanOrEqual(longToken.count, 100)
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        for filler in [4096 - 40, 4096 - 100, 4096 - 3, 4090, 250] {
            let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
                // no newline until the end, so the line is let out in pieces; the token arrives in two chunks
                ctx.log(String(repeating: "f", count: filler) + String(self.longToken.prefix(50)))
                ctx.log(String(self.longToken.dropFirst(50)) + String(repeating: "g", count: 300) + "\n")
                // and the whole token inside one long chunk, with text after it
                ctx.log(String(repeating: "h", count: filler) + self.longToken + String(repeating: "i", count: 30))
                ctx.log(String(repeating: "j", count: 4100) + "\n")
                return FixOutcome(ok: true, message: String(repeating: "m", count: 250) + self.longToken)
            })
            let w = world(secrets: secrets) { _, _ in [step("a", fix: fix)] }
            let out = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
            let log = (w.engine.logs["m1"] ?? []).joined(separator: "\n")
            noFragment(of: longToken, in: log)
            noFragment(of: longToken, in: out.message)
            XCTAssertTrue(log.contains("[secret:claude-worker]"), "filler \(filler)")
        }
    }

    func testACheckReasonWithALongTokenShowsNoFragment() async {
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        let w = world(secrets: secrets) { _, _ in [
            step("a", parse: { _, _ in
                CheckResult(state: .fail(String(repeating: "f", count: 250) + self.longToken),
                            rows: [SubRow(name: "n", ok: false, detail: String(repeating: "f", count: 250) + self.longToken)])
            }),
        ] }
        await w.engine.refresh(["m1"])
        let r = w.engine.results["m1"]?[StepID(raw: "a")]
        noFragment(of: longToken, in: String(describing: r))
        XCTAssertTrue(String(describing: r).contains("[secret:claude-worker]"))
    }

    // MARK: fix round 1

    /// I1: stdout and stderr each keep their own partial line, so a value cut between two stdout chunks is whole
    /// again even when stderr wrote in between.
    func testARunnerTellsWhichPipeEachChunkCameFrom() async {
        let seen = Box<[String]>([])
        let r = await ProcessRunner().run(
            ["/bin/sh", "-c", "printf OUT1; sleep 0.3; printf ERR1 >&2; sleep 0.3; printf OUT2"],
            cwd: nil, env: [:], stdin: nil, timeout: 10,
            onOutput: { text in seen.value.append("\(OutputStreamTag.current == .stderr ? "err" : "out"):\(text)") })
        XCTAssertEqual(r.code, 0)
        XCTAssertEqual(seen.value, ["out:OUT1", "err:ERR1", "out:OUT2"])
        XCTAssertEqual(OutputStreamTag.current, .stdout, "the tag is not left set on the calling thread")
    }

    func testAValueSplitOnOnePipeIsMaskedWhenTheOtherPipeWritesBetween() async {
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        let half = longToken.count / 2
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
            OutputStreamTag.with(.stdout) { ctx.log("using " + String(self.longToken.prefix(half))) }
            OutputStreamTag.with(.stderr) { ctx.log("warning: something\n") }
            OutputStreamTag.with(.stdout) { ctx.log(String(self.longToken.dropFirst(half)) + " done\n") }
            OutputStreamTag.with(.stderr) { ctx.log("tail without newline") }
            return FixOutcome(ok: true, message: "ok")
        })
        let w = world(secrets: secrets) { _, _ in [step("a", fix: fix)] }
        _ = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        let log = w.engine.logs["m1"] ?? []
        noFragment(of: longToken, in: log.joined(separator: "\n"))
        XCTAssertTrue(log.contains("using [secret:claude-worker] done"), "\(log)")
        XCTAssertTrue(log.contains("warning: something"))
        XCTAssertTrue(log.contains("tail without newline"), "an unfinished line of either pipe is flushed at the end")
    }

    /// M1: output cut by the remote `head -c 65536` ends in a partial line that can hold the start of a value.
    func testATailCutProbeDropsItsPartialLastLine() async {
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        let seen = Box<[String]>([])
        let w = world(secrets: secrets) { _, _ in [
            step("a", parse: { out, _ in seen.value.append(out.text); return CheckResult(state: .pass("ok")) }),
        ] }
        let partial = "tail " + String(longToken.prefix(50))
        let body = String(repeating: "f", count: 65536 - partial.utf8.count - 1) + "\n"
        w.answers.value = ["echo probe-a": (body + partial, 0)]
        XCTAssertEqual((body + partial).utf8.count, 65536)
        await w.engine.refresh(["m1"])
        let text = seen.value.last ?? ""
        noFragment(of: longToken, in: text)
        XCTAssertTrue(text.hasPrefix("fff"), "the lines before the cut edge stay")
        XCTAssertFalse(text.contains("tail "))
    }

    func testAShortProbeOutputKeepsItsLastLine() async {
        let seen = Box<[String]>([])
        let w = world { _, _ in [step("a", parse: { out, _ in seen.value.append(out.text); return CheckResult(state: .pass("ok")) })] }
        w.answers.value = ["echo probe-a": ("first\nlast", 0)]
        await w.engine.refresh(["m1"])
        XCTAssertEqual(seen.value, ["first\nlast"])
    }

    /// M1: ProcessRunner's 2 MiB cut keeps the end and puts a notice first; the first line after it starts mid-line.
    func testAHeadCutOutputDropsTheLineAfterTheNotice() async {
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        let seen = Box<[String]>([])
        let w = world(secrets: secrets) { _, _ in [
            step("a", parse: { out, _ in seen.value.append(out.text); return CheckResult(state: .pass("ok")) }),
        ] }
        let base = w.runner.handler
        w.runner.handler = { call in
            guard let r = FakeSSH.batch(call, reply: { _ in (String(self.longToken.suffix(60)) + " rest of the cut line\nreal line", 0) }) else { return base(call) }
            return (CommandResult(code: 0, out: "[earlier output cut]\n" + r.out, err: ""), [])
        }
        await w.engine.refresh(["m1"])
        let text = seen.value.last ?? ""
        noFragment(of: longToken, in: text)
        XCTAssertEqual(text, "[earlier output cut]\nreal line", "the notice stays (Task 5 reads it as can't tell)")
    }

    /// I2: each probe runs in a shell of its own, so one probe that does not parse cannot break the others.
    func testAProbeThatDoesNotParseBreaksNoOtherProbe() async {
        let seen = Box<[String: String]>([:])
        func probe(_ id: String, _ script: String) -> SetupStep {
            SetupStep(id: id == "reachable" ? .reachable : StepID(raw: id), title: id, needs: [], cadence: .everyRefresh,
                      probe: .remote(script: script, parse: { out, _ in
                          seen.value[id] = "\(out.code)|\(out.text)"
                          return out.code == 0 ? CheckResult(state: .pass(out.text)) : CheckResult(state: .fail(out.text))
                      }), fix: .howTo("x"))
        }
        let list = [
            probe("reachable", "true"),
            probe("one", "echo one; exit 3"),
            // bash 3.2 (macOS /bin/sh) cannot parse this unparenthesized case inside `$( ( … ) )`
            probe("case", "case x in x) echo from-case;; esac"),
            probe("broken", "if then fi"),
            probe("two", "echo \"two $((1+1))\""),
        ]
        let engine = SetupEngine(runner: LocalShellRunner(), secrets: FakeSecrets(), http: FakeHTTP(), mac: { .empty },
                                 machines: { [TestMachines.mac("m1")] }, save: { _ in true }, clock: { self.t0 },
                                 steps: { _, _ in list }, observeVault: false)
        await engine.refresh(["m1"])
        XCTAssertEqual(seen.value["one"], "3|one")
        XCTAssertEqual(seen.value["case"], "0|from-case")
        XCTAssertEqual(seen.value["two"], "0|two 2")
        XCTAssertTrue(seen.value["broken"]?.hasPrefix("2|") == true, seen.value["broken"] ?? "nil")
        XCTAssertEqual(engine.results["m1"]?[.reachable]?.state, .pass(""), "Reachable is not failed by another probe")
        XCTAssertEqual(engine.results["m1"]?[StepID(raw: "two")]?.state, .pass("two 2"))
    }

    func testAProbeThatExits255ItselfIsNotAnUnreachableMachine() async {
        let w = world { _, _ in [step("reachable", title: "Reachable"), step("a")] }
        w.answers.value = ["echo probe-a": ("it said 255", 255)]
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[.reachable]?.state, .pass("ok"))
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "a")]?.state, .fail("it said 255"))
    }

    func testNoMarkerAtAllFailsReachable() async {
        let w = world { _, _ in [step("reachable", title: "Reachable"), step("a")] }
        w.runner.handler = { _ in (CommandResult(code: 0, out: "Welcome banner only\n", err: ""), []) }
        await w.engine.refresh(["m1"])
        if case .fail? = w.engine.results["m1"]?[.reachable]?.state {} else {
            XCTFail("Reachable: \(String(describing: w.engine.results["m1"]?[.reachable]))")
        }
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "a")]?.state, .cantTell("m1 is not reachable"))
    }

    /// M2: only the facts steps are built from (os, home, python, sdPath) start a second pass.
    func testFactsNoStepIsBuiltFromDoNotRepeatTheChecks() async {
        let w = world { _, _ in [
            step("reachable", parse: { _, _ in
                CheckResult(state: .pass("up"), detected: Detected(fileVault: true, tokenExpires: "2027-02-01"))
            }),
        ] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.batches.count, 1)
        XCTAssertTrue(w.store.saves.last?[0].fileVault == true)
    }

    /// M4: a renewed value that arrives while the update runs keeps its own offer.
    func testAValueRenewedDuringTheUpdateKeepsItsOffer() async {
        let holder = Box<SetupEngine?>(nil)
        let usage = SecretUsage(name: "claude-worker", machine: "m1", remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey))
        let w = world(steps: { _, _ in [step("claude-login")] }, usages: { _ in [usage] }, deliver: { _, _ in
            await MainActor.run { holder.value?.vaultValueChanged(name: "claude-worker") }
            return FixOutcome(ok: true, message: "stored the old value")
        })
        holder.value = w.engine
        w.engine.vaultValueChanged(name: "claude-worker")
        _ = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(w.engine.updateOffers["claude-worker"], [usage], "the newer value still has to be sent")
    }

    /// M5: setUpAll looks at each step as it is when its turn comes, not as it was when the run started.
    func testSetUpAllUsesTheStepAsItIsAtItsTurn() async {
        let fixes = Counter()
        let flipped = Box(false)
        let fixA = StepFix.run(FixPlan(display: "a", needsAdmin: false) { _ in
            flipped.value = true
            fixes.add("a")
            return FixOutcome(ok: true, message: "did a")
        })
        let w = world { _, _ in [
            step("a", fix: fixA),
            step("b", fix: flipped.value ? runFix(fixes, "b") : .howTo("by hand")),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1), "echo probe-b": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1") { _ in .cancelled }
        XCTAssertTrue(out.ok, out.message)
        XCTAssertEqual(fixes.all, ["a", "b"])
    }

    func testTheLogKeepsOrderAndDropsLinesAfterTheFixEnded() async {
        let late = Box<(@Sendable (String) -> Void)?>(nil)
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
            late.value = ctx.log
            await withTaskGroup(of: Void.self) { group in
                group.addTask { for i in 0..<200 { ctx.log("out \(i)\n") } }
                group.addTask { for i in 0..<200 { ctx.log("err \(i)\n") } }
            }
            ctx.log("partial with no newline")
            return FixOutcome(ok: true, message: "done")
        })
        let w = world { _, _ in [step("a", fix: fix)] }
        _ = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        let log = w.engine.logs["m1"] ?? []
        XCTAssertEqual(log.first, "▸ A")
        XCTAssertEqual(log.last, "✓ done")
        XCTAssertEqual(log[log.count - 2], "partial with no newline", "an unfinished line is flushed when the Fix ends")
        let outs = log.filter { $0.hasPrefix("out ") }.compactMap { Int($0.dropFirst(4)) }
        let errs = log.filter { $0.hasPrefix("err ") }.compactMap { Int($0.dropFirst(4)) }
        XCTAssertEqual(outs, Array(0..<200), "each stream's order is kept")
        XCTAssertEqual(errs, Array(0..<200))
        let before = w.engine.logs["m1"]
        late.value?("after the end\n")
        try? await Task.sleep(nanoseconds: 50_000_000)
        XCTAssertEqual(w.engine.logs["m1"], before, "a line after the Fix ended is dropped")
    }

    func testTheLogKeepsTheLast2000Lines() async {
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
            ctx.log((0..<2500).map { "line \($0)\n" }.joined())
            return FixOutcome(ok: true, message: "done")
        })
        let w = world { _, _ in [step("a", fix: fix)] }
        _ = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        let log = w.engine.logs["m1"] ?? []
        XCTAssertEqual(log.count, 2000)
        XCTAssertEqual(log.last, "✓ done")
        XCTAssertEqual(log[log.count - 2], "line 2499")
    }

    // MARK: set up all

    func testSetUpAllRunsFailedStepsInOrderAndStopsAtTheFirstFailure() async {
        let fixes = Counter()
        let w = world { _, _ in [
            step("a", fix: runFix(fixes, "a")), step("b", fix: runFix(fixes, "b")),
            step("c", fix: runFix(fixes, "c", ok: false)), step("d", fix: runFix(fixes, "d")),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1), "echo probe-c": ("bad", 1), "echo probe-d": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1") { _ in .cancelled }
        XCTAssertEqual(fixes.all, ["a", "c"])
        XCTAssertFalse(out.ok)
        XCTAssertTrue(out.message.contains("C"), out.message)
        XCTAssertTrue(out.message.contains("could not c"), out.message)
    }

    func testSetUpAllContinuesWithAStepThatWasWaitingOnTheOneItFixed() async {
        let fixes = Counter()
        let flip = Box(false)
        let fixA = StepFix.run(FixPlan(display: "a", needsAdmin: false) { _ in
            fixes.add("a"); flip.value = true
            return FixOutcome(ok: true, message: "did a")
        })
        let w = world { _, _ in [step("a", fix: fixA), step("b", needs: ["a"], fix: runFix(fixes, "b"))] }
        // a passes once its Fix ran; b keeps failing until its own Fix
        w.runner.handler = { call in
            let r = FakeSSH.batch(call, reply: { $0.contains("probe-a") ? (flip.value ? ("ok", 0) : ("bad", 1)) : ("bad", 1) })
            return (r ?? CommandResult(code: 0, out: "", err: ""), [])
        }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "b")]?.state, .cantTell("waits for A"))
        let out = await w.engine.setUpAll(on: "m1") { _ in .cancelled }
        XCTAssertTrue(out.ok, out.message)
        XCTAssertEqual(fixes.all, ["a", "b"])
    }

    func testSetUpAllSkipsHowToAndToggleSteps() async {
        let fixes = Counter()
        let w = world { _, _ in [
            step("h", fix: .howTo("by hand")), step("t", fix: .toggle(needsAdmin: false)), step("r", fix: runFix(fixes, "r")),
        ] }
        w.answers.value = ["echo probe-h": ("bad", 1), "echo probe-t": ("bad", 1), "echo probe-r": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1") { _ in .cancelled }
        XCTAssertEqual(fixes.all, ["r"])
        XCTAssertTrue(out.ok)
    }

    func testSetUpAllAsksForEachAdminStep() async {
        let fixes = Counter()
        let asked = Counter()
        let w = world { _, _ in [
            step("a1", title: "Admin one", fix: runFix(fixes, "a1", admin: true)),
            step("p", fix: runFix(fixes, "p")),
            step("a2", title: "Admin two", fix: runFix(fixes, "a2", admin: true)),
        ] }
        w.answers.value = ["echo probe-a1": ("bad", 1), "echo probe-p": ("bad", 1), "echo probe-a2": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1") { step in
            asked.add(step.title)
            return .password("pw-\(step.title)")
        }
        XCTAssertEqual(asked.all, ["Admin one", "Admin two"])
        XCTAssertEqual(fixes.all, ["a1", "p", "a2"])
        XCTAssertTrue(out.ok)
    }

    func testCancellingTheAdminQuestionStopsSetUpAll() async {
        let fixes = Counter()
        let asked = Counter()
        let w = world { _, _ in [
            step("p", fix: runFix(fixes, "p")),
            step("a1", fix: runFix(fixes, "a1", admin: true)),
            step("q", fix: runFix(fixes, "q")),
        ] }
        w.answers.value = ["echo probe-p": ("bad", 1), "echo probe-a1": ("bad", 1), "echo probe-q": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1") { step in asked.add(step.title); return .cancelled }
        XCTAssertEqual(asked.count, 1)
        XCTAssertEqual(fixes.all, ["p"])
        XCTAssertFalse(out.ok)
    }

    func testSetUpAllForARootSSHMachineAsksForTheConfirmation() async {
        var m = TestMachines.linux("vps")
        m.admin = .rootSSH
        let fixes = Counter()
        let w = world([m]) { _, _ in [step("a1", fix: runFix(fixes, "a1", admin: true))] }
        w.answers.value = ["echo probe-a1": ("bad", 1)]
        await w.engine.refresh(["vps"])
        let pwOnly = await w.engine.setUpAll(on: "vps") { _ in .password("x") }
        XCTAssertFalse(pwOnly.ok, "a password is not the root confirmation")
        XCTAssertEqual(fixes.count, 0)
        let ok = await w.engine.setUpAll(on: "vps") { _ in .rootConfirmed }
        XCTAssertTrue(ok.ok)
        XCTAssertEqual(fixes.all, ["a1"])
    }

    // MARK: updates

    func testUpdateMachinesDeliversThenRechecks() async {
        let order = Counter()
        let usageClaude = [
            SecretUsage(name: "claude-worker", machine: "m1", remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey)),
            SecretUsage(name: "claude-worker", machine: "vps", remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey)),
        ]
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")],
                      steps: { _, _ in [step("claude-login", title: "Claude login")] },
                      usages: { _ in usageClaude },
                      deliver: { usage, ctx in
                          order.add("deliver \(usage.machine)")
                          ctx.log("sent to \(usage.machine)\n")
                          return FixOutcome(ok: true, message: "stored")
                      })
        let base = w.runner.handler
        w.runner.handler = { call in
            order.add("ssh " + (call.argv.contains { $0.hasSuffix("@m1") } ? "m1" : "vps"))
            return base(call)
        }
        w.engine.vaultValueChanged(name: "claude-worker")
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.count, 2)
        let outcomes = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(outcomes.map(\.ok), [true, true])
        XCTAssertEqual(order.all, ["deliver m1", "ssh m1", "deliver vps", "ssh vps"])
        XCTAssertNil(w.engine.updateOffers["claude-worker"], "the offer is cleared")
        XCTAssertEqual(w.engine.logs["m1"]?.contains("sent to m1"), true)
        XCTAssertNil(w.engine.running["m1"])
    }

    func testAFailedDeliveryStaysInTheOffer() async {
        let usage = { (m: String) in SecretUsage(name: "claude-worker", machine: m, remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey)) }
        let w = world([TestMachines.mac("m1"), TestMachines.linux("vps")], steps: { _, _ in [step("claude-login")] },
                      usages: { _ in [usage("m1"), usage("vps")] },
                      deliver: { u, _ in FixOutcome(ok: u.machine == "m1", message: u.machine == "m1" ? "stored" : "ssh failed") })
        w.engine.vaultValueChanged(name: "claude-worker")
        let outcomes = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(outcomes.map(\.ok), [true, false])
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.map(\.machine), ["vps"])
    }

    func testRenewingTheIssuesTokenRechecksTheScopeToo() async {
        let usage = SecretUsage(name: "m1-github-issues", machine: "m1", remotePath: MachinePaths.githubToken, format: .raw)
        let w = world(steps: { _, _ in [step("issues-token"), step("token-scope", cadence: .slow)] },
                      usages: { _ in [usage] }, deliver: { _, _ in FixOutcome(ok: true, message: "stored") })
        w.engine.vaultValueChanged(name: "m1-github-issues")
        _ = await w.engine.updateMachines(for: "m1-github-issues", approval: admin())
        XCTAssertTrue(w.runner.batches.contains { ($0.argv.last ?? "").contains("echo probe-token-scope") })
        XCTAssertEqual(w.engine.results["m1"]?[.tokenScope]?.state, .pass("ok"))
    }

    func testUpdateMachinesHidesTheValueInItsOutcome() async {
        let usage = SecretUsage(name: "claude-worker", machine: "m1", remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey))
        let w = world(secrets: FakeSecrets(values: ["claude-worker": "sk-ant-oat01-XYZ0"]), steps: { _, _ in [step("claude-login")] },
                      usages: { _ in [usage] },
                      deliver: { _, ctx in ctx.log("wrote sk-ant-oat01-XYZ0\n"); return FixOutcome(ok: false, message: "echo sk-ant-oat01-XYZ0") })
        w.engine.vaultValueChanged(name: "claude-worker")
        let outcomes = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(outcomes.first?.message, "echo [secret:claude-worker]")
        XCTAssertEqual(w.engine.logs["m1"]?.contains("wrote [secret:claude-worker]"), true)
    }

    // MARK: detected facts

    private func detectingStep(_ seen: Box<[Machine]>? = nil, detected: Detected) -> SetupStep {
        step("reachable", parse: { _, ctx in
            seen?.value.append(ctx.machine)
            return CheckResult(state: .pass("up"), detected: detected)
        })
    }

    func testDetectedFactsAreSavedOnlyWhenTheyChange() async {
        var m = TestMachines.mac("m1")
        m.os = nil
        let seen = Box<[Machine]>([])
        let facts = Detected(os: .macOS, home: "/Users/sondoan", fileVault: true, python: "/opt/homebrew/bin/python3")
        let w = world([m]) { _, _ in [self.detectingStep(seen, detected: facts)] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.store.saves.count, 1)
        let saved = w.store.saves[0][0]
        XCTAssertEqual(saved.os, .macOS)
        XCTAssertTrue(saved.fileVault)
        XCTAssertEqual(saved.abilities?.home, "/Users/sondoan")
        XCTAssertEqual(saved.python, "/opt/homebrew/bin/python3")
        await w.engine.refresh(["m1"])
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.store.saves.count, 1, "nothing changed, nothing saved")
        XCTAssertFalse(w.engine.saveRefused)
        XCTAssertEqual(seen.value.last?.os, .macOS)
    }

    func testFactsThatAreAlreadySavedAreNotSavedAgainWhenTheRegistryDoesNotChange() async {
        var m = TestMachines.mac("m1")
        m.os = nil
        let w = world([m]) { _, _ in [self.detectingStep(detected: Detected(os: .macOS))] }
        w.store.applySaves = false      // machines() keeps returning the old list
        await w.engine.refresh(["m1"])
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.store.saves.count, 1)
    }

    func testARefusedSaveKeepsTheFactsInMemory() async {
        var m = TestMachines.mac("m1")
        m.os = nil
        let seen = Box<[Machine]>([])
        let w = world([m]) { _, _ in [self.detectingStep(seen, detected: Detected(os: .macOS))] }
        w.store.saveOK = false
        await w.engine.refresh(["m1"])
        XCTAssertTrue(w.engine.saveRefused)
        await w.engine.refresh(["m1"])
        XCTAssertEqual(seen.value.last?.os, .macOS, "the next check sees the fact although it was not saved")
        XCTAssertEqual(w.store.saves.count, 2, "it tries again")
        w.store.saveOK = true
        await w.engine.refresh(["m1"])
        XCTAssertFalse(w.engine.saveRefused)
        XCTAssertEqual(w.store.machines[0].os, .macOS)
    }

    /// Plugins installed learns the sdPath; the steps after it (Service installed) are built from the machine, so
    /// they must see it in the same refresh.
    func testAStepAfterPluginsSeesTheSdPathPluginsDetected() async {
        let seenByService = Box<[String]>([])
        let found = "/Users/sondoan/.local/share/agents/system-design/plugin/scripts/sd.py"
        var m = TestMachines.mac("m1")
        m.sdWorker?.sdPath = ""
        let w = world([m]) { machine, _ in [
            step("plugins", parse: { _, _ in CheckResult(state: .pass("ok"), detected: Detected(sdPath: found)) }),
            step("service", needs: ["plugins"], parse: { _, ctx in
                seenByService.value.append(ctx.machine.sdPath)
                return CheckResult(state: .pass("sd=\(ctx.machine.sdPath)"))
            }),
        ].map { s in
            // the script of a step is built from the machine it was made for
            s.id.raw == "service" ? SetupStep(id: s.id, title: s.title, needs: s.needs, cadence: s.cadence,
                                              probe: .remote(script: "echo probe-service \(machine.sdPath)", parse: { out, ctx in
                                                  seenByService.value.append(ctx.machine.sdPath)
                                                  return CheckResult(state: .pass("sd=\(ctx.machine.sdPath)"))
                                              }), fix: s.fix) : s
        } }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(seenByService.value.last, found)
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "service")]?.state, .pass("sd=\(found)"))
        XCTAssertTrue(w.runner.batches.last?.argv.last?.contains("echo probe-service \(found)") == true)
        XCTAssertEqual(w.store.saves.last?[0].sdPath, found)
        let calls = w.runner.batches.count
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.batches.count, calls + 1, "nothing new the second time: one ssh")
    }

    func testAnUnsafePathFromAMachineIsNotKept() async {
        let w = world { _, _ in [self.detectingStep(detected: Detected(home: "/home/a;rm -rf x", python: "python3", sdPath: "../x"))] }
        await w.engine.refresh(["m1"])
        XCTAssertTrue(w.store.saves.isEmpty)
    }

    func testDetectedTokenExpiryIsSavedToTheJobAndTheVault() async {
        let w = world { _, _ in [
            step("reachable"),
            step("token-scope", cadence: .slow, parse: { _, _ in
                CheckResult(state: .pass("expires 2027-02-01"), detected: Detected(tokenExpires: "2027-02-01"))
            }),
        ] }
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.store.saves.last?[0].sdWorker?.tokenExpires, "2027-02-01")
        XCTAssertEqual(w.expiry.all, ["m1-github-issues=2027-02-01"])
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.expiry.all.count, 1, "only when it changes")
        XCTAssertEqual(w.store.saves.count, 1)
    }

    // MARK: toggle

    func testToggleRunningTurnsTheWorkerOnAndOff() async {
        let w = world([TestMachines.linux("vps")]) { _, _ in [
            step("sd-check", title: "sd check"),
            step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: true)),
        ] }
        echoStdin(w)
        await w.engine.refresh(["vps"])
        let refused = await w.engine.toggleRunning(on: "vps", to: true, approval: admin())
        XCTAssertFalse(refused.ok)
        XCTAssertFalse(w.runner.calls.contains { FakeSSH.isAdmin($0) })

        let on = await w.engine.toggleRunning(on: "vps", to: true, approval: admin(password: "hunter2-Pw!"))
        XCTAssertTrue(on.ok, on.message)
        XCTAssertTrue(w.runner.calls.last { FakeSSH.isAdmin($0) }?.argv.last?.contains("systemctl enable --now sd-worker.service") == true)
        let off = await w.engine.toggleRunning(on: "vps", to: false, approval: admin(password: "hunter2-Pw!"))
        XCTAssertTrue(off.ok, off.message)
        XCTAssertTrue(w.runner.calls.last { FakeSSH.isAdmin($0) }?.argv.last?.contains("systemctl disable --now sd-worker.service") == true)
        XCTAssertFalse((w.engine.logs["vps"] ?? []).joined().contains("hunter2-Pw!"))
        XCTAssertNil(w.engine.running["vps"])
    }

    func testToggleRunningOnAMacUsesLaunchctlWithoutAdmin() async {
        let w = world { _, _ in [step("sd-check"), step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: false))] }
        await w.engine.refresh(["m1"])
        let on = await w.engine.toggleRunning(on: "m1", to: true, approval: admin())
        XCTAssertTrue(on.ok, on.message)
        XCTAssertTrue(w.runner.calls.contains { $0.argv.last?.contains("launchctl bootstrap") == true })
    }

    func testTurningTheWorkerOnWaitsForTheSetupCheckButOffDoesNot() async {
        let w = world { _, _ in [
            step("sd-check", title: "sd check"),
            step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: false)),
        ] }
        w.answers.value = ["echo probe-sd-check": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let on = await w.engine.toggleRunning(on: "m1", to: true, approval: admin())
        XCTAssertEqual(on, FixOutcome(ok: false, message: "waits for sd check"))
        let off = await w.engine.toggleRunning(on: "m1", to: false, approval: admin())
        XCTAssertTrue(off.ok, off.message)
    }

    // MARK: integration round

    /// The catalog's real step `id` of `machine` with its real Fix, a probe that passes on this Mac, and stubs that
    /// pass for what it needs: the engine runs the real Fix on a runner that answers it.
    private func realFix(_ id: StepID, _ machine: Machine) -> [SetupStep] {
        func passes(_ id: StepID, _ title: String, _ fix: StepFix) -> SetupStep {
            SetupStep(id: id, title: title, needs: [], cadence: .everyRefresh,
                      probe: .local { _ in CheckResult(state: .pass("ok")) }, fix: fix)
        }
        guard let real = StepCatalog.steps(for: machine, all: [machine]).first(where: { $0.id == id }) else { return [] }
        return real.needs.map { passes($0, $0.raw, .howTo("")) } + [passes(real.id, real.title, real.fix)]
    }

    /// Task 5: the failure line of a Fix is cut to 200 characters; the cut falls inside the token.
    func testAClaudeInstallFailureIsMaskedBeforeItIsCut() async {
        let machine = TestMachines.mac("m1")
        let steps = realFix(.claudeCode, machine)
        XCTAssertEqual(steps.last?.id, .claudeCode)
        let w = world([machine], secrets: FakeSecrets(values: ["claude-worker": longToken])) { _, _ in steps }
        w.runner.handler = { _ in (CommandResult(code: 1, out: "", err: String(repeating: "f", count: 170) + self.longToken), []) }
        await w.engine.refresh(["m1"])
        let out = await w.engine.runFix(.claudeCode, on: "m1", approval: admin())
        XCTAssertFalse(out.ok)
        noFragment(of: longToken, in: out.message)
        noFragment(of: longToken, in: (w.engine.logs["m1"] ?? []).joined(separator: "\n"))
        XCTAssertTrue(out.message.contains("[secret:claude-worker]"), out.message)
    }

    /// Task 6: the clone failure is the first line of the output, cut to 200 characters.
    func testACloneFailureIsMaskedBeforeItIsCut() async {
        let machine = TestMachines.mac("m1")
        let id = StepID.clone(TestMachines.repo)
        let steps = realFix(id, machine)
        XCTAssertEqual(steps.last?.id, id)
        let w = world([machine], secrets: FakeSecrets(values: ["m1-github-issues": longToken])) { _, _ in steps }
        w.runner.handler = { _ in (CommandResult(code: 128, out: "", err: String(repeating: "f", count: 170) + self.longToken), []) }
        await w.engine.refresh(["m1"])
        let out = await w.engine.runFix(id, on: "m1", approval: admin())
        XCTAssertFalse(out.ok)
        noFragment(of: longToken, in: out.message)
        XCTAssertTrue(out.message.contains("[secret:m1-github-issues]"), out.message)
    }

    /// Task 9: the write failure is the first line of stderr, cut to 300 characters.
    func testAWorkerJSONWriteFailureIsMaskedBeforeItIsCut() async {
        let machine = TestMachines.mac("m1")
        let steps = realFix(.workerJSON, machine)
        XCTAssertEqual(steps.last?.id, .workerJSON)
        let w = world([machine], secrets: FakeSecrets(values: ["claude-worker": longToken])) { _, _ in steps }
        w.runner.handler = { call in
            // the read finds no file; the write (the only call with stdin) fails
            call.stdin == nil
                ? (CommandResult(code: 0, out: "", err: ""), [])
                : (CommandResult(code: 1, out: "", err: String(repeating: "f", count: 270) + self.longToken), [])
        }
        await w.engine.refresh(["m1"])
        let out = await w.engine.runFix(.workerJSON, on: "m1", approval: admin())
        XCTAssertFalse(out.ok)
        XCTAssertTrue(out.message.hasPrefix("couldn't write worker.json: "), out.message)
        noFragment(of: longToken, in: out.message)
        XCTAssertTrue(out.message.contains("[secret:claude-worker]"), out.message)
    }

    /// Task 7 through the engine's mask: another vault value of the machine, echoed where deliver's cut falls.
    func testDeliveryMasksTheMachinesOtherVaultValuesBeforeItCuts() async {
        let issues = "github_pat_" + String(longToken.dropFirst(13))
        let secrets = FakeSecrets(values: ["claude-worker": longToken, "m1-github-issues": issues])
        let w = world(secrets: secrets, steps: { _, _ in [] }, usages: SecretSteps.usages, deliver: SecretSteps.deliver)
        w.runner.handler = { call in
            call.stdin == nil
                ? (CommandResult(code: 0, out: "", err: ""), [])
                : (CommandResult(code: 1, out: "", err: String(repeating: "f", count: 270) + issues), [])
        }
        w.engine.vaultValueChanged(name: "claude-worker")
        let outcomes = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(outcomes.count, 1)
        XCTAssertEqual(outcomes.first?.ok, false)
        noFragment(of: issues, in: outcomes.first?.message ?? "")
        XCTAssertTrue(outcomes.first?.message.contains("[secret:m1-github-issues]") == true, outcomes.first?.message ?? "")
    }

    /// O1: the delivery filter keeps one redactor per pipe. A token split on stdout while stderr writes between
    /// the halves is whole again in its line.
    func testDeliveryMasksAValueSplitOnOnePipeWhenTheOtherPipeWritesBetween() async {
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        let w = world(secrets: secrets, steps: { _, _ in [] }, usages: SecretSteps.usages, deliver: SecretSteps.deliver)
        let half = longToken.count / 2
        w.runner.taggedChunks = { call in
            guard call.stdin != nil else { return [] }
            return [(.stdout, "using " + String(self.longToken.prefix(half))),
                    (.stderr, "warning: something\n"),
                    (.stdout, String(self.longToken.dropFirst(half)) + " done\n"),
                    (.stderr, "warning: again\n")]
        }
        w.engine.vaultValueChanged(name: "claude-worker")
        let outcomes = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(outcomes.first?.ok, true, outcomes.first?.message ?? "")
        let log = w.engine.logs["m1"] ?? []
        noFragment(of: longToken, in: log.joined(separator: "\n"))
        XCTAssertTrue(log.contains("using [secret:claude-worker] done"), "\(log)")
        XCTAssertTrue(log.contains("warning: something"))
        XCTAssertTrue(log.contains("warning: again"))
    }

    /// O1: what each redactor holds back at the end goes to its own pipe's line.
    func testDeliveryFlushesEachPipesHeldTailOnThatPipe() async {
        let secrets = FakeSecrets(values: ["claude-worker": longToken])
        let w = world(secrets: secrets, steps: { _, _ in [] }, usages: SecretSteps.usages, deliver: SecretSteps.deliver)
        // each ends in the start of the value (a held tail) and has no newline
        w.runner.taggedChunks = { call in
            guard call.stdin != nil else { return [] }
            return [(.stdout, "out sk-a"), (.stderr, "err sk-ant")]
        }
        w.engine.vaultValueChanged(name: "claude-worker")
        _ = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        let log = w.engine.logs["m1"] ?? []
        XCTAssertTrue(log.contains("out sk-a"), "\(log)")
        XCTAssertTrue(log.contains("err sk-ant"), "\(log)")
    }
}

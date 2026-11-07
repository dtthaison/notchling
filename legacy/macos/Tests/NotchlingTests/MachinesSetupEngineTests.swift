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

/// A vault that counts how often each value is read (every read is a Keychain read in the app).
private final class CountingSecrets: SecretSource, @unchecked Sendable {
    private let lock = NSLock()
    private let values: [String: String]
    private var reads: [String: Int] = [:]
    init(_ values: [String: String]) { self.values = values }
    func value(named name: String) -> String? {
        lock.withLock { reads[name, default: 0] += 1 }
        return values[name]
    }
    func entry(named: String) -> VaultEntry? { nil }
    func reads(of name: String) -> Int { lock.withLock { reads[name] ?? 0 } }
    func resetReads() { lock.withLock { reads = [:] } }
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
                  holdWhen: (@Sendable (Detected) -> String?)? = nil,
                  parse: @escaping @Sendable (RemoteOutput, StepContext) async -> CheckResult = { out, _ in
                      out.code == 0 ? CheckResult(state: .pass(out.text)) : CheckResult(state: .fail(out.text))
                  }) -> SetupStep {
    SetupStep(id: StepID(raw: raw), title: title ?? raw.uppercased(), needs: needs.map { StepID(raw: $0) },
              cadence: cadence, probe: .remote(script: "echo probe-\(raw)", parse: parse), fix: fix, holdWhen: holdWhen)
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

    private func world(_ machines: [Machine] = [TestMachines.mac("m1")], secrets: SecretSource = FakeSecrets(),
                       steps: @escaping (Machine, [Machine]) -> [SetupStep],
                       usages: @escaping ([Machine]) -> [SecretUsage] = { _ in [] },
                       deliver: @escaping (SecretUsage, FixContext) async -> FixOutcome = { _, _ in FixOutcome(ok: true, message: "") },
                       observeVault: Bool = false, offersFile: URL? = nil) -> World {
        let w = World(machines: machines, now: t0)
        let store = w.store, clock = w.clock, expiry = w.expiry
        w.engine = SetupEngine(
            runner: w.runner, secrets: secrets, http: FakeHTTP(), mac: { .empty },
            machines: { store.machines }, save: { store.save($0) }, clock: { clock.value },
            steps: steps, usages: usages, deliver: deliver,
            setVaultExpiry: { name, day in expiry.add("\(name)=\(day)") }, observeVault: observeVault, offersFile: offersFile)
        return w
    }

    /// A folder for one test, never the real support folder.
    private func tempFolder() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("notchling-offers-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        return dir
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

    /// M6: a password is masked from 4 characters up. Under that, replacing it as text would turn ordinary words of the
    /// log into `[admin password]` and hint at the password; `sudo -S -p ''` never echoes it anyway.
    func testAnAdminPasswordIsMaskedFromFourCharactersUp() async {
        let four = world { _, _ in [self.adminStep()] }
        echoStdin(four)
        let masked = await four.engine.runFix(StepID(raw: "adm"), on: "m1", approval: admin(password: "abcd"))
        XCTAssertTrue(masked.message.contains("[admin password]"), masked.message)
        XCTAssertFalse((four.engine.logs["m1"] ?? []).joined().contains("abcd"))

        let short = world { _, _ in [self.adminStep()] }
        echoStdin(short)
        let plain = await short.engine.runFix(StepID(raw: "adm"), on: "m1", approval: admin(password: "ab"))
        XCTAssertFalse(plain.message.contains("[admin password]"), plain.message)
        XCTAssertEqual(SetupMasks(vault: [:], adminPassword: "ab").apply("a table and abc"), "a table and abc")
        XCTAssertEqual(SetupMasks(vault: [:], adminPassword: "abc").apply("abc"), "abc")
        XCTAssertEqual(SetupMasks(vault: [:], adminPassword: "abcd").apply("x abcd y"), "x [admin password] y")
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

    /// C1 / ruling M12: sudo as the worker account on Linux is refused before any command runs, a password or not.
    /// Set up all doesn't even ask for the password of such a step.
    func testALinuxAdminStepAsTheWorkerIsRefusedBeforeAnyCommandRuns() async {
        let refusal = "On Linux the worker account can't be the admin: its own shell files could read the password. Use root over SSH or another admin account."
        let w = world([TestMachines.linux("vps")]) { _, _ in [self.adminStep()] }
        for approval in [admin(password: "hunter2-Pw!"), admin(), admin(root: true)] {
            let out = await w.engine.runFix(StepID(raw: "adm"), on: "vps", approval: approval)
            XCTAssertEqual(out, FixOutcome(ok: false, message: refusal))
        }
        XCTAssertEqual(w.runner.calls.count, 0, "refused before any command ran")
        XCTAssertNil(w.engine.logs["vps"])

        // Set up all reaches the step, and stops there without asking
        w.answers.value = ["echo probe-adm": ("bad", 1)]
        await w.engine.refresh(["vps"])
        let asked = Counter()
        let plan = w.engine.setUpAllPlan(for: "vps")
        XCTAssertEqual(plan.map(\.id.raw), ["adm"])
        let all = await w.engine.setUpAll(on: "vps", approved: plan) { _ in asked.add("asked"); return .password("hunter2-Pw!") }
        XCTAssertFalse(all.ok)
        XCTAssertTrue(all.message.contains(refusal), all.message)
        XCTAssertEqual(asked.count, 0, "the password was not asked for")
        XCTAssertFalse(w.runner.calls.contains { FakeSSH.isAdmin($0) })
    }

    /// Root over SSH is the Linux admin: the ssh goes to root, with no sudo and no password.
    func testALinuxAdminStepOverRootSSHRunsAsRootWithoutSudo() async {
        var vps = TestMachines.linux("vps")
        vps.admin = .rootSSH
        let w = world([vps]) { _, _ in [self.adminStep()] }
        echoStdin(w)
        let out = await w.engine.runFix(StepID(raw: "adm"), on: "vps", approval: admin(root: true))
        XCTAssertTrue(out.ok, out.message)
        let calls = w.runner.calls.filter { $0.argv.contains("root@vps") }
        XCTAssertEqual(calls.count, 1)
        XCTAssertEqual(calls[0].argv, SSH.admin(vps, "echo installing", asRoot: true))
        XCTAssertNil(calls[0].stdin)
        XCTAssertFalse(calls[0].argv.last!.contains("sudo"))
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

    /// M2: only the facts steps are built from (os, home, python, sdPath, and the token expiry that worker.json's
    /// check is built from) start a second pass; FileVault does not, and a fact already known does not repeat it.
    func testFactsNoStepIsBuiltFromDoNotRepeatTheChecks() async {
        let w = world { _, _ in [
            step("reachable", parse: { _, _ in CheckResult(state: .pass("up"), detected: Detected(fileVault: true)) }),
        ] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.runner.batches.count, 1)
        XCTAssertTrue(w.store.saves.last?[0].fileVault == true)

        let expiry = world { _, _ in [
            step("reachable", parse: { _, _ in CheckResult(state: .pass("up"), detected: Detected(tokenExpires: "2027-02-01")) }),
        ] }
        await expiry.engine.refresh(["m1"])
        XCTAssertEqual(expiry.runner.batches.count, 2, "a newly learned expiry builds the steps again, once")
        await expiry.engine.refresh(["m1"])
        XCTAssertEqual(expiry.runner.batches.count, 3, "the next refresh finds the expiry known and checks once")
    }

    /// A fact the checks learn about the token's expiry reaches the checks of the same refresh (worker.json's
    /// `token_expires` is built from it).
    func testANewlyLearnedTokenExpiryReachesTheChecksOfTheSameRefresh() async {
        let seen = Box<[String?]>([])
        let w = world { _, _ in [
            step("a", parse: { _, _ in CheckResult(state: .pass("a"), detected: Detected(tokenExpires: "2027-02-01")) }),
            step("b", parse: { _, ctx in
                seen.value.append(ctx.machine.sdWorker?.tokenExpires)
                return CheckResult(state: .pass("b"))
            }),
        ] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(seen.value.last ?? nil, "2027-02-01", "the second pass is built from the learned expiry: \(seen.value)")
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

    /// M5: setUpAll looks at each step as it is when its turn comes, not as it was when the run started. It runs
    /// only steps the user was shown (`approved`), with the commands shown.
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
            step("b", fix: runFix(fixes, "b")),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1), "echo probe-b": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1", approved: w.engine.setUpAllPlan(for: "m1")) { _ in .cancelled }
        XCTAssertEqual(out.message, "2 steps set up; still not ✓: A, B", "the stubs' probes keep failing, and the end says so")
        XCTAssertEqual(fixes.all, ["a", "b"])
    }

    /// Review I1: the list Set up all shows holds the steps that wait for an earlier step, with their commands.
    func testSetUpAllPlanListsWaitingStepsWithTheirCommands() async {
        let fixes = Counter()
        let w = world { _, _ in [
            step("a", fix: runFix(fixes, "a")),
            step("b", needs: ["a"], fix: runFix(fixes, "b")),
            step("c", needs: ["b"], fix: .howTo("by hand")),
            step("d", fix: runFix(fixes, "d")),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1), "echo probe-b": ("bad", 1), "echo probe-c": ("bad", 1)]
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.setUpAllPlan(for: "m1"), [
            ApprovedStep(id: StepID(raw: "a"), title: "A", display: "do a", waiting: false),
            ApprovedStep(id: StepID(raw: "b"), title: "B", display: "do b", waiting: true),
        ], "d passes, c has no run Fix")
    }

    /// Review I1: a step that was not shown is not run, even when it turns ✗ during the run.
    func testSetUpAllStopsAtAStepThatWasNotListed() async {
        let fixes = Counter()
        let broke = Box(false)
        let w = world { _, _ in [
            step("a", fix: .run(FixPlan(display: "do a", needsAdmin: false) { _ in
                broke.value = true
                fixes.add("a")
                return FixOutcome(ok: true, message: "did a")
            })),
            step("c", fix: runFix(fixes, "c"), parse: { _, _ in
                broke.value ? CheckResult(state: .fail("broke")) : CheckResult(state: .pass("fine"))
            }),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let shown = w.engine.setUpAllPlan(for: "m1")
        XCTAssertEqual(shown.map(\.id.raw), ["a"])
        let out = await w.engine.setUpAll(on: "m1", approved: shown) { _ in .cancelled }
        XCTAssertFalse(out.ok)
        XCTAssertEqual(out.message, "C needs a look; Set up again")
        XCTAssertEqual(fixes.all, ["a"], "C was not run")
    }

    /// Review I1: a listed step whose commands changed since the confirmation is not run.
    func testSetUpAllStopsWhenAListedStepsCommandsChanged() async {
        let fixes = Counter()
        let changed = Box(false)
        let w = world { _, _ in [
            step("a", fix: .run(FixPlan(display: "do a", needsAdmin: false) { _ in
                changed.value = true
                fixes.add("a")
                return FixOutcome(ok: true, message: "did a")
            }), parse: { _, _ in
                changed.value ? CheckResult(state: .pass("ok")) : CheckResult(state: .fail("bad"))
            }),
            step("b", needs: ["a"], fix: .run(FixPlan(display: changed.value ? "do b after" : "do b before", needsAdmin: false) { _ in
                fixes.add("b")
                return FixOutcome(ok: true, message: "did b")
            })),
        ] }
        w.answers.value = ["echo probe-b": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let shown = w.engine.setUpAllPlan(for: "m1")
        XCTAssertEqual(shown.map(\.display), ["do a", "do b before"])
        let out = await w.engine.setUpAll(on: "m1", approved: shown) { _ in .cancelled }
        XCTAssertFalse(out.ok)
        XCTAssertEqual(out.message, "B needs a look; Set up again")
        XCTAssertEqual(fixes.all, ["a"], "B was not run")
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

    /// Review minor: a burst of output is published in a few batches, not once per line.
    func testABurstOfOutputIsPublishedInFewBatchesInOrder() async {
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
            for i in 0..<1000 { ctx.log("line \(i)\n") }
            return FixOutcome(ok: true, message: "done")
        })
        let w = world { _, _ in [step("a", fix: fix)] }
        var publishes = 0
        let watch = w.engine.$logs.dropFirst().sink { _ in publishes += 1 }
        _ = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        watch.cancel()
        let log = w.engine.logs["m1"] ?? []
        XCTAssertEqual(log.filter { $0.hasPrefix("line ") }.compactMap { Int($0.dropFirst(5)) }, Array(0..<1000))
        XCTAssertEqual(log.first, "▸ A")
        XCTAssertEqual(log.last, "✓ done")
        XCTAssertLessThan(publishes, 60, "published \(publishes) times for 1000 lines")
    }

    /// Review minor: the cards pass the machines they hold; the engine does not read machines.json for each redraw.
    func testStepsAmongGivenMachinesReadNoRegistry() {
        let reads = Counter()
        let machine = TestMachines.mac("m1")
        let engine = SetupEngine(runner: EngineRunner(), secrets: FakeSecrets(), http: FakeHTTP(), mac: { .empty },
                                 machines: { reads.add("read"); return [machine] }, save: { _ in true }, clock: { Date() },
                                 steps: { _, _ in [step("a"), step("b")] }, observeVault: false)
        for _ in 0..<20 { _ = engine.steps(for: machine, among: [machine]) }
        XCTAssertEqual(reads.count, 0)
        XCTAssertEqual(engine.steps(for: machine, among: [machine]).map(\.id.raw), ["a", "b"])
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
        let out = await w.engine.setUpAll(on: "m1", approved: w.engine.setUpAllPlan(for: "m1")) { _ in .cancelled }
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
        let out = await w.engine.setUpAll(on: "m1", approved: w.engine.setUpAllPlan(for: "m1")) { _ in .cancelled }
        XCTAssertEqual(out.message, "2 steps set up; still not ✓: B", "b keeps failing until its own probe is fixed")
        XCTAssertEqual(fixes.all, ["a", "b"])
    }

    func testSetUpAllSkipsHowToAndToggleSteps() async {
        let fixes = Counter()
        let w = world { _, _ in [
            step("h", fix: .howTo("by hand")), step("t", fix: .toggle(needsAdmin: false)), step("r", fix: runFix(fixes, "r")),
        ] }
        w.answers.value = ["echo probe-h": ("bad", 1), "echo probe-t": ("bad", 1), "echo probe-r": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let out = await w.engine.setUpAll(on: "m1", approved: w.engine.setUpAllPlan(for: "m1")) { _ in .cancelled }
        XCTAssertEqual(fixes.all, ["r"])
        XCTAssertEqual(out.message, "1 step set up; still not ✓: H, T, R")
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
        let out = await w.engine.setUpAll(on: "m1", approved: w.engine.setUpAllPlan(for: "m1")) { step in
            asked.add(step.title)
            return .password("pw-\(step.title)")
        }
        XCTAssertEqual(asked.all, ["Admin one", "Admin two"])
        XCTAssertEqual(fixes.all, ["a1", "p", "a2"])
        XCTAssertTrue(out.message.hasPrefix("3 steps set up"), out.message)
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
        let out = await w.engine.setUpAll(on: "m1", approved: w.engine.setUpAllPlan(for: "m1")) { step in asked.add(step.title); return .cancelled }
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
        let pwOnly = await w.engine.setUpAll(on: "vps", approved: w.engine.setUpAllPlan(for: "vps")) { _ in .password("x") }
        XCTAssertFalse(pwOnly.ok, "a password is not the root confirmation")
        XCTAssertEqual(fixes.count, 0)
        let ok = await w.engine.setUpAll(on: "vps", approved: w.engine.setUpAllPlan(for: "vps")) { _ in .rootConfirmed }
        XCTAssertTrue(ok.message.hasPrefix("1 step set up"), ok.message)
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

    // MARK: M2: a single Fix runs the commands the sheet showed

    func testRunFixRefusesWhenItsCommandsChangedAfterTheSheetShowedThem() async {
        let fixes = Counter()
        let display = Box("do it with python A")
        let w = world { _, _ in [
            step("a", fix: .run(FixPlan(display: display.value, needsAdmin: false) { _ in
                fixes.add("a")
                return FixOutcome(ok: true, message: "did a")
            })),
        ] }
        w.answers.value = ["echo probe-a": ("bad", 1)]
        await w.engine.refresh(["m1"])
        // a background refresh learned another python between the sheet and the click
        display.value = "do it with python B"
        let refused = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin(), expectedDisplay: "do it with python A")
        XCTAssertEqual(refused, FixOutcome(ok: false, message: "A needs a look; Set up again"))
        XCTAssertEqual(fixes.count, 0)
        XCTAssertNil(w.engine.logs["m1"], "nothing ran")
        let same = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin(), expectedDisplay: "do it with python B")
        XCTAssertTrue(same.ok, same.message)
        XCTAssertEqual(fixes.all, ["a"])
    }

    // MARK: M3: a refresh that is out of date is dropped

    /// Answers a batched probe from `answer` as it is when the call arrives; the first call then waits for `gate`.
    private final class StaleRunner: CommandRunning, @unchecked Sendable {
        let gate = Gate()
        let answer = Box("old")
        private let lock = NSLock()
        private var calls = 0
        var holdFirst = true
        /// Calls (by number) that wait for a gate of their own, instead of the first call waiting for `gate`.
        var holds: [Int: Gate] = [:]
        var seen: Int { lock.withLock { calls } }

        func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
            await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
        }

        func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
                 onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
            let number = lock.withLock { () -> Int in calls += 1; return calls }
            let said = answer.value
            if let own = holds[number] { await own.wait() }
            else if number == 1 && holdFirst && holds.isEmpty { await gate.wait() }
            let call = RecordedCall(argv: argv, env: env, stdin: stdin, cwd: cwd)
            return FakeSSH.batch(call, reply: { _ in (out: said, code: 0) }) ?? CommandResult(code: 0, out: "", err: "")
        }
    }

    private func staleWorld(_ machines: [Machine] = [TestMachines.mac("m1")], steps: [SetupStep],
                            clock: @escaping () -> Date? = { nil }) -> (SetupEngine, StaleRunner, Store) {
        let runner = StaleRunner()
        let store = Store(machines)
        let engine = SetupEngine(runner: runner, secrets: FakeSecrets(), http: FakeHTTP(), mac: { .empty },
                                 machines: { store.machines }, save: { store.save($0) }, clock: { clock() ?? self.t0 },
                                 steps: { _, _ in steps }, observeVault: false)
        return (engine, runner, store)
    }

    private func waitUntil(_ what: String, _ ok: () -> Bool) async {
        for _ in 0..<300 where !ok() { try? await Task.sleep(nanoseconds: 10_000_000) }
        XCTAssertTrue(ok(), what)
    }

    func testARefreshThatBeganBeforeAFixEndedDoesNotOverwriteTheFixesOwn() async {
        let fix = Counter()
        let (engine, runner, _) = staleWorld(steps: [step("a", fix: runFix(fix, "a"))])
        let old = Task { await engine.refresh(["m1"]) }
        await waitUntil("the first refresh asked the machine") { runner.seen == 1 }
        runner.answer.value = "new"
        let out = await engine.runFix(StepID(raw: "a"), on: "m1", approval: admin())
        XCTAssertTrue(out.ok, out.message)
        XCTAssertEqual(engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("new"), "the Fix's own check")
        runner.gate.open()
        await old.value
        XCTAssertEqual(engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("new"), "the old answer was dropped")
    }

    func testAnOlderRefreshNeverPublishesOverANewerOne() async {
        let (engine, runner, _) = staleWorld(steps: [step("a")])
        let old = Task { await engine.refresh(["m1"]) }
        await waitUntil("the first refresh asked the machine") { runner.seen == 1 }
        runner.answer.value = "new"
        await engine.refresh(["m1"])
        XCTAssertEqual(engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("new"))
        runner.gate.open()
        await old.value
        XCTAssertEqual(engine.results["m1"]?[StepID(raw: "a")]?.state, .pass("new"))
    }

    /// The re-review's Important: the poller's cheap refresh every 60 s must not make a slower refresh (the window
    /// opening, Check, the one after a Fix) lose the slow steps' results that the cheap one never computed.
    func testASlowRefreshOvertakenByACheapOneStillPublishesItsSlowResults() async {
        let now = Box(t0)
        let (engine, runner, _) = staleWorld(steps: [step("a"), step("s", cadence: .slow)], clock: { now.value })
        let a = StepID(raw: "a"), s = StepID(raw: "s")
        let slow = Task { await engine.refresh(["m1"], includeSlow: true) }
        await waitUntil("the slow refresh asked the machine") { runner.seen == 1 }
        runner.answer.value = "new"
        await engine.refresh(["m1"])
        XCTAssertEqual(engine.results["m1"]?[a]?.state, .pass("new"))
        XCTAssertEqual(engine.results["m1"]?[s]?.state, .cantTell("not checked yet"), "the cheap refresh ran no slow step")

        runner.answer.value = "slow"
        runner.gate.open()
        await slow.value
        XCTAssertEqual(engine.results["m1"]?[a]?.state, .pass("new"), "the cheap step keeps the newer value")
        XCTAssertEqual(engine.results["m1"]?[s]?.state, .pass("slow"), "the slow step's result is published")

        // the slow set ran, so it is due again after its interval: a plain refresh then runs it (2 ssh calls, not 1)
        let before = runner.seen
        now.value = t0.addingTimeInterval(11 * 60)
        await engine.refresh(["m1"])
        XCTAssertEqual(runner.seen - before, 2, "lastSlow was set when the slow results were published")
    }

    /// The other order: a cheap refresh begun before a slow one published must not lay "not checked yet" (what it
    /// carried over for the slow steps) over the slow results when it ends later.
    func testACheapRefreshEndingAfterASlowOneKeepsTheSlowResults() async {
        let (engine, runner, _) = staleWorld(steps: [step("a"), step("s", cadence: .slow)])
        let a = StepID(raw: "a"), s = StepID(raw: "s")
        let slowGate = Gate(), cheapGate = Gate()
        runner.holds = [2: slowGate, 3: cheapGate]          // call 2 is the slow refresh's slow batch, 3 the cheap refresh
        runner.answer.value = "slow"
        let slow = Task { await engine.refresh(["m1"], includeSlow: true) }
        await waitUntil("the slow refresh reached its slow batch") { runner.seen == 2 }
        let cheap = Task { await engine.refresh(["m1"]) }
        await waitUntil("the cheap refresh asked the machine") { runner.seen == 3 }
        slowGate.open()
        await slow.value
        XCTAssertEqual(engine.results["m1"]?[s]?.state, .pass("slow"))
        cheapGate.open()
        await cheap.value
        XCTAssertEqual(engine.results["m1"]?[s]?.state, .pass("slow"), "the cheap refresh carried nothing over it")
        XCTAssertNotNil(engine.results["m1"]?[a])
    }

    func testARefreshOfAMachineRemovedMeanwhilePublishesNothing() async {
        let (engine, runner, store) = staleWorld(steps: [step("a")])
        let old = Task { await engine.refresh(["m1"]) }
        await waitUntil("the refresh asked the machine") { runner.seen == 1 }
        _ = store.save([])
        runner.gate.open()
        await old.value
        XCTAssertNil(engine.results["m1"])
    }

    // MARK: a step that holds back on what an earlier step detected

    func testAStepThatHoldsBackOnWhatEarlierStepsDetectedIsNotRun() async {
        let facts = Box(Detected(sdVersion: "1.19.2"))
        let check = StepID(raw: "check")
        let w = world { _, _ in [
            step("p", parse: { _, _ in CheckResult(state: .pass("plugins"), detected: facts.value) }),
            step("check", needs: ["p"], cadence: .slow,
                 holdWhen: { $0.sdVersion == "1.19.2" ? "sd is older than 1.19.3; update plugins first" : nil }),
        ] }
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.engine.results["m1"]?[check]?.state, .cantTell("sd is older than 1.19.3; update plugins first"))
        XCTAssertFalse(w.runner.calls.contains { ($0.argv.last ?? "").contains("probe-check") }, "its probe never went to the machine")

        // the machine's sd is new enough now: the probe runs
        facts.value = Detected(sdVersion: "1.19.3")
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.engine.results["m1"]?[check]?.state, .pass("ok"))
        XCTAssertTrue(w.runner.calls.contains { ($0.argv.last ?? "").contains("probe-check") })
    }

    // MARK: M8: a removed machine leaves nothing behind

    func testForgetDropsWhatTheEngineHoldsForAMachine() async {
        var m = TestMachines.mac("m1")
        m.os = nil
        let usage = claudeUsage("m1")
        let facts = Detected(os: .macOS, home: "/Users/other", python: "/opt/other/bin/python3")
        let w = world([m, TestMachines.mac("m2")], steps: { _, _ in [self.detectingStep(detected: facts)] }, usages: { _ in [usage, self.claudeUsage("m2")] })
        w.store.saveOK = false          // the learned facts stay in the engine's overlay
        await w.engine.refresh()
        w.engine.vaultValueChanged(name: "claude-worker")
        XCTAssertNotNil(w.engine.results["m1"])
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.count, 2)
        XCTAssertNotEqual(w.engine.overlaid(TestMachines.mac("m1")), TestMachines.mac("m1"), "the overlay holds m1's facts")

        w.engine.forget("m1")
        XCTAssertNil(w.engine.results["m1"])
        XCTAssertNil(w.engine.logs["m1"])
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.map(\.machine), ["m2"], "no offer for a machine that is gone")
        XCTAssertNotNil(w.engine.results["m2"], "the other machine is untouched")
        // a machine of that name added again starts clean: nothing is laid over it
        let fresh = TestMachines.mac("m1")
        XCTAssertEqual(w.engine.overlaid(fresh), fresh)
    }

    // MARK: I3: the offer survives a relaunch; Send again

    private func claudeUsage(_ machine: String) -> SecretUsage {
        SecretUsage(name: "claude-worker", machine: machine, remotePath: MachinePaths.claudeEnv, format: .keyValue(MachinePaths.claudeKey))
    }

    func testThePendingOfferSurvivesANewEngineAndHoldsNamesOnly() async throws {
        let file = try tempFolder().appendingPathComponent("pending-updates.json")
        let machines = [TestMachines.mac("m1"), TestMachines.linux("vps")]
        let both = [claudeUsage("m1"), claudeUsage("vps")]
        let first = world(machines, secrets: FakeSecrets(values: ["claude-worker": longToken]),
                          steps: { _, _ in [step("claude-login")] }, usages: { _ in both }, offersFile: file)
        first.engine.vaultValueChanged(name: "claude-worker")
        XCTAssertEqual(first.engine.updateOffers["claude-worker"]?.count, 2)

        // the file holds the vault name and the machine names, nothing else
        let saved = try String(contentsOf: file, encoding: .utf8)
        XCTAssertTrue(saved.contains("claude-worker") && saved.contains("m1") && saved.contains("vps"), saved)
        XCTAssertFalse(saved.contains(longToken))
        XCTAssertFalse(saved.contains(".config/agents"), "no paths")
        let mode = try FileManager.default.attributesOfItem(atPath: file.path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o600)

        // a relaunch: a new engine on the same file offers the same update, and sends nothing
        let sent = Counter()
        let second = world(machines, steps: { _, _ in [step("claude-login")] }, usages: { _ in both },
                           deliver: { u, _ in sent.add(u.machine); return FixOutcome(ok: true, message: "stored") }, offersFile: file)
        XCTAssertEqual(second.engine.updateOffers["claude-worker"], both)
        XCTAssertEqual(sent.count, 0, "loading an offer sends nothing")

        // sending it clears the file, so the third launch has none
        let outcomes = await second.engine.updateMachines(for: "claude-worker", approval: admin())
        XCTAssertEqual(outcomes.map(\.ok), [true, true])
        let third = world(machines, steps: { _, _ in [step("claude-login")] }, usages: { _ in both }, offersFile: file)
        XCTAssertTrue(third.engine.updateOffers.isEmpty)
    }

    func testAFailedDeliveryStaysInTheSavedOffer() async throws {
        let file = try tempFolder().appendingPathComponent("pending-updates.json")
        let machines = [TestMachines.mac("m1"), TestMachines.linux("vps")]
        let both = [claudeUsage("m1"), claudeUsage("vps")]
        let w = world(machines, steps: { _, _ in [step("claude-login")] }, usages: { _ in both },
                      deliver: { u, _ in FixOutcome(ok: u.machine == "m1", message: "x") }, offersFile: file)
        w.engine.vaultValueChanged(name: "claude-worker")
        _ = await w.engine.updateMachines(for: "claude-worker", approval: admin())
        let after = world(machines, steps: { _, _ in [step("claude-login")] }, usages: { _ in both }, offersFile: file)
        XCTAssertEqual(after.engine.updateOffers["claude-worker"]?.map(\.machine), ["vps"])
    }

    func testASavedOfferForAMachineThatIsGoneOrAnUnreadableFileIsDropped() async throws {
        let folder = try tempFolder()
        let file = folder.appendingPathComponent("pending-updates.json")
        try Data(#"{"version":1,"offers":{"claude-worker":["gone","m1"],"unknown-secret":["m1"]}}"#.utf8).write(to: file)
        let w = world(steps: { _, _ in [step("claude-login")] }, usages: { _ in [self.claudeUsage("m1")] }, offersFile: file)
        XCTAssertEqual(w.engine.updateOffers, ["claude-worker": [claudeUsage("m1")]])

        let broken = folder.appendingPathComponent("broken.json")
        try Data("{not json".utf8).write(to: broken)
        let b = world(steps: { _, _ in [step("claude-login")] }, usages: { _ in [self.claudeUsage("m1")] }, offersFile: broken)
        XCTAssertTrue(b.engine.updateOffers.isEmpty)
        // and a missing folder is made when there is something to save
        let deep = folder.appendingPathComponent("a/b/pending-updates.json")
        let d = world(steps: { _, _ in [step("claude-login")] }, usages: { _ in [self.claudeUsage("m1")] }, offersFile: deep)
        d.engine.vaultValueChanged(name: "claude-worker")
        XCTAssertTrue(FileManager.default.fileExists(atPath: deep.path))
    }

    /// Send again works on a usage whose check is OK, runs only through this call, and only for a file of the machines.
    func testSendAgainDeliversAUsageThatIsOKAndRefusesOtherFiles() async {
        let sent = Counter()
        let usage = claudeUsage("m1")
        let w = world(steps: { _, _ in [step("claude-login", title: "Claude login")] }, usages: { _ in [usage] },
                      deliver: { u, ctx in sent.add("\(u.name)@\(u.machine)"); ctx.log("sent\n"); return FixOutcome(ok: true, message: "stored") })
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[.claudeLogin]?.state, .pass("ok"))
        XCTAssertEqual(sent.count, 0, "checking sends nothing")

        let out = await w.engine.sendAgain(usage, approval: admin())
        XCTAssertEqual(out, FixOutcome(ok: true, message: "stored"))
        XCTAssertEqual(sent.all, ["claude-worker@m1"])
        XCTAssertEqual(w.engine.logs["m1"]?.contains("sent"), true)
        XCTAssertNil(w.engine.running["m1"])

        // a path or machine that is not one of the usages is refused, with nothing sent
        let elsewhere = SecretUsage(name: "claude-worker", machine: "m1", remotePath: "/etc/passwd", format: .raw)
        let refused = await w.engine.sendAgain(elsewhere, approval: admin())
        XCTAssertFalse(refused.ok)
        let nobody = await w.engine.sendAgain(claudeUsage("nowhere"), approval: admin())
        XCTAssertFalse(nobody.ok)
        XCTAssertEqual(sent.count, 1)
    }

    func testSendAgainSatisfiesThePendingOfferForThatUsage() async throws {
        let file = try tempFolder().appendingPathComponent("pending-updates.json")
        let machines = [TestMachines.mac("m1"), TestMachines.linux("vps")]
        let both = [claudeUsage("m1"), claudeUsage("vps")]
        let w = world(machines, steps: { _, _ in [step("claude-login")] }, usages: { _ in both },
                      deliver: { _, _ in FixOutcome(ok: true, message: "stored") }, offersFile: file)
        w.engine.vaultValueChanged(name: "claude-worker")
        _ = await w.engine.sendAgain(claudeUsage("m1"), approval: admin())
        XCTAssertEqual(w.engine.updateOffers["claude-worker"]?.map(\.machine), ["vps"])
        _ = await w.engine.sendAgain(claudeUsage("vps"), approval: admin())
        XCTAssertNil(w.engine.updateOffers["claude-worker"])
        let after = world(machines, steps: { _, _ in [step("claude-login")] }, usages: { _ in both }, offersFile: file)
        XCTAssertTrue(after.engine.updateOffers.isEmpty)
    }

    func testAFailedSendAgainKeepsTheOffer() async {
        let usage = claudeUsage("m1")
        let w = world(steps: { _, _ in [step("claude-login")] }, usages: { _ in [usage] },
                      deliver: { _, _ in FixOutcome(ok: false, message: "ssh failed") })
        w.engine.vaultValueChanged(name: "claude-worker")
        let out = await w.engine.sendAgain(usage, approval: admin())
        XCTAssertFalse(out.ok)
        XCTAssertEqual(w.engine.updateOffers["claude-worker"], [usage])
    }

    /// The hash of the machine's file is compared in memory; no result, log line or message holds it.
    func testAStaleFileShowsAsDifferentFromTheVaultAndNoHashIsKept() async {
        let m = TestMachines.mac("m1")
        let value = "sk-ant-oat01-" + String(repeating: "q", count: 40)
        let stale = String(repeating: "ab12", count: 16)
        let w = world([m], secrets: FakeSecrets(values: ["claude-worker": value]),
                      steps: { machine, _ in
                          // the stub answers every probe alike, so Reachable must not echo what it was told
                          [step("reachable", parse: { _, _ in CheckResult(state: .pass("up")) }), SecretSteps.claudeLogin(for: machine)]
                      })
        w.runner.handler = { call in
            (FakeSSH.batch(call, reply: { _ in (out: "600 \(value.count + 32)\n\(stale)", code: 0) }) ?? CommandResult(code: 0, out: "", err: ""), [])
        }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[.claudeLogin]?.state, .fail("different from the vault (Send again)"))
        let everything = String(describing: w.engine.results) + String(describing: w.engine.logs)
        XCTAssertFalse(everything.contains(stale), "the machine's hash is shown nowhere")
        XCTAssertFalse(everything.contains(value))
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
        // on a Linux machine the admin is root over ssh (sudo as the worker is refused)
        var vps = TestMachines.linux("vps")
        vps.admin = .rootSSH
        let w = world([vps]) { _, _ in [
            step("sd-check", title: "sd check"),
            step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: true)),
        ] }
        echoStdin(w)
        await w.engine.refresh(["vps"])
        let refused = await w.engine.toggleRunning(on: "vps", to: true, approval: admin())
        XCTAssertFalse(refused.ok, "root needs the user's confirmation")
        XCTAssertEqual(w.runner.calls.filter { $0.argv.contains("root@vps") }.count, 0)

        let on = await w.engine.toggleRunning(on: "vps", to: true, approval: admin(root: true))
        XCTAssertTrue(on.ok, on.message)
        XCTAssertTrue(w.runner.calls.last { $0.argv.contains("root@vps") }?.argv.last?.contains("systemctl enable --now sd-worker.service") == true)
        let off = await w.engine.toggleRunning(on: "vps", to: false, approval: admin(root: true))
        XCTAssertTrue(off.ok, off.message)
        XCTAssertTrue(w.runner.calls.last { $0.argv.contains("root@vps") }?.argv.last?.contains("systemctl disable --now sd-worker.service") == true)
        XCTAssertNil(w.engine.running["vps"])
    }

    /// On a Linux machine whose admin is the worker account, the Running switch is refused like any admin step.
    func testToggleRunningOnLinuxAsTheWorkerIsRefusedBeforeAnyCommandRuns() async {
        let w = world([TestMachines.linux("vps")]) { _, _ in [
            step("sd-check", title: "sd check"),
            step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: true)),
        ] }
        await w.engine.refresh(["vps"])
        let before = w.runner.calls.count
        for on in [true, false] {
            let out = await w.engine.toggleRunning(on: "vps", to: on, approval: admin(password: "hunter2-Pw!"))
            XCTAssertFalse(out.ok)
            XCTAssertTrue(out.message.hasPrefix("On Linux the worker account can't be the admin"), out.message)
        }
        XCTAssertEqual(w.runner.calls.count, before, "nothing ran")
    }

    func testToggleRunningOnAMacUsesLaunchctlWithoutAdmin() async {
        let w = world { _, _ in [step("sd-check"), step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: false))] }
        await w.engine.refresh(["m1"])
        let on = await w.engine.toggleRunning(on: "m1", to: true, approval: admin())
        XCTAssertTrue(on.ok, on.message)
        XCTAssertTrue(w.runner.calls.contains { $0.argv.last?.contains("launchctl bootstrap") == true })
    }

    /// The engine's default Running switch is Task 9's, not a copy: the exact scripts, on a Mac and on Linux.
    func testTheEnginesRunningSwitchUsesTheSDWorkerScripts() async {
        let mac = TestMachines.mac("m1"), vps = TestMachines.linux("vps")
        let w = world([mac]) { _, _ in [
            step("sd-check"), step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: false)),
        ] }
        await w.engine.refresh()
        for on in [true, false] {
            _ = await w.engine.toggleRunning(on: "m1", to: on, approval: admin())
            XCTAssertTrue(w.runner.calls.contains { $0.argv == SSH.script(mac, SDWorkerSteps.toggleScript(linux: false, on: on)) },
                          "Mac, on: \(on)")
        }
        // a Linux machine's switch needs admin, which there is root over ssh; the step says so
        var root = vps
        root.admin = .rootSSH
        let v = world([root]) { _, _ in [
            step("sd-check"), step("running", needs: ["sd-check"], fix: .toggle(needsAdmin: true)),
        ] }
        await v.engine.refresh()
        for on in [true, false] {
            _ = await v.engine.toggleRunning(on: "vps", to: on, approval: admin(root: true))
            XCTAssertTrue(v.runner.calls.contains {
                $0.argv == SSH.admin(root, SDWorkerSteps.toggleScript(linux: true, on: on), asRoot: true)
            }, "Linux, on: \(on)")
        }
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

    // MARK: I1 / I2: the Running switch

    /// I1: the real Running step shows its own state whatever the steps before it say, so a worker that runs while
    /// Plugins installed fails can be seen and turned off.
    func testTheRealRunningStepAnswersWhileAnEarlierStepFails() async throws {
        let mac = TestMachines.mac("m1")
        let real = try XCTUnwrap(StepCatalog.steps(for: mac, all: [mac]).first { $0.id == .running })
        XCTAssertEqual(real.needs, [.reachable], "only Reachable is needed to see the state")
        // the real step's needs and Fix, with a probe the fake machine answers
        let running = SetupStep(id: real.id, title: real.title, needs: real.needs, cadence: real.cadence,
                                probe: .remote(script: "echo probe-running", parse: { out, _ in
                                    CheckResult(state: out.code == 0 ? .pass("on") : .fail("off"))
                                }), fix: real.fix)
        let w = world([mac]) { _, _ in [step("reachable"), step("plugins", title: "Plugins installed"), running] }
        w.answers.value = ["echo probe-plugins": ("bad", 1)]
        await w.engine.refresh(["m1"], includeSlow: true)
        let results = try XCTUnwrap(w.engine.results["m1"])
        XCTAssertEqual(results[.plugins]?.state, .fail("bad"))
        XCTAssertEqual(results[.running]?.state, .pass("on"), "Running reads the machine, not the steps before it")
    }

    /// I1: with Plugins installed failing, the state is on and turning it off is possible; off runs the off script.
    func testTheWorkerCanBeTurnedOffWhileAnEarlierStepFails() async {
        let w = world { _, _ in [
            step("plugins", title: "Plugins installed"),
            step("running", fix: .toggle(needsAdmin: false)),
        ] }
        w.answers.value = ["echo probe-plugins": ("bad", 1)]
        await w.engine.refresh(["m1"])
        XCTAssertEqual(w.engine.results["m1"]?[StepID(raw: "running")]?.state, .pass("ok"))
        XCTAssertEqual(w.engine.turnOnBlocker(for: "m1"), "Plugins installed")
        let off = await w.engine.toggleRunning(on: "m1", to: false, approval: admin())
        XCTAssertTrue(off.ok, off.message)
        XCTAssertTrue(w.runner.calls.contains { $0.argv == SSH.script(TestMachines.mac("m1"), SDWorkerSteps.toggleScript(linux: false, on: false)) })
    }

    /// I2: Token scope ✗ (the token reaches 31 other repos) blocks turning the worker on, and names the step.
    func testTurningTheWorkerOnWaitsForTheFirstStepThatIsNotOK() async {
        let w = world { _, _ in [
            step("clone", title: "Clone"),
            step("token-scope", title: "Token scope", cadence: .slow),
            step("push", title: "Push is refused"),
            step("running", fix: .toggle(needsAdmin: false)),
        ] }
        w.answers.value = ["echo probe-token-scope": ("token reaches 31 other private repos", 1), "echo probe-push": ("bad", 1)]
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertEqual(w.engine.turnOnBlocker(for: "m1"), "Token scope")
        let on = await w.engine.toggleRunning(on: "m1", to: true, approval: admin())
        XCTAssertEqual(on, FixOutcome(ok: false, message: "waits for Token scope"))
        XCTAssertFalse(w.runner.calls.contains { $0.argv.last?.contains("launchctl") == true }, "nothing ran")

        // once those pass, turning it on is allowed
        w.answers.value = [:]
        await w.engine.refresh(["m1"], includeSlow: true)
        XCTAssertNil(w.engine.turnOnBlocker(for: "m1"))
        let again = await w.engine.toggleRunning(on: "m1", to: true, approval: admin())
        XCTAssertTrue(again.ok, again.message)
    }

    /// I2: a push GitHub accepted blocks turning the worker on too.
    func testAPushThatIsNotRefusedBlocksTurningTheWorkerOn() async {
        let repo = TestMachines.repo
        let w = world { _, _ in [
            step("push-refused:\(repo)", title: "Push is refused (\(repo))", cadence: .slow),
            step("running", fix: .toggle(needsAdmin: false)),
        ] }
        w.answers.value = ["echo probe-push-refused:\(repo)": ("this machine can push", 1)]
        await w.engine.refresh(["m1"], includeSlow: true)
        let on = await w.engine.toggleRunning(on: "m1", to: true, approval: admin())
        XCTAssertEqual(on, FixOutcome(ok: false, message: "waits for Push is refused (\(repo))"))
    }

    /// I2: Set up all stops at Token scope. Its Fix only opens the page and the form, and says so with ok:false.
    func testSetUpAllStopsAtTokenScope() async throws {
        let m = TestMachines.mac("m1")
        let job = try XCTUnwrap(m.sdWorker)
        let opened = Counter()
        let real = TokenScope.step(for: m, job: job, open: { _ in opened.add("page") }, form: { _ in opened.add("form") })
        let after = Counter()
        let w = world([m]) { _, _ in [
            step("before", title: "Before", fix: runFix(opened, "before")),
            SetupStep(id: .tokenScope, title: "Token scope", needs: [], cadence: .everyRefresh,
                      probe: .remote(script: "echo probe-token-scope", parse: { _, _ in CheckResult(state: .fail("token reaches 31 other private repos")) }),
                      fix: real.fix),
            step("after", title: "After", fix: runFix(after, "after")),
        ] }
        w.answers.value = ["echo probe-before": ("bad", 1), "echo probe-after": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let plan = w.engine.setUpAllPlan(for: "m1")
        XCTAssertEqual(plan.map(\.title), ["Before", "Token scope", "After"])
        let out = await w.engine.setUpAll(on: "m1", approved: plan) { _ in .cancelled }
        XCTAssertFalse(out.ok)
        XCTAssertTrue(out.message.hasPrefix("Token scope: "), out.message)
        XCTAssertTrue(out.message.contains("Check again"), out.message)
        XCTAssertEqual(opened.all, ["before", "page", "form"])
        XCTAssertEqual(after.count, 0, "the steps after Token scope did not run")
    }

    /// M4: Set up all ends with the steps that are still not ✓, so it never claims a clean finish over a step that waits.
    func testSetUpAllEndsWithTheStepsStillNotOK() async {
        let done = Counter()
        let w = world { _, _ in [
            step("a", title: "A", fix: runFix(done, "a"), parse: { _, _ in
                CheckResult(state: done.all.contains("a") ? .pass("ok") : .fail("no"))
            }),
            step("hand", title: "By hand"),
            step("waits", title: "Waits", needs: ["hand"], fix: runFix(done, "waits")),
            step("running", fix: .toggle(needsAdmin: false)),
        ] }
        w.answers.value = ["echo probe-hand": ("bad", 1)]
        await w.engine.refresh(["m1"])
        let plan = w.engine.setUpAllPlan(for: "m1")
        XCTAssertEqual(plan.map(\.title), ["A", "Waits"])
        let out = await w.engine.setUpAll(on: "m1", approved: plan) { _ in .cancelled }
        XCTAssertEqual(done.all, ["a"], "Waits had to wait for By hand")
        XCTAssertEqual(out, FixOutcome(ok: false, message: "1 step set up; still not ✓: By hand, Waits"))

        // everything ✓: the plain count
        let clean = world { _, _ in [step("a", title: "A", fix: runFix(done, "a2"), parse: { _, _ in
            CheckResult(state: done.all.contains("a2") ? .pass("ok") : .fail("no"))
        })] }
        await clean.engine.refresh(["m1"])
        let ok = await clean.engine.setUpAll(on: "m1", approved: clean.engine.setUpAllPlan(for: "m1")) { _ in .cancelled }
        XCTAssertEqual(ok, FixOutcome(ok: true, message: "1 step set up"))
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

    // MARK: fix round 1 (integration)

    /// I1: the engine fills `StepContext.mask` for the checks. A probe that masks through `ctx.mask` and cuts after
    /// it shows no piece of the token the machine echoed; with an identity mask the cut leaves a fragment that the
    /// engine's own `masks.apply` at publish can no longer match.
    func testTheEnginesCheckContextMasksBeforeAProbeCuts() async {
        let marker = "[secret:claude-worker]"
        let echoed = String(repeating: "f", count: 170) + longToken
        let shown: @Sendable (String, StepContext) -> String = { text, ctx in String(ctx.mask(text).prefix(200)) }
        let w = world(secrets: FakeSecrets(values: ["claude-worker": longToken])) { _, _ in [
            step("remote", parse: { out, ctx in CheckResult(state: .fail(shown(out.text, ctx))) }),
            SetupStep(id: StepID(raw: "local"), title: "Local", needs: [], cadence: .everyRefresh,
                      probe: .local { ctx in CheckResult(state: .fail(shown(echoed, ctx))) }, fix: .howTo("x")),
        ] }
        w.answers.value = ["echo probe-remote": (echoed, 1)]
        await w.engine.refresh(["m1"])
        for id in ["remote", "local"] {
            let reason = text(w.engine.results["m1"]?[StepID(raw: id)]?.state) ?? "no result"
            noFragment(of: longToken, in: reason)
            XCTAssertTrue(reason.contains(marker), "\(id): \(reason)")
        }

        // the draft check (the Add sheet) builds its contexts the same way
        let draft = await w.engine.check(draft: TestMachines.mac("m1"))
        for id in ["remote", "local"] {
            let reason = text(draft[StepID(raw: id)]?.state) ?? "no result"
            noFragment(of: longToken, in: reason)
            XCTAssertTrue(reason.contains(marker), "draft \(id): \(reason)")
        }
    }

    /// A refresh reads each vault value once, also when the facts it learns start a second pass; so does a Fix.
    func testARefreshAndAFixReadTheVaultOnce() async {
        var m = TestMachines.mac("m1")
        m.os = nil
        let secrets = CountingSecrets(["claude-worker": longToken])
        let seen = Box<[Machine]>([])
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: false) { ctx in
            _ = ctx.mask("a"); _ = ctx.step.mask("b")
            return FixOutcome(ok: true, message: "done")
        })
        let w = world([m], secrets: secrets) { _, _ in [
            step("reachable", fix: fix, parse: { _, ctx in
                seen.value.append(ctx.machine)
                return CheckResult(state: .fail("up"), detected: Detected(os: .macOS))
            }),
        ] }
        await w.engine.refresh(["m1"])
        XCTAssertEqual(seen.value.count, 2, "the learned OS starts a second pass")
        XCTAssertEqual(secrets.reads(of: "claude-worker"), 1, "one read for both passes and the published results")

        secrets.resetReads()
        _ = await w.engine.check(draft: TestMachines.mac("m1"))
        XCTAssertEqual(secrets.reads(of: "claude-worker"), 1, "the Add sheet's check")

        // a Fix: its masks, its step context, and the refresh after it
        secrets.resetReads()
        let out = await w.engine.runFix(StepID(raw: "reachable"), on: "m1", approval: admin())
        XCTAssertTrue(out.ok, out.message)
        XCTAssertEqual(secrets.reads(of: "claude-worker"), 2, "one for the Fix and one for the refresh after it")
    }

    /// I1: a Fix's step context hides the vault values too, and the admin password of that Fix.
    func testTheEnginesFixContextMasksTheStepAndTheFixAlike() async {
        let marker = "[secret:claude-worker]"
        let seen = Box<[String]>([])
        let fix = StepFix.run(FixPlan(display: "x", needsAdmin: true) { ctx in
            let said = String(repeating: "f", count: 170) + self.longToken + " pw=hunter22"
            seen.value = [ctx.mask(said), ctx.step.mask(said)]
            return FixOutcome(ok: false, message: String(ctx.step.mask(said).prefix(200)))
        })
        let w = world(secrets: FakeSecrets(values: ["claude-worker": longToken])) { _, _ in [step("a", fix: fix)] }
        let out = await w.engine.runFix(StepID(raw: "a"), on: "m1", approval: admin(password: "hunter22"))
        XCTAssertEqual(seen.value.count, 2)
        for masked in seen.value {
            noFragment(of: longToken, in: masked)
            XCTAssertTrue(masked.contains(marker), masked)
            XCTAssertFalse(masked.contains("hunter22"), masked)
            XCTAssertTrue(masked.contains("[admin password]"), masked)
        }
        noFragment(of: longToken, in: out.message)
        XCTAssertTrue(out.message.contains(marker), out.message)
    }
}

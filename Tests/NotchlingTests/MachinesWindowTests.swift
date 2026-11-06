import CryptoKit
import XCTest
@testable import Notchling

/// The pure parts of the Machines window: what a header chip says and what a new question's peek says.
final class MachinesWindowTests: XCTestCase {
    private let now = Date(timeIntervalSince1970: 1_800_000_000)
    private let m1 = Machine(name: "m1", host: "m1", user: "u", sdPath: "/sd.py", fileVault: true, repos: [])

    private func fixtureStatus() throws -> WorkerStatus {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "worker-status", withExtension: "json", subdirectory: "Fixtures"))
        return try XCTUnwrap(WorkerStatus.decode(try Data(contentsOf: url)))
    }

    func testChipShowsOnlineWithJobsWeekAndCurrentJob() throws {
        let report = LastReport(status: try fixtureStatus(), at: now)
        let chip = MachineChip.make(machine: m1, report: report, previous: nil, now: now)
        XCTAssertEqual(chip.word, "online")
        XCTAssertEqual(chip.tone, .good)
        XCTAssertEqual(chip.details, ["3 jobs tonight", "week 43%", "now dtthaison/lingeva-mono #214 · implementation"])
        XCTAssertNil(chip.error)
    }

    func testChipWarnsWhenTheTokenIsNearExpiry() throws {
        // the fixture's token has 14 days left
        let chip = MachineChip.make(machine: m1, report: LastReport(status: try fixtureStatus(), at: now), previous: nil, now: now)
        XCTAssertEqual(chip.tokenWarning, "GitHub token on vps-sing-1 expires in 14 days")
    }

    func testChipForAMachineNotPolledYet() {
        let chip = MachineChip.make(machine: m1, report: nil, previous: nil, now: now)
        XCTAssertEqual(chip.word, "checking…")
        XCTAssertEqual(chip.tone, .neutral)
    }

    func testChipForAFailedPollIsOfflineWithTheError() {
        let chip = MachineChip.make(machine: m1, report: LastReport(at: now, error: "timed out"), previous: nil, now: now)
        XCTAssertEqual(chip.word, "offline")
        XCTAssertEqual(chip.tone, .bad)
        XCTAssertEqual(chip.error, "timed out")
        XCTAssertTrue(chip.details.isEmpty)
    }

    func testChipForAPausedMachineIsAWarning() throws {
        var status = try fixtureStatus()
        status.paused = "production health check failed"
        let chip = MachineChip.make(machine: m1, report: LastReport(status: status, at: now), previous: nil, now: now)
        XCTAssertEqual(chip.word, MachineHealth.paused)
        XCTAssertEqual(chip.tone, .warn)
    }

    func testPeekTextPrefersTheQuestionThenTheReason() {
        XCTAssertEqual(MachineAlert.text(machine: "m1", entry: .init(issue: 207, reason: "asked", question: "Cap at 50 or 100?")),
                       "m1 #207: Cap at 50 or 100?")
        XCTAssertEqual(MachineAlert.text(machine: "m1", entry: .init(issue: 230, reason: "triage: needs a design call")),
                       "m1 #230: triage: needs a design call")
        XCTAssertEqual(MachineAlert.text(machine: "m1", entry: .init(issue: 5)), "m1 #5: needs you")
    }

    // MARK: Fields

    /// On macOS `TextField(onCommit:)` also fires when focus merely moves away, which posted a half-typed answer.
    /// A hosted-field check needs a window server; this scans the sources instead.
    func testNoFieldInMachinesPostsOnCommit() throws {
        let sources = URL(fileURLWithPath: #filePath).deletingLastPathComponent().deletingLastPathComponent()
            .deletingLastPathComponent().appendingPathComponent("Sources/Notchling/Machines")
        let files = (try? FileManager.default.contentsOfDirectory(atPath: sources.path)) ?? []
        try XCTSkipIf(files.isEmpty, "the sources are not next to the tests")
        for file in files where file.hasSuffix(".swift") {
            let code = try String(contentsOf: sources.appendingPathComponent(file), encoding: .utf8)
                .split(separator: "\n", omittingEmptySubsequences: false)
                .filter { !$0.trimmingCharacters(in: .whitespaces).hasPrefix("//") }
                .joined(separator: "\n")
            XCTAssertFalse(code.contains("onCommit"), "\(file) uses onCommit")
        }
        let window = try String(contentsOf: sources.appendingPathComponent("MachinesWindow.swift"), encoding: .utf8)
        XCTAssertEqual(window.components(separatedBy: ".onSubmit(onSubmit)").count - 1, 1, "the note field submits on Return only")
    }

    // MARK: Add form

    func testTheAddFormRefusesUnsafeNamesHostsAndUsers() {
        let nameProblem = "Name: lowercase letters, digits and hyphens"
        let hostProblem = "Host: a Tailscale name or an address"
        let userProblem = "User: lowercase letters, digits, hyphens and underscores"
        XCTAssertEqual(MachineForm.problems(name: "M1", host: "m1", user: "u", existing: []), [nameProblem])
        XCTAssertEqual(MachineForm.problems(name: "m1", host: "m1", user: "u", existing: ["m1"]), ["m1 is already a machine"])
        XCTAssertEqual(MachineForm.problems(name: "M1", host: "m1", user: "u", existing: ["m1"]), [nameProblem])
        XCTAssertEqual(MachineForm.problems(name: "", host: "m1", user: "u", existing: []), [nameProblem])
        XCTAssertEqual(MachineForm.problems(name: "-m", host: "m1", user: "u", existing: []), [nameProblem])
        XCTAssertEqual(MachineForm.problems(name: "m1\n", host: "m1", user: "u", existing: []), [nameProblem])
        for host in ["-oProxyCommand=x", "a b", "h;x", "", "h\n", "a$(x)", ".a", "a/b"] {
            XCTAssertEqual(MachineForm.problems(name: "m2", host: host, user: "u", existing: []), [hostProblem], host)
        }
        for host in ["m2.tail", "m2", "100.64.0.7", "fd7a:115c:a1e0::1", "vps-sing-1.tail1234.ts.net"] {
            XCTAssertEqual(MachineForm.problems(name: "m2", host: host, user: "u", existing: []), [], host)
        }
        for user in ["root;x", "", "Root", "-x", "a b", "u\n"] {
            XCTAssertEqual(MachineForm.problems(name: "m2", host: "m2", user: user, existing: []), [userProblem], user)
        }
        XCTAssertEqual(MachineForm.problems(name: "m2", host: "m2.tail", user: "sondoan", existing: ["m1"]), [])
        XCTAssertEqual(MachineForm.problems(name: "M", host: "-x", user: "", existing: []), [nameProblem, hostProblem, userProblem])
    }

    // MARK: Text helpers

    func testAbilitiesLine() {
        let mac = MachineAbilities(arch: "arm64", osVersion: "26.1", xcode: "26.1", iosRuntimes: ["iOS 26.1"],
                                   simulators: ["iPhone 17", "iPhone 17 Pro"], docker: .rootless, freeDiskGB: 120)
        XCTAssertEqual(AbilitiesLine.text(mac, os: .macOS),
                       "macOS 26.1 · arm64 · Xcode 26.1 · 2 simulators · Docker rootless · 120 GB free")
        let linux = MachineAbilities(arch: "x86_64", osVersion: "Ubuntu 24.04.1 LTS", docker: DockerState.none, freeDiskGB: 80)
        XCTAssertEqual(AbilitiesLine.text(linux, os: .linux), "Ubuntu 24.04.1 LTS · x86_64 · no Docker · 80 GB free")
        XCTAssertEqual(AbilitiesLine.text(MachineAbilities(simulators: ["iPhone 17"], docker: .stopped), os: .macOS),
                       "macOS · 1 simulator · Docker stopped")
        XCTAssertEqual(AbilitiesLine.text(MachineAbilities(), os: .linux), "Linux")
        XCTAssertEqual(AbilitiesLine.text(nil, os: nil), "not checked yet")
    }

    func testJobCardShowsHealthUnroutedOutboxAndTestFlight() throws {
        var status = try fixtureStatus()
        status.paused = "production health check failed"
        status.outboxFailed = 2
        status.unrouted = ["me/x#3"]
        status.testflight = WorkerStatus.TestFlightStatus(quiet: true)
        XCTAssertEqual(JobFacts.lines(status), [
            "3 jobs tonight", "week 43%", "now dtthaison/lingeva-mono #214 · implementation",
            "paused: production health check failed",
            "health github: error: 1 outbox items failed",
            "unrouted: me/x#3",
            "outbox: 2 failed",
            "TestFlight: no new feedback in over 7 days",
        ])
        // a healthy worker adds nothing to the chip's details
        var calm = status
        calm.paused = nil; calm.health = ["github": "ok", "claude": "ok"]; calm.outboxFailed = 0
        calm.unrouted = []; calm.testflight = nil
        XCTAssertEqual(JobFacts.lines(calm), ["3 jobs tonight", "week 43%", "now dtthaison/lingeva-mono #214 · implementation"])
        calm.health = ["host": "paused: load", "claude": "limit"]
        XCTAssertEqual(Array(JobFacts.lines(calm).suffix(2)), ["health claude: limit", "health host: paused: load"])
    }

    func testUpdateOfferTitle() {
        XCTAssertEqual(UpdateOffer.title(count: 2), "Update 2 machines")
        XCTAssertEqual(UpdateOffer.title(count: 1), "Update 1 machine")
    }

    func testSecretRowText() {
        let usage = SecretSteps.claudeUsage(for: TestMachines.mac())
        let entry = VaultEntry(name: "claude-worker", kind: .token, expires: "2026-11-18")
        let now = Date(timeIntervalSince1970: 1_793_966_400)   // 2026-11-06 12:00 UTC
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        let facts = CheckResult(state: .pass("mode 600, 41 bytes"))
        XCTAssertEqual(SecretRowFacts.text(usage: usage, entry: entry, facts: facts, now: now, calendar: calendar),
                       "claude-worker → ~/.config/agents/claude.env · mode 600, 41 bytes · expires in 12 days")
        XCTAssertEqual(SecretRowFacts.text(usage: usage, entry: entry, facts: CheckResult(state: .fail("not on the machine")), now: now, calendar: calendar),
                       "claude-worker → ~/.config/agents/claude.env · not on the machine · expires in 12 days")
        XCTAssertEqual(SecretRowFacts.text(usage: usage, entry: nil, facts: nil, now: now, calendar: calendar),
                       "claude-worker → ~/.config/agents/claude.env · not in the vault")
        var past = entry
        past.expires = "2026-11-01"
        XCTAssertEqual(SecretRowFacts.text(usage: usage, entry: past, facts: facts, now: now, calendar: calendar),
                       "claude-worker → ~/.config/agents/claude.env · mode 600, 41 bytes · expired 2026-11-01")
    }

    // MARK: Banners

    @MainActor
    private func makeModel(_ reg: TestRegistry, runner: CommandRunning = ScriptedRunner(), vault: [VaultEntry] = [],
                           steps: ((Machine, [Machine]) -> [SetupStep])? = nil,
                           rename: @escaping (VaultEntry, String) throws -> Void = { _, _ in }) -> MachinesModel {
        let engine: SetupEngine
        if let steps {
            engine = SetupEngine(runner: runner, secrets: FakeSecrets(), http: FakeHTTP(), mac: { .empty },
                                 machines: { reg.machines }, save: reg.save, clock: { Date(timeIntervalSince1970: 1_800_000_000) },
                                 steps: steps, observeVault: false)
        } else {
            engine = SetupEngine(runner: runner, secrets: FakeSecrets(), http: FakeHTTP(), mac: { .empty },
                                 machines: { reg.machines }, save: reg.save, clock: { Date(timeIntervalSince1970: 1_800_000_000) },
                                 observeVault: false)
        }
        let poller = MachinePoller(runner: runner, machines: { reg.machines }, clock: { Date() }, observeSleep: false)
        return MachinesModel(poller: poller, actions: MorningActions(runner: runner), engine: engine,
                             registry: { reg.state }, save: reg.save, vaultEntries: { vault }, rename: rename,
                             renew: { _ in }, registryPath: "/tmp/notchling-test/machines.json", runner: runner)
    }

    @MainActor
    func testUnreadableRegistryDisablesAddAndShowsTheReason() {
        let model = makeModel(TestRegistry(.unreadable("version 3 is not supported")))
        XCTAssertFalse(model.canAdd)
        XCTAssertEqual(model.unreadableNote,
                       "machines.json can't be read: version 3 is not supported. Notchling won't change it. Fix or move it, then Check again.")
        XCTAssertEqual(model.registryPath, "/tmp/notchling-test/machines.json")
        XCTAssertTrue(model.machines.isEmpty)
        XCTAssertEqual(model.add(name: "m2", host: "m2", user: "u", check: DraftCheck(reachable: true)),
                       "machines.json can't be read, so nothing can be added")

        let fine = makeModel(TestRegistry(.missing))
        XCTAssertTrue(fine.canAdd)
        XCTAssertNil(fine.unreadableNote)
    }

    func testMigrationNoteNamesTheBackup() {
        XCTAssertEqual(RegistryNotes.migrated(backup: "/x/machines.json.v1-backup"),
                       "machines.json was upgraded to the new format. The old file is kept at /x/machines.json.v1-backup.")
    }

    @MainActor
    func testTheModelShowsTheMigrationNoteOnlyForTheLoadThatMigrated() {
        let reg = TestRegistry(.loaded([TestMachines.mac()], migrated: true))
        let model = makeModel(reg)
        XCTAssertEqual(model.migrationNote, RegistryNotes.migrated(backup: MachineRegistry.backupURL(for: MachineRegistry.fileURL).path))
        model.dismissMigrationNote()
        XCTAssertNil(model.migrationNote)
        XCTAssertNil(makeModel(TestRegistry(machines: [TestMachines.mac()])).migrationNote)
    }

    func testRenameBannerOnlyForLegacyNamesWithAFreeTarget() {
        func entry(_ name: String) -> VaultEntry { VaultEntry(name: name, kind: .token) }
        let all = [entry("sd-worker-m1"), entry("sd-worker-vps-sing-1"), entry("sd-claude")]
        XCTAssertEqual(RegistryNotes.rename(SecretNames.pendingRenames(in: all)),
                       "Rename sd-worker-m1, sd-worker-vps-sing-1 and sd-claude to the new names")
        // a target that is taken is left alone
        let taken = all + [entry("vps-sing-1-github-issues")]
        XCTAssertEqual(RegistryNotes.rename(SecretNames.pendingRenames(in: taken)),
                       "Rename sd-worker-m1 and sd-claude to the new names")
        XCTAssertEqual(RegistryNotes.rename(SecretNames.pendingRenames(in: [entry("sd-claude")])),
                       "Rename sd-claude to the new name")
        XCTAssertNil(RegistryNotes.rename(SecretNames.pendingRenames(in: [entry("m1-github-issues"), entry("claude-worker")])))
        XCTAssertNil(RegistryNotes.rename(SecretNames.pendingRenames(in: [entry("sd-claude"), entry("claude-worker")])))
    }

    @MainActor
    func testRenameRunsOnlyOnTheClickAndAFailureSaysTryAgain() {
        let vault = [VaultEntry(name: "sd-claude", kind: .token), VaultEntry(name: "sd-worker-m1", kind: .token)]
        var renamed: [String] = []
        var fail = true
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), vault: vault, rename: { entry, to in
            if fail { throw AccountError("boom") }
            renamed.append("\(entry.name)>\(to)")
        })
        XCTAssertEqual(model.renameBanner, "Rename sd-worker-m1 and sd-claude to the new names")
        XCTAssertEqual(renamed, [], "showing the banner renames nothing")
        model.renameSecrets()
        XCTAssertEqual(model.renameNote, "Rename didn't finish; try again")
        fail = false
        model.renameSecrets()
        XCTAssertNil(model.renameNote)
        XCTAssertEqual(Set(renamed), ["sd-claude>claude-worker", "sd-worker-m1>m1-github-issues"])
    }

    // MARK: Cadence: checks run on their own, Fixes only on a click

    private let forbidden = ["cat > ", "sudo -S", "deploy-key add", "git clone", "systemctl enable", "launchctl bootstrap",
                             "pmset -a", "apt-get", "brew install", "install.sh"]

    @MainActor
    func testOpeningTheWindowAndAddingAMachineRunNoFix() async throws {
        let runner = ReachableRunner(reachable: "os=Darwin\narch=arm64\nuid=501\nuser=sondoan\nhome=/Users/sondoan\nversion=26.1\ndisk_kb=125829120\ndocker=none")
        let reg = TestRegistry(machines: [TestMachines.mac("m1")])
        let model = makeModel(reg, runner: runner)

        await model.open()
        let draft = await model.checkDraft(name: "m2", host: "m2.tail", user: "sondoan")
        XCTAssertTrue(draft.reachable)
        XCTAssertEqual(draft.os, .macOS)
        XCTAssertEqual(draft.lines.first?.title, "Reachable")
        XCTAssertEqual(draft.abilitiesLine, "macOS 26.1 · arm64 · no Docker · 120 GB free")
        XCTAssertNil(model.add(name: "m2", host: "m2.tail", user: "sondoan", check: draft))
        XCTAssertEqual(reg.machines.map(\.name), ["m1", "m2"])
        let added = try XCTUnwrap(reg.machines.last)
        XCTAssertEqual(added.os, .macOS)
        XCTAssertEqual(added.abilities?.arch, "arm64")
        XCTAssertEqual(added.abilities?.home, "/Users/sondoan")
        XCTAssertTrue(added.jobs.isEmpty)
        await model.refreshAll()
        await model.check("m2")

        let calls = runner.calls
        XCTAssertFalse(calls.isEmpty, "the checks did run")
        for call in calls {
            let text = call.argv.joined(separator: " ")
            for fragment in forbidden { XCTAssertFalse(text.contains(fragment), "a Fix ran without a click: \(fragment)") }
            XCTAssertNil(call.stdin, "nothing was sent to a machine")
        }
        XCTAssertTrue(model.engine.running.isEmpty)
        XCTAssertTrue(model.engine.logs.values.allSatisfy { $0.isEmpty }, "no Fix logged anything")
    }

    @MainActor
    func testTheAddSheetCannotAddWhatDidNotPassReachableOrIsUnsafe() async {
        let reg = TestRegistry(machines: [TestMachines.mac("m1")])
        let model = makeModel(reg, runner: ReachableRunner(reachable: ""))
        let down = await model.checkDraft(name: "m2", host: "m2", user: "u")
        XCTAssertFalse(down.reachable)
        XCTAssertEqual(model.add(name: "m2", host: "m2", user: "u", check: down), "Check Reachable first")
        let up = DraftCheck(reachable: true)
        XCTAssertEqual(model.add(name: "m1", host: "m2", user: "u", check: up), "m1 is already a machine")
        XCTAssertEqual(model.add(name: "m2", host: "-oProxyCommand=x", user: "u", check: up), "Host: a Tailscale name or an address")
        XCTAssertEqual(reg.saves.count, 0)
        // a draft with a bad host is not even checked
        let bad = await model.checkDraft(name: "m2", host: "-oProxyCommand=x", user: "u")
        XCTAssertFalse(bad.reachable)
        XCTAssertEqual(bad.problems, ["Host: a Tailscale name or an address"])
    }

    @MainActor
    func testWindowOpensWithSlowChecksAndPollerChangesRefreshOnlyWhileVisible() async throws {
        let reg = TestRegistry(machines: [TestMachines.linux()])
        let runner = ReachableRunner(reachable: "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=1048576\ndocker=none")
        let model = makeModel(reg, runner: runner)
        XCTAssertTrue(runner.calls.isEmpty, "making the model checks nothing")
        await model.open()
        XCTAssertFalse(runner.calls.isEmpty)
        XCTAssertTrue(model.windowVisible)
        model.windowClosed()
        XCTAssertFalse(model.windowVisible)
        XCTAssertFalse(model.shouldRefreshOnPollerChange)
        model.windowOpened()
        XCTAssertTrue(model.shouldRefreshOnPollerChange)
    }

    // MARK: The Fix flow: a Fix needs a click, the password is passed through

    private func adminStep(onRun: @escaping @Sendable (FixContext) async -> FixOutcome = { ctx in
        let r = await MachineRun.admin("echo hi", ctx: ctx)
        return FixOutcome(ok: r.code == 0, message: r.code == 0 ? "done" : "failed \(r.code)")
    }) -> SetupStep {
        SetupStep(id: .staysAwake, title: "Stays awake", needs: [], cadence: .everyRefresh,
                  probe: .local { _ in CheckResult(state: .fail("sleeps")) },
                  fix: .run(FixPlan(display: "sudo pmset -a sleep 0", needsAdmin: true, run: onRun)))
    }

    /// The calls that went to a machine (the window also asks `gh` about issues when it opens).
    private func ssh(_ runner: ScriptedRunner) -> [RecordedCall] { runner.calls.filter { $0.argv.first == "/usr/bin/ssh" } }

    private func plainStep(_ id: StepID, title: String, ok: Bool = true, message: String = "done") -> SetupStep {
        SetupStep(id: id, title: title, needs: [], cadence: .everyRefresh,
                  probe: .local { _ in CheckResult(state: .fail("not yet")) },
                  fix: .run(FixPlan(display: "do \(title)", needsAdmin: false) { _ in FixOutcome(ok: ok, message: message) }))
    }

    @MainActor
    func testAdminPasswordGoesThroughTheModelWithoutBeingKept() async throws {
        let password = "hunter2-correct-horse"
        let runner = SpyRunner(password: password)
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), runner: runner, steps: { _, _ in [self.adminStep()] })
        await model.open()

        model.requestSetUp(.staysAwake, on: "m1")
        guard case .fix(let prompt)? = model.setupSheet else { return XCTFail("no confirmation sheet") }
        XCTAssertEqual(prompt.title, "Stays awake")
        XCTAssertEqual(prompt.display, "sudo pmset -a sleep 0")
        XCTAssertTrue(prompt.needsAdmin)
        XCTAssertFalse(prompt.root)
        XCTAssertEqual(runner.sshArgvs.count, 0, "asking is not running")

        await model.confirm(prompt, password: password).value
        let sudo = runner.sshArgvs.indices.filter { runner.sshArgvs[$0].last?.contains("sudo -S") == true }
        XCTAssertEqual(sudo.count, 1)
        XCTAssertEqual(sudo.map { runner.stdinWasThePassword[$0] }, [true], "the password is sudo's stdin")
        XCTAssertEqual(runner.stdinWasThePassword.filter { $0 }.count, 1, "and nothing else carries it")
        XCTAssertNil(model.setupSheet)
        XCTAssertEqual(model.outcomes["m1"]?.ok, true)

        var seen = Set<ObjectIdentifier>()
        XCTAssertFalse(holds(password, in: model, seen: &seen), "the view model keeps the password")
        seen = []
        XCTAssertFalse(holds(password, in: model.engine, seen: &seen), "the engine keeps the password")
        for argv in runner.sshArgvs { XCTAssertFalse(argv.joined(separator: " ").contains(password)) }
        XCTAssertFalse(model.engine.logs.values.joined().joined().contains(password))
    }

    func testTheSecretKeepingCheckCanTellWhenAPasswordIsKept() {
        // the control for the test above: the helper does find a value held in a published property
        final class Holder { @Published var note: String? = nil; var list: [Int?: String] = [:] }
        let h = Holder()
        h.note = "the-password"
        var seen = Set<ObjectIdentifier>()
        XCTAssertTrue(holds("the-password", in: h, seen: &seen))
        seen = []
        h.note = nil
        h.list[3] = "xx the-password xx"
        XCTAssertTrue(holds("the-password", in: h, seen: &seen))
        seen = []
        h.list = [:]
        XCTAssertFalse(holds("the-password", in: h, seen: &seen))
    }

    @MainActor
    func testASudoFixWithoutAPasswordIsRefusedAndRootNeedsTheWarningAccepted() async throws {
        let runner = ScriptedRunner()
        var root = TestMachines.linux()
        root.admin = .rootSSH
        let model = makeModel(TestRegistry(machines: [TestMachines.mac(), root]), runner: runner,
                              steps: { _, _ in [self.adminStep()] })
        await model.open()

        model.requestSetUp(.staysAwake, on: "m1")
        guard case .fix(let sudoPrompt)? = model.setupSheet else { return XCTFail("no sheet") }
        await model.confirm(sudoPrompt, password: nil).value
        XCTAssertEqual(model.outcomes["m1"]?.ok, false)
        XCTAssertEqual(model.outcomes["m1"]?.text, "needs the admin password")
        XCTAssertTrue(ssh(runner).isEmpty)

        model.requestSetUp(.staysAwake, on: "vps-sing-1")
        guard case .fix(let rootPrompt)? = model.setupSheet else { return XCTFail("no sheet") }
        XCTAssertTrue(rootPrompt.root)
        XCTAssertEqual(rootPrompt.warning, "Admin steps run as root over SSH on vps-sing-1.")
        await model.confirm(rootPrompt, password: "not-used").value
        XCTAssertEqual(model.outcomes["vps-sing-1"]?.ok, true)
        let call = try XCTUnwrap(ssh(runner).last)
        XCTAssertNil(call.stdin, "as root there is no password")
        XCTAssertTrue(call.argv.contains("root@vps-sing-1"))
    }

    @MainActor
    func testCancelingTheSheetRunsNothing() async {
        let runner = ScriptedRunner()
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), runner: runner, steps: { _, _ in [self.adminStep()] })
        await model.open()
        model.requestSetUp(.staysAwake, on: "m1")
        guard case .fix(let prompt)? = model.setupSheet else { return XCTFail("no sheet") }
        model.cancel(prompt)
        XCTAssertNil(model.setupSheet)
        XCTAssertTrue(ssh(runner).isEmpty)
        XCTAssertNil(model.outcomes["m1"])
    }

    @MainActor
    func testASecretStepsGuidanceIsNotShownAsAFailure() async {
        let message = "Opened the form for claude-worker. Save it, then Set up again to send it."
        let step = plainStep(.claudeLogin, title: "Claude login", ok: false, message: message)
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: { _, _ in [step] })
        await model.open()
        model.requestSetUp(.claudeLogin, on: "m1")
        guard case .fix(let prompt)? = model.setupSheet else { return XCTFail("no sheet") }
        await model.confirm(prompt, password: nil).value
        XCTAssertEqual(model.outcomes["m1"]?.ok, false)
        XCTAssertEqual(model.outcomes["m1"]?.guidance, true)
        XCTAssertEqual(model.outcomes["m1"]?.text, message)

        let broken = plainStep(.claudeLogin, title: "Claude login", ok: false, message: "could not write")
        let model2 = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: { _, _ in [broken] })
        await model2.open()
        model2.requestSetUp(.claudeLogin, on: "m1")
        guard case .fix(let p2)? = model2.setupSheet else { return XCTFail("no sheet") }
        await model2.confirm(p2, password: nil).value
        XCTAssertEqual(model2.outcomes["m1"]?.guidance, false)
    }

    @MainActor
    func testSetUpAllShowsTheStepsFirstThenAsksTheAdminPasswordForTheAdminStepOnly() async throws {
        let order = Order()
        let first = SetupStep(id: .reachable, title: "First", needs: [], cadence: .everyRefresh,
                              probe: .local { _ in CheckResult(state: .fail("no")) },
                              fix: .run(FixPlan(display: "do first", needsAdmin: false) { _ in order.add("first"); return FixOutcome(ok: true, message: "ok") }))
        let admin = adminStep { ctx in order.add("admin:\(ctx.adminPassword ?? "-")"); return FixOutcome(ok: true, message: "ok") }
        let last = SetupStep(id: .baseTools, title: "Last", needs: [], cadence: .everyRefresh,
                             probe: .local { _ in CheckResult(state: .fail("no")) },
                             fix: .run(FixPlan(display: "do last", needsAdmin: false) { _ in order.add("last"); return FixOutcome(ok: true, message: "ok") }))
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: { _, _ in [first, admin, last] })
        await model.open()

        model.requestSetUpAll(on: "m1")
        guard case .fix(let all)? = model.setupSheet else { return XCTFail("no sheet") }
        XCTAssertEqual(all.kind, .all)
        XCTAssertFalse(all.needsAdmin, "the password is asked per admin step, when its turn comes")
        XCTAssertTrue(all.display.contains("First") && all.display.contains("do first") && all.display.contains("sudo pmset -a sleep 0"))
        XCTAssertEqual(order.items, [])

        let run = model.confirm(all, password: nil)
        var adminPrompt: FixPrompt?
        for _ in 0..<200 {
            if case .fix(let p)? = model.setupSheet, p.kind != .all { adminPrompt = p; break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        let prompt = try XCTUnwrap(adminPrompt, "the admin step asks")
        XCTAssertEqual(prompt.title, "Stays awake")
        XCTAssertTrue(prompt.needsAdmin)
        XCTAssertEqual(order.items, ["first"], "the plain step ran without a sheet; the admin step waits")
        await model.confirm(prompt, password: "pw-for-one-step").value
        await run.value
        XCTAssertEqual(order.items, ["first", "admin:pw-for-one-step", "last"])
        XCTAssertEqual(model.outcomes["m1"]?.ok, true)
        var seen = Set<ObjectIdentifier>()
        XCTAssertFalse(holds("pw-for-one-step", in: model, seen: &seen))
    }

    @MainActor
    func testSetUpAllConfirmationListsDeployKeyAndCloneWhileTheyWait() async throws {
        let repo = TestMachines.repo
        let order = Order()
        func make(_ id: StepID, _ title: String, needs: [StepID], pass: @escaping @Sendable () -> Bool) -> SetupStep {
            SetupStep(id: id, title: title, needs: needs, cadence: .everyRefresh,
                      probe: .local { _ in pass() ? CheckResult(state: .pass("ok")) : CheckResult(state: .fail("no")) },
                      fix: .run(FixPlan(display: "run \(title)", needsAdmin: false) { _ in
                          order.add(title)
                          return FixOutcome(ok: true, message: "ok")
                      }))
        }
        // each step passes once its own Fix has run
        let steps: (Machine, [Machine]) -> [SetupStep] = { _, _ in [
            make(.baseTools, "Base tools", needs: [], pass: { order.items.contains("Base tools") }),
            make(.deployKey(repo), "Deploy key", needs: [.baseTools], pass: { order.items.contains("Deploy key") }),
            make(.clone(repo), "Clone", needs: [.deployKey(repo)], pass: { order.items.contains("Clone") }),
        ] }
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: steps)
        await model.open()
        XCTAssertEqual(model.engine.results["m1"]?[.deployKey(repo)]?.state, .cantTell("waits for Base tools"))

        model.requestSetUpAll(on: "m1")
        guard case .fix(let all)? = model.setupSheet else { return XCTFail("no sheet") }
        XCTAssertEqual(all.kind, .all)
        for line in ["Base tools", "run Base tools", "Deploy key", "run Deploy key", "Clone", "run Clone"] {
            XCTAssertTrue(all.display.contains(line), "the confirmation lacks \(line)")
        }
        XCTAssertTrue(all.display.contains("once the steps above pass"))
        await model.confirm(all, password: nil).value
        XCTAssertEqual(order.items, ["Base tools", "Deploy key", "Clone"])
        XCTAssertEqual(model.outcomes["m1"]?.ok, true)
    }

    @MainActor
    func testSetUpAllRunsOnlyWhatItShowed() async throws {
        let order = Order()
        // "Late" passes when the confirmation is made and fails once Base tools has run
        let steps: (Machine, [Machine]) -> [SetupStep] = { _, _ in [
            SetupStep(id: .baseTools, title: "Base tools", needs: [], cadence: .everyRefresh,
                      probe: .local { _ in order.items.contains("Base tools") ? CheckResult(state: .pass("ok")) : CheckResult(state: .fail("no")) },
                      fix: .run(FixPlan(display: "run Base tools", needsAdmin: false) { _ in order.add("Base tools"); return FixOutcome(ok: true, message: "ok") })),
            SetupStep(id: .claudeCode, title: "Late", needs: [], cadence: .everyRefresh,
                      probe: .local { _ in order.items.contains("Base tools") ? CheckResult(state: .fail("broke")) : CheckResult(state: .pass("ok")) },
                      fix: .run(FixPlan(display: "run Late", needsAdmin: false) { _ in order.add("Late"); return FixOutcome(ok: true, message: "ok") })),
        ] }
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: steps)
        await model.open()
        model.requestSetUpAll(on: "m1")
        guard case .fix(let all)? = model.setupSheet else { return XCTFail("no sheet") }
        XCTAssertFalse(all.display.contains("Late"))
        await model.confirm(all, password: nil).value
        XCTAssertEqual(order.items, ["Base tools"], "Late was not shown, so it did not run")
        XCTAssertEqual(model.outcomes["m1"]?.text, "Late needs a look; Set up again")
        XCTAssertEqual(model.outcomes["m1"]?.guidance, true, "a look is asked for, not a red failure")
    }

    @MainActor
    func testSetUpAllStopsWhenTheAdminSheetIsCancelled() async throws {
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: { _, _ in [self.adminStep()] })
        await model.open()
        model.requestSetUpAll(on: "m1")
        guard case .fix(let all)? = model.setupSheet else { return XCTFail("no sheet") }
        let run = model.confirm(all, password: nil)
        var adminPrompt: FixPrompt?
        for _ in 0..<200 {
            if case .fix(let p)? = model.setupSheet, p.kind != .all { adminPrompt = p; break }
            try await Task.sleep(nanoseconds: 10_000_000)
        }
        model.cancel(try XCTUnwrap(adminPrompt))
        await run.value
        XCTAssertEqual(model.outcomes["m1"]?.ok, false)
        XCTAssertEqual(model.outcomes["m1"]?.text, "Set up all stopped before Stays awake: cancelled")
    }

    @MainActor
    func testSetUpAllWithNothingToDoSaysSoAndAsksNothing() async {
        let ok = SetupStep(id: .reachable, title: "Fine", needs: [], cadence: .everyRefresh,
                           probe: .local { _ in CheckResult(state: .pass("fine")) },
                           fix: .run(FixPlan(display: "x", needsAdmin: false) { _ in FixOutcome(ok: true, message: "") }))
        let model = makeModel(TestRegistry(machines: [TestMachines.mac()]), steps: { _, _ in [ok] })
        await model.open()
        model.requestSetUpAll(on: "m1")
        XCTAssertNil(model.setupSheet)
        XCTAssertEqual(model.outcomes["m1"]?.text, "nothing to set up")
    }

    @MainActor
    func testTheRunningSwitchShowsItsScriptAndRunsOnlyOnTheClick() async throws {
        let runner = ScriptedRunner()
        let running = SetupStep(id: .running, title: "Running", needs: [], cadence: .everyRefresh,
                                probe: .local { _ in CheckResult(state: .fail("off")) }, fix: .toggle(needsAdmin: true))
        let model = makeModel(TestRegistry(machines: [TestMachines.linux()]), runner: runner, steps: { _, _ in [running] })
        await model.open()
        model.requestToggle(on: "vps-sing-1", to: true)
        guard case .fix(let prompt)? = model.setupSheet else { return XCTFail("no sheet") }
        XCTAssertEqual(prompt.kind, .toggle(on: true))
        XCTAssertEqual(prompt.display, SDWorkerSteps.toggleScript(linux: true, on: true))
        XCTAssertTrue(prompt.needsAdmin)
        XCTAssertTrue(ssh(runner).isEmpty)
        await model.confirm(prompt, password: "pw-toggle").value
        let call = try XCTUnwrap(ssh(runner).first)
        XCTAssertTrue(call.argv.last?.contains("systemctl enable --now sd-worker.service") == true)
        XCTAssertEqual(model.outcomes["vps-sing-1"]?.ok, true)
    }

    @MainActor
    func testUpdateMachinesRunsOnlyOnTheClick() async {
        let runner = ScriptedRunner()
        let delivered = Order()
        let reg = TestRegistry(machines: [TestMachines.mac()])
        let engine = SetupEngine(runner: runner, secrets: FakeSecrets(values: ["claude-worker": testLongToken]), http: FakeHTTP(),
                                 mac: { .empty }, machines: { reg.machines }, save: reg.save, clock: { Date() },
                                 steps: { _, _ in [] },
                                 deliver: { usage, _ in delivered.add(usage.name); return FixOutcome(ok: true, message: "ok") },
                                 observeVault: false)
        let poller = MachinePoller(runner: runner, machines: { reg.machines }, clock: { Date() }, observeSleep: false)
        let model = MachinesModel(poller: poller, actions: MorningActions(runner: runner), engine: engine,
                                  registry: { reg.state }, save: reg.save, vaultEntries: { [] }, rename: { _, _ in },
                                  renew: { _ in }, registryPath: "/tmp/x", runner: runner)
        engine.vaultValueChanged(name: "claude-worker")
        XCTAssertEqual(engine.updateOffers["claude-worker"]?.count, 1)
        XCTAssertEqual(model.updateOffers.map(\.secret), ["claude-worker"])
        XCTAssertEqual(model.updateOffers.first?.title, "Update 1 machine")
        XCTAssertEqual(delivered.items, [], "the offer sends nothing")
        await model.update(secret: "claude-worker").value
        XCTAssertEqual(delivered.items, ["claude-worker"])
        XCTAssertTrue(model.updateOffers.isEmpty)
    }

    // MARK: Machines and jobs

    @MainActor
    func testAddingRemovingAndAdminChoiceChangeTheRecordOnly() async {
        let runner = ScriptedRunner()
        let reg = TestRegistry(machines: [TestMachines.mac("m1"), TestMachines.linux()])
        let model = makeModel(reg, runner: runner)
        model.setAdmin("vps-sing-1", .rootSSH)
        XCTAssertEqual(reg.machines.last?.admin, .rootSSH)
        model.remove("m1")
        XCTAssertEqual(reg.machines.map(\.name), ["vps-sing-1"])
        XCTAssertTrue(runner.calls.isEmpty, "nothing on a machine changes")
        reg.refuseSaves = true
        model.remove("vps-sing-1")
        XCTAssertEqual(model.registryProblem, "Couldn't save machines.json")
    }

    func testTheJobFormRefusesUnsafeValuesAndKeepsWhatItDoesNotEdit() {
        var draft = JobDraft(machine: "m1")
        XCTAssertEqual(JobForm.problems(draft), ["Add at least one repository"])
        draft.repos = [JobRepoDraft(github: "me/app", localPath: "/Users/me/app", checkCommand: "just check")]
        draft.kinds = ["ios"]
        XCTAssertEqual(JobForm.problems(draft), [])
        for bad in ["me", "me/app/x", "me/ap p", "-me/app", "me/app;x", ""] {
            draft.repos[0].github = bad
            XCTAssertEqual(JobForm.problems(draft), ["Repository: owner/name, like me/app"], bad)
        }
        draft.repos[0].github = "me/app"
        draft.repos[0].machinePath = "~/src/app;x"
        XCTAssertEqual(JobForm.problems(draft), ["Machine path: ~/ or an absolute path with plain characters"])
        draft.repos[0].machinePath = "~/work/app"
        draft.repos[0].verifySkill = "../x"
        XCTAssertEqual(JobForm.problems(draft), ["Verify skill: a path inside the checkout"])
        draft.repos[0].verifySkill = ".claude/skills/verify/SKILL.md"
        draft.repos[0].localPath = "relative"
        XCTAssertEqual(JobForm.problems(draft), ["Checkout on this Mac: an absolute path"])
        draft.repos[0].localPath = "/Users/me/app"
        draft.repos[0].checkCommand = "  "
        XCTAssertEqual(JobForm.problems(draft), ["Check command: required"])
        draft.repos[0].checkCommand = "just check"
        draft.healthURL = "file:///etc/passwd"
        XCTAssertEqual(JobForm.problems(draft), ["Health URL: http or https"])
        draft.healthURL = "https://example.com/health"
        draft.repos.append(draft.repos[0])
        XCTAssertEqual(JobForm.problems(draft), ["me/app is listed twice"])
        draft.repos.removeLast()
        XCTAssertEqual(JobForm.problems(draft), [])

        // edits keep python, sdPath, tokenExpires and the vault name
        let base = SDWorkerSettings(python: "/usr/bin/python3", sdPath: "/home/a/sd.py", repos: [], kinds: ["backend"],
                                    tokenExpires: "2027-01-01", issuesSecret: "m1-github-issues")
        let made = JobForm.settings(draft, base: base)
        XCTAssertEqual(made.python, "/usr/bin/python3")
        XCTAssertEqual(made.sdPath, "/home/a/sd.py")
        XCTAssertEqual(made.tokenExpires, "2027-01-01")
        XCTAssertEqual(made.issuesSecret, "m1-github-issues")
        XCTAssertEqual(made.kinds, ["ios"])
        XCTAssertEqual(made.healthURL, "https://example.com/health")
        XCTAssertEqual(made.repos, [MachineRepo(github: "me/app", localPath: "/Users/me/app", checkCommand: "just check",
                                                machinePath: "~/work/app", verifySkill: ".claude/skills/verify/SKILL.md")])
        // a new job starts from plan C's defaults, and an empty machine path stays nil (the default ~/src/<name>)
        draft.repos[0].machinePath = ""
        draft.machine = "m2"
        let fresh = JobForm.settings(draft, base: nil)
        XCTAssertEqual(fresh.python, "python3")
        XCTAssertEqual(fresh.sdPath, "")
        XCTAssertEqual(fresh.issuesSecret, "m2-github-issues")
        XCTAssertNil(fresh.repos[0].machinePath)
    }

    @MainActor
    func testSavingAJobAddsOrReplacesTheSDWorkerJobOnly() {
        var bare = TestMachines.mac("m3")
        bare.jobs = []
        let reg = TestRegistry(machines: [bare])
        let runner = ScriptedRunner()
        let model = makeModel(reg, runner: runner)
        var draft = JobDraft(machine: "m3")
        draft.repos = [JobRepoDraft(github: "me/app", localPath: "/Users/me/app", checkCommand: "true")]
        draft.kinds = ["ios", "backend"]
        XCTAssertNil(model.saveJob(draft))
        XCTAssertEqual(reg.machines.first?.sdWorker?.kinds, ["ios", "backend"])
        XCTAssertEqual(reg.machines.first?.sdWorker?.issuesSecret, "m3-github-issues")
        draft.repos[0].github = "bad"
        XCTAssertEqual(model.saveJob(draft), "Repository: owner/name, like me/app")
        XCTAssertEqual(reg.saves.count, 1)
        XCTAssertTrue(runner.calls.isEmpty)
    }
}

private final class Order: @unchecked Sendable {
    private let lock = NSLock()
    private var list: [String] = []
    func add(_ s: String) { lock.withLock { list.append(s) } }
    var items: [String] { lock.withLock { list } }
}

/// Records which ssh calls carried the admin password as their stdin, without keeping the password or the stdin.
private final class SpyRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    /// Only a digest of what sudo should read: the spy must not be a place the password sits.
    private let expected: SHA256.Digest
    private var argvs: [[String]] = []
    private var matches: [Bool] = []

    init(password: String) { expected = SHA256.hash(data: Data((password + "\n").utf8)) }

    var sshArgvs: [[String]] { lock.withLock { argvs } }
    var stdinWasThePassword: [Bool] { lock.withLock { matches } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: env, stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        if argv.first == "/usr/bin/ssh" {
            lock.withLock {
                argvs.append(argv)
                matches.append(stdin.map { SHA256.hash(data: $0) == expected } ?? false)
            }
        }
        return CommandResult(code: 0, out: "", err: "")
    }
}

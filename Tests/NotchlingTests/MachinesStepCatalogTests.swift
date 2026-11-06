import XCTest
@testable import Notchling

/// The step catalog's order, metadata and the one way a Fix touches a machine as admin. No ssh starts.
final class MachinesStepCatalogTests: XCTestCase {
    private let repo = TestMachines.repo

    private func ids(_ steps: [SetupStep]) -> [String] { steps.map(\.id.raw) }

    private func catalog(_ machine: Machine) -> [SetupStep] { StepCatalog.steps(for: machine, all: [machine]) }

    private func plan(_ step: SetupStep) -> FixPlan? {
        if case let .run(plan) = step.fix { return plan }
        return nil
    }

    /// run / run+admin / howTo / toggle / toggle+admin
    private func kind(_ step: SetupStep) -> String {
        switch step.fix {
        case let .run(plan): return plan.needsAdmin ? "run+admin" : "run"
        case .howTo: return "howTo"
        case let .toggle(needsAdmin): return needsAdmin ? "toggle+admin" : "toggle"
        }
    }

    func testStepOrderForAMacWithAnSDWorkerJob() {
        XCTAssertEqual(ids(catalog(TestMachines.mac())), [
            "reachable", "worker-user", "stays-awake", "filevault", "base-tools", "claude-code", "claude-login", "xcode",
            "deploy-key:\(repo)", "clone:\(repo)", "push-refused:\(repo)",
            "plugins", "issues-token", "token-scope", "worker-json", "service", "sd-check", "running",
        ])
    }

    func testLinuxHasNoMacOnlySteps() {
        let linux = ids(catalog(TestMachines.linux()))
        for id in ["stays-awake", "filevault", "xcode"] { XCTAssertFalse(linux.contains(id), id) }
        XCTAssertEqual(linux.count, 15)
        XCTAssertEqual(Array(linux.prefix(5)), ["reachable", "worker-user", "base-tools", "claude-code", "claude-login"])
    }

    func testUnknownOSKeepsTheMacStepsUntilReachableTells() {
        var machine = TestMachines.mac()
        machine.os = nil
        let all = ids(catalog(machine))
        XCTAssertEqual(all.count, 18)
        for id in ["stays-awake", "filevault", "xcode"] { XCTAssertTrue(all.contains(id), id) }
    }

    func testAMachineWithoutJobsHasOnlyMachineSteps() {
        var machine = TestMachines.mac()
        machine.jobs = []
        XCTAssertEqual(ids(catalog(machine)), [
            "reachable", "worker-user", "stays-awake", "filevault", "base-tools", "claude-code", "claude-login", "xcode",
        ])
    }

    func testEachRepoGetsItsThreeSteps() {
        var machine = TestMachines.mac()
        machine.sdWorker?.repos.append(MachineRepo(github: "me/other", localPath: "/x", checkCommand: "true"))
        let all = ids(catalog(machine))
        XCTAssertEqual(Array(all[8...13]), ["deploy-key:\(repo)", "clone:\(repo)", "push-refused:\(repo)",
                                            "deploy-key:me/other", "clone:me/other", "push-refused:me/other"])
        let workerJSON = catalog(machine).first { $0.id == .workerJSON }!
        XCTAssertEqual(Set(workerJSON.needs.map(\.raw)),
                       ["plugins", "issues-token", "claude-login", "clone:\(repo)", "clone:me/other"])
    }

    func testNeedsAdminOnlyWhereTheSpecSaysSo() {
        func admin(_ steps: [SetupStep]) -> [String] {
            steps.filter { kind($0).hasSuffix("+admin") }.map(\.id.raw)
        }
        XCTAssertEqual(admin(catalog(TestMachines.linux())), ["base-tools", "service", "running"])
        XCTAssertEqual(admin(catalog(TestMachines.mac())), ["stays-awake"])
    }

    func testEveryNeedIsAnEarlierStep() {
        var unknown = TestMachines.mac()
        unknown.os = nil
        for machine in [TestMachines.mac(), TestMachines.linux(), unknown] {
            let steps = catalog(machine)
            for (index, step) in steps.enumerated() {
                let earlier = Set(steps[..<index].map(\.id))
                for need in step.needs {
                    XCTAssertTrue(earlier.contains(need), "\(step.id.raw) needs \(need.raw), which is not an earlier step")
                }
            }
            XCTAssertEqual(Set(steps.map(\.id)).count, steps.count, "step ids are unique")
        }
    }

    func testCheckOnlyStepsHaveAHowTo() {
        let steps = catalog(TestMachines.mac())
        // (sd-check has a click-run Fix since sd 1.19.3: it creates the labels the read-only check reports missing)
        for id in ["reachable", "worker-user", "filevault", "xcode", "push-refused:\(repo)"] {
            let step = steps.first { $0.id.raw == id }!
            guard case let .howTo(text) = step.fix else { return XCTFail("\(id) is not a how-to") }
            XCTAssertFalse(text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, id)
        }
        guard case .toggle = steps.first(where: { $0.id == .running })!.fix else { return XCTFail("running is not a toggle") }
    }

    func testSlowSteps() {
        let slow = catalog(TestMachines.mac()).filter { $0.cadence == .slow }.map(\.id.raw)
        // Worker user is slow because its `sudo -n -l` must not run on every 60 s refresh (it writes the sudo auth log)
        XCTAssertEqual(slow, ["worker-user", "deploy-key:\(repo)", "push-refused:\(repo)", "token-scope", "sd-check"])
    }

    /// The table of the spec: the steps' probes and Fixes never change this.
    func testMetadataMatchesTheSpecTable() {
        let r = repo
        let rows: [(id: String, title: String, needs: [String], kind: String)] = [
            ("reachable", "Reachable", [], "howTo"),
            ("worker-user", "Worker user", ["reachable"], "howTo"),
            ("stays-awake", "Stays awake", ["reachable"], "run+admin"),
            ("filevault", "FileVault", ["reachable"], "howTo"),
            ("base-tools", "Base tools", ["reachable"], "run"),
            ("claude-code", "Claude Code", ["reachable"], "run"),
            ("claude-login", "Claude login", ["reachable"], "run"),
            ("xcode", "Xcode", ["reachable"], "howTo"),
            ("deploy-key:\(r)", "Deploy key (\(r))", ["reachable", "base-tools"], "run"),
            ("clone:\(r)", "Clone (\(r))", ["deploy-key:\(r)"], "run"),
            ("push-refused:\(r)", "Push is refused (\(r))", ["clone:\(r)"], "howTo"),
            ("plugins", "Plugins installed", ["claude-code"], "run"),
            ("issues-token", "GitHub issues token delivered", ["reachable"], "run"),
            ("token-scope", "Token scope", [], "run"),
            ("worker-json", "worker.json written", ["plugins", "issues-token", "claude-login", "clone:\(r)"], "run"),
            ("service", "Service installed", ["worker-json", "base-tools"], "run"),
            ("sd-check", "sd worker setup --check", ["service"], "run"),
            ("running", "Running", ["reachable"], "toggle"),
        ]
        let steps = catalog(TestMachines.mac())
        XCTAssertEqual(steps.count, rows.count)
        for (step, row) in zip(steps, rows) {
            XCTAssertEqual(step.id.raw, row.id)
            XCTAssertEqual(step.title, row.title, row.id)
            XCTAssertEqual(step.needs.map(\.raw), row.needs, row.id)
            XCTAssertEqual(kind(step), row.kind, row.id)
        }
        // admin on Linux for these three
        let linux = catalog(TestMachines.linux())
        for id in ["base-tools", "service"] { XCTAssertEqual(kind(linux.first { $0.id.raw == id }!), "run+admin", id) }
        XCTAssertEqual(kind(linux.first { $0.id == .running }!), "toggle+admin")
    }

    /// The stub helpers are gone (no step can be "not built yet"); no Fix still shows their text.
    func testNoCatalogFixIsAStub() {
        for machine in [TestMachines.mac(), TestMachines.linux()] {
            for step in catalog(machine) {
                if case let .run(plan) = step.fix { XCTAssertNotEqual(plan.display, "not built yet", step.id.raw) }
            }
        }
    }

    // MARK: MachineRun.admin

    private func fixContext(_ machine: Machine = TestMachines.linux(), password: String?,
                            runner: ScriptedRunner) -> FixContext {
        .test(.test(machine: machine, runner: runner), adminPassword: password)
    }

    func testAdminRunPutsThePasswordOnlyOnStdin() async {
        let password = "hunter2-Pw!"
        let mac = TestMachines.mac()
        let runner = ScriptedRunner()
        let result = await MachineRun.admin("pmset -a sleep 0", ctx: fixContext(mac, password: password, runner: runner))
        XCTAssertEqual(result.code, 0)
        XCTAssertEqual(runner.calls.count, 1)
        let call = runner.calls[0]
        XCTAssertEqual(call.stdin, Data("\(password)\n".utf8))
        XCTAssertFalse(call.argv.contains { $0.contains(password) }, "the password is in argv")
        XCTAssertFalse(call.env.values.contains { $0.contains(password) }, "the password is in the environment")
        XCTAssertEqual(call.argv, SSH.admin(mac, "pmset -a sleep 0", asRoot: false))
        XCTAssertTrue(call.argv.contains("sondoan@m1"))
        XCTAssertTrue(call.argv.last!.contains("/usr/bin/sudo -S"), "the absolute sudo")

        // root over ssh: no sudo, no password on stdin, even if one was given
        var root = TestMachines.linux()
        root.admin = .rootSSH
        for given in [nil, password] as [String?] {
            let rootRunner = ScriptedRunner()
            _ = await MachineRun.admin("apt-get update", ctx: fixContext(root, password: given, runner: rootRunner))
            XCTAssertEqual(rootRunner.calls.count, 1)
            XCTAssertNil(rootRunner.calls[0].stdin)
            XCTAssertTrue(rootRunner.calls[0].argv.contains("root@vps-sing-1"))
            XCTAssertFalse(rootRunner.calls[0].argv.contains { $0.contains(password) })
            XCTAssertEqual(rootRunner.calls[0].argv, SSH.admin(root, "apt-get update", asRoot: true))
            XCTAssertFalse(rootRunner.calls[0].argv.last!.contains("sudo"), "root over ssh has no sudo")
        }

        // sudo without a password: refused before anything runs
        let idle = ScriptedRunner()
        let refused = await MachineRun.admin("true", ctx: fixContext(mac, password: nil, runner: idle))
        XCTAssertEqual(refused.code, 126)
        XCTAssertEqual(refused.err, "needs the admin password")
        XCTAssertTrue(idle.calls.isEmpty)
    }

    /// C1 / ruling M12: on Linux the worker account can't be the admin, so sudo as the worker is refused before a
    /// command runs, with or without a password.
    func testALinuxAdminStepAsTheWorkerIsRefusedBeforeAnyCommandRuns() async {
        let linux = TestMachines.linux()
        XCTAssertEqual(linux.admin, .sudo)
        for given in [nil, "hunter2-Pw!"] as [String?] {
            let idle = ScriptedRunner()
            let refused = await MachineRun.admin("apt-get update", ctx: fixContext(linux, password: given, runner: idle))
            XCTAssertEqual(refused.code, 126)
            XCTAssertEqual(refused.err, "On Linux the worker account can't be the admin: its own shell files could read the password. Use root over SSH or another admin account.")
            XCTAssertTrue(idle.calls.isEmpty, "no command ran")
        }
        // an unknown OS is not Linux; a Mac's own login account is the admin there
        var mac = TestMachines.mac()
        XCTAssertNil(SSH.adminRefusal(mac))
        mac.os = nil
        XCTAssertNil(SSH.adminRefusal(mac))
        var root = linux
        root.admin = .rootSSH
        XCTAssertNil(SSH.adminRefusal(root))
    }

    func testScriptAndLocalStreamTheirOutputToTheLog() async {
        let runner = ScriptedRunner()
        runner.on({ _ in true }, chunks: ["one\n", "two\n"])
        let box = LogBox()
        let ctx = FixContext.test(.test(runner: runner), adminPassword: "pw", log: { box.add($0) })
        _ = await MachineRun.script("echo hi", stdin: Data("v".utf8), ctx: ctx)
        _ = await MachineRun.local(["/bin/echo", "x"], ctx: ctx)
        XCTAssertEqual(box.lines, ["one\n", "two\n", "one\n", "two\n"])
        XCTAssertEqual(runner.calls[0].argv, SSH.script(TestMachines.mac(), "echo hi"))
        XCTAssertEqual(runner.calls[0].stdin, Data("v".utf8))
        XCTAssertEqual(runner.calls[1].argv, ["/bin/echo", "x"])
        XCTAssertNil(runner.calls[1].stdin)
    }

    func testAdminPathsAreAbsoluteToAnExplicitHome() {
        XCTAssertEqual(MachinePaths.sdPy(home: "/home/agent"), "/home/agent/.local/share/agents/system-design/plugin/scripts/sd.py")
        XCTAssertEqual(MachinePaths.absolute(MachinePaths.workerJSON, home: "/home/agent"), "/home/agent/.config/sd/worker.json")
        XCTAssertEqual(MachinePaths.absolute("/etc/x", home: "/home/agent"), "/etc/x")
        XCTAssertNil(MachinePaths.absolute(MachinePaths.workerJSON, home: "/home/a b"))
        XCTAssertNil(MachinePaths.absolute(MachinePaths.workerJSON, home: "$HOME"))
    }

    func testStepIDsAreCodableAndHashable() throws {
        XCTAssertEqual(StepID.deployKey("a/b").raw, "deploy-key:a/b")
        XCTAssertEqual(StepID.clone("a/b").raw, "clone:a/b")
        XCTAssertEqual(StepID.pushRefused("a/b").raw, "push-refused:a/b")
        let data = try JSONEncoder().encode(StepID.reachable)
        XCTAssertEqual(try JSONDecoder().decode(StepID.self, from: data), .reachable)
        XCTAssertTrue(MachineSafety.productionServices.contains("lingeva-app"))
    }
}

import XCTest
@testable import Notchling

/// Steps 1–6 and 8 of the catalog: each probe's parse, and each Fix on a scripted runner. No ssh starts.
final class MachinesMachineStepsTests: XCTestCase {
    private func step(_ id: StepID, _ machine: Machine = TestMachines.mac()) -> SetupStep {
        MachineSteps.all(for: machine).first { $0.id == id }!
    }

    private func script(_ step: SetupStep) -> String {
        guard case let .remote(script, _) = step.probe else { XCTFail("\(step.id.raw) has no remote probe"); return "" }
        return script
    }

    private func check(_ id: StepID, _ text: String, code: Int32 = 0, machine: Machine = TestMachines.mac()) async -> CheckResult {
        let s = step(id, machine)
        guard case let .remote(_, parse) = s.probe else { XCTFail("\(id.raw) has no remote probe"); return CheckResult(state: .cantTell("")) }
        return await parse(RemoteOutput(text: text, code: code), .test(machine: machine))
    }

    private func plan(_ step: SetupStep) -> FixPlan {
        guard case let .run(plan) = step.fix else { XCTFail("\(step.id.raw) has no Fix"); return FixPlan(display: "", needsAdmin: false) { _ in FixOutcome(ok: false, message: "") } }
        return plan
    }

    private func howTo(_ step: SetupStep) -> String {
        guard case let .howTo(text) = step.fix else { XCTFail("\(step.id.raw) is not a how-to"); return "" }
        return text
    }

    private func fixture(_ name: String) throws -> String {
        let url = try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: "txt", subdirectory: "Fixtures"))
        return try String(contentsOf: url, encoding: .utf8)
    }

    // MARK: Reachable

    func testReachableParsesOSArchHomeVersionDiskAndDocker() async {
        let out = "os=Darwin\narch=arm64\nuid=501\nuser=sondoan\nhome=/Users/sondoan\nversion=26.1\ndisk_kb=125829120\ndocker=rootless\n"
        let r = await check(.reachable, out)
        XCTAssertEqual(r.state, .pass("macOS 26.1, arm64"))
        XCTAssertEqual(r.detected.os, .macOS)
        XCTAssertEqual(r.detected.home, "/Users/sondoan")
        XCTAssertEqual(r.detected.abilities?.freeDiskGB, 120)
        XCTAssertEqual(r.detected.abilities?.docker, .rootless)
        XCTAssertEqual(r.detected.abilities?.arch, "arm64")
        XCTAssertEqual(r.detected.abilities?.osVersion, "26.1")
        XCTAssertEqual(r.detected.abilities?.home, "/Users/sondoan")
    }

    func testReachableParsesLinuxAndKeepsWhatOtherStepsFound() async {
        var machine = TestMachines.linux()
        machine.abilities?.xcode = "kept"
        let out = "os=Linux\narch=x86_64\nuid=1001\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04.1 LTS\ndisk_kb=2097152\ndocker=none\n"
        let r = await check(.reachable, out, machine: machine)
        XCTAssertEqual(r.state, .pass("Ubuntu 24.04.1 LTS, x86_64"))
        XCTAssertEqual(r.detected.os, .linux)
        XCTAssertEqual(r.detected.abilities?.docker, DockerState.none)
        XCTAssertEqual(r.detected.abilities?.freeDiskGB, 2)
        XCTAssertEqual(r.detected.abilities?.xcode, "kept")
    }

    func testReachableKeysEachValueByItsNameNotItsPosition() async {
        // a line missing and a banner in front: nothing shifts
        let out = "Welcome to the machine\nos=Linux\nhome=/home/agent\narch=aarch64\nversion=Debian 12\ndocker=stopped\n"
        let r = await check(.reachable, out, machine: TestMachines.linux())
        XCTAssertEqual(r.state, .pass("Debian 12, aarch64"))
        XCTAssertEqual(r.detected.home, "/home/agent")
        XCTAssertEqual(r.detected.abilities?.docker, .stopped)
        XCTAssertNil(r.detected.abilities?.freeDiskGB)
    }

    func testReachableRefusesAHomeWithShellCharacters() async {
        for home in ["/home/a;touch x", "/home/$(id)", "relative/home", "/home/a/../b", "/home/a b"] {
            let r = await check(.reachable, "os=Linux\narch=x86_64\nhome=\(home)\nversion=Ubuntu\n", machine: TestMachines.linux())
            guard case let .fail(text) = r.state else { XCTFail("\(home) did not fail"); continue }
            XCTAssertTrue(text.contains("home folder"), text)
            XCTAssertFalse(text.contains(home), "the hostile text is not echoed")
            XCTAssertNil(r.detected.home, home)
            XCTAssertNil(r.detected.abilities?.home, home)
        }
        let missing = await check(.reachable, "os=Linux\narch=x86_64\n", machine: TestMachines.linux())
        guard case .fail = missing.state else { return XCTFail("no home reported should fail") }
    }

    func testReachableRefusesAnUnknownOS() async {
        let r = await check(.reachable, "os=FreeBSD\narch=amd64\nhome=/home/a\n")
        XCTAssertEqual(r.state, .fail("unsupported OS FreeBSD"))
        XCTAssertNil(r.detected.os)
    }

    func testReachableWithNoAnswerIsAFailureOrCantTell() async {
        let down = await check(.reachable, "ssh: connect to host m1 port 22: Operation timed out\n", code: 255)
        guard case .fail = down.state else { return XCTFail("a failed ssh is a fail") }
        let odd = await check(.reachable, "", code: 0)
        guard case .cantTell = odd.state else { return XCTFail("an empty answer is can't tell") }
    }

    func testReachableCutsLongMachineStrings() async {
        let long = String(repeating: "x", count: 500)
        let r = await check(.reachable, "os=Linux\narch=x86_64\nhome=/home/a\nversion=\(long)\n", machine: TestMachines.linux())
        XCTAssertEqual(r.detected.abilities?.osVersion?.count, 200)
    }

    func testReachableProbeIsReadOnlyAndPrintsEveryKey() {
        let s = script(step(.reachable))
        for key in ["os=", "arch=", "uid=", "user=", "home=", "version=", "disk_kb=", "docker="] {
            XCTAssertTrue(s.contains(key), key)
        }
        XCTAssertTrue(s.contains("docker info --format '{{json .SecurityOptions}}'"))
        for word in ["sudo", "rm ", "install", "> "] { XCTAssertFalse(s.contains(word), word) }
        XCTAssertTrue(howTo(step(.reachable)).contains("tailscale ping"))
        XCTAssertTrue(howTo(step(.reachable)).contains("ACL"))
    }

    // MARK: Worker user

    func testWorkerUserFailsForRoot() async {
        let root = await check(.workerUser, "0\n")
        guard case let .fail(text) = root.state else { return XCTFail("root should fail") }
        XCTAssertTrue(text.contains("root"), text)
        let worker = await check(.workerUser, "1001\n")
        guard case .pass = worker.state else { return XCTFail("1001 should pass") }
        let odd = await check(.workerUser, "")
        guard case .cantTell = odd.state else { return XCTFail("no answer is can't tell") }
        XCTAssertTrue(howTo(step(.workerUser)).contains("agent"))
        XCTAssertEqual(script(step(.workerUser)), "id -u")
    }

    // MARK: Stays awake

    func testStaysAwakeNeedsSleepDisksleepZeroAndAutorestartOne() async {
        let bad = await check(.staysAwake, " sleep 10\n disksleep 10\n autorestart 0\n")
        XCTAssertEqual(bad.state, .fail("sleep 10 (should be 0), disksleep 10 (should be 0), autorestart 0 (should be 1)"))
        let good = await check(.staysAwake, "System-wide power settings:\n SleepDisabled 0\nCurrent Power Settings:\n sleep 0 (sleep prevented by powerd)\n disksleep 0\n displaysleep 10\n autorestart 1\n")
        XCTAssertEqual(good.state, .pass("sleep 0, disksleep 0, autorestart 1"))
        let partial = await check(.staysAwake, " sleep 0\n disksleep 0\n autorestart 0\n")
        XCTAssertEqual(partial.state, .fail("autorestart 0 (should be 1)"))
        let none = await check(.staysAwake, "")
        guard case .cantTell = none.state else { return XCTFail("no pmset output is can't tell") }
        XCTAssertEqual(script(step(.staysAwake)), "pmset -g")
    }

    func testStaysAwakeFixIsAnAdminPmset() async {
        let runner = ScriptedRunner()
        let s = step(.staysAwake)
        let p = plan(s)
        XCTAssertTrue(p.needsAdmin)
        XCTAssertTrue(p.display.contains("pmset -a sleep 0 disksleep 0 && pmset -a autorestart 1"))
        let outcome = await p.run(.test(.test(runner: runner), adminPassword: "pw-1234"))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(runner.calls.count, 1)
        let call = runner.calls[0]
        XCTAssertTrue(call.argv.last!.contains("pmset -a sleep 0 disksleep 0 && pmset -a autorestart 1"))
        XCTAssertTrue(call.argv.last!.contains("sudo -S"))
        XCTAssertEqual(call.stdin, Data("pw-1234\n".utf8))
        XCTAssertFalse(call.argv.contains { $0.contains("pw-1234") })
    }

    func testStaysAwakeFixWithoutAPasswordRunsNothing() async {
        let runner = ScriptedRunner()
        let outcome = await plan(step(.staysAwake)).run(.test(.test(runner: runner), adminPassword: nil))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    func testAFailedFixMessageNeverHoldsTheAdminPassword() async {
        let runner = ScriptedRunner()
        runner.on({ _ in true }, result: CommandResult(code: 1, out: "", err: "Sorry, try again pw-1234\n"))
        let outcome = await plan(step(.staysAwake)).run(.test(.test(runner: runner), adminPassword: "pw-1234"))
        XCTAssertFalse(outcome.ok)
        XCTAssertFalse(outcome.message.contains("pw-1234"), outcome.message)
    }

    // MARK: FileVault

    func testFileVaultOnAndOff() async {
        let on = await check(.filevault, "FileVault is On.\n")
        XCTAssertEqual(on.state, .pass("on: after a restart it waits for an unlock"))
        XCTAssertEqual(on.detected.fileVault, true)
        let off = await check(.filevault, "FileVault is Off.\n")
        XCTAssertEqual(off.state, .pass("off"))
        XCTAssertEqual(off.detected.fileVault, false)
        let other = await check(.filevault, "fdesetup: command not found\n", code: 127)
        guard case .cantTell = other.state else { return XCTFail("any other text is can't tell") }
        XCTAssertNil(other.detected.fileVault)
        XCTAssertEqual(script(step(.filevault)), "fdesetup status")
        XCTAssertTrue(howTo(step(.filevault)).contains("FileVault"))
    }

    // MARK: Base tools

    private let goodTools = "git=/opt/homebrew/bin/git\npython3=/opt/homebrew/bin/python3\njq=/opt/homebrew/bin/jq\ngh=/opt/homebrew/bin/gh\npyver=3.12\nbrew=yes\n"

    func testBaseToolsNeedsPython310AndFindsItsPath() async {
        let bad = await check(.baseTools, "git=/usr/bin/git\npython3=/usr/bin/python3\njq=\ngh=/opt/homebrew/bin/gh\npyver=3.9\nbrew=yes\n")
        XCTAssertEqual(bad.state, .fail("missing: jq; python3 is 3.9, sd needs 3.10 or newer"))
        let good = await check(.baseTools, goodTools)
        guard case .pass = good.state else { return XCTFail("a good run passes: \(good.state)") }
        XCTAssertEqual(good.detected.python, "/opt/homebrew/bin/python3")
        let ten = await check(.baseTools, goodTools.replacingOccurrences(of: "3.12", with: "3.10"))
        guard case .pass = ten.state else { return XCTFail("3.10 is enough") }
        let ancient = await check(.baseTools, goodTools.replacingOccurrences(of: "3.12", with: "2.7"))
        XCTAssertEqual(ancient.state, .fail("python3 is 2.7, sd needs 3.10 or newer"))
        let nothing = await check(.baseTools, "git=\npython3=\njq=\ngh=\npyver=\nbrew=no\n")
        XCTAssertEqual(nothing.state, .fail("missing: git, python3, jq, gh"))
        XCTAssertNil(nothing.detected.python)
    }

    func testBaseToolsIgnoresAnUnsafePythonPath() async {
        for path in ["/tmp/x y/python3", "/tmp/$(id)/python3", "relative/python3", "/tmp/a;b/python3"] {
            let r = await check(.baseTools, goodTools.replacingOccurrences(of: "/opt/homebrew/bin/python3", with: path))
            XCTAssertNil(r.detected.python, path)
        }
    }

    func testBaseToolsProbeIsReadOnly() {
        let s = script(step(.baseTools))
        for key in ["git=", "python3=", "jq=", "gh=", "pyver=", "brew="] { XCTAssertTrue(s.contains(key), key) }
        for word in ["sudo", "install ", "rm "] { XCTAssertFalse(s.contains(word), word) }
    }

    func testLinuxBaseToolsFixKeepsProductionUntouched() async {
        let machine = TestMachines.linux()
        let s = step(.baseTools, machine)
        let p = plan(s)
        XCTAssertTrue(p.needsAdmin)
        let runner = ScriptedRunner()
        let outcome = await p.run(.test(.test(machine: machine, runner: runner), adminPassword: "pw-1234"))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(runner.calls.count, 1)
        let call = runner.calls[0]
        XCTAssertEqual(call.stdin, Data("pw-1234\n".utf8))
        let sent = call.argv.last!
        XCTAssertTrue(sent.contains("sudo -S"))
        XCTAssertTrue(sent.contains("NEEDRESTART_SUSPEND=1"))
        XCTAssertTrue(sent.contains("apt-get install -y git python3 jq wget"))
        XCTAssertTrue(sent.contains("apt-get install -y gh"))
        for word in ["restart", "upgrade", "systemctl"] { XCTAssertFalse(sent.contains(word), word) }
        for service in MachineSafety.productionServices { XCTAssertFalse(sent.contains(service), service) }
        for word in ["restart", "upgrade", "systemctl"] { XCTAssertFalse(p.display.contains(word), word) }
    }

    func testLinuxBaseToolsFixRefusesWithoutAPassword() async {
        let machine = TestMachines.linux()
        let runner = ScriptedRunner()
        let outcome = await plan(step(.baseTools, machine)).run(.test(.test(machine: machine, runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    func testMacBaseToolsFixUsesHomebrewWithoutAdmin() async {
        let machine = TestMachines.mac()
        let p = plan(step(.baseTools, machine))
        XCTAssertFalse(p.needsAdmin)
        let runner = ScriptedRunner()
        let outcome = await p.run(.test(.test(machine: machine, runner: runner)))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(runner.calls.count, 1)
        let call = runner.calls[0]
        XCTAssertNil(call.stdin)
        let sent = call.argv.last!
        XCTAssertFalse(sent.contains("sudo"))
        XCTAssertTrue(sent.contains("brew install git python jq gh"))
        XCTAssertTrue(sent.contains("Homebrew is missing: install it from https://brew.sh"))
        XCTAssertTrue(p.display.contains("brew install git python jq gh"))
    }

    func testMacBaseToolsFixReportsAMissingHomebrew() async {
        let runner = ScriptedRunner()
        runner.on({ _ in true }, result: CommandResult(code: 1, out: "Homebrew is missing: install it from https://brew.sh in the machine's own Terminal, then Check again.\n", err: ""))
        let outcome = await plan(step(.baseTools)).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("Homebrew is missing"), outcome.message)
    }

    // MARK: Claude Code

    func testClaudeCodeShowsItsVersion() async {
        let ok = await check(.claudeCode, "2.1.330 (Claude Code)\n")
        XCTAssertEqual(ok.state, .pass("2.1.330 (Claude Code)"))
        let none = await check(.claudeCode, "")
        XCTAssertEqual(none.state, .fail("not installed"))
        let missing = await check(.claudeCode, "", code: 1)
        XCTAssertEqual(missing.state, .fail("not installed"))
    }

    func testClaudeCodeFixInstallsAsTheWorkerWithoutAdmin() async {
        let p = plan(step(.claudeCode))
        XCTAssertFalse(p.needsAdmin)
        XCTAssertTrue(p.display.contains("curl -fsSL https://claude.ai/install.sh | bash"))
        let runner = ScriptedRunner()
        let outcome = await p.run(.test(.test(runner: runner)))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(runner.calls.count, 1)
        XCTAssertTrue(runner.calls[0].argv.last!.contains("curl -fsSL https://claude.ai/install.sh | bash"))
        XCTAssertFalse(runner.calls[0].argv.last!.contains("sudo"))
        runner.on({ _ in true }, result: CommandResult(code: 22, out: "", err: "curl: (22) 404"))
        let failed = await p.run(.test(.test(runner: runner)))
        XCTAssertFalse(failed.ok)
    }

    // MARK: Xcode

    func testXcodeNeedsARuntimeAndAnIPhone() async throws {
        let text = try fixture("simctl-ok")
        let ok = await check(.xcode, text)
        XCTAssertEqual(ok.state, .pass("Xcode 26.1 · iOS 26.5 · iPhone 17 and 2 more"))
        XCTAssertEqual(ok.detected.abilities?.xcode, "Xcode 26.1")
        XCTAssertEqual(ok.detected.abilities?.iosRuntimes, ["iOS 26.5"])
        XCTAssertEqual(ok.detected.abilities?.simulators, ["iPhone 17", "iPhone 17 Pro", "iPhone 17 Pro Max"])

        let noPhone = await check(.xcode, text.replacingOccurrences(of: "\"name\" : \"iPhone", with: "\"name\" : \"iPad"))
        XCTAssertEqual(noPhone.state, .fail("no iPhone simulator"))

        let noXcode = await check(.xcode, String(text.drop { $0 != "@" }))
        XCTAssertEqual(noXcode.state, .fail("Xcode is not installed"))

        let noRuntime = await check(.xcode, text.replacingOccurrences(of: "\"name\" : \"iOS 26.5\"", with: "\"name\" : \"watchOS 26.5\"")
            .replacingOccurrences(of: "\"platform\" : \"iOS\"", with: "\"platform\" : \"watchOS\""))
        XCTAssertEqual(noRuntime.state, .fail("no iOS runtime"))

        let broken = await check(.xcode, "Xcode 26.1\n@@runtimes\n{ not json\n@@devices\n")
        XCTAssertEqual(broken.state, .fail("no iOS runtime"))

        XCTAssertTrue(howTo(step(.xcode)).contains("Apple ID"))
        let s = script(step(.xcode))
        XCTAssertTrue(s.contains("xcodebuild -version | head -1"))
        XCTAssertTrue(s.contains("@@runtimes") && s.contains("@@devices"))
        XCTAssertTrue(s.contains("xcrun simctl list runtimes available -j"))
        XCTAssertTrue(s.contains("xcrun simctl list devices available -j"))
    }

    func testXcodeFixture() throws {
        // the fixture itself: three iPhones and an iPad under iOS, an Apple TV under tvOS
        let text = try fixture("simctl-ok")
        XCTAssertEqual(text.components(separatedBy: "\"name\" : \"iPhone").count - 1, 3)
    }
}

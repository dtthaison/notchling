import CryptoKit
import XCTest
@testable import Notchling

/// The sd worker job's steps: plugins, worker.json, the units and LaunchAgent, the setup check, Running.
/// No ssh starts: runners are scripted, and the one real process is `/usr/bin/tar` on a temp directory.
final class MachinesSDWorkerStepsTests: XCTestCase {
    private let repo = TestMachines.repo
    private var temp: URL!

    override func setUpWithError() throws {
        temp = FileManager.default.temporaryDirectory.appendingPathComponent("sdworker-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true)
    }

    override func tearDown() {
        try? FileManager.default.removeItem(at: temp)
    }

    // MARK: helpers

    private func fixture(_ name: String, ext: String, sub: String = "Fixtures") throws -> URL {
        try XCTUnwrap(Bundle.module.url(forResource: name, withExtension: ext, subdirectory: sub))
    }

    private var templatesDir: URL {
        get throws { try fixture("sd-worker", ext: "service", sub: "Fixtures/worker-templates").deletingLastPathComponent() }
    }

    private func templates() throws -> [String: String] {
        try XCTUnwrap(ServiceFiles.templates(at: try templatesDir))
    }

    /// A fake home with the two fixtures copied in; the system-design marketplace is a directory in `temp`.
    private func fakeHome(marketplace: String = #"{"name":"system-design"}"#) throws -> URL {
        let home = temp.appendingPathComponent("home")
        let plugins = home.appendingPathComponent(".claude/plugins")
        try FileManager.default.createDirectory(at: plugins, withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try fixture("installed_plugins", ext: "json"),
                                         to: plugins.appendingPathComponent("installed_plugins.json"))
        let market = temp.appendingPathComponent("system-design")
        try FileManager.default.createDirectory(at: market.appendingPathComponent(".claude-plugin"), withIntermediateDirectories: true)
        try marketplace.write(to: market.appendingPathComponent(".claude-plugin/marketplace.json"), atomically: true, encoding: .utf8)
        let known = try String(contentsOf: try fixture("known_marketplaces", ext: "json"))
            .replacingOccurrences(of: "@SYSTEM_DESIGN_DIR@", with: market.path)
        try known.write(to: plugins.appendingPathComponent("known_marketplaces.json"), atomically: true, encoding: .utf8)
        return home
    }

    private let sdHere = InstalledPlugin(version: "1.19.2", path: "/Users/x/.claude/plugins/cache/system-design/sd/1.19.2")
    private let spHere = InstalledPlugin(version: "7.0.0", path: "/Users/x/p/superpowers/7.0.0")

    private var mac: MacFacts {
        MacFacts(plugins: ["sd@system-design": sdHere, "superpowers@claude-plugins-official": spHere],
                 marketplaceJSON: ["system-design": #"{"name":"system-design"}"#])
    }

    private func step(_ id: StepID, _ machine: Machine, all: [Machine]? = nil) throws -> SetupStep {
        let job = try XCTUnwrap(machine.sdWorker)
        return try XCTUnwrap(SDWorkerSteps.all(for: machine, job: job, all: all ?? [machine]).first { $0.id == id })
    }

    private func check(_ id: StepID, machine: Machine = TestMachines.linux(), all: [Machine]? = nil,
                       out: String, code: Int32 = 0, mac: MacFacts? = nil) async throws -> CheckResult {
        let s = try step(id, machine, all: all)
        guard case let .remote(_, parse) = s.probe else { throw XCTSkip("not a remote probe") }
        return await parse(RemoteOutput(text: out, code: code), .test(machine: machine, all: all, mac: mac ?? self.mac))
    }

    private func script(_ id: StepID, machine: Machine = TestMachines.linux()) throws -> String {
        let s = try step(id, machine)
        guard case let .remote(script, _) = s.probe else { throw XCTSkip("not a remote probe") }
        return script
    }

    private func plan(_ id: StepID, machine: Machine = TestMachines.linux()) throws -> FixPlan {
        guard case let .run(plan) = try step(id, machine).fix else { throw XCTSkip("not a run fix") }
        return plan
    }

    private func pluginsJSON(sd: String?, sp: String? = "7.0.0") -> String {
        var entries: [String] = []
        if let sp { entries.append(#""superpowers@claude-plugins-official":[{"scope":"user","installPath":"/x","version":"\#(sp)"}]"#) }
        if let sd { entries.append(#""sd@system-design":[{"scope":"user","installPath":"/y","version":"\#(sd)"}]"#) }
        return #"{"version":2,"plugins":{\#(entries.joined(separator: ","))}}"#
    }

    private let sdLine = "sd=/home/agent/.local/share/agents/system-design/plugin/scripts/sd.py\n"

    // MARK: MacFacts and plugins

    func testMacFactsReadsVersionsAndPaths() throws {
        let facts = MacFacts.load(home: try fakeHome())
        XCTAssertEqual(facts.plugins["sd@system-design"],
                       InstalledPlugin(version: "1.19.2", path: "/Users/x/.claude/plugins/cache/system-design/sd/1.19.2"))
        // the user-scope entry, not the project one listed first
        XCTAssertEqual(facts.plugins["superpowers@claude-plugins-official"]?.version, "7.0.0")
        XCTAssertEqual(facts.marketplaceJSON["system-design"], #"{"name":"system-design"}"#)
    }

    func testMacFactsWithNothingInstalledIsEmpty() {
        let facts = MacFacts.load(home: temp.appendingPathComponent("nowhere"))
        XCTAssertTrue(facts.plugins.isEmpty)
        XCTAssertTrue(facts.marketplaceJSON.isEmpty)
    }

    func testPluginsPassOnlyAtTheMacsVersions() async throws {
        let old = try await check(.plugins, out: pluginsJSON(sd: "1.19.1") + "\n@@sd\n" + sdLine)
        XCTAssertEqual(old.state, .fail("sd 1.19.1 here, 1.19.2 on this Mac"))

        let ok = try await check(.plugins, out: pluginsJSON(sd: "1.19.2") + "\n@@sd\n" + sdLine)
        XCTAssertEqual(ok.state, .pass("superpowers 7.0.0, sd 1.19.2"))
        XCTAssertEqual(ok.detected.sdPath, "/home/agent/.local/share/agents/system-design/plugin/scripts/sd.py")

        let none = try await check(.plugins, out: "\n@@sd\n")
        XCTAssertEqual(none.state, .fail("superpowers missing, 7.0.0 on this Mac; sd missing, 1.19.2 on this Mac"))
    }

    func testPluginsCantTellWhenThisMacLacksOne() async throws {
        let bare = MacFacts(plugins: ["sd@system-design": sdHere], marketplaceJSON: [:])
        let result = try await check(.plugins, out: pluginsJSON(sd: "1.19.2") + "\n@@sd\n" + sdLine, mac: bare)
        guard case .cantTell = result.state else { return XCTFail("\(result.state)") }
    }

    func testPluginsFailWhenSdPyIsNotWhereTheStepPutsIt() async throws {
        let result = try await check(.plugins, out: pluginsJSON(sd: "1.19.2") + "\n@@sd\n")
        guard case .fail = result.state else { return XCTFail("\(result.state)") }
        XCTAssertNil(result.detected.sdPath)
    }

    func testAnUnsafeSdPathIsNeverDetected() async throws {
        let result = try await check(.plugins, out: pluginsJSON(sd: "1.19.2") + "\n@@sd\nsd=/home/a;touch /tmp/x\n")
        XCTAssertNil(result.detected.sdPath)
    }

    func testPluginsProbeIsTheBriefsScript() throws {
        let s = try script(.plugins)
        XCTAssertTrue(s.contains(#"head -c 1048576 "$HOME/.claude/plugins/installed_plugins.json" 2>/dev/null"#))
        XCTAssertTrue(s.contains("echo @@sd"))
        XCTAssertTrue(s.contains(#"[ -f "$d" ] && printf 'sd=%s\n' "$d""#))
    }

    func testPluginVersionsFromTheMachineNeverReachAScript() async throws {
        let hostile = "1.0';touch /tmp/x;'"
        let result = try await check(.plugins, out: pluginsJSON(sd: hostile) + "\n@@sd\n" + sdLine)
        XCTAssertEqual(result.state, .fail("sd (unreadable version) here, 1.19.2 on this Mac"))
        XCTAssertFalse("\(result)".contains("touch /tmp/x"))

        // the Fix takes only this Mac's facts: its display and scripts hold nothing from the machine
        let runner = ScriptedRunner()
        let plan = try plan(.plugins)
        XCTAssertFalse(plan.display.contains("touch /tmp/x"))
        let fixCtx = FixContext.test(.test(runner: runner, mac: mac))
        _ = await plan.run(fixCtx)
        for call in runner.calls { XCTAssertFalse(call.argv.joined(separator: " ").contains("touch /tmp/x")) }
    }

    func testPluginsFixShipsThisMacsSd() async throws {
        let sdDir = temp.appendingPathComponent("sd-1.19.2")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("scripts"), withIntermediateDirectories: true)
        try "print('sd')".write(to: sdDir.appendingPathComponent("scripts/sd.py"), atomically: true, encoding: .utf8)
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path),
                                       "superpowers@claude-plugins-official": spHere],
                             marketplaceJSON: ["system-design": #"{"name":"it's"}"#])
        let runner = RoutingRunner()
        let outcome = try await plan(.plugins).run(.test(.test(runner: runner, mac: facts)))
        XCTAssertTrue(outcome.ok, outcome.message)

        let tar = try XCTUnwrap(runner.tarCalls.first)
        XCTAssertEqual(tar.argv.first, "/usr/bin/tar")
        XCTAssertTrue(tar.argv.contains(sdDir.path))
        XCTAssertEqual(tar.env["COPYFILE_DISABLE"], "1", "no ._ metadata entries in the archive")
        let ssh = try XCTUnwrap(runner.sshCalls.first)
        XCTAssertEqual(ssh.argv.first, "/usr/bin/ssh")
        let tgz = try XCTUnwrap(runner.producedArchive)
        XCTAssertFalse(tgz.isEmpty)
        XCTAssertEqual(ssh.stdin, tgz)
        let text = ssh.argv.last ?? ""
        XCTAssertTrue(text.contains("claude plugin install sd@system-design"))
        XCTAssertTrue(text.contains("claude plugin install superpowers@claude-plugins-official"))
        // the old folder is set aside, not deleted, before the new one goes in; a failed move puts it back
        XCTAssertFalse(text.contains(#"rm -rf "$d"; "#))
        XCTAssertTrue(text.contains(#"mv "$d" "$d.old""#))
        XCTAssertTrue(text.contains(#"mv "$d.old" "$d""#))
        // the temp archive is gone
        XCTAssertFalse(FileManager.default.fileExists(atPath: try XCTUnwrap(runner.archivePath)))
    }

    func testPluginsFixWithoutThisMacsSdFailsBeforeAnyCommand() async throws {
        let runner = ScriptedRunner()
        let outcome = try await plan(.plugins).run(.test(.test(runner: runner, mac: .empty)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(runner.calls.isEmpty)
    }

    // MARK: worker.json

    func testWorkerJSONOwnedKeys() throws {
        let m1 = TestMachines.mac(), vps = TestMachines.linux()
        let owned = try WorkerJSON.owned(machine: vps, job: XCTUnwrap(vps.sdWorker), all: [m1, vps]).get()
        XCTAssertEqual(owned["machine"] as? String, "vps-sing-1")
        XCTAssertEqual(owned["token_file"] as? String, "~/.config/agents/github-token")
        XCTAssertEqual(owned["claude_env"] as? String, "~/.config/agents/claude.env")
        XCTAssertTrue(owned["token_expires"] is NSNull)
        XCTAssertTrue(owned["health_url"] is NSNull)
        let repos = try XCTUnwrap(owned["repos"] as? [[String: Any]])
        XCTAssertEqual(repos.count, 1)
        XCTAssertEqual(repos[0]["github"] as? String, repo)
        XCTAssertEqual(repos[0]["kinds"] as? [String: String], ["ios": "m1", "backend": "vps-sing-1"])
        XCTAssertEqual(repos[0]["path"] as? String, "~/src/lingeva-mono")
        XCTAssertEqual(repos[0]["check_cmd"] as? String, "true")
        XCTAssertNil(repos[0]["verify_skill"])
    }

    func testWorkerJSONUsesTheRepoPathVerifySkillAndSettings() throws {
        var vps = TestMachines.linux()
        vps.jobs[0].sdWorker?.repos[0].machinePath = "/srv/lingeva"
        vps.jobs[0].sdWorker?.repos[0].verifySkill = ".claude/skills/verify/SKILL.md"
        vps.jobs[0].sdWorker?.healthURL = "https://x.example/health"
        vps.jobs[0].sdWorker?.tokenExpires = "2027-02-01"
        let owned = try WorkerJSON.owned(machine: vps, job: XCTUnwrap(vps.sdWorker), all: [vps]).get()
        XCTAssertEqual(owned["health_url"] as? String, "https://x.example/health")
        XCTAssertEqual(owned["token_expires"] as? String, "2027-02-01")
        let repos = try XCTUnwrap(owned["repos"] as? [[String: Any]])
        XCTAssertEqual(repos[0]["path"] as? String, "/srv/lingeva")
        XCTAssertEqual(repos[0]["verify_skill"] as? String, "/srv/lingeva/.claude/skills/verify/SKILL.md")
    }

    func testWorkerJSONKeepsKeysNotchlingDoesNotOwn() throws {
        let vps = TestMachines.linux()
        let owned = try WorkerJSON.owned(machine: vps, job: XCTUnwrap(vps.sdWorker), all: [TestMachines.mac(), vps]).get()
        let remote = Data(#"""
        {"machine":"vps-sing-1","token_file":"~/.config/agents/github-token","claude_env":"~/.config/agents/claude.env",
         "token_expires":null,"health_url":"http://old",
         "budget":{"weekly_pct":50},
         "testflight":{"app_id":"1","key_id":"k","issuer_id":"i","p8":"~/k.p8","repo":"a/b"},
         "repos":[{"github":"dtthaison/lingeva-mono","path":"~/src/lingeva-mono","check_cmd":"true","writer_model":"opus",
                   "kinds":{"ios":"m1","backend":"vps-sing-1"}}]}
        """#.utf8)
        XCTAssertEqual(WorkerJSON.differences(remote: remote, owned: owned), ["health_url"])

        let merged = try XCTUnwrap(JSONSerialization.jsonObject(with: WorkerJSON.merged(remote: remote, owned: owned)) as? [String: Any])
        XCTAssertEqual((merged["budget"] as? [String: Any])?["weekly_pct"] as? Int, 50)
        XCTAssertNotNil(merged["testflight"])
        XCTAssertTrue(merged["health_url"] is NSNull)
        let repos = try XCTUnwrap(merged["repos"] as? [[String: Any]])
        XCTAssertEqual(repos[0]["writer_model"] as? String, "opus")
        XCTAssertEqual(WorkerJSON.differences(remote: WorkerJSON.merged(remote: remote, owned: owned), owned: owned), [])
    }

    func testWorkerJSONMergedIsSortedAndPretty() throws {
        let vps = TestMachines.linux()
        let owned = try WorkerJSON.owned(machine: vps, job: XCTUnwrap(vps.sdWorker), all: [vps]).get()
        let text = String(decoding: WorkerJSON.merged(remote: nil, owned: owned), as: UTF8.self)
        XCTAssertTrue(text.hasSuffix("\n"))
        XCTAssertTrue(text.contains("\n  \"claude_env\""))
        XCTAssertLessThan(try XCTUnwrap(text.range(of: "claude_env")).lowerBound, try XCTUnwrap(text.range(of: "token_file")).lowerBound)
        XCTAssertTrue(text.contains("~/.config/agents/claude.env"), "slashes stay unescaped")
        // a missing key equals null, so the two null keys are not differences
        XCTAssertEqual(WorkerJSON.differences(remote: nil, owned: owned), ["claude_env", "machine", "repos", "token_file"])
    }

    func testWorkerJSONDifferencesNameTheOwnedKeysThatDiffer() throws {
        var vps = TestMachines.linux()
        vps.jobs[0].sdWorker?.healthURL = "https://x/h"
        let owned = try WorkerJSON.owned(machine: vps, job: XCTUnwrap(vps.sdWorker), all: [vps]).get()
        let remote = Data(#"""
        {"machine":"vps-sing-1","token_file":"~/.config/agents/github-token","claude_env":"~/.config/agents/claude.env",
         "repos":[{"github":"dtthaison/lingeva-mono","path":"~/src/lingeva-mono","check_cmd":"false","kinds":{"backend":"vps-sing-1"}}]}
        """#.utf8)
        XCTAssertEqual(WorkerJSON.differences(remote: remote, owned: owned), ["health_url", "repos"])
    }

    func testAKindClaimedByTwoMachinesFails() {
        var m1 = TestMachines.mac()
        m1.jobs[0].sdWorker?.kinds = ["ios", "backend"]
        let vps = TestMachines.linux()
        let result = WorkerJSON.owned(machine: vps, job: vps.sdWorker!, all: [m1, vps])
        XCTAssertEqual(result.failure, "kind backend of \(repo) is claimed by m1 and vps-sing-1")
    }

    func testNoKindsFails() {
        var vps = TestMachines.linux()
        vps.jobs[0].sdWorker?.kinds = []
        XCTAssertEqual(WorkerJSON.owned(machine: vps, job: vps.sdWorker!, all: [vps]).failure, "no kinds chosen for vps-sing-1")
    }

    func testOwnedRefusesWhatSdRejects() {
        func failure(_ edit: (inout Machine) -> Void) -> StepFailure? {
            var vps = TestMachines.linux()
            edit(&vps)
            return WorkerJSON.owned(machine: vps, job: vps.sdWorker!, all: [vps]).failure
        }
        XCTAssertEqual(failure { $0.jobs[0].sdWorker?.repos = [] }, "no repos chosen for vps-sing-1")
        XCTAssertEqual(failure { $0.jobs[0].sdWorker?.repos[0].checkCommand = "  " }, "no check command for \(repo)")
        XCTAssertEqual(failure { $0.jobs[0].sdWorker?.kinds = ["ios", "web"] }, "kind web must be ios or backend")
        for name in ["VPS", "-vps", "vps_1", "vps sing", ""] {
            XCTAssertNotNil(failure { $0.name = name }, name)
        }
        XCTAssertNil(failure { $0.name = "vps-2" })
    }

    func testWorkerJSONNeedsAbsolutePythonAndSdPath() {
        var vps = TestMachines.linux()
        vps.jobs[0].sdWorker?.python = "python3"
        XCTAssertEqual(WorkerJSON.owned(machine: vps, job: vps.sdWorker!, all: [vps]).failure,
                       "python or sd.py is not an absolute path yet")
    }

    func testWorkerJSONProbeAndFix() async throws {
        let vps = TestMachines.linux()
        let probe = try script(.workerJSON)
        XCTAssertEqual(probe, #"head -c 262144 "$HOME/.config/sd/worker.json" 2>/dev/null"#)

        let owned = try WorkerJSON.owned(machine: vps, job: XCTUnwrap(vps.sdWorker), all: [vps]).get()
        let current = String(decoding: WorkerJSON.merged(remote: nil, owned: owned), as: UTF8.self)
        let same = try await check(.workerJSON, out: current)
        XCTAssertEqual(same.state, .pass("current"))

        let stale = try await check(.workerJSON, out: current.replacingOccurrences(of: "\"machine\" : \"vps-sing-1\"", with: "\"machine\" : \"other\""))
        guard case let .fail(why) = stale.state else { return XCTFail("\(stale.state)") }
        XCTAssertTrue(why.hasPrefix("differs: "), why)

        let missing = try await check(.workerJSON, out: "")
        XCTAssertEqual(missing.state, .fail("missing"))
        let broken = try await check(.workerJSON, out: "{not json")
        XCTAssertEqual(broken.state, .fail("not valid JSON"))

        // the Fix reads the file, merges, and writes the merged bytes on stdin (mode 600)
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("head -c 262144") == true },
                  result: CommandResult(code: 0, out: #"{"budget":{"weekly_pct":50}}"#, err: ""))
        let outcome = try await plan(.workerJSON).run(.test(.test(machine: vps, runner: runner)))
        XCTAssertTrue(outcome.ok, outcome.message)
        let write = try XCTUnwrap(runner.calls.last)
        XCTAssertTrue(write.argv.last?.contains("chmod 600") == true)
        let written = try XCTUnwrap(JSONSerialization.jsonObject(with: try XCTUnwrap(write.stdin)) as? [String: Any])
        XCTAssertEqual(written["claude_env"] as? String, "~/.config/agents/claude.env")
        XCTAssertNotNil(written["budget"])
    }

    func testWorkerJSONFixWritesANewFileWhenTheMachineHasNone() async throws {
        let vps = TestMachines.linux()
        for code: Int32 in [0, 1] {
            let runner = ScriptedRunner()
            runner.on({ $0.last?.contains("head -c 262144") == true }, result: CommandResult(code: code, out: "", err: ""))
            let outcome = try await plan(.workerJSON).run(.test(.test(machine: vps, runner: runner)))
            XCTAssertTrue(outcome.ok, "\(code): \(outcome.message)")
            XCTAssertEqual(runner.calls.count, 2, "read, then write")
            let written = try XCTUnwrap(WorkerJSON.parse(runner.calls.last?.stdin))
            XCTAssertEqual(written["machine"] as? String, "vps-sing-1")
        }
    }

    func testAReadErrorIsNeverTreatedAsAMissingFile() async throws {
        // exit 1 with nothing on stdout, but a reason on stderr: the file exists and can't be read
        for (code, err) in [(Int32(1), "head: /home/agent/.config/sd/worker.json: Permission denied\n"), (0, "sh: Permission denied\n")] {
            let runner = ScriptedRunner()
            runner.on({ $0.last?.contains("head -c 262144") == true }, result: CommandResult(code: code, out: "", err: err))
            let outcome = try await plan(.workerJSON).run(.test(.test(runner: runner)))
            XCTAssertFalse(outcome.ok, "\(code)")
            XCTAssertTrue(outcome.message.contains("Permission denied"), outcome.message)
            XCTAssertEqual(runner.calls.count, 1, "the file was not overwritten")
        }
    }

    func testWorkerJSONFixShowsTheReasonOfARealReadError() async throws {
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("head -c 262144") == true },
                  result: CommandResult(code: 1, out: "", err: "head: I/O error\n"))
        var outcome = try await plan(.workerJSON).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("head: I/O error"), outcome.message)
        XCTAssertEqual(runner.calls.count, 1)

        let ssh = ScriptedRunner()
        ssh.on({ $0.last?.contains("head -c 262144") == true }, result: CommandResult(code: 255, out: "", err: "ssh: connect timed out\n"))
        outcome = try await plan(.workerJSON).run(.test(.test(runner: ssh)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("connect timed out"), outcome.message)
        XCTAssertEqual(ssh.calls.count, 1, "nothing was written")
    }

    func testWorkerJSONFixRefusesToOverwriteAFileItCantRead() async throws {
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("head -c 262144") == true }, result: CommandResult(code: 0, out: "{broken", err: ""))
        let outcome = try await plan(.workerJSON).run(.test(.test(runner: runner)))
        XCTAssertFalse(outcome.ok)
        XCTAssertEqual(runner.calls.count, 1, "nothing was written")
    }

    // MARK: service files

    func testServiceFilesForLinux() throws {
        let files = try ServiceFiles.render(templates: templates(), linux: true, user: "agent", home: "/home/agent",
                                            python: "/usr/bin/python3", sdPath: MachinePaths.sdPy(home: "/home/agent")).get()
        XCTAssertEqual(files.count, 4)
        XCTAssertTrue(files.allSatisfy { $0.remotePath == "/etc/systemd/system/\($0.name)" })
        XCTAssertEqual(files.map(\.name).sorted(), ["sd-testflight.service", "sd-testflight.timer", "sd-worker.service", "sd-worker.slice"])
        let service = try XCTUnwrap(files.first { $0.name == "sd-worker.service" })
        let lines = service.content.components(separatedBy: "\n")
        XCTAssertTrue(lines.contains("User=agent"))
        XCTAssertTrue(lines.contains("ExecStart=/usr/bin/python3 /home/agent/.local/share/agents/system-design/plugin/scripts/sd.py worker run"))
        XCTAssertTrue(lines.contains("Environment=PATH=" + RemoteShell.servicePath(home: "/home/agent")))
        for file in files {
            XCTAssertFalse(file.content.contains("@SD_PY@") || file.content.contains("@PYTHON3@") || file.content.contains("@HOME@"), file.name)
            XCTAssertFalse(file.content.contains("EnvironmentFile"), file.name)
        }
    }

    func testServiceFilesSetTheUser() throws {
        let files = try ServiceFiles.render(templates: templates(), linux: true, user: "worker", home: "/home/worker",
                                            python: "/usr/bin/python3", sdPath: MachinePaths.sdPy(home: "/home/worker")).get()
        for name in ["sd-worker.service", "sd-testflight.service"] {
            let lines = try XCTUnwrap(files.first { $0.name == name }).content.components(separatedBy: "\n")
            XCTAssertTrue(lines.contains("User=worker"), name)
            XCTAssertFalse(lines.contains("User=agent"), name)
        }
    }

    func testServiceFilesRefuseRootAndOldTemplates() throws {
        let sd = MachinePaths.sdPy(home: "/home/agent")
        XCTAssertEqual(ServiceFiles.render(templates: try templates(), linux: true, user: "root", home: "/root",
                                           python: "/usr/bin/python3", sdPath: sd).failure,
                       "the worker must not run as root")
        var old = try templates()
        old["sd-worker.service"] = old["sd-worker.service"]! + "EnvironmentFile=/home/agent/.config/agents/claude.env\n"
        XCTAssertEqual(ServiceFiles.render(templates: old, linux: true, user: "agent", home: "/home/agent",
                                           python: "/usr/bin/python3", sdPath: sd).failure,
                       "sd on this Mac is older than 1.19.3; update it first")
        var leftover = try templates()
        leftover["sd-worker.slice"] = leftover["sd-worker.slice"]! + "Foo=@MYSTERY_1@\n"
        guard case .failure = ServiceFiles.render(templates: leftover, linux: true, user: "agent", home: "/home/agent",
                                                  python: "/usr/bin/python3", sdPath: sd) else { return XCTFail("a leftover placeholder passed") }
        var noUser = try templates()
        noUser["sd-worker.service"] = noUser["sd-worker.service"]!.replacingOccurrences(of: "User=agent\n", with: "")
        guard case .failure = ServiceFiles.render(templates: noUser, linux: true, user: "agent", home: "/home/agent",
                                                  python: "/usr/bin/python3", sdPath: sd) else { return XCTFail("a unit with no User= passed") }
    }

    func testServiceFilesRefuseUnsafeValues() throws {
        let t = try templates()
        XCTAssertNotNil(ServiceFiles.render(templates: t, linux: true, user: "agent\nExecStartPre=/bin/x", home: "/home/agent",
                                            python: "/usr/bin/python3", sdPath: "/a/sd.py").failure)
        XCTAssertNotNil(ServiceFiles.render(templates: t, linux: true, user: "agent", home: "/home/agent",
                                            python: "/usr/bin/python3 --evil", sdPath: "/a/sd.py").failure)
        XCTAssertNotNil(ServiceFiles.render(templates: t, linux: true, user: "agent", home: "/home/a b",
                                            python: "/usr/bin/python3", sdPath: "/a/sd.py").failure)
        XCTAssertNotNil(ServiceFiles.render(templates: t, linux: true, user: "agent", home: "/home/agent",
                                            python: "python3", sdPath: "/a/sd.py").failure)
    }

    func testServiceFilesForMac() throws {
        let sd = MachinePaths.sdPy(home: "/Users/sondoan")
        let files = try ServiceFiles.render(templates: templates(), linux: false, user: "sondoan", home: "/Users/sondoan",
                                            python: "/opt/homebrew/bin/python3", sdPath: sd).get()
        XCTAssertEqual(files.count, 1)
        XCTAssertEqual(files[0].name, "com.sd.worker.plist")
        XCTAssertEqual(files[0].remotePath, "~/Library/LaunchAgents/com.sd.worker.plist")
        let plist = try XCTUnwrap(PropertyListSerialization.propertyList(from: Data(files[0].content.utf8), format: nil) as? [String: Any])
        XCTAssertEqual(plist["ProgramArguments"] as? [String], ["/opt/homebrew/bin/python3", sd, "worker", "run"])
        XCTAssertEqual((plist["EnvironmentVariables"] as? [String: String])?["PATH"], RemoteShell.servicePath(home: "/Users/sondoan"))
        XCTAssertEqual(plist["StandardOutPath"] as? String, "/Users/sondoan/Library/Logs/sd-worker.log")
        XCTAssertEqual(plist["StandardErrorPath"] as? String, "/Users/sondoan/Library/Logs/sd-worker.log")
        XCTAssertFalse(files[0].content.contains("@"))
        XCTAssertFalse(files[0].content.contains("EnvironmentFile"))
    }

    func testTemplatesAreReadFromADirectoryAndNilWhenOneIsMissing() throws {
        XCTAssertEqual(try templates().count, 5)
        XCTAssertNil(ServiceFiles.templates(at: temp))
    }

    func testLinuxServiceFixOnlyWritesTheFourUnitsAndReloads() async throws {
        let files = try ServiceFiles.render(templates: templates(), linux: true, user: "agent", home: "/home/agent",
                                            python: "/usr/bin/python3", sdPath: MachinePaths.sdPy(home: "/home/agent")).get()
        let script = SDWorkerSteps.installScript(files: files)
        let lines = script.components(separatedBy: "\n")
        XCTAssertEqual(lines.filter { $0.contains("base64 -d > /etc/systemd/system/") }.count, 4)
        XCTAssertTrue(script.contains("systemctl daemon-reload"))
        XCTAssertTrue(script.hasPrefix("set -e\numask 022\n"))
        // the words are looked for outside the base64 payloads
        let outside = lines.map { $0.replacingOccurrences(of: "'[A-Za-z0-9+/=]*'", with: "''", options: .regularExpression) }
        XCTAssertTrue(outside.contains("printf '%s' '' | base64 -d > /etc/systemd/system/sd-worker.service.new"), outside.joined(separator: "\n"))
        for word in ["enable", "start", "restart", "stop", "disable"] {
            XCTAssertFalse(outside.joined(separator: "\n").contains(word), word)
        }
        // each unit goes through a temp file and a move
        XCTAssertEqual(lines.filter { $0.hasPrefix("mv -f /etc/systemd/system/") }.count, 4)
        for service in MachineSafety.productionServices { XCTAssertFalse(script.contains(service), service) }
        XCTAssertEqual(script.components(separatedBy: "systemctl").count - 1, 1)

        // through the engine's path: admin script, password only on stdin, with the macs templates read from the Mac's sd dir
        let sdDir = temp.appendingPathComponent("sd")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])
        let runner = ScriptedRunner()
        var vps = TestMachines.linux()
        vps.admin = .rootSSH                   // on Linux the admin is root over ssh, never the worker account
        vps.jobs[0].sdWorker?.sdPath = MachinePaths.sdPy(home: "/home/agent")
        let outcome = try await plan(.service).run(.test(.test(machine: vps, runner: runner, mac: facts)))
        XCTAssertTrue(outcome.ok, outcome.message)
        let call = try XCTUnwrap(runner.calls.first)
        XCTAssertTrue(call.argv.contains("root@vps-sing-1"))
        XCTAssertFalse(try XCTUnwrap(call.argv.last).contains("sudo"))
        XCTAssertNil(call.stdin)
        XCTAssertEqual(runner.calls.count, 1)
    }

    func testServiceFixOnAMacWritesOnlyTheLaunchAgentWithoutAdmin() async throws {
        let sdDir = temp.appendingPathComponent("sd")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])
        let m1 = TestMachines.mac()
        let plan = try plan(.service, machine: m1)
        XCTAssertFalse(plan.needsAdmin)
        let runner = ScriptedRunner()
        let outcome = await plan.run(.test(.test(machine: m1, runner: runner, mac: facts)))
        XCTAssertTrue(outcome.ok, outcome.message)
        // it looks at whether Running is on, then the label is disabled first, so a failure leaves no plist for a
        // login to start
        XCTAssertEqual(runner.calls.first?.argv.last?.contains("print-disabled"), true)
        XCTAssertEqual(runner.calls[1].argv.last?.contains(SDWorkerSteps.macDisableScript), true)
        XCTAssertNil(runner.calls[1].stdin)
        let call = try XCTUnwrap(runner.calls.last)
        XCTAssertTrue(try XCTUnwrap(call.argv.last).contains("com.sd.worker.plist"))
        XCTAssertTrue(try XCTUnwrap(call.argv.last).contains("chmod 644"))
        XCTAssertFalse(try XCTUnwrap(call.argv.last).contains("launchctl"))
        let plist = try XCTUnwrap(PropertyListSerialization.propertyList(from: try XCTUnwrap(call.stdin), format: nil) as? [String: Any])
        XCTAssertEqual((plist["ProgramArguments"] as? [String])?.first, "/opt/homebrew/bin/python3")
        // the plist is sd's own (RunAtLoad and KeepAlive stay), so the Fix disables the label until Running is turned on
        XCTAssertEqual(plist["RunAtLoad"] as? Bool, true)
        XCTAssertEqual(plist["KeepAlive"] as? Bool, true)
        XCTAssertEqual(runner.calls.count, 3, "the state probe, the disable, the write")
        // never loads or enables it
        let all = runner.calls.map { $0.argv.joined(separator: " ") }.joined(separator: "\n")
        for verb in ["bootstrap", "load", "enable", "kickstart", "bootout"] {
            XCTAssertNil(all.range(of: "launchctl \(verb)"), verb)
        }
        XCTAssertEqual(SDWorkerSteps.macDisableScript, #"launchctl disable "gui/$(id -u)/com.sd.worker""#)
        XCTAssertTrue(plan.display.contains(SDWorkerSteps.macDisableScript))
    }

    func testServiceFixOnAMacLeavesNoPlistWhenTheDisableFails() async throws {
        let sdDir = temp.appendingPathComponent("sd")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("launchctl disable") == true }, result: CommandResult(code: 1, out: "", err: "no GUI domain\n"))
        let m1 = TestMachines.mac()
        let outcome = try await plan(.service, machine: m1).run(.test(.test(machine: m1, runner: runner, mac: facts)))
        XCTAssertFalse(outcome.ok)
        XCTAssertTrue(outcome.message.contains("no GUI domain"), outcome.message)
        XCTAssertTrue(outcome.message.contains("no service file was written"), outcome.message)
        XCTAssertEqual(runner.calls.count, 2, "the state probe and the failed disable; stopped before the write")
        XCTAssertTrue(runner.calls.allSatisfy { $0.stdin == nil })
        XCTAssertFalse(runner.calls.contains { $0.argv.joined().contains("LaunchAgents") })
    }

    private func macServiceCheck(digestMatches: Bool = true, extra: String) async throws -> CheckResult {
        let sdDir = temp.appendingPathComponent("sd-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])
        let m1 = TestMachines.mac()
        let files = try ServiceFiles.render(templates: templates(), linux: false, user: "sondoan", home: "/Users/sondoan",
                                            python: "/opt/homebrew/bin/python3", sdPath: MachinePaths.sdPy(home: "/Users/sondoan")).get()
        let digest = SHA256.hash(data: Data((digestMatches ? files[0].content : "other").utf8)).map { String(format: "%02x", $0) }.joined()
        return try await check(.service, machine: m1, out: "\(digest)  /x/com.sd.worker.plist\n" + extra, mac: facts)
    }

    func testMacServiceCheckFailsWhenInstalledButNotDisabled() async throws {
        let probe = try script(.service, machine: TestMachines.mac())
        XCTAssertTrue(probe.contains(#"launchctl print-disabled "gui/$(id -u)""#), probe)
        XCTAssertTrue(probe.contains(#"launchctl print "gui/$(id -u)/com.sd.worker""#), probe)
        let line = { (state: String) in "\t\"com.sd.worker\" => \(state)\n" }

        // plist present, label not disabled (unlisted or enabled), not loaded: a login would start it
        for extra in ["notloaded\n", "notloaded\n" + line("enabled")] {
            let r = try await macServiceCheck(extra: extra)
            XCTAssertEqual(r.state, .fail("installed but not disabled; a login would start it"), extra)
        }
        // plist present and disabled: pass, loaded or not
        for extra in ["notloaded\n" + line("disabled"), "notloaded\n" + line("true"), "loaded\n" + line("disabled")] {
            let r = try await macServiceCheck(extra: extra)
            XCTAssertEqual(r.state, .pass("com.sd.worker.plist matches"), extra)
        }
        // Running is on: enabled and loaded
        for extra in ["loaded\n", "loaded\n" + line("enabled")] {
            let r = try await macServiceCheck(extra: extra)
            XCTAssertEqual(r.state, .pass("com.sd.worker.plist matches"), extra)
        }
        // a plist that differs still says so first
        let differs = try await macServiceCheck(digestMatches: false, extra: "notloaded\n")
        XCTAssertEqual(differs.state, .fail("differs: com.sd.worker.plist"))
    }

    func testServiceProbeComparesTheDigestsOfTheRenderedUnits() async throws {
        let sdDir = temp.appendingPathComponent("sd")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])
        var vps = TestMachines.linux()
        vps.jobs[0].sdWorker?.sdPath = MachinePaths.sdPy(home: "/home/agent")
        let probe = try script(.service)
        XCTAssertEqual(probe.components(separatedBy: "sha256sum").count - 1, 4)
        XCTAssertTrue(probe.contains("missing sd-worker.slice"))

        let files = try ServiceFiles.render(templates: templates(), linux: true, user: "agent", home: "/home/agent",
                                            python: "/usr/bin/python3", sdPath: MachinePaths.sdPy(home: "/home/agent")).get()
        func digest(_ s: String) -> String { SHA256.hash(data: Data(s.utf8)).map { String(format: "%02x", $0) }.joined() }
        let good = files.map { "\(digest($0.content))  \($0.remotePath)" }.joined(separator: "\n")
        let pass = try await check(.service, machine: vps, out: good, mac: facts)
        XCTAssertEqual(pass.state, .pass("4 units match"))

        var lines = good.components(separatedBy: "\n")
        lines[1] = String(repeating: "0", count: 64) + "  /etc/systemd/system/x"
        lines[2] = "missing sd-testflight.service"
        let bad = try await check(.service, machine: vps, out: lines.joined(separator: "\n"), mac: facts)
        XCTAssertEqual(bad.state, .fail("differs: \(files[1].name); missing: \(files[2].name)"))

        let macProbe = try script(.service, machine: TestMachines.mac())
        XCTAssertTrue(macProbe.contains("shasum -a 256"))
        XCTAssertFalse(macProbe.contains("sha256sum"))
    }

    // MARK: sd worker setup --check

    private let checkRows = #"""
    [{"name":"tools","ok":true,"detail":"claude, git, gh"},
     {"name":"labels","ok":false,"detail":"missing sd:queued"},
     {"name":"token expiry","ok":false,"detail":"9 days left"},
     {"name":"com.sd.worker","ok":false,"detail":"the LaunchAgent is not loaded"}]
    """#

    func testSetupCheckRowsBecomeSubSteps() async throws {
        let m1 = TestMachines.mac()
        let failed = try await check(.sdCheck, machine: m1, out: "\(checkRows)\n@@exit 4\n", code: 0)
        XCTAssertEqual(failed.state, .fail("2 of 3 checks failed"))
        XCTAssertEqual(failed.rows.count, 4, "the com.sd.worker row is listed")
        XCTAssertEqual(failed.rows[0], SubRow(name: "tools", ok: true, detail: "claude, git, gh"))
        XCTAssertEqual(failed.rows[3].name, "com.sd.worker")

        let passRows = #"[{"name":"tools","ok":true,"detail":"x"},{"name":"login","ok":true,"detail":"y"}]"#
        let ok = try await check(.sdCheck, machine: m1, out: "\(passRows)\n@@exit 0\n")
        XCTAssertEqual(ok.state, .pass("all 2 checks passed"))

        // only the Running row fails: exit 4, but nothing the user has to fix here
        let onlyRunning = #"[{"name":"tools","ok":true,"detail":"x"},{"name":"com.sd.worker","ok":false,"detail":"not loaded"}]"#
        let idle = try await check(.sdCheck, machine: m1, out: "\(onlyRunning)\n@@exit 4\n")
        XCTAssertEqual(idle.state, .pass("all 1 checks passed"))

        let broken = try await check(.sdCheck, machine: m1,
                                     out: "\n@@exit 2\nconfig: token_file: ~/.config/agents/github-token does not exist\nsecond line\n")
        XCTAssertEqual(broken.state, .fail("config: token_file: ~/.config/agents/github-token does not exist"))
        XCTAssertTrue(broken.rows.isEmpty)

        let empty = try await check(.sdCheck, machine: m1, out: "\n@@exit 3\ntoken_file /x is empty\n")
        XCTAssertEqual(empty.state, .fail("token_file /x is empty"))
    }

    func testSetupCheckBoundsWhatTheMachineSends() async throws {
        let long = String(repeating: "a", count: 1000)
        let many = (0..<300).map { #"{"name":"r\#($0)","ok":true,"detail":"\#(long)"}"# }.joined(separator: ",")
        let result = try await check(.sdCheck, machine: TestMachines.mac(), out: "[\(many)]\n@@exit 0\n")
        XCTAssertEqual(result.rows.count, 200)
        XCTAssertEqual(result.rows[0].detail.count, 300)
        XCTAssertEqual(result.state, .pass("all 200 checks passed"))

        let garbage = try await check(.sdCheck, machine: TestMachines.mac(), out: "not json\n@@exit 0\n")
        guard case .fail = garbage.state else { return XCTFail("\(garbage.state)") }
        let noMarker = try await check(.sdCheck, machine: TestMachines.mac(), out: "")
        guard case .fail = noMarker.state else { return XCTFail("\(noMarker.state)") }
    }

    /// A row's text and a failure line are masked before they are cut to 300 characters: the cut falls inside the token.
    func testSetupCheckMasksWhatTheMachineSaysBeforeItCutsIt() async throws {
        let token = testLongToken
        let marker = "[secret:claude-worker]"
        let m1 = TestMachines.mac()
        let ctx = StepContext.test(machine: m1, mac: mac, mask: testMask(token, as: marker))
        guard case let .remote(_, parse) = try step(.sdCheck, m1).probe else { return XCTFail("not remote") }
        let said = String(repeating: "f", count: 270) + token

        let rows = await parse(RemoteOutput(text: #"[{"name":"\#(said)","ok":true,"detail":"\#(said)"}]"# + "\n@@exit 0\n", code: 0), ctx)
        XCTAssertEqual(rows.rows.count, 1)
        assertNoFragment(of: token, in: rows.rows[0].name)
        assertNoFragment(of: token, in: rows.rows[0].detail)
        XCTAssertTrue(rows.rows[0].detail.contains(marker), rows.rows[0].detail)

        let broken = await parse(RemoteOutput(text: "\n@@exit 2\n\(said)\n", code: 0), ctx)
        guard case let .fail(text) = broken.state else { return XCTFail("\(broken.state)") }
        assertNoFragment(of: token, in: text)
        XCTAssertTrue(text.contains(marker), text)
    }

    /// The Fixes of this file: a write failure is masked before it is cut to 300 characters (the plugins install,
    /// the units, the LaunchAgent, worker.json's read).
    func testFixFailuresAreMaskedBeforeTheyAreCut() async throws {
        let token = testLongToken
        let marker = "[secret:claude-worker]"
        let said = String(repeating: "f", count: 270) + token
        let mask = testMask(token, as: marker)
        let sdDir = temp.appendingPathComponent("sd")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])

        // the LaunchAgent write (the disable before it succeeds)
        let m1 = TestMachines.mac()
        let agent = ScriptedRunner()
        agent.on({ $0.last?.contains("LaunchAgents") == true }, result: CommandResult(code: 1, out: "", err: said))
        let written = try await plan(.service, machine: m1).run(.test(.test(machine: m1, runner: agent, mac: facts, mask: mask)))
        XCTAssertFalse(written.ok)
        assertNoFragment(of: token, in: written.message)
        XCTAssertTrue(written.message.contains(marker), written.message)

        // the Linux units
        var vps = TestMachines.linux()
        vps.jobs[0].sdWorker?.sdPath = MachinePaths.sdPy(home: "/home/agent")
        let units = ScriptedRunner()
        units.on({ _ in true }, result: CommandResult(code: 1, out: "", err: said))
        let linux = try await plan(.service, machine: vps)
            .run(.test(.test(machine: vps, runner: units, mac: facts, mask: mask), adminPassword: "pw-123"))
        XCTAssertFalse(linux.ok)
        assertNoFragment(of: token, in: linux.message)

        // worker.json: a read error shows the first line of stderr
        let read = ScriptedRunner()
        read.on({ _ in true }, result: CommandResult(code: 1, out: "", err: said))
        let json = try await plan(.workerJSON, machine: m1).run(.test(.test(machine: m1, runner: read, mac: facts, mask: mask)))
        XCTAssertFalse(json.ok)
        assertNoFragment(of: token, in: json.message)
    }

    // MARK: Service Fix while Running is on

    private func macServiceFix(running probe: CommandResult) async throws -> (FixOutcome, ScriptedRunner) {
        let sdDir = temp.appendingPathComponent("sd-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: sdDir.appendingPathComponent("templates"), withIntermediateDirectories: true)
        try FileManager.default.copyItem(at: try templatesDir, to: sdDir.appendingPathComponent("templates/worker"))
        let facts = MacFacts(plugins: ["sd@system-design": InstalledPlugin(version: "1.19.2", path: sdDir.path)], marketplaceJSON: [:])
        let m1 = TestMachines.mac()
        let runner = ScriptedRunner()
        // the Running probe: `loaded`/`notloaded`, then the label's `print-disabled` line
        runner.on({ $0.last?.contains("print-disabled") == true }, result: probe)
        let outcome = try await plan(.service, machine: m1).run(.test(.test(machine: m1, runner: runner, mac: facts)))
        return (outcome, runner)
    }

    private func calls(_ runner: ScriptedRunner, containing text: String) -> Int {
        runner.calls.filter { $0.argv.last?.contains(text) == true }.count
    }

    /// With the label loaded and enabled, Running is on: the Fix rewrites the plist and leaves the label enabled.
    func testServiceFixLeavesTheLabelEnabledWhileRunningIsOn() async throws {
        let (outcome, runner) = try await macServiceFix(running: CommandResult(code: 1, out: "loaded\n", err: ""))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(calls(runner, containing: "launchctl disable"), 0, "Running would turn off")
        XCTAssertEqual(calls(runner, containing: "launchctl bootout"), 0)
        let write = try XCTUnwrap(runner.calls.last)
        XCTAssertTrue(try XCTUnwrap(write.argv.last).contains("com.sd.worker.plist"))
        XCTAssertNotNil(write.stdin, "the plist is written")
        XCTAssertEqual(runner.calls.count, 2, "the state probe, then the write")
    }

    /// A loaded label that `print-disabled` lists as enabled is Running on too.
    func testServiceFixLeavesAListedEnabledLabelEnabled() async throws {
        let (outcome, runner) = try await macServiceFix(running: CommandResult(code: 0, out: "loaded\n\t\"com.sd.worker\" => enabled\n", err: ""))
        XCTAssertTrue(outcome.ok, outcome.message)
        XCTAssertEqual(calls(runner, containing: "launchctl disable"), 0, "Running would turn off")
        XCTAssertEqual(runner.calls.count, 2, "the state probe, then the write")
        XCTAssertTrue(outcome.message.contains("Running stays on"), outcome.message)
    }

    /// Anything but loaded-and-enabled disables the label before the plist is written.
    func testServiceFixDisablesFirstUnlessRunningIsOn() async throws {
        let disabledLine = "\t\"com.sd.worker\" => disabled\n"
        let states: [(String, CommandResult)] = [
            ("not loaded, unlisted", CommandResult(code: 1, out: "notloaded\n", err: "")),
            ("not loaded, enabled", CommandResult(code: 0, out: "notloaded\n\t\"com.sd.worker\" => enabled\n", err: "")),
            ("loaded but disabled", CommandResult(code: 0, out: "loaded\n" + disabledLine, err: "")),
            ("the probe failed", CommandResult(code: 255, out: "", err: "ssh: connect timed out")),
            ("nothing printed", CommandResult(code: 0, out: "", err: "")),
        ]
        for (name, probe) in states {
            let (outcome, runner) = try await macServiceFix(running: probe)
            XCTAssertTrue(outcome.ok, "\(name): \(outcome.message)")
            let order = runner.calls.map { call -> String in
                let script = call.argv.last ?? ""
                if script.contains("launchctl disable") { return "disable" }
                if script.contains("com.sd.worker.plist") { return "write" }
                return script.contains("print-disabled") ? "probe" : "other"
            }
            XCTAssertEqual(order, ["probe", "disable", "write"], name)
        }
    }

    // MARK: sd 1.19.3: the check is read-only, its Fix creates the labels

    private let sd1193 = InstalledPlugin(version: "1.19.3", path: "/Users/x/.claude/plugins/cache/system-design/sd/1.19.3")

    private var mac1193: MacFacts {
        MacFacts(plugins: ["sd@system-design": sd1193, "superpowers@claude-plugins-official": spHere],
                 marketplaceJSON: ["system-design": #"{"name":"system-design"}"#])
    }

    private let labelRows = #"""
    [{"name":"tools","ok":true,"detail":"claude, git, gh"},
     {"name":"labels dtthaison/lingeva-mono","ok":true,"detail":"created sd:ready, sd:claimed; the token can write issues"},
     {"name":"com.sd.worker","ok":false,"detail":"the LaunchAgent is not loaded"}]
    """#

    func testTheSetupCheckKeepsItsReadOnlyCommandAndItsFixCreatesTheLabels() throws {
        let m1 = TestMachines.mac()
        let sd = MachinePaths.sdPy(home: "/Users/sondoan")
        let check = try script(.sdCheck, machine: m1)
        XCTAssertTrue(check.contains("'/opt/homebrew/bin/python3' -B '\(sd)' worker setup --check --json"), check)
        XCTAssertFalse(check.contains("--create-labels"), "the check never creates anything")
        let p = try plan(.sdCheck, machine: m1)
        XCTAssertFalse(p.needsAdmin)
        XCTAssertTrue(p.display.contains("'/opt/homebrew/bin/python3' -B '\(sd)' worker setup --create-labels --json"), p.display)
        XCTAssertFalse(p.display.contains("--check"), p.display)
        XCTAssertFalse(p.display.contains("--live"))
    }

    func testTheCreateLabelsFixRunsOnlyThatCommandAndReportsTheLabelRows() async throws {
        let m1 = TestMachines.mac()
        let runner = ScriptedRunner()
        runner.on({ $0.last?.contains("--create-labels") == true }, result: CommandResult(code: 0, out: "\(labelRows)\n@@exit 4\n", err: ""))
        let out = await (try plan(.sdCheck, machine: m1)).run(.test(.test(machine: m1, runner: runner, mac: mac1193)))
        XCTAssertEqual(runner.calls.count, 1)
        let sent = runner.calls[0].argv.last ?? ""
        XCTAssertTrue(sent.contains("worker setup --create-labels --json"), sent)
        XCTAssertFalse(sent.contains("--live"))
        XCTAssertEqual(out.ok, true, out.message)
        XCTAssertTrue(out.message.contains("created sd:ready, sd:claimed"), out.message)
    }

    func testTheCreateLabelsFixFailsOnALabelRowAndAsksForACheckWhenOtherRowsFail() async throws {
        let m1 = TestMachines.mac()
        let cannot = #"[{"name":"labels dtthaison/lingeva-mono","ok":false,"detail":"cannot create labels: HTTP 403"}]"#
        let denied = ScriptedRunner()
        denied.on({ _ in true }, result: CommandResult(code: 0, out: "\(cannot)\n@@exit 4\n", err: ""))
        let no = try await plan(.sdCheck, machine: m1).run(.test(.test(machine: m1, runner: denied, mac: mac1193)))
        XCTAssertFalse(no.ok)
        XCTAssertTrue(no.message.contains("cannot create labels: HTTP 403"), no.message)

        let others = #"[{"name":"labels dtthaison/lingeva-mono","ok":true,"detail":"6 labels present"},{"name":"token expiry","ok":false,"detail":"9 days left"}]"#
        let partly = ScriptedRunner()
        partly.on({ _ in true }, result: CommandResult(code: 0, out: "\(others)\n@@exit 4\n", err: ""))
        let some = try await plan(.sdCheck, machine: m1).run(.test(.test(machine: m1, runner: partly, mac: mac1193)))
        XCTAssertFalse(some.ok)
        XCTAssertTrue(some.message.contains("Check again"), some.message)

        let broken = ScriptedRunner()
        broken.on({ _ in true }, result: CommandResult(code: 0, out: "\n@@exit 2\nsd: worker setup needs --check, --create-labels or both\n", err: ""))
        let bad = try await plan(.sdCheck, machine: m1).run(.test(.test(machine: m1, runner: broken, mac: mac1193)))
        XCTAssertFalse(bad.ok)
        XCTAssertTrue(bad.message.contains("worker setup needs"), bad.message)
    }

    /// sd 1.19.3 is the first with `--create-labels`: an older sd on this Mac (which is the one copied to the machine)
    /// stops the Fix before any command.
    func testTheCreateLabelsFixNeedsSd1193OnThisMac() async throws {
        let m1 = TestMachines.mac()
        for facts in [mac, MacFacts.empty] {
            let runner = ScriptedRunner()
            let out = try await plan(.sdCheck, machine: m1).run(.test(.test(machine: m1, runner: runner, mac: facts)))
            XCTAssertFalse(out.ok)
            XCTAssertTrue(runner.calls.isEmpty, "nothing ran")
        }
        let old = try await plan(.sdCheck, machine: m1).run(.test(.test(machine: m1, runner: ScriptedRunner(), mac: mac)))
        XCTAssertEqual(old.message, "sd on this Mac is older than 1.19.3; update it first")
        XCTAssertTrue(SDWorkerSteps.isAtLeast("1.19.3", "1.19.3"))
        XCTAssertTrue(SDWorkerSteps.isAtLeast("1.20.0", "1.19.3"))
        XCTAssertTrue(SDWorkerSteps.isAtLeast("2.0", "1.19.3"))
        XCTAssertTrue(SDWorkerSteps.isAtLeast("1.19.10", "1.19.3"), "numbers, not text")
        XCTAssertFalse(SDWorkerSteps.isAtLeast("1.19.2", "1.19.3"))
        XCTAssertFalse(SDWorkerSteps.isAtLeast("1.9.9", "1.19.3"))
        XCTAssertFalse(SDWorkerSteps.isAtLeast("(unreadable version)", "1.19.3"))
    }

    func testSetupCheckCommandUsesAbsolutePathsAndNeverLive() throws {
        let vps = TestMachines.linux()
        let s = try script(.sdCheck, machine: vps)
        let sd = MachinePaths.sdPy(home: "/home/agent")
        XCTAssertTrue(s.contains("'/usr/bin/python3' -B '\(sd)' worker setup --check --json"), s)
        XCTAssertFalse(s.contains("--live"))
        if case .remote = try step(.sdCheck, vps).probe {} else { XCTFail("not remote") }
        XCTAssertEqual(try step(.sdCheck, vps).cadence, .slow)

        var young = vps
        young.jobs[0].sdWorker?.python = "python3"
        let refused = try script(.sdCheck, machine: young)
        XCTAssertFalse(refused.contains("worker setup"), "a relative python never runs")
    }

    // MARK: Running

    /// Runs the macOS toggle scripts with `/bin/sh` and a fake `launchctl` that logs its arguments.
    private func runToggle(on: Bool, loaded: Bool) throws -> [String] {
        let bin = temp.appendingPathComponent("bin-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: bin, withIntermediateDirectories: true)
        let log = bin.appendingPathComponent("log").path
        let fake = """
        #!/bin/sh
        echo "$*" >> '\(log)'
        if [ "$1" = print ]; then [ \(loaded ? "1" : "0") = 1 ]; exit $?; fi
        exit 0
        """
        let path = bin.appendingPathComponent("launchctl")
        try fake.write(to: path, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: path.path)
        let process = Process()
        process.executableURL = URL(fileURLWithPath: "/bin/sh")
        process.arguments = ["-c", SDWorkerSteps.toggleScript(linux: false, on: on)]
        process.environment = ["PATH": "\(bin.path):/usr/bin:/bin", "HOME": "/Users/x"]
        try process.run()
        process.waitUntilExit()
        XCTAssertEqual(process.terminationStatus, 0)
        let text = (try? String(contentsOfFile: log, encoding: .utf8)) ?? ""
        return text.split(separator: "\n").map(String.init)
    }

    func testMacToggleOnEnablesThenBootstrapsOnlyWhenNotLoaded() throws {
        let uid = String(getuid())
        let fresh = try runToggle(on: true, loaded: false)
        XCTAssertEqual(fresh, ["enable gui/\(uid)/com.sd.worker", "print gui/\(uid)/com.sd.worker",
                               "bootstrap gui/\(uid) /Users/x/Library/LaunchAgents/com.sd.worker.plist"])
        let already = try runToggle(on: true, loaded: true)
        XCTAssertEqual(already, ["enable gui/\(uid)/com.sd.worker", "print gui/\(uid)/com.sd.worker"], "already loaded is ok")
    }

    func testMacToggleOffBootsOutOnlyWhenLoadedThenDisables() throws {
        let uid = String(getuid())
        let loaded = try runToggle(on: false, loaded: true)
        XCTAssertEqual(loaded, ["print gui/\(uid)/com.sd.worker", "bootout gui/\(uid)/com.sd.worker", "disable gui/\(uid)/com.sd.worker"])
        let idle = try runToggle(on: false, loaded: false)
        XCTAssertEqual(idle, ["print gui/\(uid)/com.sd.worker", "disable gui/\(uid)/com.sd.worker"], "not loaded is ok")
    }

    func testRunningToggleScriptsAndState() async throws {
        XCTAssertEqual(SDWorkerSteps.toggleScript(linux: true, on: true), "systemctl enable --now sd-worker.service")
        XCTAssertEqual(SDWorkerSteps.toggleScript(linux: true, on: false), "systemctl disable --now sd-worker.service")
        let macOnScript = SDWorkerSteps.toggleScript(linux: false, on: true)
        XCTAssertTrue(macOnScript.hasPrefix(#"launchctl enable "gui/$(id -u)/com.sd.worker""#))
        XCTAssertTrue(macOnScript.hasSuffix(#"launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.sd.worker.plist""#))
        let macOffScript = SDWorkerSteps.toggleScript(linux: false, on: false)
        XCTAssertTrue(macOffScript.contains(#"launchctl bootout "gui/$(id -u)/com.sd.worker""#))
        XCTAssertTrue(macOffScript.hasSuffix(#"launchctl disable "gui/$(id -u)/com.sd.worker""#))
        for linux in [true, false] {
            for on in [true, false] {
                let s = SDWorkerSteps.toggleScript(linux: linux, on: on)
                XCTAssertFalse(s.contains("sd-testflight"))
                for service in MachineSafety.productionServices { XCTAssertFalse(s.contains(service)) }
            }
        }

        XCTAssertEqual(try script(.running), "systemctl is-enabled sd-worker.service; systemctl is-active sd-worker.service")
        XCTAssertEqual(try script(.running, machine: TestMachines.mac()),
                       #"launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1 && echo loaded || echo notloaded; launchctl print-disabled "gui/$(id -u)" 2>/dev/null | grep '"com.sd.worker" =>'"#)
        let on = try await check(.running, out: "enabled\nactive\n")
        XCTAssertEqual(on.state, .pass("on"))
        for off in ["disabled\ninactive\n", "enabled\ninactive\n", "disabled\nactive\n", "", "enabled\nfailed\n"] {
            let r = try await check(.running, out: off)
            XCTAssertEqual(r.state, .fail("off"), off)
        }
        // macOS: on is loaded and not disabled; a label print-disabled doesn't list counts as enabled
        let line = { (state: String) in "\t\"com.sd.worker\" => \(state)\n" }
        let macOn = try await check(.running, machine: TestMachines.mac(), out: "loaded\n")
        XCTAssertEqual(macOn.state, .pass("on"))
        let macOn2 = try await check(.running, machine: TestMachines.mac(), out: "loaded\n" + line("enabled"))
        XCTAssertEqual(macOn2.state, .pass("on"))
        for off in ["notloaded\n", "notloaded\n" + line("disabled"), "loaded\n" + line("disabled"), "loaded\n" + line("true"),
                    "notloaded\n" + line("enabled"), ""] {
            let r = try await check(.running, machine: TestMachines.mac(), out: off)
            XCTAssertEqual(r.state, .fail("off"), off)
        }

        // the toggle is a click's switch, not a Fix
        guard case let .toggle(admin) = try step(.running, TestMachines.linux()).fix else { return XCTFail() }
        XCTAssertTrue(admin)
        guard case let .toggle(macAdmin) = try step(.running, TestMachines.mac()).fix else { return XCTFail() }
        XCTAssertFalse(macAdmin)
    }
}

// MARK: helpers

private extension Result {
    var failure: Failure? {
        if case let .failure(error) = self { return error }
        return nil
    }
}

/// Sends `/usr/bin/tar` to a real `ProcessRunner` and records it, and everything else to a scripted runner that
/// answers 0. It remembers the archive tar produced, to compare with the Fix's ssh stdin.
private final class RoutingRunner: CommandRunning, @unchecked Sendable {
    private let lock = NSLock()
    private let scripted = ScriptedRunner()
    private var tar: [RecordedCall] = []
    private var archive: Data?
    private var path: String?

    var tarCalls: [RecordedCall] { lock.withLock { tar } }
    var sshCalls: [RecordedCall] { scripted.calls }
    var producedArchive: Data? { lock.withLock { archive } }
    var archivePath: String? { lock.withLock { path } }

    func run(_ argv: [String], cwd: String?, timeout: TimeInterval) async -> CommandResult {
        await run(argv, cwd: cwd, env: [:], stdin: nil, timeout: timeout, onOutput: nil)
    }

    func run(_ argv: [String], cwd: String?, env: [String: String], stdin: Data?, timeout: TimeInterval,
             onOutput: (@Sendable (String) -> Void)?) async -> CommandResult {
        guard argv.first == "/usr/bin/tar" else {
            return await scripted.run(argv, cwd: cwd, env: env, stdin: stdin, timeout: timeout, onOutput: onOutput)
        }
        let result = await ProcessRunner().run(argv, cwd: cwd, env: env, stdin: stdin, timeout: timeout, onOutput: onOutput)
        let file = argv.drop { $0 != "-czf" }.dropFirst().first
        lock.withLock {
            tar.append(RecordedCall(argv: argv, env: env, stdin: stdin, cwd: cwd))
            path = file
            archive = file.flatMap { FileManager.default.contents(atPath: $0) }
        }
        return result
    }
}

import XCTest
@testable import Notchling

/// The end-to-end safety properties of machine management, over the real step catalog of a Mac and a Linux
/// machine. Nothing starts ssh or `gh`: Fixes run against a runner that records and answers success.
final class MachinesSafetyTests: XCTestCase {
    private let machines = [TestMachines.mac(), TestMachines.linux()]

    private func catalog(_ machine: Machine) -> [SetupStep] { StepCatalog.steps(for: machine, all: [machine]) }

    /// Real Fixes of these steps open a browser or the vault form on this Mac; the tests never run them.
    private let opensThisMac: Set<StepID> = [.claudeLogin, .issuesToken, .tokenScope]

    private func fixContext(_ machine: Machine, runner: CommandRunning, password: String? = "pw-admin-test") -> FixContext {
        FixContext.test(StepContext.test(machine: machine, runner: runner), adminPassword: password)
    }

    /// The base64 blobs a script carries (the files an admin step installs), decoded and appended.
    private func withDecodedBlobs(_ text: String) -> String {
        var out = text
        let blob = try! NSRegularExpression(pattern: "[A-Za-z0-9+/]{40,}={0,2}")
        for m in blob.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
            guard let r = Range(m.range, in: text), let data = Data(base64Encoded: String(text[r])),
                  let decoded = String(data: data, encoding: .utf8) else { continue }
            out += "\n" + decoded
        }
        return out
    }

    /// Every text a step can put in front of a machine or the user: displays, probe scripts, toggle scripts, and
    /// the argv of every Fix run against a recording runner.
    private func allTexts(for machine: Machine, adminOnly: Bool = false) async -> [(String, String)] {
        var texts: [(String, String)] = []
        for step in catalog(machine) {
            let label = "\(machine.name)/\(step.id.raw)"
            if case .remote(let script, _) = step.probe, !adminOnly { texts.append((label + " probe", script)) }
            switch step.fix {
            case .howTo: break
            case .toggle(let needsAdmin):
                if adminOnly && !needsAdmin { break }
                for on in [true, false] {
                    texts.append((label + " toggle \(on)", SDWorkerSteps.toggleScript(linux: machine.os == .linux, on: on)))
                }
            case .run(let plan):
                if adminOnly && !plan.needsAdmin { break }
                texts.append((label + " display", plan.display))
                guard !opensThisMac.contains(step.id) else { break }
                let runner = ScriptedRunner()
                _ = await plan.run(fixContext(machine, runner: runner))
                for call in runner.calls { texts.append((label + " run", call.argv.joined(separator: " "))) }
            }
        }
        return texts
    }

    func testAdminScriptsNeverTouchProductionServices() async throws {
        var seenAdmin = 0, seenSystemctl = 0
        let allowedSystemctl: Set<String> = ["systemctl daemon-reload", "systemctl enable --now sd-worker.service",
                                             "systemctl disable --now sd-worker.service"]
        let systemctl = try NSRegularExpression(pattern: "systemctl[^\\n;'&|\"]*")
        let apt = try NSRegularExpression(pattern: "apt-get")
        // the verb, not words that contain it (`pmset autorestart`, `NEEDRESTART_SUSPEND`)
        let restart = try NSRegularExpression(pattern: "\\b(try-)?restart\\b|reload-or-restart", options: .caseInsensitive)
        for machine in machines {
            for (label, raw) in await allTexts(for: machine, adminOnly: true) {
                seenAdmin += 1
                let text = withDecodedBlobs(raw)
                let lower = text.lowercased()
                for name in MachineSafety.productionServices {
                    XCTAssertFalse(lower.contains(name), "\(label) names \(name)")
                }
                XCTAssertTrue(restart.matches(in: text, range: NSRange(text.startIndex..., in: text)).isEmpty, "\(label) restarts something")
                XCTAssertFalse(lower.replacingOccurrences(of: "daemon-reload", with: "").contains(" reload"), "\(label) reloads a service")
                XCTAssertFalse(lower.contains("docker"), "\(label) touches docker")
                for m in systemctl.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    let found = String(text[Range(m.range, in: text)!]).trimmingCharacters(in: .whitespaces)
                    seenSystemctl += 1
                    XCTAssertTrue(allowedSystemctl.contains(found), "\(label) runs `\(found)`")
                }
                if !apt.matches(in: text, range: NSRange(text.startIndex..., in: text)).isEmpty {
                    XCTAssertTrue(text.contains("NEEDRESTART_SUSPEND=1"), "\(label) runs apt-get without NEEDRESTART_SUSPEND=1")
                }
            }
        }
        XCTAssertGreaterThan(seenAdmin, 6, "the admin steps were looked at")
        XCTAssertGreaterThanOrEqual(seenSystemctl, 3, "the systemctl commands were looked at")
    }

    func testTheAdminStepsOfTheCatalogAreTheOnesTheSpecNames() async {
        var names: [String] = []
        for machine in machines {
            for step in catalog(machine) {
                switch step.fix {
                case .run(let plan) where plan.needsAdmin: names.append("\(machine.name)/\(step.id.raw)")
                case .toggle(true): names.append("\(machine.name)/\(step.id.raw)")
                default: break
                }
            }
        }
        XCTAssertEqual(names, ["m1/stays-awake", "vps-sing-1/base-tools", "vps-sing-1/service", "vps-sing-1/running"])
    }

    func testNoDisplayTextOrScriptContainsAVaultValue() async throws {
        let values = ["claude-worker": testLongToken,
                      "m1-github-issues": "github_pat_" + String(testLongToken.dropFirst(13)),
                      "vps-sing-1-github-issues": "ghp_" + String(testLongToken.reversed().prefix(60))]
        for machine in machines {
            // the catalog is built with the vault in reach of the engine; no step's text may carry a value
            for (label, text) in await allTexts(for: machine) {
                for (name, value) in values {
                    XCTAssertFalse(text.contains(value), "\(label) contains the value of \(name)")
                    assertNoFragment(of: value, in: text)
                }
            }
        }
    }

    @MainActor
    func testHostileRemoteValuesNeverReachAScript() async throws {
        var machine = TestMachines.linux()
        machine.abilities = nil
        machine.sdWorker?.sdPath = ""
        let runner = ReachableRunner(reachable: "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/a b;x\nversion=Ubuntu 24.04\ndisk_kb=1048576\ndocker=none")
        let reg = TestRegistry(machines: [machine])
        let engine = SetupEngine(runner: runner, secrets: FakeSecrets(), http: FakeHTTP(), mac: { .empty },
                                 machines: { reg.machines }, save: reg.save, clock: { Date(timeIntervalSince1970: 1_800_000_000) },
                                 observeVault: false)
        await engine.refresh([machine.name], includeSlow: true)
        XCTAssertEqual(engine.results[machine.name]?[.reachable]?.state,
                       .fail("the machine's home folder is missing or has characters Notchling won't use"))

        // everything built from here on: the next refresh's probes, every step's script and display
        let before = runner.calls.count
        await engine.refresh([machine.name], includeSlow: true)
        XCTAssertGreaterThan(runner.calls.count, before, "the second refresh built scripts")
        for call in runner.calls.dropFirst(before) {
            let text = call.argv.joined(separator: " ")
            XCTAssertFalse(text.contains(";x"), "a hostile home reached a command: \(text)")
            XCTAssertFalse(text.contains("a b"), "a hostile home reached a command")
        }
        let current = try XCTUnwrap(reg.machines.first)
        for step in engine.steps(for: current) {
            if case .remote(let script, _) = step.probe { XCTAssertFalse(script.contains(";x"), step.id.raw) }
            if case .run(let plan) = step.fix { XCTAssertFalse(plan.display.contains(";x"), step.id.raw) }
        }
        let saved = String(decoding: try JSONEncoder().encode(reg.machines), as: UTF8.self)
        XCTAssertFalse(saved.contains(";x"), "the registry holds the hostile home")
        XCTAssertNotEqual(current.abilities?.home, "/home/a b;x")

        guard case .cantTell? = engine.results[machine.name]?[.service]?.state else {
            return XCTFail("Service installed said \(String(describing: engine.results[machine.name]?[.service]?.state))")
        }
    }

    func testNothingPushesExceptTheReadOnlyDeployKeyAdd() async throws {
        let push = try NSRegularExpression(pattern: "git push[^\\n;'\"&|]*")
        let deployAdd = try NSRegularExpression(pattern: "gh repo deploy-key add[^\\n;'\"&|]*")
        let writers = ["gh issue", "gh pr ", "gh release", "gh secret", "gh gist", "gh repo create", "gh repo delete",
                       "gh repo edit", "gh repo fork", "deploy-key delete", "--method POST", "--method PUT", "--method PATCH",
                       "--method DELETE", "-X POST", "-X PUT", "-X PATCH", "-X DELETE", "--allow-write"]
        var deployAdds = 0
        for machine in machines {
            for (label, raw) in await allTexts(for: machine) {
                let text = withDecodedBlobs(raw)
                for m in push.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    let found = String(text[Range(m.range, in: text)!])
                    XCTAssertTrue(found.contains("--dry-run"), "\(label) pushes: \(found)")
                }
                for writer in writers { XCTAssertFalse(text.contains(writer), "\(label) uses \(writer)") }
                for m in deployAdd.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    deployAdds += 1
                    let found = String(text[Range(m.range, in: text)!])
                    XCTAssertTrue(found.contains("--title agents-"), "\(label): \(found)")
                }
            }
        }
        XCTAssertGreaterThan(deployAdds, 0, "the deploy-key add was looked at")
    }
}

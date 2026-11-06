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

    /// A Linux machine runs its admin steps as root over ssh (sudo as the worker is refused); a Mac as its own account.
    private func adminRunnable(_ machine: Machine) -> Machine {
        var m = machine
        if m.os == .linux { m.admin = .rootSSH }
        return m
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
                _ = await plan.run(fixContext(adminRunnable(machine), runner: runner))
                for call in runner.calls { texts.append((label + " run", call.argv.joined(separator: " "))) }
            }
            // Replace key is no Fix of Set up or Set up all, but its commands are held to the same scans
            if let replace = step.replace, !adminOnly {
                texts.append((label + " replace display", replace.display))
                let runner = replaceKeyRunner(machine.name)
                _ = await replace.run(fixContext(machine, runner: runner))
                for call in runner.calls { texts.append((label + " replace run", call.argv.joined(separator: " "))) }
            }
        }
        return texts
    }

    private let oldKeyBody = "AAAAC3NzaC1lZDI1NTE5AAAAIMachineKeyForTestsOnly0123456789abcdefg"
    private let newKeyBody = "AAAAC3NzaC1lZDI1NTE5AAAAINewKeyForReplaceTestsOnly0123456789abcdefgh"

    /// A machine and GitHub that let Replace key run to the end: the old key is id 11, the new one id 22.
    private func replaceKeyRunner(_ machineName: String) -> ScriptedRunner {
        let runner = ScriptedRunner()
        // the first listing is the one before the add; it does not hold the new key
        let old = "{\"id\":11,\"key\":\"ssh-ed25519 \(oldKeyBody)\",\"title\":\"agents-\(machineName)\",\"read_only\":true}"
        let new = "{\"id\":22,\"key\":\"ssh-ed25519 \(newKeyBody)\",\"title\":\"agents-\(machineName)-20270115\",\"read_only\":true}"
        let listings = Listings()
        runner.on({ $0.contains("--paginate") && listings.next() == 1 }, result: CommandResult(code: 0, out: "[\(old)]", err: ""))
        runner.on({ $0.contains("--paginate") }, result: CommandResult(code: 0, out: "[\(old),\(new)]", err: ""))
        runner.on({ $0.last?.contains("ssh-keygen") == true }, result: CommandResult(code: 0, out: "ssh-ed25519 \(newKeyBody) agents-new\n", err: ""))
        runner.on({ $0.last?.contains("head -c 1024") == true }, result: CommandResult(code: 0, out: "ssh-ed25519 \(oldKeyBody) agents-old\n", err: ""))
        runner.on({ $0.last?.contains("-T git@github.com") == true },
                  result: CommandResult(code: 0, out: "Hi o/n! You've successfully authenticated, but GitHub does not provide shell access.\n", err: ""))
        runner.on({ $0.last?.contains("push --dry-run") == true },
                  result: CommandResult(code: 0, out: "ERROR: The key you are authenticating with has been marked as read only.\n@@push 128\n", err: ""))
        return runner
    }

    func testAdminScriptsNeverTouchProductionServices() async throws {
        var seenAdmin = 0, seenSystemctl = 0, seenApt = 0
        let aptCommand = try NSRegularExpression(pattern: "apt-get [^\\n;'&|\"]*")
        let killer = try NSRegularExpression(pattern: "\\b(p|k|x)?kill(all)?\\b")
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
                // M5: apt only installs what is missing: never an upgrade of what is there, never a removal
                for m in aptCommand.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    let found = String(text[Range(m.range, in: text)!])
                    seenApt += 1
                    if found.contains(" install") {
                        XCTAssertTrue(found.contains("--no-upgrade"), "\(label) installs without --no-upgrade: \(found)")
                    } else {
                        XCTAssertTrue(found.contains(" update"), "\(label) runs an apt-get verb that is not install or update: \(found)")
                    }
                    for verb in ["upgrade", "dist-upgrade", "full-upgrade", "remove", "purge", "autoremove"] {
                        XCTAssertFalse(found.replacingOccurrences(of: "--no-upgrade", with: "").contains(verb), "\(label) runs apt-get \(verb): \(found)")
                    }
                }
                XCTAssertTrue(killer.matches(in: text, range: NSRange(text.startIndex..., in: text)).isEmpty, "\(label) kills a process")
            }
        }
        XCTAssertGreaterThanOrEqual(seenApt, 4, "the apt-get commands were looked at")
        XCTAssertGreaterThan(seenAdmin, 6, "the admin steps were looked at")
        XCTAssertGreaterThanOrEqual(seenSystemctl, 3, "the systemctl commands were looked at")
    }

    /// C1: every admin command of the catalog (the Fixes and the Running switch) goes out through the absolute
    /// `/usr/bin/sudo` (or as root over ssh, with no sudo), runs its script under `env -i` with a fixed PATH, and
    /// carries nothing of the worker account's own: no user prelude, `~/.local/bin`, cargo or mise.
    func testEveryAdminCommandUsesTheAbsoluteSudoAndAFixedEnvironment() async {
        var seen = 0
        for machine in machines {
            let m = adminRunnable(machine)
            var commands: [(String, [String])] = []
            for step in catalog(m) {
                switch step.fix {
                case .run(let plan) where plan.needsAdmin:
                    let runner = ScriptedRunner()
                    _ = await plan.run(fixContext(m, runner: runner))
                    commands += runner.calls.map { ("\(m.name)/\(step.id.raw)", $0.argv) }
                case .toggle(true):
                    for on in [true, false] {
                        let runner = ScriptedRunner()
                        _ = await MachineRun.admin(SDWorkerSteps.toggleScript(linux: m.os == .linux, on: on),
                                                   ctx: fixContext(m, runner: runner))
                        commands += runner.calls.map { ("\(m.name)/\(step.id.raw) \(on)", $0.argv) }
                    }
                default: break
                }
            }
            for (label, argv) in commands {
                seen += 1
                let text = argv.joined(separator: "\n")
                for needle in [RemoteShell.prelude, "export PATH", "$HOME", ".local/bin", ".cargo", "mise", "/opt/homebrew", "/usr/local"] {
                    XCTAssertFalse(text.contains(needle), "\(label) carries \(needle)")
                }
                XCTAssertTrue(text.contains("/usr/bin/env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin HOME="), "\(label) has no fixed environment")
                if m.admin == .rootSSH {
                    XCTAssertTrue(argv.contains("root@\(m.host)"), label)
                    XCTAssertFalse(text.contains("sudo"), "\(label) uses sudo as root")
                } else {
                    XCTAssertTrue(text.contains("/usr/bin/sudo -S -k"), "\(label) doesn't call the absolute sudo")
                    // every sudo is the absolute one
                    XCTAssertEqual(text.components(separatedBy: "sudo").count - 1, text.components(separatedBy: "/usr/bin/sudo").count - 1,
                                   "\(label) has a sudo found by PATH")
                }
            }
        }
        // Stays awake, Base tools and the Running switch both ways (Service installed needs this Mac's sd to run)
        XCTAssertGreaterThanOrEqual(seen, 4, "the admin commands were looked at")
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

    /// The vault of the test, with the values inside a closure: a walk over the engine must not find them in the
    /// fake itself (production's `VaultSecretSource` holds only lookups too).
    private struct ClosureSecrets: SecretSource {
        let lookup: @Sendable (String) -> String?
        init(_ lookup: @escaping @Sendable (String) -> String?) { self.lookup = lookup }
        func value(named name: String) -> String? { lookup(name) }
        func entry(named name: String) -> VaultEntry? { nil }
    }

    /// GitHub says 404 and nothing is kept (`FakeHTTP` records each bearer, which a walk over the engine would find).
    private struct NoHTTP: HTTPFetching {
        func get(_ url: URL, bearer: String) async -> HTTPAnswer { HTTPAnswer(status: 404, headers: [:], body: Data()) }
    }

    private final class Listings: @unchecked Sendable {
        private let lock = NSLock()
        private var count = 0
        func next() -> Int { lock.withLock { count += 1; return count } }
    }

    private final class CallLog: @unchecked Sendable {
        private let lock = NSLock()
        private var list: [RecordedCall] = []
        func add(_ call: RecordedCall) { lock.withLock { list.append(call) } }
        var calls: [RecordedCall] { lock.withLock { list } }
    }

    /// Real fake values go through the real delivery steps (Claude login, issues token, Update N machines) against a
    /// recording runner, and through the real model. A value may be in the stdin of the call that writes its file and
    /// nowhere else: not in argv, env, a log, an outcome, a result, a confirmation display or anything the view model
    /// publishes.
    @MainActor
    func testNoDisplayTextOrScriptContainsAVaultValue() async throws {
        let reachableText: [String: String] = [
            "m1": "os=Darwin\narch=arm64\nuid=501\nuser=sondoan\nhome=/Users/sondoan\nversion=26.1\ndisk_kb=125829120\ndocker=none",
            "vps-sing-1": "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=83886080\ndocker=none",
        ]
        for machine in machines {
            let issuesName = "\(machine.name)-github-issues"
            let values = ["claude-worker": testLongToken,
                          issuesName: "github_pat_" + String(testLongToken.dropFirst(13).reversed()),
                          "unrelated": "zZ9-unrelated-vault-value-\(machine.name)-0123456789"]
            // the value, and pieces of it a cut or a split could leave
            var needles: [String] = []
            for value in values.values {
                let chars = Array(value)
                needles += [value, String(chars.prefix(8)), String(chars[(chars.count / 2)..<(chars.count / 2 + 8)]), String(chars.suffix(8))]
            }
            let log = CallLog()
            let runner = ReachableRunner(reachable: reachableText[machine.name] ?? "", sink: { log.add($0) })
            let reg = TestRegistry(machines: [machine])
            let engine = SetupEngine(runner: runner, secrets: ClosureSecrets { values[$0] }, http: NoHTTP(), mac: { .empty },
                                     machines: { reg.machines }, save: reg.save, clock: { Date(timeIntervalSince1970: 1_800_000_000) },
                                     observeVault: false)
            let model = MachinesModel(poller: MachinePoller(runner: runner, machines: { reg.machines }, clock: { Date() }, observeSleep: false),
                                      actions: MorningActions(runner: runner), engine: engine, registry: { reg.state }, save: reg.save,
                                      vaultEntries: { [] }, rename: { _, _ in }, renew: { _ in },
                                      registryPath: "/tmp/notchling-test/machines.json", runner: runner)
            await model.open()

            // every confirmation the window can show, and the two delivery Fixes run for real
            var shown: [String] = []
            for step in engine.steps(for: machine) {
                model.requestSetUp(step.id, on: machine.name)
                guard case .fix(let prompt)? = model.setupSheet else { continue }
                shown.append(prompt.display)
                if step.id == .claudeLogin || step.id == .issuesToken {
                    await model.confirm(prompt, password: nil).value
                } else {
                    model.cancel(prompt)
                }
            }
            model.requestSetUpAll(on: machine.name)
            if case .fix(let all)? = model.setupSheet { shown.append(all.display); model.cancel(all) }
            // Off is always possible. On waits for every step before Running to be ✓ and then shows no sheet (and
            // leaves its own line in `outcomes`, which the check below reads).
            model.requestToggle(on: machine.name, to: false)
            if case .fix(let toggle)? = model.setupSheet { shown.append(toggle.display); model.cancel(toggle) }

            // Update N machines for both renewed values
            for name in ["claude-worker", issuesName] {
                engine.vaultValueChanged(name: name)
                await model.update(secret: name).value
            }
            XCTAssertEqual(model.outcomes[machine.name]?.ok, true, "the delivery Fixes ran: \(String(describing: model.outcomes[machine.name]))")

            // 1. what ran: a value only in the stdin of the call that writes its file
            let calls = log.calls
            for value in [values["claude-worker"]!, values[issuesName]!] {
                let carrying = calls.filter { call in call.stdin.map { String(decoding: $0, as: UTF8.self).contains(value) } ?? false }
                XCTAssertGreaterThanOrEqual(carrying.count, 2, "\(machine.name): the value was delivered by the Fix and by Update")
                for call in carrying { XCTAssertTrue(call.argv.last?.contains("cat > ") == true, "a value went to a call that writes no file") }
            }
            for call in calls {
                let text = call.argv.joined(separator: " ") + " " + call.env.map { "\($0.key)=\($0.value)" }.joined(separator: " ")
                for needle in needles { XCTAssertFalse(text.contains(needle), "\(machine.name): argv or env carries a value piece: \(text.prefix(200))") }
            }

            // 2. what the app says: displays, scripts, logs, outcomes, results, notes
            var said = shown
            said += engine.logs.values.flatMap { $0 }
            said += model.outcomes.values.map(\.text) + Array(model.updateNotes.values)
            for (_, byStep) in engine.results {
                for (_, r) in byStep {
                    switch r.state { case .pass(let t), .fail(let t), .cantTell(let t): said.append(t) }
                    said += r.rows.flatMap { [$0.name, $0.detail] }
                }
            }
            for step in engine.steps(for: machine) {
                if case .remote(let script, _) = step.probe { said.append(script) }
                if case .run(let plan) = step.fix { said.append(plan.display) }
                if let replace = step.replace { said.append(replace.display) }
            }
            XCTAssertFalse(shown.isEmpty)
            for text in said {
                for needle in needles { XCTAssertFalse(text.contains(needle), "\(machine.name): shown text carries a value piece: \(text.prefix(200))") }
            }

            // 3. what the view model and the engine hold
            for needle in needles {
                var seen = Set<ObjectIdentifier>()
                XCTAssertFalse(holds(needle, in: model, seen: &seen), "\(machine.name): the view model holds a value piece")
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
        let ghApi = try NSRegularExpression(pattern: "gh api[^\\n;'\"&|]*")
        let deployDelete = try NSRegularExpression(pattern: "\\S*gh repo deploy-key delete[^\\n;'\"&|]*")
        var deployAdds = 0, deployDeletes = 0
        for machine in machines {
            for (label, raw) in await allTexts(for: machine) {
                let text = withDecodedBlobs(raw)
                for m in push.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    let found = String(text[Range(m.range, in: text)!])
                    XCTAssertTrue(found.contains("--dry-run"), "\(label) pushes: \(found)")
                }
                // Replace key is the one place that deletes a deploy key, and only the old key by an integer id
                let replacing = label.contains("replace")
                for writer in writers where !(replacing && writer == "deploy-key delete") {
                    XCTAssertFalse(text.contains(writer), "\(label) uses \(writer)")
                }
                for m in deployDelete.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    let found = String(text[Range(m.range, in: text)!])
                    XCTAssertTrue(replacing, "\(label) deletes a deploy key: \(found)")
                    if label.hasSuffix("replace run") {
                        XCTAssertNotNil(found.range(of: #"^\S*gh repo deploy-key delete [0-9]+ -R [A-Za-z0-9._/-]+$"#, options: .regularExpression),
                                        "\(label): \(found)")
                        deployDeletes += 1
                    } else {
                        XCTAssertTrue(found.contains("<id of the old key>") || found.contains("<id of the new key>"), "\(label): \(found)")
                    }
                }
                // neither the write flag nor its short form, anywhere
                XCTAssertFalse(text.contains("allow-write"), "\(label) has the write flag")
                XCTAssertNil(text.range(of: #"deploy-key (add|delete)[^\n;'"&|]* -w( |$)"#, options: .regularExpression), "\(label) has -w")
                // M5: `gh api` only reads (the host-key `meta` call). A field or an input body turns it into a POST.
                for m in ghApi.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    let found = " " + String(text[Range(m.range, in: text)!]) + " "
                    for flag in [" -f ", " -F ", " --field", " --raw-field", " --input"] {
                        XCTAssertFalse(found.contains(flag), "\(label) sends a body with gh api: \(found)")
                    }
                }
                for m in deployAdd.matches(in: text, range: NSRange(text.startIndex..., in: text)) {
                    deployAdds += 1
                    let found = String(text[Range(m.range, in: text)!])
                    XCTAssertTrue(found.contains("--title agents-"), "\(label): \(found)")
                }
            }
        }
        XCTAssertGreaterThan(deployAdds, 0, "the deploy-key add was looked at")
        XCTAssertGreaterThan(deployDeletes, 0, "Replace key's delete of the old key was looked at")
    }

    /// Replace key runs from its own confirmed click and nowhere else: a refresh, a Check of a draft, a changed
    /// vault value, Set up all and Set up on the step never delete a deploy key or make a `.new` key.
    @MainActor
    func testNoPathRunsReplaceKeyWithoutItsOwnClick() async throws {
        let reachableText: [String: String] = [
            "m1": "os=Darwin\narch=arm64\nuid=501\nuser=sondoan\nhome=/Users/sondoan\nversion=26.1\ndisk_kb=125829120\ndocker=none",
            "vps-sing-1": "os=Linux\narch=x86_64\nuid=1000\nuser=agent\nhome=/home/agent\nversion=Ubuntu 24.04\ndisk_kb=83886080\ndocker=none",
        ]
        for machine in machines {
            let log = CallLog()
            let runner = ReachableRunner(reachable: reachableText[machine.name] ?? "", sink: { log.add($0) })
            let reg = TestRegistry(machines: [machine])
            let engine = SetupEngine(runner: runner, secrets: ClosureSecrets { _ in "zZ9-some-vault-value-0123456789" }, http: NoHTTP(), mac: { .empty },
                                     machines: { reg.machines }, save: reg.save, clock: { Date(timeIntervalSince1970: 1_800_000_000) },
                                     observeVault: false)
            await engine.refresh([machine.name], includeSlow: true)
            await engine.refresh(nil, includeSlow: true)
            _ = await engine.check(draft: machine)
            for name in ["claude-worker", "\(machine.name)-github-issues"] { engine.vaultValueChanged(name: name) }
            let plan = engine.setUpAllPlan(for: machine.name)
            _ = await engine.setUpAll(on: machine.name, approved: plan, askAdmin: { _ in .cancelled })
            _ = await engine.runFix(.deployKey(TestMachines.repo), on: machine.name, approval: FixApproval(adminPassword: nil, asRootConfirmed: false))
            XCTAssertFalse(log.calls.isEmpty, "the checks did run")
            // the Deploy key step was really reached by every path above: the engine holds a result for it (it waits for
            // Base tools here, so its probe and Fix could not run), Set up all listed it, and Replace key is not offered
            XCTAssertNotNil(engine.results[machine.name]?[.deployKey(TestMachines.repo)], "\(machine.name): the Deploy key step was never evaluated")
            XCTAssertTrue(plan.contains { $0.id == .deployKey(TestMachines.repo) }, "\(machine.name): Set up all did not list the Deploy key step")
            XCTAssertFalse(RepoSteps.offersReplace(engine.results[machine.name]?[.deployKey(TestMachines.repo)]))
            for call in log.calls {
                let text = call.argv.joined(separator: " ")
                XCTAssertFalse(text.contains("deploy-key delete"), "\(machine.name): a deploy key was deleted without a click: \(text)")
                XCTAssertFalse(text.contains("agents-dtthaison-lingeva-mono.new"), "\(machine.name): a new key was made without a click")
            }
        }
    }
}

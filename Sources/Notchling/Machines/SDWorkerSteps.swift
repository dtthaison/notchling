import CryptoKit
import Foundation

/// A plugin as this Mac has it installed.
struct InstalledPlugin: Equatable, Sendable {
    let version: String
    let path: String
}

/// What this Mac has installed, read once per refresh: the plugin versions to put on a machine.
struct MacFacts: Sendable {
    /// By plugin id, e.g. `sd@system-design`.
    var plugins: [String: InstalledPlugin]
    /// The marketplace manifests by marketplace name.
    var marketplaceJSON: [String: String]

    private static let readCap = 4 * 1024 * 1024

    /// Reads `~/.claude/plugins/installed_plugins.json` (version 2, user scope) and the `system-design` directory
    /// marketplace in `known_marketplaces.json`, with that marketplace's `.claude-plugin/marketplace.json`.
    /// Missing or unreadable files give empty facts.
    static func load(home: URL) -> MacFacts {
        let plugins = home.appendingPathComponent(".claude/plugins")
        var facts = MacFacts(plugins: [:], marketplaceJSON: [:])
        if let root = object(plugins.appendingPathComponent("installed_plugins.json")),
           let all = root["plugins"] as? [String: Any] {
            for (id, value) in all {
                let entries = (value as? [[String: Any]]) ?? []
                guard let user = entries.first(where: { $0["scope"] as? String == "user" }),
                      let version = user["version"] as? String, let path = user["installPath"] as? String else { continue }
                facts.plugins[id] = InstalledPlugin(version: version, path: path)
            }
        }
        if let known = object(plugins.appendingPathComponent("known_marketplaces.json")),
           let market = known["system-design"] as? [String: Any],
           let source = market["source"] as? [String: Any], source["source"] as? String == "directory",
           let dir = source["path"] as? String ?? market["installLocation"] as? String,
           let data = read(URL(fileURLWithPath: dir).appendingPathComponent(".claude-plugin/marketplace.json")),
           let text = String(data: data, encoding: .utf8) {
            facts.marketplaceJSON["system-design"] = text
        }
        return facts
    }

    private static func read(_ url: URL) -> Data? {
        guard let handle = try? FileHandle(forReadingFrom: url) else { return nil }
        defer { try? handle.close() }
        return try? handle.read(upToCount: readCap)
    }

    private static func object(_ url: URL) -> [String: Any]? {
        read(url).flatMap { try? JSONSerialization.jsonObject(with: $0) as? [String: Any] }
    }
}

/// Why a step can't build what it writes. A string literal makes one, so `.failure("no kinds chosen")` reads as text
/// (`Result` needs an `Error`, and `String` isn't one).
struct StepFailure: Error, Equatable, CustomStringConvertible, ExpressibleByStringInterpolation {
    let message: String
    var description: String { message }
    init(stringLiteral value: String) { message = value }
}

// MARK: worker.json

/// The part of the machine's `worker.json` that Notchling owns. Every other key stays as the machine has it.
enum WorkerJSON {
    static let tokenFile = MachinePaths.githubToken
    static let claudeEnv = MachinePaths.claudeEnv
    /// The most a probe reads.
    static let readCap = 262_144

    /// What sd accepts for `machine`: `^[a-z0-9][a-z0-9-]*$`.
    private static func isValidMachineName(_ name: String) -> Bool {
        guard let first = name.first, first.isASCII, first.isLowercase || first.isNumber else { return false }
        return name.allSatisfy { $0.isASCII && ($0.isLowercase || $0.isNumber || $0 == "-") }
    }

    /// The keys Notchling writes for `machine`: its name, the token and Claude login files, the expiry and health
    /// URL, and each served repo with the machine that runs each kind (from every machine whose job serves the repo).
    static func owned(machine: Machine, job: SDWorkerSettings, all: [Machine]) -> Result<[String: Any], StepFailure> {
        guard isValidMachineName(machine.name) else {
            return .failure("the machine name \(machine.name) must be lowercase letters, digits and hyphens")
        }
        guard !job.kinds.isEmpty else { return .failure("no kinds chosen for \(machine.name)") }
        if let bad = job.kinds.first(where: { !["ios", "backend"].contains($0) }) {
            return .failure("kind \(bad) must be ios or backend")
        }
        guard !job.repos.isEmpty else { return .failure("no repos chosen for \(machine.name)") }
        if let repo = job.repos.first(where: { $0.checkCommand.trimmingCharacters(in: .whitespaces).isEmpty }) {
            return .failure("no check command for \(repo.github)")
        }
        guard job.python.hasPrefix("/"), job.sdPath.hasPrefix("/") else {
            return .failure("python or sd.py is not an absolute path yet")
        }
        let others = all.filter { $0.name != machine.name } + [machine]
        var repos: [[String: Any]] = []
        for repo in job.repos {
            var claimed: [String: [String]] = [:]
            for other in others {
                // this machine is `machine` as passed in, not the saved copy in `all`
                let settings = other.name == machine.name ? job : other.sdWorker
                guard let settings, settings.repos.contains(where: { $0.github == repo.github }) else { continue }
                for kind in settings.kinds where !(claimed[kind]?.contains(other.name) ?? false) {
                    claimed[kind, default: []].append(other.name)
                }
            }
            var kinds: [String: String] = [:]
            for (kind, names) in claimed.sorted(by: { $0.key < $1.key }) {
                if names.count > 1 {
                    let sorted = names.sorted()
                    return .failure("kind \(kind) of \(repo.github) is claimed by \(sorted[0]) and \(sorted[1])")
                }
                kinds[kind] = names[0]
            }
            let name = repo.github.split(separator: "/").last.map(String.init) ?? repo.github
            let path = repo.machinePath ?? "~/src/\(name)"
            var entry: [String: Any] = ["github": repo.github, "path": path, "kinds": kinds, "check_cmd": repo.checkCommand]
            if let skill = repo.verifySkill, !skill.isEmpty { entry["verify_skill"] = "\(path)/\(skill)" }
            repos.append(entry)
        }
        return .success([
            "machine": machine.name,
            "token_file": tokenFile,
            "claude_env": claudeEnv,
            "token_expires": job.tokenExpires.map { $0 as Any } ?? NSNull(),
            "health_url": job.healthURL.map { $0 as Any } ?? NSNull(),
            "repos": repos,
        ])
    }

    /// The remote file with the owned keys laid over it: sorted, pretty, a final newline. A repo the machine already
    /// lists keeps the keys Notchling doesn't own (`writer_model`); a repo that is no longer served goes.
    static func merged(remote: Data?, owned: [String: Any]) -> Data {
        var object = parse(remote) ?? [:]
        let remoteRepos = (object["repos"] as? [[String: Any]]) ?? []
        for (key, value) in owned where key != "repos" { object[key] = value }
        let ownedRepos = (owned["repos"] as? [[String: Any]]) ?? []
        object["repos"] = ownedRepos.map { repo -> [String: Any] in
            var entry = remoteRepos.first { $0["github"] as? String == repo["github"] as? String } ?? [:]
            for (key, value) in repo { entry[key] = value }
            return entry
        }
        let data = (try? JSONSerialization.data(withJSONObject: object,
                                                options: [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes])) ?? Data("{}".utf8)
        return data + Data("\n".utf8)
    }

    /// The owned keys whose value on the machine differs, sorted. A key that is missing equals null. A repo is
    /// compared on the keys Notchling owns only.
    static func differences(remote: Data?, owned: [String: Any]) -> [String] {
        let object = parse(remote) ?? [:]
        return owned.keys.sorted().filter { key in
            if key == "repos" { return !reposMatch(object["repos"], owned["repos"]) }
            return !same(object[key] ?? NSNull(), owned[key] ?? NSNull())
        }
    }

    /// The object in `data`, or nil for nothing or anything that is not a JSON object.
    static func parse(_ data: Data?) -> [String: Any]? {
        guard let data, !data.isEmpty else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    }

    private static func same(_ a: Any, _ b: Any) -> Bool { NSArray(array: [a]).isEqual(to: [b]) }

    private static func reposMatch(_ remote: Any?, _ owned: Any?) -> Bool {
        guard let remote = remote as? [[String: Any]], let owned = owned as? [[String: Any]],
              remote.count == owned.count else { return false }
        return owned.allSatisfy { repo in
            guard let there = remote.first(where: { $0["github"] as? String == repo["github"] as? String }) else { return false }
            return repo.allSatisfy { key, value in there[key].map { same($0, value) } ?? false }
        }
    }
}

// MARK: service files

struct ServiceFile: Equatable {
    let name: String
    /// Absolute for a systemd unit; `~/Library/LaunchAgents/…` for the plist, which the worker user writes.
    let remotePath: String
    let content: String
}

/// The sd worker's units (Linux) and LaunchAgent (macOS), rendered from sd's own templates.
enum ServiceFiles {
    static let linuxNames = ["sd-worker.slice", "sd-worker.service", "sd-testflight.service", "sd-testflight.timer"]
    static let plistName = "com.sd.worker.plist"
    static let unitDirectory = "/etc/systemd/system"
    static let plistPath = "~/Library/LaunchAgents/com.sd.worker.plist"

    /// The five templates in `dir` by file name, or nil when one is missing or unreadable.
    static func templates(at dir: URL) -> [String: String]? {
        var found: [String: String] = [:]
        for name in linuxNames + [plistName] {
            guard let text = try? String(contentsOf: dir.appendingPathComponent(name), encoding: .utf8) else { return nil }
            found[name] = text
        }
        return found
    }

    static func render(templates: [String: String], linux: Bool, user: String, home: String,
                       python: String, sdPath: String) -> Result<[ServiceFile], StepFailure> {
        if user == "root" { return .failure("the worker must not run as root") }
        guard isSafeUser(user) else { return .failure("the worker user name isn't a plain account name") }
        guard RemoteShell.isSafeMachinePath(home) else { return .failure("the home folder isn't a safe path") }
        guard RemoteShell.isSafeMachinePath(python), RemoteShell.isSafeMachinePath(sdPath) else {
            return .failure("python or sd.py is not an absolute path yet")
        }
        let names = linux ? linuxNames : [plistName]
        var files: [ServiceFile] = []
        for name in names {
            guard let template = templates[name] else { return .failure("sd on this Mac has no \(name) template") }
            let hasEnvironmentFile = template.components(separatedBy: "\n")
                .contains { $0.trimmingCharacters(in: .whitespaces).hasPrefix("EnvironmentFile=") }
            if hasEnvironmentFile { return .failure("sd on this Mac is older than 1.19.2; update it first") }
            var text = template
                .replacingOccurrences(of: "@SD_PY@", with: sdPath)
                .replacingOccurrences(of: "@PYTHON3@", with: python)
                .replacingOccurrences(of: "@HOME@", with: home)
            if linux {
                text = setLines(text, user: user, path: RemoteShell.servicePath(home: home))
                if name.hasSuffix(".service"), !text.components(separatedBy: "\n").contains("User=\(user)") {
                    return .failure("\(name) has no User= line to set")
                }
            } else {
                guard let plist = plist(text, path: RemoteShell.servicePath(home: home)) else {
                    return .failure("\(name) is not a plist this Notchling can fill in")
                }
                text = plist
            }
            if let left = text.range(of: "@[A-Z0-9_]+@", options: .regularExpression) {
                return .failure("\(name) still has the placeholder \(text[left])")
            }
            files.append(ServiceFile(name: name, remotePath: linux ? "\(unitDirectory)/\(name)" : plistPath, content: text))
        }
        return .success(files)
    }

    private static func isSafeUser(_ user: String) -> Bool {
        guard (1...32).contains(user.utf8.count), let first = user.first, first == "_" || (first >= "a" && first <= "z") else {
            return false
        }
        return user.allSatisfy { $0 == "_" || $0 == "-" || ($0 >= "a" && $0 <= "z") || ($0 >= "0" && $0 <= "9") }
    }

    /// `User=agent` becomes `User=<user>` and the `Environment=PATH=` line takes the service PATH. Whole lines only.
    private static func setLines(_ text: String, user: String, path: String) -> String {
        text.components(separatedBy: "\n").map { line in
            if line == "User=agent" { return "User=\(user)" }
            if line.hasPrefix("Environment=PATH=") { return "Environment=PATH=\(path)" }
            return line
        }.joined(separator: "\n")
    }

    /// The plist with its `PATH` string set and its XML comments dropped: the comment holds the placeholders'
    /// documentation, and a `--` in a path would make it invalid XML. Nil when it is not a plist with a PATH.
    private static func plist(_ text: String, path: String) -> String? {
        var text = text.replacingOccurrences(of: "(?s)<!--(?:(?!-->).)*-->\\s*", with: "", options: .regularExpression)
        guard let key = text.range(of: "<key>PATH</key>"),
              let open = text.range(of: "<string>", range: key.upperBound..<text.endIndex),
              let close = text.range(of: "</string>", range: open.upperBound..<text.endIndex) else { return nil }
        text.replaceSubrange(open.upperBound..<close.lowerBound, with: path)
        guard (try? PropertyListSerialization.propertyList(from: Data(text.utf8), format: nil)) != nil else { return nil }
        return text
    }
}

// MARK: steps

/// Steps 12 and 15–18 of the sd worker job: plugins, worker.json, service, setup check, running.
enum SDWorkerSteps {
    static let sdPlugin = "sd@system-design"
    static let superpowersPlugin = "superpowers@claude-plugins-official"
    static let unreadableVersion = "(unreadable version)"
    static let pluginsProbe = """
    head -c 1048576 "$HOME/.claude/plugins/installed_plugins.json" 2>/dev/null
    echo; echo @@sd
    d="$HOME/.local/share/agents/system-design/plugin/scripts/sd.py"
    [ -f "$d" ] && printf 'sd=%s\\n' "$d"
    """

    static func all(for machine: Machine, job: SDWorkerSettings, all: [Machine]) -> [SetupStep] {
        let linux = machine.os == .linux
        let clones = job.repos.map { StepID.clone($0.github) }
        return [
            SetupStep(id: .plugins, title: "Plugins installed", needs: [.claudeCode], cadence: .everyRefresh,
                      probe: .remote(script: pluginsProbe, parse: { out, ctx in pluginsCheck(out, ctx) }),
                      fix: .run(FixPlan(display: pluginsDisplay, needsAdmin: false, run: installPlugins))),
            SetupStep(id: .workerJSON, title: "worker.json written",
                      needs: [.plugins, .issuesToken, .claudeLogin] + clones, cadence: .everyRefresh,
                      probe: .remote(script: workerJSONProbe, parse: { out, ctx in workerJSONCheck(out, ctx) }),
                      fix: .run(FixPlan(display: workerJSONDisplay(machine: machine, job: job, all: all), needsAdmin: false,
                                        run: writeWorkerJSON))),
            SetupStep(id: .service, title: "Service installed", needs: [.workerJSON, .baseTools], cadence: .everyRefresh,
                      probe: .remote(script: serviceProbe(linux: linux), parse: { out, ctx in serviceCheck(out, ctx) }),
                      fix: .run(FixPlan(display: serviceDisplay(linux: linux), needsAdmin: linux, run: installService))),
            SetupStep(id: .sdCheck, title: "sd worker setup --check", needs: [.service], cadence: .slow,
                      probe: .remote(script: setupCheckScript(job), parse: { out, _ in setupCheckResult(out) }),
                      fix: .howTo("Read the rows under this step. Fix what they name, then check again.")),
            SetupStep(id: .running, title: "Running", needs: [.sdCheck], cadence: .everyRefresh,
                      probe: .remote(script: runningProbe(linux: linux), parse: { out, _ in runningCheck(out, linux: linux) }),
                      fix: .toggle(needsAdmin: linux)),
        ]
    }

    // MARK: Plugins installed

    private static func isSafeVersion(_ v: String) -> Bool {
        (1...32).contains(v.utf8.count)
            && v.allSatisfy { ($0 >= "0" && $0 <= "9") || ($0 >= "A" && $0 <= "Z") || ($0 >= "a" && $0 <= "z")
                || $0 == "." || $0 == "+" || $0 == "-" }
    }

    /// The user-scope version of each plugin on the machine. A version that isn't plain is `(unreadable version)`.
    private static func machineVersions(_ json: String) -> [String: String] {
        guard let data = json.trimmingCharacters(in: .whitespacesAndNewlines).data(using: .utf8),
              let root = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any],
              let plugins = root["plugins"] as? [String: Any] else { return [:] }
        var found: [String: String] = [:]
        for id in [sdPlugin, superpowersPlugin] {
            guard let entry = (plugins[id] as? [[String: Any]])?.first(where: { $0["scope"] as? String == "user" }) else { continue }
            let version = entry["version"] as? String ?? ""
            found[id] = isSafeVersion(version) ? version : unreadableVersion
        }
        return found
    }

    private static func pluginsCheck(_ out: RemoteOutput, _ ctx: StepContext) -> CheckResult {
        let lines = out.text.components(separatedBy: "\n")
        let marker = lines.firstIndex(of: "@@sd")
        let json = lines[..<(marker ?? lines.count)].joined(separator: "\n")
        var detected = Detected()
        if let marker, let line = lines[(marker + 1)...].first(where: { $0.hasPrefix("sd=") }) {
            let path = String(line.dropFirst(3))
            if RemoteShell.isSafeMachinePath(path) { detected.sdPath = path }
        }
        guard let sd = ctx.mac.plugins[sdPlugin], let superpowers = ctx.mac.plugins[superpowersPlugin] else {
            return CheckResult(state: .cantTell("this Mac doesn't have both superpowers and sd installed"), detected: detected)
        }
        let here = machineVersions(json)
        var wrong: [String] = []
        for (name, id, mac) in [("superpowers", superpowersPlugin, superpowers), ("sd", sdPlugin, sd)] {
            guard let version = here[id] else { wrong.append("\(name) missing, \(mac.version) on this Mac"); continue }
            if version != mac.version { wrong.append("\(name) \(version) here, \(mac.version) on this Mac") }
        }
        if !wrong.isEmpty { return CheckResult(state: .fail(wrong.joined(separator: "; ")), detected: detected) }
        guard detected.sdPath != nil else {
            return CheckResult(state: .fail("sd.py is not at ~/.local/share/agents/system-design/plugin"), detected: detected)
        }
        return CheckResult(state: .pass("superpowers \(superpowers.version), sd \(sd.version)"), detected: detected)
    }

    static func pluginsScript(marketplace: String) -> String {
        """
        set -e; d="$HOME/.local/share/agents/system-design"
        rm -rf "$d.new"; mkdir -p "$d.new/plugin" "$d.new/.claude-plugin"
        tar -xzf - -C "$d.new/plugin"
        printf '%s' \(LaneStore.quote(marketplace)) > "$d.new/.claude-plugin/marketplace.json"
        rm -rf "$d.old"; [ ! -e "$d" ] || mv "$d" "$d.old"
        mv "$d.new" "$d" || { [ ! -e "$d.old" ] || mv "$d.old" "$d"; exit 1; }
        rm -rf "$d.old"
        claude plugin marketplace add "$d" >/dev/null 2>&1 || claude plugin marketplace update system-design
        claude plugin install sd@system-design || claude plugin update sd@system-design
        claude plugin marketplace add anthropics/claude-plugins-official >/dev/null 2>&1 || claude plugin marketplace update claude-plugins-official
        claude plugin install superpowers@claude-plugins-official || claude plugin update superpowers@claude-plugins-official
        """
    }

    private static let pluginsDisplay = """
    On this Mac: tar -czf <temp>/sd-plugin.tgz -C <this Mac's sd folder> .
    On the machine, with that archive on stdin:
    \(pluginsScript(marketplace: "<this Mac's marketplace.json>"))
    """

    private static func installPlugins(_ ctx: FixContext) async -> FixOutcome {
        guard let sd = ctx.step.mac.plugins[sdPlugin], let market = ctx.step.mac.marketplaceJSON["system-design"] else {
            return FixOutcome(ok: false, message: "this Mac has no sd plugin or system-design marketplace to copy")
        }
        let temp = FileManager.default.temporaryDirectory.appendingPathComponent("sd-plugin-\(UUID().uuidString)")
        do { try FileManager.default.createDirectory(at: temp, withIntermediateDirectories: true) } catch {
            return FixOutcome(ok: false, message: "couldn't make a temporary folder")
        }
        defer { try? FileManager.default.removeItem(at: temp) }
        let archive = temp.appendingPathComponent("sd-plugin.tgz").path
        // COPYFILE_DISABLE keeps macOS tar from adding `._*` metadata entries that GNU tar would unpack as files.
        // `MachineRun.local` takes no environment, so this one local command goes to the runner directly.
        let tar = await ctx.step.runner.run(["/usr/bin/tar", "-czf", archive, "-C", sd.path, "."], cwd: nil,
                                            env: ["COPYFILE_DISABLE": "1"], stdin: nil, timeout: 120, onOutput: ctx.log)
        guard tar.code == 0, let data = FileManager.default.contents(atPath: archive), !data.isEmpty else {
            return FixOutcome(ok: false, message: "couldn't pack sd \(sd.version) on this Mac: \(firstLine(tar.err))")
        }
        let run = await MachineRun.script(pluginsScript(marketplace: market), stdin: data, ctx: ctx)
        return run.code == 0
            ? FixOutcome(ok: true, message: "sd \(sd.version) and superpowers installed")
            : FixOutcome(ok: false, message: "the install failed (exit \(run.code)): \(firstLine(run.err))")
    }

    // MARK: worker.json written

    private static let workerJSONProbe = #"head -c \#(WorkerJSON.readCap) "$HOME/.config/sd/worker.json" 2>/dev/null"#

    /// The Fix's read keeps stderr, so a real failure has a reason; a missing file prints nothing.
    private static let workerJSONRead = #"[ -f "$HOME/.config/sd/worker.json" ] || exit 0; head -c \#(WorkerJSON.readCap) "$HOME/.config/sd/worker.json""#

    private static func workerJSONCheck(_ out: RemoteOutput, _ ctx: StepContext) -> CheckResult {
        let machine = ctx.machine
        guard let job = machine.sdWorker else { return CheckResult(state: .cantTell("no sd worker job")) }
        let owned: [String: Any]
        switch WorkerJSON.owned(machine: machine, job: job, all: ctx.all) {
        case let .failure(why): return CheckResult(state: .fail(why.message))
        case let .success(value): owned = value
        }
        if out.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty { return CheckResult(state: .fail("missing")) }
        let data = Data(out.text.utf8)
        guard WorkerJSON.parse(data) != nil else { return CheckResult(state: .fail("not valid JSON")) }
        let differ = WorkerJSON.differences(remote: data, owned: owned)
        return CheckResult(state: differ.isEmpty ? .pass("current") : .fail("differs: " + differ.joined(separator: ", ")))
    }

    private static func workerJSONDisplay(machine: Machine, job: SDWorkerSettings, all: [Machine]) -> String {
        let head = "Read \(MachinePaths.workerJSON) (head -c \(WorkerJSON.readCap)), keep the keys Notchling doesn't own, then write:\n"
            + RemoteShell.writeFile(MachinePaths.workerJSON)
        switch WorkerJSON.owned(machine: machine, job: job, all: all) {
        case let .failure(why): return head + "\n(can't be written yet: \(why))"
        case let .success(owned):
            let data = (try? JSONSerialization.data(withJSONObject: owned, options: [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes])) ?? Data()
            return head + "\nwith these keys on stdin:\n" + String(decoding: data, as: UTF8.self)
        }
    }

    private static func writeWorkerJSON(_ ctx: FixContext) async -> FixOutcome {
        let machine = ctx.step.machine
        guard let job = machine.sdWorker else { return FixOutcome(ok: false, message: "this machine has no sd worker job") }
        let owned: [String: Any]
        switch WorkerJSON.owned(machine: machine, job: job, all: ctx.step.all) {
        case let .failure(why): return FixOutcome(ok: false, message: why.message)
        case let .success(value): owned = value
        }
        let read = await MachineRun.script(workerJSONRead, ctx: ctx, timeout: 60)
        // A fresh machine has no worker.json: `head` exits 1 with nothing printed, and that is "missing", not an error.
        let missing = read.out.isEmpty && (read.code == 0 || read.code == 1)
        guard read.code == 0 || missing else {
            let why = firstLine(read.err)
            return FixOutcome(ok: false, message: "couldn't read worker.json (exit \(read.code))" + (why.isEmpty ? "" : ": \(why)"))
        }
        let remote = Data(read.out.utf8)
        if !read.out.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty, WorkerJSON.parse(remote) == nil {
            return FixOutcome(ok: false, message: "worker.json on the machine isn't a JSON object; fix or remove it first")
        }
        let write = await MachineRun.script(RemoteShell.writeFile(MachinePaths.workerJSON),
                                            stdin: WorkerJSON.merged(remote: remote, owned: owned), ctx: ctx, timeout: 60)
        return write.code == 0
            ? FixOutcome(ok: true, message: "worker.json written")
            : FixOutcome(ok: false, message: "couldn't write worker.json: \(firstLine(write.err))")
    }

    // MARK: Service installed

    private static let macTarget = #""gui/$(id -u)/com.sd.worker""#
    /// After the plist is written: the label stays disabled (launchd keeps that across logins) until Running turns on.
    static let macDisableScript = "launchctl disable \(macTarget)"

    private static func serviceTarget(_ name: String, linux: Bool) -> String {
        linux ? "\(ServiceFiles.unitDirectory)/\(name)" : ServiceFiles.plistPath
    }

    private static func serviceProbe(linux: Bool) -> String {
        let digest = linux ? "sha256sum" : "shasum -a 256"
        return (linux ? ServiceFiles.linuxNames : [ServiceFiles.plistName]).map { name in
            let path = RemoteShell.path(serviceTarget(name, linux: linux))
            return "if [ -f \(path) ]; then \(digest) \(path); else echo \(LaneStore.quote("missing " + name)); fi"
        }.joined(separator: "; ")
    }

    private static func hex(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
    }

    /// The files this Mac's sd templates give for `ctx.machine`.
    private static func renderedService(_ ctx: StepContext) -> Result<[ServiceFile], StepFailure> {
        let machine = ctx.machine
        guard let job = machine.sdWorker else { return .failure("no sd worker job") }
        guard let sd = ctx.mac.plugins[sdPlugin] else { return .failure("this Mac has no sd plugin to take the templates from") }
        guard let templates = ServiceFiles.templates(at: URL(fileURLWithPath: sd.path).appendingPathComponent("templates/worker")) else {
            return .failure("sd on this Mac has no worker templates")
        }
        guard let home = machine.abilities?.home else { return .failure("the machine's home folder isn't known yet") }
        return ServiceFiles.render(templates: templates, linux: machine.os == .linux, user: machine.user, home: home,
                                   python: job.python, sdPath: job.sdPath)
    }

    private static func serviceCheck(_ out: RemoteOutput, _ ctx: StepContext) -> CheckResult {
        let files: [ServiceFile]
        switch renderedService(ctx) {
        case let .failure(why): return CheckResult(state: .fail(why.message))
        case let .success(value): files = value
        }
        let lines = out.text.components(separatedBy: "\n")
        var missing: [String] = [], differ: [String] = []
        for (index, file) in files.enumerated() {
            let line = index < lines.count ? lines[index] : ""
            if line.hasPrefix("missing") { missing.append(file.name); continue }
            let word = line.split(separator: " ").first.map(String.init) ?? ""
            if word != hex(file.content) { differ.append(file.name) }
        }
        if differ.isEmpty && missing.isEmpty {
            return CheckResult(state: .pass(files.count == 1 ? "\(files[0].name) matches" : "\(files.count) units match"))
        }
        var parts: [String] = []
        if !differ.isEmpty { parts.append("differs: " + differ.joined(separator: ", ")) }
        if !missing.isEmpty { parts.append("missing: " + missing.joined(separator: ", ")) }
        return CheckResult(state: .fail(parts.joined(separator: "; ")))
    }

    /// The admin script that writes the units: each file's bytes as base64, then `daemon-reload`. It enables and
    /// starts nothing, and it reads no stdin.
    static func installScript(files: [ServiceFile]) -> String {
        var lines = ["set -e", "umask 022"]
        for file in files {
            // a temp file, then a move, on its own line so `set -e` stops on a failed decode
            lines.append("printf '%s' '\(Data(file.content.utf8).base64EncodedString())' | base64 -d > \(file.remotePath).new")
            lines.append("mv -f \(file.remotePath).new \(file.remotePath)")
        }
        lines.append("systemctl daemon-reload")
        return lines.joined(separator: "\n")
    }

    private static func serviceDisplay(linux: Bool) -> String {
        guard linux else {
            return "Write the LaunchAgent (mode 644) with the plist on stdin:\n"
                + RemoteShell.writeFile(ServiceFiles.plistPath, mode: "644")
                + "\nThen keep it from starting at login until you turn Running on:\n" + macDisableScript
        }
        let lines = ServiceFiles.linuxNames.flatMap { name -> [String] in
            let path = "\(ServiceFiles.unitDirectory)/\(name)"
            return ["printf '%s' '<base64 of \(name)>' | base64 -d > \(path).new", "mv -f \(path).new \(path)"]
        }
        return (["set -e", "umask 022"] + lines + ["systemctl daemon-reload", "(nothing is enabled or started)"]).joined(separator: "\n")
    }

    private static func installService(_ ctx: FixContext) async -> FixOutcome {
        let files: [ServiceFile]
        switch renderedService(ctx.step) {
        case let .failure(why): return FixOutcome(ok: false, message: why.message)
        case let .success(value): files = value
        }
        if ctx.step.machine.os == .linux {
            let run = await MachineRun.admin(installScript(files: files), ctx: ctx)
            return run.code == 0
                ? FixOutcome(ok: true, message: "\(files.count) units written")
                : FixOutcome(ok: false, message: "couldn't write the units (exit \(run.code)): \(firstLine(run.err))")
        }
        let run = await MachineRun.script(RemoteShell.writeFile(ServiceFiles.plistPath, mode: "644"),
                                          stdin: Data(files[0].content.utf8), ctx: ctx, timeout: 60)
        guard run.code == 0 else { return FixOutcome(ok: false, message: "couldn't write the LaunchAgent: \(firstLine(run.err))") }
        // The plist has RunAtLoad, so launchd would start it at the next login. Disabled, it waits for the Running click.
        // This never loads it: no bootstrap, no enable.
        let off = await MachineRun.script(macDisableScript, ctx: ctx, timeout: 60)
        return off.code == 0
            ? FixOutcome(ok: true, message: "\(ServiceFiles.plistName) written and disabled until Running is turned on")
            : FixOutcome(ok: false, message: "wrote \(ServiceFiles.plistName) but couldn't disable it (exit \(off.code)): \(firstLine(off.err))")
    }

    // MARK: sd worker setup --check

    private static let rowCap = 200, textCap = 300, notCounted = "com.sd.worker"

    /// Stdout, then `@@exit N`, then stderr, so the parse tells the two apart whatever the engine does with `code`.
    private static func setupCheckScript(_ job: SDWorkerSettings) -> String {
        guard RemoteShell.isSafeMachinePath(job.python), RemoteShell.isSafeMachinePath(job.sdPath) else {
            return "echo; echo '@@exit 2'; echo 'python or sd.py is not an absolute path yet'"
        }
        return "e=$(mktemp); \(LaneStore.quote(job.python)) \(LaneStore.quote(job.sdPath)) worker setup --check --json 2>\"$e\"; "
            + "c=$?; echo; echo \"@@exit $c\"; head -c 4096 \"$e\"; rm -f \"$e\""
    }

    private static func cut(_ s: String) -> String { String(s.prefix(textCap)) }

    private static func firstLine(_ s: String) -> String {
        cut(s.split(separator: "\n", omittingEmptySubsequences: true).first.map(String.init) ?? "")
    }

    private static func setupCheckResult(_ out: RemoteOutput) -> CheckResult {
        let text = out.text
        guard let marker = text.range(of: "@@exit ", options: .backwards) else {
            return CheckResult(state: .fail("setup --check printed no result"))
        }
        let tail = text[marker.upperBound...]
        let code = String(tail.prefix { $0.isNumber })
        let stderr = String(tail.drop { $0 != "\n" }.dropFirst())
        let stdout = String(text[..<marker.lowerBound])
        guard code == "0" || code == "4" else {
            let why = firstLine(stderr)
            return CheckResult(state: .fail(why.isEmpty ? "setup --check exited \(code)" : why))
        }
        guard let data = stdout.trimmingCharacters(in: .whitespacesAndNewlines).data(using: .utf8),
              let array = (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]] else {
            return CheckResult(state: .fail("setup --check printed rows this Notchling can't read"))
        }
        let rows = array.prefix(rowCap).map { row in
            SubRow(name: cut(row["name"] as? String ?? ""), ok: row["ok"] as? Bool ?? false, detail: cut(row["detail"] as? String ?? ""))
        }
        let counted = rows.filter { $0.name != notCounted }
        let failed = counted.filter { !$0.ok }.count
        if failed > 0 { return CheckResult(state: .fail("\(failed) of \(counted.count) checks failed"), rows: rows) }
        if code == "4" && counted.count == rows.count {
            return CheckResult(state: .fail("setup --check failed but named no row"), rows: rows)
        }
        return CheckResult(state: .pass("all \(counted.count) checks passed"), rows: rows)
    }

    // MARK: Running

    private static func runningProbe(linux: Bool) -> String {
        linux
            ? "systemctl is-enabled sd-worker.service; systemctl is-active sd-worker.service"
            : #"launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1 && echo loaded || echo notloaded; launchctl print-disabled "gui/$(id -u)" 2>/dev/null | grep '"com.sd.worker" =>'"#
    }

    private static func runningCheck(_ out: RemoteOutput, linux: Bool) -> CheckResult {
        let lines = out.text.split(separator: "\n").map { $0.trimmingCharacters(in: .whitespaces) }
        if linux { return CheckResult(state: lines == ["enabled", "active"] ? .pass("on") : .fail("off")) }
        // macOS: loaded, and not disabled. A label `print-disabled` doesn't list is enabled.
        let disabled = lines.contains { $0.contains(#""com.sd.worker" =>"#) && ($0.hasSuffix("disabled") || $0.hasSuffix("true")) }
        return CheckResult(state: lines.first == "loaded" && !disabled ? .pass("on") : .fail("off"))
    }

    /// What the Running switch runs: enable and start, or disable and stop, the worker's own service only.
    static func toggleScript(linux: Bool, on: Bool) -> String {
        switch (linux, on) {
        case (true, true): return "systemctl enable --now sd-worker.service"
        case (true, false): return "systemctl disable --now sd-worker.service"
        case (false, true):
            // already loaded is fine: bootstrap only when the label isn't loaded
            return #"launchctl enable "gui/$(id -u)/com.sd.worker" || exit 1; "#
                + #"launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1 || "#
                + #"launchctl bootstrap "gui/$(id -u)" "$HOME/Library/LaunchAgents/com.sd.worker.plist""#
        case (false, false):
            // not loaded is fine: bootout only when it is loaded
            return #"if launchctl print "gui/$(id -u)/com.sd.worker" >/dev/null 2>&1; then "#
                + #"launchctl bootout "gui/$(id -u)/com.sd.worker" || exit 1; fi; "#
                + #"launchctl disable "gui/$(id -u)/com.sd.worker""#
        }
    }
}

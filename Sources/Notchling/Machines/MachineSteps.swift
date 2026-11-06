import Foundation

/// Steps 1–6 and 8 of the catalog: the machine itself. Claude login (7) comes from `SecretSteps`.
///
/// Every probe is a read-only `sh` script whose output is keyed by name (`os=Darwin`, `@@runtimes`), never by line
/// position, so a probe that printed nothing for one key shifts nothing else. Everything a machine prints is
/// untrusted: a value is validated before it is kept (`isSafeMachinePath`), cut to 200 characters, and otherwise
/// only shown. No machine text goes into a script.
enum MachineSteps {
    /// Reachable, Worker user, Stays awake, FileVault, Base tools, Claude Code, Xcode. The catalog leaves out the
    /// macOS-only ones for a Linux machine.
    static func all(for machine: Machine) -> [SetupStep] {
        return [
            SetupStep(id: .reachable, title: "Reachable", needs: [], cadence: .everyRefresh,
                      probe: .remote(script: reachableScript, parse: parseReachable),
                      fix: .howTo("Turn the machine on and check it from this Mac with `tailscale ping \(machine.host)`. For a new Linux user, add the user to the Tailscale ACL's ssh users. Then check again.")),
            SetupStep(id: .workerUser, title: "Worker user", needs: [.reachable], cadence: .everyRefresh,
                      probe: .remote(script: workerUserScript, parse: parseWorkerUser),
                      fix: .howTo("Use the worker account (agent on vps-sing-1). Root is only for steps marked needs admin. On Linux the worker account must not be able to use sudo: remove it from the sudo group (admin steps run as root over SSH). Then check again.")),
            SetupStep(id: .staysAwake, title: "Stays awake", needs: [.reachable], cadence: .everyRefresh,
                      probe: .remote(script: "pmset -g", parse: parseStaysAwake),
                      fix: .run(staysAwakeFix)),
            SetupStep(id: .filevault, title: "FileVault", needs: [.reachable], cadence: .everyRefresh,
                      probe: .remote(script: "fdesetup status", parse: parseFileVault),
                      fix: .howTo("Notchling only reads this. Turn FileVault on or off in System Settings on the machine. With it on, the machine waits for an unlock after a restart. Then check again.")),
            SetupStep(id: .baseTools, title: "Base tools", needs: [.reachable], cadence: .everyRefresh,
                      probe: .remote(script: baseToolsScript, parse: parseBaseTools),
                      fix: .run(baseToolsFix(os: machine.os))),
            SetupStep(id: .claudeCode, title: "Claude Code", needs: [.reachable], cadence: .everyRefresh,
                      probe: .remote(script: claudeScript, parse: parseClaudeCode),
                      fix: .run(claudeCodeFix)),
            SetupStep(id: .xcode, title: "Xcode", needs: [.reachable], cadence: .everyRefresh,
                      probe: .remote(script: xcodeScript, parse: parseXcode),
                      fix: .howTo("Notchling doesn't install Xcode, which needs your Apple ID. On the machine: App Store, open it once, Xcode > Settings > Components for the iOS runtime, then Check again.")),
        ]
    }

    // MARK: Parsing helpers

    /// Machine text is cut to this many characters.
    private static let cutLength = 200

    private static func cut(_ s: String) -> String { String(s.prefix(cutLength)) }

    /// A piece of machine text for a message or a saved fact: masked first (`mask` is the context's), then cut, so
    /// a cut can't leave a piece of a vault value.
    private static func shown(_ s: String, _ mask: (String) -> String) -> String { cut(mask(s)) }

    /// The first `key=value` line for each wanted key. Lines are matched by their key, so a missing line, a banner
    /// before the output or a repeated key moves nothing.
    private static func keyed(_ text: String, keys: Set<String>) -> [String: String] {
        var found: [String: String] = [:]
        for raw in text.split(whereSeparator: \.isNewline) {
            guard let eq = raw.firstIndex(of: "=") else { continue }
            let key = String(raw[raw.startIndex..<eq])
            guard keys.contains(key), found[key] == nil else { continue }
            found[key] = String(raw[raw.index(after: eq)...]).trimmingCharacters(in: .whitespaces)
        }
        return found
    }

    private static func firstLine(_ text: String) -> String {
        for raw in text.split(whereSeparator: \.isNewline) {
            let line = raw.trimmingCharacters(in: .whitespaces)
            if !line.isEmpty { return line }
        }
        return ""
    }

    /// A short failure line from a Fix's result: the last line of stderr (where brew, apt and curl say what went
    /// wrong), or of stdout when stderr is empty. The vault values and the admin password are masked first
    /// (`ctx.mask`, which the engine builds with both), then the line is cut, so no piece of a secret survives a
    /// cut. Output `ProcessRunner` cut at
    /// its start is a notice and a half line that can hold the end of a value: that half line is not shown.
    private static func outcome(_ r: CommandResult, ok: String, ctx: FixContext) -> FixOutcome {
        if r.code == 0 { return FixOutcome(ok: true, message: ok) }
        func lastLine(_ output: String) -> String? {
            var lines = ctx.mask(output).split(whereSeparator: \.isNewline).map { $0.trimmingCharacters(in: .whitespaces) }
                .filter { !$0.isEmpty }
            if output.hasPrefix(ProcessRunner.cutNotice), lines.count > 1 { lines.remove(at: 1) }
            return lines.last
        }
        let last = lastLine(r.err) ?? lastLine(r.out) ?? "no output"
        return FixOutcome(ok: false, message: "failed (code \(r.code)): \(cut(last))")
    }

    // MARK: 1 Reachable

    private static let reachableScript = #"""
    echo "os=$(uname -s)"
    echo "arch=$(uname -m)"
    echo "uid=$(id -u)"
    echo "user=$(id -un)"
    echo "home=$HOME"
    if [ "$(uname -s)" = Darwin ]; then v=$(sw_vers -productVersion 2>/dev/null); else v=$(sed -n 's/^PRETTY_NAME=//p' /etc/os-release 2>/dev/null | head -1 | tr -d '"'); fi
    echo "version=$v"
    echo "disk_kb=$(df -Pk "$HOME" 2>/dev/null | awk 'NR==2 {print $4}')"
    if command -v docker >/dev/null 2>&1; then if o=$(docker info --format '{{json .SecurityOptions}}' 2>/dev/null); then case "$o" in *rootless*) echo docker=rootless;; *) echo docker=rootful;; esac; else echo docker=stopped; fi; else echo docker=none; fi
    """#

    private static func parseReachable(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        // masked before anything is cut from it
        let text = ctx.mask(out.text)
        let kv = keyed(text, keys: ["os", "arch", "home", "version", "disk_kb", "docker"])
        guard let osName = kv["os"], !osName.isEmpty else {
            if out.code != 0 {
                let why = firstLine(text)
                return CheckResult(state: .fail(why.isEmpty ? "can't reach the machine" : "can't reach the machine: \(cut(why))"))
            }
            return CheckResult(state: .cantTell("the machine gave no answer"))
        }
        let os: MachineOS
        switch osName {
        case "Darwin": os = .macOS
        case "Linux": os = .linux
        default: return CheckResult(state: .fail("unsupported OS \(cut(osName))"))
        }
        var detected = Detected()
        detected.os = os
        guard let home = kv["home"], RemoteShell.isSafeMachinePath(home) else {
            return CheckResult(state: .fail("the machine's home folder is missing or has characters Notchling won't use"),
                               detected: detected)
        }
        detected.home = home

        var abilities = ctx.machine.abilities ?? MachineAbilities()
        abilities.home = home
        let arch = kv["arch"].flatMap { $0.isEmpty ? nil : cut($0) }
        let version = kv["version"].flatMap { $0.isEmpty ? nil : cut($0) }
        if let arch { abilities.arch = arch }
        if let version { abilities.osVersion = version }
        if let docker = kv["docker"].flatMap({ DockerState(rawValue: $0) }) { abilities.docker = docker }
        if let kb = kv["disk_kb"].flatMap({ Int($0) }), kb >= 0 { abilities.freeDiskGB = kb / 1_048_576 }
        detected.abilities = abilities

        let label = os == .macOS ? (version.map { "macOS \($0)" } ?? "macOS") : (version ?? "Linux")
        return CheckResult(state: .pass(arch.map { "\(label), \($0)" } ?? label), detected: detected)
    }

    // MARK: 2 Worker user

    /// The user id, then (keyed) the OS, the groups, and on Linux whether `sudo -n -l` works. Read-only: `sudo -n`
    /// never asks for a password.
    private static let workerUserScript = """
    id -u
    echo "os=$(uname -s)"
    echo "groups=$(id -nG 2>/dev/null)"
    if [ "$(uname -s)" = Linux ]; then sudo -n -l >/dev/null 2>&1; echo "sudo_l=$?"; fi
    """

    /// The groups that let a Linux account use sudo.
    private static let sudoGroups: Set<String> = ["sudo", "wheel", "admin"]

    private static func parseWorkerUser(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        let uid = out.text.split(whereSeparator: \.isNewline)
            .map { $0.trimmingCharacters(in: .whitespaces) }
            .first { !$0.isEmpty && $0.allSatisfy(\.isASCII) && $0.allSatisfy(\.isNumber) }
        guard let uid, let number = Int(uid) else { return CheckResult(state: .cantTell("the machine did not say who this is")) }
        if number == 0 {
            return CheckResult(state: .fail("signed in as root; use the worker account. Root is only for steps marked needs admin"))
        }
        // On Linux the worker account must not be able to use sudo: the admin password would pass through shell
        // files it owns, and it could sudo as itself. On a Mac the user's own account is the admin.
        let kv = keyed(out.text, keys: ["os", "groups", "sudo_l"])
        if kv["os"] == "Linux" {
            let groups = Set((kv["groups"] ?? "").split(whereSeparator: { $0 == " " || $0 == "\t" }).map(String.init))
            if !groups.isDisjoint(with: sudoGroups) || kv["sudo_l"] == "0" {
                return CheckResult(state: .fail("the worker account can use sudo; remove it from the sudo group"))
            }
        }
        return CheckResult(state: .pass("user id \(number), not root"))
    }

    // MARK: 3 Stays awake

    private static func parseStaysAwake(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        let wanted: [(key: String, value: Int)] = [("sleep", 0), ("disksleep", 0), ("autorestart", 1)]
        var seen: [String: Int] = [:]
        for raw in out.text.split(whereSeparator: \.isNewline) {
            let words = raw.split(whereSeparator: { $0 == " " || $0 == "\t" })
            guard words.count >= 2, wanted.contains(where: { $0.key == String(words[0]) }),
                  seen[String(words[0])] == nil, let value = Int(words[1]) else { continue }
            seen[String(words[0])] = value
        }
        let missing = wanted.filter { seen[$0.key] == nil }.map(\.key)
        if !missing.isEmpty { return CheckResult(state: .cantTell("pmset did not report \(missing.joined(separator: ", "))")) }
        let wrong = wanted.filter { seen[$0.key] != $0.value }
        if wrong.isEmpty { return CheckResult(state: .pass(wanted.map { "\($0.key) \($0.value)" }.joined(separator: ", "))) }
        return CheckResult(state: .fail(wrong.map { "\($0.key) \(seen[$0.key]!) (should be \($0.value))" }.joined(separator: ", ")))
    }

    private static let staysAwakeScript = "pmset -a sleep 0 disksleep 0 && pmset -a autorestart 1"

    private static var staysAwakeFix: FixPlan {
        FixPlan(display: "as admin: \(staysAwakeScript)", needsAdmin: true) { ctx in
            let r = await MachineRun.admin(staysAwakeScript, ctx: ctx)
            return outcome(r, ok: "sleep 0, disksleep 0, autorestart 1 set", ctx: ctx)
        }
    }

    // MARK: 4 FileVault

    private static func parseFileVault(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        if out.text.contains("FileVault is On") {
            var detected = Detected()
            detected.fileVault = true
            return CheckResult(state: .pass("on: after a restart it waits for an unlock"), detected: detected)
        }
        if out.text.contains("FileVault is Off") {
            var detected = Detected()
            detected.fileVault = false
            return CheckResult(state: .pass("off"), detected: detected)
        }
        let why = firstLine(ctx.mask(out.text))
        return CheckResult(state: .cantTell(why.isEmpty ? "fdesetup said nothing" : cut(why)))
    }

    // MARK: 5 Base tools

    private static let baseToolNames = ["git", "python3", "jq", "gh"]

    private static let baseToolsScript = #"""
    echo "git=$(command -v git)"
    echo "python3=$(command -v python3)"
    echo "jq=$(command -v jq)"
    echo "gh=$(command -v gh)"
    if [ "$(uname -s)" = Darwin ] && ! xcode-select -p >/dev/null 2>&1; then echo clt=missing; pv=; else pv=$(python3 -c 'import sys; print("%d.%d" % sys.version_info[:2])' 2>/dev/null); fi
    echo "pyver=$pv"
    if command -v brew >/dev/null 2>&1; then echo brew=yes; else echo brew=no; fi
    """#

    /// `3.12` as (3, 12), or nil.
    private static func majorMinor(_ s: String) -> (Int, Int)? {
        let parts = s.split(separator: ".")
        guard parts.count == 2, let major = Int(parts[0]), let minor = Int(parts[1]) else { return nil }
        return (major, minor)
    }

    private static func baseToolRows(_ kv: [String: String], _ mask: (String) -> String) -> [SubRow] {
        baseToolNames.map { name -> SubRow in
            let path = kv[name] ?? ""
            return SubRow(name: name, ok: !path.isEmpty, detail: path.isEmpty ? "missing" : shown(path, mask))
        }
    }

    private static func parseBaseTools(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        let kv = keyed(out.text, keys: Set(baseToolNames + ["pyver", "brew", "clt"]))
        let found = baseToolNames.filter { !(kv[$0] ?? "").isEmpty }
        // Without the Command Line Tools /usr/bin/git and /usr/bin/python3 are stubs that open an install dialog on
        // the machine when run, so the probe never runs them there.
        if kv["clt"] == "missing" {
            return CheckResult(state: .fail("Command Line Tools missing"), rows: baseToolRows(kv, ctx.mask))
        }
        var problems: [String] = []
        let missing = baseToolNames.filter { !found.contains($0) }
        if !missing.isEmpty { problems.append("missing: \(missing.joined(separator: ", "))") }

        let version = kv["pyver"].flatMap { majorMinor($0) }
        var pythonOK = false
        if found.contains("python3") {
            if let version {
                if version >= (3, 10) { pythonOK = true }
                else { problems.append("python3 is \(version.0).\(version.1), sd needs 3.10 or newer") }
            } else {
                problems.append("python3 does not run")
            }
        }

        var detected = Detected()
        if pythonOK, let path = kv["python3"], RemoteShell.isSafeMachinePath(path) { detected.python = path }

        let rows = baseToolRows(kv, ctx.mask)
        if !problems.isEmpty { return CheckResult(state: .fail(problems.joined(separator: "; ")), rows: rows, detected: detected) }
        let python = version.map { "python3 \($0.0).\($0.1)" } ?? "python3"
        return CheckResult(state: .pass(["git", python, "jq", "gh"].joined(separator: ", ")), rows: rows, detected: detected)
    }

    private static let macBaseToolsScript = #"xcode-select -p >/dev/null 2>&1 || { echo 'Command Line Tools are missing: run xcode-select --install in the machine'"'"'s own Terminal, then Check again.'; exit 1; }; command -v brew >/dev/null 2>&1 || { echo 'Homebrew is missing: install it from https://brew.sh in the machine'"'"'s own Terminal, then Check again.'; exit 1; }; brew install git python jq gh"#

    /// Runs as root through sudo, so it names no path of the worker's home.
    private static let linuxBaseToolsScript = #"""
    set -e
    export DEBIAN_FRONTEND=noninteractive NEEDRESTART_SUSPEND=1 NEEDRESTART_MODE=l
    apt-get update
    apt-get install -y git python3 jq wget --no-upgrade
    mkdir -p -m 755 /etc/apt/keyrings
    wget -nv -O /etc/apt/keyrings/githubcli-archive-keyring.gpg https://cli.github.com/packages/githubcli-archive-keyring.gpg
    chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" > /etc/apt/sources.list.d/github-cli.list
    apt-get update
    apt-get install -y gh --no-upgrade
    """#

    /// The Fix is chosen by the OS the step was built with and checked again when it runs. With the OS not detected
    /// yet it refuses, rather than guess Homebrew.
    private static func baseToolsFix(os: MachineOS?) -> FixPlan {
        switch os {
        case .linux:
            return FixPlan(display: "as admin:\n\(linuxBaseToolsScript)", needsAdmin: true) { ctx in
                guard ctx.step.machine.os == .linux else { return FixOutcome(ok: false, message: notSettled(ctx.step.machine.os)) }
                let r = await MachineRun.admin(linuxBaseToolsScript, ctx: ctx)
                return outcome(r, ok: "git, python3, jq, wget and gh installed", ctx: ctx)
            }
        case .macOS:
            return FixPlan(display: macBaseToolsScript, needsAdmin: false) { ctx in
                guard ctx.step.machine.os == .macOS else { return FixOutcome(ok: false, message: notSettled(ctx.step.machine.os)) }
                let r = await MachineRun.script(macBaseToolsScript, ctx: ctx)
                return outcome(r, ok: "git, python, jq and gh installed with Homebrew", ctx: ctx)
            }
        case nil:
            return FixPlan(display: "OS not detected yet; run Reachable first", needsAdmin: false) { _ in
                FixOutcome(ok: false, message: notDetected)
            }
        }
    }

    private static let notDetected = "OS not detected yet; run Reachable first"

    private static func notSettled(_ now: MachineOS?) -> String {
        now == nil ? notDetected : "this machine's OS changed since this step was built; check again first"
    }

    // MARK: 6 Claude Code

    private static let claudeScript = "command -v claude >/dev/null 2>&1 && claude --version 2>/dev/null"

    private static func parseClaudeCode(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        let line = firstLine(ctx.mask(out.text))
        guard !line.isEmpty, line.contains(where: \.isNumber) else { return CheckResult(state: .fail("not installed")) }
        return CheckResult(state: .pass(cut(line)))
    }

    /// The download is held in a variable first: in `curl … | bash` a failed download leaves `bash` an empty script,
    /// which exits 0, and the Fix would report success.
    static let claudeInstall = #"s=$(curl -fsSL https://claude.ai/install.sh) && printf '%s\n' "$s" | bash"#

    private static var claudeCodeFix: FixPlan {
        FixPlan(display: claudeInstall, needsAdmin: false) { ctx in
            let r = await MachineRun.script(claudeInstall, ctx: ctx)
            return outcome(r, ok: "Claude Code installed", ctx: ctx)
        }
    }

    // MARK: 8 Xcode

    /// Without the Command Line Tools `xcodebuild` and `xcrun` are stubs that open an install dialog, so nothing runs
    /// then. `simctl list devices iPhone` keeps only the iPhones in the JSON (the full list can pass the 64 KiB cap
    /// with several runtimes); each device's `isAvailable` is still read from it.
    private static let xcodeScript = """
    if ! xcode-select -p >/dev/null 2>&1; then echo @@noclt; exit 0; fi
    xcodebuild -version | head -1
    echo @@runtimes
    xcrun simctl list runtimes available 2>/dev/null | grep '^iOS '
    echo @@devices
    xcrun simctl list devices iPhone -j 2>/dev/null
    """

    /// `ProcessRunner` keeps the last `ProcessRunner.outputCap` (2 MiB) of a command's output and says so with this
    /// first line. The 64 KiB check below guards the other cut: the `head -c 65536` the engine's batch wrapper
    /// puts on each probe's output on the machine.
    private static let outputCap = 65_536
    private static let cutNotice = "[earlier output cut]"

    /// Output that was cut, or sections that can't be read, say nothing about the machine: "can't tell", and the
    /// abilities already saved stay as they are.
    private static func parseXcode(_ out: RemoteOutput, _ ctx: StepContext) async -> CheckResult {
        let unreadable = CheckResult(state: .cantTell("the Xcode check's output was too long or not readable"))
        if out.text.hasPrefix(cutNotice) || out.text.utf8.count >= outputCap { return unreadable }
        if out.text.split(whereSeparator: \.isNewline).contains("@@noclt") {
            return CheckResult(state: .fail("Xcode is not installed: the Command Line Tools are missing"))
        }

        var xcodeLine: String?
        var runtimeLines: [String] = []
        var devicesJSON = ""
        var section = 0                                              // 0 head, 1 runtimes, 2 devices
        var sawRuntimes = false, sawDevices = false
        for raw in out.text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline) {
            let line = String(raw)
            if line == "@@runtimes" { section = 1; sawRuntimes = true; continue }
            if line == "@@devices" { section = 2; sawDevices = true; continue }
            switch section {
            case 0: if xcodeLine == nil, line.hasPrefix("Xcode ") { xcodeLine = shown(line.trimmingCharacters(in: .whitespaces), ctx.mask) }
            case 1: runtimeLines.append(line)
            default: devicesJSON += line + "\n"
            }
        }
        if !sawRuntimes || !sawDevices {
            // no marker at all: the probe did not run; markers missing next to an Xcode line: part of it is gone
            return unreadable
        }

        // iOS runtimes from `simctl list runtimes available | grep '^iOS '`: `iOS 26.5 (26.5 - 23F77) - com.apple…`
        var runtimes: [(name: String, version: String)] = []
        for line in runtimeLines where line.hasPrefix("iOS ") {
            guard let open = line.range(of: " ("),
                  let version = line[open.upperBound...].split(whereSeparator: { $0 == " " || $0 == ")" }).first,
                  majorMinorOrMore(String(version)) else { return unreadable }
            runtimes.append((shown(String(line[..<open.lowerBound]), ctx.mask), String(version)))
        }
        runtimes.sort { $0.version.compare($1.version, options: .numeric) == .orderedDescending }

        // iPhone names under iOS runtimes, the newest runtime first, without repeats
        var phones: [String] = []
        if !devicesJSON.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            guard let root = json(devicesJSON) as? [String: Any], let byRuntime = root["devices"] as? [String: Any] else { return unreadable }
            let keys = byRuntime.keys.filter { $0.contains("iOS") }
                .sorted { $0.compare($1, options: .numeric) == .orderedDescending }
            for key in keys {
                for item in (byRuntime[key] as? [[String: Any]]) ?? [] {
                    guard let name = item["name"] as? String, name.hasPrefix("iPhone"),
                          (item["isAvailable"] as? Bool) != false else { continue }
                    let phone = shown(name, ctx.mask)
                    if !phones.contains(phone) { phones.append(phone) }
                }
            }
        }

        var abilities = ctx.machine.abilities ?? MachineAbilities()
        abilities.xcode = xcodeLine
        abilities.iosRuntimes = runtimes.map(\.name)
        abilities.simulators = phones
        var detected = Detected()
        detected.abilities = abilities

        guard let xcodeLine else { return CheckResult(state: .fail("Xcode is not installed"), detected: detected) }
        guard let runtime = runtimes.first else { return CheckResult(state: .fail("no iOS runtime"), detected: detected) }
        guard let phone = phones.first else { return CheckResult(state: .fail("no iPhone simulator"), detected: detected) }
        let more = phones.count > 1 ? " and \(phones.count - 1) more" : ""
        return CheckResult(state: .pass("\(xcodeLine) · \(runtime.name) · \(phone)\(more)"), detected: detected)
    }

    /// `26.5` or `26.5.1`: digits and dots only.
    private static func majorMinorOrMore(_ s: String) -> Bool {
        let parts = s.split(separator: ".", omittingEmptySubsequences: false)
        return parts.count >= 2 && parts.allSatisfy { !$0.isEmpty && $0.allSatisfy { $0.isASCII && $0.isNumber } }
    }

    private static func json(_ text: String) -> Any? {
        try? JSONSerialization.jsonObject(with: Data(text.utf8))
    }
}

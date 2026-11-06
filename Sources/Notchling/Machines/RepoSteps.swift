import Foundation

/// Steps 9–11 for one repo of the sd worker job: deploy key, clone, push refused.
///
/// The machine gets a read-only deploy key: an ed25519 pair made on the machine, so the private key never leaves
/// it. Only the public line comes back, and only a line that is exactly one ed25519 key is accepted. GitHub's
/// host key is compared with `gh api meta` before the machine trusts it, and the key is added with the user's own
/// `gh`, never with the flag that makes it writable. Nothing here pushes: the last step only asks git for a dry run.
enum RepoSteps {
    static func all(for machine: Machine, repo: MachineRepo) -> [SetupStep] {
        let github = repo.github
        guard isValidRepo(github) else {
            let bad = CheckResult(state: .fail("\(github) is not owner/name"))
            let text = "Change the repo to owner/name in the machine's settings."
            return [
                SetupStep(id: .deployKey(github), title: "Deploy key (\(github))", needs: [.reachable, .baseTools],
                          cadence: .slow, probe: .local { _ in bad }, fix: .howTo(text)),
                SetupStep(id: .clone(github), title: "Clone (\(github))", needs: [.deployKey(github)],
                          cadence: .everyRefresh, probe: .local { _ in bad }, fix: .howTo(text)),
                SetupStep(id: .pushRefused(github), title: "Push is refused (\(github))", needs: [.clone(github)],
                          cadence: .slow, probe: .local { _ in bad }, fix: .howTo(text)),
            ]
        }
        return [
            SetupStep(id: .deployKey(github), title: "Deploy key (\(github))", needs: [.reachable, .baseTools],
                      cadence: .slow, probe: deployKeyProbe(github), fix: .run(deployKeyFix(machine, github))),
            SetupStep(id: .clone(github), title: "Clone (\(github))", needs: [.deployKey(github)],
                      cadence: .everyRefresh, probe: cloneProbe(repo), fix: .run(cloneFix(repo))),
            SetupStep(id: .pushRefused(github), title: "Push is refused (\(github))", needs: [.clone(github)],
                      cadence: .slow, probe: pushProbe(repo),
                      fix: .howTo("A push GitHub accepted means the key can write. Remove it on GitHub (Settings > Deploy keys) and run Deploy key's Set up again; it adds the key read-only.")),
        ]
    }

    // MARK: Names

    /// `owner/name` as one word for file and host names: lowercase, anything outside `[a-z0-9-]` becomes `-`.
    static func slug(_ github: String) -> String {
        var out = String.UnicodeScalarView()
        for scalar in github.lowercased().unicodeScalars {
            let plain = scalar.isASCII && ((scalar >= "a" && scalar <= "z") || (scalar >= "0" && scalar <= "9") || scalar == "-")
            out.append(plain ? scalar : "-")
        }
        return String(out)
    }
    /// The ssh host alias git uses for the repo: `github-agents-<slug>`.
    static func alias(_ github: String) -> String { "github-agents-\(slug(github))" }
    /// The private key file on the machine: `~/.ssh/agents-<slug>`.
    static func keyFile(_ github: String) -> String { "~/.ssh/agents-\(slug(github))" }
    /// The clone URL through the alias: `git@github-agents-<slug>:<owner>/<name>.git`.
    static func remoteURL(_ github: String) -> String { "git@\(alias(github)):\(github).git" }

    /// Where the repo is checked out on the machine.
    static func checkoutPath(_ repo: MachineRepo) -> String {
        if let path = repo.machinePath, !path.isEmpty { return path }
        let name = repo.github.split(separator: "/").last.map(String.init) ?? repo.github
        return "~/src/\(name)"
    }

    /// `owner/name` with plain characters only and no leading dash or dot, so it can sit in a path, a host alias,
    /// an ssh config and a `gh` argument.
    static func isValidRepo(_ github: String) -> Bool {
        github.range(of: #"\A[A-Za-z0-9][A-Za-z0-9._-]*/(?!\.{1,2}\z)[A-Za-z0-9._][A-Za-z0-9._-]*\z"#, options: .regularExpression) != nil
    }

    static func isValidMachineName(_ name: String) -> Bool {
        name.range(of: #"\A[A-Za-z0-9][A-Za-z0-9._-]{0,63}\z"#, options: .regularExpression) != nil
    }

    // MARK: Validation of what a machine or GitHub sent

    private static let keyPattern = #"\Assh-ed25519 [A-Za-z0-9+/]{60,80}={0,3}( [A-Za-z0-9@._-]{1,100})?\z"#
    private static let bodyPattern = #"\Assh-ed25519 [A-Za-z0-9+/]{60,80}={0,3}\z"#

    /// The public key line when `text` is exactly one ed25519 key (trailing whitespace allowed), else nil. A
    /// machine's file can hold anything; only a line that passes here is ever shown, compared or sent to GitHub.
    static func singleEd25519Line(_ text: String) -> String? {
        var line = Substring(text)
        while let last = line.last, last.isWhitespace { line = line.dropLast() }
        guard !line.contains("\n"), !line.contains("\r"),
              line.range(of: keyPattern, options: .regularExpression) != nil else { return nil }
        return String(line)
    }

    /// `ssh-ed25519 <body>`: the first two fields of a key line.
    private static func keyBody(_ line: String) -> String {
        line.split(separator: " ", omittingEmptySubsequences: true).prefix(2).joined(separator: " ")
    }

    /// The first non-empty line of what a machine or a tool said, for a message: masked first (`mask` is the
    /// context's), then cut, so a cut can't leave a piece of a vault value.
    private static func firstLine(_ text: String, _ mask: (String) -> String, limit: Int = 200) -> String {
        let line = mask(text).split(whereSeparator: { $0 == "\n" || $0 == "\r" })
            .map { $0.trimmingCharacters(in: .whitespaces) }.first { !$0.isEmpty } ?? ""
        return String(line.prefix(limit))
    }

    private static var gh: String { MachineTools.path("gh") }

    /// The repo's deploy keys, all pages merged into one array (`--paginate`; not `--slurp`, which nests them).
    private static func listArgv(_ github: String) -> [String] { [gh, "api", "--paginate", "repos/\(github)/keys"] }

    /// The keys in gh's answer, or nil when it is not a JSON array of objects.
    private static func parseKeys(_ text: String) -> [[String: Any]]? {
        guard let data = text.data(using: .utf8) else { return nil }
        return (try? JSONSerialization.jsonObject(with: data)) as? [[String: Any]]
    }

    /// The entry for `pub` (its first two fields), if GitHub lists it.
    private static func listed(_ pub: String, in keys: [[String: Any]]) -> [String: Any]? {
        let mine = keyBody(pub)
        return keys.first { ($0["key"] as? String).map(keyBody) == mine }
    }

    /// A line `Host <alias>` in the ssh config, indented or followed by spaces or more host names.
    static func aliasPattern(_ alias: String) -> String { "^[[:space:]]*Host[[:space:]]+\(alias)([[:space:]]|$)" }

    private static func canWrite(_ github: String) -> String {
        "the key can write to \(github); remove it in Settings > Deploy keys on GitHub, then set up again"
    }

    // MARK: Deploy key

    /// The remote part prints the key, then `@@config` with the alias check, then `@@ssh` with what GitHub said.
    /// The markers are read from the end, so a key file that holds a marker line cannot fake a later part.
    private static func deployKeyProbe(_ github: String) -> Probe {
        let alias = alias(github)
        let pub = RemoteShell.path(keyFile(github) + ".pub")
        let script = "f=\(pub); if [ -f \"$f\" ]; then head -c 1024 \"$f\"; fi; echo; "
            + "echo @@config; if grep -Eq \(LaneStore.quote(aliasPattern(alias))) \"$HOME/.ssh/config\" 2>/dev/null; then echo alias=yes; else echo alias=no; fi; "
            + "echo @@ssh; ssh -T -o BatchMode=yes -o ConnectTimeout=8 \(alias) </dev/null 2>&1 || true"
        return .remote(script: script) { out, ctx in
            await parseDeployKey(out, github: github, ctx: ctx)
        }
    }

    private static func parseDeployKey(_ out: RemoteOutput, github: String, ctx: StepContext) async -> CheckResult {
        let machine = ctx.machine.name
        let lines = out.text.split(separator: "\n", omittingEmptySubsequences: false).map(String.init)
        guard let sshAt = lines.lastIndex(of: "@@ssh"), let configAt = lines[..<sshAt].lastIndex(of: "@@config") else {
            return CheckResult(state: .cantTell("no answer from \(machine)"))
        }
        let pubText = lines[..<configAt].joined(separator: "\n")
        let aliasOK = lines[(configAt + 1)..<sshAt].contains("alias=yes")
        let sshText = lines[(sshAt + 1)...].joined(separator: "\n")

        if pubText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return CheckResult(state: .fail("no deploy key on \(machine) yet"))
        }
        guard let pub = singleEd25519Line(pubText) else {
            return CheckResult(state: .fail("unexpected public key on \(machine)"))
        }

        let answer = await ctx.runner.run(listArgv(github), cwd: nil, timeout: 30)
        guard answer.code == 0 else {
            let why = firstLine(answer.err.isEmpty ? answer.out : answer.err, ctx.mask)
            return CheckResult(state: .cantTell("gh could not list the deploy keys of \(github)" + (why.isEmpty ? "" : ": \(why)")))
        }
        guard let keys = parseKeys(answer.out) else {
            return CheckResult(state: .cantTell("can't read GitHub's list of deploy keys"))
        }
        guard let entry = listed(pub, in: keys) else {
            return CheckResult(state: .fail("the key is not on \(github) yet"),
                               rows: [SubRow(name: "Key on GitHub", ok: false, detail: "not listed")])
        }
        guard (entry["read_only"] as? Bool) == true else {
            return CheckResult(state: .fail(canWrite(github)),
                               rows: [SubRow(name: "Key on GitHub", ok: false, detail: "can write")])
        }
        var rows = [SubRow(name: "Key on GitHub", ok: true, detail: "read-only")]

        guard aliasOK else {
            rows.append(SubRow(name: "ssh alias", ok: false, detail: "missing"))
            return CheckResult(state: .fail("no ssh alias \(alias(github)) on \(machine)"), rows: rows)
        }
        rows.append(SubRow(name: "ssh alias", ok: true, detail: alias(github)))

        if sshText.contains("successfully authenticated") {
            rows.append(SubRow(name: "ssh to GitHub", ok: true, detail: "authenticated"))
            return CheckResult(state: .pass("read-only key on \(github)"), rows: rows)
        }
        let said = firstLine(sshText, ctx.mask)
        rows.append(SubRow(name: "ssh to GitHub", ok: false, detail: said))
        if sshText.contains("Permission denied") || sshText.contains("Host key verification failed") {
            return CheckResult(state: .fail("GitHub refused ssh through \(alias(github)): \(said)"), rows: rows)
        }
        return CheckResult(state: .cantTell("can't tell if ssh reaches GitHub" + (said.isEmpty ? "" : ": \(said)")), rows: rows)
    }

    private static func deployKeyFix(_ machine: Machine, _ github: String) -> FixPlan {
        let name = machine.name
        let key = RemoteShell.path(keyFile(github))
        let keyPub = RemoteShell.path(keyFile(github) + ".pub")
        let alias = alias(github)
        let configBlock = "\nHost \(alias)\n  HostName github.com\n  User git\n  IdentityFile ~/.ssh/agents-\(slug(github))\n  IdentitiesOnly yes\n"
        let keygen = "set -e; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"\n"
            + "[ -f \(key) ] || ssh-keygen -q -t ed25519 -N '' -C \(LaneStore.quote("agents-\(name)")) -f \(key)\n"
            + "touch \"$HOME/.ssh/config\"; chmod 600 \"$HOME/.ssh/config\"\n"
            + "grep -Eq \(LaneStore.quote(aliasPattern(alias))) \"$HOME/.ssh/config\" || printf \(LaneStore.quote(configBlock)) >> \"$HOME/.ssh/config\"\n"
            + "cat \(keyPub)"
        let keyscan = "ssh-keyscan -t ed25519 github.com 2>/dev/null"
        let metaArgv = [gh, "api", "meta", "--jq", ".ssh_keys[]"]
        @Sendable func knownHosts(_ hostKey: String) -> String {
            "set -e; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"; kh=\"$HOME/.ssh/known_hosts\"; touch \"$kh\"; chmod 600 \"$kh\"; "
                + "line=\(LaneStore.quote("github.com " + hostKey)); "
                + "grep -qxF \"$line\" \"$kh\" || printf '\\n%s\\n' \"$line\" >> \"$kh\""
        }
        @Sendable func addArgv(_ file: String) -> [String] {
            [gh, "repo", "deploy-key", "add", file, "-R", github, "--title", "agents-\(name)"]
        }
        let display = "On \(name):\n\(keygen)\n\nOn this Mac:\n\(metaArgv.joined(separator: " "))\n\n"
            + "On \(name):\n\(keyscan)\n(its key must be one of the keys from gh api meta)\n\n"
            + "On \(name): add GitHub's key from gh api meta to ~/.ssh/known_hosts unless it is already there\n\n"
            + "On this Mac:\n\(listArgv(github).joined(separator: " "))\n(the key is not added again if GitHub already lists it)\n\n"
            + "On this Mac:\n\(addArgv("<the public key file>").joined(separator: " "))\n(read-only: the key is added without write access)"

        return FixPlan(display: display, needsAdmin: false) { ctx in
            guard isValidMachineName(name) else { return FixOutcome(ok: false, message: "the machine name \(name) is not a plain name") }

            // 1. the key and the ssh alias, made on the machine; only the public line comes back
            let made = await MachineRun.script(keygen, ctx: ctx)
            guard made.code == 0 else {
                return FixOutcome(ok: false, message: "could not make the key on \(name): \(firstLine(made.err.isEmpty ? made.out : made.err, ctx.mask))")
            }
            guard let pub = singleEd25519Line(made.out) else {
                return FixOutcome(ok: false, message: "unexpected public key from \(name); nothing was added")
            }

            // 2. GitHub's host keys, from this Mac
            let meta = await MachineRun.local(metaArgv, ctx: ctx)
            var trusted = Set<String>()
            for line in meta.out.split(whereSeparator: { $0 == "\n" || $0 == "\r" }) {
                let text = line.trimmingCharacters(in: .whitespaces)
                if text.range(of: bodyPattern, options: .regularExpression) != nil { trusted.insert(text) }
            }
            guard meta.code == 0, !trusted.isEmpty else {
                return FixOutcome(ok: false, message: "gh api meta gave no GitHub host key: \(firstLine(meta.err, ctx.mask)); nothing was added")
            }

            // 3. what the machine sees at github.com must be one of them
            let scanned = await MachineRun.script(keyscan, ctx: ctx)
            var seen: [String] = []
            for line in scanned.out.split(whereSeparator: { $0 == "\n" || $0 == "\r" }) {
                let words = line.split(separator: " ", omittingEmptySubsequences: true)
                if words.count >= 3, words[1] == "ssh-ed25519" { seen.append("ssh-ed25519 \(words[2])") }
            }
            guard !seen.isEmpty else {
                return FixOutcome(ok: false, message: "could not read GitHub's host key from \(name); nothing was added")
            }
            guard let hostKey = seen.first(where: { trusted.contains($0) }) else {
                return FixOutcome(ok: false, message: "GitHub's host key seen from \(name) differs from gh api meta; nothing was added")
            }

            // 4. trust this Mac's copy of it, never the machine's
            let trust = await MachineRun.script(knownHosts(hostKey), ctx: ctx)
            guard trust.code == 0 else {
                return FixOutcome(ok: false, message: "could not write known_hosts on \(name): \(firstLine(trust.err, ctx.mask))")
            }

            // 5. a key GitHub already lists is not added again: read-only is a repair that worked, writable stops
            let current = await MachineRun.local(listArgv(github), ctx: ctx)
            guard current.code == 0, let keys = parseKeys(current.out) else {
                return FixOutcome(ok: false, message: "gh could not list the deploy keys of \(github): \(firstLine(current.err.isEmpty ? current.out : current.err, ctx.mask))")
            }
            if let entry = listed(pub, in: keys) {
                if (entry["read_only"] as? Bool) == true {
                    return FixOutcome(ok: true, message: "the read-only deploy key is already on \(github); the machine is set up")
                }
                return FixOutcome(ok: false, message: canWrite(github))
            }

            // 6. the public key goes to GitHub through the user's gh, read-only
            let dir = (NSTemporaryDirectory() as NSString).appendingPathComponent("notchling-keys")
            let file = (dir as NSString).appendingPathComponent("\(name)-\(slug(github)).pub")
            do {
                try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true,
                                                        attributes: [.posixPermissions: 0o700])
                try (pub + "\n").write(toFile: file, atomically: true, encoding: .utf8)
            } catch {
                return FixOutcome(ok: false, message: "could not write the public key file on this Mac")
            }
            defer { try? FileManager.default.removeItem(atPath: file) }
            let added = await MachineRun.local(addArgv(file), ctx: ctx)
            guard added.code == 0 else {
                return FixOutcome(ok: false, message: "gh could not add the deploy key to \(github): \(firstLine(added.err.isEmpty ? added.out : added.err, ctx.mask))")
            }
            return FixOutcome(ok: true, message: "added a read-only deploy key to \(github)")
        }
    }

    // MARK: Clone

    private static func cloneProbe(_ repo: MachineRepo) -> Probe {
        let path = RemoteShell.path(checkoutPath(repo))
        let wanted = remoteURL(repo.github)
        return .remote(script: "git -C \(path) remote get-url origin 2>/dev/null || echo none") { out, ctx in
            let origin = firstLine(out.text, ctx.mask)
            if origin.isEmpty && out.code != 0 { return CheckResult(state: .cantTell("no answer from \(ctx.machine.name)")) }
            if origin == wanted { return CheckResult(state: .pass(checkoutPath(repo))) }
            if origin.isEmpty || origin == "none" { return CheckResult(state: .fail("not cloned")) }
            return CheckResult(state: .fail("\(checkoutPath(repo)) has origin \(origin), not \(wanted)"))
        }
    }

    private static func cloneFix(_ repo: MachineRepo) -> FixPlan {
        let path = RemoteShell.path(checkoutPath(repo))
        let url = remoteURL(repo.github)
        let script = "set -e; p=\(path); if [ -e \"$p\" ]; then echo \"$p already exists; Notchling won't touch it\" >&2; exit 1; fi; "
            + "mkdir -p \"$(dirname \"$p\")\"; GIT_SSH_COMMAND='ssh -o BatchMode=yes' git clone -- \(url) \"$p\""
        return FixPlan(display: script, needsAdmin: false) { ctx in
            let result = await MachineRun.script(script, ctx: ctx)
            guard result.code == 0 else {
                return FixOutcome(ok: false, message: firstLine(result.err.isEmpty ? result.out : result.err, ctx.mask))
            }
            return FixOutcome(ok: true, message: "cloned \(repo.github) to \(checkoutPath(repo))")
        }
    }

    // MARK: Push is refused

    /// `git push --dry-run` talks to GitHub's receive-pack and sends nothing: with a read-only key GitHub closes
    /// the connection first, and with a writable one git only lists what it would do. No ref is ever created.
    private static func pushProbe(_ repo: MachineRepo) -> Probe {
        let path = RemoteShell.path(checkoutPath(repo))
        let script = "GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND='ssh -o BatchMode=yes -o ConnectTimeout=8' "
            + "git -c core.hooksPath=/dev/null -C \(path) push --dry-run origin HEAD:refs/heads/agents-push-test </dev/null 2>&1"
        return .remote(script: script) { out, ctx in
            if out.code != 0 && out.text.contains("marked as read only") {
                return CheckResult(state: .pass("GitHub refuses a push with this key"))
            }
            if out.code == 0 {
                return CheckResult(state: .fail("this machine can push to \(repo.github); the deploy key can write"))
            }
            let line = firstLine(out.text, ctx.mask)
            return CheckResult(state: .fail(line.isEmpty ? "the push check failed (code \(out.code))" : line))
        }
    }
}

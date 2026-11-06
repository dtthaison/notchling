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
                      cadence: .slow, probe: deployKeyProbe(github), fix: .run(deployKeyFix(machine, github)),
                      replace: replaceKeyPlan(machine, github)),
            SetupStep(id: .clone(github), title: "Clone (\(github))", needs: [.deployKey(github)],
                      cadence: .everyRefresh, probe: cloneProbe(repo), fix: .run(cloneFix(repo))),
            SetupStep(id: .pushRefused(github), title: "Push is refused (\(github))", needs: [.clone(github)],
                      cadence: .slow, probe: pushProbe(repo, linux: machine.os == .linux),
                      fix: .howTo("A push GitHub accepted means the key can write. Remove it on GitHub (Settings > Deploy keys) and run Deploy key's Set up again; it adds the key read-only.\nA login of the worker account itself (gh, a default ssh key, the keychain, ~/.git-credentials) is a second way to push: log it out or remove it on the machine, then Check again.")),
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

    /// `ssh-ed25519 <base64>` of a key line: the type and the body, the comment dropped. Nil unless `text` is a single
    /// line that holds an ed25519 key, so a multi-line input can never be taken for a match.
    static func normalisedKey(_ text: String) -> String? {
        var line = Substring(text)
        while let last = line.last, last.isWhitespace { line = line.dropLast() }
        guard !line.contains("\n"), !line.contains("\r") else { return nil }
        let words = line.split(separator: " ", omittingEmptySubsequences: true)
        guard words.count >= 2, words[0] == "ssh-ed25519" else { return nil }
        let normal = "ssh-ed25519 \(words[1])"
        return normal.range(of: bodyPattern, options: .regularExpression) != nil ? normal : nil
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

    /// The entry for `pub` (type and body, the comment ignored), if GitHub lists it.
    private static func listed(_ pub: String, in keys: [[String: Any]]) -> [String: Any]? {
        guard let mine = normalisedKey(pub) else { return nil }
        return keys.first { ($0["key"] as? String).flatMap(normalisedKey) == mine }
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
                               rows: [notListedRow])
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

    // MARK: Replace key

    /// The Check's row that says GitHub has no record of the machine's key. Replace key is offered for it.
    private static let notListedRow = SubRow(name: "Key on GitHub", ok: false, detail: "not listed")

    /// Whether the card offers Replace key for this Check: the key is ✓, or it is on the machine and GitHub has no
    /// record of it. A missing key, a writable one, a missing alias and "can't tell" are not offered.
    static func offersReplace(_ result: CheckResult?) -> Bool {
        switch result?.state {
        case .pass?: return true
        case .fail?: return result?.rows.contains(notListedRow) ?? false
        default: return false
        }
    }

    /// `yyyymmdd` in UTC, for the new key's title and comment.
    private static func dateStamp(_ date: Date) -> String {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC") ?? .current
        let c = calendar.dateComponents([.year, .month, .day], from: date)
        return String(format: "%04d%02d%02d", c.year ?? 0, c.month ?? 0, c.day ?? 0)
    }

    /// GitHub's id of a key: a positive JSON integer and nothing else, since it goes into an argv.
    private static func keyID(_ entry: [String: Any]) -> Int? {
        guard let number = entry["id"] as? NSNumber, CFGetTypeID(number) != CFBooleanGetTypeID(),
              !CFNumberIsFloatType(number) else { return nil }
        let id = number.intValue
        return id > 0 ? id : nil
    }

    /// Whether a machine name ends in `-` and 8 ASCII digits, which is how Replace key dates a title: such a name
    /// could own another machine's `agents-<machine>-<date>` title.
    static func endsInReplacedKeyDate(_ name: String) -> Bool {
        let tail = Array(name.unicodeScalars.suffix(9))
        return tail.count == 9 && tail[0] == "-" && tail.dropFirst().allSatisfy { $0.value >= 48 && $0.value <= 57 }
    }

    /// The titles this machine gives its keys: `agents-<machine>` (Set up) or `agents-<machine>-<8 digits>` (Replace key).
    private static func isOwnKeyTitle(_ title: String, machine: String) -> Bool {
        let base = "agents-\(machine)"
        if title == base { return true }
        guard title.hasPrefix(base + "-") else { return false }
        let tail = title.dropFirst(base.count + 1)
        return tail.count == 8 && tail.allSatisfy { $0.isASCII && $0.isNumber }
    }

    private static func addArgv(_ file: String, github: String, title: String) -> [String] {
        [gh, "repo", "deploy-key", "add", file, "-R", github, "--title", title]
    }

    private static func deleteArgv(_ id: String, github: String) -> [String] {
        [gh, "repo", "deploy-key", "delete", id, "-R", github]
    }

    /// Replaces the machine's deploy key for one repo. The old key stays valid until the new one is proven: the
    /// new pair is made beside the old (`<key>.new`), added read-only, and has to authenticate, be refused a push
    /// and be listed read-only before it is moved over the old files. Only then is the old key deleted from GitHub,
    /// found by its public key text (never by title). A failure before the move removes the `.new` files and the
    /// new key from GitHub and leaves the old key and the ssh alias as they were.
    private static func replaceKeyPlan(_ machine: Machine, _ github: String) -> FixPlan {
        let name = machine.name
        let path = keyFile(github)
        let key = RemoteShell.path(path), keyPub = RemoteShell.path(path + ".pub")
        let newKey = RemoteShell.path(path + ".new"), newPub = RemoteShell.path(path + ".new.pub")
        let readOld = "f=\(keyPub); if [ -f \"$f\" ]; then head -c 1024 \"$f\"; fi"
        func keygen(_ comment: String) -> String {
            "set -e; mkdir -p \"$HOME/.ssh\"; chmod 700 \"$HOME/.ssh\"\n"
                + "rm -f \(newKey) \(newPub)\n"
                + "ssh-keygen -q -t ed25519 -N '' -C \(LaneStore.quote(comment)) -f \(newKey)\n"
                + "cat \(newPub)"
        }
        // -F /dev/null: the check uses the new key alone, not whatever the ssh config would offer
        let sshCheck = "ssh -F /dev/null -i \(newKey) -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o BatchMode=yes -o ConnectTimeout=8 -T git@github.com </dev/null 2>&1 || true"
        // a throwaway repo, so no checkout is needed; hooks off and stdin closed, as the Push-is-refused check
        let pushCheck = "k=\(newKey); t=$(mktemp -d) || { echo '@@push 99'; exit 0; }\n"
            + "trap 'rm -rf \"$t\"' EXIT\n"
            + "git -c core.hooksPath=/dev/null init -q \"$t\" </dev/null >/dev/null 2>&1 && "
            + "git -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=notchling -c user.email=notchling@localhost "
            + "-C \"$t\" commit -q --allow-empty -m probe </dev/null >/dev/null 2>&1 || { echo '@@push 98'; exit 0; }\n"
            + "GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND=\"ssh -F /dev/null -i '$k' -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o BatchMode=yes -o ConnectTimeout=8\" "
            + "git -c core.hooksPath=/dev/null -C \"$t\" push --dry-run git@github.com:\(github).git HEAD:refs/heads/agents-push-test </dev/null 2>&1\n"
            + "echo \"@@push $?\""
        // Exit 43: a file is missing and 41: the private key could not move, so nothing moved; 42: only the .pub could
        // not move (it is then rewritten from the private key, through a temporary file). These are far from the
        // codes a signal (1 to 15, as the runner reports it), ssh (255) or a timeout (124) can give: any other code
        // proves nothing about what was moved.
        let tmpPub = RemoteShell.path(path + ".pub.tmp")
        let swap = "[ -f \(newKey) ] && [ -f \(newPub) ] || exit 43\n"
            + "mv -f \(newKey) \(key) || exit 41\n"
            + "mv -f \(newPub) \(keyPub) || { ssh-keygen -y -f \(key) > \(tmpPub) 2>/dev/null && mv -f \(tmpPub) \(keyPub); rm -f \(tmpPub); exit 42; }"
        let cleanup = "rm -f \(newKey) \(newPub)"

        let display = "On \(name):\n\(readOld)\n(the old public key is read first)\n\n"
            + "On \(name) (<yyyymmdd> is today's date in UTC):\n\(keygen("agents-\(name)-<yyyymmdd>"))\n(the new key is made next to the old one; the old key stays valid)\n\n"
            + "On this Mac:\n\(listArgv(github).joined(separator: " "))\n(the keys now on GitHub; a new key that is already there stops the run before anything is added)\n\n"
            + "On this Mac:\n\(addArgv("<the new public key file>", github: github, title: "agents-\(name)-<yyyymmdd>").joined(separator: " "))\n(read-only: the key is added without write access)\n\n"
            + "On \(name):\n\(sshCheck)\n(the new key must authenticate)\n\n"
            + "On \(name):\n\(pushCheck)\n(GitHub must refuse the push)\n\n"
            + "On this Mac:\n\(listArgv(github).joined(separator: " "))\n(GitHub must list the new key as read-only)\n\n"
            + "Only when every check above passed:\nOn \(name):\n\(swap)\n\n"
            + "On this Mac:\n\(deleteArgv("<id of the old key>", github: github).joined(separator: " "))\n(the old key is found by its public key text, and only deleted when its title is this machine's own: agents-\(name) or agents-\(name)-<8 digits>)\n\n"
            + "If a check fails before the swap, these files are removed from \(name):\n\(cleanup)\n\n"
            + "and the new key is deleted from GitHub, only a key that was not listed before the add and has the new key's title:\n"
            + "\(listArgv(github).joined(separator: " "))\n\(deleteArgv("<id of the new key>", github: github).joined(separator: " "))"

        return FixPlan(display: display, needsAdmin: false) { ctx in
            guard isValidMachineName(name) else { return FixOutcome(ok: false, message: "the machine name \(name) is not a plain name") }
            guard !endsInReplacedKeyDate(name) else {
                return FixOutcome(ok: false, message: "a machine name can't end in -<8 digits>; that's how replaced keys are titled (\(name))")
            }
            let title = "agents-\(name)-\(dateStamp(ctx.step.now))"
            func said(_ r: CommandResult) -> String { firstLine(r.err.isEmpty ? r.out : r.err, ctx.mask) }

            /// The ids on GitHub before the add: nothing in this list is ever deleted by this run.
            var before = Set<Int>()

            /// Deletes the new key from GitHub: nil when it is gone (or was never there), else what to tell the user.
            /// Only an entry with the new key's text that this run added is deleted: its id was not in the listing
            /// before the add and its title is this run's own.
            func removeNewKeyFromGitHub(_ pub: String) async -> String? {
                let still = "the new key (\(title)) is still on GitHub; remove it in Settings → Deploy keys"
                let current = await MachineRun.local(listArgv(github), ctx: ctx)
                guard current.code == 0, let keys = parseKeys(current.out) else { return still }
                guard let entry = listed(pub, in: keys) else { return nil }
                guard let id = keyID(entry), !before.contains(id), (entry["title"] as? String) == title else {
                    return "a key with the new key's text is on GitHub, but this run did not add it under \(title), so nothing was deleted; look in Settings → Deploy keys"
                }
                let gone = await MachineRun.local(deleteArgv(String(id), github: github), ctx: ctx)
                return gone.code == 0 ? nil : still
            }

            /// A failure before the move: the `.new` files go, and so does the new key on GitHub if it got there.
            func undo(_ why: String, newKey pub: String? = nil, onGitHub: Bool = false) async -> FixOutcome {
                var notes: [String] = []
                let removed = await MachineRun.script(cleanup, ctx: ctx, timeout: 60)
                if removed.code != 0 { notes.append("the .new key files could not be removed from ~/.ssh on \(name)") }
                if onGitHub, let pub, let note = await removeNewKeyFromGitHub(pub) { notes.append(note) }
                return FixOutcome(ok: false, message: ([why] + notes + ["the old key and the ssh alias are untouched"]).joined(separator: "; "))
            }

            // 1. the old public key, read before anything changes: it is how the old key is found on GitHub
            let reading = await MachineRun.script(readOld, ctx: ctx, timeout: 60)
            guard reading.code == 0, let oldPub = singleEd25519Line(reading.out) else {
                return FixOutcome(ok: false, message: "unexpected public key on \(name); nothing was changed")
            }

            // 2. the new pair, next to the old one; only its public line comes back
            let made = await MachineRun.script(keygen("\(title)"), ctx: ctx)
            guard made.code == 0 else { return await undo("could not make the new key on \(name): \(said(made))") }
            guard let pub = singleEd25519Line(made.out) else { return await undo("unexpected public key from \(name); nothing was added") }
            guard normalisedKey(pub) != normalisedKey(oldPub) else { return await undo("the new key is the old key; nothing was added") }

            // 2b. what GitHub lists now. The new key must not be on the repo already (a machine could hand back
            // another key's text); and nothing in this list is ever deleted by this run.
            let prior = await MachineRun.local(listArgv(github), ctx: ctx)
            guard prior.code == 0, let priorKeys = parseKeys(prior.out) else {
                return await undo("gh could not list the deploy keys of \(github): \(said(prior)); nothing was added")
            }
            guard listed(pub, in: priorKeys) == nil else { return await undo("the new key is already on GitHub; nothing changed") }
            before = Set(priorKeys.compactMap(keyID))

            // 3. read-only, through the user's gh. A failed add is looked for on GitHub too: gh may have been cut short.
            let dir = (NSTemporaryDirectory() as NSString).appendingPathComponent("notchling-keys")
            let file = (dir as NSString).appendingPathComponent("\(name)-\(slug(github))-new.pub")
            do {
                try FileManager.default.createDirectory(atPath: dir, withIntermediateDirectories: true,
                                                        attributes: [.posixPermissions: 0o700])
                try (pub + "\n").write(toFile: file, atomically: true, encoding: .utf8)
            } catch {
                return await undo("could not write the public key file on this Mac")
            }
            defer { try? FileManager.default.removeItem(atPath: file) }
            let added = await MachineRun.local(addArgv(file, github: github, title: title), ctx: ctx)
            guard added.code == 0 else {
                return await undo("gh could not add the new deploy key to \(github): \(said(added))", newKey: pub, onGitHub: true)
            }

            // 4. the new key has to work and still be read-only, from three sides
            let auth = await MachineRun.script(sshCheck, ctx: ctx, timeout: 60)
            guard auth.out.contains("successfully authenticated") else {
                let line = firstLine(auth.out.isEmpty ? auth.err : auth.out, ctx.mask)
                return await undo("the new key could not authenticate to GitHub over ssh" + (line.isEmpty ? "" : ": \(line)"),
                                  newKey: pub, onGitHub: true)
            }
            let pushed = await MachineRun.script(pushCheck, ctx: ctx, timeout: 90)
            let pushLines = pushed.out.components(separatedBy: "\n")
            guard let marker = pushLines.lastIndex(where: { $0.hasPrefix("@@push ") }), let pushCode = Int(pushLines[marker].dropFirst(7)) else {
                let line = firstLine(pushed.out.isEmpty ? pushed.err : pushed.out, ctx.mask)
                return await undo("the push check with the new key failed" + (line.isEmpty ? "" : ": \(line)"), newKey: pub, onGitHub: true)
            }
            let git = pushLines[..<marker].joined(separator: "\n")
            if pushCode == 0 {
                return await undo("the new key can write to \(github): a dry-run push was accepted", newKey: pub, onGitHub: true)
            }
            guard git.contains("marked as read only") else {
                let line = firstLine(git, ctx.mask)
                return await undo("the push check with the new key failed" + (line.isEmpty ? " (code \(pushCode))" : ": \(line)"),
                                  newKey: pub, onGitHub: true)
            }
            let current = await MachineRun.local(listArgv(github), ctx: ctx)
            guard current.code == 0, let keys = parseKeys(current.out) else {
                return await undo("gh could not list the deploy keys of \(github): \(said(current))", newKey: pub, onGitHub: true)
            }
            guard let entry = listed(pub, in: keys) else {
                return await undo("GitHub does not list the new key on \(github)", newKey: pub, onGitHub: true)
            }
            guard (entry["read_only"] as? Bool) == true else {
                return await undo("the new key can write to \(github) according to GitHub", newKey: pub, onGitHub: true)
            }
            // the old key's id comes from GitHub's JSON and goes into an argv: it has to be an integer, and it
            // is found before the move so that a bad one stops everything while the old key is still in place
            var oldID: Int?
            var foreignTitle: String?
            if let old = listed(oldPub, in: keys) {
                guard let id = keyID(old) else {
                    return await undo("the old key's id on GitHub is not a number; no key was deleted and nothing was replaced",
                                      newKey: pub, onGitHub: true)
                }
                // the text matches; it is deleted only when the title is this machine's own as well
                let found = (old["title"] as? String) ?? ""
                if isOwnKeyTitle(found, machine: name) { oldID = id } else { foreignTitle = found }
            }

            // 5. every check passed: the new pair takes the old one's file names, so ~/.ssh/config stays as it is
            let moved = await MachineRun.script(swap, ctx: ctx, timeout: 60)
            switch moved.code {
            case 0: break
            case 42:
                return FixOutcome(ok: false, message: "new key in use, but the public key file on \(name) may be stale; Check again. The old key is still on GitHub: remove it in Settings → Deploy keys once the check passes")
            case 41, 43:
                // the script's own failures: nothing was moved
                return await undo("could not move the new key over the old one on \(name): \(said(moved))", newKey: pub, onGitHub: true)
            default:
                // 255 (the connection dropped), a timeout, a kill or a bare 1, 2, 3 (a signal): the move may have happened.
                // Nothing is deleted anywhere.
                return FixOutcome(ok: false, message: "can't tell whether the key was replaced on \(name) (code \(moved.code)); the swap may have happened; Check again before anything else. Nothing was removed from GitHub or from \(name)")
            }
            if let foreignTitle {
                // the title came from GitHub: masked, then cut, like any text from outside
                let shown = firstLine(foreignTitle, ctx.mask, limit: 80)
                return FixOutcome(ok: false, message: "replaced the key on \(name); the old key on GitHub is titled '\(shown)', not agents-\(name), so nothing was deleted; remove it in Settings → Deploy keys if it's \(name)'s")
            }
            guard let oldID else {
                return FixOutcome(ok: true, message: "replaced the deploy key of \(github) on \(name); the old key was not listed on GitHub, so nothing was deleted")
            }
            let deleted = await MachineRun.local(deleteArgv(String(oldID), github: github), ctx: ctx)
            guard deleted.code == 0 else {
                let why = said(deleted)
                return FixOutcome(ok: false, message: "new key in use; the old key is still on GitHub, remove it in Settings → Deploy keys" + (why.isEmpty ? "" : " (gh said: \(why))"))
            }
            return FixOutcome(ok: true, message: "replaced the deploy key of \(github) on \(name); the old key was deleted from GitHub")
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
    /// After the dry run, the worker account's own ways into GitHub (I5). The dry run goes through the `github-agents-*`
    /// alias with `IdentitiesOnly yes`, so it never sees a personal `gh` login, a default ssh key, a keychain password
    /// or a stored credential of the account. Each check keeps what its tool printed on the machine and says only a
    /// token line: an exit code, or for ssh the account name GitHub greeted (cut to GitHub's own name characters).
    /// None of them writes: `ssh -T` only authenticates, and `StrictHostKeyChecking=yes` keeps a user's ssh config
    /// (`accept-new`) from letting it add a host key.
    private static func loginChecks(linux: Bool) -> String {
        var lines = [
            "if command -v gh >/dev/null 2>&1; then gh auth status --hostname github.com >/dev/null 2>&1; echo \"gh=$?\"; else echo gh=none; fi",
            "r=$(ssh -o BatchMode=yes -o ConnectTimeout=8 -o StrictHostKeyChecking=yes -T git@github.com </dev/null 2>&1); "
                + "n=$(printf '%s\\n' \"$r\" | sed -n 's|^Hi \\([A-Za-z0-9._/-]*\\)!.*|\\1|p' | head -n 1); "
                + "if [ -n \"$n\" ]; then echo \"ssh=hi:$n\"; else case \"$r\" in *denied*) echo ssh=denied;; *) echo ssh=unknown;; esac; fi",
        ]
        if !linux { lines.append("security find-internet-password -s github.com >/dev/null 2>&1; echo \"keychain=$?\"") }
        lines.append("grep -q 'github.com' ~/.git-credentials 2>/dev/null; echo \"credfile=$?\"")
        return lines.joined(separator: "\n")
    }

    private static func pushProbe(_ repo: MachineRepo, linux: Bool) -> Probe {
        let path = RemoteShell.path(checkoutPath(repo))
        let script = "GIT_TERMINAL_PROMPT=0 GIT_SSH_COMMAND='ssh -o BatchMode=yes -o ConnectTimeout=8' "
            + "git -c core.hooksPath=/dev/null -C \(path) push --dry-run origin HEAD:refs/heads/agents-push-test </dev/null 2>&1\n"
            + "echo \"@@push $?\"\n" + loginChecks(linux: linux)
        return .remote(script: script) { out, ctx in
            // git's output, then its exit code on the marker line, then the login checks' token lines
            let lines = out.text.components(separatedBy: "\n")
            guard let marker = lines.firstIndex(where: { $0.hasPrefix("@@push ") }), let code = Int(lines[marker].dropFirst(7)) else {
                let line = firstLine(out.text, ctx.mask)
                return CheckResult(state: .fail(line.isEmpty ? "the push check failed (code \(out.code))" : line))
            }
            let git = lines[..<marker].joined(separator: "\n")
            if code == 0 {
                return CheckResult(state: .fail("this machine can push to \(repo.github); the deploy key can write"))
            }
            guard git.contains("marked as read only") else {
                let line = firstLine(git, ctx.mask)
                return CheckResult(state: .fail(line.isEmpty ? "the push check failed (code \(code))" : line))
            }
            return loginResult(Array(lines[(marker + 1)...]))
        }
    }

    /// The verdict of the login checks' token lines. A login is a ✗ with its reason; a check that couldn't tell is
    /// can't tell, never a pass.
    private static func loginResult(_ tokens: [String]) -> CheckResult {
        var values: [String: String] = [:]
        for line in tokens {
            guard let eq = line.firstIndex(of: "=") else { continue }
            values[String(line[..<eq])] = String(line[line.index(after: eq)...])
        }
        var rows: [SubRow] = [SubRow(name: "Push with the deploy key", ok: true, detail: "GitHub refuses it")]
        var reasons: [String] = [], doubts: [String] = []

        switch values["gh"] {
        case "0":
            reasons.append("gh is logged in as the worker; run gh auth logout")
            rows.append(SubRow(name: "gh login", ok: false, detail: "logged in"))
        case "none", "1": rows.append(SubRow(name: "gh login", ok: true, detail: "not logged in"))
        default:
            doubts.append("couldn't check gh")
            rows.append(SubRow(name: "gh login", ok: false, detail: "couldn't check"))
        }

        let ssh = values["ssh"] ?? ""
        if ssh.hasPrefix("hi:") {
            let name = String(ssh.dropFirst(3))
            if name.contains("/") {
                // a deploy key greets with owner/repo: scoped to one repo, and a writable one fails the dry run above
                rows.append(SubRow(name: "Default ssh key", ok: true, detail: "a deploy key for one repo"))
            } else {
                let plain = name.range(of: #"^[A-Za-z0-9-]{1,39}$"#, options: .regularExpression) != nil
                reasons.append("a default ssh key logs in to GitHub" + (plain ? " as \(name)" : ""))
                rows.append(SubRow(name: "Default ssh key", ok: false, detail: plain ? "logs in as \(name)" : "logs in"))
            }
        } else if ssh == "denied" {
            rows.append(SubRow(name: "Default ssh key", ok: true, detail: "GitHub does not know it"))
        } else {
            doubts.append("couldn't ask GitHub about a default ssh key")
            rows.append(SubRow(name: "Default ssh key", ok: false, detail: "no answer from GitHub"))
        }

        if let keychain = values["keychain"] {
            switch keychain {
            case "0":
                reasons.append("the keychain holds a github.com password")
                rows.append(SubRow(name: "Keychain", ok: false, detail: "holds a github.com password"))
            case "44": rows.append(SubRow(name: "Keychain", ok: true, detail: "no github.com password"))
            default:
                doubts.append("couldn't read the keychain")
                rows.append(SubRow(name: "Keychain", ok: false, detail: "couldn't read it"))
            }
        }

        if values["credfile"] == "0" {
            reasons.append("~/.git-credentials holds a github.com login")
            rows.append(SubRow(name: "~/.git-credentials", ok: false, detail: "holds a github.com login"))
        } else {
            rows.append(SubRow(name: "~/.git-credentials", ok: true, detail: "no github.com login"))
        }

        if !reasons.isEmpty { return CheckResult(state: .fail(reasons.joined(separator: "; ")), rows: rows) }
        if !doubts.isEmpty { return CheckResult(state: .cantTell(doubts.joined(separator: "; ")), rows: rows) }
        return CheckResult(state: .pass("GitHub refuses a push with this key; no other GitHub login on this account"), rows: rows)
    }
}

import Foundation

/// Credentials the user saves for agents to use without typing them into a chat.
///
/// - The list (names, kind, URL, variable names, a note for agents) is `vault.json`; anyone may read it.
/// - The values are one Keychain item that only the Notchling app may read (`VaultStore`).
/// - An agent runs `Notchling --secret <name> -- <command>`. That process asks the running app over a
///   socket the app answers only for its own binary (`VaultServer`); the app asks the user first when the
///   entry says so, then hands over the values. The command gets them as environment variables and its
///   output comes back with every value replaced, so a value never lands in a transcript.
/// - `--secret-request` opens the form for the user to add or update one (`VaultPrompt`).
struct VaultEntry: Codable, Identifiable, Equatable {
    enum Kind: String, Codable, CaseIterable { case login, token }

    var name: String
    var kind: Kind
    /// Shown to agents by `--secrets`: what it's for, which host or tool.
    var note: String = ""
    /// Where it's used (a site, an API, a registry), so an agent can tell which secret a task needs.
    /// Optional for lists saved before it existed.
    var url: String?
    /// Variable for the username (logins only).
    var userVar: String = ""
    /// Variable for the password or token.
    var secretVar: String = ""
    /// Ask the user before each use. Optional for lists saved before it existed.
    var ask: Bool?
    var updated: Date = Date()

    var id: String { name.lowercased() }

    var asks: Bool { ask ?? false }

    var variables: [String] { kind == .login ? [userVar, secretVar] : [secretVar] }

    /// `$<NAME>_URL`, set when the entry has a URL.
    var urlVar: String? { (url ?? "").isEmpty ? nil : "\(Self.prefix(for: name))_URL" }

    /// Every variable a command gets, URL included.
    var allVariables: [String] { variables + (urlVar.map { [$0] } ?? []) }

    /// `api.github.com` from `https://api.github.com/v3` or `api.github.com`, without `www.`.
    var host: String? { url.flatMap(Self.host(of:)) }

    static func host(of text: String) -> String? {
        let text = text.trimmingCharacters(in: .whitespaces)
        guard text.contains("://") || text.contains(".") else { return nil }
        guard let host = URL(string: text.contains("://") ? text : "https://" + text)?.host?.lowercased(), !host.isEmpty else { return nil }
        return host.hasPrefix("www.") ? String(host.dropFirst(4)) : host
    }

    /// `my-api` → `MY_API`, the prefix of the default variable names.
    static func prefix(for name: String) -> String {
        let mapped = name.uppercased().map { $0.isLetter || $0.isNumber ? $0 : "_" }
        let text = String(mapped)
        return text.first?.isNumber == true ? "_" + text : text
    }

    static func defaultVars(name: String, kind: Kind) -> (user: String, secret: String) {
        let p = prefix(for: name)
        return ("\(p)_USERNAME", kind == .login ? "\(p)_PASSWORD" : "\(p)_TOKEN")
    }

    static func validName(_ name: String) -> Bool {
        name.range(of: #"^[A-Za-z][A-Za-z0-9_.-]{0,63}$"#, options: .regularExpression) != nil
    }

    static func validVar(_ name: String) -> Bool {
        name.range(of: #"^[A-Za-z_][A-Za-z0-9_]{0,127}$"#, options: .regularExpression) != nil
    }
}

/// The username and the password or token, as stored in Keychain.
struct VaultValue: Codable, Equatable {
    var username: String = ""
    var secret: String = ""
}

/// The list in `vault.json`: no values, so the command line reads it directly.
enum SecretVault {
    static let indexURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Notchling/vault.json")

    static func all() -> [VaultEntry] {
        guard let data = try? Data(contentsOf: indexURL) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return ((try? decoder.decode([VaultEntry].self, from: data)) ?? []).sorted { $0.id < $1.id }
    }

    static func entry(_ name: String) -> VaultEntry? { all().first { $0.id == name.lowercased() } }

    static func write(_ list: [VaultEntry]) throws {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try FileManager.default.createDirectory(at: indexURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(list.sorted { $0.id < $1.id }).write(to: indexURL, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: indexURL.path)
        VaultSkill.update(list)
    }

    /// Why `entry` can't be saved into `list` in place of `old`, or nil.
    static func problem(saving entry: VaultEntry, in list: [VaultEntry], replacing old: VaultEntry?) -> String? {
        guard VaultEntry.validName(entry.name) else {
            return "Use letters, digits, - _ or . for the name, starting with a letter."
        }
        if let bad = entry.variables.first(where: { !VaultEntry.validVar($0) }) {
            return "\"\(bad)\" isn't a valid variable name."
        }
        if Set(entry.allVariables).count != entry.allVariables.count {
            return "Each value needs its own variable (\(entry.urlVar ?? "none") is the URL's)."
        }
        if list.contains(where: { $0.id == entry.id && $0.id != old?.id }) {
            return "There's already a secret named \(entry.name)."
        }
        let taken = Set(list.filter { $0.id != old?.id }.flatMap(\.allVariables))
        if let clash = entry.allVariables.first(where: taken.contains) {
            return "Another secret already uses \(clash)."
        }
        return nil
    }

    /// Entries that fit a URL, a host or words (from the task), best first; empty when none does.
    static func find(_ query: String, in list: [VaultEntry]) -> [VaultEntry] {
        let q = query.lowercased().trimmingCharacters(in: .whitespaces)
        guard !q.isEmpty else { return list }
        let qHost = VaultEntry.host(of: q)
        let words = q.split { !($0.isLetter || $0.isNumber || $0 == "." || $0 == "-" || $0 == "_") }
            .map(String.init).filter { $0.count >= 2 }
        func score(_ e: VaultEntry) -> Int {
            var s = 0
            if let qHost, let h = e.host {
                if h == qHost { s += 10 } else if qHost.hasSuffix("." + h) || h.hasSuffix("." + qHost) { s += 6 }
                // api.github.com and uploads.github.com share github.com.
                else if h.split(separator: ".").suffix(2) == qHost.split(separator: ".").suffix(2) { s += 3 }
            }
            if e.id == q { s += 8 }
            let text = "\(e.name) \(e.url ?? "") \(e.note)".lowercased()
            s += words.filter { text.contains($0) }.count
            return s
        }
        return list.map { ($0, score($0)) }.filter { $0.1 > 0 }.sorted { $0.1 > $1.1 }.map(\.0)
    }

    /// The environment a command gets for these entries, and the strings to hide in its output.
    static func environment(for entries: [(VaultEntry, VaultValue)]) -> (env: [String: String], hide: [String: String]) {
        var env: [String: String] = [:], hide: [String: String] = [:]
        for (entry, value) in entries {
            env[entry.secretVar] = value.secret
            if let urlVar = entry.urlVar, let url = entry.url { env[urlVar] = url }
            hide[value.secret] = "[secret:\(entry.name)]"
            if entry.kind == .login {
                env[entry.userVar] = value.username
                if !value.username.isEmpty, hide[value.username] == nil { hide[value.username] = "[secret:\(entry.name) username]" }
            }
        }
        return (env, hide)
    }

    /// One entry as `--secrets` prints it.
    static func describe(_ e: VaultEntry) -> String {
        var lines = ["\(e.name)  (\(e.kind.rawValue))  \(e.allVariables.map { "$" + $0 }.joined(separator: " "))"]
        if let url = e.url, !url.isEmpty { lines.append("    \(url)") }
        if !e.note.isEmpty { lines.append("    \(e.note)") }
        if e.asks { lines.append("    The user approves each use.") }
        return lines.joined(separator: "\n")
    }
}

/// Runs a command with extra environment variables, streaming its stdout and stderr through a `Redactor`.
enum SecretRunner {
    static func run(_ command: [String], extra: [String: String], hide: [String: String]) -> Int32 {
        let process = Process()
        if command.count == 1 {
            process.executableURL = URL(fileURLWithPath: "/bin/sh")
            process.arguments = ["-c", command[0]]
        } else {
            process.executableURL = URL(fileURLWithPath: "/usr/bin/env")
            process.arguments = command
        }
        process.environment = ProcessInfo.processInfo.environment.merging(extra) { _, new in new }
        process.standardInput = FileHandle.standardInput
        let out = Pipe(), err = Pipe()
        process.standardOutput = out
        process.standardError = err
        do { try process.run() } catch {
            FileHandle.standardError.write(Data("Couldn't run \(command[0]): \(error.localizedDescription)\n".utf8))
            return 127
        }
        let group = DispatchGroup()
        for (pipe, sink) in [(out, FileHandle.standardOutput), (err, FileHandle.standardError)] {
            group.enter()
            Thread.detachNewThread {
                let redactor = Redactor(hide)
                while true {
                    let chunk = pipe.fileHandleForReading.availableData
                    if chunk.isEmpty { break }
                    sink.write(redactor.feed(chunk))
                }
                sink.write(redactor.finish())
                group.leave()
            }
        }
        process.waitUntilExit()
        group.wait()
        return process.terminationReason == .uncaughtSignal ? 128 + process.terminationStatus : process.terminationStatus
    }
}

/// Replaces secret strings in a byte stream. A tail that could be the start of a secret waits for the next
/// read, so a value split across two reads is still caught. Strings shorter than 4 bytes aren't hidden:
/// they'd blank out ordinary output.
final class Redactor {
    private let pairs: [(Data, Data)]
    private let hold: Int
    private var pending = Data()

    init(_ hide: [String: String]) {
        pairs = hide.filter { $0.key.utf8.count >= 4 }
            .map { (Data($0.key.utf8), Data($0.value.utf8)) }
            .sorted { $0.0.count > $1.0.count }
        hold = max(0, (pairs.first?.0.count ?? 1) - 1)
    }

    func feed(_ data: Data) -> Data {
        pending.append(data)
        pending = replaced(pending)
        let keep = partialTail(pending)
        let cut = pending.count - keep
        let ready = Data(pending.prefix(cut))
        pending = Data(pending.suffix(keep))
        return ready
    }

    /// The longest tail of `data` (shorter than `hold + 1`) that could be the start of a secret.
    private func partialTail(_ data: Data) -> Int {
        let bytes = [UInt8](data)
        for length in stride(from: min(hold, bytes.count), to: 0, by: -1) {
            let tail = bytes[(bytes.count - length)...]
            if pairs.contains(where: { $0.0.count > length && $0.0.starts(with: tail) }) { return length }
        }
        return 0
    }

    func finish() -> Data {
        defer { pending = Data() }
        return replaced(pending)
    }

    private func replaced(_ data: Data) -> Data {
        var data = data
        for (secret, mask) in pairs {
            var start = data.startIndex
            while let range = data.range(of: secret, in: start..<data.endIndex) {
                data.replaceSubrange(range, with: mask)
                start = range.lowerBound + mask.count
            }
        }
        return Data(data)
    }
}

// MARK: - Skill

/// The `notchling-vault` skill: tells agents the vault exists, which names it holds, how to pick one, how
/// to use it without reading it, and how to ask the user for a missing one. Written for each tool that
/// reads global skills (Claude Code, Codex, Antigravity CLI), with a note on that tool's sandbox.
enum VaultSkill {
    enum Tool: CaseIterable {
        case claude, codex, antigravity

        var dir: URL {
            let home = FileManager.default.homeDirectoryForCurrentUser
            switch self {
            case .claude: return home.appendingPathComponent(".claude/skills/notchling-vault")
            case .codex: return home.appendingPathComponent(".codex/skills/notchling-vault")
            case .antigravity: return home.appendingPathComponent(".gemini/config/skills/notchling-vault")
            }
        }

        /// The tool's own folder must exist; a skill isn't a reason to create `~/.codex` for someone without Codex.
        var installed: Bool {
            FileManager.default.fileExists(atPath: dir.deletingLastPathComponent().deletingLastPathComponent().path)
        }

        /// What the agent must know about running Notchling from inside this tool's sandbox.
        var sandboxNote: String {
            switch self {
            case .claude:
                return ""
            case .codex:
                return """

                ## In Codex's sandbox

                Notchling's command talks to the Notchling app over a local socket, and the commands that use a secret usually need the network, so neither works inside Codex's workspace-write sandbox without network ("Notchling isn't running" means the sandbox blocked it). Run `--secrets`, `--secret` and `--secret-request` with escalated permissions (outside the sandbox) and give the user a one-line reason; if this session can't escalate, tell the user it needs network access.

                """
            case .antigravity:
                return """

                ## In Antigravity

                Run these through the terminal tool. If the session runs with `--sandbox` and the call says "Notchling isn't running", the sandbox blocked the local socket; ask the user to allow the command.

                """
            }
        }
    }

    static func update(_ entries: [VaultEntry]? = nil) {
        let entries = entries ?? SecretVault.all()
        let names = entries.map(label).sorted()
        let binary = Bundle.main.bundlePath + "/Contents/MacOS/Notchling"
        for tool in Tool.allCases where tool.installed {
            let file = tool.dir.appendingPathComponent("SKILL.md")
            let text = content(binary: binary, names: names, tool: tool)
            if (try? String(contentsOf: file, encoding: .utf8)) == text { continue }
            try? FileManager.default.createDirectory(at: tool.dir, withIntermediateDirectories: true)
            try? text.write(to: file, atomically: true, encoding: .utf8)
        }
    }

    /// `github (api.github.com)`: the name with its URL's host, so the description matches a task by site.
    static func label(_ e: VaultEntry) -> String {
        guard let host = e.host else { return e.name }
        return "\(e.name) (\(host))"
    }

    static func content(binary: String, names: [String], tool: Tool = .claude) -> String {
        let saved = names.isEmpty ? "none yet" : names.joined(separator: ", ")
        return """
        ---
        name: notchling-vault
        description: Use whenever a task needs a password, API token, username or other credential (logging in, calling an API, publishing a package, pushing to a registry, a .env value) — instead of asking the user to paste one into the chat. The user keeps credentials in Notchling's Secret Vault; saved now: \(saved).
        ---

        # Notchling Secret Vault

        The user keeps credentials in Notchling so they never go through the chat. Only the Notchling app can read them. You use one by name and never see its value. Never ask the user to type a secret into the chat.

        ## 1. Find the right one

        ```sh
        '\(binary)' --secrets '<URL, host or words from the task>'
        ```

        Prints the saved secrets that fit, best first (exit code 1 when none does). Each is a name, its kind (login or token) and the environment variables it sets, then its URL (also in `$<NAME>_URL`), the user's note, and whether the user approves each use. `--secrets` alone lists them all. Values are never printed.

        ## 2. Missing or wrong? Ask the user to add or update it

        ```sh
        '\(binary)' --secret-request <name> --kind token|login --url '<url>' --note '<what it is for>' --reason '<why you need it now>'
        ```

        Opens Notchling's form with those fields filled in; the user types the value there. For a name that exists (an expired token, a changed password) it opens that secret for editing. It waits for the user, so give it the longest timeout your shell tool allows (Claude Code's Bash: 600000 ms). It prints the saved secret (exit 0) or says the user cancelled (exit 1). Use names like `github`, `npm`, `aws-prod`.

        ## 3. Use it

        ```sh
        '\(binary)' --secret <name>[,<name>…] -- <command> [args…]
        ```

        The command runs with the secret's variables set. With a single argument after `--` it runs through `sh -c`, so single-quote it and let that shell expand the variables:

        ```sh
        '\(binary)' --secret github -- 'curl -sf -H "Authorization: Bearer $GITHUB_TOKEN" "$GITHUB_URL/user"'
        '\(binary)' --secret registry -- 'printf %s "$REGISTRY_PASSWORD" | docker login -u "$REGISTRY_USERNAME" --password-stdin "$REGISTRY_URL"'
        ```

        Its output comes back with every value replaced by `[secret:<name>]`; its exit code is the command's. When the secret says the user approves each use, Notchling shows them the command first and the call waits (exit 3 when they deny it), so give it a long timeout too.

        Rules:
        - Never echo, print, encode, log or write a value to a file, a commit, a URL you show, or your reply. Don't put a value on a command line where `ps` would see it when a tool takes it from the environment or stdin.
        - Don't try to read the vault's Keychain item or work around Notchling. A command whose purpose is to reveal a value is misuse even if its output is hidden.
        - Notchling must be running; if it isn't, tell the user.
        \(tool.sandboxNote)
        """
    }
}

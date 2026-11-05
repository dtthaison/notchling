import Foundation

/// Credentials the user saves for agents to use without typing them into a chat. The list (names, kind,
/// variable names, a note for agents) is `vault.json`; the values are in Keychain (service
/// `Notchling Vault`, base64 JSON, since `security -w` prints a value with newlines as hex). An agent runs
/// `Notchling --secret <name> -- <command>`: the command gets the values as environment variables and its
/// output comes back with every value replaced, so a value never lands in a transcript.
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
    var updated: Date = Date()

    var id: String { name.lowercased() }

    var variables: [String] { kind == .login ? [userVar, secretVar] : [secretVar] }

    /// `$<NAME>_URL`, set when the entry has a URL.
    var urlVar: String? { (url ?? "").isEmpty ? nil : "\(Self.prefix(for: name))_URL" }

    /// Every variable a command gets, URL included.
    var allVariables: [String] { variables + (urlVar.map { [$0] } ?? []) }

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

enum SecretVault {
    static let service = "Notchling Vault"
    static let indexURL = FileManager.default.homeDirectoryForCurrentUser
        .appendingPathComponent("Library/Application Support/Notchling/vault.json")

    static func all() -> [VaultEntry] {
        guard let data = try? Data(contentsOf: indexURL) else { return [] }
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601
        return ((try? decoder.decode([VaultEntry].self, from: data)) ?? []).sorted { $0.id < $1.id }
    }

    static func entry(_ name: String) -> VaultEntry? { all().first { $0.id == name.lowercased() } }

    static func value(for entry: VaultEntry) -> VaultValue? {
        guard let text = Keychain.read(service: service, account: entry.id),
              let data = Data(base64Encoded: text) else { return nil }
        return try? JSONDecoder().decode(VaultValue.self, from: data)
    }

    /// Saves an entry; a nil value keeps the stored one (editing the note or variables only). Renaming
    /// moves the stored value to the new name.
    static func save(_ entry: VaultEntry, value: VaultValue?, replacing old: VaultEntry? = nil) throws {
        guard VaultEntry.validName(entry.name) else {
            throw AccountError("Use letters, digits, - _ or . for the name, starting with a letter.")
        }
        for v in entry.variables where !VaultEntry.validVar(v) {
            throw AccountError("\"\(v)\" isn't a valid variable name.")
        }
        if Set(entry.allVariables).count != entry.allVariables.count {
            throw AccountError("Each value needs its own variable (\(entry.urlVar ?? "none") is the URL's).")
        }
        var list = all()
        if list.contains(where: { $0.id == entry.id && $0.id != old?.id }) {
            throw AccountError("There's already a secret named \(entry.name).")
        }
        let taken = Set(list.filter { $0.id != old?.id }.flatMap(\.allVariables))
        if let clash = entry.allVariables.first(where: taken.contains) {
            throw AccountError("Another secret already uses \(clash).")
        }
        var stored = value
        if stored == nil, let old { stored = Self.value(for: old) }
        guard let stored, !stored.secret.isEmpty else {
            throw AccountError(entry.kind == .login ? "Enter the password." : "Enter the token.")
        }
        let blob = try JSONEncoder().encode(stored).base64EncodedString()
        guard Keychain.write(service: service, account: entry.id, value: blob) else {
            throw AccountError("Couldn't save it to Keychain.")
        }
        if let old, old.id != entry.id { Keychain.delete(service: service, account: old.id) }
        list.removeAll { $0.id == entry.id || $0.id == old?.id }
        var saved = entry
        saved.updated = Date()
        list.append(saved)
        try write(list)
    }

    static func remove(_ entry: VaultEntry) throws {
        Keychain.delete(service: service, account: entry.id)
        try write(all().filter { $0.id != entry.id })
    }

    private static func write(_ list: [VaultEntry]) throws {
        let encoder = JSONEncoder()
        encoder.dateEncodingStrategy = .iso8601
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try FileManager.default.createDirectory(at: indexURL.deletingLastPathComponent(), withIntermediateDirectories: true)
        try encoder.encode(list.sorted { $0.id < $1.id }).write(to: indexURL, options: .atomic)
        try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: indexURL.path)
        VaultSkill.update(list)
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
}

// MARK: - Command line

extension SecretVault {
    /// `--secrets`: the saved names, variables and notes. Never values.
    static func runList() -> Never {
        let list = all()
        guard !list.isEmpty else {
            print("No secrets saved. The user adds them in Notchling: Chat > key button at the bottom of the sidebar.")
            exit(1)
        }
        for e in list {
            print("\(e.name)  (\(e.kind.rawValue))  \(e.allVariables.map { "$" + $0 }.joined(separator: " "))")
            if let url = e.url, !url.isEmpty { print("    \(url)") }
            if !e.note.isEmpty { print("    \(e.note)") }
        }
        exit(0)
    }

    /// `--secret a,b -- command…`: runs the command with those secrets in its environment and prints its
    /// output with the values hidden. One argument after `--` runs through `/bin/sh -c`.
    static func runCLI(_ args: [String]) -> Never {
        guard let i = args.firstIndex(of: "--secret"), i + 1 < args.count else { usage() }
        let names = args[i + 1].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        guard !names.isEmpty, let dash = args[(i + 1)...].firstIndex(of: "--"), dash + 1 < args.count else { usage() }
        let command = Array(args[(dash + 1)...])
        let list = all()
        var picked: [(VaultEntry, VaultValue)] = []
        for name in names {
            guard let entry = list.first(where: { $0.id == name.lowercased() }) else {
                fail("No secret named \(name). Saved: \(list.isEmpty ? "none" : list.map(\.name).joined(separator: ", ")).")
            }
            guard let value = value(for: entry) else { fail("\(entry.name) has no value in Keychain. Ask the user to save it again in Notchling.") }
            picked.append((entry, value))
        }
        let (env, hide) = environment(for: picked)
        let status = SecretRunner.run(command, extra: env, hide: hide)
        exit(status)
    }

    private static func usage() -> Never {
        fail("Usage: Notchling --secret <name>[,<name>…] -- <command> [args…]   (one argument runs through sh -c)\n       Notchling --secrets")
    }

    private static func fail(_ message: String) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(2)
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

/// Replaces secret strings in a byte stream. It holds back the last (longest secret − 1) bytes of each
/// chunk, so a value split across two reads is still caught. Strings shorter than 4 bytes aren't hidden:
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

    /// The longest tail of `data` (shorter than `hold + 1`) that could be the start of a secret; only that
    /// waits for the next read, so ordinary output flows through at once.
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

/// `~/.claude/skills/notchling-vault`: tells agents the vault exists, which names it holds, and how to use
/// one without reading it. Present only while the vault has entries.
enum VaultSkill {
    static let dir = FileManager.default.homeDirectoryForCurrentUser.appendingPathComponent(".claude/skills/notchling-vault")
    static let file = dir.appendingPathComponent("SKILL.md")

    static func update(_ entries: [VaultEntry]? = nil) {
        let entries = entries ?? SecretVault.all()
        guard !entries.isEmpty else {
            if (try? String(contentsOf: file, encoding: .utf8))?.contains("name: notchling-vault") == true {
                try? FileManager.default.removeItem(at: dir)
            }
            return
        }
        let text = content(binary: Bundle.main.bundlePath + "/Contents/MacOS/Notchling", names: entries.map(label).sorted())
        if (try? String(contentsOf: file, encoding: .utf8)) == text { return }
        try? FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try? text.write(to: file, atomically: true, encoding: .utf8)
    }

    /// `github (api.github.com)`: the name with its URL's host, so the description matches a task by site.
    static func label(_ e: VaultEntry) -> String {
        guard let url = e.url, !url.isEmpty else { return e.name }
        let host = URL(string: url.contains("://") ? url : "https://" + url)?.host ?? url
        return "\(e.name) (\(host))"
    }

    static func content(binary: String, names: [String]) -> String {
        """
        ---
        name: notchling-vault
        description: Use whenever a task needs a password, API token, username or other credential (logging in, calling an API, publishing a package, pushing to a registry) — before asking the user to paste one. The user keeps credentials in Notchling's Secret Vault; saved now: \(names.joined(separator: ", ")).
        ---

        # Notchling Secret Vault

        The user saves credentials in Notchling so they never go through the chat. Use them by name; never ask the user to paste a secret that the vault already has, and never try to read a value.

        ## See what's saved

        ```sh
        '\(binary)' --secrets
        ```

        Each line is a name, its kind (login or token) and the environment variables it sets, then the URL it's for (also in `$<NAME>_URL`) and the user's note. Pick the secret whose URL matches the site or API the task talks to. Values are never printed.

        ## Use one

        ```sh
        '\(binary)' --secret <name>[,<name>…] -- <command> [args…]
        ```

        The command runs with the secret's variables set. With a single argument after `--` it runs through `sh -c`, so single-quote it and let that shell expand the variables:

        ```sh
        '\(binary)' --secret github -- 'curl -sf -H "Authorization: Bearer $GITHUB_TOKEN" "$GITHUB_URL/user"'
        '\(binary)' --secret registry -- 'printf %s "$REGISTRY_PASSWORD" | docker login -u "$REGISTRY_USERNAME" --password-stdin "$REGISTRY_URL"'
        ```

        Its output comes back with every saved value replaced by `[secret:<name>]`, and its exit code is the command's.

        Rules:
        - Never echo, print, log or write a value to a file, a commit, a URL you show, or your reply. Don't pass a value on a command line where `ps` would see it when a tool takes it from the environment or stdin.
        - Don't read the vault's Keychain items (`security find-generic-password -s "Notchling Vault"`) or `vault.json` yourself.
        - If the credential the task needs isn't saved, tell the user its name and kind and ask them to add it in Notchling (Chat, the key button at the bottom of the sidebar), then continue.

        """
    }
}

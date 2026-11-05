import Foundation

/// `Notchling --secrets [query]`, `--secret a,b -- command…` and `--secret-request name [--kind …]`.
/// Run by agents. None of them touches Keychain: values come from the running app (`VaultClient`).
enum VaultCLI {
    /// `--secrets [URL | host | words]`: the saved secrets that fit, best first. Never values.
    static func list(_ args: [String]) -> Never {
        let all = SecretVault.all()
        let query = args.dropFirst(2).joined(separator: " ")
        guard !all.isEmpty else {
            print("No secrets saved yet. Ask the user for one with --secret-request <name> --kind token|login --url <url> --note <what for> --reason <why>.")
            exit(1)
        }
        let shown = SecretVault.find(query, in: all)
        guard !shown.isEmpty else {
            print("Nothing saved fits \"\(query)\". Saved: \(all.map(VaultSkill.label).joined(separator: ", ")).")
            print("If none of them is the one, ask the user with --secret-request.")
            exit(1)
        }
        print(shown.map(SecretVault.describe).joined(separator: "\n"))
        exit(0)
    }

    /// `--secret a,b -- command…`: runs the command with those secrets in its environment and prints its
    /// output with the values hidden. One argument after `--` runs through `/bin/sh -c`.
    static func use(_ args: [String]) -> Never {
        guard args.count > 2 else { usage() }
        let names = args[2].split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
        guard !names.isEmpty, let dash = args[2...].firstIndex(of: "--"), dash + 1 < args.count else { usage() }
        let command = Array(args[(dash + 1)...])
        let shown = command.count == 1 ? command[0] : command.map(quoted).joined(separator: " ")
        let reply = VaultClient.send(VaultRequest(op: .use, names: names, command: shown,
                                                  cwd: FileManager.default.currentDirectoryPath))
        guard reply.ok, let env = reply.env else {
            fail(reply.error ?? "The vault didn't answer.", code: reply.denied == true ? 3 : 2)
        }
        exit(SecretRunner.run(command, extra: env, hide: reply.hide ?? [:]))
    }

    /// `--secret-request name [--kind token|login] [--url U] [--note N] [--reason R]`: opens the form for
    /// the user, filled in, and waits. Prints the saved secret, or exits 1 when the user cancels.
    static func request(_ args: [String]) -> Never {
        guard args.count > 2, !args[2].hasPrefix("--") else {
            fail("Usage: Notchling --secret-request <name> [--kind token|login] [--url <url>] [--note <what for>] [--reason <why now>]")
        }
        func option(_ flag: String) -> String? {
            guard let i = args.firstIndex(of: flag), i + 1 < args.count else { return nil }
            return args[i + 1]
        }
        let kind = option("--kind")
        if let kind, VaultEntry.Kind(rawValue: kind) == nil { fail("--kind is token or login.") }
        let reply = VaultClient.send(VaultRequest(op: .request, name: args[2], kind: kind, url: option("--url"),
                                                  note: option("--note"), reason: option("--reason")))
        guard reply.ok, let entry = reply.entry else {
            fail(reply.error ?? "The vault didn't answer.", code: reply.denied == true ? 1 : 2)
        }
        print("Saved.")
        print(SecretVault.describe(entry))
        print("Use it: Notchling --secret \(entry.name) -- '<command using \(entry.allVariables.map { "$" + $0 }.joined(separator: ", "))>'")
        exit(0)
    }

    private static func quoted(_ arg: String) -> String {
        arg.range(of: #"^[A-Za-z0-9_./:=@%+-]+$"#, options: .regularExpression) != nil
            ? arg : "'" + arg.replacingOccurrences(of: "'", with: #"'\''"#) + "'"
    }

    private static func usage() -> Never {
        fail("""
        Usage: Notchling --secret <name>[,<name>…] -- <command> [args…]   (one argument runs through sh -c)
               Notchling --secrets [url | host | words]
               Notchling --secret-request <name> [--kind token|login] [--url <url>] [--note <what for>] [--reason <why now>]
        """)
    }

    private static func fail(_ message: String, code: Int32 = 2) -> Never {
        FileHandle.standardError.write(Data((message + "\n").utf8))
        exit(code)
    }
}

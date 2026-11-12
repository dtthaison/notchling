import Foundation

enum MachineOS: String, Codable { case macOS = "macos", linux }
enum AdminAccess: String, Codable { case sudo, rootSSH = "root-ssh" }
enum DockerState: String, Codable { case none, stopped, rootful, rootless }

/// What the Ability checks found on a machine. Every field may be missing.
struct MachineAbilities: Codable, Equatable {
    var arch: String?
    var osVersion: String?
    var home: String?
    var xcode: String?
    var iosRuntimes: [String] = []
    var simulators: [String] = []
    var docker: DockerState?
    var freeDiskGB: Int?
}

extension MachineAbilities {
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        arch = try c.decodeIfPresent(String.self, forKey: .arch)
        osVersion = try c.decodeIfPresent(String.self, forKey: .osVersion)
        home = try c.decodeIfPresent(String.self, forKey: .home)
        xcode = try c.decodeIfPresent(String.self, forKey: .xcode)
        iosRuntimes = try c.decodeIfPresent([String].self, forKey: .iosRuntimes) ?? []
        simulators = try c.decodeIfPresent([String].self, forKey: .simulators) ?? []
        docker = try c.decodeIfPresent(DockerState.self, forKey: .docker)
        freeDiskGB = try c.decodeIfPresent(Int.self, forKey: .freeDiskGB)
    }
}

/// A repository a worker machine serves: where it lives on GitHub, where the MacBook keeps its checkout,
/// and the fast checks to run there after a merge.
struct MachineRepo: Codable, Equatable {
    /// `owner/name`
    var github: String
    /// The MacBook checkout.
    var localPath: String
    var checkCommand: String
    /// The checkout on the machine; nil means `~/src/<name>`.
    var machinePath: String? = nil
    /// A path inside the checkout, e.g. `.claude/skills/verify-lingeva/SKILL.md`.
    var verifySkill: String? = nil
}

enum JobKind: String, Codable { case sdWorker = "sd-worker" }

/// The settings of a machine's sd worker job.
struct SDWorkerSettings: Codable, Equatable {
    /// Absolute on the machine; `python3` until Base tools finds it.
    var python: String
    /// Absolute path of `sd.py` on the machine; Plugins installed sets it.
    var sdPath: String
    var repos: [MachineRepo]
    /// The kinds this machine runs: `ios`, `backend`.
    var kinds: [String]
    var healthURL: String? = nil
    /// `YYYY-MM-DD`, from Token scope.
    var tokenExpires: String? = nil
    /// The vault name of the issues token: `<machine>-github-issues`.
    var issuesSecret: String
    /// The user chose one issues token for all repositories: Token scope then accepts a token that reaches other
    /// private repos. nil or false is the strict check. Written to machines.json only when true.
    var tokenAllRepos: Bool? = nil
}

extension SDWorkerSettings {
    /// Every key may be missing. A missing `issuesSecret` decodes as "" and `Machine` names it after itself.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        python = try c.decodeIfPresent(String.self, forKey: .python) ?? "python3"
        sdPath = try c.decodeIfPresent(String.self, forKey: .sdPath) ?? ""
        repos = try c.decodeIfPresent([MachineRepo].self, forKey: .repos) ?? []
        kinds = try c.decodeIfPresent([String].self, forKey: .kinds) ?? []
        healthURL = try c.decodeIfPresent(String.self, forKey: .healthURL)
        tokenExpires = try c.decodeIfPresent(String.self, forKey: .tokenExpires)
        issuesSecret = try c.decodeIfPresent(String.self, forKey: .issuesSecret) ?? ""
        tokenAllRepos = try c.decodeIfPresent(Bool.self, forKey: .tokenAllRepos)
    }
}

/// One job a machine does. One job per kind per machine.
struct Job: Codable, Equatable {
    var kind: JobKind
    var sdWorker: SDWorkerSettings?
}

/// A machine Notchling sets up and watches over Tailscale SSH. It does jobs; the sd worker is the first.
struct Machine: Codable, Equatable {
    /// `m1`
    var name: String
    /// Tailscale name.
    var host: String
    /// `agent` on the VPS.
    var user: String
    var os: MachineOS? = nil
    /// A FileVault machine waits for an unlock after a restart.
    var fileVault: Bool = false
    var admin: AdminAccess = .sudo
    /// The vault name of the Claude token the machine runs with.
    var claudeSecret: String = "claude-worker"
    var abilities: MachineAbilities? = nil
    var jobs: [Job] = []

    private enum CodingKeys: String, CodingKey {
        case name, host, user, os, fileVault, admin, claudeSecret, abilities, jobs
    }

    init(name: String, host: String, user: String, os: MachineOS? = nil, fileVault: Bool = false,
         admin: AdminAccess = .sudo, claudeSecret: String = "claude-worker",
         abilities: MachineAbilities? = nil, jobs: [Job] = []) {
        self.name = name
        self.host = host
        self.user = user
        self.os = os
        self.fileVault = fileVault
        self.admin = admin
        self.claudeSecret = claudeSecret
        self.abilities = abilities
        self.jobs = jobs
    }

    /// Plan C's initializer, kept for its callers and tests: one sd worker job, no kinds.
    init(name: String, host: String, user: String, python: String = "python3", sdPath: String,
         fileVault: Bool, repos: [MachineRepo]) {
        self.init(name: name, host: host, user: user, fileVault: fileVault, jobs: [
            Job(kind: .sdWorker, sdWorker: SDWorkerSettings(
                python: python, sdPath: sdPath, repos: repos, kinds: [],
                issuesSecret: "\(name)-github-issues")),
        ])
    }

    /// Only `name`, `host` and `user` are required; a hand-written file may leave the rest out.
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        host = try c.decode(String.self, forKey: .host)
        user = try c.decode(String.self, forKey: .user)
        os = try c.decodeIfPresent(MachineOS.self, forKey: .os)
        fileVault = try c.decodeIfPresent(Bool.self, forKey: .fileVault) ?? false
        // On Linux the worker account can't be the admin (`SSH.adminRefusal`), so a file that says nothing gets root
        // over ssh there; what a file says is kept.
        admin = try c.decodeIfPresent(AdminAccess.self, forKey: .admin) ?? (os == .linux ? .rootSSH : .sudo)
        claudeSecret = try c.decodeIfPresent(String.self, forKey: .claudeSecret) ?? "claude-worker"
        abilities = try c.decodeIfPresent(MachineAbilities.self, forKey: .abilities)
        jobs = try c.decodeIfPresent([Job].self, forKey: .jobs) ?? []
        for i in jobs.indices where jobs[i].sdWorker?.issuesSecret == "" {
            jobs[i].sdWorker?.issuesSecret = "\(name)-github-issues"
        }
    }

    /// The sd worker job's settings. Setting adds the job or replaces its settings; nil removes it.
    var sdWorker: SDWorkerSettings? {
        get { jobs.first { $0.kind == .sdWorker }?.sdWorker }
        set {
            let index = jobs.firstIndex { $0.kind == .sdWorker }
            switch (index, newValue) {
            case let (i?, value?): jobs[i].sdWorker = value
            case let (i?, nil): jobs.remove(at: i)
            case let (nil, value?): jobs.append(Job(kind: .sdWorker, sdWorker: value))
            case (nil, nil): break
            }
        }
    }
    var python: String { sdWorker?.python ?? "python3" }
    var sdPath: String { sdWorker?.sdPath ?? "" }
    var repos: [MachineRepo] { sdWorker?.repos ?? [] }
}

enum RegistryState: Equatable {
    /// No `machines.json`.
    case missing
    /// The machines; `migrated` is true on the one load that turned a v1 file into v2.
    case loaded([Machine], migrated: Bool)
    /// The file exists but can't be read or decoded. It is never overwritten.
    case unreadable(String)
}

/// Plan C's machine: the sd worker's settings sit inline.
private struct MachineV1: Decodable {
    var name: String
    var host: String
    var user: String
    var python: String
    var sdPath: String
    var fileVault: Bool
    var repos: [MachineRepo]

    private enum CodingKeys: String, CodingKey { case name, host, user, python, sdPath, fileVault, repos }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        host = try c.decode(String.self, forKey: .host)
        user = try c.decode(String.self, forKey: .user)
        python = try c.decodeIfPresent(String.self, forKey: .python) ?? "python3"
        sdPath = try c.decode(String.self, forKey: .sdPath)
        fileVault = try c.decode(Bool.self, forKey: .fileVault)
        repos = try c.decode([MachineRepo].self, forKey: .repos)
    }

    var machine: Machine {
        Machine(name: name, host: host, user: user, python: python, sdPath: sdPath, fileVault: fileVault, repos: repos)
    }
}

/// The machines on disk (`machines.json` in Application Support/Notchling).
///
/// v2 is `{"version": 2, "machines": [...]}`, pretty-printed with sorted keys. Each machine needs `name`,
/// `host` and `user`; everything else takes its default. Example:
///
///     {"version": 2, "machines": [
///       {"name": "vps-sing-1", "host": "vps-sing-1", "user": "agent", "os": "linux", "admin": "sudo",
///        "jobs": [{"kind": "sd-worker",
///                  "sdWorker": {"python": "python3", "sdPath": "/home/agent/sd/sd.py",
///                               "kinds": ["backend"], "issuesSecret": "vps-sing-1-github-issues",
///                               "repos": [{"github": "dtthaison/lingeva-mono",
///                                          "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
///                                          "checkCommand": "just check-affected"}]}}]}
///     ]}
///
/// v1 (plan C) was the bare array of machines with `python`, `sdPath`, `fileVault` and `repos` inline. The
/// first `load` of a v1 file copies it byte for byte to `machines.json.v1-backup` (never over an existing
/// backup) and rewrites it as v2. If the backup can't be written the file is left as it was. A file that
/// exists but can't be read, isn't JSON, has another version, or has an unknown job kind is `.unreadable`
/// and is never written over.
enum MachineRegistry {
    static let fileURL = HookBridge.dir.appendingPathComponent("machines.json")

    private struct FileV1: Decodable { var machines: [MachineV1] }
    private struct FileV2: Codable { var version: Int; var machines: [Machine] }
    private struct Header: Decodable { var version: Int }

    private enum Raw {
        case missing
        case unreadable(String)
        case v1([Machine], original: Data)
        case v2([Machine])
    }

    static func backupURL(for url: URL) -> URL {
        url.deletingLastPathComponent().appendingPathComponent(url.lastPathComponent + ".v1-backup")
    }

    private final class BackupBook: @unchecked Sendable {
        private let lock = NSLock()
        private var byFile: [String: URL] = [:]
        func set(_ backup: URL, for url: URL) { lock.withLock { byFile[url.path] = backup } }
        func get(for url: URL) -> URL? { lock.withLock { byFile[url.path] } }
    }
    private static let book = BackupBook()

    /// The backup the last migration of the file at `url` wrote in this run (it may carry a stamp when the usual
    /// name was taken), or nil when this run did not migrate it.
    static func lastBackup(for url: URL = fileURL) -> URL? { book.get(for: url) }

    /// `url` is injectable for tests. Migrates a v1 file once.
    static func load(at url: URL = fileURL) -> RegistryState {
        switch read(url) {
        case .missing: return .missing
        case .unreadable(let reason): return .unreadable(reason)
        case .v2(let machines): return .loaded(machines, migrated: false)
        case .v1(let machines, let original):
            guard let backup = writeBackup(original, for: url) else { return .loaded(machines, migrated: false) }
            if write(machines, to: url) {
                book.set(backup, for: url)
                return .loaded(machines, migrated: true)
            }
            try? FileManager.default.removeItem(at: backup)    // the file is as it was; don't leave a stray copy
            return .loaded(machines, migrated: false)
        }
    }

    /// The machines as `machines.json` says, for a reader that must not change anything: a v1 file is decoded
    /// as it is (no backup, no rewrite), and `[]` stands for a missing or unreadable file.
    static func peek(at url: URL = fileURL) -> [Machine] {
        switch read(url) {
        case .missing, .unreadable: return []
        case .v1(let machines, _), .v2(let machines): return machines
        }
    }

    /// `[]` for a missing and an unreadable file.
    static func all(at url: URL = fileURL) -> [Machine] {
        if case .loaded(let machines, _) = load(at: url) { return machines }
        return []
    }

    /// Writes v2 only when the file is missing or already a readable v2 file; false means nothing was written.
    @discardableResult
    static func save(_ machines: [Machine], to url: URL = fileURL) -> Bool {
        switch read(url) {
        case .missing, .v2: return write(machines, to: url)
        case .v1, .unreadable: return false
        }
    }

    static func machine(named name: String, at url: URL = fileURL) -> Machine? {
        all(at: url).first { $0.name == name }
    }

    // MARK: private

    private static func read(_ url: URL) -> Raw {
        let data: Data
        do {
            data = try Data(contentsOf: url)
        } catch {
            let e = error as NSError
            if e.domain == NSCocoaErrorDomain, e.code == NSFileReadNoSuchFileError { return .missing }
            return .unreadable(e.localizedDescription)
        }
        let decoder = JSONDecoder()
        if let v1 = try? decoder.decode([MachineV1].self, from: data) {
            return .v1(v1.map(\.machine), original: data)
        }
        guard let header = try? decoder.decode(Header.self, from: data) else {
            return .unreadable("not a machines.json file")
        }
        do {
            switch header.version {
            case 1: return .v1(try decoder.decode(FileV1.self, from: data).machines.map(\.machine), original: data)
            case 2: return .v2(try decoder.decode(FileV2.self, from: data).machines)
            default: return .unreadable("version \(header.version) is not supported")
            }
        } catch {
            return .unreadable("\(error)")
        }
    }

    private static func write(_ machines: [Machine], to url: URL) -> Bool {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys, .withoutEscapingSlashes]
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            try encoder.encode(FileV2(version: 2, machines: machines)).write(to: url, options: .atomic)
            return true
        } catch {
            return false
        }
    }

    /// Copies the original bytes to `<file>.v1-backup`, or to `<file>.v1-backup-<unix seconds>` when that
    /// exists. An existing backup is never overwritten. Returns the backup's URL, or nil if none was written.
    private static func writeBackup(_ original: Data, for url: URL) -> URL? {
        let fm = FileManager.default
        let base = backupURL(for: url)
        let stamp = Int(Date().timeIntervalSince1970)
        var candidates = [base, URL(fileURLWithPath: base.path + "-\(stamp)")]
        candidates += (2...50).map { URL(fileURLWithPath: base.path + "-\(stamp)-\($0)") }
        for candidate in candidates where !fm.fileExists(atPath: candidate.path) {
            do {
                try original.write(to: candidate, options: .withoutOverwriting)
            } catch {
                return nil
            }
            guard (try? Data(contentsOf: candidate)) == original else {
                try? fm.removeItem(at: candidate)
                return nil
            }
            return candidate
        }
        return nil
    }
}

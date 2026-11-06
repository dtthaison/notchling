import Foundation

/// A repository a worker machine serves: where it lives on GitHub, where the MacBook keeps its checkout,
/// and the fast checks to run there after a merge.
struct MachineRepo: Codable, Equatable {
    /// `owner/name`
    var github: String
    /// The MacBook checkout.
    var localPath: String
    var checkCommand: String
}

/// A worker machine Notchling watches over Tailscale SSH.
struct Machine: Codable, Equatable {
    /// `m1`
    var name: String
    /// Tailscale name.
    var host: String
    /// `agent` on the VPS.
    var user: String
    var python: String = "python3"
    /// Path to `sd.py` on the machine.
    var sdPath: String
    /// A FileVault machine waits for an unlock after a restart.
    var fileVault: Bool
    var repos: [MachineRepo]

    init(name: String, host: String, user: String, python: String = "python3", sdPath: String,
         fileVault: Bool, repos: [MachineRepo]) {
        self.name = name
        self.host = host
        self.user = user
        self.python = python
        self.sdPath = sdPath
        self.fileVault = fileVault
        self.repos = repos
    }

    /// A hand-written `machines.json` may leave `python` out.
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
}

/// The machines on disk (`machines.json` in Application Support/Notchling). A missing or unreadable file
/// is an empty list. Example:
///
///     [
///       {"name": "m1", "host": "m1", "user": "sondoan", "python": "python3",
///        "sdPath": "/Users/sondoan/sd/sd.py", "fileVault": true,
///        "repos": [{"github": "dtthaison/lingeva-mono",
///                   "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
///                   "checkCommand": "just check-affected"}]},
///       {"name": "vps-sing-1", "host": "vps-sing-1", "user": "agent", "python": "python3",
///        "sdPath": "/home/agent/sd/sd.py", "fileVault": false,
///        "repos": [{"github": "dtthaison/lingeva-mono",
///                   "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
///                   "checkCommand": "just check-affected"}]}
///     ]
enum MachineRegistry {
    static let fileURL = HookBridge.dir.appendingPathComponent("machines.json")

    /// `url` is injectable for tests.
    static func all(at url: URL = fileURL) -> [Machine] {
        guard let data = try? Data(contentsOf: url) else { return [] }
        return (try? JSONDecoder().decode([Machine].self, from: data)) ?? []
    }

    static func save(_ machines: [Machine], to url: URL = fileURL) {
        try? FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        if let data = try? JSONEncoder().encode(machines) { try? data.write(to: url, options: .atomic) }
    }

    static func machine(named name: String, at url: URL = fileURL) -> Machine? {
        all(at: url).first { $0.name == name }
    }
}

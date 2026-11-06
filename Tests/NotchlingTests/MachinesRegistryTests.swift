import XCTest
@testable import Notchling

final class MachinesRegistryTests: XCTestCase {
    private func tempURL() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("machines-\(UUID().uuidString)")
            .appendingPathComponent("machines.json")
    }

    private func sample() -> [Machine] {
        let repo = MachineRepo(github: "dtthaison/lingeva-mono",
                               localPath: "/Users/sondoan/go/src/lingeva/lingeva-mono",
                               checkCommand: "just check-affected")
        return [
            Machine(name: "m1", host: "m1", user: "sondoan", sdPath: "/opt/sd/sd.py", fileVault: true, repos: [repo]),
            Machine(name: "vps-sing-1", host: "vps-sing-1", user: "agent", python: "python3.12",
                    sdPath: "/home/agent/sd/sd.py", fileVault: false, repos: [repo]),
        ]
    }

    func testRegistryRoundTrip() throws {
        let url = tempURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        MachineRegistry.save(sample(), to: url)
        XCTAssertEqual(MachineRegistry.all(at: url), sample())
        XCTAssertEqual(MachineRegistry.machine(named: "vps-sing-1", at: url)?.user, "agent")
        XCTAssertEqual(MachineRegistry.machine(named: "m1", at: url)?.python, "python3")
        XCTAssertNil(MachineRegistry.machine(named: "nope", at: url))
    }

    func testMissingFileIsEmpty() {
        XCTAssertEqual(MachineRegistry.all(at: tempURL()), [])
    }

    func testBadJSONIsEmptyNotCrash() throws {
        let url = tempURL()
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        try Data("{ not json".utf8).write(to: url)
        XCTAssertEqual(MachineRegistry.all(at: url), [])
    }

    func testPythonDefaultsWhenKeyIsMissing() throws {
        let url = tempURL()
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let json = #"[{"name":"m1","host":"m1","user":"sondoan","sdPath":"/x/sd.py","fileVault":true,"repos":[]}]"#
        try Data(json.utf8).write(to: url)
        XCTAssertEqual(MachineRegistry.all(at: url).first?.python, "python3")
    }

    // MARK: machines.json v2 and the one-time migration

    /// Plan C's doc-comment example, as the v1 file a user may already have.
    private let v1Text = """
    [
      {"name": "m1", "host": "m1", "user": "sondoan", "python": "python3",
       "sdPath": "/Users/sondoan/sd/sd.py", "fileVault": true,
       "repos": [{"github": "dtthaison/lingeva-mono",
                  "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
                  "checkCommand": "just check-affected"}]},
      {"name": "vps-sing-1", "host": "vps-sing-1", "user": "agent", "python": "python3",
       "sdPath": "/home/agent/sd/sd.py", "fileVault": false,
       "repos": [{"github": "dtthaison/lingeva-mono",
                  "localPath": "/Users/sondoan/go/src/lingeva/lingeva-mono",
                  "checkCommand": "just check-affected"}]}
    ]
    """

    private func makeDir() throws -> URL {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("machines-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        return dir
    }

    func testV1MigratesEveryFieldAndKeepsAByteIdenticalBackup() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("machines.json")
        let original = Data(v1Text.utf8)
        try original.write(to: url)

        let state = MachineRegistry.load(at: url)
        guard case .loaded(let machines, let migrated) = state else { return XCTFail("\(state)") }
        XCTAssertTrue(migrated)
        XCTAssertEqual(machines.map(\.name), ["m1", "vps-sing-1"])
        let repo = MachineRepo(github: "dtthaison/lingeva-mono",
                               localPath: "/Users/sondoan/go/src/lingeva/lingeva-mono",
                               checkCommand: "just check-affected")
        let m1 = machines[0], vps = machines[1]
        XCTAssertEqual(m1.host, "m1"); XCTAssertEqual(m1.user, "sondoan")
        XCTAssertEqual(m1.python, "python3"); XCTAssertEqual(m1.sdPath, "/Users/sondoan/sd/sd.py")
        XCTAssertTrue(m1.fileVault); XCTAssertEqual(m1.repos, [repo])
        XCTAssertEqual(vps.host, "vps-sing-1"); XCTAssertEqual(vps.user, "agent")
        XCTAssertEqual(vps.python, "python3"); XCTAssertEqual(vps.sdPath, "/home/agent/sd/sd.py")
        XCTAssertFalse(vps.fileVault); XCTAssertEqual(vps.repos, [repo])
        for m in machines {
            XCTAssertEqual(m.jobs.count, 1)
            XCTAssertEqual(m.jobs.first?.kind, .sdWorker)
            XCTAssertEqual(m.sdWorker?.kinds, [])
            XCTAssertEqual(m.claudeSecret, "claude-worker")
            XCTAssertEqual(m.admin, .sudo)
        }
        XCTAssertEqual(m1.sdWorker?.issuesSecret, "m1-github-issues")
        XCTAssertEqual(vps.sdWorker?.issuesSecret, "vps-sing-1-github-issues")

        XCTAssertEqual(try Data(contentsOf: MachineRegistry.backupURL(for: url)), original)
        let text = try String(contentsOf: url, encoding: .utf8)
        XCTAssertTrue(text.contains("\"version\" : 2"), text)
        XCTAssertEqual(MachineRegistry.all(at: url), machines)
    }

    func testMigrationRunsOnce() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("machines.json")
        let original = Data(v1Text.utf8)
        try original.write(to: url)
        guard case .loaded(let first, true) = MachineRegistry.load(at: url) else { return XCTFail("first load") }
        let second = MachineRegistry.load(at: url)
        XCTAssertEqual(second, .loaded(first, migrated: false))
        XCTAssertEqual(try Data(contentsOf: MachineRegistry.backupURL(for: url)), original)
        let names = try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted()
        XCTAssertEqual(names, ["machines.json", "machines.json.v1-backup"])
    }

    func testASecondV1FileGetsItsOwnBackup() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("machines.json")
        let first = Data(v1Text.utf8)
        try first.write(to: url)
        _ = MachineRegistry.load(at: url)
        let second = Data(#"[{"name":"z","host":"z","user":"u","sdPath":"/z/sd.py","fileVault":false,"repos":[]}]"#.utf8)
        try second.write(to: url)

        guard case .loaded(let machines, true) = MachineRegistry.load(at: url) else { return XCTFail("second load") }
        XCTAssertEqual(machines.map(\.name), ["z"])
        XCTAssertEqual(try Data(contentsOf: MachineRegistry.backupURL(for: url)), first)
        let extra = try FileManager.default.contentsOfDirectory(atPath: dir.path)
            .filter { $0.hasPrefix("machines.json.v1-backup-") }
        XCTAssertEqual(extra.count, 1)
        let digits = extra[0].dropFirst("machines.json.v1-backup-".count)
        XCTAssertFalse(digits.isEmpty)
        XCTAssertTrue(digits.allSatisfy(\.isNumber), "\(extra[0])")
        XCTAssertEqual(try Data(contentsOf: dir.appendingPathComponent(extra[0])), second)
    }

    /// Review minor: the window's note names the backup `load` really wrote, not the usual name.
    func testLoadReportsTheBackupItWroteEvenWhenTheUsualNameWasTaken() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("machines.json")
        XCTAssertNil(MachineRegistry.lastBackup(for: url))
        try Data(v1Text.utf8).write(to: url)
        _ = MachineRegistry.load(at: url)
        XCTAssertEqual(MachineRegistry.lastBackup(for: url), MachineRegistry.backupURL(for: url))
        try Data(#"[{"name":"z","host":"z","user":"u","sdPath":"/z/sd.py","fileVault":false,"repos":[]}]"#.utf8).write(to: url)
        _ = MachineRegistry.load(at: url)
        let second = try XCTUnwrap(MachineRegistry.lastBackup(for: url))
        XCTAssertNotEqual(second, MachineRegistry.backupURL(for: url), "the usual name was taken")
        XCTAssertTrue(FileManager.default.fileExists(atPath: second.path))
        XCTAssertTrue(second.lastPathComponent.hasPrefix("machines.json.v1-backup-"))
    }

    func testABackupThatCannotBeWrittenStopsTheMigration() throws {
        let dir = try makeDir()
        let url = dir.appendingPathComponent("machines.json")
        let original = Data(v1Text.utf8)
        try original.write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o500], ofItemAtPath: dir.path)
        defer {
            try? FileManager.default.setAttributes([.posixPermissions: 0o700], ofItemAtPath: dir.path)
            try? FileManager.default.removeItem(at: dir)
        }
        let state = MachineRegistry.load(at: url)
        guard case .loaded(let machines, let migrated) = state else { return XCTFail("\(state)") }
        XCTAssertFalse(migrated)
        XCTAssertEqual(machines.map(\.name), ["m1", "vps-sing-1"])
        XCTAssertEqual(try Data(contentsOf: url), original)
        XCTAssertFalse(FileManager.default.fileExists(atPath: MachineRegistry.backupURL(for: url).path))
    }

    func testUnreadableFileIsNeverOverwritten() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }

        let bad = dir.appendingPathComponent("bad.json")
        let badBytes = Data("{ not json".utf8)
        try badBytes.write(to: bad)
        guard case .unreadable = MachineRegistry.load(at: bad) else { return XCTFail("bad json") }
        XCTAssertFalse(MachineRegistry.save(sample(), to: bad))
        XCTAssertEqual(try Data(contentsOf: bad), badBytes)

        let locked = dir.appendingPathComponent("locked.json")
        let lockedBytes = Data(v1Text.utf8)
        try lockedBytes.write(to: locked)
        try FileManager.default.setAttributes([.posixPermissions: 0o000], ofItemAtPath: locked.path)
        defer { try? FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: locked.path) }
        guard case .unreadable = MachineRegistry.load(at: locked) else { return XCTFail("0o000 file") }
        XCTAssertEqual(MachineRegistry.all(at: locked), [])
        XCTAssertFalse(MachineRegistry.save(sample(), to: locked))
        try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: locked.path)
        XCTAssertEqual(try Data(contentsOf: locked), lockedBytes)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted(), ["bad.json", "locked.json"])
    }

    func testUnknownVersionOrJobKindIsUnreadable() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let v3 = dir.appendingPathComponent("v3.json")
        let v3Bytes = Data(#"{"version":3,"machines":[]}"#.utf8)
        try v3Bytes.write(to: v3)
        let kind = dir.appendingPathComponent("kind.json")
        let kindBytes = Data(#"{"version":2,"machines":[{"name":"m","host":"m","user":"u","jobs":[{"kind":"triage-bot"}]}]}"#.utf8)
        try kindBytes.write(to: kind)
        for (url, bytes) in [(v3, v3Bytes), (kind, kindBytes)] {
            guard case .unreadable = MachineRegistry.load(at: url) else { return XCTFail("\(url.lastPathComponent)") }
            XCTAssertFalse(MachineRegistry.save(sample(), to: url))
            XCTAssertEqual(try Data(contentsOf: url), bytes)
        }
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: dir.path).sorted(), ["kind.json", "v3.json"])
    }

    /// M10 / ruling M12: a Linux machine's admin can't be the worker account, so a file that doesn't say defaults it to
    /// root over SSH. A Mac keeps sudo, and what a file says is kept.
    func testALinuxMachineWithoutAnAdminKeyDefaultsToRootOverSSH() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("machines.json")
        try Data(#"{"version":2,"machines":[{"name":"a","host":"a","user":"agent","os":"linux"},{"name":"b","host":"b","user":"u","os":"macos"},{"name":"c","host":"c","user":"u"},{"name":"d","host":"d","user":"u","os":"linux","admin":"sudo"}]}"#.utf8).write(to: url)
        let machines = MachineRegistry.all(at: url)
        XCTAssertEqual(machines.map(\.admin), [.rootSSH, .sudo, .sudo, .sudo])
    }

    func testV2FieldsMayBeMissingExceptNameHostUser() throws {
        let dir = try makeDir()
        defer { try? FileManager.default.removeItem(at: dir) }
        let url = dir.appendingPathComponent("machines.json")
        try Data(#"{"version":2,"machines":[{"name":"vps","host":"vps.tail","user":"agent"}]}"#.utf8).write(to: url)
        XCTAssertEqual(MachineRegistry.load(at: url),
                       .loaded([Machine(name: "vps", host: "vps.tail", user: "agent")], migrated: false))
        let m = MachineRegistry.machine(named: "vps", at: url)
        XCTAssertEqual(m?.python, "python3"); XCTAssertEqual(m?.sdPath, ""); XCTAssertEqual(m?.repos, [])
        XCTAssertNil(m?.sdWorker)
        XCTAssertEqual(m?.claudeSecret, "claude-worker"); XCTAssertEqual(m?.admin, .sudo)
        // A job with no settings in it gets the defaults, with the secret named after the machine.
        try Data(#"{"version":2,"machines":[{"name":"vps","host":"h","user":"u","jobs":[{"kind":"sd-worker","sdWorker":{}}]}]}"#.utf8).write(to: url)
        XCTAssertEqual(MachineRegistry.machine(named: "vps", at: url)?.sdWorker?.issuesSecret, "vps-github-issues")
        XCTAssertEqual(MachineRegistry.machine(named: "vps", at: url)?.sdWorker?.python, "python3")
    }

    func testSaveWritesWhenMissingAndRoundTripsV2() throws {
        let url = tempURL()
        defer { try? FileManager.default.removeItem(at: url.deletingLastPathComponent()) }
        let worker = SDWorkerSettings(
            python: "/usr/bin/python3.12", sdPath: "/home/agent/sd/sd.py",
            repos: [MachineRepo(github: "dtthaison/lingeva-mono", localPath: "/Users/me/lingeva",
                                checkCommand: "just check", machinePath: "~/src/lingeva",
                                verifySkill: ".claude/skills/verify-lingeva/SKILL.md")],
            kinds: ["backend"], healthURL: "http://127.0.0.1:8080/health", tokenExpires: "2027-02-01",
            issuesSecret: "vps-sing-1-github-issues")
        let abilities = MachineAbilities(arch: "x86_64", osVersion: "Ubuntu 24.04", home: "/home/agent",
                                         xcode: nil, iosRuntimes: [], simulators: [],
                                         docker: .rootless, freeDiskGB: 42)
        let machine = Machine(name: "vps-sing-1", host: "vps-sing-1", user: "agent", os: .linux,
                              fileVault: false, admin: .rootSSH, claudeSecret: "claude-worker",
                              abilities: abilities, jobs: [Job(kind: .sdWorker, sdWorker: worker)])
        XCTAssertTrue(MachineRegistry.save([machine], to: url))
        XCTAssertEqual(MachineRegistry.load(at: url), .loaded([machine], migrated: false))
        XCTAssertEqual(MachineRegistry.all(at: url), [machine])
        XCTAssertEqual(machine.python, "/usr/bin/python3.12")
        let text = try String(contentsOf: url, encoding: .utf8)
        XCTAssertTrue(text.contains("\"version\" : 2"))
        let a = try XCTUnwrap(text.range(of: "\"admin\""))
        let c = try XCTUnwrap(text.range(of: "\"claudeSecret\""))
        XCTAssertLessThan(a.lowerBound, c.lowerBound)
        XCTAssertTrue(text.contains("\"root-ssh\"") && text.contains("\"sd-worker\"") && text.contains("\"linux\""))
        // A saved v2 file is saved again without a backup.
        XCTAssertTrue(MachineRegistry.save([machine], to: url))
        XCTAssertFalse(FileManager.default.fileExists(atPath: MachineRegistry.backupURL(for: url).path))
    }

    func testTheSdWorkerSetterAddsReplacesAndRemovesTheJob() {
        var m = Machine(name: "m", host: "m", user: "u")
        XCTAssertNil(m.sdWorker)
        XCTAssertEqual(m.python, "python3"); XCTAssertEqual(m.sdPath, "")
        let s1 = SDWorkerSettings(python: "p1", sdPath: "/a/sd.py", repos: [], kinds: [], issuesSecret: "m-github-issues")
        m.sdWorker = s1
        XCTAssertEqual(m.jobs.count, 1); XCTAssertEqual(m.sdPath, "/a/sd.py")
        var s2 = s1
        s2.sdPath = "/b/sd.py"
        m.sdWorker = s2
        XCTAssertEqual(m.jobs.count, 1); XCTAssertEqual(m.sdPath, "/b/sd.py")
        m.sdWorker = nil
        XCTAssertTrue(m.jobs.isEmpty)
    }
}

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
}

import XCTest
@testable import Notchling

final class AgentMonitorExitedTests: XCTestCase {
    func testExitedSessionsSurviveARoundTripAsResumableRows() {
        let since = Date(timeIntervalSince1970: 1_800_000_000)
        let agents = [
            AgentMonitor.exitedAgent(id: "a1", name: "Sổ thu chi", project: "maneki", cwd: "/Users/x/maneki",
                                     detail: "done", since: since, configDir: "/Users/x/.claude-lane"),
            AgentMonitor.exitedAgent(id: "a2", name: "nháp", project: "x", cwd: "/Users/x", detail: nil, since: since,
                                     configDir: nil),
        ]

        let restored = AgentMonitor.decodeExited(AgentMonitor.encodeExited(agents))

        XCTAssertEqual(restored.map(\.id), ["a1", "a2"])
        XCTAssertEqual(restored[0].name, "Sổ thu chi")
        XCTAssertEqual(restored[0].configDir, "/Users/x/.claude-lane")
        XCTAssertEqual(restored[0].detail, "done")
        XCTAssertNil(restored[1].configDir)
        XCTAssertEqual(restored[1].since, since)
        for agent in restored {
            XCTAssertEqual(agent.group, .completed)
            XCTAssertEqual(agent.label, "Exited")
            guard case .resume(let id, let cwd) = agent.target else { return XCTFail("not resumable: \(agent.target)") }
            XCTAssertEqual(id, agent.id)
            XCTAssertEqual(cwd, agent.cwd)
        }
    }

    func testDecodeSkipsDamagedEntriesAndFillsNamesFromTheFolder() {
        let stored: [Any] = [
            "not a dictionary",
            ["name": "no id", "since": 1.0],
            ["id": "", "since": 1.0],
            ["id": "no-since"],
            ["id": "ok", "cwd": "/Users/x/batoan", "since": 2.0],
        ]

        let restored = AgentMonitor.decodeExited(stored)

        XCTAssertEqual(restored.map(\.id), ["ok"])
        XCTAssertEqual(restored[0].name, "batoan")
        XCTAssertEqual(restored[0].project, "batoan")
        XCTAssertTrue(AgentMonitor.decodeExited(nil).isEmpty)
    }

    func testANewMonitorStartsWithTheStoredRows() throws {
        let suite = "AgentMonitorExitedTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let agent = AgentMonitor.exitedAgent(id: "kept", name: "Aram", project: "x", cwd: "/Users/x", detail: nil,
                                             since: Date(), configDir: nil)
        defaults.set(AgentMonitor.encodeExited([agent]), forKey: AgentMonitor.exitedKey)

        let restored = AgentMonitor.decodeExited(defaults.array(forKey: AgentMonitor.exitedKey))

        XCTAssertEqual(restored.map(\.id), ["kept"])
    }
}

import XCTest
@testable import Notchling

/// Putting every session on one account: lane sessions come to the main login, the account's lane closes.
final class MoveAllPlanTests: XCTestCase {
    let laneA = Lane(name: "a", path: "/lanes/a", accountID: "acct-a", email: "a@x.com", folder: nil)
    let laneB = Lane(name: "b", path: "/lanes/b", accountID: "acct-b", email: "b@x.com", folder: nil)

    func agent(_ id: String, in dir: String?, target: AgentTarget, label: String = "Working",
               transcript: String? = "/t/x.jsonl") -> Agent {
        var a = Agent(id: id, name: id, project: "p", cwd: "/p", group: .working, label: label, detail: nil,
                      since: Date(), target: target)
        a.configDir = dir
        a.transcriptPath = transcript
        return a
    }

    func testAccountInALane() {
        let agents = [
            agent("main-job", in: nil, target: .job("m1")),
            agent("a-job", in: "/lanes/a", target: .job("a1")),
            agent("a-stopped", in: "/lanes/a", target: .job("a2"), label: "Stopped"),
            agent("a-term", in: "/lanes/a", target: .process(42)),
            agent("b-job", in: "/lanes/b", target: .job("b1")),
            agent("b-term", in: "/lanes/b", target: .process(43)),
        ]
        let plan = MoveAllPlan.make(agents: agents, lanes: [laneA, laneB], activeID: "main", target: "acct-a")
        XCTAssertEqual(plan.jobs.map(\.jobID), ["a1", "b1"])
        XCTAssertEqual(plan.closeLane, "a")
        XCTAssertEqual(plan.switchMain, "acct-a")
        XCTAssertEqual(plan.signedOut, ["a-term"])
    }

    func testMainAccountOnlyGathersLaneSessions() {
        let agents = [agent("b-job", in: "/lanes/b", target: .job("b1"))]
        let plan = MoveAllPlan.make(agents: agents, lanes: [laneA, laneB], activeID: "main", target: "main")
        XCTAssertEqual(plan.jobs.map(\.jobID), ["b1"])
        XCTAssertNil(plan.closeLane)
        XCTAssertNil(plan.switchMain)
    }

    func testNothingToDo() {
        let agents = [agent("main-job", in: nil, target: .job("m1"))]
        XCTAssertTrue(MoveAllPlan.make(agents: agents, lanes: [laneA], activeID: "main", target: nil).isEmpty)
        // A free account: only the main login switches.
        let free = MoveAllPlan.make(agents: agents, lanes: [laneA], activeID: "main", target: "acct-free")
        XCTAssertEqual(free.switchMain, "acct-free")
        XCTAssertNil(free.closeLane)
        XCTAssertTrue(free.jobs.isEmpty)
    }
}

import XCTest
@testable import ConsoleCore

/// A fixture session through VizEngine and the router's `viz.get`, the way the phone's Visualize tab asks for it.
final class VizBridgeTests: XCTestCase {
    func testVizGetServesFlowDiagramAndChartSpecsFromARealTranscript() async throws {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("vizb-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let file = directory.appendingPathComponent("t.jsonl")
        let mermaid = "```mermaid\\ngraph TD\\n  A[Phone] --> B[Mac]\\n  B --> C[Claude]\\n```"
        let lines = [
            #"{"type":"user","uuid":"u1","timestamp":"2026-11-01T10:00:00.000Z","message":{"role":"user","content":"add the login page"}}"#,
            #"{"type":"assistant","uuid":"a1","timestamp":"2026-11-01T10:00:01.000Z","message":{"id":"m1","role":"assistant","model":"claude-opus-5-5","usage":{"input_tokens":1200,"output_tokens":300,"cache_read_input_tokens":4000},"content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/p/lib/login.dart"}},{"type":"tool_use","id":"t2","name":"Edit","input":{"file_path":"/p/lib/login.dart"}}]}}"#,
            #"{"type":"assistant","uuid":"a2","timestamp":"2026-11-01T10:00:09.000Z","message":{"id":"m2","role":"assistant","model":"claude-opus-5-5","usage":{"input_tokens":2400,"output_tokens":500,"cache_read_input_tokens":6000},"content":[{"type":"text","text":"Plan:\n\#(mermaid)\nOpen http://localhost:5173/ to look."}]}}"#,
        ]
        try Data(lines.joined(separator: "\n").utf8).write(to: file)

        var session = makeSession("fx")
        session.transcriptURL = file
        let engine = VizEngine(now: { fixtureNow }, git: { _ in GitSnapshot(branch: "feat/login", diffStat: " lib/login.dart | 12 ++++++") })
        let bundle = engine.build(session: session)
        XCTAssertEqual(bundle.flow.kind, .flow)
        XCTAssertEqual(bundle.diagram.kind, .diagram)
        XCTAssertEqual(bundle.chart.kind, .chart)
        XCTAssertNil(bundle.diagram.error, "\(String(describing: bundle.diagram.error))")
        XCTAssertNil(bundle.diagram.empty)
        XCTAssertEqual(bundle.webURL?.absoluteString.hasPrefix("http://localhost:5173"), true)

        try Data("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT);\nCREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER REFERENCES users(id), total REAL);".utf8)
            .write(to: directory.appendingPathComponent("schema.sql"))
        let entity = engine.entitySpec(cwd: directory.path)
        XCTAssertEqual(entity.kind, .entity)
        XCTAssertEqual(entity.entity?.tableCount, 2)

        let rig = try makeBridgeRig()
        rig.backend.sessionList = [session]
        rig.backend.vizResult = BridgeViz(suggestedTab: "diagram", webURL: bundle.webURL?.absoluteString,
                                          specs: ["flow": bundle.flow, "diagram": bundle.diagram, "chart": bundle.chart, "entity": entity], webCandidates: ["http://localhost:5173"])
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"viz.get","sessionId":"fx","entity":true}"#, from: connection)
        let reply = try io.lastJSON()
        XCTAssertTrue(reply.isOK, "\(reply)")
        let data = reply.dataObject
        XCTAssertEqual(Set(data.keys), ["suggested", "webURL", "webCandidates", "flow", "diagram", "chart", "entity"])
        for (key, kind) in [("flow", "flow"), ("diagram", "diagram"), ("chart", "chart"), ("entity", "entity")] {
            let spec = try XCTUnwrap(data[key] as? [String: Any], key)
            XCTAssertEqual(spec["kind"] as? String, kind)
            XCTAssertNotNil(spec[kind], "the body the phone's viz.js reads: spec.\(kind)")
        }
        if let target = ProcessInfo.processInfo.environment["VIZ_DUMP_DIR"] {
            for key in ["flow", "diagram", "chart", "entity"] {
                let json = try JSONSerialization.data(withJSONObject: try XCTUnwrap(data[key]), options: [.sortedKeys])
                try json.write(to: URL(fileURLWithPath: target).appendingPathComponent("\(key).json"))
            }
        }
    }
}

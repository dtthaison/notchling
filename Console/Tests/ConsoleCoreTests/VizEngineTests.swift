import XCTest
@testable import ConsoleCore

final class VizEngineTests: XCTestCase {
    private func assistant(_ blocks: String, id: String, time: String, usage: String? = nil, model: String? = nil) -> Substring {
        let usageJSON = usage.map { #","usage":\#($0)"# } ?? ""
        let modelJSON = model.map { #","model":"\#($0)""# } ?? ""
        return Substring(#"{"type":"assistant","uuid":"u-\#(id)","timestamp":"\#(time)","message":{"id":"\#(id)","role":"assistant"\#(modelJSON)\#(usageJSON),"content":[\#(blocks)]}}"#)
    }

    private func user(_ text: String, time: String, sidechain: Bool = false) -> Substring {
        Substring(#"{"type":"user","uuid":"u-\#(text.hashValue)","isSidechain":\#(sidechain),"timestamp":"\#(time)","message":{"role":"user","content":"\#(text)"}}"#)
    }

    private func tool(_ name: String, _ input: String) -> String {
        #"{"type":"tool_use","id":"t","name":"\#(name)","input":\#(input)}"#
    }

    func testFlowGroupsToolCallsIntoTurnsAndCollectsFiles() {
        let lines: [Substring] = [
            user("fix the login bug", time: "2026-11-01T10:00:00.000Z"),
            assistant(tool("Read", #"{"file_path":"/p/lib/login.dart"}"#), id: "a1", time: "2026-11-01T10:00:01.000Z"),
            assistant(tool("Edit", #"{"file_path":"/p/lib/login.dart"}"#) + "," + tool("Bash", #"{"command":"flutter analyze lib/features/login --no-pub && echo this part is cut off past sixty chars"}"#),
                      id: "a2", time: "2026-11-01T10:00:02.000Z"),
            assistant(tool("Agent", #"{"description":"Research the API","subagent_type":"general-purpose"}"#), id: "a3", time: "2026-11-01T10:00:03.000Z"),
            user("now add a test", time: "2026-11-01T10:05:00.000Z"),
            assistant(tool("Write", #"{"file_path":"/p/test/login_test.dart"}"#), id: "a4", time: "2026-11-01T10:05:01.000Z"),
            user("sidechain noise", time: "2026-11-01T10:06:00.000Z", sidechain: true),
        ]
        let facts = TranscriptAnalyzer.analyze(lines: lines, isMain: true, origin: "main")
        XCTAssertEqual(facts.turns.count, 2, "the sidechain user line does not open a turn")
        XCTAssertEqual(facts.turns[0].prompt, "fix the login bug")
        XCTAssertEqual(facts.turns[0].calls.map(\.tool), ["Read", "Edit", "Bash", "Agent"])
        XCTAssertEqual(facts.turns[0].calls.map(\.target), ["login.dart", "login.dart",
                                                             String("flutter analyze lib/features/login --no-pub && echo this part is cut off past sixty chars".prefix(60)), "Research the API"])
        XCTAssertEqual(facts.turns[0].calls[2].target.count, 60)
        XCTAssertEqual(facts.turns[0].calls.map(\.subagent), [false, false, false, true])
        XCTAssertEqual(facts.turns[0].calls.map(\.edit), [false, true, false, false])
        XCTAssertEqual(facts.turns[1].calls.map(\.tool), ["Write"])
        XCTAssertEqual(facts.fileEdits, ["/p/lib/login.dart": 1, "/p/test/login_test.dart": 1], "Read is not an edit")
        XCTAssertEqual(facts.toolCounts["Read"], 1)
    }

    func testToolCallsBeforeAnyPromptLandInAnImplicitTurn() {
        let lines = [assistant(tool("Read", #"{"file_path":"/p/a.swift"}"#), id: "x", time: "2026-11-01T10:00:01.000Z")]
        let facts = TranscriptAnalyzer.analyze(lines: lines, isMain: true, origin: "main")
        XCTAssertEqual(facts.turns.count, 1)
        XCTAssertEqual(facts.turns[0].prompt, "")
    }

    func testFlowSpecShowsFilesBranchAndDiffStat() {
        let lines = [
            user("go", time: "2026-11-01T10:00:00.000Z"),
            assistant(tool("Edit", #"{"file_path":"/p/a.swift"}"#) + "," + tool("Edit", #"{"file_path":"/p/a.swift"}"#) + "," + tool("Write", #"{"file_path":"/p/b.swift"}"#),
                      id: "a", time: "2026-11-01T10:00:01.000Z"),
        ]
        let facts = TranscriptAnalyzer.analyze(lines: lines, isMain: true, origin: "main")
        let engine = VizEngine(git: { _ in GitSnapshot(branch: "feat/x", diffStat: " a.swift | 4 ++--\n 1 file changed") })
        let spec = engine.flowSpec(main: facts, combined: facts, cwd: "/p")
        XCTAssertEqual(spec.flow?.branch, "feat/x")
        XCTAssertEqual(spec.flow?.diffStat?.contains("1 file changed"), true)
        XCTAssertEqual(spec.flow?.files.map(\.name), ["a.swift", "b.swift"])
        XCTAssertEqual(spec.flow?.files.map(\.count), [2, 1])
        XCTAssertEqual(spec.flow?.totalCalls, 3)

        let empty = VizEngine(git: { _ in GitSnapshot() }).flowSpec(main: TranscriptFacts(), combined: TranscriptFacts(), cwd: "")
        XCTAssertNotNil(empty.empty, "nothing ran yet: empty state, not a blank flow")
    }

    func testMermaidBlocksAreExtractedNewestFirstFromMainAndSubagents() {
        let text = "Here:\\n```mermaid\\ngraph TD\\n  A-->B\\n```\\nand\\n```swift\\nlet x = 1\\n```\\n```mermaid\\nsequenceDiagram\\n  A->>B: hi\\n```"
        let main = TranscriptAnalyzer.analyze(lines: [
            assistant(#"{"type":"text","text":"\#(text)"}"#, id: "m1", time: "2026-11-01T10:00:00.000Z"),
        ], isMain: true, origin: "main")
        XCTAssertEqual(main.mermaid.map(\.source), ["graph TD\n  A-->B", "sequenceDiagram\n  A->>B: hi"])
        XCTAssertEqual(TranscriptAnalyzer.mermaidSources(in: "no fences"), [])
        XCTAssertEqual(TranscriptAnalyzer.mermaidSources(in: "```mermaid\n```"), [], "empty block ignored")

        let sub = TranscriptAnalyzer.analyze(lines: [
            assistant(#"{"type":"text","text":"```mermaid\nflowchart LR\n  X-->Y\n```"}"#, id: "s1", time: "2026-11-01T11:00:00.000Z"),
        ], isMain: false, origin: "Scan the repo")
        var combined = main
        combined.merge(sub)
        let spec = VizEngine().diagramSpec(combined)
        XCTAssertEqual(spec.diagram?.blocks.count, 3)
        XCTAssertEqual(spec.diagram?.blocks.first?.origin, "Scan the repo", "newest first")
        XCTAssertEqual(spec.diagram?.blocks.first?.source, "flowchart LR\n  X-->Y")
        XCTAssertNotNil(VizEngine().diagramSpec(TranscriptFacts()).empty)
    }

    func testUsageIsExtractedAndDedupedByMessageId() {
        let usage1 = #"{"input_tokens":10,"output_tokens":1,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000}"#
        let usage2 = #"{"input_tokens":10,"output_tokens":50,"cache_creation_input_tokens":100,"cache_read_input_tokens":1000}"#
        let usage3 = #"{"input_tokens":7,"output_tokens":3}"#
        let text = #"{"type":"text","text":"hi"}"#
        let facts = TranscriptAnalyzer.analyze(lines: [
            assistant(text, id: "msg1", time: "2026-11-01T10:00:00.000Z", usage: usage1),
            assistant(text, id: "msg1", time: "2026-11-01T10:00:00.500Z", usage: usage2),
            assistant(text, id: "msg2", time: "2026-11-01T10:01:00.000Z", usage: usage3),
        ], isMain: true, origin: "main")
        XCTAssertEqual(facts.usage.count, 2, "a streamed message counts once, with its last usage")
        XCTAssertEqual(facts.usage[0].output, 50)
        XCTAssertEqual(facts.usage[0].cacheRead, 1000)
        XCTAssertEqual(facts.usage[1], UsagePoint(time: facts.usage[1].time, input: 7, output: 3, cacheCreation: 0, cacheRead: 0))

        let spec = VizEngine().chartSpec(facts, subagents: [])
        XCTAssertEqual(spec.chart?.totals, UsageTotals(input: 17, output: 53, cacheCreation: 100, cacheRead: 1000))
        XCTAssertEqual(spec.chart?.tokens.count, 2)
    }

    func testChartCountsToolsAndSubagentModels() {
        let lines = [assistant(tool("Read", "{}") + "," + tool("Read", "{}") + "," + tool("Bash", #"{"command":"ls"}"#), id: "a", time: "2026-11-01T10:00:00.000Z")]
        let facts = TranscriptAnalyzer.analyze(lines: lines, isMain: true, origin: "main")
        func agent(_ id: String, _ model: String) -> Subagent {
            Subagent(id: id, type: "t", description: "d", model: model, depth: 1, parentToolUseId: nil, finished: true,
                     lastActivity: fixtureNow, transcriptURL: URL(fileURLWithPath: "/x"), state: .done)
        }
        let spec = VizEngine().chartSpec(facts, subagents: [agent("1", "haiku"), agent("2", "sonnet"), agent("3", "haiku")])
        XCTAssertEqual(spec.chart?.tools, [ChartCount(name: "Read", count: 2), ChartCount(name: "Bash", count: 1)])
        XCTAssertEqual(spec.chart?.models, [ModelCount(model: "haiku", count: 2), ModelCount(model: "sonnet", count: 1)])
        XCTAssertNotNil(VizEngine().chartSpec(TranscriptFacts(), subagents: []).empty)
    }

    func testLocalhostURLsAndDeviceHintsAreCollected() {
        let lines = [
            assistant(tool("Bash", #"{"command":"xcrun simctl boot 'iPhone 16 Pro' && xcrun simctl list"}"#), id: "d", time: "2026-11-01T10:00:00.000Z"),
            Substring(#"{"type":"user","uuid":"r","message":{"role":"user","content":[{"type":"tool_result","content":"ready on http://localhost:3000/app, also https://example.com/x and http://127.0.0.1:8080."}]}}"#),
        ]
        let facts = TranscriptAnalyzer.analyze(lines: lines, isMain: true, origin: "main")
        XCTAssertEqual(facts.localURLs, ["http://localhost:3000/app", "http://127.0.0.1:8080"])
        XCTAssertEqual(facts.deviceHints, ["iPhone 16 Pro"])
    }

    // MARK: - URL precedence

    private func home(launch: String?) throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("home-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        try FileManager.default.createDirectory(at: url.appendingPathComponent(".claude"), withIntermediateDirectories: true)
        if let launch { try Data(launch.utf8).write(to: url.appendingPathComponent(".claude/launch.json")) }
        return url
    }

    func testURLPrecedenceTranscriptThenProjectLaunchThenHomeLaunch() throws {
        let project = FileManager.default.temporaryDirectory.appendingPathComponent("proj-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: project) }
        try FileManager.default.createDirectory(at: project.appendingPathComponent(".claude"), withIntermediateDirectories: true)
        let homeDir = try home(launch: """
        {"configurations":[{"name":"other-dev","runtimeArgs":["--cwd","/somewhere/else"],"port":4000},
                           {"name":"mine-prod","runtimeArgs":["--cwd","\(project.path)"],"port":5002},
                           {"name":"mine-dev","runtimeArgs":["--cwd","\(project.path)"],"port":5001}]}
        """)

        // 1) home launch.json, preferring the dev config that mentions the folder
        XCTAssertEqual(WebURLResolver.resolve(transcriptURLs: [], cwd: project.path, home: homeDir)?.absoluteString, "http://localhost:5001")
        // 2) the project's own launch.json wins over the home one
        try Data(#"{"configurations":[{"name":"web","port":6000}]}"#.utf8)
            .write(to: project.appendingPathComponent(".claude/launch.json"))
        XCTAssertEqual(WebURLResolver.resolve(transcriptURLs: [], cwd: project.path, home: homeDir)?.absoluteString, "http://localhost:6000")
        // 3) a URL in the transcript wins over both, and the newest one wins
        XCTAssertEqual(WebURLResolver.resolve(transcriptURLs: ["http://localhost:3000", "http://127.0.0.1:3001/x"], cwd: project.path, home: homeDir)?.absoluteString,
                       "http://127.0.0.1:3001/x")
        // non-local URLs never count
        XCTAssertNil(WebURLResolver.resolve(transcriptURLs: ["https://example.com"], cwd: "/nowhere", home: try home(launch: nil)))
        XCTAssertNil(WebURLResolver.localURL("file:///etc/passwd"))
    }

    // MARK: - Suggester

    func testSuggesterRulesWithSignals() throws {
        let suggester = VisualizeSuggester()
        let empty = FileManager.default.temporaryDirectory.appendingPathComponent("sug-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: empty, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: empty) }

        XCTAssertEqual(suggester.suggest(cwd: empty.path), .flow)
        XCTAssertEqual(suggester.suggest(cwd: empty.path, signals: VizSignals(hasLocalhostURL: true)), .web)
        XCTAssertEqual(suggester.suggest(cwd: empty.path, signals: VizSignals(hasRecentMermaid: true)), .diagram)
        XCTAssertEqual(suggester.suggest(cwd: empty.path, signals: VizSignals(hasRecentMermaid: true, hasLocalhostURL: true)), .diagram,
                       "a fresh diagram beats everything")

        try Data("name: a".utf8).write(to: empty.appendingPathComponent("pubspec.yaml"))
        XCTAssertEqual(suggester.suggest(cwd: empty.path, signals: VizSignals(hasLocalhostURL: true)), .phone, "cwd rules come before the URL")
    }

    func testRecentMermaidSignalUsesTheClock() throws {
        let text = #"{"type":"text","text":"```mermaid\ngraph TD\n A-->B\n```"}"#
        let lines = [assistant(text, id: "m", time: "2026-11-01T13:50:00.000Z")]
        let facts = TranscriptAnalyzer.analyze(lines: lines, isMain: true, origin: "main")
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("t-\(UUID().uuidString).jsonl")
        try Data((String(lines[0]) + "\n").utf8).write(to: url)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        var session = Session(id: "s", pid: nil, name: "n", project: "p", cwd: "", entrypoint: .cli, kind: "x", state: .done,
                              statusUpdatedAt: fixtureNow, startedAt: fixtureNow, transcriptURL: url, control: .viewOnly)
        XCTAssertEqual(facts.mermaid.count, 1)

        // fixtureNow is 13:57:45, seven minutes after the block.
        XCTAssertTrue(VizEngine(now: { fixtureNow }, git: { _ in GitSnapshot() }).build(session: session).signals.hasRecentMermaid)
        XCTAssertFalse(VizEngine(now: { fixtureNow.addingTimeInterval(3600) }, git: { _ in GitSnapshot() }).build(session: session).signals.hasRecentMermaid)
        session.transcriptURL = nil
        XCTAssertFalse(VizEngine(now: { fixtureNow }, git: { _ in GitSnapshot() }).build(session: session).signals.hasRecentMermaid)
    }

    // MARK: - Spec JSON and device windows

    func testSpecJSONIsStable() throws {
        let spec = VizSpec(kind: .flow, flow: FlowSpec(
            branch: "main", diffStat: nil, files: [FlowFile(path: "/a/b.swift", name: "b.swift", count: 2)],
            turns: [FlowTurn(index: 1, prompt: "go", calls: [FlowCall(tool: "Edit", target: "b.swift", subagent: false, edit: true, time: 1000)])],
            totalCalls: 1))
        let expected = #"{"flow":{"branch":"main","files":[{"count":2,"name":"b.swift","path":"/a/b.swift"}],"totalCalls":1,"turns":[{"calls":[{"edit":true,"subagent":false,"target":"b.swift","time":1000,"tool":"Edit"}],"index":1,"prompt":"go"}]},"kind":"flow"}"#
        XCTAssertEqual(try spec.json(), expected)
        XCTAssertEqual(try spec.json(), try spec.json())
        let decoded = try JSONDecoder().decode(VizSpec.self, from: Data(expected.utf8))
        XCTAssertEqual(decoded, spec)

        let empty = VizSpec(kind: .chart, empty: .init(title: "T", detail: nil))
        XCTAssertEqual(try empty.json(), #"{"empty":{"title":"T"},"kind":"chart"}"#)
    }

    func testDeviceWindowFilterAndPicker() {
        XCTAssertTrue(DeviceWindowInfo.isDeviceWindow(bundleId: "com.apple.dt.Devices", appName: "DeviceHub", width: 395, height: 860))
        XCTAssertTrue(DeviceWindowInfo.isDeviceWindow(bundleId: "com.apple.iphonesimulator", appName: "Simulator", width: 400, height: 800))
        XCTAssertTrue(DeviceWindowInfo.isDeviceWindow(bundleId: "x", appName: "qemu-system-aarch64", width: 400, height: 800))
        XCTAssertTrue(DeviceWindowInfo.isDeviceWindow(bundleId: "x", appName: "Android Emulator - Pixel_8:5554", width: 400, height: 800))
        XCTAssertFalse(DeviceWindowInfo.isDeviceWindow(bundleId: "com.apple.dt.Devices", appName: "DeviceHub", width: 1512, height: 33), "menu bar sliver")
        XCTAssertFalse(DeviceWindowInfo.isDeviceWindow(bundleId: "com.apple.Safari", appName: "Safari", width: 800, height: 600))

        func window(_ id: UInt32, _ title: String, onScreen: Bool = true, active: Bool = false) -> DeviceWindowInfo {
            DeviceWindowInfo(id: id, appBundleId: "com.apple.dt.Devices", appName: "DeviceHub", title: title,
                             isOnScreen: onScreen, isAppActive: active, width: 400, height: 800)
        }
        let windows = [window(1, "iPhone 16 Pro", onScreen: false), window(2, "WorkFlow Links", active: true), window(3, "iPad Air")]
        XCTAssertEqual(DeviceWindowPicker.pick(from: windows, hints: ["iPad Air"])?.id, 3, "title matches a hint from the transcript")
        XCTAssertEqual(DeviceWindowPicker.pick(from: windows, hints: [])?.id, 2, "otherwise the active app's visible window")
        XCTAssertEqual(DeviceWindowPicker.pick(from: [window(1, "a", onScreen: false)], hints: [])?.id, 1)
        XCTAssertNil(DeviceWindowPicker.pick(from: [], hints: ["iPhone"]))
    }
}

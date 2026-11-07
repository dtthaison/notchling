import XCTest
@testable import Notchling

final class DelegateTests: XCTestCase {
    func testParseFlagsAndPrompt() throws {
        let o = try Delegate.parse(["agy", "--model", "gemini-4-argon", "--effort", "HIGH", "--folder", "src", "--out", "~/x",
                                    "--write", "--timeout", "60", "--", "review", "--this"], cwd: "/tmp/w").get()
        XCTAssertEqual(o.tool, .agy)
        XCTAssertEqual(o.model, "gemini-4-argon")
        XCTAssertEqual(o.effort, "high")
        XCTAssertEqual(o.folder, "/tmp/w/src")
        XCTAssertEqual(o.out, NSHomeDirectory() + "/x")
        XCTAssertTrue(o.write)
        XCTAssertEqual(o.timeout, 60)
        XCTAssertEqual(o.prompt, "review --this", "everything after -- is the prompt, flags included")
    }

    func testParseErrors() {
        XCTAssertThrowsError(try Delegate.parse(["claude", "--", "x"], cwd: "/").get())
        XCTAssertThrowsError(try Delegate.parse(["codex"], cwd: "/").get(), "no prompt")
        XCTAssertThrowsError(try Delegate.parse(["codex", "--model", "--", "x"], cwd: "/").get(), "flag without value")
        XCTAssertThrowsError(try Delegate.parse(["codex", "--nope", "x"], cwd: "/").get())
        XCTAssertEqual(try Delegate.parse(["Antigravity", "make", "it"], cwd: "/").get().tool, .agy)
    }

    func testAgyPickSplitsSuffixAndMapsEffort() {
        let models = ["gemini-4-argon", "gemini-3.8-flash", "gpt-oss-120b-medium"]
        XCTAssertEqual(Delegate.agyPick(model: "gemini-4-argon-high", effort: nil, models: models).model, "gemini-4-argon")
        XCTAssertEqual(Delegate.agyPick(model: "gemini-4-argon-high", effort: nil, models: models).effort, "high")
        XCTAssertEqual(Delegate.agyPick(model: "gemini-4-argon-high", effort: "low", models: models).effort, "low")
        XCTAssertEqual(Delegate.agyPick(model: "gpt-oss-120b-medium", effort: nil, models: models).model, "gpt-oss-120b-medium",
                       "a value Antigravity offers as is stays whole")
        XCTAssertEqual(Delegate.agyPick(model: "gpt-oss-120b", effort: nil, models: models).model, "gpt-oss-120b-medium")
        XCTAssertEqual(Delegate.agyPick(model: nil, effort: "xhigh", models: models).effort, "high")
        XCTAssertEqual(Delegate.agyPick(model: nil, effort: "minimal", models: models).effort, "low")
    }

    func testAgyModelsGroupsLevels() {
        let listing = "Fetching available models...\ngemini-4-argon-medium\tGemini 4 Argon (Medium)\ngemini-4-argon-high\tGemini 4 Argon (High)\ngpt-oss-120b\tGPT-OSS\n"
        let groups = Delegate.agyModels(listing)
        XCTAssertEqual(groups.map(\.model), ["gemini-4-argon", "gpt-oss-120b"])
        XCTAssertEqual(groups.first?.levels, ["medium", "high"])
    }

    func testMediaSinceAndCopyNeverOverwrites() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("delegate-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: dir) }
        let old = dir.appendingPathComponent("old.png"), new = dir.appendingPathComponent("cup.jpg"), note = dir.appendingPathComponent("plan.md")
        for f in [old, new, note] { try Data("x".utf8).write(to: f) }
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-3600)], ofItemAtPath: old.path)
        let found = Delegate.media(in: dir, since: Date().addingTimeInterval(-60))
        XCTAssertEqual(found.map(\.lastPathComponent), ["cup.jpg"])

        let out = dir.appendingPathComponent("out")
        let first = try Delegate.copy(found, to: out)
        let second = try Delegate.copy(found, to: out)
        XCTAssertEqual(first.map(\.lastPathComponent), ["cup.jpg"])
        XCTAssertEqual(second.map(\.lastPathComponent), ["cup-2.jpg"])
    }

    /// Laid out the way agy 1.5.1 left a session whose image subagent finished after the turn ended.
    func testAgyProgressWaitsForSubagentAndParentAnswer() throws {
        let system = FileManager.default.temporaryDirectory.appendingPathComponent("agy-\(UUID().uuidString)/.system_generated")
        defer { try? FileManager.default.removeItem(at: system.deletingLastPathComponent()) }
        for sub in ["subagents", "messages", "logs"] {
            try FileManager.default.createDirectory(at: system.appendingPathComponent(sub), withIntermediateDirectories: true)
        }
        let since = Date()
        XCTAssertEqual(Delegate.agyProgress(system: system, since: since), .finished)

        try Data(#"{"conversationId":"sub1","state":"SUBAGENT_STATE_ALIVE"}"#.utf8).write(to: system.appendingPathComponent("subagents/sub1.json"))
        let transcript = system.appendingPathComponent("logs/transcript.jsonl")
        try Data(#"{"step_index":5,"type":"PLANNER_RESPONSE","status":"DONE","content":"I asked the image generator."}"#.utf8).write(to: transcript)
        XCTAssertEqual(Delegate.agyProgress(system: system, since: since), .waiting(subagents: ["sub1"]))

        try Data(#"{"id":"m1","sender":"sub1","content":"Here is the image"}"#.utf8).write(to: system.appendingPathComponent("messages/m1.json"))
        try Data(#"{"m1":true}"#.utf8).write(to: system.appendingPathComponent("messages/read.json"))
        var lines = try String(contentsOf: transcript, encoding: .utf8) + "\n" + #"{"step_index":6,"type":"SYSTEM_MESSAGE","status":"DONE","content":"x"}"#
        try lines.write(to: transcript, atomically: true, encoding: .utf8)
        XCTAssertEqual(Delegate.agyProgress(system: system, since: since), .waiting(subagents: ["sub1"]), "woken, not answered yet")

        lines += "\n" + #"{"step_index":7,"type":"PLANNER_RESPONSE","status":"DONE","content":"Saved: /x/teapot.jpg"}"#
        try lines.write(to: transcript, atomically: true, encoding: .utf8)
        XCTAssertEqual(Delegate.agyProgress(system: system, since: since), .answered("Saved: /x/teapot.jpg", subagents: ["sub1"]))
    }

    func testRenderAndExitCode() {
        var o = Delegate.Outcome(tool: .codex, session: "s1", model: "m", effort: "high")
        o.reply = "Done."
        o.media = ["/a.png"]
        o.seconds = 42
        let text = Delegate.render(o)
        XCTAssertTrue(text.hasPrefix("Codex (m, high): done in 42s"))
        XCTAssertTrue(text.contains("Session: s1\nGenerated:\n  /a.png"))
        XCTAssertTrue(text.hasSuffix("\n\nDone."))
        XCTAssertEqual(o.exitCode, 0)
        o.error = "x"
        XCTAssertEqual(o.exitCode, 1)
        o.timedOut = true
        XCTAssertEqual(o.exitCode, 124)
        XCTAssertTrue(Delegate.json(o).contains("\"media\":[\"/a.png\"]"))
    }

    func testSkillNamesBinaryAndToolNotes() {
        let claude = DelegateSkill.content(binary: "/Apps/N", tool: .claude)
        XCTAssertTrue(claude.contains("name: notchling-delegate"))
        XCTAssertTrue(claude.contains("'/Apps/N' --delegate <codex|agy>"))
        XCTAssertTrue(claude.contains("run_in_background"))
        XCTAssertTrue(DelegateSkill.content(binary: "/Apps/N", tool: .codex).contains("escalated permissions"))
    }
}

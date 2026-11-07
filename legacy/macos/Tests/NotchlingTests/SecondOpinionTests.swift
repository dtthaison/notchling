import XCTest
@testable import Notchling

final class SecondOpinionTests: XCTestCase {
    private let diff = [
        DiffLine(kind: .hunk, text: "@@ -1,2 +1,2 @@", number: nil),
        DiffLine(kind: .context, text: "let a = 1", number: 1),
        DiffLine(kind: .remove, text: "let b = 2", number: 2),
        DiffLine(kind: .add, text: "let b = 3", number: 2),
    ]

    func testChangePromptHasFolderFileAndUnifiedDiff() {
        let p = SecondOpinionPrompt.build(.change(path: "/w/App/Foo.swift", diff: diff), folder: "/w")
        XCTAssertTrue(p.hasPrefix("Review this change for bugs"))
        XCTAssertTrue(p.contains("Folder: /w"))
        XCTAssertTrue(p.contains("File: /w/App/Foo.swift"))
        XCTAssertTrue(p.contains("```diff\n@@ -1,2 +1,2 @@\n let a = 1\n-let b = 2\n+let b = 3\n```"))
        XCTAssertEqual(SecondOpinionPrompt.title(.change(path: "/w/App/Foo.swift", diff: diff)), "Foo.swift")
    }

    func testMessagePromptAndClipping() {
        let p = SecondOpinionPrompt.build(.message("  The cache is safe.  "), folder: "/w")
        XCTAssertTrue(p.contains("Folder: /w"))
        XCTAssertTrue(p.hasSuffix("Message:\n\nThe cache is safe."))
        let long = String(repeating: "x", count: SecondOpinionPrompt.bodyLimit + 50)
        let clipped = SecondOpinionPrompt.build(.message(long), folder: "/w")
        XCTAssertTrue(clipped.contains("(50 more characters left out)"))
        XCTAssertEqual(SecondOpinionPrompt.title(.message("x")), "Reply")
    }

    func testArgumentsAreReadOnlyAndSkipDefaults() throws {
        let a = SecondOpinion.arguments(tool: .codex, model: nil, effort: "", folder: "/w", promptFile: "/tmp/p.txt")
        XCTAssertEqual(a, ["--delegate", "codex", "--folder", "/w", "--timeout", "900", "--json", "--prompt-file", "/tmp/p.txt"])
        let file = NSTemporaryDirectory() + "so-prompt-\(UUID().uuidString).txt"
        try "from file".write(toFile: file, atomically: true, encoding: .utf8)
        defer { try? FileManager.default.removeItem(atPath: file) }
        let b = SecondOpinion.arguments(tool: .agy, model: "gemini-4-argon", effort: "high", folder: "/w", promptFile: file)
        XCTAssertFalse(b.contains("--write"))
        XCTAssertEqual(Array(b[0...5]), ["--delegate", "agy", "--model", "gemini-4-argon", "--effort", "high"])
        // The command accepts them.
        let parsed = try Delegate.parse(Array(b.dropFirst()), cwd: "/").get()
        XCTAssertEqual(parsed.tool, .agy)
        XCTAssertEqual(parsed.prompt, "from file")
    }

    func testParsesDelegateJSON() throws {
        var o = Delegate.Outcome(tool: .codex, session: "abc", model: "gpt-x", effort: "high")
        o.reply = "Line 3 looks off."
        o.seconds = 12
        o.tokens = 900
        let r = try XCTUnwrap(SecondOpinionResult.parse("noise\n" + Delegate.json(o) + "\n"))
        XCTAssertTrue(r.ok)
        XCTAssertEqual(r.reply, "Line 3 looks off.")
        XCTAssertEqual(r.session, "abc")
        XCTAssertEqual(r.model, "gpt-x")
        XCTAssertEqual(r.seconds, 12)
        XCTAssertEqual(r.tokens, 900)
        XCTAssertNil(SecondOpinionResult.parse("Error: no folder"))

        o.error = "boom"
        o.timedOut = true
        let failed = try XCTUnwrap(SecondOpinionResult.parse(Delegate.json(o)))
        XCTAssertFalse(failed.ok)
        XCTAssertTrue(failed.timedOut)
        XCTAssertEqual(failed.error, "boom")
    }

    func testFinishAppliesResult() throws {
        var run = SecondOpinion(session: "s", anchor: "a", subject: "Reply", tool: "agy", folder: "/w")
        var o = Delegate.Outcome(tool: .agy, session: "conv-1", model: "m", effort: "low")
        o.reply = "Fine."
        run.finish(with: SecondOpinionResult.parse(Delegate.json(o)), output: "", cancelled: false)
        XCTAssertEqual(run.status, .done)
        XCTAssertEqual(run.delegatedSession, "conv-1")
        XCTAssertEqual(run.model, "m")

        var failed = SecondOpinion(session: "s", anchor: "a", subject: "Reply", tool: "codex", folder: "/w")
        failed.finish(with: nil, output: "crash\n", cancelled: false)
        XCTAssertEqual(failed.status, .failed)
        XCTAssertEqual(failed.error, "crash")

        var empty = SecondOpinion(session: "s", anchor: "a", subject: "Reply", tool: "codex", folder: "/w")
        o.reply = ""
        empty.finish(with: SecondOpinionResult.parse(Delegate.json(o)), output: "", cancelled: false)
        XCTAssertEqual(empty.status, .failed)

        var cancelled = SecondOpinion(session: "s", anchor: "a", subject: "Reply", tool: "codex", folder: "/w")
        cancelled.finish(with: nil, output: "", cancelled: true)
        XCTAssertEqual(cancelled.status, .cancelled)
    }

    func testStoreKeepsResultsAcrossLaunchesAndFailsStaleRuns() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("so-\(UUID().uuidString)")
        let file = dir.appendingPathComponent("second-opinions.json")
        defer { try? FileManager.default.removeItem(at: dir) }
        let first = SecondOpinionStore(fileURL: file)
        // A folder that doesn't exist makes the command fail to start, so nothing external runs.
        let run = first.start(.init(session: "s", anchor: "a", subject: "Reply", tool: .codex, model: nil, effort: nil,
                                    folder: "/nonexistent-\(UUID().uuidString)", prompt: "hi"))
        XCTAssertEqual(first.runs(session: "s", anchors: ["a"]).map(\.id), [run.id])
        XCTAssertEqual(first.runs(session: "s", anchors: ["b"]), [])
        XCTAssertEqual(first.runs(session: "other", anchors: ["a"]), [])
        first.remove(run.id)
        XCTAssertTrue(first.opinions.isEmpty)

        let data = try JSONEncoder().encode([SecondOpinion(session: "s", anchor: "a", subject: "Reply", tool: "codex", folder: "/w")])
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        try data.write(to: file)
        let second = SecondOpinionStore(fileURL: file)
        XCTAssertEqual(second.opinions.count, 1)
        XCTAssertEqual(second.opinions[0].status, .failed, "a run that was going when the app quit")
    }

    func testAgyModelOptions() {
        let o = SecondOpinionModels.options(agyListing: "gemini-4-argon-low\tA\ngemini-4-argon-high\tA\nflash\tF\n")
        XCTAssertEqual(o.map(\.model), ["gemini-4-argon", "flash"])
        XCTAssertEqual(o[0].efforts, ["low", "high"])
        XCTAssertEqual(o[1].efforts, [])
    }
}

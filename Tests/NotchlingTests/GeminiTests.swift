import XCTest
@testable import Notchling

/// Gemini CLI over ACP. `acp-gemini.jsonl` starts with a real exchange (initialize, and the error
/// `session/new` gives while Gemini isn't signed in); the load and the turn after it are rebuilt from the
/// update shapes in gemini-cli 0.65's source (see the `#` lines in the file).
final class GeminiTests: XCTestCase {
    private struct Replay {
        var transcript = ACPTranscript()
        var loadResult: [String: Any]?
        var permission: [String: Any]?
        var newSessionError: String?
    }

    private func replay() throws -> Replay {
        let url = try XCTUnwrap(Bundle.module.url(forResource: "acp-gemini", withExtension: "jsonl", subdirectory: "Fixtures"))
        var r = Replay()
        for line in try String(contentsOf: url, encoding: .utf8).split(separator: "\n") where !line.hasPrefix("#") {
            let sent = line.hasPrefix(">")
            let json = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(line.dropFirst(2).utf8)) as? [String: Any])
            let method = json["method"] as? String
            if sent {
                if method == "session/prompt",
                   let prompt = ((json["params"] as? [String: Any])?["prompt"] as? [[String: Any]])?.first?["text"] as? String {
                    // What ACPRunner does: Gemini doesn't send the prompt back.
                    r.transcript.beginTurn()
                    if !ACPTool.gemini.echoesPrompt {
                        r.transcript.apply(["sessionUpdate": "user_message_chunk", "content": ["type": "text", "text": prompt]])
                    }
                }
                continue
            }
            let params = json["params"] as? [String: Any]
            if method == "session/update", let update = params?["update"] as? [String: Any] {
                if update["sessionUpdate"] as? String != "available_commands_update" { r.transcript.apply(update) }
            } else if method == "session/request_permission", let call = params?["toolCall"] as? [String: Any] {
                r.permission = params
                r.transcript.apply(call.merging(["sessionUpdate": "tool_call_update"]) { _, new in new })
            } else if let result = json["result"] as? [String: Any] {
                if result["stopReason"] != nil {
                    r.transcript.endTurn(stopReason: result["stopReason"] as? String, usage: result["usage"] as? [String: Any])
                } else if result["modes"] != nil {
                    r.loadResult = result
                }
            } else if let error = json["error"] as? [String: Any] {
                r.newSessionError = error["message"] as? String
            }
        }
        return r
    }

    func testHistoryAndTurn() throws {
        let t = try replay().transcript
        let kinds = t.items.map { item -> String in
            switch item.kind {
            case .user: return "user"
            case .assistant: return "assistant"
            case .thinking: return "thinking"
            case .tools(let calls): return "tools(\(calls.count))"
            case .turnEnd: return "end"
            default: return "other"
            }
        }
        XCTAssertEqual(kinds, ["user", "thinking", "tools(1)", "assistant", "user", "thinking", "tools(1)", "assistant", "end"])

        guard case .tools(let ls) = t.items[2].kind else { return XCTFail() }
        XCTAssertEqual(ls[0].kind, .shell)
        XCTAssertEqual(ls[0].title, "ls")
        XCTAssertTrue(ls[0].finished)
        XCTAssertEqual(ls[0].output, "hello.txt\nnotes.md")

        guard case .user(let asked, _) = t.items[4].kind else { return XCTFail() }
        XCTAssertEqual(asked, "change hello.txt to say hi")

        guard case .tools(let edit) = t.items[6].kind else { return XCTFail() }
        XCTAssertEqual(edit[0].kind, .edit)
        XCTAssertTrue(edit[0].finished)
        XCTAssertEqual(edit[0].diff.filter { $0.kind == .remove }.map(\.text), ["hello"])
        XCTAssertEqual(edit[0].diff.filter { $0.kind == .add }.map(\.text), ["hi"])

        // The [MODE_UPDATE] notice is dropped and the reply's chunks join.
        guard case .assistant(let reply) = t.items[7].kind else { return XCTFail() }
        XCTAssertEqual(reply, "Done: hello.txt now says hi.")

        let c = t.conversation()
        XCTAssertNil(c.turnStart)
        XCTAssertEqual(c.facts.turns, 1)
        XCTAssertEqual(c.facts.files.count, 1)
        // The usage_update's size wins over adding the cached tokens a second time.
        XCTAssertEqual(c.facts.contextTokens, 4210)
    }

    func testPermissionRequest() throws {
        let p = try XCTUnwrap(try replay().permission)
        let call = ACPTranscript.toolCall(try XCTUnwrap(p["toolCall"] as? [String: Any]), merging: nil)
        XCTAssertEqual(call.kind, .edit)
        XCTAssertEqual(call.title, "Edit hello.txt: hello => hi")
        XCTAssertFalse(call.diff.isEmpty)
        let options = (p["options"] as? [[String: Any]] ?? []).compactMap { $0["kind"] as? String }
        XCTAssertEqual(options, ["allow_always", "allow_once", "reject_once"])
    }

    func testLoadResultOffersModelAndModeAsOptions() throws {
        let options = ACPConfigOption.parse(result: try XCTUnwrap(try replay().loadResult))
        XCTAssertEqual(options.map(\.id), ["model", "mode"])
        XCTAssertEqual(options.map(\.source), [.model, .mode])
        XCTAssertEqual(options[0].currentName, "Auto")
        XCTAssertEqual(options[1].values.map(\.value), ["default", "autoEdit", "yolo"])
        XCTAssertEqual(options[1].currentName, "Default")
        // configOptions, when a tool sends them, win.
        let agy = ACPConfigOption.parse(result: ["configOptions": [["id": "model", "name": "Model", "type": "select", "currentValue": "a",
                                                                    "options": [["value": "a", "name": "A"]]]],
                                                 "models": ["currentModelId": "x", "availableModels": [["modelId": "x"]]]])
        XCTAssertEqual(agy.map(\.source), [.configOption])
    }

    func testSignedOutErrorIsExplained() throws {
        let message = try XCTUnwrap(try replay().newSessionError)
        XCTAssertTrue(ACPRunner.friendly(message, tool: .gemini).hasPrefix("Gemini CLI isn't signed in"))
        XCTAssertEqual(ACPRunner.friendly(message, tool: .antigravity), message)
        XCTAssertEqual(ACPRunner.friendly("Rate limit exceeded. Try again later.", tool: .gemini), "Rate limit exceeded. Try again later.")
    }

    // MARK: Sessions on disk

    private func makeRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("gemini-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        return root
    }

    private func write(_ text: String, to url: URL, modified: Date = Date()) throws {
        try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
        try text.write(to: url, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.modificationDate: modified], ofItemAtPath: url.path)
    }

    func testSummaryFromJSONLLines() {
        let lines = [
            #"{"sessionId":"abc-1","projectHash":"h","startTime":"2026-11-05T10:00:00Z","lastUpdated":"2026-11-05T10:00:00Z","kind":"main"}"#,
            #"{"id":"m1","timestamp":"2026-11-05T10:00:01Z","type":"user","content":[{"text":"fix the "},{"text":"build"}]}"#,
            #"{"id":"m2","timestamp":"2026-11-05T10:00:05Z","type":"gemini","content":"Sure.","thoughts":[],"toolCalls":[]}"#,
            "not json",
            #"{"$set":{"summary":"Build fix"}}"#,
        ]
        let s = GeminiSessions.summary(lines: lines)
        XCTAssertEqual(s, GeminiSessions.Summary(id: "abc-1", summary: "Build fix", firstUser: "fix the build"))
        XCTAssertNil(GeminiSessions.summary(lines: [lines[0]]), "no message yet: nothing to resume")
        XCTAssertNil(GeminiSessions.summary(lines: [#"{"sessionId":"s","projectHash":"h","kind":"subagent"}"#, lines[1]]))
    }

    func testSummaryFromLegacyJSON() {
        let data = Data(#"{"sessionId":"old-1","projectHash":"h","messages":[{"id":"1","type":"user","content":"hello there"},{"id":"2","type":"gemini","content":"hi"}]}"#.utf8)
        XCTAssertEqual(GeminiSessions.summary(legacy: data), GeminiSessions.Summary(id: "old-1", summary: nil, firstUser: "hello there"))
    }

    func testRowsComeFromTheProjectRegistryAndSkipOldOnes() throws {
        let root = try makeRoot()
        let now = Date()
        try write(#"{"projects":{"/Users/me/app":"app","/Users/me/lib":"lib"}}"#, to: root.appendingPathComponent("projects.json"))
        let chats = root.appendingPathComponent("tmp/app/chats")
        let user = #"{"id":"m1","type":"user","content":"write the readme"}"#
        try write(["{\"sessionId\":\"new-1\",\"projectHash\":\"h\"}", user].joined(separator: "\n") + "\n",
                  to: chats.appendingPathComponent("session-2026-11-06T09-00-new-1xxxx.jsonl"), modified: now.addingTimeInterval(-60))
        // The same session in its older .json form, and an old session, and a file that isn't a session.
        try write(#"{"sessionId":"new-1","messages":[{"id":"1","type":"user","content":"write the readme"}]}"#,
                  to: chats.appendingPathComponent("session-2026-11-06T08-00-new-1xxxx.json"), modified: now.addingTimeInterval(-3600))
        try write(["{\"sessionId\":\"old-2\",\"projectHash\":\"h\"}", user].joined(separator: "\n") + "\n",
                  to: chats.appendingPathComponent("session-2026-10-01T09-00-old-2xxxx.jsonl"), modified: now.addingTimeInterval(-30 * 86400))
        try write("x", to: chats.appendingPathComponent("notes.txt"))
        let lib = root.appendingPathComponent("tmp/lib/chats")
        try write(["{\"sessionId\":\"lib-1\",\"projectHash\":\"h\"}", #"{"id":"m1","type":"user","content":"explain\nthe api"}"#].joined(separator: "\n") + "\n",
                  to: lib.appendingPathComponent("session-2026-11-06T09-30-lib-1xxxx.jsonl"), modified: now.addingTimeInterval(-10))

        let rows = GeminiSessions.rows(now: now, root: root)
        XCTAssertEqual(rows.map(\.id), ["lib-1", "new-1"])
        XCTAssertEqual(rows.map(\.folder), ["/Users/me/lib", "/Users/me/app"])
        XCTAssertEqual(rows.map(\.title), ["explain", "write the readme"])
    }

    func testRowsAreEmptyWithoutGemini() throws {
        XCTAssertTrue(GeminiSessions.rows(root: try makeRoot()).isEmpty)
    }

    // MARK: Login, ids, choices

    func testLoginReadsTheGoogleAccount() throws {
        let home = try makeRoot()
        XCTAssertEqual(GeminiLogin.read(home: home, environment: [:]), GeminiLogin.State(email: nil, method: nil, signedIn: false))
        try write(#"{"active":"me@example.com","old":[]}"#, to: home.appendingPathComponent(".gemini/google_accounts.json"))
        try write(#"{"security":{"auth":{"selectedType":"oauth-personal"}}}"#, to: home.appendingPathComponent(".gemini/settings.json"))
        XCTAssertEqual(GeminiLogin.read(home: home, environment: [:]),
                       GeminiLogin.State(email: "me@example.com", method: "oauth-personal", signedIn: true))
        XCTAssertTrue(GeminiLogin.read(home: try makeRoot(), environment: ["GEMINI_API_KEY": "k"]).signedIn)
    }

    func testIdsTargetsAndChoices() {
        XCTAssertEqual(GeminiSessions.agentID("s1"), "gemini:s1")
        XCTAssertEqual(ACPTool.gemini.draftPrefix, "gemini-new:")
        let target = AgentTarget.gemini(sessionID: "s1")
        XCTAssertEqual(target.acp?.tool, .gemini)
        XCTAssertEqual(target.acp?.session, "s1")
        XCTAssertTrue(AgentTarget.gemini(sessionID: "").isDraft)
        XCTAssertEqual(Agent(id: "gemini:s1", name: "n", project: "p", cwd: "/", group: .completed, label: "Gemini",
                             detail: nil, since: Date(), target: target).sessionID, "s1")
        XCTAssertEqual(SessionChoice(key: SessionChoice.gemini.key), .gemini)
        XCTAssertEqual(SessionTool(key: SessionTool.gemini.key), .gemini)
        XCTAssertEqual(SessionTool(target), .gemini)
    }
}

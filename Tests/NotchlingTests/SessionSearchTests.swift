import XCTest
@testable import Notchling

final class SessionSearchTests: XCTestCase {
    private func line(_ json: String) -> Data { Data(json.utf8) }

    // MARK: Extraction

    func testClaudeUserAndAssistantText() {
        let user = line(#"{"type":"user","isSidechain":false,"cwd":"/Users/me/app","message":{"role":"user","content":"fix the login bug"}}"#)
        XCTAssertEqual(TranscriptText.claude(user), .message(.user, "fix the login bug", cwd: "/Users/me/app"))
        let assistant = line(#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"Found it."},{"type":"tool_use","name":"Bash","input":{}},{"type":"text","text":"Fixed."}]}}"#)
        XCTAssertEqual(TranscriptText.claude(assistant), .message(.assistant, "Found it.\nFixed.", cwd: nil))
    }

    func testClaudeSkipsNoise() {
        // Tool results, commands, caveats, meta lines, sidechains and long lines carry no conversation.
        XCTAssertNil(TranscriptText.claude(line(#"{"type":"user","message":{"role":"user","content":[{"type":"tool_result","content":"ok"}]}}"#)))
        XCTAssertNil(TranscriptText.claude(line(#"{"type":"user","message":{"role":"user","content":"<command-name>/clear</command-name>"}}"#)))
        XCTAssertNil(TranscriptText.claude(line(#"{"type":"user","isMeta":true,"message":{"role":"user","content":"hello"}}"#)))
        XCTAssertNil(TranscriptText.claude(line(#"{"type":"user","isSidechain":true,"message":{"role":"user","content":"hello"}}"#)))
        XCTAssertNil(TranscriptText.claude(line(#"{"type":"attachment","attachment":{"type":"hook_success"}}"#)))
        XCTAssertNil(TranscriptText.claude(line(#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"thinking","thinking":"x"}]}}"#)))
        let huge = #"{"type":"user","message":{"role":"user","content":""# + String(repeating: "a", count: 100_100) + #""}}"#
        XCTAssertNil(TranscriptText.claude(line(huge)))
    }

    func testClaudeTitles() {
        XCTAssertEqual(TranscriptText.claude(line(#"{"type":"custom-title","customTitle":"notching","sessionId":"s"}"#)), .customTitle("notching"))
        XCTAssertEqual(TranscriptText.claude(line(#"{"type":"ai-title","aiTitle":"Piaz Switch","sessionId":"s"}"#)), .aiTitle("Piaz Switch"))
    }

    func testCodexMessages() {
        let user = line(#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"add a retry"}]}}"#)
        XCTAssertEqual(TranscriptText.codex(user), .message(.user, "add a retry", cwd: nil))
        let env = line(#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"<environment_context>x</environment_context>"}]}}"#)
        XCTAssertNil(TranscriptText.codex(env))
        let developer = line(#"{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"type":"input_text","text":"rules"}]}}"#)
        XCTAssertNil(TranscriptText.codex(developer))
        let reply = line(#"{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done."}]}}"#)
        XCTAssertEqual(TranscriptText.codex(reply), .message(.assistant, "Done.", cwd: nil))
    }

    func testBuilderTitleAndCap() {
        var b = SessionTextBuilder()
        b.add(.message(.user, "first prompt here", cwd: "/x"))
        b.add(.message(.assistant, "an answer", cwd: nil))
        XCTAssertEqual(b.title, "first prompt here")
        XCTAssertEqual(b.cwd, "/x")
        b.add(.aiTitle("Generated"))
        XCTAssertEqual(b.title, "Generated")
        b.add(.customTitle("Mine"))
        XCTAssertEqual(b.title, "Mine")
        XCTAssertEqual(b.text, "first prompt here\nan answer")

        // Over the cap the newest text stays.
        var big = SessionTextBuilder()
        let chunk = String(repeating: "x", count: 6_000)
        for i in 0..<10 { big.add(.message(.user, "\(i)" + chunk, cwd: nil)) }
        XCTAssertLessThanOrEqual(big.text.count, SessionTextBuilder.maxChars + 10)
        XCTAssertTrue(big.text.contains("9" + chunk))
        XCTAssertFalse(big.text.hasPrefix("0"))
    }

    func testCodexIDFromFileName() {
        XCTAssertEqual(SessionIndex.codexID(fileName: "rollout-2026-11-05T23-05-22-01a1ac75-2a19-7750-a0d3-2df380733486.jsonl"),
                       "01a1ac75-2a19-7750-a0d3-2df380733486")
    }

    // MARK: Matching

    private func session(_ id: String, title: String, folder: String = "/Users/me/app", text: String,
                         age: TimeInterval = 3600, tool: SessionTool = .claude, now: Date = Date()) -> IndexedSession {
        IndexedSession(meta: SessionMeta(key: "\(tool.key):\(id)", toolKey: tool.key, sessionID: id, title: title,
                                         folder: folder, path: "", configDir: nil,
                                         modified: now.addingTimeInterval(-age).timeIntervalSince1970), text: text)
    }

    func testEveryWordMustMatch() {
        let now = Date()
        let a = session("a", title: "Login flow", text: "we rewrote the token refresh", now: now)
        let b = session("b", title: "Billing", text: "invoices and refresh of the plan", now: now)
        let hits = SessionSearch.rank(["token", "refresh"], in: [a, b], now: now)
        XCTAssertEqual(hits.map(\.meta.sessionID), ["a"])
        XCTAssertTrue(SessionSearch.rank(["nothing"], in: [a, b], now: now).isEmpty)
        XCTAssertTrue(SessionSearch.rank([], in: [a, b], now: now).isEmpty)
    }

    func testCaseInsensitiveAndFolderMatch() {
        let now = Date()
        let s = session("a", title: "T", folder: "/Users/me/Notchling", text: "The Keychain item", now: now)
        XCTAssertEqual(SessionSearch.rank(["keychain"], in: [s], now: now).count, 1)
        XCTAssertEqual(SessionSearch.rank(["notchling"], in: [s], now: now).count, 1)
        // A folder-only match has no snippet.
        XCTAssertNil(SessionSearch.rank(["notchling"], in: [s], now: now).first?.snippet)
    }

    func testTitleOutranksBodyAndNewerBreaksTies() {
        let now = Date()
        let body = session("body", title: "Other", text: "something about caching here", age: 60, now: now)
        let title = session("title", title: "Caching layer", text: "unrelated", age: 86400 * 5, now: now)
        XCTAssertEqual(SessionSearch.rank(["caching"], in: [body, title], now: now).map(\.meta.sessionID), ["title", "body"])

        let old = session("old", title: "Same", text: "same words", age: 86400 * 6, now: now)
        let new = session("new", title: "Same", text: "same words", age: 60, now: now)
        XCTAssertEqual(SessionSearch.rank(["words"], in: [old, new], now: now).map(\.meta.sessionID), ["new", "old"])
    }

    func testPhraseOutranksScatteredWords() {
        let now = Date()
        let scattered = session("s", title: "A", text: "the cache is slow; later we added a token", age: 3600, now: now)
        let phrase = session("p", title: "B", text: "we added a token cache to the app", age: 3600, now: now)
        XCTAssertEqual(SessionSearch.rank(["token", "cache"], in: [scattered, phrase], now: now).first?.meta.sessionID, "p")
    }

    func testLimit() {
        let now = Date()
        let many = (0..<10).map { session("s\($0)", title: "t", text: "match", now: now) }
        XCTAssertEqual(SessionSearch.rank(["match"], in: many, now: now, limit: 3).count, 3)
    }

    func testLatestHasNoSnippet() {
        let now = Date()
        let hits = SessionIndex.latest([session("a", title: "a", text: "x", age: 500, now: now),
                                        session("b", title: "b", text: "x", age: 10, now: now)], limit: 5)
        XCTAssertEqual(hits.map(\.meta.sessionID), ["b", "a"])
        XCTAssertNil(hits[0].snippet)
    }

    // MARK: Snippet

    func testSnippetAroundMatch() {
        let filler = String(repeating: "lorem ipsum dolor ", count: 20)
        let s = session("a", title: "T", text: filler + "the Keychain prompt appeared twice " + filler)
        let snippet = SessionSearch.snippet(in: s, words: ["keychain"])
        XCTAssertNotNil(snippet)
        XCTAssertTrue(snippet!.contains("Keychain prompt"), snippet!)
        XCTAssertTrue(snippet!.hasPrefix("…") && snippet!.hasSuffix("…"))
        XCTAssertLessThan(snippet!.count, 200)
    }

    func testSnippetCollapsesNewlinesAndKeepsShortText() {
        let s = session("a", title: "T", text: "line one\n\nline   two with needle\nend")
        XCTAssertEqual(SessionSearch.snippet(in: s, words: ["needle"]), "line one line two with needle end")
    }

    func testSnippetWithMultibyteText() {
        let s = session("a", title: "T", text: String(repeating: "đã sửa lỗi ", count: 30) + "Xác thực token mới " + String(repeating: "xong rồi ", count: 30))
        let snippet = SessionSearch.snippet(in: s, words: ["token"])
        XCTAssertNotNil(snippet)
        XCTAssertTrue(snippet!.contains("token"))
        XCTAssertFalse(snippet!.contains("\u{FFFD}"))
    }

    func testHighlights() {
        let text = "Fix the Token refresh and token cache"
        let ranges = SessionSearch.highlights(in: text, words: ["token"])
        XCTAssertEqual(ranges.map { String(text[$0]) }, ["Token", "token"])
        XCTAssertTrue(SessionSearch.highlights(in: text, words: ["absent"]).isEmpty)
    }

    func testAge() {
        let now = Date()
        XCTAssertEqual(SessionAge.text(from: now.addingTimeInterval(-10), now: now), "now")
        XCTAssertEqual(SessionAge.text(from: now.addingTimeInterval(-300), now: now), "5m")
        XCTAssertEqual(SessionAge.text(from: now.addingTimeInterval(-3 * 3600), now: now), "3h")
        XCTAssertEqual(SessionAge.text(from: now.addingTimeInterval(-2 * 86400), now: now), "2d")
    }
}

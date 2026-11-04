import XCTest
@testable import ConsoleCore

final class ChatPagingTests: XCTestCase {
    private func file() -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("chat-\(UUID().uuidString).jsonl")
        FileManager.default.createFile(atPath: url.path, contents: nil)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func append(_ lines: [String], to url: URL, newline: Bool = true) throws {
        let handle = try FileHandle(forWritingTo: url)
        defer { try? handle.close() }
        try handle.seekToEnd()
        try handle.write(contentsOf: Data((lines.joined(separator: "\n") + (newline ? "\n" : "")).utf8))
    }

    private func time(_ n: Int) -> String { String(format: "2026-11-03T08:%02d:%02d.000Z", (n / 60) % 60, n % 60) }

    private func user(_ text: String, _ n: Int) -> String {
        #"{"type":"user","uuid":"u\#(n)","timestamp":"\#(time(n))","message":{"role":"user","content":\#(json(text))}}"#
    }
    private func assistant(_ text: String, _ n: Int) -> String {
        #"{"type":"assistant","uuid":"a\#(n)","timestamp":"\#(time(n))","message":{"role":"assistant","model":"claude-x","content":[{"type":"text","text":\#(json(text))}]}}"#
    }
    private func tool(_ n: Int) -> String {
        #"{"type":"assistant","uuid":"t\#(n)","timestamp":"\#(time(n))","message":{"role":"assistant","model":"claude-x","content":[{"type":"tool_use","id":"x\#(n)","name":"Bash","input":{"command":"ls"}}]}}"#
    }
    private func json(_ text: String) -> String {
        String(data: try! JSONSerialization.data(withJSONObject: [text]), encoding: .utf8)!.dropFirst().dropLast().description
    }
    private func padding(_ n: Int) -> String { String(repeating: "x", count: 600) + " \(n)" }

    /// user, tools, assistant, repeated: the shape of a real transcript, large enough to need several pages.
    private func bigTranscript(rounds: Int) throws -> URL {
        let url = file()
        var lines: [String] = []
        for i in 0..<rounds {
            lines.append(user("question \(i) " + padding(i), i * 4))
            lines.append(tool(i * 4 + 1))
            lines.append(tool(i * 4 + 2))
            lines.append(assistant("answer \(i) " + padding(i), i * 4 + 3))
        }
        try append(lines, to: url)
        return url
    }

    func testHistoryPagesBackToTheFirstLineWithoutGapsOrRepeats() throws {
        let url = try bigTranscript(rounds: 1500)   // about 1 MB+ of text, 4500 messages
        let size = try FileManager.default.attributesOfItem(atPath: url.path)[.size] as! UInt64
        XCTAssertGreaterThan(size, 1_000_000)
        let everything = ChatTranscript.parse(sourceLines: try XCTUnwrap(ChatTranscript.readWindow(url: url, from: 0, to: nil)).lines, isMain: true, path: "x", limit: .max).messages
        XCTAssertEqual(everything.count, 4500)

        let cache = ChatTranscriptCache()
        var collected = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 100))
        var all = collected.messages
        var pages = 0
        while collected.hasMore {
            let next = try XCTUnwrap(cache.page(at: url, isMain: true, before: all.first!.id, limit: 100))
            XCTAssertLessThanOrEqual(next.messages.count, 100)
            all = next.messages + all
            collected = ChatTranscriptCache.Page(messages: next.messages, hasMore: next.hasMore, model: nil)
            pages += 1
            XCTAssertLessThan(pages, 100)
        }
        XCTAssertEqual(all.map(\.id), everything.map(\.id), "the pages join up exactly to the whole transcript")
        XCTAssertEqual(all.first?.text.hasPrefix("question 0"), true, "it reached the very first message")
        XCTAssertGreaterThan(pages, 20)
    }

    func testAnUnknownBeforeIdIsNotFound() throws {
        let url = try bigTranscript(rounds: 5)
        XCTAssertNil(ChatTranscriptCache().page(at: url, isMain: true, before: UUID(), limit: 10))
    }

    func testAGrowingFileIsReadFromWhereItChanged() throws {
        let url = file()
        try append([user("one", 1), assistant("reply", 2)], to: url)
        let cache = ChatTranscriptCache()
        XCTAssertEqual(cache.latest(at: url, isMain: true, limit: 10)?.messages.map(\.text), ["one", "reply"])

        // The assistant keeps talking (the last message grows), then a tool row and a new user message come in.
        try append([assistant("more", 3), tool(4), user("two", 5)], to: url)
        let messages = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 10)).messages
        XCTAssertEqual(messages.map(\.text), ["one", "reply\n\nmore", "Ran 1 shell command", "two"])
        let whole = ChatTranscript.parse(sourceLines: try XCTUnwrap(ChatTranscript.readWindow(url: url, from: 0, to: nil)).lines, isMain: true, path: "x").messages
        XCTAssertEqual(messages.map(\.id), whole.map(\.id), "same result as reading everything again")
    }

    func testALastLineStillBeingWrittenIsPickedUpOnceComplete() throws {
        let url = file()
        try append([user("hello", 1)], to: url)
        let cache = ChatTranscriptCache()
        XCTAssertEqual(cache.latest(at: url, isMain: true, limit: 10)?.messages.count, 1)
        let reply = assistant("half written", 2)
        try append([String(reply.prefix(40))], to: url, newline: false)
        XCTAssertEqual(cache.latest(at: url, isMain: true, limit: 10)?.messages.count, 1, "a partial line is not a message yet")
        try append([String(reply.dropFirst(40))], to: url)
        XCTAssertEqual(cache.latest(at: url, isMain: true, limit: 10)?.messages.map(\.text), ["hello", "half written"])
    }

    func testNewerThanAnIdAndAReset() throws {
        let url = file()
        try append([user("a", 1), assistant("b", 2), user("c", 3), assistant("d", 4)], to: url)
        let cache = ChatTranscriptCache()
        let all = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 10)).messages
        let newer = try XCTUnwrap(cache.newer(at: url, isMain: true, after: all[1].id, limit: 10))
        XCTAssertTrue(newer.found)
        XCTAssertEqual(newer.messages.map(\.text), ["c", "d"])
        XCTAssertEqual(try XCTUnwrap(cache.newer(at: url, isMain: true, after: all[3].id, limit: 10)).messages.count, 0)
        XCTAssertFalse(try XCTUnwrap(cache.newer(at: url, isMain: true, after: UUID(), limit: 10)).found)
    }

    // MARK: Queued messages

    private func enqueue(_ text: String, _ n: Int) -> String {
        #"{"type":"queue-operation","operation":"enqueue","timestamp":"\#(time(n))","sessionId":"s","content":\#(json(text))}"#
    }
    private func dequeue(_ n: Int) -> String { #"{"type":"queue-operation","operation":"dequeue","timestamp":"\#(time(n))","sessionId":"s"}"# }
    private func remove(_ text: String, _ n: Int) -> String {
        #"{"type":"queue-operation","operation":"remove","timestamp":"\#(time(n))","sessionId":"s","content":\#(json(text)),"reason":"absorbed_mid_turn","commandUuid":"c\#(n)"}"#
    }
    private func queuedAttachment(_ prompt: String, _ n: Int, origin: String = "human") -> String {
        #"{"type":"attachment","uuid":"q\#(n)","timestamp":"\#(time(n))","attachment":{"type":"queued_command","prompt":\#(json(prompt)),"origin":{"kind":"\#(origin)"},"humanTurn":true}}"#
    }

    private func parse(_ lines: [String]) -> [ChatMessage] {
        ChatTranscript.parse(lines: lines.map { Substring($0) }, isMain: true, path: "x").messages
    }

    func testAMessageTypedWhileTheAgentWorksShowsAtOnceAsWaiting() {
        let messages = parse([user("start", 1), tool(2), enqueue("and also this", 3)])
        XCTAssertEqual(messages.map(\.text), ["start", "Ran 1 shell command", "and also this"])
        XCTAssertEqual(messages.map(\.queued), [false, false, true])
        XCTAssertEqual(messages.last?.role, .user)
    }

    func testItBecomesAnOrdinaryMessageOnceDelivered_WithoutAppearingTwice() {
        let waiting = parse([user("start", 1), tool(2), enqueue("and also this", 3)])
        let delivered = parse([user("start", 1), tool(2), enqueue("and also this", 3), remove("and also this", 4),
                               queuedAttachment("and also this", 4), assistant("ok", 5)])
        XCTAssertEqual(delivered.map(\.text), ["start", "Ran 1 shell command", "and also this", "ok"])
        XCTAssertEqual(delivered.filter { $0.text == "and also this" }.count, 1)
        XCTAssertFalse(delivered.contains { $0.queued })
        XCTAssertNotEqual(waiting.last?.id, delivered[2].id, "a different message once delivered (it moved into the turn)")
    }

    func testTheAttachmentAloneAlsoDelivers() {
        let messages = parse([user("start", 1), enqueue("later", 2), queuedAttachment("later", 3), assistant("ok", 4)])
        XCTAssertEqual(messages.map(\.text), ["start", "later", "ok"])
        XCTAssertEqual(messages.map(\.queued), [false, false, false])
    }

    func testAnIdleQueueRoundTripIsJustTheUserMessage() {
        let messages = parse([enqueue("hello", 1), dequeue(1), user("hello", 2), assistant("hi", 3)])
        XCTAssertEqual(messages.map(\.text), ["hello", "hi"])
        XCTAssertEqual(messages.map(\.queued), [false, false])
    }

    func testInjectedAndNonHumanQueueEntriesAreNotShown() {
        let messages = parse([enqueue("<task-notification>done</task-notification>", 1), remove("<task-notification>done</task-notification>", 2),
                              queuedAttachment("<task-notification>done</task-notification>", 2), queuedAttachment("from a hook", 3, origin: "system")])
        XCTAssertTrue(messages.isEmpty)
    }

    func testTwoWaitingMessagesKeepTheirOrderAndLeaveOneByOne() {
        let both = parse([user("go", 1), enqueue("first", 2), enqueue("second", 3)])
        XCTAssertEqual(both.suffix(2).map(\.text), ["first", "second"])
        XCTAssertTrue(both.suffix(2).allSatisfy(\.queued))
        let oneDelivered = parse([user("go", 1), enqueue("first", 2), enqueue("second", 3), remove("first", 4), queuedAttachment("first", 4)])
        XCTAssertEqual(oneDelivered.map(\.text), ["go", "first", "second"])
        XCTAssertEqual(oneDelivered.map(\.queued), [false, false, true])
    }

    func testWaitingMessagesSurviveIncrementalReads() throws {
        let url = file()
        try append([user("go", 1), tool(2), enqueue("wait for it", 3)], to: url)
        let cache = ChatTranscriptCache()
        XCTAssertEqual(cache.latest(at: url, isMain: true, limit: 10)?.messages.map(\.queued), [false, false, true])
        try append([remove("wait for it", 4), queuedAttachment("wait for it", 4), assistant("done", 5)], to: url)
        let messages = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 10)).messages
        XCTAssertEqual(messages.map(\.text), ["go", "Ran 1 shell command", "wait for it", "done"])
        XCTAssertFalse(messages.contains { $0.queued })
    }

    func testTheQueuedFlagTravelsToThePhone() {
        let queued = parse([enqueue("hold on", 1)])
        XCTAssertTrue(ChatMessageDTO(queued[0]).queued)
        XCTAssertFalse(ChatMessageDTO(parse([user("x", 1)])[0]).queued)
    }

    // MARK: chat.get through the router

    func testChatGetAfterAndBeforeThroughTheBridge() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login()
        rig.backend.sessionList = [makeSession("s1")]
        let messages = (0..<10).map { makeMessage("m\($0)") }
        rig.backend.chat = messages

        await rig.router.handle(#"{"id":2,"op":"chat.get","sessionId":"s1","after":"\#(messages[6].id.uuidString)","limit":50}"#, from: connection)
        var answer = try io.lastJSON().dataObject
        XCTAssertEqual((answer["messages"] as? [[String: Any]])?.map { $0["text"] as? String }, ["m7", "m8", "m9"])
        XCTAssertNil(answer["reset"])

        await rig.router.handle(#"{"id":3,"op":"chat.get","sessionId":"s1","after":"\#(UUID().uuidString)","limit":4}"#, from: connection)
        answer = try io.lastJSON().dataObject
        XCTAssertEqual(answer["reset"] as? Bool, true)
        XCTAssertEqual((answer["messages"] as? [[String: Any]])?.count, 4)
        XCTAssertEqual(answer["hasMore"] as? Bool, true)

        await rig.router.handle(#"{"id":4,"op":"chat.get","sessionId":"s1","before":"\#(messages[3].id.uuidString)","limit":2}"#, from: connection)
        answer = try io.lastJSON().dataObject
        XCTAssertEqual((answer["messages"] as? [[String: Any]])?.map { $0["text"] as? String }, ["m1", "m2"])
        XCTAssertEqual(answer["hasMore"] as? Bool, true)
    }

    func testSubscribeWithTheLastKnownMessageLetsTheHubPushWhatCameAfter() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login()
        rig.backend.sessionList = [makeSession("s1")]
        let messages = (0..<5).map { makeMessage("m\($0)") }
        rig.backend.chat = messages
        await rig.router.handle(#"{"id":2,"op":"subscribe","sessionId":"s1","lastMessageId":"\#(messages[2].id.uuidString)"}"#, from: connection)
        await rig.core.hub.tick(backend: rig.backend)
        let pushed = try io.texts.compactMap { try JSONSerialization.jsonObject(with: Data($0.utf8)) as? [String: Any] }.first { $0["event"] as? String == "chat" }
        XCTAssertEqual((pushed?["messages"] as? [[String: Any]])?.map { $0["text"] as? String }, ["m3", "m4"])
    }
}

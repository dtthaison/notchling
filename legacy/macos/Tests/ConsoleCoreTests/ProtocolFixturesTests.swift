import XCTest
@testable import ConsoleCore

/// Pins the written protocol (`protocol/`) to the real bridge. Everything the Mac sends is captured from the real
/// `BridgeRouter`, `BridgeHub` and `BridgeWire` encoders running against a canned backend and compared with
/// `protocol/fixtures/<Name>.json` (key order doesn't matter). Everything only the phone sends
/// (`<Name>Request*.json`, written by hand from `apps/flutter/lib/core/net/bridge_client.dart`) is fed to the real
/// router, which must accept it. To rewrite the captured files after a protocol change:
///
///     PROTOCOL_FIXTURES_WRITE=1 swift test --filter ProtocolFixturesTests
///
/// The only values replaced before comparing are the random ones (device id, token, upload id).
final class ProtocolFixturesTests: XCTestCase {
    // MARK: Fixed ids the request fixtures name

    static let sessionA = "5b6d9a30-6f0e-4d0a-9b57-0c1f3a2e8d11"   // Claude Code, owned terminal, permission pending
    static let sessionB = "c3a1e5f2-0b4d-4f6e-8a7c-2d9e1b0f4a65"   // view-only, original still running
    static let sessionC = "e8f0a7b4-3c2d-4a19-b6e5-7f1d0c9a2b38"   // background job, question pending
    static let sessionD = "codex:0199e0c5-7a31-7c52-9d4e-5b8a1f2c3d60"
    static let sessionE = "1f4b8c27-9d3e-4a60-8b15-6c0e2a7d9f43"   // view-only, can be taken over

    static func messageId(_ n: Int) -> UUID { UUID(uuidString: "0A000000-0000-4000-8000-00000000000\(n)")! }

    private static let root = URL(fileURLWithPath: #filePath)
        .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
        .deletingLastPathComponent().deletingLastPathComponent()
    static let fixtures = root.appendingPathComponent("protocol/fixtures")
    private static let writing = ProcessInfo.processInfo.environment["PROTOCOL_FIXTURES_WRITE"] == "1"

    // MARK: The canned backend

    final class Backend: BridgeBackend, @unchecked Sendable {
        private let lock = NSLock()
        var sessionList: [Session] = []
        var chat: [ChatMessage] = []
        var prompts: [String: PendingPrompt] = [:]
        private(set) var sent: [(String, String)] = []
        private(set) var interrupted: [String] = []
        private(set) var answers: [(String, PromptResponse)] = []
        private(set) var newSessions: [(URL, String?)] = []
        private(set) var phoneStarts: [PhoneOptions] = []
        private(set) var takeovers: [(String, Bool)] = []

        func sessions() async -> [Session] { lock.withLock { sessionList } }
        func chatMessages(sessionId: String, subagentId: String?) async throws -> [ChatMessage] { lock.withLock { chat } }
        func chatImage(sessionId: String, imageId: String, maxWidth: Int) async throws -> Data { Data([0xFF, 0xD8, 0xFF, 0xD9]) }
        func commands(sessionId: String) async -> [CommandInfo] {
            [CommandInfo(name: "compact", description: "Summarise the conversation to free context", argumentHint: "[instructions]"),
             CommandInfo(name: "clear", description: "Start a new conversation")]
        }
        func viz(sessionId: String, includeEntity: Bool) async throws -> BridgeViz {
            var specs: [String: VizSpec] = [
                "flow": VizSpec(kind: .flow, flow: FlowSpec(
                    branch: "main", diffStat: "2 files changed, 14 insertions(+), 3 deletions(-)",
                    files: [FlowFile(path: "/Users/example/src/app/main.swift", name: "main.swift", count: 3)],
                    turns: [FlowTurn(index: 1, prompt: "Fix the crash on launch", calls: [
                        FlowCall(tool: "Read", target: "main.swift", subagent: false, edit: false, time: 1_793_541_400_000),
                        FlowCall(tool: "Edit", target: "main.swift", subagent: false, edit: true, time: nil)])],
                    totalCalls: 2)),
                "diagram": VizSpec(kind: .diagram, diagram: DiagramSpec(blocks: [
                                       DiagramBlock(id: "d-1", label: "Launch sequence", source: "sequenceDiagram\n  A->>B: hi",
                                                    origin: "assistant", time: 1_793_541_300_000)])),
                "chart": VizSpec(kind: .chart, chart: ChartSpec(
                    tokens: [UsagePoint(time: 1_793_541_300_000, input: 1200, output: 340, cacheCreation: 0, cacheRead: 9000)],
                    totals: UsageTotals(input: 1200, output: 340, cacheCreation: 0, cacheRead: 9000),
                    tools: [ChartCount(name: "Read", count: 4), ChartCount(name: "Edit", count: 2)],
                    models: [ModelCount(model: "claude-opus-5-5", count: 6)])),
            ]
            if includeEntity {
                specs["entity"] = VizSpec(kind: .entity, entity: EntitySpec(
                    mermaid: "erDiagram\n  USER ||--o{ ORDER : places",
                    tables: [EntityTable(id: "USER", name: "user", columns: 3), EntityTable(id: "ORDER", name: "order", columns: 4)],
                    links: [EntityLink(from: "USER", to: "ORDER", inferred: false)],
                    files: [EntityFile(path: "db/schema.sql", tables: 2)],
                    tableCount: 2, relationCount: 1, inferredCount: 0, warnings: ["no primary key on order_item"]))
            }
            return BridgeViz(suggestedTab: includeEntity ? "entity" : "flow", webURL: "http://localhost:3000", specs: specs,
                             webCandidates: ["http://localhost:3000", "http://localhost:5173"])
        }
        func webSnapshot(url: URL, width: Int) async throws -> Data { Data([0xFF, 0xD8, 0xFF, 0xD9]) }
        func sendMessage(_ text: String, to session: Session) async throws { lock.withLock { sent.append((session.id, text)) } }
        func sendNow(session: Session) async throws { lock.withLock { interrupted.append(session.id) } }
        func pendingPrompt(requestId: String) async -> PendingPrompt? { lock.withLock { prompts[requestId] } }
        func respond(to prompt: PendingPrompt, with response: PromptResponse) async throws { lock.withLock { answers.append((prompt.id, response)) } }
        func newSession(cwd: URL, model: String?) async throws { lock.withLock { newSessions.append((cwd, model)) } }
        func takeOver(session: Session, stopOriginal: Bool) async throws -> TakeoverOutcome {
            lock.withLock { takeovers.append((session.id, stopOriginal)) }
            return TakeoverOutcome(sessionId: "pty:7C3A91E2", stoppedPid: stopOriginal ? 4242 : nil)
        }
        func recentProjects() async -> [String] { ["/Users/example/src/app", "/Users/example/src/notes"] }
        func requestApproval(deviceName: String, platform: String) async -> Bool { true }
        func startPhone(connection: BridgeConnection, options: PhoneOptions) async throws { lock.withLock { phoneStarts.append(options) } }
        func stopPhone(connection: BridgeConnection) async {}
    }

    static let permission = PendingPrompt(id: "req-0001", sessionId: sessionA, kind: .permission, toolName: "Bash",
                                          summary: "rm -rf build", created: fixtureNow)
    static let question = PendingPrompt(
        id: "req-0002", sessionId: sessionC,
        kind: .question([PromptQuestion(header: "Database", question: "Which database should the service use?",
                                        options: ["SQLite", "Postgres"], multiSelect: false)]),
        toolName: "AskUserQuestion", summary: "Which database should the service use?", created: fixtureNow)

    static func makeSessions() -> [Session] {
        let url = URL(fileURLWithPath: "/Users/example/.claude/projects/app/\(sessionA).jsonl")
        var a = Session(id: sessionA, pid: 4100, name: "Fix the launch crash", project: "app", cwd: "/Users/example/src/app",
                        entrypoint: .cli, kind: "interactive", state: .needsInput, statusUpdatedAt: fixtureNow,
                        startedAt: fixtureNow.addingTimeInterval(-3600), transcriptURL: url,
                        subagents: [Subagent(id: "agent-a1b2c3", type: "Explore", description: "Find the crash site", model: "haiku",
                                             depth: 1, parentToolUseId: "toolu_01", finished: false,
                                             lastActivity: fixtureNow.addingTimeInterval(-5), transcriptURL: url, state: .running)],
                        pending: permission, control: .consolePTY)
        a.activity = "Editing main.swift"
        a.facts = SessionFactsInfo(contextTokens: 52_000, contextWindow: 400_000, model: "claude-opus-5-5", limits: [
            LimitInfo(label: "5h", percent: 42, resetsAt: fixtureNow.addingTimeInterval(7200)),
            LimitInfo(label: "Week", percent: 12.5)])
        a.lastMessage = LastMessageInfo(id: messageId(6).uuidString, text: "The crash was a nil optional in main.swift.")

        var b = Session(id: sessionB, pid: 3900, name: "Review the diff", project: "notes", cwd: "/Users/example/src/notes",
                        entrypoint: .desktop, kind: "interactive", state: .running, statusUpdatedAt: fixtureNow,
                        startedAt: fixtureNow.addingTimeInterval(-7200), transcriptURL: nil, control: .viewOnly)
        b.takeover = .stoppable(busy: true)

        var c = Session(id: sessionC, pid: nil, name: "Pick a database", project: "app", cwd: "/Users/example/src/app",
                        entrypoint: .other("claude-vscode"), kind: "background", state: .needsInput, statusUpdatedAt: fixtureNow,
                        startedAt: fixtureNow.addingTimeInterval(-600), transcriptURL: nil, pending: question,
                        control: .job("job-91ab"))
        c.takeover = nil

        var d = Session(id: sessionD, pid: nil, name: "Port the parser", project: "parser", cwd: "/Users/example/src/parser",
                        entrypoint: .other("codex"), kind: "codex", state: .done, statusUpdatedAt: fixtureNow.addingTimeInterval(-90),
                        startedAt: fixtureNow.addingTimeInterval(-1800), transcriptURL: nil, control: .job("codex"))
        d.tool = "codex"

        var e = Session(id: sessionE, pid: nil, name: "Old experiment", project: "lab", cwd: "/Users/example/src/lab",
                        entrypoint: .sdk, kind: "interactive", state: .exited, statusUpdatedAt: fixtureNow.addingTimeInterval(-86_400),
                        startedAt: fixtureNow.addingTimeInterval(-90_000), transcriptURL: nil, control: .viewOnly)
        e.takeover = .available
        return [a, b, c, d, e]
    }

    static func makeChat() -> [ChatMessage] {
        func message(_ n: Int, _ role: ChatRole, _ text: String, at offset: Double, build: (inout ChatMessage) -> Void = { _ in }) -> ChatMessage {
            var message = ChatMessage(id: messageId(n), role: role, text: text, timestamp: fixtureNow.addingTimeInterval(offset), toolSummary: nil)
            build(&message)
            return message
        }
        return [
            message(1, .user, "The app crashes on launch. Can you look?", at: -300) {
                $0.images = [ImageRef(id: "b-4096-0", mediaType: "image/png", width: 1170, height: 2532, origin: .user,
                                      byteCount: 245_760, lineOffset: 4096, blockIndex: 0, path: nil)]
            },
            message(2, .assistant, "I'll read `main.swift` first.", at: -280),
            message(3, .tools, "", at: -270) {
                $0 = ChatMessage(id: messageId(3), role: .tools, text: "", timestamp: fixtureNow.addingTimeInterval(-270), toolSummary: "Read 1 file, edited 1 file")
                $0.steps = [
                    ToolStep(id: "toolu_01", name: "Read", summary: "main.swift", output: "import AppKit\n", path: "/Users/example/src/app/main.swift"),
                    ToolStep(id: "toolu_02", name: "Edit", summary: "main.swift", path: "/Users/example/src/app/main.swift",
                             diff: [DiffRow(.context, "let a = 1"), DiffRow(.removed, "let b = a!"), DiffRow(.added, "let b = a ?? 0")]),
                ]
            },
            message(4, .user, "!git status --short", at: -200) {
                $0.kind = .shell
                $0.output = " M main.swift\n"
            },
            message(5, .user, "Also add a test", at: -100) { $0.queued = true },
            message(6, .assistant, "The crash was a nil optional in main.swift.", at: -10),
        ]
    }

    static func compaction() -> ChatMessage {
        var message = ChatMessage(id: messageId(7), role: .assistant, text: "Conversation compacted.", timestamp: fixtureNow, toolSummary: nil)
        message.kind = .compaction
        return message
    }

    // MARK: A connected phone

    struct Phone {
        let rig: BridgeRig
        let backend: Backend
        let connection: BridgeConnection
        let io: RecordingIO
        let token: String
    }

    func makeBackend() -> Backend {
        let backend = Backend()
        backend.sessionList = Self.makeSessions()
        backend.chat = Self.makeChat()
        backend.prompts = [Self.permission.id: Self.permission, Self.question.id: Self.question]
        return backend
    }

    func makeRig(_ backend: Backend) throws -> BridgeRig {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("protocol-\(UUID().uuidString)")
        let home = root.appendingPathComponent("home")
        try FileManager.default.createDirectory(at: home.appendingPathComponent("src/app"), withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        return BridgeRig(core: BridgeCore(directory: root.appendingPathComponent("bridge"), backend: backend, home: home, now: { fixtureNow }),
                         backend: FakeBackend(), directory: root.appendingPathComponent("bridge"), home: home)
    }

    func connect() async throws -> Phone {
        let backend = makeBackend()
        let rig = try makeRig(backend)
        let (_, token) = try rig.core.devices.register(name: "Son's iPhone", platform: "ios")
        let (connection, io) = rig.connect()
        await rig.router.handle(#"{"id":1,"op":"hello","token":"\#(token)"}"#, from: connection)
        XCTAssertTrue(connection.isAuthenticated)
        return Phone(rig: rig, backend: backend, connection: connection, io: io, token: token)
    }

    // MARK: Fixture files

    /// Every fixture this test sent or compared. The last assertion checks the folder holds nothing else.
    private var touched = Set<String>()

    static func data(_ name: String) throws -> Data { try Data(contentsOf: fixtures.appendingPathComponent(name + ".json")) }

    /// The request fixture as the phone sends it, with `patch` replacing values that only exist at run time.
    func request(_ name: String, patch: [String: Any] = [:]) throws -> String {
        touched.insert(name)
        var object = try XCTUnwrap(try JSONSerialization.jsonObject(with: Self.data(name)) as? [String: Any], name)
        for (key, value) in patch { object[key] = value }
        return String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
    }

    /// Compares what the Mac sent with its fixture, or writes the fixture. `scrub` replaces random values first.
    func capture(_ name: String, _ wire: String, scrub: (inout [String: Any]) -> Void = { _ in }, file: StaticString = #filePath, line: UInt = #line) throws {
        touched.insert(name)
        var object = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(wire.utf8)) as? [String: Any], wire, file: file, line: line)
        scrub(&object)
        let url = Self.fixtures.appendingPathComponent(name + ".json")
        if Self.writing {
            try Data(Self.pretty(object).utf8).write(to: url)
            return
        }
        guard let existing = try? Data(contentsOf: url) else {
            return XCTFail("protocol/fixtures/\(name).json is missing. Run PROTOCOL_FIXTURES_WRITE=1 swift test --filter ProtocolFixturesTests", file: file, line: line)
        }
        let saved = try JSONSerialization.jsonObject(with: existing) as AnyObject
        XCTAssertTrue(saved.isEqual(object), "\(name).json differs from what the Mac sends. Run PROTOCOL_FIXTURES_WRITE=1 swift test --filter ProtocolFixturesTests\n\(Self.pretty(object))", file: file, line: line)
    }

    /// Sorted keys, two-space indent, `"key": value`: the same shape on every machine.
    static func pretty(_ value: Any, indent: String = "") -> String {
        let inner = indent + "  "
        switch value {
        case let dict as [String: Any]:
            if dict.isEmpty { return "{}" }
            let rows = dict.keys.sorted().map { "\(inner)\(scalar($0)): \(pretty(dict[$0] ?? NSNull(), indent: inner))" }
            return "{\n" + rows.joined(separator: ",\n") + "\n\(indent)}" + (indent.isEmpty ? "\n" : "")
        case let list as [Any]:
            if list.isEmpty { return "[]" }
            return "[\n" + list.map { inner + pretty($0, indent: inner) }.joined(separator: ",\n") + "\n\(indent)]"
        default:
            return scalar(value)
        }
    }

    private static func scalar(_ value: Any) -> String {
        guard let data = try? JSONSerialization.data(withJSONObject: value, options: [.fragmentsAllowed, .withoutEscapingSlashes]) else { return "null" }
        return String(decoding: data, as: UTF8.self)
    }

    // MARK: Sends and reads

    /// Sends one request through the router and returns the texts the Mac wrote for it.
    @discardableResult
    func send(_ phone: Phone, _ text: String) async -> [String] {
        let before = phone.io.texts.count
        await phone.rig.router.handle(text, from: phone.connection)
        return Array(phone.io.texts[before...])
    }

    func reply(_ texts: [String], file: StaticString = #filePath, line: UInt = #line) throws -> (text: String, object: [String: Any]) {
        let text = try XCTUnwrap(texts.last, "no reply", file: file, line: line)
        let object = try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any], text, file: file, line: line)
        return (text, object)
    }

    /// Sends a request fixture, expects `ok`, and captures the reply as `response`.
    @discardableResult
    func roundTrip(_ phone: Phone, _ requestName: String, patch: [String: Any] = [:], response: String? = nil,
                   scrub: (inout [String: Any]) -> Void = { _ in }, file: StaticString = #filePath, line: UInt = #line) async throws -> [String] {
        let texts = await send(phone, try request(requestName, patch: patch))
        let (text, object) = try reply(texts, file: file, line: line)
        XCTAssertTrue(object.isOK, "\(requestName): \(text)", file: file, line: line)
        if let response { try capture(response, text, scrub: scrub, file: file, line: line) }
        return texts
    }

    // MARK: Tests

    func testEveryFixtureIsCapturedOrAccepted() async throws {
        try await authentication()
        try await queries()
        try await actions()
        try await uploads()
        try await subscriptions()
        try await errors()
        try binaryFrames()

        let names = try FileManager.default.contentsOfDirectory(atPath: Self.fixtures.path)
            .filter { $0.hasSuffix(".json") }.map { String($0.dropLast(5)) }
        if !Self.writing {
            XCTAssertEqual(Set(names).subtracting(touched).sorted(), [], "fixtures nothing sends or compares")
        }
        XCTAssertEqual(touched.subtracting(names).sorted(), [], "fixtures this test needs")
    }

    private func authentication() async throws {
        let backend = makeBackend()
        let rig = try makeRig(backend)
        let (_, token) = try rig.core.devices.register(name: "Son's iPhone", platform: "ios")

        let (hello, helloIO) = rig.connect()
        await rig.router.handle(try request("HelloRequest", patch: ["token": token]), from: hello)
        XCTAssertTrue(hello.isAuthenticated)
        try capture("HelloResponse", helloIO.texts[0]) { object in
            var data = object["data"] as? [String: Any] ?? [:]
            var device = data["device"] as? [String: Any] ?? [:]
            device["id"] = "6D0F2A58-3B77-4C0E-9A41-5E8B1C7D2F90"
            data["device"] = device
            object["data"] = data
        }

        let (rejected, rejectedIO) = rig.connect(from: "192.168.1.30")
        await rig.router.handle(try request("HelloRequest"), from: rejected)
        try capture("ErrorResponse.unauthorized", rejectedIO.texts[0])

        let (pairing, pairingIO) = rig.connect(from: "192.168.1.31")
        let code = rig.core.codes.issue(now: fixtureNow).code
        await rig.router.handle(try request("PairRequest", patch: ["code": code]), from: pairing)
        XCTAssertTrue(pairing.isAuthenticated)
        try capture("PairResponse", pairingIO.texts[0]) { object in
            var data = object["data"] as? [String: Any] ?? [:]
            data["token"] = "tok-new-device-1111111111111111"
            data["deviceId"] = "9B3E7C14-2A60-4D85-8F1E-0C5D7A9B3E62"
            object["data"] = data
        }
    }

    private func queries() async throws {
        let phone = try await connect()
        try await roundTrip(phone, "PingRequest", response: "PingResponse")
        try await roundTrip(phone, "PingRequest.string-id", response: "PingResponse.string-id")
        try await roundTrip(phone, "SessionsListRequest", response: "SessionsListResponse")
        try await roundTrip(phone, "ChatGetRequest", response: "ChatGetResponse")
        try await roundTrip(phone, "ChatGetRequest.before", response: "ChatGetResponse.before")
        try await roundTrip(phone, "ChatGetRequest.after-gone", response: "ChatGetResponse.reset")
        try await roundTrip(phone, "ChatGetRequest.subagent")
        phone.backend.chat = [Self.compaction()]
        try await roundTrip(phone, "ChatGetRequest", response: "ChatGetResponse.compaction")
        try await roundTrip(phone, "CommandsListRequest", response: "CommandsListResponse")
        try await roundTrip(phone, "ProjectsRecentRequest", response: "ProjectsRecentResponse")
        try await roundTrip(phone, "VizGetRequest", response: "VizGetResponse")
        try await roundTrip(phone, "VizGetRequest.entity", response: "VizGetResponse.entity")

        // Two text frames for one request: the binary announcement, then `{sent: true}`.
        let snapshot = try await roundTrip(phone, "WebSnapshotRequest")
        XCTAssertEqual(snapshot.count, 2)
        try capture("WebSnapshotResponse", snapshot[0])
        try capture("SentResponse", snapshot[1])
        let byUrl = try await roundTrip(phone, "WebSnapshotRequest.url")
        XCTAssertEqual(byUrl.count, 2)
        let image = try await roundTrip(phone, "ChatImageRequest")
        XCTAssertEqual(image.count, 2)
        try capture("ChatImageResponse", image[0])
    }

    private func actions() async throws {
        let phone = try await connect()
        let home = phone.rig.home.appendingPathComponent("src/app").path
        try await roundTrip(phone, "SessionMessageRequest", response: "SessionMessageResponse")
        XCTAssertEqual(phone.backend.sent.map(\.1), ["Run the tests again"])
        try await roundTrip(phone, "SessionSendNowRequest", response: "SessionSendNowResponse")
        XCTAssertEqual(phone.backend.interrupted, [Self.sessionA])

        try await roundTrip(phone, "PromptAnswerRequest", response: "PromptAnswerResponse")
        try await roundTrip(phone, "PromptAnswerRequest.deny")
        try await roundTrip(phone, "PromptAnswerRequest.choice")
        try await roundTrip(phone, "PromptAnswerRequest.label")
        try await roundTrip(phone, "PromptAnswerRequest.text")
        XCTAssertEqual(phone.backend.answers.map(\.0), ["req-0001", "req-0001", "req-0002", "req-0002", "req-0002"])

        try await roundTrip(phone, "SessionNewRequest", patch: ["cwd": home], response: "SessionNewResponse")
        try await roundTrip(phone, "SessionNewRequest.no-model", patch: ["cwd": home])
        XCTAssertEqual(phone.backend.newSessions.map(\.1), ["opus", nil])

        try await roundTrip(phone, "SessionTakeoverRequest", response: "SessionTakeoverResponse")
        try await roundTrip(phone, "SessionTakeoverRequest.plain", response: "SessionTakeoverResponse.plain")
        XCTAssertEqual(phone.backend.takeovers.map(\.1), [true, false])
    }

    private func uploads() async throws {
        let phone = try await connect()
        let begin = try await roundTrip(phone, "SessionImageBeginRequest", response: "SessionImageBeginResponse") { object in
            var data = object["data"] as? [String: Any] ?? [:]
            data["uploadId"] = "upload-example"
            object["data"] = data
        }
        let uploadId = try XCTUnwrap(try reply(begin).object.dataObject["uploadId"] as? String)
        try await roundTrip(phone, "SessionImageChunkRequest", patch: ["uploadId": uploadId], response: "SessionImageChunkResponse")
        try await roundTrip(phone, "SessionImageEndRequest", patch: ["uploadId": uploadId], response: "SessionImageEndResponse")
        XCTAssertTrue(try XCTUnwrap(phone.backend.sent.last).1.hasPrefix("What is this? "), "the caption then the saved file's path")
    }

    private func subscriptions() async throws {
        let phone = try await connect()
        // `lastMessageId` is the second message, so the next tick pushes the ones after it as a `chat` event.
        let subscribed = try await roundTrip(phone, "SubscribeRequest", response: "SubscribeResponse")
        XCTAssertEqual(subscribed.count, 2, "the first `sessions` snapshot, then the answer")
        try capture("SessionsEvent", subscribed[0])

        await phone.rig.core.hub.tick(backend: phone.backend)
        let first = phone.io.texts.count
        let chatText = try XCTUnwrap(phone.io.texts.last)
        XCTAssertTrue(chatText.contains("\"event\":\"chat\""), chatText)
        try capture("ChatEvent", chatText)

        // The permission is answered elsewhere: a `sessions` update and a resolved `prompt`.
        phone.backend.sessionList = Self.makeSessions().map { session in
            var copy = session
            if copy.id == Self.sessionA { copy.pending = nil }
            return copy
        }
        await phone.rig.core.hub.tick(backend: phone.backend)
        let resolved = Array(phone.io.texts[first...])
        let resolvedText = try XCTUnwrap(resolved.first { $0.contains("\"resolved\":true") })
        try capture("PromptResolvedEvent", resolvedText)

        // A new question appears on a session.
        let fresh = PendingPrompt(id: "req-0003", sessionId: Self.sessionA, kind: .permission, toolName: "Write",
                                  summary: "Write /Users/example/src/app/notes.md", created: fixtureNow)
        phone.backend.sessionList = Self.makeSessions().map { session in
            var copy = session
            if copy.id == Self.sessionA { copy.pending = fresh }
            return copy
        }
        let beforePrompt = phone.io.texts.count
        await phone.rig.core.hub.tick(backend: phone.backend)
        let promptText = try XCTUnwrap(phone.io.texts[beforePrompt...].first { $0.contains("\"event\":\"prompt\"") })
        try capture("PromptEvent", promptText)

        XCTAssertEqual(phone.rig.core.hub.notifyOpen(), 1)
        try capture("OpenEvent", try XCTUnwrap(phone.io.texts.last))

        let unsubscribed = try await roundTrip(phone, "UnsubscribeRequest", response: "UnsubscribeResponse")
        XCTAssertEqual(unsubscribed.count, 1)

        try await roundTrip(phone, "SubscribeRequest.minimal")
        try await roundTrip(phone, "SubscribeRequest.phone")
        XCTAssertEqual(phone.backend.phoneStarts, [PhoneOptions(deviceWindowId: 1234, fps: 10, width: 400)])
    }

    private func errors() async throws {
        let phone = try await connect()
        // invalid: an op the Mac does not know.
        try capture("ErrorResponse.invalid", try reply(await send(phone, #"{"id":9,"op":"nope"}"#)).text)
        // not_found: a session that is not there.
        let missing = try request("ChatGetRequest", patch: ["sessionId": "00000000-0000-4000-8000-000000000000"])
        try capture("ErrorResponse.not_found", try reply(await send(phone, missing)).text)
        // view_only: a message to a session the Mac cannot type into.
        let viewOnly = try request("SessionMessageRequest", patch: ["sessionId": Self.sessionB])
        try capture("ErrorResponse.view_only", try reply(await send(phone, viewOnly)).text)
        // busy: the second interrupt inside the gate's three seconds.
        try await roundTrip(phone, "SessionSendNowRequest")
        try capture("ErrorResponse.busy", try reply(await send(phone, try request("SessionSendNowRequest"))).text)
        // No id: a frame that is not JSON.
        try capture("ErrorResponse.no-id", try reply(await send(phone, "not json")).text)
        // The two codes only the live backend raises, through the same encoder.
        try capture("ErrorResponse.not_reachable", BridgeWire.error(id: 9, BridgeFailure(.notReachable, "localhost:3000 is not running on the Mac.")))
        try capture("ErrorResponse.internal", BridgeWire.error(id: 9, BridgeFailure(.internal, "Internal error on the Mac.")))
    }

    /// The three binary frames after a JSON answer; the layouts `protocol/README.md` documents.
    private func binaryFrames() throws {
        let jpeg = Data([0xFF, 0xD8, 0xFF, 0xD9])
        let image = [UInt8](PhoneFrame.encodeImage(requestId: 0x0102_0304, jpeg: jpeg))
        XCTAssertEqual(image, Array("IMG1".utf8) + [1, 2, 3, 4] + [0xFF, 0xD8, 0xFF, 0xD9])
        let web = [UInt8](PhoneFrame.encodeSnapshot(requestId: 0x0102_0304, jpeg: jpeg))
        XCTAssertEqual(web, Array("WEB1".utf8) + [1, 2, 3, 4] + [0xFF, 0xD8, 0xFF, 0xD9])
        let stream = [UInt8](PhoneFrame.encode(sequence: 7, jpeg: jpeg))
        XCTAssertEqual(stream, Array("PHN1".utf8) + [0, 0, 0, 7] + [0xFF, 0xD8, 0xFF, 0xD9])
    }
}

import XCTest
@testable import ConsoleCore

final class RecordingIO: BridgeConnectionIO, @unchecked Sendable {
    private let lock = NSLock()
    private var _texts: [String] = []
    private var _binaries: [Data] = []
    private var _closed = false
    private var held: [@Sendable () -> Void] = []
    /// When true, binary completions are kept until `releaseBinary()`, so the frame stays "in flight".
    var holdBinaryCompletions = false

    var texts: [String] { lock.withLock { _texts } }
    var binaries: [Data] { lock.withLock { _binaries } }
    var isClosed: Bool { lock.withLock { _closed } }

    func sendText(_ text: String) { lock.withLock { _texts.append(text) } }
    func sendBinary(_ data: Data, completion: @escaping @Sendable () -> Void) {
        lock.lock(); _binaries.append(data)
        let hold = holdBinaryCompletions
        if hold { held.append(completion) }
        lock.unlock()
        if !hold { completion() }
    }
    func close() { lock.withLock { _closed = true } }
    func releaseBinary() {
        let pending = lock.withLock { () -> [@Sendable () -> Void] in let copy = held; held = []; return copy }
        pending.forEach { $0() }
    }

    func json(_ index: Int) throws -> [String: Any] {
        let text = texts[index]
        return try XCTUnwrap(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any], text)
    }

    func lastJSON() throws -> [String: Any] { try json(texts.count - 1) }
}

extension Dictionary where Key == String, Value == Any {
    var errorCode: String? { (self["error"] as? [String: Any])?["code"] as? String }
    var isOK: Bool { self["ok"] as? Bool == true }
    var dataObject: [String: Any] { self["data"] as? [String: Any] ?? [:] }
}

final class FakeBackend: BridgeBackend, @unchecked Sendable {
    private let lock = NSLock()
    var sessionList: [Session] = []
    var chat: [ChatMessage] = []
    var prompts: [String: PendingPrompt] = [:]
    var approve = true
    var sendDelay: TimeInterval = 0
    var phoneError: BridgeFailure?
    var snapshot = Data([0xFF, 0xD8, 0xFF, 0xD9])
    var commandList: [CommandInfo] = []
    var vizResult = BridgeViz(suggestedTab: "flow", webURL: "http://localhost:3000", specs: [:])

    private(set) var sent: [(String, String)] = []
    private(set) var responses: [(String, PromptResponse)] = []
    private(set) var newSessions: [(URL, String?)] = []
    private(set) var phoneStarts: [PhoneOptions] = []
    private(set) var approvalCalls = 0
    private(set) var sessionCalls = 0

    func sessions() async -> [Session] { lock.withLock { sessionCalls += 1; return sessionList } }
    func chatMessages(sessionId: String, subagentId: String?) async throws -> [ChatMessage] { lock.withLock { chat } }
    func commands(sessionId: String) async -> [CommandInfo] { lock.withLock { commandList } }
    func viz(sessionId: String, includeEntity: Bool) async throws -> BridgeViz { vizResult }
    func webSnapshot(url: URL, width: Int) async throws -> Data { snapshot }
    func sendMessage(_ text: String, to session: Session) async throws {
        if sendDelay > 0 { try await Task.sleep(nanoseconds: UInt64(sendDelay * 1_000_000_000)) }
        lock.withLock { sent.append((session.id, text)) }
    }
    private(set) var interrupted: [String] = []
    func sendNow(session: Session) async throws { lock.withLock { interrupted.append(session.id) } }
    func pendingPrompt(requestId: String) async -> PendingPrompt? { lock.withLock { prompts[requestId] } }
    func respond(to prompt: PendingPrompt, with response: PromptResponse) async throws { lock.withLock { responses.append((prompt.id, response)) } }
    func newSession(cwd: URL, model: String?) async throws { lock.withLock { newSessions.append((cwd, model)) } }
    var takeoverResult: Result<TakeoverOutcome, BridgeFailure> = .success(TakeoverOutcome(sessionId: "resumed-id"))
    private(set) var takeoverCalls: [String] = []
    private(set) var takeoverStopFlags: [Bool] = []
    func takeOver(session: Session, stopOriginal: Bool) async throws -> TakeoverOutcome {
        lock.withLock { takeoverCalls.append(session.id); takeoverStopFlags.append(stopOriginal) }
        return try takeoverResult.get()
    }
    func recentProjects() async -> [String] { ["/a", "/b"] }
    func requestApproval(deviceName: String, platform: String) async -> Bool { lock.withLock { approvalCalls += 1 }; return approve }
    func startPhone(connection: BridgeConnection, options: PhoneOptions) async throws {
        if let phoneError { throw phoneError }
        lock.withLock { phoneStarts.append(options) }
    }
    func stopPhone(connection: BridgeConnection) async {}
}

func makeSession(_ id: String, control: SessionControl = .consolePTY, pending: PendingPrompt? = nil, state: SessionState = .running) -> Session {
    var session = Session(id: id, pid: 1, name: "Session \(id)", project: "p", cwd: "/p", entrypoint: .cli, kind: "interactive",
                          state: state, statusUpdatedAt: fixtureNow, startedAt: fixtureNow, transcriptURL: nil, control: control)
    session.pending = pending
    return session
}

func makeMessage(_ text: String, role: ChatRole = .user) -> ChatMessage {
    ChatMessage(id: UUID(), role: role, text: text, timestamp: fixtureNow, toolSummary: nil)
}

/// A bridge core on a temp folder with a fake backend.
struct BridgeRig {
    let core: BridgeCore
    let backend: FakeBackend
    let directory: URL
    let home: URL

    var router: BridgeRouter { core.router }

    func connect(from address: String = "192.168.1.20") -> (BridgeConnection, RecordingIO) {
        let io = RecordingIO()
        let connection = BridgeConnection(remoteAddress: address, io: io)
        core.hub.add(connection)
        return (connection, io)
    }

    /// Pairs a device and returns a connection that is already logged in, plus its token.
    func login(name: String = "test-phone", from address: String = "192.168.1.20") async throws -> (BridgeConnection, RecordingIO, String) {
        let (device, token) = try core.devices.register(name: name, platform: "ios")
        _ = device
        let (connection, io) = connect(from: address)
        await router.handle(#"{"id":1,"op":"hello","token":"\#(token)"}"#, from: connection)
        XCTAssertTrue(connection.isAuthenticated)
        return (connection, io, token)
    }
}

extension XCTestCase {
    func makeBridgeRig(clock: @escaping () -> Date = Date.init) throws -> BridgeRig {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("bridge-\(UUID().uuidString)")
        let directory = root.appendingPathComponent("bridge")
        let home = root.appendingPathComponent("home")
        try FileManager.default.createDirectory(at: home, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let backend = FakeBackend()
        return BridgeRig(core: BridgeCore(directory: directory, backend: backend, home: home, now: clock),
                         backend: backend, directory: directory, home: home)
    }
}

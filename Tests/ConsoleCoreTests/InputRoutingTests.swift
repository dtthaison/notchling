import XCTest
@testable import ConsoleCore

private final class FakeSender: MessageSender, @unchecked Sendable {
    private let lock = NSLock()
    private var recorded: [(String, String)] = []
    var sent: [(String, String)] { lock.lock(); defer { lock.unlock() }; return recorded }

    func send(_ text: String, to session: Session) async throws {
        lock.withLock { recorded.append((text, session.id)) }
    }
}

final class InputRoutingTests: XCTestCase {
    private func session(_ control: SessionControl) -> Session {
        Session(id: "s1", pid: 1, name: "n", project: "p", cwd: "/p", entrypoint: .cli, kind: "interactive",
                state: .done, statusUpdatedAt: fixtureNow, startedAt: fixtureNow, transcriptURL: nil, control: control)
    }

    func testRoutesByControl() async throws {
        let job = FakeSender(), pty = FakeSender()
        let router = InputRouter(job: job, pty: pty)

        try await router.send("to job", to: session(.job("abc")))
        try await router.send("to pty", to: session(.consolePTY))
        XCTAssertEqual(job.sent.map(\.0), ["to job"])
        XCTAssertEqual(pty.sent.map(\.0), ["to pty"])
    }

    func testViewOnlyNeverReachesASender() async {
        let job = FakeSender(), pty = FakeSender()
        let router = InputRouter(job: job, pty: pty)
        do {
            try await router.send("nope", to: session(.viewOnly))
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? SendError, .viewOnly)
        }
        XCTAssertTrue(job.sent.isEmpty)
        XCTAssertTrue(pty.sent.isEmpty)
    }

    func testReplyChunksUseBracketedPasteThenReturn() {
        XCTAssertEqual(TerminalKeys.replyChunks("a\r\nb"), ["\u{1b}[200~a\nb\u{1b}[201~", "\r"])
    }

    func testPTYSenderWritesIntoTheOwnedPty() async throws {
        let pty = try PTYSession.spawn(executable: URL(fileURLWithPath: "/bin/cat"), arguments: [],
                                       cwd: FileManager.default.temporaryDirectory, model: nil,
                                       environment: ["TERM": "xterm-256color"])
        addTeardownBlock { pty.terminate() }
        let owned = OwnedSessions()
        owned.add(pty)

        let received = expectation(description: "echo")
        received.assertForOverFulfill = false
        let collected = Collected()
        pty.addListener { data in
            collected.append(data)
            if collected.text.contains("hello console") { received.fulfill() }
        }
        var owner = session(.consolePTY)
        owner.pid = pty.rootPid
        try await PTYMessageSender(owned: owned, pause: 0.05).send("hello console", to: owner)
        await fulfillment(of: [received], timeout: 5)
    }

    func testPTYSenderFailsWhenTheSessionIsNotOwned() async {
        let sender = PTYMessageSender(owned: OwnedSessions(), pause: 0)
        do {
            try await sender.send("x", to: session(.consolePTY))
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? SendError, .notOwned)
        }
    }
}

final class Collected: @unchecked Sendable {
    private let lock = NSLock()
    private var data = Data()
    func append(_ chunk: Data) { lock.lock(); data.append(chunk); lock.unlock() }
    var text: String { lock.lock(); defer { lock.unlock() }; return String(decoding: data, as: UTF8.self) }
}

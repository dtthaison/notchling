import XCTest
@testable import ConsoleCore

/// Switching the chat straight from one running session to another must end up on the right session's messages,
/// whatever the timing of the 1 s poll and of the selection change. (The Mac bug that needed a third session to be
/// opened first was in the terminal view reusing the previous session's pty, fixed with `.id(pty.id)`; this pins
/// the store side so it stays out of the suspects.)
@MainActor
final class SessionSwitchTests: XCTestCase {
    private func fixture() throws -> (ClaudePaths, [(id: String, url: URL)]) {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("switch-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let claude = root.appendingPathComponent(".claude")
        let sessions = claude.appendingPathComponent("sessions")
        try FileManager.default.createDirectory(at: sessions, withIntermediateDirectories: true)
        var made: [(String, URL)] = []
        for (index, name) in ["alpha", "beta"].enumerated() {
            let id = "0000000\(index + 1)-aaaa-4bbb-8ccc-dddddddddddd"
            let cwd = root.appendingPathComponent(name).path
            try FileManager.default.createDirectory(atPath: cwd, withIntermediateDirectories: true)
            // Both "running": this process's own pid is alive.
            let file = #"{"pid":\#(getpid()),"sessionId":"\#(id)","cwd":"\#(cwd)","status":"busy","statusUpdatedAt":\#(Date().timeIntervalSince1970 * 1000)}"#
            try Data(file.utf8).write(to: sessions.appendingPathComponent("\(index + 1)\(getpid()).json"))
            // The pid in the file name is only a file name; the pid inside is what is checked.
            let folder = claude.appendingPathComponent("projects/\(TranscriptLocator.encode(cwd: cwd))")
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let transcript = folder.appendingPathComponent("\(id).jsonl")
            try Data("".utf8).write(to: transcript)
            made.append((id, transcript))
        }
        return (ClaudePaths(claudeDir: claude, requestsDir: root.appendingPathComponent("requests")), made)
    }

    nonisolated private func line(_ text: String, _ n: Int, role: String = "user") -> String {
        let content = role == "user" ? "\"\(text)\"" : "[{\"type\":\"text\",\"text\":\"\(text)\"}]"
        return #"{"type":"\#(role)","uuid":"\#(UUID().uuidString)","timestamp":"2026-11-03T08:00:\#(String(format: "%02d", n % 60)).000Z","message":{"role":"\#(role)","content":\#(content)}}"# + "\n"
    }

    nonisolated private func append(_ text: String, to url: URL) throws {
        let handle = try FileHandle(forWritingTo: url)
        defer { try? handle.close() }
        try handle.seekToEnd()
        try handle.write(contentsOf: Data(text.utf8))
    }

    private func waitUntil(_ timeout: TimeInterval = 6, _ condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition(), Date() < deadline { try? await Task.sleep(nanoseconds: 20_000_000) }
    }

    func testSwitchingBetweenTwoRunningSessionsLandsOnTheSelectedOnesMessages() async throws {
        let (paths, made) = try fixture()
        try append(line("alpha question", 1) + line("alpha answer", 2, role: "assistant"), to: made[0].url)
        try append(line("beta question", 1) + line("beta answer", 2, role: "assistant"), to: made[1].url)
        let store = SessionStore(catalog: SessionCatalog(paths: paths), claudeDir: paths.claudeDir)
        store.start()
        addTeardownBlock { await MainActor.run { store.stop() } }
        await waitUntil { store.sessions.count == 2 }
        XCTAssertEqual(store.sessions.count, 2)

        // Both keep producing output while the user switches back and forth quickly.
        let writer = Task.detached {
            for n in 0..<60 {
                try? self.append(self.line("alpha tick \(n)", n, role: "assistant"), to: made[0].url)
                try? self.append(self.line("beta tick \(n)", n, role: "assistant"), to: made[1].url)
                try? await Task.sleep(nanoseconds: 30_000_000)
            }
        }
        for round in 0..<12 {
            let pick = round % 2 == 0 ? made[0] : made[1]
            store.selection = ChatSelection(sessionId: pick.id)
            try? await Task.sleep(nanoseconds: UInt64.random(in: 0...120_000_000))
        }
        store.selection = ChatSelection(sessionId: made[1].id)
        await waitUntil { store.chatLoadedFor == ChatSelection(sessionId: made[1].id) && !store.chat.isEmpty }
        await writer.value
        await waitUntil { store.chat.contains { $0.text.contains("beta tick 59") } }

        XCTAssertEqual(store.chatLoadedFor, ChatSelection(sessionId: made[1].id))
        XCTAssertTrue(store.chat.contains { $0.text.contains("beta") })
        XCTAssertFalse(store.chat.contains { $0.text.contains("alpha") }, "no message of the session switched away from is shown")

        store.selection = ChatSelection(sessionId: made[0].id)
        await waitUntil { store.chatLoadedFor == ChatSelection(sessionId: made[0].id) && store.chat.contains { $0.text.contains("alpha") } }
        XCTAssertEqual(store.chatLoadedFor, ChatSelection(sessionId: made[0].id))
        XCTAssertFalse(store.chat.contains { $0.text.contains("beta") })
    }
}

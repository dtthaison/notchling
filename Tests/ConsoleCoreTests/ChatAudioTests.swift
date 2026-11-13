import XCTest
@testable import ConsoleCore

final class ChatAudioTests: XCTestCase {
    private let home = URL(fileURLWithPath: "/Users/me")
    private let folder = "/Users/me/Desktop/Voices/Bay"

    private var disk: Set<String> {
        [folder, "/Users/me/Desktop/Voices/Bay/line.bay1.mp3", "/Users/me/Desktop/Voices/Bay/line.bay_luom.mp3",
         "/Users/me/Desktop/Voices/Ut/real_1.mp3", "/Users/me/Desktop/Voices/Ut", "/Users/me/notes.txt", "/etc/outside.mp3"]
    }

    private func files(_ text: String, cwd: String? = "/Users/me") -> [String] {
        ChatAudio.files(in: text, cwd: cwd, home: home, isFile: { self.disk.contains($0) && !$0.hasPrefix("/etc") },
                        isFolder: { self.disk.contains($0) && !$0.hasSuffix(".mp3") })
    }

    func testBareNamesResolveUnderARelativeFolderTheMessageNames() {
        let message = """
        Đã chép 6 câu thoại vào `Desktop/Voices/Bay/`:

        - `line.bay1.mp3`
        - `line.bay_luom.mp3`: "Từ hồi thằng Lượm chìm ngoài vàm…"
        """
        XCTAssertEqual(files(message), ["\(folder)/line.bay1.mp3", "\(folder)/line.bay_luom.mp3"])
    }

    func testAPathRelativeToTheSessionFolderResolves() {
        XCTAssertEqual(files("- `Desktop/Voices/Ut/real_1.mp3`"), ["/Users/me/Desktop/Voices/Ut/real_1.mp3"])
        XCTAssertEqual(files("/Users/me/Desktop/Voices/Ut/real_1.mp3", cwd: nil), ["/Users/me/Desktop/Voices/Ut/real_1.mp3"])
    }

    func testRepeatsAreDroppedAndOrderIsKept() {
        XCTAssertEqual(files("`Desktop/Voices/Bay/line.bay_luom.mp3` then `Desktop/Voices/Bay/line.bay1.mp3` and again Desktop/Voices/Bay/line.bay_luom.mp3."),
                       ["\(folder)/line.bay_luom.mp3", "\(folder)/line.bay1.mp3"])
    }

    func testMissingNonAudioAndOutsideFilesAreIgnored() {
        XCTAssertTrue(files("`nothing.mp3` and notes.txt and /etc/outside.mp3").isEmpty)
    }

    func testRealFilesFollowTheHomeAndTmpRule() throws {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("audio-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root.appendingPathComponent("v"), withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let mp3 = root.appendingPathComponent("v/a.mp3")
        try Data([1, 2, 3]).write(to: mp3)
        try Data([1]).write(to: root.appendingPathComponent("v/b.txt"))
        XCTAssertTrue(ChatAudio.isAllowedFile(mp3.path, home: root))
        XCTAssertFalse(ChatAudio.isAllowedFile(root.appendingPathComponent("v/b.txt").path, home: root), "not audio")
        XCTAssertFalse(ChatAudio.isAllowedFile("/etc/hosts.mp3", home: root), "outside home and /tmp")
        XCTAssertFalse(ChatAudio.isAllowedFile("v/a.mp3", home: root), "relative")
        let found = ChatAudio.files(in: "`v/` has `a.mp3`", cwd: root.path, home: root)
        XCTAssertEqual(found.map { ($0 as NSString).lastPathComponent }, ["a.mp3"])
    }

    func testIdIsStableAndShort() {
        XCTAssertEqual(ChatAudio.id(for: "/a/b.mp3"), ChatAudio.id(for: "/a/b.mp3"))
        XCTAssertTrue(ChatAudio.id(for: "/a/b.mp3").hasPrefix("a-"))
        XCTAssertEqual(ChatAudio.id(for: "/a/b.mp3").count, 18)
    }

    // MARK: chunks

    private func makeRef(bytes: Int) throws -> (AudioRef, URL, Data) {
        let root = FileManager.default.temporaryDirectory.appendingPathComponent("audio-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        let file = root.appendingPathComponent("x.wav")
        let data = Data((0..<bytes).map { UInt8(truncatingIfNeeded: $0 &* 7) })
        try data.write(to: file)
        return (AudioRef(id: ChatAudio.id(for: file.path), path: file.path, mediaType: "audio/wav", byteCount: bytes), root, data)
    }

    func testChunksCoverTheFileAndTheLastOneIsShort() throws {
        let size = ChatAudio.maxChunk * 2 + 100
        let (ref, root, data) = try makeRef(bytes: size)
        var offset = 0
        var assembled = Data()
        var calls = 0
        while offset < size {
            let chunk = try ChatAudio.chunk(of: ref, offset: offset, length: 10_000_000, home: root)
            XCTAssertEqual(chunk.total, size)
            XCTAssertLessThanOrEqual(chunk.data.count, ChatAudio.maxChunk, "length is capped")
            assembled.append(chunk.data)
            offset += chunk.data.count
            calls += 1
        }
        XCTAssertEqual(calls, 3)
        XCTAssertEqual(assembled, data)
        let past = try ChatAudio.chunk(of: ref, offset: size, length: 10, home: root)
        XCTAssertTrue(past.data.isEmpty, "at the end there is nothing more")
        XCTAssertThrowsError(try ChatAudio.chunk(of: ref, offset: size + 1, length: 10, home: root))
        XCTAssertThrowsError(try ChatAudio.chunk(of: ref, offset: -1, length: 10, home: root))
    }

    func testAFileThatLeftTheAllowedFoldersIsRefusedAtReadTime() throws {
        let (ref, _, _) = try makeRef(bytes: 10)
        XCTAssertThrowsError(try ChatAudio.chunk(of: ref, offset: 0, length: 10, home: URL(fileURLWithPath: "/Users/nobody-here"))) { error in
            // The temp folder is not under /tmp on macOS, so a foreign home refuses it.
            XCTAssertEqual(error as? AudioError, .notAllowed)
        }
    }

    // MARK: transcript and bridge

    private func transcriptLine(_ text: String, cwd: String) -> String {
        #"{"type":"assistant","uuid":"u1","cwd":"\#(cwd)","timestamp":"2026-11-03T08:00:01.000Z","message":{"role":"assistant","model":"m","content":[{"type":"text","text":"\#(text)"}]}}"#
    }

    func testTheParserFillsAudioFromTheLinesCwdAndOnlyServedIdsAreReadable() throws {
        // The parser uses the real home, so the files live under /tmp, which is allowed too.
        let root = URL(fileURLWithPath: "/tmp/audio-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: root.appendingPathComponent("voices"), withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: root) }
        try Data(repeating: 9, count: 50).write(to: root.appendingPathComponent("voices/hi.mp3"))
        let transcript = root.appendingPathComponent("t.jsonl")
        try (transcriptLine("In `voices/`: `hi.mp3`", cwd: root.path) + "\n").write(to: transcript, atomically: true, encoding: .utf8)

        let cache = ChatTranscriptCache()
        let messages = cache.messages(at: transcript, isMain: true)
        let audio = try XCTUnwrap(messages.first?.audio.first)
        XCTAssertEqual(audio.name, "hi.mp3")
        XCTAssertEqual(audio.byteCount, 50)
        XCTAssertEqual(audio.mediaType, "audio/mpeg")

        let dto = ChatMessageDTO(try XCTUnwrap(messages.first))
        let json = String(decoding: try JSONEncoder().encode(dto), as: UTF8.self)
        XCTAssertTrue(json.contains("\"audio\""))
        XCTAssertFalse(json.contains(root.path), "the Mac path never goes to the phone")

        let chunk = try cache.audio(at: transcript, id: audio.id, offset: 10, length: 100, home: root)
        XCTAssertEqual(chunk.data.count, 40)
        XCTAssertEqual(chunk.total, 50)
        XCTAssertThrowsError(try cache.audio(at: transcript, id: "a-0123456789abcdef", offset: 0, length: 10, home: root)) {
            XCTAssertEqual($0 as? AudioError, .notFound)
        }
    }

    func testMessagesWithoutAudioOmitTheField() {
        let message = ChatMessage(id: UUID(), role: .assistant, text: "hi", timestamp: Date())
        let json = String(decoding: (try? JSONEncoder().encode(ChatMessageDTO(message))) ?? Data(), as: UTF8.self)
        XCTAssertFalse(json.contains("audio"))
    }

    func testChatAudioOpValidatesAndAnswersInBase64() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login()
        rig.backend.sessionList = [makeSession("s1")]

        await rig.router.handle(#"{"id":1,"op":"chat.audio","sessionId":"s1","audioId":"a-0123456789abcdef"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found", "an id the chat never served")

        await rig.router.handle(#"{"id":2,"op":"chat.audio","sessionId":"s1","audioId":"../etc"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await rig.router.handle(#"{"id":3,"op":"chat.audio","sessionId":"s1","audioId":"a-1","offset":-5}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await rig.router.handle(#"{"id":4,"op":"chat.audio","sessionId":"nope","audioId":"a-1"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")

        rig.backend.audioResult = ChatAudio.Chunk(data: Data([1, 2, 3]), offset: 0, total: 3, mediaType: "audio/mpeg")
        await rig.router.handle(#"{"id":5,"op":"chat.audio","sessionId":"s1","audioId":"a-1","offset":0,"length":99999999}"#, from: connection)
        let reply = try io.lastJSON()
        XCTAssertTrue(reply.isOK)
        XCTAssertEqual(reply.dataObject["data"] as? String, "AQID")
        XCTAssertEqual(reply.dataObject["total"] as? Int, 3)
        XCTAssertEqual(rig.backend.audioCalls.last?.2, ChatAudio.maxChunk, "length is capped before the backend sees it")
    }
}

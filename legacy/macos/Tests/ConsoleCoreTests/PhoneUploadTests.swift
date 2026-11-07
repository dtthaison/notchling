import XCTest
@testable import ConsoleCore

final class PhoneUploadTests: XCTestCase {
    private let jpeg = Data([0xFF, 0xD8, 0xFF, 0xE0] + [UInt8](repeating: 7, count: 300_000))

    private func upload(_ rig: BridgeRig, _ connection: BridgeConnection, _ io: RecordingIO, session: String = "p", caption: String = "look", data: Data, chunk: Int = 96 * 1024) async throws {
        await rig.router.handle(#"{"id":10,"op":"session.image.begin","sessionId":"\#(session)","bytes":\#(data.count),"caption":"\#(caption)"}"#, from: connection)
        let begin = try io.lastJSON()
        XCTAssertTrue(begin.isOK, "\(begin)")
        let uploadId = try XCTUnwrap(begin.dataObject["uploadId"] as? String)
        var sequence = 0
        var offset = 0
        while offset < data.count {
            let piece = data[offset..<min(offset + chunk, data.count)]
            await rig.router.handle(#"{"id":\#(20 + sequence),"op":"session.image.chunk","uploadId":"\#(uploadId)","seq":\#(sequence),"data":"\#(piece.base64EncodedString())"}"#, from: connection)
            XCTAssertTrue(try io.lastJSON().isOK, "chunk \(sequence)")
            sequence += 1
            offset += chunk
        }
        await rig.router.handle(#"{"id":99,"op":"session.image.end","uploadId":"\#(uploadId)"}"#, from: connection)
    }

    func testAPhotoArrivesInPiecesIsSavedPrivatelyAndItsPathGoesToTheSession() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        let (connection, io, _) = try await rig.login()
        try await upload(rig, connection, io, data: jpeg)
        XCTAssertTrue(try io.lastJSON().isOK)
        let (sessionId, text) = try XCTUnwrap(rig.backend.sent.first)
        XCTAssertEqual(sessionId, "p")
        XCTAssertTrue(text.hasPrefix("look "), text)
        let path = String(text.dropFirst("look ".count))
        XCTAssertTrue(path.hasSuffix(".jpg"))
        XCTAssertTrue(path.hasPrefix(rig.home.path), "under the (test) home, never a path of the phone's choosing")
        XCTAssertEqual(try Data(contentsOf: URL(fileURLWithPath: path)), jpeg)
        let mode = try FileManager.default.attributesOfItem(atPath: path)[.posixPermissions] as? Int
        XCTAssertEqual(mode, 0o600)
        let entry = try XCTUnwrap(rig.core.audit.recent().last)
        XCTAssertEqual(entry.op, "image")
        XCTAssertEqual(entry.length, jpeg.count)
    }

    func testRefusals() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p"), makeSession("v", control: .viewOnly)]
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"session.image.begin","sessionId":"v","bytes":10}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "view_only")
        await rig.router.handle(#"{"id":2,"op":"session.image.begin","sessionId":"p","bytes":\#(PhoneUpload.maxBytes + 1)}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid", "over the size limit")
        await rig.router.handle(#"{"id":3,"op":"session.image.begin","sessionId":"ghost","bytes":10}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        await rig.router.handle(#"{"id":4,"op":"session.image.chunk","uploadId":"nope","seq":0,"data":"AAAA"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        // Not a picture: bytes that are not JPEG, PNG, GIF or WebP.
        try await upload(rig, connection, io, data: Data(repeating: 0x41, count: 5000))
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        // Short: fewer bytes than announced.
        await rig.router.handle(#"{"id":5,"op":"session.image.begin","sessionId":"p","bytes":5000}"#, from: connection)
        let id = try XCTUnwrap(try io.lastJSON().dataObject["uploadId"] as? String)
        await rig.router.handle(#"{"id":6,"op":"session.image.end","uploadId":"\#(id)"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        XCTAssertTrue(rig.backend.sent.isEmpty, "nothing reached the session")
    }

    func testChunksMustComeInOrderAndNotExceedTheAnnouncedSize() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"session.image.begin","sessionId":"p","bytes":4}"#, from: connection)
        let id = try XCTUnwrap(try io.lastJSON().dataObject["uploadId"] as? String)
        await rig.router.handle(#"{"id":2,"op":"session.image.chunk","uploadId":"\#(id)","seq":1,"data":"AAAA"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid", "sequence 1 before 0")
        await rig.router.handle(#"{"id":3,"op":"session.image.chunk","uploadId":"\#(id)","seq":0,"data":"AAAAAAAA"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid", "6 bytes into a 4 byte upload")
    }

    func testAnotherConnectionCannotFinishSomeoneElsesUpload() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"session.image.begin","sessionId":"p","bytes":4}"#, from: connection)
        let id = try XCTUnwrap(try io.lastJSON().dataObject["uploadId"] as? String)
        let (other, otherIO, _) = try await rig.login(name: "other-phone", from: "192.168.1.21")
        await rig.router.handle(#"{"id":2,"op":"session.image.end","uploadId":"\#(id)"}"#, from: other)
        XCTAssertEqual(try otherIO.lastJSON().errorCode, "not_found")
    }

    func testSlashCommandsComeFromTheBackend() async throws {
        let rig = try makeBridgeRig()
        rig.backend.sessionList = [makeSession("p")]
        rig.backend.commandList = [CommandInfo(name: "clear", description: "Clear"), CommandInfo(name: "sd:approve", description: "Approve", argumentHint: "[id]")]
        let (connection, io, _) = try await rig.login()
        await rig.router.handle(#"{"id":1,"op":"commands.list","sessionId":"p"}"#, from: connection)
        let commands = try XCTUnwrap(try io.lastJSON().dataObject["commands"] as? [[String: Any]])
        XCTAssertEqual(commands.map { $0["name"] as? String }, ["clear", "sd:approve"])
        XCTAssertEqual(commands[1]["argumentHint"] as? String, "[id]")
        XCTAssertNil(commands[0]["argumentHint"])
        await rig.router.handle(#"{"id":2,"op":"commands.list","sessionId":"ghost"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
    }

    func testThePathIsQuotedWhenItHasSpacesAndTheCaptionMayBeEmpty() {
        XCTAssertEqual(PhoneImageStore.message(caption: "", path: "/a b/c.jpg"), "\"/a b/c.jpg\"")
        XCTAssertEqual(PhoneImageStore.message(caption: "  hi  ", path: "/a/c.jpg"), "hi /a/c.jpg")
    }

    func testOldFilesArePrunedAndTypesAreDetectedByContent() throws {
        let dir = FileManager.default.temporaryDirectory.appendingPathComponent("imgs-\(UUID().uuidString)")
        addTeardownBlock { try? FileManager.default.removeItem(at: dir) }
        let store = PhoneImageStore(directory: dir, keepFor: 60)
        let old = try store.save(jpeg, ext: "jpg", now: Date().addingTimeInterval(-3600))
        try FileManager.default.setAttributes([.modificationDate: Date().addingTimeInterval(-3600)], ofItemAtPath: old.path)
        _ = try store.save(jpeg, ext: "jpg")
        XCTAssertFalse(FileManager.default.fileExists(atPath: old.path))
        XCTAssertEqual(PhoneImageStore.detect(Data([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0])), "png")
        XCTAssertNil(PhoneImageStore.detect(Data("not an image".utf8)))
    }
}

import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import XCTest
@testable import ConsoleCore

final class ChatImagesTests: XCTestCase {
    /// A real PNG of noise (so it does not compress away): the big-screenshot case.
    private func png(width: Int, height: Int) -> Data {
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        var seed: UInt64 = 42
        for index in pixels.indices {
            seed = seed &* 6364136223846793005 &+ 1442695040888963407
            pixels[index] = index % 4 == 3 ? 255 : UInt8(truncatingIfNeeded: seed >> 33)
        }
        let context = CGContext(data: &pixels, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
                                space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
        let output = NSMutableData()
        let destination = CGImageDestinationCreateWithData(output, UTType.png.identifier as CFString, 1, nil)!
        CGImageDestinationAddImage(destination, context.makeImage()!, nil)
        CGImageDestinationFinalize(destination)
        return output as Data
    }

    private func transcript() -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("img-\(UUID().uuidString).jsonl")
        FileManager.default.createFile(atPath: url.path, contents: nil)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func append(_ lines: [String], to url: URL) throws {
        let handle = try FileHandle(forWritingTo: url)
        defer { try? handle.close() }
        try handle.seekToEnd()
        try handle.write(contentsOf: Data((lines.joined(separator: "\n") + "\n").utf8))
    }

    private func imageBlock(_ base64: String, type: String = "image/png") -> String {
        #"{"type":"image","source":{"type":"base64","media_type":"\#(type)","data":"\#(base64)"}}"#
    }

    private func userLine(_ n: Int, text: String?, images: [String]) -> String {
        var blocks = images.map { imageBlock($0) }
        if let text { blocks.append(#"{"type":"text","text":"\#(text)"}"#) }
        return #"{"type":"user","uuid":"u\#(n)","timestamp":"2026-11-03T08:00:\#(String(format: "%02d", n)).000Z","message":{"role":"user","content":[\#(blocks.joined(separator: ","))]}}"#
    }

    private func toolResultLine(_ n: Int, base64: String) -> String {
        #"{"type":"user","uuid":"r\#(n)","timestamp":"2026-11-03T08:00:\#(String(format: "%02d", n)).000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"x","content":[\#(imageBlock(base64)),{"type":"text","text":"ok"}]}]}}"#
    }

    private func toolUseLine(_ n: Int) -> String {
        #"{"type":"assistant","uuid":"t\#(n)","timestamp":"2026-11-03T08:00:\#(String(format: "%02d", n)).000Z","message":{"role":"assistant","model":"m","content":[{"type":"tool_use","id":"x","name":"Read","input":{"file_path":"/a.png"}}]}}"#
    }

    private func parse(_ lines: [String]) -> [ChatMessage] {
        ChatTranscript.parse(lines: lines.map { Substring($0) }, isMain: true, path: "x").messages
    }

    // MARK: Blocks

    func testAPastedPictureBelongsToTheUserMessageAndNeverCarriesItsBytes() {
        let small = png(width: 40, height: 30).base64EncodedString()
        let messages = parse([userLine(1, text: "look at this", images: [small])])
        XCTAssertEqual(messages.count, 1)
        XCTAssertEqual(messages[0].text, "look at this")
        let ref = try! XCTUnwrap(messages[0].images.first)
        XCTAssertEqual(ref.mediaType, "image/png")
        XCTAssertEqual(ref.origin, .user)
        XCTAssertEqual(ref.width, 40)
        XCTAssertEqual(ref.height, 30)
        XCTAssertEqual(ref.blockIndex, 0)
        let json = String(data: try! JSONSerialization.data(withJSONObject: [ChatMessageDTO(messages[0]).images.count]), encoding: .utf8)!
        XCTAssertEqual(json, "[1]")
        XCTAssertFalse(String(describing: ChatMessageDTO(messages[0])).contains(small.prefix(60)), "the DTO has no image bytes")
    }

    func testAPictureWithNoTextIsStillAMessage() {
        let messages = parse([userLine(1, text: nil, images: [png(width: 8, height: 8).base64EncodedString()])])
        XCTAssertEqual(messages.count, 1)
        XCTAssertEqual(messages[0].text, "")
        XCTAssertEqual(messages[0].images.count, 1)
    }

    func testAToolsScreenshotGoesUnderItsToolRow() {
        let messages = parse([toolUseLine(1), toolResultLine(2, base64: png(width: 20, height: 10).base64EncodedString())])
        XCTAssertEqual(messages.count, 1)
        XCTAssertEqual(messages[0].role, .tools)
        XCTAssertEqual(messages[0].images.first?.origin, .toolResult)
        XCTAssertEqual(messages[0].images.first?.width, 20)
    }

    func testALineOverTheSizeCapIsNotDroppedWhenTheBulkIsAPicture() throws {
        let big = png(width: 500, height: 500).base64EncodedString()
        let line = userLine(1, text: "a big screenshot", images: [big])
        XCTAssertGreaterThan(line.utf8.count, ChatTranscript.maxLineBytes * 3)
        let messages = parse([line, userLine(2, text: "next", images: [])])
        XCTAssertEqual(messages.map(\.text), ["a big screenshot\n\nnext"].isEmpty ? [] : messages.map(\.text))
        XCTAssertTrue(messages[0].text.hasPrefix("a big screenshot"))
        let ref = try XCTUnwrap(messages[0].images.first)
        XCTAssertEqual(ref.width, 500)
        XCTAssertEqual(ref.byteCount, big.utf8.count * 3 / 4, accuracy: 3)
    }

    func testALongLineWithoutPicturesIsStillSkipped() {
        let junk = String(repeating: "x", count: ChatTranscript.maxLineBytes + 10)
        let line = #"{"type":"user","uuid":"u","timestamp":"2026-11-03T08:00:01.000Z","message":{"role":"user","content":"\#(junk)"}}"#
        XCTAssertTrue(parse([line]).isEmpty)
    }

    func testPictureInAQueuedMessage() {
        let attachment = #"{"type":"attachment","uuid":"q1","timestamp":"2026-11-03T08:00:05.000Z","attachment":{"type":"queued_command","prompt":[\#(imageBlock(png(width: 9, height: 9).base64EncodedString())),{"type":"text","text":"see"}],"origin":{"kind":"human"}}}"#
        let messages = parse([attachment])
        XCTAssertEqual(messages.first?.text, "see")
        XCTAssertEqual(messages.first?.images.count, 1)
    }

    // MARK: Files named in the text

    private func scratchFile(_ name: String, in directory: URL, data: Data = Data([1, 2, 3])) throws -> String {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let url = directory.appendingPathComponent(name)
        try data.write(to: url)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        return url.path
    }

    func testMarkerPathRules() throws {
        let home = FileManager.default.temporaryDirectory.appendingPathComponent("home-\(UUID().uuidString)")
        let allowedInHome = try scratchFile("shot.png", in: home.appendingPathComponent("Pictures"))
        let allowedInTmp = try scratchFile("tmp.JPG", in: URL(fileURLWithPath: "/tmp/mk-\(UUID().uuidString.prefix(6))"))
        let wrongType = try scratchFile("notes.txt", in: home)
        let outside = try scratchFile("out.png", in: FileManager.default.temporaryDirectory.appendingPathComponent("outside-\(UUID().uuidString)"))
        let directory = home.appendingPathComponent("dir.png")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let link = home.appendingPathComponent("link.png")
        try FileManager.default.createSymbolicLink(atPath: link.path, withDestinationPath: "/etc/hosts")

        XCTAssertTrue(ChatImages.isAllowedFile(allowedInHome, home: home))
        XCTAssertTrue(ChatImages.isAllowedFile(allowedInTmp, home: home))
        XCTAssertFalse(ChatImages.isAllowedFile(wrongType, home: home), "not an image extension")
        XCTAssertFalse(ChatImages.isAllowedFile(outside, home: home), "outside home and /tmp")
        XCTAssertFalse(ChatImages.isAllowedFile(directory.path, home: home), "a folder is not a file")
        XCTAssertFalse(ChatImages.isAllowedFile(link.path, home: home), "a link that leads out is judged by where it leads")
        XCTAssertFalse(ChatImages.isAllowedFile(home.appendingPathComponent("missing.png").path, home: home))
        XCTAssertFalse(ChatImages.isAllowedFile("relative/x.png", home: home))
        XCTAssertFalse(ChatImages.isAllowedFile(home.path + "/../../etc/x.png", home: home), "dot-dot does not climb out")
    }

    func testMarkersBecomeReferencesOnlyForAllowedFiles() throws {
        let home = FileManager.default.homeDirectoryForCurrentUser
        let folder = URL(fileURLWithPath: "/tmp/mk-\(UUID().uuidString.prefix(6))")
        let path = try scratchFile("pasted.png", in: folder)
        let text = "before [Image: source: \(path)] after [Image: source: /etc/passwd.png]"
        let result = ChatImages.fileMarkers(in: text, home: home)
        XCTAssertEqual(result.refs.count, 1)
        XCTAssertEqual(result.refs[0].origin, .file)
        XCTAssertEqual(result.refs[0].path, path)
        XCTAssertTrue(result.text.contains("[Image: source: /etc/passwd.png]"), "a refused marker stays as written")
        XCTAssertFalse(result.text.contains(path))
    }

    func testAFileMarkerCarriesThePicturesRealSize() throws {
        // A tall screenshot without its size was drawn as a 4:3 thumbnail and cropped on the phone.
        let folder = URL(fileURLWithPath: "/tmp/mk-\(UUID().uuidString.prefix(6))")
        let path = try scratchFile("tall.png", in: folder, data: png(width: 30, height: 65))
        let result = ChatImages.fileMarkers(in: "[Image: source: \(path)]", home: FileManager.default.homeDirectoryForCurrentUser)
        XCTAssertEqual(result.refs.count, 1)
        XCTAssertEqual(result.refs[0].width, 30)
        XCTAssertEqual(result.refs[0].height, 65)
    }

    // MARK: Fetching

    func testOnlyPicturesTheChatHasShownCanBeFetched() throws {
        let url = transcript()
        let source = png(width: 300, height: 200)
        try append([userLine(1, text: "hi", images: [source.base64EncodedString()])], to: url)
        let cache = ChatTranscriptCache()
        let ref = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 10)?.messages.first?.images.first)

        let jpeg = try cache.image(at: url, id: ref.id, maxWidth: 100)
        XCTAssertEqual(Array(jpeg.prefix(2)), [0xFF, 0xD8], "a JPEG")
        let size = CGImageSourceCreateWithData(jpeg as CFData, nil).flatMap { CGImageSourceCopyPropertiesAtIndex($0, 0, nil) as? [CFString: Any] }
        XCTAssertLessThanOrEqual(size?[kCGImagePropertyPixelWidth] as? Int ?? 999, 100)

        XCTAssertThrowsError(try cache.image(at: url, id: "b-0-7", maxWidth: 100), "a block the chat never showed")
        XCTAssertThrowsError(try cache.image(at: url, id: "f-0123456789abcdef", maxWidth: 100), "a file it never named")
        XCTAssertThrowsError(try cache.image(at: transcript(), id: ref.id, maxWidth: 100), "another session's transcript knows nothing of it")
    }

    func testAFileNamedInTheTextIsFetchedOnlyThroughItsReference() throws {
        let folder = URL(fileURLWithPath: "/tmp/mk-\(UUID().uuidString.prefix(6))")
        let path = try scratchFile("note.png", in: folder, data: png(width: 64, height: 64))
        let url = transcript()
        let line = #"{"type":"user","uuid":"u1","timestamp":"2026-11-03T08:00:01.000Z","message":{"role":"user","content":"see [Image: source: \#(path)]"}}"#
        try append([line], to: url)
        let cache = ChatTranscriptCache()
        let ref = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 10)?.messages.first?.images.first)
        XCTAssertEqual(ref.origin, .file)
        XCTAssertNoThrow(try cache.image(at: url, id: ref.id, maxWidth: 64))
        // The file is replaced by a link to somewhere else: refused at fetch time, not just at parse time.
        try FileManager.default.removeItem(atPath: path)
        try FileManager.default.createSymbolicLink(atPath: path, withDestinationPath: "/etc/hosts")
        XCTAssertThrowsError(try cache.image(at: url, id: ref.id, maxWidth: 64))
    }

    func testPagingReadsAHugeLineWhole() throws {
        let url = transcript()
        var lines: [String] = []
        for i in 0..<30 { lines.append(userLine(i, text: "before \(i) " + String(repeating: "p", count: 20_000), images: [])) }
        lines.append(userLine(31, text: "the screenshot", images: [png(width: 900, height: 900).base64EncodedString()]))
        for i in 32..<45 { lines.append(userLine(i, text: "after \(i) " + String(repeating: "q", count: 20_000), images: [])) }
        try append(lines, to: url)
        let cache = ChatTranscriptCache()
        var page = try XCTUnwrap(cache.latest(at: url, isMain: true, limit: 5))
        var all = page.messages
        var rounds = 0
        while page.hasMore, rounds < 50 {
            page = try XCTUnwrap(cache.page(at: url, isMain: true, before: all[0].id, limit: 5))
            all = page.messages + all
            rounds += 1
        }
        XCTAssertTrue(all.contains { $0.text.contains("the screenshot") && $0.images.count == 1 }, "the picture's message is reached when paging back")
    }

    func testChatImageThroughTheBridgeChecksTheRequest() async throws {
        let rig = try makeBridgeRig()
        let (connection, io, _) = try await rig.login()
        rig.backend.sessionList = [makeSession("s1")]
        await rig.router.handle(#"{"id":2,"op":"chat.image","sessionId":"s1","imageId":"../../etc/passwd"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await rig.router.handle(#"{"id":3,"op":"chat.image","sessionId":"nope","imageId":"b-1-0"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found")
        await rig.router.handle(#"{"id":4,"op":"chat.image","sessionId":"s1"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "invalid")
        await rig.router.handle(#"{"id":5,"op":"chat.image","sessionId":"s1","imageId":"b-1-0"}"#, from: connection)
        XCTAssertEqual(try io.lastJSON().errorCode, "not_found", "the fake backend knows no pictures")
        XCTAssertTrue(io.binaries.isEmpty)
    }

    func testTheBinaryFrameCarriesTheRequestId() {
        let frame = PhoneFrame.encodeImage(requestId: 7, jpeg: Data([0xFF, 0xD8]))
        XCTAssertEqual(Array(frame.prefix(4)), Array("IMG1".utf8))
        XCTAssertEqual(Array(frame[4..<8]), [0, 0, 0, 7])
        XCTAssertEqual(Array(frame.suffix(2)), [0xFF, 0xD8])
    }
}

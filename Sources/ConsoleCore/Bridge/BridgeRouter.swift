import Foundation

/// Reads one text frame from a connection and answers it. Nothing is served before `hello` or `pair`.
public final class BridgeRouter: @unchecked Sendable {
    public static let maxMessageBytes = 256 * 1024
    public static let maxTextCharacters = 20_000
    public static let models: Set<String> = ["default", "opus", "sonnet", "haiku"]

    private let backend: BridgeBackend
    private let devices: DeviceStore
    private let codes: PairingCodes
    private let limiter: AuthRateLimiter
    private let audit: AuditLog
    private let hub: BridgeHub
    private let home: URL
    private let now: () -> Date
    private let images: PhoneImageStore
    private let uploads = PhoneUploads()
    private let sendNowGate = SendNowGate()

    public init(backend: BridgeBackend, devices: DeviceStore, codes: PairingCodes, limiter: AuthRateLimiter,
                audit: AuditLog, hub: BridgeHub, home: URL = FileManager.default.homeDirectoryForCurrentUser,
                images: PhoneImageStore? = nil, now: @escaping () -> Date = Date.init) {
        self.images = images ?? PhoneImageStore(directory: home.appendingPathComponent("Library/Caches/AgentConsole/phone-images"))
        self.backend = backend
        self.devices = devices
        self.codes = codes
        self.limiter = limiter
        self.audit = audit
        self.hub = hub
        self.home = home
        self.now = now
    }

    public func handle(_ text: String, from connection: BridgeConnection) async {
        if !connection.isAuthenticated, case .blocked = limiter.check(connection.remoteAddress, now: now()) {
            ConsoleLog.logger.notice("bridge: \(connection.remoteAddress, privacy: .public) is locked out, closing")
            connection.close()
            return
        }
        guard text.utf8.count <= Self.maxMessageBytes,
              let object = (try? JSONSerialization.jsonObject(with: Data(text.utf8))) as? [String: Any],
              let op = object["op"] as? String else {
            reply(connection, id: nil, failure: BridgeFailure(.invalid, "Tin nhắn không phải JSON hợp lệ."))
            if !connection.isAuthenticated { fail(connection) }
            return
        }
        let id = object["id"]

        guard connection.isAuthenticated else {
            switch op {
            case "hello": await hello(object, id: id, connection)
            case "pair": await pair(object, id: id, connection)
            default:
                reply(connection, id: id, failure: BridgeFailure(.unauthorized, "Cần đăng nhập (hello) trước."))
                fail(connection)
            }
            return
        }

        do {
            let data = try await dispatch(op, object, id: id, connection)
            connection.io.sendText(BridgeWire.ok(id: id, data: data))
        } catch {
            reply(connection, id: id, failure: Self.failure(from: error, op: op))
        }
    }

    // MARK: - Auth

    private func fail(_ connection: BridgeConnection) {
        limiter.recordFailure(connection.remoteAddress, now: now())
        connection.close()
    }

    private func hello(_ object: [String: Any], id: Any?, _ connection: BridgeConnection) async {
        guard let token = object["token"] as? String, let device = devices.authenticate(token: token) else {
            reply(connection, id: id, failure: BridgeFailure(.unauthorized, "Token không đúng hoặc đã bị thu hồi."))
            fail(connection)
            return
        }
        limiter.recordSuccess(connection.remoteAddress)
        connection.authenticate(as: device)
        devices.touch(id: device.id)
        connection.io.sendText(BridgeWire.ok(id: id, data: ["device": ["id": device.id, "name": device.name], "version": BridgeWire.version]))
    }

    private func pair(_ object: [String: Any], id: Any?, _ connection: BridgeConnection) async {
        guard let code = object["code"] as? String, codes.consume(code, now: now()) else {
            reply(connection, id: id, failure: BridgeFailure(.unauthorized, "Mã ghép đôi sai hoặc đã hết hạn."))
            fail(connection)
            return
        }
        let info = object["device"] as? [String: Any]
        let name = (info?["name"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        let platform = (info?["platform"] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? "unknown"
        guard !name.isEmpty else {
            reply(connection, id: id, failure: BridgeFailure(.invalid, "Thiếu tên thiết bị."))
            fail(connection)
            return
        }
        guard await backend.requestApproval(deviceName: name, platform: platform) else {
            reply(connection, id: id, failure: BridgeFailure(.unauthorized, "Mac đã từ chối thiết bị này."))
            connection.close()
            return
        }
        do {
            let (device, token) = try devices.register(name: name, platform: platform)
            limiter.recordSuccess(connection.remoteAddress)
            connection.authenticate(as: device)
            audit.append(device: device.name, op: "pair", address: connection.auditAddress)
            connection.io.sendText(BridgeWire.ok(id: id, data: ["token": token, "deviceId": device.id, "version": BridgeWire.version]))
        } catch {
            ConsoleLog.error("BridgeRouter.pair", name, "\(error)")
            reply(connection, id: id, failure: BridgeFailure(.internal, "Không lưu được thiết bị."))
            connection.close()
        }
    }

    // MARK: - Ops

    private func dispatch(_ op: String, _ body: [String: Any], id: Any?, _ connection: BridgeConnection) async throws -> Any {
        switch op {
        case "ping":
            return ["time": Int(now().timeIntervalSince1970 * 1000)]
        case "sessions.list":
            return ["sessions": BridgeWire.object(await backend.sessions().map(SessionDTO.init))]
        case "chat.get":
            return try await chatGet(body)
        case "viz.get":
            return try await vizGet(body)
        case "web.snapshot":
            return try await webSnapshot(body, id: id, connection)
        case "session.message":
            return try await sessionMessage(body, connection)
        case "session.sendNow":
            return try await sessionSendNow(body, connection)
        case "session.image.begin":
            return try await imageBegin(body, connection)
        case "session.image.chunk":
            return try imageChunk(body, connection)
        case "session.image.end":
            return try await imageEnd(body, connection)
        case "commands.list":
            return try await commandsList(body)
        case "prompt.answer":
            return try await promptAnswer(body, connection)
        case "session.new":
            return try await sessionNew(body, connection)
        case "chat.image":
            return try await chatImage(body, id: id, connection)
        case "chat.audio":
            return try await chatAudio(body)
        case "session.takeover":
            return try await sessionTakeover(body, connection)
        case "projects.recent":
            return ["projects": await backend.recentProjects()]
        case "subscribe":
            return try await subscribe(body, connection)
        case "unsubscribe":
            await unsubscribe(connection)
            return ["subscribed": false]
        default:
            throw BridgeFailure(.invalid, "Không có lệnh \"\(op)\".")
        }
    }

    private func requireString(_ body: [String: Any], _ key: String) throws -> String {
        guard let value = body[key] as? String, !value.isEmpty else {
            throw BridgeFailure(.invalid, "Thiếu \"\(key)\".")
        }
        return value
    }

    private func session(_ id: String) async throws -> Session {
        guard let found = await backend.sessions().first(where: { $0.id == id }) else {
            throw BridgeFailure(.notFound, "Không có session này.")
        }
        return found
    }

    private func chatGet(_ body: [String: Any]) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        _ = try await session(sessionId)
        let limit = min(max((body["limit"] as? Int) ?? 50, 1), 200)
        let page = try await backend.chatPage(sessionId: sessionId, subagentId: body["subagentId"] as? String,
                                              before: body["before"] as? String, after: body["after"] as? String, limit: limit)
        var answer: [String: Any] = ["messages": BridgeWire.object(page.messages.map(ChatMessageDTO.init)), "hasMore": page.hasMore]
        if page.reset { answer["reset"] = true }
        return answer
    }

    /// `chat.image {sessionId, imageId, maxWidth?}`: the JSON answer first, then a binary frame "IMG1" + request id + JPEG.
    private func chatImage(_ body: [String: Any], id: Any?, _ connection: BridgeConnection) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        let imageId = try requireString(body, "imageId")
        guard imageId.count <= 80, imageId.range(of: #"^[A-Za-z0-9\-]+$"#, options: .regularExpression) != nil else {
            throw BridgeFailure(.invalid, "imageId không hợp lệ.")
        }
        _ = try await session(sessionId)
        let width = min(max((body["maxWidth"] as? Int) ?? 800, 64), 1600)
        let jpeg = try await backend.chatImage(sessionId: sessionId, imageId: imageId, maxWidth: width)
        let requestId = (id as? Int).map { UInt32(truncatingIfNeeded: $0) } ?? 0
        connection.io.sendText(BridgeWire.ok(id: id, data: ["binary": true, "bytes": jpeg.count, "format": "jpeg", "imageId": imageId]))
        connection.io.sendBinary(PhoneFrame.encodeImage(requestId: requestId, jpeg: jpeg)) {}
        return ["sent": true]
    }

    /// `chat.audio {sessionId, audioId, offset, length}`: one piece (at most 160 KB) of an audio file the chat names,
    /// as `{data: base64, offset, total, mediaType}`. The phone asks again from `offset + data length` until `total`.
    private func chatAudio(_ body: [String: Any]) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        let audioId = try requireString(body, "audioId")
        guard audioId.count <= 80, audioId.range(of: #"^[A-Za-z0-9\-]+$"#, options: .regularExpression) != nil else {
            throw BridgeFailure(.invalid, "audioId không hợp lệ.")
        }
        let offset = (body["offset"] as? Int) ?? 0
        let length = min((body["length"] as? Int) ?? ChatAudio.maxChunk, ChatAudio.maxChunk)
        guard offset >= 0, length > 0 else { throw BridgeFailure(.invalid, "offset hoặc length không hợp lệ.") }
        _ = try await session(sessionId)
        let chunk = try await backend.chatAudio(sessionId: sessionId, audioId: audioId, offset: offset, length: length)
        return ["data": chunk.data.base64EncodedString(), "offset": chunk.offset, "total": chunk.total, "mediaType": chunk.mediaType]
    }

    private func vizGet(_ body: [String: Any]) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        _ = try await session(sessionId)
        let viz = try await backend.viz(sessionId: sessionId, includeEntity: (body["entity"] as? Bool) == true)
        var data: [String: Any] = ["suggested": viz.suggestedTab]
        if let url = viz.webURL { data["webURL"] = url }
        if !viz.webCandidates.isEmpty { data["webCandidates"] = viz.webCandidates }
        for (key, spec) in viz.specs { data[key] = BridgeWire.object(spec) }
        return data
    }

    private func webSnapshot(_ body: [String: Any], id: Any?, _ connection: BridgeConnection) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        _ = try await session(sessionId)
        let width = min(max((body["width"] as? Int) ?? 400, 100), 1200)
        let url: URL
        if let text = body["url"] as? String {
            guard let local = WebURLResolver.localURL(text) else {
                throw BridgeFailure(.invalid, "Chỉ chụp được URL http(s) trên localhost hoặc 127.0.0.1.")
            }
            url = local
        } else {
            let viz = try await backend.viz(sessionId: sessionId, includeEntity: false)
            guard let found = viz.webURL.flatMap(WebURLResolver.localURL) else {
                throw BridgeFailure(.notFound, "Session này chưa có URL web.")
            }
            url = found
        }
        let jpeg = try await backend.webSnapshot(url: url, width: width)
        let requestId = (id as? Int).map { UInt32(truncatingIfNeeded: $0) } ?? 0
        // The JSON answer goes first, so the client knows a binary frame follows.
        connection.io.sendText(BridgeWire.ok(id: id, data: ["binary": true, "bytes": jpeg.count, "format": "jpeg", "url": url.absoluteString]))
        connection.io.sendBinary(PhoneFrame.encodeSnapshot(requestId: requestId, jpeg: jpeg)) {}
        return ["sent": true]
    }

    private func sessionMessage(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        let text = try requireString(body, "text")
        guard text.count <= Self.maxTextCharacters else { throw BridgeFailure(.invalid, "Tin nhắn quá dài.") }
        let target = try await session(sessionId)
        guard target.control != .viewOnly else {
            throw BridgeFailure(.viewOnly, "Session mở ngoài Console — chỉ xem.")
        }
        guard connection.beginSend(to: sessionId) else { throw BridgeFailure(.busy, "Đang gửi tin trước đó vào session này.") }
        defer { connection.endSend(to: sessionId) }
        try await backend.sendMessage(text, to: target)
        audit.append(device: connection.device?.name ?? "?", op: "message", session: sessionId, length: text.count, address: connection.auditAddress)
        return ["sent": true]
    }

    /// `session.sendNow {sessionId}`: one ESC interrupts the running turn, then Claude Code runs the queued message by
    /// itself (checked against a real session). Only terminals Console owns; a session opened elsewhere is view-only.
    private func sessionSendNow(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        let target = try await session(sessionId)
        guard target.control == .consolePTY else {
            throw BridgeFailure(.viewOnly, "Chỉ ngắt được session do Console giữ terminal.")
        }
        // Two ESC in a row would also cancel the queued message, so one interrupt per session at a time.
        guard connection.beginSend(to: sessionId) else { throw BridgeFailure(.busy, "Đang gửi lệnh trước đó vào session này.") }
        defer { connection.endSend(to: sessionId) }
        guard sendNowGate.allow(sessionId, at: now()) else { throw BridgeFailure(.busy, "Vừa ngắt xong, đợi một chút.") }
        try await backend.sendNow(session: target)
        audit.append(device: connection.device?.name ?? "?", op: "sendNow", session: sessionId, length: 0, address: connection.auditAddress)
        return ["sent": true]
    }

    // MARK: Photos and commands

    /// `session.image.begin {sessionId, bytes, caption?}` then `session.image.chunk {uploadId, seq, data}` (base64, in order)
    /// then `session.image.end {uploadId}`: the photo is saved to a private temp file and its path goes to the session
    /// with the caption, as a dragged-in picture does. Chunked because a bridge message is capped at 256 KB.
    private func imageBegin(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        guard let bytes = body["bytes"] as? Int, bytes > 0, bytes <= PhoneUpload.maxBytes else {
            throw BridgeFailure(.invalid, "Ảnh phải nhỏ hơn \(PhoneUpload.maxBytes / 1_048_576) MB.")
        }
        let caption = (body["caption"] as? String) ?? ""
        guard caption.count <= Self.maxTextCharacters else { throw BridgeFailure(.invalid, "Chú thích quá dài.") }
        let target = try await session(sessionId)
        guard target.control != .viewOnly else { throw BridgeFailure(.viewOnly, "Session mở ngoài Console — chỉ xem.") }
        return ["uploadId": uploads.begin(connection: connection.id, sessionId: sessionId, bytes: bytes, caption: caption, now: now()), "chunkBytes": 96 * 1024]
    }

    private func imageChunk(_ body: [String: Any], _ connection: BridgeConnection) throws -> Any {
        let uploadId = try requireString(body, "uploadId")
        guard let sequence = body["seq"] as? Int, let encoded = body["data"] as? String, let chunk = Data(base64Encoded: encoded) else {
            throw BridgeFailure(.invalid, "Thiếu \"seq\" hoặc \"data\" (base64).")
        }
        do { try uploads.append(uploadId, connection: connection.id, sequence: sequence, chunk: chunk) } catch { throw Self.uploadFailure(error) }
        return ["received": sequence]
    }

    private func imageEnd(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let uploadId = try requireString(body, "uploadId")
        let upload: PhoneUpload
        do { upload = try uploads.finish(uploadId, connection: connection.id) } catch { throw Self.uploadFailure(error) }
        guard let ext = PhoneImageStore.detect(upload.data) else { throw BridgeFailure(.invalid, "Chỉ nhận ảnh JPEG, PNG, GIF hoặc WebP.") }
        let target = try await session(upload.sessionId)
        guard target.control != .viewOnly else { throw BridgeFailure(.viewOnly, "Session mở ngoài Console — chỉ xem.") }
        guard connection.beginSend(to: upload.sessionId) else { throw BridgeFailure(.busy, "Đang gửi tin trước đó vào session này.") }
        defer { connection.endSend(to: upload.sessionId) }
        let file: URL
        do { file = try images.save(upload.data, ext: ext, now: now()) } catch {
            ConsoleLog.error("BridgeRouter.imageEnd", upload.sessionId, "\(error)")
            throw BridgeFailure(.internal, "Mac không lưu được ảnh.")
        }
        try await backend.sendMessage(PhoneImageStore.message(caption: upload.caption, path: file.path), to: target)
        audit.append(device: connection.device?.name ?? "?", op: "image", session: upload.sessionId, length: upload.data.count, address: connection.auditAddress)
        return ["sent": true]
    }

    private static func uploadFailure(_ error: Error) -> BridgeFailure {
        switch error as? PhoneUploadError {
        case .outOfOrder?: return BridgeFailure(.invalid, "Mảnh ảnh sai thứ tự.")
        case .tooBig?: return BridgeFailure(.invalid, "Mảnh ảnh quá lớn.")
        case .incomplete?: return BridgeFailure(.invalid, "Ảnh chưa gửi đủ.")
        default: return BridgeFailure(.notFound, "Không có lượt gửi ảnh này (hết hạn?).")
        }
    }

    /// `commands.list {sessionId}`: the session's slash commands, so the phone offers the real ones after "/".
    private func commandsList(_ body: [String: Any]) async throws -> Any {
        let sessionId = try requireString(body, "sessionId")
        _ = try await session(sessionId)
        let commands = await backend.commands(sessionId: sessionId)
        return ["commands": commands.map { command -> [String: Any] in
            var row: [String: Any] = ["name": command.name, "description": command.description]
            if let hint = command.argumentHint { row["argumentHint"] = hint }
            return row
        }]
    }

    private func promptAnswer(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let requestId = try requireString(body, "requestId")
        guard let prompt = await backend.pendingPrompt(requestId: requestId) else {
            throw BridgeFailure(.notFound, "Không còn câu hỏi hoặc permission này.")
        }
        let response: PromptResponse
        switch prompt.kind {
        case .permission:
            guard let allow = body["allow"] as? Bool else { throw BridgeFailure(.invalid, "Permission cần \"allow\": true hoặc false.") }
            response = allow ? .allow : .deny
        case .question(let questions):
            if questions.contains(where: \.multiSelect) {
                throw BridgeFailure(.invalid, "Câu hỏi chọn nhiều đáp án: trả lời trên terminal.")
            }
            response = .answers(try Self.answers(body, for: questions))
        }
        try await backend.respond(to: prompt, with: response)
        audit.append(device: connection.device?.name ?? "?", op: "answer", session: prompt.sessionId, address: connection.auditAddress)
        return ["answered": true]
    }

    /// "choice": 1-based option number or label (an array for several questions); "text": a free-text answer.
    static func answers(_ body: [String: Any], for questions: [PromptQuestion]) throws -> [String] {
        let raw: [Any]
        if let text = body["text"] as? String {
            guard questions.count == 1, !text.isEmpty, text.count <= 500 else { throw BridgeFailure(.invalid, "\"text\" chỉ dùng cho một câu hỏi, tối đa 500 ký tự.") }
            return [text]
        } else if let list = body["choice"] as? [Any] {
            raw = list
        } else if let single = body["choice"] {
            raw = [single]
        } else {
            throw BridgeFailure(.invalid, "Cần \"choice\" hoặc \"text\".")
        }
        guard raw.count == questions.count else { throw BridgeFailure(.invalid, "Cần \(questions.count) câu trả lời.") }
        return try zip(raw, questions).map { value, question in
            if let number = value as? Int {
                guard (1...max(question.options.count, 1)).contains(number), number <= question.options.count else {
                    throw BridgeFailure(.invalid, "Lựa chọn \(number) không có.")
                }
                return question.options[number - 1]
            }
            if let label = value as? String, question.options.contains(label) { return label }
            throw BridgeFailure(.invalid, "Lựa chọn không khớp đáp án nào.")
        }
    }

    private func sessionNew(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let cwd = try requireString(body, "cwd")
        let model = body["model"] as? String
        if let model, !Self.models.contains(model) { throw BridgeFailure(.invalid, "Model không hợp lệ.") }
        let folder = try Self.validatedFolder(cwd, home: home)
        try await backend.newSession(cwd: folder, model: model == "default" ? nil : model)
        audit.append(device: connection.device?.name ?? "?", op: "newSession", address: connection.auditAddress)
        return ["started": true]
    }

    /// `session.takeover {sessionId}` -> {sessionId}: the id to switch to (a `pty:` id while the resumed session
    /// is still starting). Only view-only sessions; the backend re-checks that the original process is gone.
    private func sessionTakeover(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        let id = try requireString(body, "sessionId")
        var stopOriginal = false
        if let raw = body["stopOriginal"] {
            guard let flag = raw as? Bool else { throw BridgeFailure(.invalid, "\"stopOriginal\" phải là true hoặc false.") }
            stopOriginal = flag
        }
        let target = try await session(id)
        guard target.control == .viewOnly else { throw BridgeFailure(.invalid, TakeoverError.alreadyControllable.localizedDescription) }
        let outcome = try await backend.takeOver(session: target, stopOriginal: stopOriginal)
        audit.append(device: connection.device?.name ?? "?", op: "takeover", session: id, address: connection.auditAddress,
                     stoppedPid: outcome.stoppedPid.map(Int.init))
        var answer: [String: Any] = ["sessionId": outcome.sessionId, "resumed": true]
        if let pid = outcome.stoppedPid { answer["stoppedPid"] = Int(pid) }
        return answer
    }

    /// An existing directory inside the user's home (symlinks resolved, so a link cannot point out of it).
    public static func validatedFolder(_ path: String, home: URL) throws -> URL {
        guard path.hasPrefix("/") else { throw BridgeFailure(.invalid, "cwd phải là đường dẫn tuyệt đối.") }
        let resolved = URL(fileURLWithPath: path).standardizedFileURL.resolvingSymlinksInPath()
        let homePath = home.standardizedFileURL.resolvingSymlinksInPath().path
        guard resolved.path == homePath || resolved.path.hasPrefix(homePath + "/") else {
            throw BridgeFailure(.invalid, "Chỉ mở được thư mục trong thư mục home của Mac.")
        }
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: resolved.path, isDirectory: &isDirectory), isDirectory.boolValue else {
            throw BridgeFailure(.notFound, "Thư mục không tồn tại.")
        }
        return resolved
    }

    private func subscribe(_ body: [String: Any], _ connection: BridgeConnection) async throws -> Any {
        var phoneOptions: PhoneOptions?
        if let phone = body["phone"] as? [String: Any] {
            phoneOptions = PhoneOptions(deviceWindowId: (phone["deviceWindowId"] as? Int).map { UInt32(truncatingIfNeeded: $0) },
                                        fps: min(max((phone["fps"] as? Int) ?? 10, 1), 15),
                                        width: min(max((phone["width"] as? Int) ?? 400, 100), 600))
        }
        let sessionId = body["sessionId"] as? String
        if let sessionId { _ = try await session(sessionId) }

        let sessions = await backend.sessions()
        if (body["sessions"] as? Bool) == true {
            connection.wantsSessions = true
            connection.knownPromptIds = Set(sessions.compactMap { $0.pending?.id })
            let json = hub.sessionsJSON(sessions)
            connection.lastSessionsJSON = json
            connection.io.sendText(BridgeWire.event("sessions", ["data": BridgeWire.object(sessions.map(SessionDTO.init))]))
        }
        if let sessionId {
            connection.chatSessionId = sessionId
            connection.chatSubagentId = body["subagentId"] as? String
            let current = (try? await backend.chatMessages(sessionId: sessionId, subagentId: connection.chatSubagentId)) ?? []
            // The phone says the newest message it already has: everything after it is news and gets pushed, so a reply
            // that arrived while the phone was on another session is not lost. Unknown or absent: all current is known.
            if let last = body["lastMessageId"] as? String, let index = current.firstIndex(where: { $0.id.uuidString == last }) {
                connection.knownChatIds = Set(current[...index].map { $0.id.uuidString })
            } else {
                connection.knownChatIds = Set(current.map { $0.id.uuidString })
            }
        }
        if let phoneOptions {
            try await backend.startPhone(connection: connection, options: phoneOptions)
            connection.phoneActive = true
        }
        return ["subscribed": true]
    }

    private func unsubscribe(_ connection: BridgeConnection) async {
        connection.wantsSessions = false
        connection.chatSessionId = nil
        connection.chatSubagentId = nil
        if connection.phoneActive {
            connection.phoneActive = false
            await backend.stopPhone(connection: connection)
        }
    }

    // MARK: - Errors

    private func reply(_ connection: BridgeConnection, id: Any?, failure: BridgeFailure) {
        connection.io.sendText(BridgeWire.error(id: id, failure))
    }

    static func failure(from error: Error, op: String) -> BridgeFailure {
        switch error {
        case let failure as BridgeFailure: return failure
        case let send as SendError:
            switch send {
            case .viewOnly: return BridgeFailure(.viewOnly, send.localizedDescription)
            case .notOwned: return BridgeFailure(.notFound, send.localizedDescription)
            case .failed(let why): return BridgeFailure(.internal, why)
            }
        case let responder as HookResponder.Failure:
            return BridgeFailure(.invalid, responder.localizedDescription)
        default:
            ConsoleLog.error("BridgeRouter.\(op)", "", "\(error)")
            return BridgeFailure(.internal, "Lỗi nội bộ trên Mac.")
        }
    }
}

/// At most one interrupt per session every few seconds: a second ESC would cancel the queued message that just started.
final class SendNowGate: @unchecked Sendable {
    private let lock = NSLock()
    private var last: [String: Date] = [:]
    private let interval: TimeInterval = 3

    func allow(_ sessionId: String, at date: Date) -> Bool {
        lock.withLock {
            if let previous = last[sessionId], date.timeIntervalSince(previous) < interval { return false }
            last[sessionId] = date
            return true
        }
    }
}

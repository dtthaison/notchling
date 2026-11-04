import AppKit
import ConsoleCore
import Foundation
import UserNotifications

/// What the bridge router asks of the running app. Everything that touches UI state hops to the main actor;
/// transcript and viz work stays off it.
final class LiveBridgeBackend: BridgeBackend, @unchecked Sendable {
    private let model: PhoneModel
    private let controller: BridgeController
    private let chatCache = ChatTranscriptCache()
    private let vizEngine = VizEngine()
    private let phone = PhoneStreamer()

    init(model: PhoneModel, controller: BridgeController) {
        self.model = model
        self.controller = controller
    }

    func sessions() async -> [Session] {
        let list = await rawSessions()
        let limits = await MainActor.run { PhoneService.shared.limitsSource() }
        return list.map { session in
            var copy = session
            copy.takeover = takeoverState(for: session)
            enrich(&copy, limits: limits)
            return copy
        }
    }

    /// What Notchling already knows about a session, for the phone's list and chat header: the activity line ("Editing
    /// main.swift"), the context in use and the account's limits. Both readers keep a cursor per transcript and read only
    /// what was appended, so this is cheap on every list push.
    private func enrich(_ session: inout Session, limits: [LimitInfo]) {
        guard session.state != .exited, let path = session.transcriptURL?.path,
              Date().timeIntervalSince(session.lastActivity) < 6 * 3600 else { return }
        if session.state == .running { session.activity = TranscriptReader.snapshot(atPath: path).activity }
        let conversation = ConversationReader.read(path)
        let facts = conversation.facts
        if let item = conversation.items.last(where: { if case .assistant = $0.kind { return true } else { return false } }),
           case .assistant(let text) = item.kind {
            session.lastMessage = LastMessageInfo(id: item.id, text: String(text.trimmingCharacters(in: .whitespacesAndNewlines).prefix(240)))
        }
        guard facts.contextTokens > 0 || !limits.isEmpty else { return }
        session.facts = SessionFactsInfo(
            contextTokens: facts.contextTokens > 0 ? facts.contextTokens : nil,
            contextWindow: ChatFormat.contextWindow(model: facts.model),
            model: facts.model,
            limits: limits)
    }

    func commands(sessionId: String) async -> [CommandInfo] {
        guard let session = await rawSessions().first(where: { $0.id == sessionId }) else { return [] }
        let cwd = session.cwd
        return await Task.detached(priority: .utility) {
            SlashCommands.all(for: cwd).map { CommandInfo(name: $0.name, description: $0.description, argumentHint: $0.argumentHint) }
        }.value
    }

    private let takeoverLock = NSLock()
    private var takeoverMemo: [String: (at: Date, state: TakeoverState?)] = [:]

    /// The checks read ~/.claude, and the list is built several times a second, so each answer is kept for 3 s.
    private func takeoverState(for session: Session) -> TakeoverState? {
        guard session.control == .viewOnly else { return nil }
        let now = Date()
        takeoverLock.lock()
        if let memo = takeoverMemo[session.id], now.timeIntervalSince(memo.at) < 3 { takeoverLock.unlock(); return memo.state }
        takeoverLock.unlock()
        let state = Takeover.state(for: session, paths: model.paths)
        takeoverLock.lock(); takeoverMemo[session.id] = (now, state); takeoverLock.unlock()
        return state
    }

    private func rawSessions() async -> [Session] {
        await MainActor.run { model.bridgeSessions }
    }

    private func session(_ id: String) async throws -> Session {
        guard let found = await sessions().first(where: { $0.id == id }) else {
            throw BridgeFailure(.notFound, "Không có session này.")
        }
        return found
    }

    func chatMessages(sessionId: String, subagentId: String?) async throws -> [ChatMessage] {
        let session = try await session(sessionId)
        let url: URL?
        let isMain: Bool
        if let subagentId {
            url = session.subagents.first { $0.id == subagentId }?.transcriptURL
            isMain = false
        } else {
            url = session.transcriptURL
            isMain = true
        }
        guard let url else { throw BridgeFailure(.notFound, "Chưa tìm thấy transcript.") }
        return chatCache.messages(at: url, isMain: isMain)
    }

    func chatPage(sessionId: String, subagentId: String?, before: String?, after: String?, limit: Int) async throws -> BridgeChatPage {
        let session = try await session(sessionId)
        let url: URL?
        let isMain: Bool
        if let subagentId {
            url = session.subagents.first { $0.id == subagentId }?.transcriptURL
            isMain = false
        } else {
            url = session.transcriptURL
            isMain = true
        }
        guard let url else { throw BridgeFailure(.notFound, "Chưa tìm thấy transcript.") }
        let cache = chatCache
        return try await Task.detached(priority: .userInitiated) { () -> BridgeChatPage in
            if let before {
                guard let id = UUID(uuidString: before), let page = cache.page(at: url, isMain: isMain, before: id, limit: limit) else {
                    throw BridgeFailure(.notFound, "Không thấy tin \"before\".")
                }
                return BridgeChatPage(messages: page.messages, hasMore: page.hasMore)
            }
            if let after, let id = UUID(uuidString: after), let newer = cache.newer(at: url, isMain: isMain, after: id, limit: limit) {
                if newer.found { return BridgeChatPage(messages: newer.messages, hasMore: newer.hasMore) }
                let latest = cache.latest(at: url, isMain: isMain, limit: limit)
                return BridgeChatPage(messages: latest?.messages ?? [], hasMore: latest?.hasMore ?? false, reset: true)
            }
            let latest = cache.latest(at: url, isMain: isMain, limit: limit)
            return BridgeChatPage(messages: latest?.messages ?? [], hasMore: latest?.hasMore ?? false)
        }.value
    }

    func chatImage(sessionId: String, imageId: String, maxWidth: Int) async throws -> Data {
        let session = try await session(sessionId)
        guard let url = session.transcriptURL else { throw BridgeFailure(.notFound, "Chưa tìm thấy transcript.") }
        let cache = chatCache
        return try await Task.detached(priority: .userInitiated) {
            do { return try cache.image(at: url, id: imageId, maxWidth: maxWidth) } catch let error as ImageError {
                throw BridgeFailure(error == .notAllowed ? .invalid : .notFound, error.localizedDescription)
            }
        }.value
    }

    func viz(sessionId: String, includeEntity: Bool) async throws -> BridgeViz {
        let session = try await session(sessionId)
        let engine = vizEngine
        return await Task.detached(priority: .utility) {
            let bundle = engine.build(session: session)
            var specs: [String: VizSpec] = ["flow": bundle.flow, "diagram": bundle.diagram, "chart": bundle.chart]
            if includeEntity { specs["entity"] = engine.entitySpec(cwd: session.cwd) }
            let tab = VisualizeSuggester().suggest(cwd: session.cwd, signals: bundle.signals)
            return BridgeViz(suggestedTab: tab.rawValue, webURL: bundle.webURL?.absoluteString, specs: specs,
                              webCandidates: bundle.webCandidates.map(\.absoluteString))
        }.value
    }

    func webSnapshot(url: URL, width: Int) async throws -> Data {
        guard await Self.isReachable(url) else {
            throw BridgeFailure(.notReachable, "\(url.host ?? "localhost"):\(url.port.map(String.init) ?? "80") không chạy trên Mac (không trả lời trong 1 giây).")
        }
        return try await WebSnapshotter.snapshot(url: url, width: width)
    }

    func sendMessage(_ text: String, to session: Session) async throws {
        try await model.deliver(text, to: session)
    }

    func sendNow(session: Session) async throws {
        guard let pid = session.pid, let pty = model.owned.pty(owning: pid) else { throw SendError.notOwned }
        pty.write("\u{1b}")
    }

    func pendingPrompt(requestId: String) async -> PendingPrompt? {
        await MainActor.run { model.sessions.compactMap(\.pending).first { $0.id == requestId } }
    }

    func respond(to prompt: PendingPrompt, with response: PromptResponse) async throws {
        try await MainActor.run { try model.submit(response, to: prompt) }
    }

    func newSession(cwd: URL, model chosen: String?) async throws {
        try await MainActor.run { try model.launchSession(folder: cwd, model: chosen) }
        notifyNewSession(folder: cwd)
    }

    func takeOver(session: Session, stopOriginal: Bool) async throws -> TakeoverOutcome {
        do {
            let pty: any TerminalHandle
            var stoppedPid: Int32?
            if stopOriginal {
                let result = try await model.takeOverStoppingOriginal(sessionId: session.id)
                pty = result.pty
                stoppedPid = result.stoppedPid
            } else {
                pty = try await MainActor.run { try model.takeOver(sessionId: session.id) }
            }
            await MainActor.run { controller.recordTakeover(sessionName: session.name, from: "Điện thoại", pty: pty, stoppedPid: stoppedPid) }
            // The phone wants the real session; claude writes its session file after its startup prompts.
            for _ in 0..<40 {
                if let real = await MainActor.run(body: { model.session(ownedBy: pty) }) { return TakeoverOutcome(sessionId: real.id, stoppedPid: stoppedPid) }
                try await Task.sleep(nanoseconds: 500_000_000)
            }
            return TakeoverOutcome(sessionId: "pty:\(pty.id.uuidString)", stoppedPid: stoppedPid)
        } catch let error as TakeoverError {
            throw BridgeFailure(error == .alive ? .busy : .invalid, error.localizedDescription)
        } catch let error as OriginalStopError {
            if case .stillRunning = error { throw BridgeFailure(.busy, error.localizedDescription) }
            throw BridgeFailure(.invalid, error.localizedDescription)
        }
    }

    func recentProjects() async -> [String] {
        let sessions = await sessions()
        return RecentProjects.list(sessions: sessions, projectsDir: ClaudePaths.live.projectsDir)
    }

    func requestApproval(deviceName: String, platform: String) async -> Bool {
        await controller.requestApproval(deviceName: deviceName, platform: platform)
    }

    func startPhone(connection: BridgeConnection, options: PhoneOptions) async throws {
        try await phone.start(connection: connection, options: options)
    }

    func stopPhone(connection: BridgeConnection) async {
        await phone.stop(connection: connection)
    }

    private func notifyNewSession(folder: URL) {
        let content = UNMutableNotificationContent()
        content.title = "Điện thoại đã mở session mới"
        content.body = folder.lastPathComponent
        let request = UNNotificationRequest(identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request) { error in
            if let error { ConsoleLog.error("notifyNewSession", folder.path, "\(error)") }
        }
    }

    /// HEAD with a 1 s timeout; any HTTP answer, whatever its status, means a server is there.
    private static func isReachable(_ url: URL) async -> Bool {
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 1)
        request.httpMethod = "HEAD"
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 1
        configuration.timeoutIntervalForResource = 1
        do {
            let (_, response) = try await URLSession(configuration: configuration).data(for: request)
            return response is HTTPURLResponse
        } catch {
            ConsoleLog.logger.debug("probe \(url.absoluteString, privacy: .public): \(error.localizedDescription, privacy: .public)")
            return false
        }
    }
}

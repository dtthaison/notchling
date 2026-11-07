import Foundation

/// The live clients, and what changed since each one last heard: sessions, prompts and chat messages.
/// `tick` is called every 300 ms, which is also what debounces the pushes.
public final class BridgeHub: @unchecked Sendable {
    private let lock = NSLock()
    private var connections: [UUID: BridgeConnection] = [:]

    public init() {}

    public func add(_ connection: BridgeConnection) {
        lock.lock(); connections[connection.id] = connection; lock.unlock()
    }

    public func remove(_ connection: BridgeConnection) {
        lock.lock(); connections[connection.id] = nil; lock.unlock()
    }

    public var all: [BridgeConnection] {
        lock.lock(); defer { lock.unlock() }
        return Array(connections.values)
    }

    /// Closes every live connection of a device (after revoke).
    @discardableResult
    public func disconnect(deviceId: String) -> Int {
        let targets = all.filter { $0.device?.id == deviceId }
        for connection in targets { connection.close() }
        return targets.count
    }

    /// Tells every logged-in client to open (the user picked "Open Agent Console" in Notchling).
    /// Clients that have not authenticated get nothing. Returns how many were told.
    @discardableResult
    public func notifyOpen() -> Int {
        let targets = all.filter(\.isAuthenticated)
        let message = BridgeWire.event("open", [:])
        for connection in targets { connection.io.sendText(message) }
        return targets.count
    }

    func sessionsJSON(_ sessions: [Session]) -> String {
        BridgeWire.serialize(["data": BridgeWire.object(sessions.map(SessionDTO.init))])
    }

    public func tick(backend: BridgeBackend) async {
        let subscribers = all.filter { $0.isAuthenticated && ($0.wantsSessions || $0.chatSessionId != nil) }
        guard !subscribers.isEmpty else { return }

        if subscribers.contains(where: \.wantsSessions) {
            let sessions = await backend.sessions()
            let json = sessionsJSON(sessions)
            let prompts = sessions.compactMap(\.pending)
            let promptIds = Set(prompts.map(\.id))
            for connection in subscribers where connection.wantsSessions {
                if connection.lastSessionsJSON != json {
                    connection.lastSessionsJSON = json
                    connection.io.sendText(BridgeWire.event("sessions", ["data": BridgeWire.object(sessions.map(SessionDTO.init))]))
                }
                for prompt in prompts where !connection.knownPromptIds.contains(prompt.id) {
                    connection.io.sendText(BridgeWire.event("prompt", ["sessionId": prompt.sessionId, "prompt": BridgeWire.object(PromptDTO(prompt))]))
                }
                for resolved in connection.knownPromptIds.subtracting(promptIds) {
                    connection.io.sendText(BridgeWire.event("prompt", ["requestId": resolved, "resolved": true]))
                }
                connection.knownPromptIds = promptIds
            }
        }

        for connection in subscribers {
            guard let sessionId = connection.chatSessionId else { continue }
            let messages = (try? await backend.chatMessages(sessionId: sessionId, subagentId: connection.chatSubagentId)) ?? []
            let fresh = messages.filter { !connection.knownChatIds.contains($0.id.uuidString) }
            connection.knownChatIds = Set(messages.map { $0.id.uuidString })
            if !fresh.isEmpty {
                connection.io.sendText(BridgeWire.event("chat", ["sessionId": sessionId, "messages": BridgeWire.object(fresh.map(ChatMessageDTO.init))]))
            }
        }
    }
}

/// Everything the bridge keeps on disk and in memory, wired together. The app owns one.
public final class BridgeCore: @unchecked Sendable {
    public let devices: DeviceStore
    public let codes = PairingCodes()
    public let limiter = AuthRateLimiter()
    public let audit: AuditLog
    public let hub = BridgeHub()
    public let router: BridgeRouter
    public let access: AccessConfigStore
    public let gate: TunnelGate

    private let tunnelLock = NSLock()
    private var _lastTunnelConnection: Date?
    /// When a client last got in through the Cloudflare tunnel (nil: not since the Mac app started).
    public var lastTunnelConnection: Date? { tunnelLock.withLock { _lastTunnelConnection } }

    public func recordTunnelConnection(at date: Date = Date()) { tunnelLock.withLock { _lastTunnelConnection = date } }

    public init(directory: URL, backend: BridgeBackend, home: URL = FileManager.default.homeDirectoryForCurrentUser,
                fetchAccessKeys: @escaping AccessKeySet.Fetch = AccessKeySet.liveFetch,
                now: @escaping () -> Date = Date.init) {
        let access = AccessConfigStore(fileURL: directory.appendingPathComponent("access.json"))
        self.access = access
        gate = TunnelGate(config: { access.config }, verifier: AccessJWTVerifier(keys: AccessKeySet(fetch: fetchAccessKeys)), limiter: limiter)
        devices = DeviceStore(fileURL: directory.appendingPathComponent("devices.json"), now: now)
        audit = AuditLog(fileURL: directory.appendingPathComponent("remote-log.jsonl"), now: now)
        router = BridgeRouter(backend: backend, devices: devices, codes: codes, limiter: limiter, audit: audit,
                              hub: hub, home: home, now: now)
    }

    /// Handles `agentconsole://open?phones=1`: pushes the event to phones and audits it. Only called when
    /// the bridge is running; returns the number of phones told.
    @discardableResult
    public func handleOpen(phones: Bool) -> Int {
        guard phones else { return 0 }
        let count = hub.notifyOpen()
        audit.append(device: "mac", op: "open", session: nil, length: count)
        ConsoleLog.logger.notice("bridge: open event sent to \(count) phone(s)")
        return count
    }

    /// Removes the device and drops its live connections. Its token stops working at once.
    public func revoke(deviceId: String) throws {
        guard let removed = try devices.revoke(id: deviceId) else { return }
        audit.append(device: removed.name, op: "revoke", session: nil, length: nil)
        ConsoleLog.logger.notice("bridge: revoked \(removed.name, privacy: .public), closed \(self.hub.disconnect(deviceId: deviceId)) connection(s)")
    }
}

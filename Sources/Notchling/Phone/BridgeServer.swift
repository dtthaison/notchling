import ConsoleCore
import Foundation
import Network

/// One client on the wire.
final class NWBridgeConnection: BridgeConnectionIO, @unchecked Sendable {
    let connection: NWConnection
    private let queue: DispatchQueue
    private let lock = NSLock()
    private var outstandingPings = 0

    init(_ connection: NWConnection, queue: DispatchQueue) {
        self.connection = connection
        self.queue = queue
    }

    func sendText(_ text: String) {
        send(Data(text.utf8), opcode: .text) {}
    }

    func sendBinary(_ data: Data, completion: @escaping @Sendable () -> Void) {
        send(data, opcode: .binary, completion: completion)
    }

    /// Sends a WebSocket close frame behind anything already queued (an error answer, say), then cancels.
    /// Cancelling at once could drop that answer, and the phone would see a bare disconnect instead of why.
    func close() {
        let metadata = NWProtocolWebSocket.Metadata(opcode: .close)
        metadata.closeCode = .protocolCode(.normalClosure)
        let context = NWConnection.ContentContext(identifier: "close", metadata: [metadata])
        connection.send(content: nil, contentContext: context, isComplete: true, completion: .contentProcessed { [connection] _ in
            connection.cancel()
        })
        queue.asyncAfter(deadline: .now() + 2) { [connection] in connection.cancel() }
    }

    private func send(_ data: Data, opcode: NWProtocolWebSocket.Opcode, completion: @escaping @Sendable () -> Void) {
        let metadata = NWProtocolWebSocket.Metadata(opcode: opcode)
        let context = NWConnection.ContentContext(identifier: "bridge", metadata: [metadata])
        connection.send(content: data, contentContext: context, isComplete: true, completion: .contentProcessed { error in
            if let error { ConsoleLog.logger.error("bridge send failed: \(error.localizedDescription, privacy: .public)") }
            completion()
        })
    }

    /// Every 20 s: one more ping out. A client that has two unanswered is dropped.
    func pingTick() {
        lock.lock()
        if outstandingPings >= 2 {
            lock.unlock()
            ConsoleLog.logger.notice("bridge: dropping a client that missed 2 pings")
            connection.cancel()
            return
        }
        outstandingPings += 1
        lock.unlock()
        let metadata = NWProtocolWebSocket.Metadata(opcode: .ping)
        metadata.setPongHandler(queue) { [weak self] _ in
            self?.lock.lock(); self?.outstandingPings = 0; self?.lock.unlock()
        }
        connection.send(content: nil, contentContext: NWConnection.ContentContext(identifier: "ping", metadata: [metadata]),
                        isComplete: true, completion: .contentProcessed { _ in })
    }
}

/// The TLS WebSocket listener. Network.framework is confined to this file and `NWBridgeConnection`;
/// what clients may do is decided by `BridgeRouter` in ConsoleCore.
final class BridgeServer: @unchecked Sendable {
    static let subprotocol = "agentconsole.v1"
    static let defaultPort: UInt16 = BridgePorts.lan

    /// Response header the upgrade handler uses to tell `accept` which way a client came in. The handshake
    /// handler cannot see the connection, and tunnel clients all arrive from loopback, so this is how the
    /// decision (and the real client address) reaches the connection. A connection without it is closed.
    static let routeHeader = "X-Agent-Console-Route"

    private let core: BridgeCore
    private let backend: BridgeBackend
    private let queue = DispatchQueue(label: "local.agentconsole.bridge")
    /// The Access token check may fetch Cloudflare's keys (a blocking GET), so it gets its own queue.
    private let handshakeQueue = DispatchQueue(label: "local.agentconsole.bridge.handshake")
    private var listener: NWListener?
    private var connections: [UUID: NWBridgeConnection] = [:]
    private var tickTimer: DispatchSourceTimer?
    private var pingTimer: DispatchSourceTimer?
    private var ticking = false

    init(core: BridgeCore, backend: BridgeBackend) {
        self.core = core
        self.backend = backend
    }

    /// Starts listening on the fixed port. A busy port is an error, not a reason to pick another: phones and
    /// the pairing QR rely on 47821.
    ///
    /// Known limit: NWProtocolWebSocket answers nothing to a request that is not a clean WebSocket upgrade
    /// (a plain GET, or "Connection: keep-alive, Upgrade"); the connection just sits. The tunnel does not
    /// use this listener (see TunnelOriginServer), and LAN phones send a plain upgrade.
    func start(identity: BridgeIdentity, port: UInt16, serviceName: String) async throws {
        _ = try await listen(identity: identity, port: NWEndpoint.Port(rawValue: port), serviceName: serviceName)
    }

    private func listen(identity: BridgeIdentity, port: NWEndpoint.Port?, serviceName: String) async throws -> UInt16 {
        let tls = NWProtocolTLS.Options()
        sec_protocol_options_set_local_identity(tls.securityProtocolOptions, identity.identity)
        sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions, .TLSv12)

        let webSocket = NWProtocolWebSocket.Options()
        webSocket.autoReplyPing = true
        webSocket.maximumMessageSize = BridgeRouter.maxMessageBytes
        // The handshake does not expose the request path, so clients identify themselves with this
        // subprotocol instead; anything else is refused before it can send a frame.
        let gate = core.gate
        webSocket.setClientRequestHandler(handshakeQueue) { subprotocols, headers in
            guard subprotocols.contains(Self.subprotocol) else {
                return NWProtocolWebSocket.Response(status: .reject, subprotocol: nil)
            }
            switch gate.evaluate(headers: headers) {
            case .local:
                return NWProtocolWebSocket.Response(status: .accept, subprotocol: Self.subprotocol,
                                                    additionalHeaders: [(Self.routeHeader, "local")])
            case .tunnel(let clientAddress):
                return NWProtocolWebSocket.Response(status: .accept, subprotocol: Self.subprotocol,
                                                    additionalHeaders: [(Self.routeHeader, "tunnel \(clientAddress)")])
            case .reject(let reason):
                ConsoleLog.logger.notice("bridge: upgrade refused (\(reason, privacy: .public))")
                return NWProtocolWebSocket.Response(status: .reject, subprotocol: nil)
            }
        }

        let parameters = NWParameters(tls: tls, tcp: NWProtocolTCP.Options())
        parameters.defaultProtocolStack.applicationProtocols.insert(webSocket, at: 0)
        let listener = try port.map { try NWListener(using: parameters, on: $0) } ?? NWListener(using: parameters)
        let fingerprintPrefix = String(identity.fingerprint.prefix(16))
        listener.service = NWListener.Service(name: serviceName, type: "_agentconsole._tcp",
                                              txtRecord: NWTXTRecord(["fp": fingerprintPrefix, "v": "1"]))
        listener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }

        let resolved: UInt16 = try await withCheckedThrowingContinuation { continuation in
            var finished = false
            listener.stateUpdateHandler = { [weak self] state in
                switch state {
                case .ready:
                    if !finished, let value = listener.port?.rawValue { finished = true; continuation.resume(returning: value) }
                case .failed(let error):
                    ConsoleLog.error("BridgeServer.listen", "listener", "\(error)")
                    if !finished { finished = true; continuation.resume(throwing: error) } else { self?.stop() }
                case .cancelled:
                    if !finished { finished = true; continuation.resume(throwing: CancellationError()) }
                default:
                    break
                }
            }
            listener.start(queue: queue)
        }
        self.listener = listener
        startTimers()
        ConsoleLog.logger.notice("bridge: listening on port \(resolved)")
        return resolved
    }

    func stop() {
        queue.async { [self] in
            tickTimer?.cancel(); tickTimer = nil
            pingTimer?.cancel(); pingTimer = nil
            listener?.cancel(); listener = nil
            for connection in connections.values { connection.close() }
            connections = [:]
        }
    }

    // MARK: - Connections

    private func accept(_ nwConnection: NWConnection) {
        let address = Self.remoteAddress(of: nwConnection)
        guard let address, AddressFilter.isAllowed(address) else {
            ConsoleLog.logger.notice("bridge: refused a connection from \(address ?? "unknown", privacy: .public)")
            nwConnection.cancel()
            return
        }
        let io = NWBridgeConnection(nwConnection, queue: queue)
        // The BridgeConnection is made once the upgrade is done and the route header is readable.
        var attached: BridgeConnection?
        nwConnection.stateUpdateHandler = { [weak self] state in
            guard let self else { return }
            switch state {
            case .ready:
                self.queue.async {
                    guard attached == nil, let route = Self.route(of: nwConnection) else {
                        ConsoleLog.logger.error("bridge: no route on a ready connection, closing")
                        nwConnection.cancel()
                        return
                    }
                    let bridgeConnection: BridgeConnection
                    switch route {
                    case .local:
                        bridgeConnection = BridgeConnection(remoteAddress: address, io: io)
                    case .tunnel(let clientAddress):
                        bridgeConnection = BridgeConnection(remoteAddress: clientAddress, io: io, isTunnel: true)
                        self.core.recordTunnelConnection()
                    }
                    attached = bridgeConnection
                    self.core.hub.add(bridgeConnection)
                    self.connections[bridgeConnection.id] = io
                    self.receive(on: io, bridgeConnection)
                }
            case .failed, .cancelled:
                self.queue.async {
                    guard let bridgeConnection = attached else { return }
                    self.connections[bridgeConnection.id] = nil
                    self.core.hub.remove(bridgeConnection)
                    Task { [backend = self.backend] in await backend.stopPhone(connection: bridgeConnection) }
                }
            default:
                break
            }
        }
        nwConnection.start(queue: queue)
    }

    enum Route: Equatable {
        case local
        case tunnel(clientAddress: String)
    }

    /// Reads back what the upgrade handler decided, from the response headers on the connection.
    static func route(of connection: NWConnection) -> Route? {
        guard let metadata = connection.metadata(definition: NWProtocolWebSocket.definition) as? NWProtocolWebSocket.Metadata,
              let value = metadata.additionalServerHeaders?.first(where: { $0.0.lowercased() == routeHeader.lowercased() })?.1 else { return nil }
        if value == "local" { return .local }
        if value.hasPrefix("tunnel ") { return .tunnel(clientAddress: String(value.dropFirst("tunnel ".count))) }
        return nil
    }

    private func receive(on io: NWBridgeConnection, _ bridgeConnection: BridgeConnection) {
        io.connection.receiveMessage { [weak self] data, context, _, error in
            guard let self else { return }
            if let error {
                ConsoleLog.logger.debug("bridge receive ended: \(error.localizedDescription, privacy: .public)")
                io.connection.cancel()
                return
            }
            let metadata = context?.protocolMetadata(definition: NWProtocolWebSocket.definition) as? NWProtocolWebSocket.Metadata
            switch metadata?.opcode {
            case .text:
                if let data, let text = String(data: data, encoding: .utf8) {
                    Task { await self.core.router.handle(text, from: bridgeConnection) }
                }
            case .close:
                io.connection.cancel()
                return
            default:
                break  // binary frames from clients are not part of the protocol
            }
            self.receive(on: io, bridgeConnection)
        }
    }

    /// The peer's IP as text, from the connection endpoint (nil when it is not an address).
    static func remoteAddress(of connection: NWConnection) -> String? {
        guard case let .hostPort(host, _) = connection.endpoint else { return nil }
        switch host {
        case .ipv4(let address): return "\(address)".components(separatedBy: "%").first
        case .ipv6(let address): return "\(address)"
        default: return nil
        }
    }

    // MARK: - Timers

    private func startTimers() {
        let tick = DispatchSource.makeTimerSource(queue: queue)
        tick.schedule(deadline: .now() + 0.3, repeating: .milliseconds(300), leeway: .milliseconds(50))
        tick.setEventHandler { [weak self] in
            guard let self, !self.ticking else { return }
            self.ticking = true
            Task {
                await self.core.hub.tick(backend: self.backend)
                self.queue.async { self.ticking = false }
            }
        }
        tick.resume()
        tickTimer = tick

        let ping = DispatchSource.makeTimerSource(queue: queue)
        ping.schedule(deadline: .now() + 20, repeating: .seconds(20))
        ping.setEventHandler { [weak self] in
            self?.connections.values.forEach { $0.pingTick() }
        }
        ping.resume()
        pingTimer = ping
    }
}

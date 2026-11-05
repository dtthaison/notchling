import ConsoleCore
import Foundation
import Network

/// One tunnel client after the upgrade: RFC 6455 frames over a plain TCP connection from loopback.
final class RawWebSocketIO: BridgeConnectionIO, @unchecked Sendable {
    let connection: NWConnection
    private let queue: DispatchQueue
    private let lock = NSLock()
    private var outstandingPings = 0

    init(_ connection: NWConnection, queue: DispatchQueue) {
        self.connection = connection
        self.queue = queue
    }

    func sendText(_ text: String) { write(WSFrame.encode(opcode: WSFrame.text, payload: Data(text.utf8))) }

    func sendBinary(_ data: Data, completion: @escaping @Sendable () -> Void) {
        write(WSFrame.encode(opcode: WSFrame.binary, payload: data), completion: completion)
    }

    func sendPong(_ payload: Data) { write(WSFrame.encode(opcode: WSFrame.pong, payload: payload)) }

    func pongReceived() { lock.lock(); outstandingPings = 0; lock.unlock() }

    /// A close frame behind whatever is queued, then the socket goes.
    func close(code: UInt16 = 1000) {
        connection.send(content: WSFrame.closeFrame(code: code), completion: .contentProcessed { [connection] _ in connection.cancel() })
        queue.asyncAfter(deadline: .now() + 2) { [connection] in connection.cancel() }
    }

    func close() { close(code: 1000) }

    func pingTick() {
        lock.lock()
        if outstandingPings >= 2 { lock.unlock(); connection.cancel(); return }
        outstandingPings += 1
        lock.unlock()
        write(WSFrame.encode(opcode: WSFrame.ping, payload: Data()))
    }

    private func write(_ data: Data, completion: @escaping @Sendable () -> Void = {}) {
        connection.send(content: data, completion: .contentProcessed { error in
            if let error { ConsoleLog.logger.error("tunnel send failed: \(error.localizedDescription, privacy: .public)") }
            completion()
        })
    }
}

/// The tunnel origin: cloudflared connects here over plain HTTP on loopback and Cloudflare Access has
/// already done TLS. This does the WebSocket server side by hand, because NWProtocolWebSocket stalled on
/// cloudflared's upgrade request without ever calling its handler. Every request is answered with a real
/// HTTP status; nothing is left hanging.
final class TunnelOriginServer: @unchecked Sendable {
    static let port: UInt16 = BridgePorts.tunnel

    private let core: BridgeCore
    private let backend: BridgeBackend
    private let queue = DispatchQueue(label: "local.agentconsole.bridge.tunnel")
    /// Verifying the Access token may fetch Cloudflare's keys (a blocking GET), so it runs off `queue`.
    private let gateQueue = DispatchQueue(label: "local.agentconsole.bridge.tunnel.gate", attributes: .concurrent)
    private var listener: NWListener?
    private var clients: [UUID: RawWebSocketIO] = [:]
    private var pingTimer: DispatchSourceTimer?

    init(core: BridgeCore, backend: BridgeBackend) {
        self.core = core
        self.backend = backend
    }

    func start() async throws {
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: .ipv4(.loopback), port: NWEndpoint.Port(rawValue: Self.port)!)
        let listener = try NWListener(using: parameters)
        listener.newConnectionHandler = { [weak self] connection in self?.accept(connection) }
        try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
            let finished = Flag()
            listener.stateUpdateHandler = { [weak self] state in
                switch state {
                case .ready:
                    if finished.set() { continuation.resume() }
                case .failed(let error):
                    ConsoleLog.error("TunnelOriginServer.start", "127.0.0.1:\(Self.port)", "\(error)")
                    if finished.set() { continuation.resume(throwing: error) } else { self?.stop() }
                case .cancelled:
                    if finished.set() { continuation.resume(throwing: CancellationError()) }
                default:
                    break
                }
            }
            listener.start(queue: queue)
        }
        self.listener = listener
        let ping = DispatchSource.makeTimerSource(queue: queue)
        ping.schedule(deadline: .now() + 20, repeating: .seconds(20))
        ping.setEventHandler { [weak self] in self?.clients.values.forEach { $0.pingTick() } }
        ping.resume()
        pingTimer = ping
        ConsoleLog.logger.notice("bridge: tunnel origin listening on 127.0.0.1:\(Self.port)")
    }

    func stop() {
        queue.async { [self] in
            pingTimer?.cancel(); pingTimer = nil
            listener?.cancel(); listener = nil
            for client in clients.values { client.connection.cancel() }
            clients = [:]
        }
    }

    private final class Flag: @unchecked Sendable {
        private let lock = NSLock()
        private var done = false
        func set() -> Bool { lock.lock(); defer { lock.unlock() }; if done { return false }; done = true; return true }
    }

    // MARK: - One connection

    private func accept(_ connection: NWConnection) {
        guard let address = BridgeServer.remoteAddress(of: connection), address == "127.0.0.1" || address == "::1" else {
            connection.cancel()
            return
        }
        var reader = HTTPHeadReader()
        var attached: BridgeConnection?
        var io: RawWebSocketIO?
        let timeout = DispatchWorkItem { [weak self] in
            guard attached == nil else { return }
            self?.refuse(connection, HTTPResponse.build(status: 408, body: "request timeout\n"))
        }
        queue.asyncAfter(deadline: .now() + HTTPHeadReader.timeout, execute: timeout)

        connection.stateUpdateHandler = { [weak self] state in
            switch state {
            case .failed, .cancelled:
                timeout.cancel()
                self?.queue.async {
                    guard let attached else { return }
                    self?.clients[attached.id] = nil
                    self?.core.hub.remove(attached)
                    Task { [backend = self?.backend] in await backend?.stopPhone(connection: attached) }
                }
            default:
                break
            }
        }
        connection.start(queue: queue)

        func readHead() {
            connection.receive(minimumIncompleteLength: 1, maximumLength: 4096) { [weak self] data, _, isComplete, error in
                guard let self else { return }
                if let data, !data.isEmpty {
                    switch reader.feed(data) {
                    case .needMore:
                        if error == nil && !isComplete { readHead() } else { connection.cancel() }
                    case .failure(let status, let reason):
                        timeout.cancel()
                        ConsoleLog.logger.notice("bridge: tunnel request refused (\(reason, privacy: .public))")
                        self.refuse(connection, HTTPResponse.build(status: status, body: reason + "\n"))
                    case .complete(let head, let leftover):
                        timeout.cancel()
                        self.gateQueue.async {
                            let outcome = TunnelUpgrade.evaluate(head, gate: self.core.gate)
                            self.queue.async {
                                switch outcome {
                                case .refuse(let response, _):
                                    self.refuse(connection, response)
                                case .accept(let response, let clientAddress):
                                    let raw = RawWebSocketIO(connection, queue: self.queue)
                                    let bridgeConnection = BridgeConnection(remoteAddress: clientAddress, io: raw, isTunnel: true)
                                    io = raw
                                    attached = bridgeConnection
                                    self.clients[bridgeConnection.id] = raw
                                    self.core.hub.add(bridgeConnection)
                                    self.core.recordTunnelConnection()
                                    connection.send(content: response, completion: .contentProcessed { _ in })
                                    var decoder = WSFrameDecoder()
                                    self.pump(decoder.feed(leftover), raw, bridgeConnection)
                                    self.readFrames(connection, raw, bridgeConnection, decoder)
                                }
                            }
                        }
                    }
                } else if error != nil || isComplete {
                    connection.cancel()
                } else {
                    readHead()
                }
            }
        }
        readHead()
        _ = io
    }

    private func refuse(_ connection: NWConnection, _ response: Data) {
        connection.send(content: response, completion: .contentProcessed { _ in connection.cancel() })
        queue.asyncAfter(deadline: .now() + 2) { connection.cancel() }
    }

    private func readFrames(_ connection: NWConnection, _ io: RawWebSocketIO, _ bridgeConnection: BridgeConnection, _ decoder: WSFrameDecoder) {
        var decoder = decoder
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] data, _, isComplete, error in
            guard let self else { return }
            if let data, !data.isEmpty {
                if !self.pump(decoder.feed(data), io, bridgeConnection) { return }
            }
            if error != nil || isComplete { connection.cancel(); return }
            self.readFrames(connection, io, bridgeConnection, decoder)
        }
    }

    /// Acts on decoded events; false once the connection is being closed.
    @discardableResult
    private func pump(_ events: [WSFrameDecoder.Event], _ io: RawWebSocketIO, _ bridgeConnection: BridgeConnection) -> Bool {
        for event in events {
            switch event {
            case .text(let text):
                Task { await core.router.handle(text, from: bridgeConnection) }
            case .binary:
                break  // binary frames from clients are not part of the protocol
            case .ping(let payload):
                io.sendPong(payload)
            case .pong:
                io.pongReceived()
            case .close:
                io.close()
                return false
            case .failure(let code, let reason):
                ConsoleLog.logger.notice("bridge: tunnel client broke the framing (\(reason, privacy: .public))")
                io.close(code: code)
                return false
            }
        }
        return true
    }
}

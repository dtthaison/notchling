import Foundation

/// The wire side of one client. The live one wraps an NWConnection; tests use a recorder.
public protocol BridgeConnectionIO: AnyObject, Sendable {
    func sendText(_ text: String)
    /// `completion` runs when the frame has left; until then the connection counts as busy.
    func sendBinary(_ data: Data, completion: @escaping @Sendable () -> Void)
    func close()
}

/// 4-byte magic "PHN1", uint32 big-endian sequence, then the JPEG.
public enum PhoneFrame {
    public static let magic = Array("PHN1".utf8)

    public static func encode(sequence: UInt32, jpeg: Data) -> Data {
        var data = Data(magic)
        withUnsafeBytes(of: sequence.bigEndian) { data.append(contentsOf: $0) }
        data.append(jpeg)
        return data
    }

    /// Binary reply to chat.image: magic "IMG1", the request id, then the JPEG.
    public static func encodeImage(requestId: UInt32, jpeg: Data) -> Data {
        var data = Data("IMG1".utf8)
        withUnsafeBytes(of: requestId.bigEndian) { data.append(contentsOf: $0) }
        data.append(jpeg)
        return data
    }

    /// Binary reply to web.snapshot: magic "WEB1", the request id (0 when it is not a number), then the JPEG.
    public static func encodeSnapshot(requestId: UInt32, jpeg: Data) -> Data {
        var data = Data("WEB1".utf8)
        withUnsafeBytes(of: requestId.bigEndian) { data.append(contentsOf: $0) }
        data.append(jpeg)
        return data
    }
}

/// At most one phone frame in flight per client: a frame offered while the last one is still being
/// sent is dropped, never queued.
public final class FrameGate: @unchecked Sendable {
    private let lock = NSLock()
    private var inFlight = false
    private var sequence: UInt32 = 0
    public private(set) var dropped = 0
    public private(set) var sent = 0

    public init() {}

    @discardableResult
    public func offer(jpeg: Data, via io: BridgeConnectionIO) -> Bool {
        lock.lock()
        if inFlight { dropped += 1; lock.unlock(); return false }
        inFlight = true
        sequence &+= 1
        let frame = PhoneFrame.encode(sequence: sequence, jpeg: jpeg)
        sent += 1
        lock.unlock()
        io.sendBinary(frame) { [weak self] in
            self?.lock.lock(); self?.inFlight = false; self?.lock.unlock()
        }
        return true
    }
}

public final class BridgeConnection: @unchecked Sendable {
    public let id = UUID()
    /// The peer's address; for a tunnel client it is the real client (Cf-Connecting-Ip), not the loopback cloudflared uses.
    public let remoteAddress: String
    public let isTunnel: Bool
    public let io: BridgeConnectionIO
    public let frameGate = FrameGate()
    public let openedAt = Date()

    private let lock = NSLock()
    private var _device: PairedDevice?
    private var _closing = false
    private var _inflightSends: Set<String> = []

    // Subscription state, used by BridgeHub.
    var wantsSessions = false
    var chatSessionId: String?
    var chatSubagentId: String?
    var lastSessionsJSON: String?
    var knownPromptIds: Set<String> = []
    var knownChatIds: Set<String> = []
    var phoneActive = false

    public init(remoteAddress: String, io: BridgeConnectionIO, isTunnel: Bool = false) {
        self.remoteAddress = remoteAddress
        self.isTunnel = isTunnel
        self.io = io
    }

    public var device: PairedDevice? {
        lock.lock(); defer { lock.unlock() }
        return _device
    }

    func authenticate(as device: PairedDevice) {
        lock.lock(); _device = device; lock.unlock()
    }

    public var isAuthenticated: Bool { device != nil }

    /// Recorded in the audit log for tunnel clients only; local clients are the person at the Mac or their own network.
    var auditAddress: String? { isTunnel ? remoteAddress : nil }

    /// Marks a message send to `sessionId` as running; false when one already is (answered with `busy`).
    func beginSend(to sessionId: String) -> Bool {
        lock.lock(); defer { lock.unlock() }
        return _inflightSends.insert(sessionId).inserted
    }

    func endSend(to sessionId: String) {
        lock.lock(); _inflightSends.remove(sessionId); lock.unlock()
    }

    func close() {
        lock.lock()
        if _closing { lock.unlock(); return }
        _closing = true
        lock.unlock()
        io.close()
    }
}

import CryptoKit
import Foundation

/// The two bridge ports. Fixed: a busy port is reported, never replaced by another.
public enum BridgePorts {
    /// TLS with the pinned certificate, for the LAN and Tailscale.
    public static let lan: UInt16 = 47821
    /// Plain HTTP on 127.0.0.1 only, for cloudflared.
    public static let tunnel: UInt16 = 47822
}

/// A parsed HTTP/1.1 request head.
public struct HTTPHead: Equatable, Sendable {
    public let method: String
    public let target: String
    public let headers: [Header]

    public struct Header: Equatable, Sendable {
        public let name: String
        public let value: String
    }

    public var path: String { String(target.split(separator: "?", maxSplits: 1, omittingEmptySubsequences: false).first ?? "") }

    public func value(_ name: String) -> String? {
        headers.first { $0.name.lowercased() == name.lowercased() }?.value
    }

    /// Comma-separated tokens of every header called `name`, lowercased ("Connection: keep-alive, Upgrade").
    public func tokens(_ name: String) -> [String] {
        headers.filter { $0.name.lowercased() == name.lowercased() }
            .flatMap { $0.value.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces).lowercased() } }
            .filter { !$0.isEmpty }
    }

    public var pairs: [(name: String, value: String)] { headers.map { ($0.name, $0.value) } }
}

public enum HeadParse: Equatable, Sendable {
    case needMore
    case complete(HTTPHead, leftover: Data)
    case failure(status: Int, reason: String)
}

/// Collects bytes until the blank line that ends an HTTP head. Bounded in size and time, so a client that
/// sends too much, or too slowly, is answered and dropped rather than left hanging.
public struct HTTPHeadReader: Sendable {
    public static let maxHeadBytes = 16 * 1024
    public static let timeout: TimeInterval = 5

    private let startedAt: Date
    private var buffer = Data()

    public init(now: Date = Date()) { startedAt = now }

    public func timedOut(now: Date = Date()) -> Bool { now.timeIntervalSince(startedAt) >= Self.timeout }

    public mutating func feed(_ data: Data) -> HeadParse {
        buffer.append(data)
        let terminator = Data("\r\n\r\n".utf8)
        guard let end = buffer.range(of: terminator) else {
            return buffer.count > Self.maxHeadBytes ? .failure(status: 400, reason: "request head too large") : .needMore
        }
        if end.upperBound > Self.maxHeadBytes { return .failure(status: 400, reason: "request head too large") }
        let leftover = Data(buffer[end.upperBound...])
        guard let text = String(data: buffer[..<end.lowerBound], encoding: .utf8) else {
            return .failure(status: 400, reason: "request head is not text")
        }
        let lines = text.components(separatedBy: "\r\n")
        let request = lines[0].split(separator: " ", omittingEmptySubsequences: false)
        guard request.count == 3, !request[0].isEmpty, request[1].hasPrefix("/"), request[2] == "HTTP/1.1" else {
            return .failure(status: 400, reason: "bad request line")
        }
        var headers: [HTTPHead.Header] = []
        for line in lines.dropFirst() {
            guard let colon = line.firstIndex(of: ":"), colon != line.startIndex else { return .failure(status: 400, reason: "bad header line") }
            let name = String(line[..<colon])
            if name.contains(where: { $0 == " " || $0 == "\t" }) { return .failure(status: 400, reason: "bad header name") }
            headers.append(.init(name: name, value: line[line.index(after: colon)...].trimmingCharacters(in: .whitespaces)))
        }
        return .complete(HTTPHead(method: String(request[0]), target: String(request[1]), headers: headers), leftover: leftover)
    }
}

public enum HTTPResponse {
    public static func build(status: Int, body: String, headers: [(String, String)] = []) -> Data {
        let reasons = [101: "Switching Protocols", 400: "Bad Request", 401: "Unauthorized", 403: "Forbidden", 404: "Not Found",
                       405: "Method Not Allowed", 408: "Request Timeout", 426: "Upgrade Required", 429: "Too Many Requests"]
        var text = "HTTP/1.1 \(status) \(reasons[status] ?? "Error")\r\n"
        for (name, value) in headers { text += "\(name): \(value)\r\n" }
        text += "Content-Type: text/plain; charset=utf-8\r\nContent-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n\(body)"
        return Data(text.utf8)
    }
}

public enum WebSocketAccept {
    static let guid = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"

    public static func value(forKey key: String) -> String {
        Data(Insecure.SHA1.hash(data: Data((key + guid).utf8))).base64EncodedString()
    }

    /// A Sec-WebSocket-Key is 16 random bytes in base64.
    public static func isValidKey(_ key: String) -> Bool { Data(base64Encoded: key)?.count == 16 }
}

/// What to do with the upgrade request that reached the tunnel-origin port.
public enum TunnelUpgradeOutcome: Equatable, Sendable {
    case accept(response: Data, clientAddress: String)
    case refuse(response: Data, status: Int)
}

public enum TunnelUpgrade {
    public static let path = "/v1"

    public static func evaluate(_ head: HTTPHead, gate: TunnelGate) -> TunnelUpgradeOutcome {
        func refuse(_ status: Int, _ body: String, _ headers: [(String, String)] = []) -> TunnelUpgradeOutcome {
            .refuse(response: HTTPResponse.build(status: status, body: body + "\n", headers: headers), status: status)
        }
        // This port exists for cloudflared; anything without Cloudflare's headers is somebody else.
        guard TunnelGate.isTunnelOrigin(head.pairs) else { return refuse(403, "this port is for the tunnel only") }
        guard head.method == "GET" else { return refuse(400, "GET only") }
        guard head.path == path else { return refuse(404, "not found") }
        guard head.tokens("Connection").contains("upgrade"), head.tokens("Upgrade").contains("websocket") else {
            return refuse(426, "websocket upgrade required", [("Upgrade", "websocket")])
        }
        guard head.value("Sec-WebSocket-Version") == "13" else {
            return refuse(426, "websocket version 13 required", [("Sec-WebSocket-Version", "13")])
        }
        guard let key = head.value("Sec-WebSocket-Key"), WebSocketAccept.isValidKey(key) else { return refuse(400, "bad Sec-WebSocket-Key") }
        guard head.tokens("Sec-WebSocket-Protocol").contains(where: { $0 == "agentconsole.v1" }) else {
            return refuse(400, "subprotocol agentconsole.v1 required")
        }
        switch gate.evaluateTunnel(headers: head.pairs) {
        case .failure(let refusal):
            ConsoleLog.logger.notice("bridge: tunnel upgrade refused (\(refusal.reason, privacy: .public))")
            return refuse(refusal.status, refusal.status == 429 ? "too many attempts" : "access refused")
        case .success(let client):
            let response = Data(("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n"
                + "Sec-WebSocket-Accept: \(WebSocketAccept.value(forKey: key))\r\nSec-WebSocket-Protocol: agentconsole.v1\r\n\r\n").utf8)
            return .accept(response: response, clientAddress: client)
        }
    }
}

// MARK: - RFC 6455 framing

public enum WSFrame {
    public static let text: UInt8 = 0x1, binary: UInt8 = 0x2, close: UInt8 = 0x8, ping: UInt8 = 0x9, pong: UInt8 = 0xA

    /// A final, unmasked frame (servers never mask).
    public static func encode(opcode: UInt8, payload: Data) -> Data {
        var frame = Data([0x80 | opcode])
        switch payload.count {
        case ..<126: frame.append(UInt8(payload.count))
        case ..<65536: frame.append(126); frame.append(contentsOf: withUnsafeBytes(of: UInt16(payload.count).bigEndian) { Array($0) })
        default: frame.append(127); frame.append(contentsOf: withUnsafeBytes(of: UInt64(payload.count).bigEndian) { Array($0) })
        }
        frame.append(payload)
        return frame
    }

    public static func closeFrame(code: UInt16) -> Data {
        encode(opcode: close, payload: Data(withUnsafeBytes(of: code.bigEndian) { Array($0) }))
    }
}

/// Turns the bytes a client sends into messages. Clients must mask; anything off the rules ends in `.failure`
/// with the close code to answer with, and the decoder takes nothing more.
public struct WSFrameDecoder: Sendable {
    public enum Event: Equatable, Sendable {
        case text(String)
        case binary(Data)
        case ping(Data)
        case pong(Data)
        case close(code: UInt16?)
        case failure(code: UInt16, reason: String)
    }

    public let maxMessageBytes: Int
    private var buffer: [UInt8] = []
    private var fragmentOpcode: UInt8?
    private var fragments = Data()
    private var failed = false

    public init(maxMessageBytes: Int = BridgeRouter.maxMessageBytes) { self.maxMessageBytes = maxMessageBytes }

    public mutating func feed(_ data: Data) -> [Event] {
        guard !failed else { return [] }
        buffer.append(contentsOf: data)
        var events: [Event] = []
        while let event = next() {
            events.append(event)
            if case .failure = event { failed = true; buffer = []; break }
        }
        return events
    }

    private mutating func next() -> Event? {
        while true {
            guard buffer.count >= 2 else { return nil }
            let first = buffer[0], second = buffer[1]
            let fin = first & 0x80 != 0, opcode = first & 0x0F
            if first & 0x70 != 0 { return .failure(code: 1002, reason: "reserved bits set") }
            guard second & 0x80 != 0 else { return .failure(code: 1002, reason: "client frame not masked") }

            var length = Int(second & 0x7F), offset = 2
            if length == 126 {
                guard buffer.count >= 4 else { return nil }
                length = Int(buffer[2]) << 8 | Int(buffer[3]); offset = 4
            } else if length == 127 {
                guard buffer.count >= 10 else { return nil }
                var wide: UInt64 = 0
                for index in 2..<10 { wide = wide << 8 | UInt64(buffer[index]) }
                if wide >> 63 != 0 { return .failure(code: 1002, reason: "bad length") }
                guard wide <= UInt64(maxMessageBytes) else { return .failure(code: 1009, reason: "message too big") }
                length = Int(wide); offset = 10
            }
            if length > maxMessageBytes { return .failure(code: 1009, reason: "message too big") }
            guard buffer.count >= offset + 4 + length else { return nil }

            let mask = Array(buffer[offset..<offset + 4])
            let start = offset + 4
            var payload = Data(count: length)
            for index in 0..<length { payload[index] = buffer[start + index] ^ mask[index % 4] }
            buffer.removeFirst(start + length)

            if opcode >= 0x8 {
                guard fin, length <= 125 else { return .failure(code: 1002, reason: "bad control frame") }
                switch opcode {
                case 0x8:
                    if length == 1 { return .failure(code: 1002, reason: "bad close payload") }
                    return .close(code: length >= 2 ? UInt16(payload[0]) << 8 | UInt16(payload[1]) : nil)
                case 0x9: return .ping(payload)
                case 0xA: return .pong(payload)
                default: return .failure(code: 1002, reason: "unknown control opcode")
                }
            }

            switch opcode {
            case 0x0:
                guard fragmentOpcode != nil else { return .failure(code: 1002, reason: "unexpected continuation") }
            case 0x1, 0x2:
                guard fragmentOpcode == nil else { return .failure(code: 1002, reason: "new message inside a fragmented one") }
                fragmentOpcode = opcode
                fragments = Data()
            default:
                return .failure(code: 1002, reason: "unknown opcode")
            }
            if fragments.count + length > maxMessageBytes { return .failure(code: 1009, reason: "message too big") }
            fragments.append(payload)
            guard fin else { continue }
            let kind = fragmentOpcode
            fragmentOpcode = nil
            let message = fragments
            fragments = Data()
            if kind == 0x1 {
                guard let text = String(data: message, encoding: .utf8) else { return .failure(code: 1007, reason: "text is not UTF-8") }
                return .text(text)
            }
            return .binary(message)
        }
    }
}

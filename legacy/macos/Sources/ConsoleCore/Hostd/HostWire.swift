import Foundation

/// The wire format between Agent Console and the session host, over a Unix socket.
///
/// A frame is `[length: UInt32 big-endian][type: UInt8][payload]`, where length counts the type byte and the
/// payload. Type 1 is a JSON message. Type 2 is binary terminal data: a 16-byte session id followed by the bytes
/// (output going to the app, or input going to the pty).
///
/// JSON requests: {"id":Int,"op":String, ...}. Replies: {"id":Int,"ok":true,"data":{...}} or
/// {"id":Int,"ok":false,"error":String}. Events: {"event":"exit","sessionId":String,"status":Int}.
public enum HostFrame: Equatable, Sendable {
    case json([String: HostJSON])
    case data(sessionId: UUID, bytes: Data)

    public static let maxPayload = 16 * 1024 * 1024
}

/// A JSON value that is Equatable and Sendable (JSONSerialization's `Any` is neither).
public enum HostJSON: Equatable, Sendable {
    case string(String)
    case int(Int)
    case bool(Bool)
    case array([HostJSON])
    case object([String: HostJSON])
    case null

    init(any value: Any) {
        switch value {
        case let v as String: self = .string(v)
        case let v as NSNumber:
            if CFGetTypeID(v) == CFBooleanGetTypeID() { self = .bool(v.boolValue) } else { self = .int(v.intValue) }
        case let v as [Any]: self = .array(v.map(HostJSON.init(any:)))
        case let v as [String: Any]: self = .object(v.mapValues(HostJSON.init(any:)))
        default: self = .null
        }
    }

    var any: Any {
        switch self {
        case .string(let v): return v
        case .int(let v): return v
        case .bool(let v): return v
        case .array(let v): return v.map(\.any)
        case .object(let v): return v.mapValues(\.any)
        case .null: return NSNull()
        }
    }

    public var string: String? { if case .string(let v) = self { return v } else { return nil } }
    public var int: Int? { if case .int(let v) = self { return v } else { return nil } }
    public var bool: Bool? { if case .bool(let v) = self { return v } else { return nil } }
    public var array: [HostJSON]? { if case .array(let v) = self { return v } else { return nil } }
    public var object: [String: HostJSON]? { if case .object(let v) = self { return v } else { return nil } }
}

public enum HostWire {
    public static func encode(_ frame: HostFrame) -> Data {
        var payload = Data()
        switch frame {
        case .json(let object):
            payload.append(1)
            let any = object.mapValues(\.any)
            payload.append((try? JSONSerialization.data(withJSONObject: any, options: [.sortedKeys])) ?? Data("{}".utf8))
        case .data(let id, let bytes):
            payload.append(2)
            payload.append(contentsOf: withUnsafeBytes(of: id.uuid) { Array($0) })
            payload.append(bytes)
        }
        var out = Data()
        out.append(contentsOf: withUnsafeBytes(of: UInt32(payload.count).bigEndian) { Array($0) })
        out.append(payload)
        return out
    }

    public static func json(_ pairs: [String: HostJSON]) -> Data { encode(.json(pairs)) }
}

/// Collects bytes from a socket and yields whole frames. A frame that is too big or malformed ends the stream.
public struct HostFrameReader: Sendable {
    public enum Failure: Error, Equatable { case tooLarge, malformed }

    private var buffer = Data()
    public private(set) var failure: Failure?

    public init() {}

    public mutating func feed(_ data: Data) -> [HostFrame] {
        guard failure == nil else { return [] }
        buffer.append(data)
        var frames: [HostFrame] = []
        while buffer.count >= 5 {
            let length = Int(buffer[buffer.startIndex]) << 24 | Int(buffer[buffer.startIndex + 1]) << 16 | Int(buffer[buffer.startIndex + 2]) << 8 | Int(buffer[buffer.startIndex + 3])
            if length < 1 || length > HostFrame.maxPayload { failure = length < 1 ? .malformed : .tooLarge; buffer = Data(); break }
            guard buffer.count >= 4 + length else { break }
            let type = buffer[buffer.startIndex + 4]
            let payload = Data(buffer[(buffer.startIndex + 5)..<(buffer.startIndex + 4 + length)])
            buffer.removeFirst(4 + length)
            switch type {
            case 1:
                guard let object = (try? JSONSerialization.jsonObject(with: payload)) as? [String: Any] else { failure = .malformed; return frames }
                frames.append(.json(object.mapValues(HostJSON.init(any:))))
            case 2:
                guard payload.count >= 16 else { failure = .malformed; return frames }
                let uuid = UUID(uuid: payload.prefix(16).withUnsafeBytes { $0.load(as: uuid_t.self) })
                frames.append(.data(sessionId: uuid, bytes: Data(payload.dropFirst(16))))
            default:
                failure = .malformed
                return frames
            }
        }
        return frames
    }
}

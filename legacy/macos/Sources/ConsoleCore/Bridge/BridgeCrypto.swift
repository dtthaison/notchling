import CryptoKit
import Foundation
import Security

public enum BridgeCrypto {
    public static func sha256(_ data: Data) -> [UInt8] { Array(SHA256.hash(data: data)) }

    public static func hex(_ bytes: [UInt8]) -> String {
        bytes.map { String(format: "%02x", $0) }.joined()
    }

    public static func sha256Hex(_ data: Data) -> String { hex(sha256(data)) }

    public static func sha256Hex(_ text: String) -> String { sha256Hex(Data(text.utf8)) }

    /// Compares without stopping at the first difference, so timing does not leak how much matched.
    public static func constantTimeEqual(_ lhs: [UInt8], _ rhs: [UInt8]) -> Bool {
        guard lhs.count == rhs.count else { return false }
        var difference: UInt8 = 0
        for index in lhs.indices { difference |= lhs[index] ^ rhs[index] }
        return difference == 0
    }

    public static func constantTimeEqual(_ lhs: String, _ rhs: String) -> Bool {
        constantTimeEqual(Array(lhs.utf8), Array(rhs.utf8))
    }

    public static func randomBytes(_ count: Int) -> [UInt8] {
        var bytes = [UInt8](repeating: 0, count: count)
        let status = SecRandomCopyBytes(kSecRandomDefault, count, &bytes)
        precondition(status == errSecSuccess, "SecRandomCopyBytes failed: \(status)")
        return bytes
    }

    /// 32 random bytes as base64url without padding.
    public static func randomToken(bytes: Int = 32) -> String {
        Data(randomBytes(bytes)).base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }
}

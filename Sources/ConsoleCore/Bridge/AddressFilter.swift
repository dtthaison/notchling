import Foundation

/// Who may even open a connection: this Mac, the private LAN, link-local and Tailscale. Everything else
/// is closed on sight.
public enum AddressFilter {
    public static func isAllowed(_ address: String) -> Bool {
        var text = address
        if let zone = text.firstIndex(of: "%") { text = String(text[..<zone]) }
        if text.hasPrefix("[") && text.hasSuffix("]") { text = String(text.dropFirst().dropLast()) }

        var v4 = in_addr()
        if inet_pton(AF_INET, text, &v4) == 1 { return isAllowedV4(UInt32(bigEndian: v4.s_addr)) }

        var v6 = in6_addr()
        guard inet_pton(AF_INET6, text, &v6) == 1 else { return false }
        let bytes = withUnsafeBytes(of: &v6) { Array($0) }
        return isAllowedV6(bytes)
    }

    private static func isAllowedV4(_ ip: UInt32) -> Bool {
        let a = ip >> 24, b = (ip >> 16) & 0xFF
        if a == 127 { return true }                              // loopback
        if a == 10 { return true }                               // 10/8
        if a == 172 && (16...31).contains(b) { return true }     // 172.16/12
        if a == 192 && b == 168 { return true }                  // 192.168/16
        if a == 169 && b == 254 { return true }                  // link-local
        if a == 100 && (64...127).contains(b) { return true }    // Tailscale 100.64/10
        return false
    }

    private static func isAllowedV6(_ bytes: [UInt8]) -> Bool {
        guard bytes.count == 16 else { return false }
        if bytes[0..<15].allSatisfy({ $0 == 0 }) && bytes[15] == 1 { return true }   // ::1
        // ::ffff:a.b.c.d is an IPv4 client seen through a dual-stack socket.
        if bytes[0..<10].allSatisfy({ $0 == 0 }) && bytes[10] == 0xFF && bytes[11] == 0xFF {
            let ip = UInt32(bytes[12]) << 24 | UInt32(bytes[13]) << 16 | UInt32(bytes[14]) << 8 | UInt32(bytes[15])
            return isAllowedV4(ip)
        }
        if bytes[0] & 0xFE == 0xFC { return true }                                    // fc00::/7
        if bytes[0] == 0xFE && bytes[1] & 0xC0 == 0x80 { return true }                // fe80::/10
        let tailscale: [UInt8] = [0xFD, 0x7A, 0x11, 0x5C, 0xA1, 0xE0]                 // fd7a:115c:a1e0::/48
        if Array(bytes[0..<6]) == tailscale { return true }
        return false
    }

    public static func isTailscaleV4(_ address: String) -> Bool {
        var v4 = in_addr()
        guard inet_pton(AF_INET, address, &v4) == 1 else { return false }
        let ip = UInt32(bigEndian: v4.s_addr)
        return ip >> 24 == 100 && (64...127).contains((ip >> 16) & 0xFF)
    }

    public static func isPrivateLANV4(_ address: String) -> Bool {
        var v4 = in_addr()
        guard inet_pton(AF_INET, address, &v4) == 1 else { return false }
        let ip = UInt32(bigEndian: v4.s_addr)
        let a = ip >> 24, b = (ip >> 16) & 0xFF
        return a == 10 || (a == 172 && (16...31).contains(b)) || (a == 192 && b == 168)
    }
}

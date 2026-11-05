import Foundation

public struct LocalAddresses: Equatable, Sendable {
    public var lan: [String]
    public var tailscale: String?

    /// Private IPv4 addresses of up interfaces, and the Tailscale one (100.64/10 on a utun).
    public static func current() -> LocalAddresses {
        var result = LocalAddresses(lan: [], tailscale: nil)
        var list: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0, let first = list else { return result }
        defer { freeifaddrs(list) }
        var pointer: UnsafeMutablePointer<ifaddrs>? = first
        while let entry = pointer {
            defer { pointer = entry.pointee.ifa_next }
            let flags = Int32(entry.pointee.ifa_flags)
            guard flags & IFF_UP != 0, flags & IFF_LOOPBACK == 0,
                  let address = entry.pointee.ifa_addr, address.pointee.sa_family == UInt8(AF_INET) else { continue }
            var host = [CChar](repeating: 0, count: Int(NI_MAXHOST))
            guard getnameinfo(address, socklen_t(address.pointee.sa_len), &host, socklen_t(host.count), nil, 0, NI_NUMERICHOST) == 0 else { continue }
            let text = String(cString: host)
            let name = String(cString: entry.pointee.ifa_name)
            if AddressFilter.isTailscaleV4(text) {
                if name.hasPrefix("utun"), result.tailscale == nil { result.tailscale = text }
            } else if AddressFilter.isPrivateLANV4(text), !result.lan.contains(text) {
                result.lan.append(text)
            }
        }
        return result
    }
}
